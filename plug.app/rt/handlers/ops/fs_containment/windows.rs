// windows feasibility spike
// implements verified parent handle walking and handle-directed atomic deletion

#![allow(unused_imports, dead_code)]
use super::*;
use std::path::{Path, PathBuf};

pub struct WindowsContainmentEngine;

impl WindowsContainmentEngine {
    pub fn new() -> Self {
        Self
    }
}

impl AtomicContainmentEngine for WindowsContainmentEngine {
    fn open_beneath(&self, _root_fd: i32, _rel_path: &str, _flags: OpenFlags) -> Result<i32, i32> {
        // feasibility track in progress
        Err(WASI_ENOTCAPABLE)
    }

    fn mkdir_beneath(&self, _root_fd: i32, _rel_path: &str) -> Result<(), i32> {
        Err(WASI_ENOTCAPABLE)
    }

    fn unlink_file_beneath(&self, _root_fd: i32, _rel_path: &str) -> Result<(), i32> {
        Err(WASI_ENOTCAPABLE)
    }

    fn rmdir_beneath(&self, _root_fd: i32, _rel_path: &str) -> Result<(), i32> {
        Err(WASI_ENOTCAPABLE)
    }
}
