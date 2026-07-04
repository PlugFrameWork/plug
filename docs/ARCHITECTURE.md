# Architecture

## Subsystem Layers

The framework splits operations into two layers connected via C FFI:
- **UI Host (C++17)**: Manages native window loops (Win32 GDI on Windows, GTK4/Cairo on Linux). Renders UI elements (carets, scrollbars, text selection) and forwards keyboard input to the core.
- **Core Runtime (Rust)**: Instantiates Wasmer 4.3 (Cranelift compilation), processes shell commands, maps active tab contexts, and enforces sandboxing.

---

## FFI Boundary (`cmn/inc/sys/c.h`)

### Rust Functions (Called by C++)
- `c_init()` / `c_cleanup()`: Lifecycle initialization and shutdown.
- `c_is_running()`: Returns runtime status.
- `c_parse(input_str)`: Parses and executes keyboard inputs.
- `c_on_tab_close(idx)`: Unloads plugins running inside the closed tab.

### C++ Functions (Called by Rust / Plugins)
- `main_*_print_info(str)` / `main_*_print_error(str)`: Output logs to active text buffers.
- `main_*_add_tab(name)`: Spawns a new tab context.
- `main_*_set_prompt_visibility(visible)`: Controls standard input prompt fields.
- `main_*_get_tab_owner(idx)` / `main_*_set_tab_owner(idx, name)`: Syncs tab ownership state.

---

## Command Routing & Permissions

Inputs starting with `/` invoke built-in commands (e.g., `/tab` or `/plug`). Other inputs are dispatched to the WASM plugin owning the active tab.

If a plugin requests access to sensitive FFI imports (such as `host_exec` to run local shell subprocesses), the runtime validates its manifest permissions list (`plugin.toml`) before delegating the call to the host OS.
