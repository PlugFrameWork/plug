use std::fs;
use std::env;
use crate::ops::cmds::SESSION_PLUGINS;
use crate::ops::host::{print_info, print_error};

#[derive(serde::Deserialize)]
pub struct CloudPlugin {
    pub name: String,
    pub version: String,
    pub author: String,
    pub description: String,
    pub official: bool,
    #[serde(default)]
    pub sha256: Option<String>,
}

#[derive(serde::Deserialize)]
pub struct CloudRepo {
    pub repo_version: String,
    pub plugins: Vec<CloudPlugin>,
}



fn compare_versions(registry: &str, local: &str) -> bool {
    let parse_ver = |s: &str| -> Vec<u32> {
        let clean_s: String = s.chars().filter(|c| c.is_digit(10) || *c == '.').collect();
        clean_s.split('.')
            .filter_map(|x| x.parse::<u32>().ok())
            .collect()
    };
    parse_ver(registry) > parse_ver(local)
}

pub fn download_file_hr(url: &str, dest: &str) -> i32 {
    let req = ureq::get(url)
        .set("Cache-Control", "no-cache")
        .set("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36");
    match req.call() {
        Ok(res) => {
            if let Ok(mut file) = fs::File::create(dest) {
                if let Ok(_) = std::io::copy(&mut res.into_reader(), &mut file) {
                    return 0; // Success
                }
            }
        }
        Err(_) => {}
    }
    -1 // Generic error
}

pub fn c_check(_args: *const i8) -> i32 {
    let _ = crate::ops::utils::safe_cstr_to_string(_args).unwrap_or_default();
    print_info("\n--- SYSTEM STATUS ---");
    let loaded = crate::ops::plugin_mgr::get_loaded_plugins();
    if loaded.is_empty() {
        print_info("No plugins running.");
    } else {
        print_info("Running plugins:");
        for (p, h) in loaded {
            print_info(&format!("  [{}] {}", h, p));
        }
    }

    print_info("\n--- PLUGIN REGISTRY ---");
    print_info("Connecting to official repository...");

    let registry_url = std::env::var("PLUG_REGISTRY_URL").unwrap_or_else(|_| {
        "https://raw.githubusercontent.com/PlugFrameWork/plug/trunk/pluglists.json".to_string()
    });
    let temp_dir = env::temp_dir();
    let temp_file = temp_dir.join("pluglists.json");
    let temp_path = temp_file.to_str().unwrap();
    let last_hr = download_file_hr(&registry_url, temp_path);
    let mut ok = last_hr == 0;
    if !ok {
        // local fallback: copy registry from pluglists.json if download fail
        if let Some(local_registry) = find_local_file("pluglists.json") {
            if let Ok(_) = fs::copy(&local_registry, temp_path) {
                ok = true;
            }
        }
    }
    if !ok {
        print_info(&format!(
            "  [WARN] Download failed (HRESULT=0x{:08X})", last_hr as u32
        ));
    }

    if ok {
        if let Ok(content) = fs::read_to_string(temp_path) {
            let mut session = SESSION_PLUGINS.lock().unwrap();
            session.clear();

            match serde_json::from_str::<CloudRepo>(&content) {
                Ok(repo) => {
                    print_info(&format!("Plug Registry v{}", repo.repo_version));
                    let loaded_info = crate::ops::plugin_mgr::get_loaded_plugins_info();
                    for p in repo.plugins {
                        let hash = crate::ops::utils::rand_hash(&p.name);
                        session.insert(hash.clone(), (p.name.clone(), p.sha256.clone().unwrap_or_default()));
                        let off_tag = if p.official { "[Official]" } else { "" };
                        let mut status_tag = String::new();
                        if let Some((_, _, local_ver)) = loaded_info.iter().find(|(n, _, _)| *n == p.name) {
                            if compare_versions(&p.version, local_ver) {
                                status_tag = format!(" [OUTDATED: Local v{} -> Registry v{}]", local_ver, p.version);
                            } else {
                                status_tag = " [Installed]".to_string();
                            }
                        }
                        print_info(&format!("  [{}] {} v{} (author: {}) {}{}", hash, p.name, p.version, p.author, off_tag, status_tag));
                        print_info(&format!("         -> {}", p.description));
                    }
                }
                Err(e) => {
                    print_error(&format!("Failed to parse JSON registry: {}", e));
                }
            }
            if session.is_empty() {
                print_info("Plugin list is empty.");
            } else {
                print_info("\nUse /plug [hash] to download and install a plugin.");
            }
        } else {
            print_info("Failed to read plugin list from temp storage.");
        }
        let _ = fs::remove_file(temp_path);
    } else {
        print_info(&format!("Network Error: Could not fetch plugin list from Github. Last HRESULT=0x{:08X}", last_hr as u32));
    }
    0
}

fn find_local_file(filename: &str) -> Option<std::path::PathBuf> {
    // scan upwards from executable path to find target file
    if let Ok(exe) = std::env::current_exe() {
        let mut parent = exe.parent();
        while let Some(p) = parent {
            let path = p.join(filename);
            if path.exists() && path.is_file() {
                return Some(path);
            }
            parent = p.parent();
        }
    }
    // scan upwards from current directory
    if let Ok(cur) = std::env::current_dir() {
        let mut parent = Some(cur.as_path());
        while let Some(p) = parent {
            let path = p.join(filename);
            if path.exists() && path.is_file() {
                return Some(path);
            }
            parent = p.parent();
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deserialize_registry_with_sha256() {
        let json_data = r#"{
            "repo_version": "1.0.0",
            "plugins": [
                {
                    "name": "pTerm",
                    "author": "plug",
                    "version": "1.0.0",
                    "description": "Terminal environment",
                    "official": true,
                    "sha256": "0c36f35332eada4fd8dd6e5d3e6678bf637d3be6176d6ef638bffbe4a643e044"
                }
            ]
        }"#;

        let parsed: Result<CloudRepo, _> = serde_json::from_str(json_data);
        assert!(parsed.is_ok(), "Registry with sha256 should parse successfully");
        let repo = parsed.unwrap();
        assert_eq!(repo.plugins[0].name, "pTerm");
        assert_eq!(repo.plugins[0].sha256, Some("0c36f35332eada4fd8dd6e5d3e6678bf637d3be6176d6ef638bffbe4a643e044".to_string()));
    }

    #[test]
    fn test_deserialize_registry_missing_sha256() {
        let json_data = r#"{
            "repo_version": "1.0.0",
            "plugins": [
                {
                    "name": "pTerm",
                    "author": "plug",
                    "version": "1.0.0",
                    "description": "Terminal environment",
                    "official": true
                }
            ]
        }"#;

        let parsed: Result<CloudRepo, _> = serde_json::from_str(json_data);
        assert!(parsed.is_ok(), "Registry without sha256 should parse successfully (backwards compatible)");
        let repo = parsed.unwrap();
        assert_eq!(repo.plugins[0].name, "pTerm");
        assert_eq!(repo.plugins[0].sha256, None, "Missing sha256 field should deserialize to None");
    }
}
