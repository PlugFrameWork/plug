# pTerm (Terminal Interface)

`pTerm` is the default official shell runner plugin. It bridges the WebAssembly container to host shell processors.

---

## 1. Specifications

- **Role**: Spawns and manages CMD and PowerShell sessions inside dedicated UI tabs.
- **Required Permissions**: `host_exec`, `main_w_add_tab`, `host_set_tab_owner`, `host_get_tab_label`.
- **Trust Tier**: Recognized as a trusted core utility by matching the compile-time hardcoded SHA-256 hash list. Bypasses the sandbox interpreter bans (shells block) and canonical-path allowlist checks.

---

## 2. Execution Mechanics

The diagram below illustrates the exact control flow when a user runs a shell command inside a `pTerm`-owned tab:

```
[User Input] (e.g. "dir")
     |
     v
[C++ Host UI] -> Intercepts input, calls FFI `c_parse("dir")`
     |
     v
[Rust Runtime] -> Detects active tab owner is "pTerm", calls WASM `run()` export
     |
     v
[WASM Sandbox (pTerm.wasm)]
  1. Calls `get_args` to retrieve input ("dir")
  2. Resolves active tab mode (PowerShell vs CMD)
  3. Prepend shell execution wrapper (e.g. `cmd /c dir` or `powershell -Command "dir"`)
     |
     v
[Rust Host Runtime] -> 1. Verifies memory buffer SHA-256 matches compile-time TRUSTED_PLUGIN_HASHES
                       2. Sets is_trusted = true, bypassing standard execution sandbox restrictions
     |
     v
[Host OS Subprocess] -> Spawns native shell, pipes output back to C++ UI
```

---

## 3. Operations Routing

### Spawning a new Sub-Shell (`/cmd` or `/ps`)
1. User types `/cmd` or `/ps`.
2. Rust router catches the command and invokes the `pTerm` module with initialization arguments.
3. `pTerm` calls `main_w_add_tab("pTerm")` to request a new UI tab from the host.
4. `pTerm` labels the tab owner name as `"CMD"` or `"PS"` using the `host_set_tab_owner` hook.
5. The tab context is initialized, and the user can now type terminal commands directly.

### Running Subprocess Commands
1. User types `git status` in a tab labeled `"PS"`.
2. The input does not start with `/`, so Rust router resolves the active tab owner (`"pTerm"`) and delegates the call.
3. `pTerm` checks the tab label, matches it to `"PS"`, and reformats the command:
   `powershell -NoProfile -Command "git status"`
4. `pTerm` calls `host_exec` to execute the string on the host OS, streaming the outputs directly to the UI rendering buffers.
