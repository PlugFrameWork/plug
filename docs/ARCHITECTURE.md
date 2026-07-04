# Architecture Design

This document details the high-level architecture, subsystem divisions, and FFI boundaries of the **plug** framework.

---

## High-Level Architecture

The plug framework is divided into two primary process layers:

1. **The Native UI Host (C++)**: Handled via Win32 APIs (Windows) or GTK4 (Linux). Renders active tab controls, caret animations, custom scrollbars, and user prompts.
2. **The Security Core & Command Processor (Rust)**: Manages WASM module compilation, permission allocations, command routing, and plugin execution sandboxing.

```
+--------------------------------------------------------+
|                      C++ Host UI                       |
|  [Main Window] [Tab Control] [Caret/Scrollbar/Visuals]  |
+---------------------------+----------------------------+
                            | FFI (C-linkage)
                            v
+--------------------------------------------------------+
|                     Rust Core Runtime                  |
|  - Command Router (/plug, /tab, /cmd, /ps)             |
|  - Session registry & plugins hash validator           |
+---------------------------+----------------------------+
                            | Wasmer Instance API
                            v
+--------------------------------------------------------+
|                     Wasmer 4.3 Engine                  |
|  - Cranelift JIT compilation                           |
|  - Sandbox with limited host call imports              |
+--------------------------------------------------------+
```

---

## 1. Native UI Subsystem (C++)

The front-end is written in C++17. It manages native windows and rendering loops:

- **Windows Subsystem (`src/windows/*`)**: Initializes custom Win32 controls. Custom widgets (Scrollbar, Caret, Label, Prompt) are custom-painted using standard GDI. Text selection ranges and copy/paste interactions hook directly into the Win32 clipboard APIs.
- **Linux Subsystem (`src/linux/*`)**: Implements matching components utilizing GTK4 and Cairo context drawing hooks.
- **Main Loop**: Evaluates keyboard shortcuts and pipes typed lines into the command parser via `c_parse()`.

---

## 2. FFI Boundary (`cmn/inc/sys/c.h`)

All communication between C++ and Rust passes through clean C-linkage FFI wrappers:

### Rust exports (called by C++):
- `c_init()`: Configures global states, sets up paths, and pre-loads local manifest registries.
- `c_cleanup()`: De-allocates threads and unloads plugins.
- `c_is_running()`: Returns application state flag.
- `c_parse(*mut c_char)`: Submits keyboard entries for command processing.
- `c_on_tab_close(tab_idx)`: Tells Rust to reclaim resources assigned to a closing tab.

### C++ exports (called by Rust / Plugins):
- `main_w_print_info(*const c_char)` / `main_l_print_info`: Prints text logs to active output areas.
- `main_w_add_tab(*const c_char)`: Spawns a new tab assigned to the caller.
- `main_w_set_tab_owner(...)` / `main_w_get_tab_owner(...)`: Updates tab context identity.
- `main_w_set_prompt_visibility(...)`: Restricts/enables standard inputs.

---

## 3. Rust Core Runtime (`tm_main`)

The Rust library compiles to a `staticlib` which compiles into the native target (e.g. `.lib` or `.a`).

### Commands Routing
The core parses typed input.
- Standard inputs starting with `/` trigger local handlers (e.g., `/tab` -> `main_w_add_tab`, `/plug*` -> fetches manifest lists).
- Raw inputs are dispatched to whichever WASM plugin currently "owns" the active tab (stored in `TAB_CWDS` and `SESSION_PLUGINS`).

### Wasmer Engine Sandboxing
- WASM plugins run in isolated memory environments compile-configured by Cranelift.
- Restricted system permissions (e.g., `host_exec`, `main_w_add_tab`) are checked before FFI callbacks are delegated to the underlying OS.
