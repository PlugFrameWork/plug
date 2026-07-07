# Plugin Development

Plugins are WebAssembly (`wasm32-unknown-unknown`) binaries.

## 1. Rust Template

Create a library project (`cargo new my_plugin --lib`) with `crate-type = ["cdylib"]` in `Cargo.toml`.

### `src/lib.rs`
```rust
extern "C" {
    fn get_args(ptr: *mut u8, max_len: usize);
    fn print_info(ptr: *const u8, len: usize);
}

#[no_mangle]
pub extern "C" fn run() {
    let mut buf = [0u8; 256];
    unsafe { get_args(buf.as_mut_ptr(), buf.len()); }
    
    let len = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    let args = std::str::from_utf8(&buf[..len]).unwrap_or("").trim();

    let msg = format!("Args received: {}", args);
    unsafe { print_info(msg.as_ptr(), msg.len()); }
}
```

---

## 2. Permissions Manifest (`plugin.toml`)

Permissions must be explicitly requested to call sensitive host FFI imports:

```toml
[[plugin]]
name = "my_plugin"
version = "1.0.0"
author = "Dev"
api_version = "xxx"
permissions = [
  "host_exec",         # Spawning local commands (CMD / PowerShell / shell)
  "host_add_tab",      # Spawning new tabs
  "host_set_tab_owner", # Taking tab ownership
  "host_get_tab_label", # Reading active tab label
  "get_env",           # Reading host environment variables
  "net_post"           # HTTP POST requests from plugin runtime
]
```

---

## 3. Compilation

Compile and optimize the WASM binary:

```bash
# Add target
rustup target add wasm32-unknown-unknown

# Compile optimized WASM
rustc --target wasm32-unknown-unknown \
      --crate-type cdylib src/lib.rs \
      -o plugins/my_plugin/my_plugin.wasm \
      -C opt-level=z \
      -C lto=true \
      -C strip=symbols \
      -C link-arg=--allow-undefined
```
