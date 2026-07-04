use std::fs;
use std::path::PathBuf;

pub fn c_del(args: *const i8) -> i32 {
    let hash = match crate::ops::utils::safe_cstr_to_string(args) {
        Some(val) => val.trim().to_string(),
        None => return -1,
    };

    if hash.is_empty() {
        crate::ops::host::print_error("Usage: /plug- [hash]");
        return -1;
    }

    let mut sys_drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".to_string());
    if sys_drive.ends_with(':') {
        sys_drive.push('\\');
    }
    let mut plugins_dir = PathBuf::from(sys_drive);
    plugins_dir.push(".plug");
    plugins_dir.push("plugins");

    if !plugins_dir.exists() {
        crate::ops::host::print_error("Plugins directory not found.");
        return -1;
    }

    let mut deleted_count = 0;

    let mut hash_files = Vec::new();
    if let Ok(entries) = fs::read_dir(&plugins_dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                if let Ok(sub_entries) = fs::read_dir(&p) {
                    for sub_entry in sub_entries.flatten() {
                        let sub_p = sub_entry.path();
                        if sub_p.extension().map_or(false, |ext| ext == "hash") {
                            hash_files.push(sub_p);
                        }
                    }
                }
            }
        }
    }

    for p in hash_files {
        if let Ok(content) = fs::read_to_string(&p) {
            if content.trim() == hash {
                let stem = p.file_stem().unwrap().to_str().unwrap().to_string();
                crate::ops::host::print_info(&format!("Deleting plugin: {}...", stem));

                let parent = p.parent().unwrap();
                if parent == plugins_dir {
                    let files_to_del = vec![
                        plugins_dir.join(format!("{}.plug", stem)),
                        plugins_dir.join(format!("{}.toml", stem)),
                        p.clone(),
                    ];
                    for f in files_to_del {
                        if f.exists() { let _ = fs::remove_file(f); }
                    }
                } else {
                    let _ = fs::remove_dir_all(parent);
                }
                deleted_count += 1;
            }
        }
    }

    if deleted_count > 0 {
        crate::ops::host::print_info("Plugin(s) deleted successfully.");
        crate::ops::host::print_info("Refreshing system plugins...");
        crate::ops::plugin_mgr::init_plugins(plugins_dir.to_str().unwrap());
    } else {
        crate::ops::host::print_error("No plugin found with that hash.");
    }

    0
}
