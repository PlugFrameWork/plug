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

---

## Security Model & Sandbox Limits

### 1. The FFI Gate Architecture
The WebAssembly sandbox isolates memory space and prevents direct OS API execution. System capabilities are exposed exclusively through imported FFI functions. The core runtime acts as a **binary gate**:
- When a plugin attempts an operation (e.g., `host_exec`), the host runtime checks if the permission is listed in the plugin's manifest.
- **Escape Vector by Design**: If a plugin is granted `host_exec` permission, the WASM boundary is crossed. A plugin with `host_exec` can potentially launch processes and interact with the host OS, making it a critical capability.

### 2. Execution Containment for Untrusted Plugins
For third-party or untrusted plugins that require `host_exec` to run specific external tasks, the host runtime applies strict validation checks:
- **Interpreter / LOLBin Ban**: Common shell binaries (`cmd.exe`, `powershell.exe`, `bash`, `python`) and code-execution LOLBins (`rundll32.exe`, `regsvr32.exe`) are explicitly banned.
- **Canonical-Path Allowlist**: The target binary's canonical path must be explicitly mapped in the plugin manifest's `allowed_commands`.
- **Regex Argument Filtering**: The command line arguments are evaluated against a linear-time regex pattern declared in the manifest.

### 3. Trust Tiers & Shell Emulator Bypass
Core system utilities (like the shell terminal emulator `pTerm`) require execution of host shells to operate:
- **Compile-Time Hash Pinning**: During compilation, the builder script computes the SHA-256 hash of the compiled `pTerm.wasm` binary and embeds it directly into the host Rust library (`plugin_mgr.rs` via `pterm_hash.txt`).
- **Trust Bypass**: If a plugin's SHA-256 matches this hardcoded trust list (`TRUSTED_PLUGIN_HASHES`), it is marked as `is_trusted`. Trusted plugins bypass the interpreter bans and allowlist validations inside `host_exec`.
- **Tamper Protection**: If a trusted plugin is modified, its hash changes. It falls back to the standard flow, requiring an `.integrity` file, and is blocked if tampered.

### 4. Cryptographic Integrity Validation
- **Single-Read Verification**: To mitigate TOCTOU (Time-of-Check to Time-of-Use) filesystem attacks, plugin binaries are read into memory in a single syscall. The runtime computes the SHA-256 hash of this memory buffer and verifies it against the sidecar before Wasmer compiles it.
- **Atomic Sidecar Writes**: Plugin installations write both the WASM binary and `.integrity` file to temporary files, followed by an atomic rename to prevent corrupted states.
- **Registry Hash Pinning**: Remote plugin downloads are validated against SHA-256 hashes pinned in the official registry (`pluglists.json`) to prevent MITM attacks.
- **Global Migration**: Pre-existing plugins are backfilled with `.integrity` files on startup, and a global marker is written. Post-migration, any loaded plugin lacking a valid `.integrity` sidecar is blocked.

### 5. Upstream Dependency Security Notes
- **Wasmer & memmap2 Soundness (`RUSTSEC-2026-0186`)**:
  - The unsoundness vulnerability in `memmap2` (`RUSTSEC-2026-0186` for versions `< 0.9.11`) is currently ignored in [.cargo/audit.toml](file:///O:/prj/p01/.wip/plug/plug.app/rt/.cargo/audit.toml) because our current Wasmer version does not invoke the unsound functions (`advise_range` or `flush_range`).
  - **Upgrade Checklist**: Every time `wasmer` is upgraded, the developers must audit if the new Wasmer version invokes these `memmap2` range advising/flushing routines. If it does, `memmap2` must be upgraded to a safe patched version (e.g. `>= 0.9.11`) or Wasmer must be configured to bypass those calls to maintain full system safety.
