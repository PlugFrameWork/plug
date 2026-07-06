# Plug: A WebAssembly Plugin Framework

<p align="left">
  <img src="plug.res/ast/ico/p-1.png" alt="logo" width="500"/>
</p>

<p align="center">
  <a href="https://github.com/PlugFrameWork/plug/actions/workflows/ci.yml"><img src="https://github.com/PlugFrameWork/plug/actions/workflows/ci.yml/badge.svg" alt="CI Build" /></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/License-MIT-blue.svg" alt="License: MIT" /></a>
</p>

## Features

- **Sandboxed WASM Runtime**: Isolated memory execution using the Wasmer compiler. Note that the sandbox confines the plugin's memory space and relies on a host FFI permission gate for system capabilities.
- **Granular Permissions Model**: Checks plugin FFI import access (e.g., tab management or host execution) against manifest declarations (`plugin.toml`).
- **Multitab Desktop Shell**: Launch and run multiple independent plugins concurrently in separate workspace tabs.
- **Cross-Platform Native UI**: Compiles to Windows (Win32 GDI) and Linux (GTK4) with zero browser engine footprint.

---

## Overview

```mermaid
graph TD
    A[C++ UI Subsystem] -- Input Commands --> B(FFI Interface)
    B -- Parse / Process --> C[Rust Runtime Core]
    C -- Load / Run --> D[Wasmer Sandbox Engine]
    D -- Host Imports / Callbacks --> C
    C -- Control UI --> B
    B -- UI Updates --> A
```

---

## Getting Started

Refer to [docs/BUILD.md](docs/BUILD.md) for full toolchain setup.

### Quick Build
```bash
# Windows
cd plug.cross\windows && .\make.bat

# Linux
cd plug.cross/linux && ./make.sh
```

---

## Console Commands

| Command | Args | Description |
|---|---|---|
| `/?` | None | Displays help details. |
| `/a` | None | Displays system environment and memory statistics. |
| `/tab` | None | Spawns a new workspace tab. |
| `/plug*` | None | Lists available online registry plugins and hashes. |
| `/plug` | `[hash]` | Downloads and initializes a plugin. |
| `/plug-` | `[hash]` | Unloads the specified plugin. |
| `/e` | None | Exits the application. |

---

## Plugin Development

Plugins are compiled targeting `wasm32-unknown-unknown`.

```rust
extern "C" {
    fn get_args(ptr: *mut u8, max_len: usize);
    fn print_info(ptr: *const u8, len: usize);
}

#[no_mangle]
pub extern "C" fn run() {
    let mut buf = [0u8; 128];
    unsafe { get_args(buf.as_mut_ptr(), buf.len()); }
    let msg = "Hello from WASM plugin!";
    unsafe { print_info(msg.as_ptr(), msg.len()); }
}
```

Refer to [docs/PLUGIN_DEVELOPMENT.md](docs/PLUGIN_DEVELOPMENT.md) for details on permissions configuration (`plugin.toml`) and FFI imports.
