use std::ffi::CStr;
use std::os::raw::c_char;

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

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
