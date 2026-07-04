use std::path::PathBuf;
use std::fs;
use crate::ops::cmds::SESSION_PLUGINS;
use crate::ops::host::{print_info, print_error};

pub mod check;
pub mod progress;
pub mod del;

pub fn c_plug(args: *const i8) -> i32 {
    let hash = match crate::ops::utils::safe_cstr_to_string(args) {
        Some(val) => val.trim().to_string(),
        None => return -1,
    };

    if hash.is_empty() {
        print_error("Usage: /plug [hash]");
        return -1;
    }

    let (plugin_name, registry_sha256) = {
        let session = SESSION_PLUGINS.lock().unwrap();
        match session.get(&hash) {
            Some((name, sha)) => (name.clone(), sha.clone()),
            None => {
                print_error("Invalid or expired hash. Run /plug* to get a fresh list.");
                return -1;
            }
        }
    };

    let clean_name = plugin_name
        .trim_end_matches(".plug")
        .trim_end_matches(".wasm")
        .to_string();

    install_plugin_manually(&clean_name, &hash, &registry_sha256, false)
}

pub fn install_plugin_manually(clean_name: &str, session_hash: &str, registry_sha256: &str, silent: bool) -> i32 {
    if !silent {
        print_info(&format!("Downloading: {}.plug...", clean_name));
    }

    let mut sys_drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".to_string());
    if sys_drive.ends_with(':') {
        sys_drive.push('\\');
    }
    let mut dest_root = PathBuf::from(sys_drive);
    dest_root.push(".plug");
    dest_root.push("plugins");

    if !dest_root.exists() {
        let _ = fs::create_dir_all(&dest_root);
    }

    let mut dest_dir = dest_root.clone();
    dest_dir.push(clean_name);
    
    if !dest_dir.exists() {
        let _ = fs::create_dir_all(&dest_dir);
    }

    let mut dest_wasm = dest_dir.clone();
    dest_wasm.push(format!("{}.{}", clean_name, session_hash));
    let mut tmp_wasm = dest_wasm.clone();
    let mut tmp_wasm_name = tmp_wasm.file_name().unwrap_or_default().to_os_string();
    tmp_wasm_name.push(".tmp");
    tmp_wasm.set_file_name(tmp_wasm_name);
    
    let base_url = std::env::var("PLUG_WASM_BASE_URL").unwrap_or_else(|_| {
        "https://raw.githubusercontent.com/PlugFrameWork/plug/trunk/plugins/".to_string()
    });
    let base_url = if base_url.is_empty() || base_url.ends_with('/') {
        base_url
    } else {
        format!("{}/", base_url)
    };

    let url_wasm = format!("{}{}/{}", base_url, clean_name, clean_name);
    let url_toml = format!("{}plugin.toml", base_url);
    let mut dest_toml = dest_root.clone();
    dest_toml.push("plugin.toml");
    let mut tmp_toml = dest_toml.clone();
    let mut tmp_toml_name = tmp_toml.file_name().unwrap_or_default().to_os_string();
    tmp_toml_name.push(".tmp");
    tmp_toml.set_file_name(tmp_toml_name);

    let mut success = false;

    if progress::download_with_progress(&url_wasm, tmp_wasm.to_str().unwrap(), silent) {
        if !silent {
            print_info("Downloading unified manifest file...");
        }
        if progress::download_with_progress(&url_toml, tmp_toml.to_str().unwrap(), true) {
            let wasm_bytes = match fs::read(&tmp_wasm) {
                Ok(bytes) => bytes,
                Err(e) => {
                    print_error(&format!("Failed to read downloaded WASM: {}", e));
                    let _ = fs::remove_file(&tmp_wasm);
                    let _ = fs::remove_file(&tmp_toml);
                    return -1;
                }
            };

            let sha256_hex = crate::ops::utils::calculate_buffer_sha256(&wasm_bytes);

            // Phase 2.3: Verify against registry-pinned hash (fail-closed on missing or mismatch)
            if registry_sha256.is_empty() {
                print_error(&format!("[SECURITY] Missing pinned hash in registry for plugin: {}. Aborting installation.", clean_name));
                let _ = fs::remove_file(&tmp_wasm);
                let _ = fs::remove_file(&tmp_toml);
                return -1;
            }

            if sha256_hex != registry_sha256 {
                print_error(&format!("[SECURITY] Pinned registry hash mismatch for plugin: {} (Expected: {}, Found: {})", clean_name, registry_sha256, sha256_hex));
                let _ = fs::remove_file(&tmp_wasm);
                let _ = fs::remove_file(&tmp_toml);
                return -1;
            }
            
            // Rename files atomically
            if let Err(e) = fs::rename(&tmp_wasm, &dest_wasm) {
                print_error(&format!("Failed to finalize WASM file: {}", e));
                let _ = fs::remove_file(&tmp_wasm);
                let _ = fs::remove_file(&tmp_toml);
                return -1;
            }

            if let Err(e) = fs::rename(&tmp_toml, &dest_toml) {
                print_error(&format!("Failed to finalize manifest file: {}", e));
                let _ = fs::remove_file(&dest_wasm);
                let _ = fs::remove_file(&tmp_toml);
                return -1;
            }

            let mut dest_integrity = dest_dir.clone();
            dest_integrity.push(format!("{}.integrity", clean_name));
            if let Err(e) = crate::ops::utils::write_atomic(&dest_integrity, sha256_hex.as_bytes()) {
                print_error(&format!("Failed to write integrity file: {}", e));
            }

            let mut dest_hash = dest_dir.clone();
            dest_hash.push(format!("{}.hash", clean_name));
            if let Err(e) = crate::ops::utils::write_atomic(&dest_hash, session_hash.as_bytes()) {
                print_error(&format!("Failed to write selection hash: {}", e));
            }

            if !silent {
                print_info("Files downloaded.");
                print_info(&format!("Plugin '{}' successfully installed.", clean_name));
                print_info(&format!("  Hash   : {}", session_hash));
                print_info("  Status : Installation complete. Plugin is ready to use.");
            }

            crate::ops::plugin_mgr::init_plugins(dest_root.to_str().unwrap());
            success = true;
        } else {
            print_error("Failed to download the plugin manifest. Installation aborted.");
            let _ = fs::remove_file(&tmp_wasm);
            let _ = fs::remove_file(&tmp_toml);
        }
    } else {
        print_error("Failed to download the plugin. Check your network or the plugin name.");
        let _ = fs::remove_file(&tmp_wasm);
    }

    if success { 0 } else { -1 }
}
