# Plugin Development Guide

This guide describes how to write, compile, and distribute WebAssembly (WASM) plugins for the **plug** platform.

---

## 1. Plugin Architecture

Plugins are WebAssembly modules compiled targeting `wasm32-unknown-unknown`. When a plugin is loaded, the host exposes specific environment functions (imports) and expects a entry function `run` (export).

```
+--------------------------------------------------------+
|                      Host UI                           |
|       (Displays and updates active terminals)           |
+--------------------------^-----------------------------+
                           |
                     Host FFI Imports
   - print_info(ptr, len)      - main_w_add_tab(ptr, len)
   - print_error(ptr, len)     - host_set_tab_owner(ptr, len)
   - get_args(ptr, max_len)    - host_get_tab_label(ptr, len)
                           |
+--------------------------+-----------------------------+
|                  WASM Plugin Sandbox                   |
|  - Exports: `pub extern "C" fn run()`                  |
|  - Internal logic: commands executing / terminal sync |
+--------------------------------------------------------+
```

---

## 2. Basic Rust Template

Create a new Rust library project to write a plugin:

```bash
cargo new my_plugin --lib
```

Add the following to `Cargo.toml`:

```toml
[lib]
crate-type = ["cdylib"]
```

### Implementing `lib.rs`

```rust
// Declare the FFI interfaces exposed by the plug host environment
extern "C" {
    /// Retrieve the parameters submitted to the plugin command.
    fn get_args(ptr: *mut u8, max_len: usize);
    
    /// Print message strings to the active console terminal area.
    fn print_info(ptr: *const u8, len: usize);
    
    /// Create a new layout Tab.
    fn main_w_add_tab(ptr: *const u8, len: usize);
    
    /// Re-label the tab owner ID.
    fn host_set_tab_owner(ptr: *const u8, len: usize);
}

/// The main entry point called by the Rust runtime core when the plugin is invoked
#[no_mangle]
pub extern "C" fn run() {
    let mut buf = [0u8; 256];
    unsafe { get_args(buf.as_mut_ptr(), buf.len()); }
    
    let len = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    let args = std::str::from_utf8(&buf[..len]).unwrap_or("").trim();
    
    let output = format!("Hello, you submitted these args: {}", args);
    unsafe { print_info(output.as_ptr(), output.len()); }
}
```

---

## 3. Configuring Permissions (`plugin.toml`)

Before a plugin can invoke certain critical host imports, it must request permissions in `plugin.toml`.

Example unified plugin declaration:

```toml
[[plugin]]
name = "my_plugin"
version = "1.0.0"
author = "Your Name"
api_version = "0.1.0a"
# Request specific capabilities:
permissions = [
  "host_exec",         # Execute arbitrary shell commands (e.g. CMD/PowerShell)
  "main_w_add_tab",    # Open new tabs
  "host_set_tab_owner" # Take ownership of inputs inside tabs
]
```

If a plugin attempts to make a restricted system call (like `host_exec`) without registering it in `permissions`, the Wasmer runtime will abort the call and log a `[SECURITY] Plugin attempted to call host_exec without permission` error.

---

## 4. Compiling the WASM Binary

Use `rustc` or `cargo` to compile targeting WASM:

```bash
# Add the target if you haven't already
rustup target add wasm32-unknown-unknown

# Build using cargo
cargo build --target wasm32-unknown-unknown --release
```

To optimize the WASM binary for minimal size (essential for fast load times), use `rustc` directly with size optimizations:

```bash
rustc --target wasm32-unknown-unknown \
      --crate-type cdylib src/lib.rs \
      -o plugins/my_plugin/my_plugin.wasm \
      -C opt-level=z \
      -C lto=true \
      -C strip=symbols
```
