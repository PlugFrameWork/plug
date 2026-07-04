use wasmer::{Instance, Module, Store, Function, imports, Memory, FunctionEnv, FunctionEnvMut};
use wasmer_compiler_cranelift::Cranelift;
use std::path::{Path, PathBuf};
use std::fs;
use once_cell::sync::Lazy;
use std::sync::{Mutex, Arc, mpsc};
use std::process::{Command, Stdio};
use std::io::{BufRead, BufReader};
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use serde::Deserialize;
use crate::ops::host::{
    print_info, print_error, 
    add_tab, set_tab_cwd, set_tab_owner, 
    get_current_print_tab, set_prompt_visibility
};
use crate::ops::net::http_post_json;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;
const HOST_API_VERSION: &str = "0.1.0a";

#[derive(Deserialize, Clone, Debug)]
pub struct Manifest {
    pub name: String,
    pub version: String,
    pub author: String,
    pub api_version: String,
    pub permissions: Vec<String>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct PluginManifest {
    pub plugin: Vec<Manifest>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct SinglePluginManifest {
    pub plugin: Manifest,
}

pub struct Plugin {
    pub name: String,
    pub hash: String,
    pub manifest: Manifest,
    store: Store,
    instance: Instance,
}

struct Env {
    memory: Option<Memory>,
    permissions: Vec<String>,
}

static PLUGINS: Lazy<Mutex<Vec<Plugin>>> = Lazy::new(|| Mutex::new(Vec::new()));
static CURRENT_ARGS: Lazy<Mutex<String>> = Lazy::new(|| Mutex::new(String::new()));
static TAB_CWDS: Lazy<Mutex<std::collections::HashMap<i32, String>>> = Lazy::new(|| Mutex::new(std::collections::HashMap::new()));
static TAB_LABELS: Lazy<Mutex<std::collections::HashMap<i32, String>>> = Lazy::new(|| Mutex::new(std::collections::HashMap::new()));

type Job = Box<dyn FnOnce() + Send + 'static>;

struct SimpleThreadPool {
    sender: mpsc::Sender<Job>,
}

impl SimpleThreadPool {
    fn new(size: usize) -> Self {
        let (sender, receiver) = mpsc::channel::<Job>();
        let receiver = Arc::new(Mutex::new(receiver));

        for _ in 0..size {
            let receiver = Arc::clone(&receiver);
            std::thread::spawn(move || {
                loop {
                    let job = {
                        let Ok(lock) = receiver.lock() else { break };
                        match lock.recv() {
                            Ok(j) => j,
                            Err(_) => break,
                        }
                    };

                    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        job();
                    }));
                }
            });
        }
        Self { sender }
    }

    fn execute<F>(&self, f: F)
    where
        F: FnOnce() + Send + 'static,
    {
        let job = Box::new(f);
        let _ = self.sender.send(job);
    }
}

static POOL: Lazy<SimpleThreadPool> = Lazy::new(|| SimpleThreadPool::new(4));

pub fn init_plugins(plugins_dir: &str) {
    let path = Path::new(plugins_dir);
    
    {
        let mut plugins = PLUGINS.lock().unwrap();
        plugins.clear();
    }

    if !path.exists() {
        let _ = fs::create_dir_all(path);
        return;
    }

    let unified_manifest_path = path.join("plugin.toml");
    let mut manifest_map = std::collections::HashMap::new();
    if unified_manifest_path.exists() {
        if let Ok(content) = fs::read_to_string(&unified_manifest_path) {
            if let Ok(p_manifest) = toml::from_str::<PluginManifest>(&content) {
                for m in p_manifest.plugin {
                    manifest_map.insert(m.name.clone(), m);
                }
            } else {
                print_error(&format!("[SECURITY] Failed to parse unified manifest: {}", unified_manifest_path.display()));
            }
        }
    } else {
         print_error(&format!("[SECURITY] Missing unified manifest: {}", unified_manifest_path.display()));
    }

    let mut plug_files = Vec::new();

    fn walk_dir(dir: &Path, files: &mut Vec<PathBuf>) {
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_dir() {
                    walk_dir(&p, files);
                } else if p.extension().map_or(false, |ext| ext == "hash") {
                    files.push(p);
                }
            }
        }
    }
    walk_dir(path, &mut plug_files);

    for p in plug_files {
        let stem_opt = p.file_stem().and_then(|s| s.to_str());
        if let Some(stem) = stem_opt {
            if let Some(manifest) = manifest_map.get(stem) {
                let hash_val = fs::read_to_string(&p).unwrap_or_default().trim().to_string();
                let wasm_path = p.with_extension(&hash_val);
                if let Err(e) = load_plugin(&wasm_path, manifest, &hash_val) {
                     print_error(&format!("[WASM] Failed to load {}: {}", wasm_path.display(), e));
                }
            } else {
                let individual_toml = p.with_extension("toml");
                if individual_toml.exists() {
                    if let Ok(content) = fs::read_to_string(&individual_toml) {
                        if let Ok(single) = toml::from_str::<SinglePluginManifest>(&content) {
                            let hash_val = fs::read_to_string(&p).unwrap_or_default().trim().to_string();
                            let wasm_path = p.with_extension(&hash_val);
                            if let Err(e) = load_plugin(&wasm_path, &single.plugin, &hash_val) {
                                 print_error(&format!("[WASM] Failed to load {}: {}", wasm_path.display(), e));
                            }
                        } else {
                            print_error(&format!("[SECURITY] Invalid manifest format: {}", individual_toml.display()));
                        }
                    }
                } else {
                    print_error(&format!("[SECURITY] Missing manifest entry for {}", p.display()));
                }
            }
        }
    }
}

fn load_plugin(wasm_path: &Path, manifest: &Manifest, hash: &str) -> Result<(), Box<dyn std::error::Error>> {
    let wasm_bytes = fs::read(wasm_path)?;

    if !manifest.api_version.starts_with("0.1") {
        return Err(format!("Incompatible API version: {} (Host: {})", manifest.api_version, HOST_API_VERSION).into());
    }

    let mut store = Store::new(Cranelift::default());
    let module = Module::new(&store, wasm_bytes)?;

    let env = FunctionEnv::new(&mut store, Env { 
        memory: None, 
        permissions: manifest.permissions.clone() 
    });

    let import_object = imports! {
        "env" => {
            "print_info" => Function::new_typed_with_env(&mut store, &env, |mut env: FunctionEnvMut<Env>, ptr: i32, len: i32| {
                let (env_data, store) = env.data_and_store_mut();
                if let Some(memory) = &env_data.memory {
                    let view = memory.view(&store);
                    let mut buffer = vec![0u8; len as usize];
                    if view.read(ptr as u64, &mut buffer).is_ok() {
                        if let Ok(s) = std::str::from_utf8(&buffer) {
                             print_info(&format!("[PLUGIN] {}", s));
                        }
                    }
                }
            }),
            "print_error" => Function::new_typed_with_env(&mut store, &env, |mut env: FunctionEnvMut<Env>, ptr: i32, len: i32| {
                let (env_data, store) = env.data_and_store_mut();
                if let Some(memory) = &env_data.memory {
                    let view = memory.view(&store);
                    let mut buffer = vec![0u8; len as usize];
                    if view.read(ptr as u64, &mut buffer).is_ok() {
                        if let Ok(s) = std::str::from_utf8(&buffer) {
                             print_error(&format!("[PLUGIN ERROR] {}", s));
                        }
                    }
                }
            }),

            "get_args" => Function::new_typed_with_env(&mut store, &env, |mut env: FunctionEnvMut<Env>, ptr: i32, max_len: i32| {
                let (env_data, store) = env.data_and_store_mut();
                if max_len <= 0 { return; }
                if let Some(memory) = &env_data.memory {
                    let view = memory.view(&store);
                    let args = CURRENT_ARGS.lock().unwrap();
                    let bytes = args.as_bytes();
                    let cap = (max_len as usize).saturating_sub(1);
                    let n = std::cmp::min(bytes.len(), cap);
                    if view.write(ptr as u64, &bytes[..n]).is_ok() {
                        let _ = view.write((ptr as u64) + n as u64, &[0u8]);
                    }
                }
            }),

            "host_exec" => Function::new_typed_with_env(&mut store, &env, |mut env: FunctionEnvMut<Env>, ptr: i32, len: i32| {
                let (env_data, store) = env.data_and_store_mut();
                if !env_data.permissions.iter().any(|p| p == "host_exec") {
                    print_error("[SECURITY] Plugin attempted to call host_exec without permission");
                    return;
                }
                if let Some(memory) = &env_data.memory {
                    let view = memory.view(&store);
                    if len <= 0 || len > 65536 { return; }
                    let mut buf = vec![0u8; len as usize];
                    if view.read(ptr as u64, &mut buf).is_ok() {
                        let full_cmd_line = std::str::from_utf8(&buf).unwrap_or("").trim();
                        if full_cmd_line.is_empty() { return; }
                        let tab_idx = get_current_print_tab();
                        
                        if full_cmd_line.to_lowercase().starts_with("cd ") || full_cmd_line.to_lowercase() == "cd" {
                            let mut target_dir = full_cmd_line[2..].trim().to_string();
                            if target_dir.is_empty() { return; }
                            if target_dir.starts_with('"') && target_dir.ends_with('"') {
                                target_dir = target_dir[1..target_dir.len()-1].to_string();
                            }
                            let base_dir = {
                                let cwds = TAB_CWDS.lock().unwrap();
                                cwds.get(&tab_idx).cloned().unwrap_or_else(|| {
                                    std::env::current_dir().unwrap_or_default().to_string_lossy().to_string()
                                })
                            };
                            let new_path = Path::new(&base_dir).join(target_dir);
                            if let Ok(abs_path) = fs::canonicalize(new_path) {
                                let path_str = abs_path.to_string_lossy().replace("\\\\?\\", "").to_string();
                                {
                                    let mut cwds = TAB_CWDS.lock().unwrap();
                                    cwds.insert(tab_idx, path_str.clone());
                                }
                                set_tab_cwd(tab_idx, &path_str);
                            } else {
                                print_error("The system cannot find the path specified.");
                            }
                            return;
                        }

                        let mut parsed_args = parse_args(full_cmd_line);
                        if parsed_args.is_empty() { return; }
                        let exe = parsed_args.remove(0);
                        let sub_cwd = {
                            let cwds = TAB_CWDS.lock().unwrap();
                            cwds.get(&tab_idx).cloned().unwrap_or_else(|| {
                                std::env::current_dir().unwrap_or_default().to_string_lossy().to_string()
                            })
                        };
                        let mut cmd = Command::new(&exe);
                        cmd.args(&parsed_args)
                           .current_dir(sub_cwd)
                           .stdout(Stdio::piped())
                           .stderr(Stdio::piped());
                        
                        #[cfg(windows)]
                        cmd.creation_flags(CREATE_NO_WINDOW);
                        
                        match cmd.spawn() {
                            Ok(mut child) => {
                                if let Some(stdout) = child.stdout.take() {
                                    let reader = BufReader::new(stdout);
                                    for line in reader.lines().flatten() { print_info(&line); }
                                }
                                if let Some(stderr) = child.stderr.take() {
                                    let reader = BufReader::new(stderr);
                                    for line in reader.lines().flatten() { print_error(&format!("[STDERR] {}", line)); }
                                }
                                let _ = child.wait();
                            }
                            Err(e) => {
                                if e.kind() == std::io::ErrorKind::NotFound { print_error("Unknown command."); }
                                else { print_error(&format!("[EXEC FAILED] '{}': {}", exe, e)); }
                            }
                        }
                    }
                }
            }),
            "get_env" => Function::new_typed_with_env(&mut store, &env, |mut env: FunctionEnvMut<Env>, name_ptr: i32, name_len: i32, resp_ptr: i32, resp_max_len: i32| {
                let (env_data, store) = env.data_and_store_mut();
                if let Some(memory) = &env_data.memory {
                    let view = memory.view(&store);
                    let mut name_buf = vec![0u8; name_len as usize];
                    if view.read(name_ptr as u64, &mut name_buf).is_ok() {
                        let name = std::str::from_utf8(&name_buf).unwrap_or("");
                        let val = std::env::var(name).unwrap_or_default();
                        let val_bytes = val.as_bytes();
                        let len = std::cmp::min(val_bytes.len(), resp_max_len as usize);
                        let _ = view.write(resp_ptr as u64, &val_bytes[..len]);
                    }
                }
            }),
            "main_w_add_tab" => Function::new_typed_with_env(&mut store, &env, |mut env: FunctionEnvMut<Env>, ptr: i32, len: i32| {
                let (env_data, store) = env.data_and_store_mut();
                if let Some(memory) = &env_data.memory {
                    let view = memory.view(&store);
                    let mut buffer = vec![0u8; len as usize];
                    if view.read(ptr as u64, &mut buffer).is_ok() {
                        if let Ok(name) = std::str::from_utf8(&buffer) {
                             add_tab(name);
                             let tab_idx = get_current_print_tab();
                             let mut labels = TAB_LABELS.lock().unwrap();
                             labels.insert(tab_idx, name.to_string());
                             return;
                        }
                    }
                }
                add_tab("");
            }),
            "host_set_tab_owner" => Function::new_typed_with_env(&mut store, &env, |mut env: FunctionEnvMut<Env>, ptr: i32, len: i32| {
                let (env_data, store) = env.data_and_store_mut();
                if let Some(memory) = &env_data.memory {
                    let view = memory.view(&store);
                    let mut buffer = vec![0u8; len as usize];
                    if view.read(ptr as u64, &mut buffer).is_ok() {
                        if let Ok(name) = std::str::from_utf8(&buffer) {
                             let tab_idx = get_current_print_tab();
                             set_tab_owner(tab_idx, name);
                        }
                    }
                }
            }),
            "host_get_tab_label" => Function::new_typed_with_env(&mut store, &env, |mut env: FunctionEnvMut<Env>, ptr: i32, max_len: i32| {
                let (env_data, store) = env.data_and_store_mut();
                if let Some(memory) = &env_data.memory {
                    let view = memory.view(&store);
                    let tab_idx = get_current_print_tab();
                    let labels = TAB_LABELS.lock().unwrap();
                    if let Some(label) = labels.get(&tab_idx) {
                        let bytes = label.as_bytes();
                        let n = std::cmp::min(bytes.len(), max_len as usize);
                        let _ = view.write(ptr as u64, &bytes[..n]);
                        return n as i32;
                    }
                }
                0
            }),
            "net_post" => Function::new_typed_with_env(&mut store, &env, |mut env: FunctionEnvMut<Env>, url_ptr: i32, url_len: i32, body_ptr: i32, body_len: i32, resp_ptr: i32, resp_max_len: i32| {
                let (env_data, store) = env.data_and_store_mut();
                if let Some(memory) = &env_data.memory {
                    let view = memory.view(&store);
                    let mut url_buf = vec![0u8; url_len as usize];
                    let mut body_buf = vec![0u8; body_len as usize];
                    if view.read(url_ptr as u64, &mut url_buf).is_ok() && view.read(body_ptr as u64, &mut body_buf).is_ok() {
                        let url = std::str::from_utf8(&url_buf).unwrap_or("");
                        let body = std::str::from_utf8(&body_buf).unwrap_or("");
                        match http_post_json(url, body) {
                            Ok(resp) => {
                                let resp_bytes = resp.as_bytes();
                                let len = std::cmp::min(resp_bytes.len(), resp_max_len as usize);
                                let _ = view.write(resp_ptr as u64, &resp_bytes[..len]);
                            }
                            Err(e) => {
                                print_error(&format!("[NET ERROR] {}", e));
                                let _ = view.write(resp_ptr as u64, b"{}");
                            }
                        }
                    }
                }
            }),
        },
        "wasi_snapshot_preview1" => {
            "args_sizes_get" => Function::new_typed_with_env(&mut store, &env, |mut env: FunctionEnvMut<Env>, argc_ptr: i32, argv_buf_size_ptr: i32| -> i32 {
                let (env_data, store) = env.data_and_store_mut();
                if let Some(memory) = &env_data.memory {
                    let view = memory.view(&store);
                    let args = CURRENT_ARGS.lock().unwrap();
                    let parsed = parse_args(&args);
                    let mut argc = 1;
                    let mut size = "plugin\0".len();
                    for arg in parsed.iter() { argc += 1; size += arg.len() + 1; }
                    let _ = view.write(argc_ptr as u64, &(argc as u32).to_le_bytes());
                    let _ = view.write(argv_buf_size_ptr as u64, &(size as u32).to_le_bytes());
                }
                0
            }),
            "args_get" => Function::new_typed_with_env(&mut store, &env, |mut env: FunctionEnvMut<Env>, mut argv_ptr: i32, mut argv_buf_ptr: i32| -> i32 {
                let (env_data, store) = env.data_and_store_mut();
                if let Some(memory) = &env_data.memory {
                    let view = memory.view(&store);
                    let args = CURRENT_ARGS.lock().unwrap();
                    let parsed = parse_args(&args);
                    let mut write_arg = |arg: &str| {
                        let _ = view.write(argv_ptr as u64, &(argv_buf_ptr as u32).to_le_bytes());
                        argv_ptr += 4;
                        let bytes = arg.as_bytes();
                        let _ = view.write(argv_buf_ptr as u64, bytes);
                        argv_buf_ptr += bytes.len() as i32;
                        let _ = view.write(argv_buf_ptr as u64, &[0u8]);
                        argv_buf_ptr += 1;
                    };
                    write_arg("plugin");
                    for arg in parsed.iter() { write_arg(arg); }
                }
                0
            }),
            "environ_sizes_get" => Function::new_typed_with_env(&mut store, &env, |mut env: FunctionEnvMut<Env>, count: i32, buf_size: i32| -> i32 {
                let (env_data, store) = env.data_and_store_mut();
                if let Some(memory) = &env_data.memory {
                    let view = memory.view(&store);
                    let _ = view.write(count as u64, &0u32.to_le_bytes());
                    let _ = view.write(buf_size as u64, &0u32.to_le_bytes());
                }
                0
            }),
            "environ_get" => Function::new_typed(&mut store, |_: i32, _: i32| -> i32 { 0 }),
            "proc_exit" => Function::new_typed(&mut store, |_: i32| {}),
            "fd_write" => Function::new_typed_with_env(&mut store, &env, |mut env: FunctionEnvMut<Env>, fd: i32, iovs_ptr: i32, iovs_len: i32, nwritten: i32| -> i32 {
                let (env_data, store) = env.data_and_store_mut();
                if fd != 1 && fd != 2 { return 0; }
                if let Some(memory) = &env_data.memory {
                    let view = memory.view(&store);
                    let mut total_written = 0;
                    for i in 0..iovs_len {
                        let mut iov_buf = [0u8; 8];
                        if view.read((iovs_ptr + i * 8) as u64, &mut iov_buf).is_ok() {
                            let ptr = u32::from_le_bytes(iov_buf[0..4].try_into().unwrap()) as u64;
                            let len = u32::from_le_bytes(iov_buf[4..8].try_into().unwrap()) as usize;
                            let mut str_buf = vec![0u8; len];
                            if view.read(ptr, &mut str_buf).is_ok() {
                                let s = String::from_utf8_lossy(&str_buf);
                                let cleaned = s.trim_end_matches(|c| c == '\n' || c == '\r');
                                if !cleaned.is_empty() {
                                    if fd == 1 { print_info(cleaned); } else { print_error(cleaned); }
                                }
                                total_written += len as i32;
                            }
                        }
                    }
                    let _ = view.write(nwritten as u64, &total_written.to_le_bytes());
                }
                0
            }),
            "fd_read" => Function::new_typed(&mut store, |_: i32, _: i32, _: i32, _: i32| -> i32 { 0 }),
            "fd_close" => Function::new_typed(&mut store, |_: i32| -> i32 { 0 }),
            "fd_seek" => Function::new_typed(&mut store, |_: i32, _: i64, _: i32, _: i32| -> i32 { 0 }),
            "fd_fdstat_get" => Function::new_typed(&mut store, |_: i32, _: i32| -> i32 { 0 }),
            "fd_fdstat_set_flags" => Function::new_typed(&mut store, |_: i32, _: i32| -> i32 { 0 }),
            "fd_fdstat_set_rights" => Function::new_typed(&mut store, |_: i32, _: i64, _: i64| -> i32 { 0 }),
            "fd_prestat_get" => Function::new_typed(&mut store, |_: i32, _: i32| -> i32 { 8 }),
            "fd_prestat_dir_name" => Function::new_typed(&mut store, |_: i32, _: i32, _: i32| -> i32 { 8 }),
            "fd_advise" => Function::new_typed(&mut store, |_: i32, _: i64, _: i64, _: i32| -> i32 { 0 }),
            "fd_allocate" => Function::new_typed(&mut store, |_: i32, _: i64, _: i64| -> i32 { 0 }),
            "fd_datasync" => Function::new_typed(&mut store, |_: i32| -> i32 { 0 }),
            "fd_filestat_get" => Function::new_typed(&mut store, |_: i32, _: i32| -> i32 { 0 }),
            "fd_filestat_set_size" => Function::new_typed(&mut store, |_: i32, _: i64| -> i32 { 0 }),
            "fd_filestat_set_times" => Function::new_typed(&mut store, |_: i32, _: i64, _: i64, _: i32| -> i32 { 0 }),
            "fd_pread" => Function::new_typed(&mut store, |_: i32, _: i32, _: i32, _: i64, _: i32| -> i32 { 0 }),
            "fd_pwrite" => Function::new_typed(&mut store, |_: i32, _: i32, _: i32, _: i64, _: i32| -> i32 { 0 }),
            "fd_readdir" => Function::new_typed(&mut store, |_: i32, _: i32, _: i32, _: i64, _: i32| -> i32 { 0 }),
            "fd_renumber" => Function::new_typed(&mut store, |_: i32, _: i32| -> i32 { 0 }),
            "fd_sync" => Function::new_typed(&mut store, |_: i32| -> i32 { 0 }),
            "fd_tell" => Function::new_typed(&mut store, |_: i32, _: i32| -> i32 { 0 }),
            "path_create_directory" => Function::new_typed(&mut store, |_: i32, _: i32, _: i32| -> i32 { 0 }),
            "path_filestat_get" => Function::new_typed(&mut store, |_: i32, _: i32, _: i32, _: i32, _: i32| -> i32 { 0 }),
            "path_filestat_set_times" => Function::new_typed(&mut store, |_: i32, _: i32, _: i32, _: i32, _: i64, _: i64, _: i32| -> i32 { 0 }),
            "path_link" => Function::new_typed(&mut store, |_: i32, _: i32, _: i32, _: i32, _: i32, _: i32, _: i32| -> i32 { 0 }),
            "path_open" => Function::new_typed(&mut store, |_: i32, _: i32, _: i32, _: i32, _: i32, _: i64, _: i64, _: i32, _: i32| -> i32 { 0 }),
            "path_readlink" => Function::new_typed(&mut store, |_: i32, _: i32, _: i32, _: i32, _: i32, _: i32| -> i32 { 0 }),
            "path_remove_directory" => Function::new_typed(&mut store, |_: i32, _: i32, _: i32| -> i32 { 0 }),
            "path_rename" => Function::new_typed(&mut store, |_: i32, _: i32, _: i32, _: i32, _: i32, _: i32| -> i32 { 0 }),
            "path_symlink" => Function::new_typed(&mut store, |_: i32, _: i32, _: i32, _: i32, _: i32| -> i32 { 0 }),
            "path_unlink_file" => Function::new_typed(&mut store, |_: i32, _: i32, _: i32| -> i32 { 0 }),
            "poll_oneoff" => Function::new_typed(&mut store, |_: i32, _: i32, _: i32, _: i32| -> i32 { 0 }),
            "proc_raise" => Function::new_typed(&mut store, |_: i32| -> i32 { 0 }),
            "sock_accept" => Function::new_typed(&mut store, |_: i32, _: i32, _: i32| -> i32 { 0 }),
            "sock_bind" => Function::new_typed(&mut store, |_: i32, _: i32, _: i32| -> i32 { 0 }),
            "sock_listen" => Function::new_typed(&mut store, |_: i32, _: i32| -> i32 { 0 }),
            "sock_connect" => Function::new_typed(&mut store, |_: i32, _: i32, _: i32| -> i32 { 0 }),
            "sock_getpeername" => Function::new_typed(&mut store, |_: i32, _: i32, _: i32| -> i32 { 0 }),
            "sock_getsockname" => Function::new_typed(&mut store, |_: i32, _: i32, _: i32| -> i32 { 0 }),
            "sock_getsockopt" => Function::new_typed(&mut store, |_: i32, _: i32, _: i32, _: i32, _: i32| -> i32 { 0 }),
            "sock_setsockopt" => Function::new_typed(&mut store, |_: i32, _: i32, _: i32, _: i32, _: i32| -> i32 { 0 }),
            "sock_recv" => Function::new_typed(&mut store, |_: i32, _: i32, _: i32, _: i32, _: i32, _: i32| -> i32 { 0 }),
            "sock_send" => Function::new_typed(&mut store, |_: i32, _: i32, _: i32, _: i32, _: i32| -> i32 { 0 }),
            "sock_shutdown" => Function::new_typed(&mut store, |_: i32, _: i32| -> i32 { 0 }),
            "clock_res_get" => Function::new_typed(&mut store, |_: i32, _: i32| -> i32 { 0 }),
            "clock_time_get" => Function::new_typed(&mut store, |_: i32, _: i64, _: i32| -> i32 { 0 }),
            "random_get" => Function::new_typed(&mut store, |_: i32, _: i32| -> i32 { 0 }),
            "sched_yield" => Function::new_typed(&mut store, || -> i32 { 0 }),
        }
    };

    let instance = Instance::new(&mut store, &module, &import_object)?;
    let memory = instance.exports.get_memory("memory")?.clone();
    env.as_mut(&mut store).memory = Some(memory);

    if let Ok(ctors) = instance.exports.get_typed_function::<(), ()>(&store, "__wasm_call_ctors") {
        let _ = ctors.call(&mut store);
    }

    let mut plugins = PLUGINS.lock().unwrap();
    plugins.push(Plugin {
        name: manifest.name.clone(),
        hash: hash.to_string(),
        manifest: manifest.clone(),
        store,
        instance,
    });
    Ok(())
}

fn parse_args(s: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current_arg = String::new();
    let mut in_quotes = false;
    for c in s.trim().chars() {
        match c {
            '"' => in_quotes = !in_quotes,
            ' ' if !in_quotes => {
                if !current_arg.is_empty() { args.push(current_arg.clone()); current_arg.clear(); }
            }
            _ => current_arg.push(c),
        }
    }
    if !current_arg.is_empty() { args.push(current_arg); }
    args
}

pub fn get_loaded_plugins() -> Vec<(String, String)> {
    let plugins = PLUGINS.lock().unwrap();
    plugins.iter().filter(|p| p.name != "api").map(|p| (p.name.clone(), p.hash.clone())).collect()
}

pub fn get_loaded_plugins_info() -> Vec<(String, String, String)> {
    let plugins = PLUGINS.lock().unwrap();
    plugins.iter().filter(|p| p.name != "api").map(|p| (p.name.clone(), p.hash.clone(), p.manifest.version.clone())).collect()
}

/// Called when a UI tab is closed. Drops any plugin instance that was
/// running inside that tab so we don't leak WASM memory or get stale
/// callbacks firing into a now-deleted tab index.
pub fn unload_plugin_by_tab(tab_idx: i32) {
    use crate::ops::host::get_tab_owner;
    let owner = match get_tab_owner(tab_idx) {
        Some(o) if !o.is_empty() => o,
        _ => return, // Tab had no plugin owner — nothing to do
    };
    let mut plugins = PLUGINS.lock().unwrap();
    let before = plugins.len();
    plugins.retain(|p| p.name != owner);
    let removed = before - plugins.len();
    if removed > 0 {
        drop(plugins); // Release lock before calling back into UI
        crate::ops::host::print_info(&format!("[PLUG] Plugin '{}' unloaded (tab closed).", owner));
    }
}


fn dispatch_call_plugin_entry(plugin: &mut Plugin) -> bool {
    if let Ok(f) = plugin.instance.exports.get_typed_function::<(), i32>(&plugin.store, "main_plugin") {
        let _ = f.call(&mut plugin.store); return true;
    }
    if let Ok(f) = plugin.instance.exports.get_typed_function::<(), i32>(&plugin.store, "run") {
        let _ = f.call(&mut plugin.store); return true;
    }
    if let Ok(f) = plugin.instance.exports.get_typed_function::<(), ()>(&plugin.store, "run") {
        let _ = f.call(&mut plugin.store); return true;
    }
    if let Ok(f) = plugin.instance.exports.get_typed_function::<(), ()>(&plugin.store, "_start") {
        let _ = f.call(&mut plugin.store); return true;
    }
    if let Ok(f) = plugin.instance.exports.get_typed_function::<(), i32>(&plugin.store, "main") {
        let _ = f.call(&mut plugin.store); return true;
    }
    print_error(&format!("[WASM] Plugin '{}' is loaded but has no callable export.", plugin.name));
    false
}

pub fn dispatch_plugin_cmd(cmd: &str, args: &str) -> bool {
    let args_str = args.to_string();
    let cmd_str = cmd.to_string();
    let idx = {
        let plugins = PLUGINS.lock().unwrap();
        match plugins.iter().position(|p| p.name == cmd_str) {
            Some(i) => i,
            None => return false,
        }
    };

    set_prompt_visibility(false);

    POOL.execute(move || {
        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            let mut plugin = {
                let mut plugins = PLUGINS.lock().unwrap();
                if idx < plugins.len() { plugins.remove(idx) }
                else { set_prompt_visibility(true); return; }
            };

            let start_time = std::time::Instant::now();
            let label = cmd_str.to_uppercase();
            { let mut curr_args = CURRENT_ARGS.lock().unwrap(); *curr_args = args_str; }

            let _ = dispatch_call_plugin_entry(&mut plugin);
            let elapsed = start_time.elapsed().as_secs_f32();
            print_info(&format!("<~> {} responded in {:.1}s.", label, elapsed));

            { let mut plugins = PLUGINS.lock().unwrap(); plugins.push(plugin); }
        }));

        if let Err(e) = res {
            print_error(&format!("[RUST PANIC] Plugin task failed! {:?}", e));
        }
        set_prompt_visibility(true);
    });
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_args_empty() {
        let args = parse_args("");
        assert!(args.is_empty(), "Empty input should return empty vector");
    }

    #[test]
    fn test_parse_args_simple() {
        let args = parse_args("command arg1 arg2");
        assert_eq!(args, vec!["command", "arg1", "arg2"]);
    }

    #[test]
    fn test_parse_args_with_quotes() {
        let args = parse_args("command \"quoted argument\" regular_arg");
        assert_eq!(args, vec!["command", "quoted argument", "regular_arg"]);
    }

    #[test]
    fn test_parse_args_multiple_spaces() {
        let args = parse_args("   command    arg1   \"arg  with  spaces\"  ");
        assert_eq!(args, vec!["command", "arg1", "arg  with  spaces"]);
    }
}
