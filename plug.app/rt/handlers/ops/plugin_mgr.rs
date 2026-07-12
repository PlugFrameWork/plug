use wasmer::{Instance, Module, Store, Function, imports, Memory, FunctionEnv, FunctionEnvMut};
use wasmer_compiler_cranelift::Cranelift;
use std::path::{Path, PathBuf};
use std::fs;
use std::cell::RefCell;
use once_cell::sync::Lazy;
use std::sync::{Mutex, Arc, mpsc};
use std::process::{Command, Stdio};
use std::io::{BufRead, BufReader};
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use serde::{Deserialize, Serialize};
use toml;
use crate::ops::host::{
    print_info, print_error,
    add_tab, set_tab_cwd, set_tab_owner,
    get_current_print_tab, set_prompt_visibility
};
use crate::ops::net::http_post_json;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;
const HOST_API_VERSION: &str = "0.1.2a";
// maximum allowed lengths for FFI string reads (prevent OOB/alloc DoS)
const MAX_FFI_STRING_LEN: usize = 64 * 1024;      // 64 KiB general
const MAX_ARGS_LEN: usize = 4096;                  // command args
const MAX_ENV_NAME_LEN: usize = 256;               // env var name
const MAX_URL_LEN: usize = 2048;                   // URL
const MAX_JSON_PAYLOAD_LEN: usize = 16 * 1024;     // JSON body
const MAX_RESPONSE_BUF_LEN: usize = 1024 * 1024;   // 1 MiB response buffer
const MAX_TAB_LABEL_LEN: usize = 256;              // tab label

// thread-local storage for current command args (avoids race between plugins)
thread_local! {
    static CURRENT_ARGS: RefCell<String> = RefCell::new(String::new());
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct AllowedCommand {
    pub path: String,
    pub args_pattern: String,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Manifest {
    pub name: String,
    pub version: String,
    pub author: String,
    pub api_version: String,
    pub permissions: Vec<String>,
    pub allowed_commands: Option<Vec<AllowedCommand>>,
}

fn resolve_binary_path(exe: &str) -> Option<PathBuf> {
    let path = Path::new(exe);
    if path.is_absolute() {
        return fs::canonicalize(path).ok();
    }
    
    if let Ok(cur) = std::env::current_dir() {
        let p = cur.join(exe);
        if p.exists() {
            return fs::canonicalize(p).ok();
        }
    }
    
    if let Ok(paths) = std::env::var("PATH") {
        for p in std::env::split_paths(&paths) {
            let bin_path = p.join(exe);
            if bin_path.exists() {
                return fs::canonicalize(bin_path).ok();
            }
            if cfg!(windows) {
                for ext in &[".exe", ".bat", ".cmd", ".com"] {
                    let bin_path_ext = p.join(format!("{}{}", exe, ext));
                    if bin_path_ext.exists() {
                        return fs::canonicalize(bin_path_ext).ok();
                    }
                }
            }
        }
    }
    None
}

static TRUSTED_PLUGIN_HASHES: &[&str] = &[
    include_str!("../../pterm_hash.txt")
];

#[derive(Deserialize, Clone, Debug)]
pub struct PluginManifest {
    pub plugin: Vec<Manifest>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct SinglePluginManifest {
    pub plugin: Manifest,
}

// TOML format for .integrity sidecar
#[derive(Deserialize)]
struct IntegrityToml {
    wasm_sha256: String,
    permissions_hash: Option<String>,
}

pub struct Plugin {
    pub name: String,
    pub hash: String,
    pub manifest: Manifest,
    pub is_trusted: bool,
    store: Store,
    instance: Instance,
}

struct Env {
    memory: Option<Memory>,
    permissions: Vec<String>,
    allowed_commands: Vec<AllowedCommand>,
    is_trusted: bool,
}

static PLUGINS: Lazy<Mutex<Vec<Plugin>>> = Lazy::new(|| Mutex::new(Vec::new()));
static PLUGIN_EXEC_LOCKS: Lazy<Mutex<std::collections::HashMap<String, Arc<Mutex<()>>>>> =
    Lazy::new(|| Mutex::new(std::collections::HashMap::new()));
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

fn plugin_exec_lock(name: &str) -> Arc<Mutex<()>> {
    let mut locks = PLUGIN_EXEC_LOCKS.lock().unwrap();
    locks
        .entry(name.to_string())
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone()
}

fn import_requires_permission(name: &str) -> Option<&'static str> {
    match name {
        "host_exec" => Some("host_exec"),
        "host_add_tab" => Some("host_add_tab"),
        "host_set_tab_owner" => Some("host_set_tab_owner"),
        "host_get_tab_label" => Some("host_get_tab_label"),
        "get_env" => Some("get_env"),
        "net_post" => Some("net_post"),
        "host_get_platform" => Some("host_get_platform"),
        _ => None,
    }
}

fn has_permission(permissions: &[String], required: &str) -> bool {
    permissions.iter().any(|p| p == required)
}

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

    let dest_root = path.parent().unwrap_or(path);
    let marker_path = dest_root.join(".integrity_migration_v1_done");

    // phase 1.5 one-time global migration check
    if !marker_path.exists() {
        print_info("[SECURITY-WARNING] Global integrity migration started. Backfilling plugin .integrity files...");
        for p in &plug_files {
            if let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
                let name = stem.split('.').next().unwrap_or(stem);
                let mut integrity_path = p.parent().unwrap().to_path_buf();
                integrity_path.push(format!("{}.integrity", name));
                if !integrity_path.exists() {
                    let hash_val = fs::read_to_string(p).unwrap_or_default().trim().to_string();
                    if !hash_val.is_empty() {
                        let wasm_path = p.with_extension(&hash_val);
                        if wasm_path.exists() {
                            if let Ok(bytes) = fs::read(&wasm_path) {
                                let sha256_hex = crate::ops::utils::calculate_buffer_sha256(&bytes);
                                // write in TOML format with wasm_sha256 only (permissions_hash comes
                                // from registry install, not from migration)
                                let integrity_content = format!("wasm_sha256 = \"{}\"\n", sha256_hex);
                                if let Err(e) = crate::ops::utils::write_atomic(&integrity_path, integrity_content.as_bytes()) {
                                    print_error(&format!("Migration failed to write integrity file for {}: {}", name, e));
                                } else {
                                    print_info(&format!("[SECURITY-WARNING] Generated integrity file for pre-existing plugin: {}", name));
                                }
                            }
                        }
                    }
                }
            }
        }

        // write marker atomically
        if let Err(e) = crate::ops::utils::write_atomic(&marker_path, b"v1_done") {
            print_error(&format!("Failed to write global migration marker: {}", e));
        } else {
            print_info("[SECURITY] Global integrity migration complete. Marker written.");
        }
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
    let sha256_hex = crate::ops::utils::calculate_buffer_sha256(&wasm_bytes);

    let mut is_trusted = false;
    if TRUSTED_PLUGIN_HASHES.iter().any(|h| h.trim() == sha256_hex) {
        is_trusted = true;
    }

    if !is_trusted {
        let stem_opt = wasm_path.file_stem().and_then(|s| s.to_str());
        let name = if let Some(stem) = stem_opt {
            stem.split('.').next().unwrap_or(stem)
        } else {
            manifest.name.as_str()
        };
        let integrity_path = wasm_path.parent().unwrap().join(format!("{}.integrity", name));
        if !integrity_path.exists() {
            return Err(format!("[SECURITY] Missing integrity sidecar for {}", name).into());
        }

        // parse integrity file — support TOML format (wasm_sha256 + permissions_hash)
        // fall back to legacy single-line SHA256 hex
        let integrity_raw = fs::read_to_string(&integrity_path)?.trim().to_string();
        let (expected_wasm_hash, expected_perms_hash) = if integrity_raw.starts_with("wasm_sha256") {
            // TOML format
            match toml::from_str::<IntegrityToml>(&integrity_raw) {
                Ok(t) => (t.wasm_sha256, t.permissions_hash),
                Err(_) => return Err(format!("[SECURITY] Corrupt integrity file for {}", name).into()),
            }
        } else {
            // legacy single-line SHA256
            (integrity_raw, None)
        };

        if expected_wasm_hash != sha256_hex {
            return Err(format!("[SECURITY] Integrity mismatch for {}", name).into());
        }

        // if integrity pins permissions_hash, compute current manifest permissions hash
        // and compare. fail-closed on mismatch — manifest may have been tampered.
        if let Some(ref pinned_perms_hash) = expected_perms_hash {
            let current_perms_hash = crate::ops::utils::calculate_permissions_hash(manifest);
            if current_perms_hash != *pinned_perms_hash {
                return Err(format!("[SECURITY] Permissions hash mismatch for {} (manifest may have been tampered)", name).into());
            }
        }
    }

    if !manifest.api_version.starts_with("0.1") {
        return Err(format!("Incompatible API version: {} (Host: {})", manifest.api_version, HOST_API_VERSION).into());
    }

    let mut store = Store::new(Cranelift::default());
    let module = Module::new(&store, &wasm_bytes)?;

    // verify imported function against manifest permission declaration
    for import in module.imports() {
        if import.module() == "env" {
            let name = import.name();
            if let Some(required) = import_requires_permission(name) {
                if !has_permission(&manifest.permissions, required) {
                    return Err(format!("[SECURITY] Unauthorized import: {}", name).into());
                }
            }
        } else if import.module() == "wasi_snapshot_preview1" {
            // SECURITY: wasi_snapshot_preview1 imports MUST be explicitly allowed
            // only a safe subset of WASI preview1 is exposed; dangerous syscalls are blocked
            // path_* and sock_* are NOT allowed - plugins must use host FFI with permissions
            // proc_raise, random_get are NOT allowed - they provide dangerous capabilities
            let name = import.name();
            const ALLOWED_WASI: &[&str] = &[
                // process / exit
                "proc_exit",
                // file descriptor ops (stdin/stdout/stderr only)
                "fd_write", "fd_read", "fd_close", "fd_seek", "fd_fdstat_get",
                "fd_fdstat_set_flags", "fd_fdstat_set_rights", "fd_prestat_get",
                "fd_prestat_dir_name", "fd_advise", "fd_allocate", "fd_datasync",
                "fd_filestat_get", "fd_filestat_set_size", "fd_filestat_set_times",
                "fd_pread", "fd_pwrite", "fd_readdir", "fd_renumber", "fd_sync",
                "fd_tell",
                // clocks / args / env (read-only stubs)
                "clock_res_get", "clock_time_get",
                "args_sizes_get", "args_get", "environ_sizes_get", "environ_get",
                // poll / sched (stubs returning ENOSYS)
                "poll_oneoff", "sched_yield",
            ];
            if !ALLOWED_WASI.contains(&name) {
                return Err(format!("[SECURITY] Unauthorized WASI import: {}", name).into());
            }
        }
    }

    let env = FunctionEnv::new(&mut store, Env { 
        memory: None, 
        permissions: manifest.permissions.clone(),
        allowed_commands: manifest.allowed_commands.clone().unwrap_or_default(),
        is_trusted,
    });

    let import_object = imports! {
        "env" => {
            "print_info" => Function::new_typed_with_env(&mut store, &env, |mut env: FunctionEnvMut<Env>, ptr: i32, len: i32| {
                let (env_data, store) = env.data_and_store_mut();
                if len <= 0 || len as usize > MAX_FFI_STRING_LEN { return; }
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
                if len <= 0 || len as usize > MAX_FFI_STRING_LEN { return; }
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
                if max_len <= 0 || max_len as usize > MAX_ARGS_LEN { return; }
                if let Some(memory) = &env_data.memory {
                    let view = memory.view(&store);
                    CURRENT_ARGS.with(|args| {
                        let args = args.borrow();
                        let bytes = args.as_bytes();
                        let cap = (max_len as usize).saturating_sub(1);
                        let n = std::cmp::min(bytes.len(), cap);
                        if view.write(ptr as u64, &bytes[..n]).is_ok() {
                            let _ = view.write((ptr as u64) + n as u64, &[0u8]);
                        }
                    });
                }
            }),

            "host_exec" => Function::new_typed_with_env(&mut store, &env, |mut env: FunctionEnvMut<Env>, ptr: i32, len: i32| {
                let (env_data, store) = env.data_and_store_mut();
                if !env_data.permissions.iter().any(|p| p == "host_exec") {
                    print_error("[SECURITY] Plugin attempted to call host_exec without permission");
                    return;
                }
                if len <= 0 || len as usize > MAX_FFI_STRING_LEN { return; }
                if let Some(memory) = &env_data.memory {
                    let view = memory.view(&store);
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
                                // security: enforce path containment within fixed jail root
                                // use the tab's original working directory as the jail root
                                // (not the dynamic CWD which can be changed via cd)
                                let jail_root = {
                                    let cwds = TAB_CWDS.lock().unwrap();
                                    cwds.get(&tab_idx).cloned().unwrap_or_else(|| {
                                        std::env::current_dir().unwrap_or_default().to_string_lossy().replace("\\\\?\\", "").to_string()
                                    })
                                };
                                if !path_str.starts_with(&jail_root) {
                                    print_error("Access denied: path escapes allowed directory");
                                    return;
                                };
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

                        let canonical_exe = if env_data.is_trusted {
                            resolve_binary_path(&exe).unwrap_or_else(|| std::path::PathBuf::from(&exe))
                        } else {
                            // b1. resolve absolute canonical path
                            match resolve_binary_path(&exe) {
                                Some(p) => p,
                                None => {
                                    print_error(&format!("[SECURITY] Blocked execution: binary not found: {}", exe));
                                    return;
                                }
                            }
                        };

                        if !env_data.is_trusted {
                            // allowlist-only execution: no blacklist (blacklists are bypassable)
                            // only binaries explicitly declared in manifest allowed_commands with matching
                            // canonical path and args regex are permitted
                            let mut allowed = false;
                            let args_string = parsed_args.join(" ");
                            for entry in &env_data.allowed_commands {
                                if let Ok(entry_canonical) = fs::canonicalize(&entry.path) {
                                    if entry_canonical == canonical_exe {
                                        if let Ok(re) = regex::Regex::new(&entry.args_pattern) {
                                            if re.is_match(&args_string) {
                                                allowed = true;
                                                break;
                                            }
                                        }
                                    }
                                }
                            }
                            
                            if !allowed {
                                // test coupling: this message is asserted in tests/integration/test_sandbox_rules.py
                                // (has_unauth_block check). rename here → must update assertion string there
                                print_error(&format!("[SECURITY] Blocked execution of unauthorized binary: {}", canonical_exe.display()));
                                return;
                            }
                        }

                        let sub_cwd = {
                            let cwds = TAB_CWDS.lock().unwrap();
                            cwds.get(&tab_idx).cloned().unwrap_or_else(|| {
                                std::env::current_dir().unwrap_or_default().to_string_lossy().to_string()
                            })
                        };
                        let mut cmd = Command::new(&canonical_exe);
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
                if !has_permission(&env_data.permissions, "get_env") {
                    print_error("[SECURITY] Plugin attempted to call get_env without permission");
                    return;
                }
                if name_len <= 0 || name_len as usize > MAX_ENV_NAME_LEN { return; }
                if resp_max_len <= 0 || resp_max_len as usize > MAX_FFI_STRING_LEN { return; }
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
            "host_get_platform" => Function::new_typed_with_env(&mut store, &env, |mut _env: FunctionEnvMut<Env>| -> i32 {
                if cfg!(windows) { 0 } else { 1 }
            }),
            "host_add_tab" => Function::new_typed_with_env(&mut store, &env, |mut env: FunctionEnvMut<Env>, ptr: i32, len: i32| {
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
                if max_len <= 0 || max_len as usize > MAX_TAB_LABEL_LEN { return 0; }
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
                if !has_permission(&env_data.permissions, "net_post") {
                    print_error("[SECURITY] Plugin attempted to call net_post without permission");
                    return;
                }
                if url_len <= 0 || url_len as usize > MAX_URL_LEN { return; }
                if body_len <= 0 || body_len as usize > MAX_JSON_PAYLOAD_LEN { return; }
                if resp_max_len <= 0 || resp_max_len as usize > MAX_RESPONSE_BUF_LEN { return; }
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
                    CURRENT_ARGS.with(|args| {
                        let args = args.borrow();
                        let parsed = parse_args(&args);
                        let mut argc = 1;
                        let mut size = "plugin\0".len();
                        for arg in parsed.iter() { argc += 1; size += arg.len() + 1; }
                        let _ = view.write(argc_ptr as u64, &(argc as u32).to_le_bytes());
                        let _ = view.write(argv_buf_size_ptr as u64, &(size as u32).to_le_bytes());
                    });
                }
                0
            }),
            "args_get" => Function::new_typed_with_env(&mut store, &env, |mut env: FunctionEnvMut<Env>, mut argv_ptr: i32, mut argv_buf_ptr: i32| -> i32 {
                let (env_data, store) = env.data_and_store_mut();
                if let Some(memory) = &env_data.memory {
                    let view = memory.view(&store);
                    CURRENT_ARGS.with(|args| {
                        let args = args.borrow();
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
                    });
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
            "poll_oneoff" => Function::new_typed(&mut store, |_: i32, _: i32, _: i32, _: i32| -> i32 { 0 }),
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
        is_trusted,
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

/// called when ui tab is closed. drops any plugin instance that was
/// running inside that tab so we don't leak wasm memory or get stale
/// callback firing into now-deleted tab index
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
    {
        let plugins = PLUGINS.lock().unwrap();
        if !plugins.iter().any(|p| p.name == cmd_str) {
            return false;
        }
    }

    let exec_lock = plugin_exec_lock(&cmd_str);
    set_prompt_visibility(false);

    POOL.execute(move || {
        let _guard = exec_lock.lock().unwrap();
        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            let start_time = std::time::Instant::now();
            let label = cmd_str.to_uppercase();
            // set thread-local args for this plugin execution
            CURRENT_ARGS.with(|args| {
                *args.borrow_mut() = args_str;
            });

            let dispatched = {
                let mut plugins = PLUGINS.lock().unwrap();
                if let Some(plugin) = plugins.iter_mut().find(|p| p.name == cmd_str) {
                    dispatch_call_plugin_entry(plugin)
                } else {
                    false
                }
            };

            if dispatched {
                let elapsed = start_time.elapsed().as_secs_f32();
                print_info(&format!("<~> {} responded in {:.1}s.", label, elapsed));
            }
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
    fn test_import_requires_permission_mapping() {
        assert_eq!(import_requires_permission("host_exec"), Some("host_exec"));
        assert_eq!(import_requires_permission("get_env"), Some("get_env"));
        assert_eq!(import_requires_permission("net_post"), Some("net_post"));
        assert_eq!(import_requires_permission("print_info"), None);
        assert_eq!(import_requires_permission("host_get_platform"), Some("host_get_platform"));
    }

    #[test]
    fn test_has_permission() {
        let perms = vec!["host_exec".to_string(), "get_env".to_string()];
        assert!(has_permission(&perms, "host_exec"));
        assert!(has_permission(&perms, "get_env"));
        assert!(!has_permission(&perms, "net_post"));
    }

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
