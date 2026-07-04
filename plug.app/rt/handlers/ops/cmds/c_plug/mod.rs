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

    let plugin_name = {
        let session = SESSION_PLUGINS.lock().unwrap();
        match session.get(&hash) {
            Some(name) => name.clone(),
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

    install_plugin_manually(&clean_name, &hash, false)
}

pub fn install_plugin_manually(clean_name: &str, session_hash: &str, silent: bool) -> i32 {
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
    
    let url_wasm = format!(
        "https://raw.githubusercontent.com/PlugFrameWork/plug/plug/trunk/plugins/{}",
        clean_name
    );

    let url_toml = "https://raw.githubusercontent.com/PlugFrameWork/plug/plug/trunk/plugins/plugin.toml";
    let mut dest_toml = dest_root.clone();
    dest_toml.push("plugin.toml");

    if progress::download_with_progress(&url_wasm, dest_wasm.to_str().unwrap(), silent) {
        if !silent {
            print_info("Downloading unified manifest file...");
        }
        if progress::download_with_progress(url_toml, dest_toml.to_str().unwrap(), true) {
            if !silent {
                print_info("Files downloaded.");
            }
            
            let mut dest_hash = dest_dir.clone();
            dest_hash.push(format!("{}.hash", clean_name));
            if let Err(e) = fs::write(&dest_hash, session_hash) {
                print_error(&format!("Failed to write selection hash: {}", e));
            }

            if !silent {
                print_info(&format!("Plugin '{}' successfully installed.", clean_name));
                print_info(&format!("  Hash   : {}", session_hash));
                print_info("  Status : Installation complete. Plugin is ready to use.");
            }

            crate::ops::plugin_mgr::init_plugins(dest_root.to_str().unwrap());
        } else {
            print_error("Failed to download the plugin manifest. Installation aborted.");
        }
    } else {
        print_error("Failed to download the plugin. Check your network or the plugin name.");
    }

    0
}
