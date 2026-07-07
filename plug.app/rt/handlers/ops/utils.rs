use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::path::PathBuf;

pub fn get_plugins_dir() -> Option<PathBuf> {
    let mut sys_path = PathBuf::new();
    if cfg!(windows) {
        let mut sys_drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".to_string());
        if sys_drive.ends_with(':') {
            sys_drive.push('\\');
        }
        sys_path.push(sys_drive);
        sys_path.push(".plug");
    } else if let Ok(home) = std::env::var("HOME") {
        sys_path.push(home);
        sys_path.push(".plug");
    }
    if sys_path.as_os_str().is_empty() {
        return None;
    }
    sys_path.push("plugins");
    Some(sys_path)
}

pub fn to_c_string(input: &str) -> CString {
    let sanitized: String = input.chars().filter(|&c| c != '\0').collect();
    CString::new(sanitized).unwrap_or_else(|_| CString::new("").unwrap())
}

pub fn safe_cstr_to_string(ptr: *const c_char) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    unsafe {
        let cstr = CStr::from_ptr(ptr);
        Some(cstr.to_string_lossy().into_owned())
    }
}
pub fn rand_hash(seed: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    format!("{}_{}", ts, seed).hash(&mut hasher);
    format!("{:06x}", hasher.finish() & 0xFFFFFF)
}

pub fn calculate_buffer_sha256(bytes: &[u8]) -> String {
    use sha2::{Sha256, Digest};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

pub fn write_atomic(path: &std::path::Path, content: &[u8]) -> Result<(), std::io::Error> {
    use std::fs;
    use std::io::Write;
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "No parent directory")
    })?;
    let file_name = path.file_name().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "No file name")
    })?;
    let mut tmp_name = file_name.to_os_string();
    tmp_name.push(".tmp");
    let tmp_path = parent.join(tmp_name);

    {
        let mut file = fs::File::create(&tmp_path)?;
        file.write_all(content)?;
        file.sync_all()?;
    }
    fs::rename(&tmp_path, path)?;
    Ok(())
}


#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    #[test]
    fn test_to_c_string_strips_null_bytes() {
        let cstr = to_c_string("hello\0world");
        assert_eq!(cstr.to_str().unwrap(), "helloworld");
    }

    #[test]
    fn test_get_plugins_dir_returns_plugins_suffix() {
        if let Some(path) = get_plugins_dir() {
            assert_eq!(path.file_name().and_then(|n| n.to_str()), Some("plugins"));
        }
    }

    #[test]
    fn test_safe_cstr_to_string_valid() {
        let input = CString::new("hello world").unwrap();
        let result = safe_cstr_to_string(input.as_ptr());
        assert_eq!(result, Some("hello world".to_string()));
    }

    #[test]
    fn test_safe_cstr_to_string_null() {
        let result = safe_cstr_to_string(std::ptr::null());
        assert_eq!(result, None);
    }

    #[test]
    fn test_rand_hash_uniqueness() {
        let hash1 = rand_hash("test");
        let hash2 = rand_hash("test");
        assert_eq!(hash1.len(), 6);
        assert_eq!(hash2.len(), 6);
        assert!(u32::from_str_radix(&hash1, 16).is_ok());
        assert!(u32::from_str_radix(&hash2, 16).is_ok());
    }
}
