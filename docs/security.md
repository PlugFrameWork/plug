# Security Model & Threat Analysis

## Threat Model

### Assets
- Host filesystem (read/write/execute)
- Host network (internal/external)
- Host process execution
- Plugin integrity (supply chain)
- User data in plugin tabs

### Actors
- **Malicious plugin author**: Publishes plugin to registry
- **Compromised registry**: GitHub repo / CDN hijacked
- **Local attacker**: Code execution on host machine
- **Network attacker**: MITM on plugin download

### Trust Boundaries
```
┌─────────────────────────────────────────────────────┐
│                   HOST OS                            │
│  ┌─────────────────────────────────────────────┐    │
│  │           PLUG RUNTIME (Rust)                │    │
│  │  ┌─────────────────────────────────────┐    │    │
│  │  │      │  │      WASM SANDBOX (Wasmer)           │    │    │
│ │  │  - Linear memory (isolated)          │    │    │
│ │  │  - No direct syscalls                │    │    │
│ │  │  - Host imports ONLY via FFI gate    │    │    │
│      └─────────────────────────────────────┘    │
│  │  ┌─────────────────────────────────────┐    │    │
│  │  │     PERMISSION GATE                  │    │    │
│  │  │  - Load-time import validation       │    │    │
│  │  │  - Call-time runtime checks          │    │    │
│  │  │  - WASI allowlist enforcement        │    │    │
│  │  └─────────────────────────────────────┘    │
│  └─────────────────────────────────────────────┘
└─────────────────────────────────────────────────────┘
```

## Security Controls

### 1. WASM Sandbox (Wasmer 4.3 Cranelift)
- Linear memory isolation (no host pointer access)
- No direct syscall instruction execution
- All host interaction via explicit FFI imports

### 2. Import Validation (Load Time)
**Env namespace** (`env.*`):
- Every import checked against manifest `permissions[]`
- Missing permission → load failure
- Imports: `host_exec`, `host_add_tab`, `host_set_tab_owner`, `host_get_tab_label`, `host_get_platform`, `get_env`, `net_post`, `print_info`, `print_error`, `get_args`

**WASI namespace** (`wasi_snapshot_preview1.*`):
- **Explicit allowlist only** (see `plugin_mgr.rs:ALLOWED_WASI`)
- Blocked: `path_open`, `path_readlink`, `path_rename`, `path_unlink_file`, `path_create_directory`, `path_remove_directory`, `path_symlink`, `path_link`, `sock_connect`, `sock_bind`, `sock_listen`, `sock_accept`, `proc_raise`, `random_get` (stubbed), etc.
- Allowed: `fd_write`/`fd_read` (stdout/stderr only), `proc_exit`, `clock_time_get`, `args_*`, `environ_*` (stubs), `poll_oneoff`, `sched_yield`, `sock_*` (stubs returning ENOSYS)

### 3. Runtime Gates (Call Time)
Each sensitive import re-checks permission before executing:
```rust
if !env_data.permissions.iter().any(|p| p == "host_exec") {
    print_error("[SECURITY] Plugin attempted to call host_exec without permission");
    return;
}
```

### 4. Command Execution Hardening (`host_exec`)
- **No shell**: Direct `Command::new(exe).args(args)` — no `cmd /c`, `sh -c`
- **Allowlist-only**: Manifest `allowed_commands` with canonical path + args regex
- **Path canonicalization**: `resolve_binary_path()` → `fs::canonicalize()`
- **No blacklist**: Blacklists are bypassable; removed entirely

### 5. Network Hardening (`net_post`)
- HTTPS only (scheme validation via `url::Url`)
- Private IP blocking (RFC1918, RFC3927, RFC6598, loopback, multicast, reserved)
- Hostname blocking: `localhost`, `localhost.localdomain`
- Response size limit: 1 MiB
- Timeout: 30s (configurable via `DEFAULT_TIMEOUT`)

### 6. Filesystem Containment
- `cd` command: `canonicalize()` + prefix check against process CWD
- No WASI `path_*` functions exposed
- Plugin working directory tracked per-tab (`TAB_CWDS`)

### 7. Supply Chain Integrity
- Registry (`pluglists.json`) signed with minisign/Ed25519
- Public key baked into binary (`REGISTRY_PUBKEY`)
- Signature verified before parsing any registry content
- Plugin WASM verified against registry-pinned SHA256
- Atomic write with same-FS verification (`write_atomic`)

### 8. Input Validation
- All FFI string reads bounded by constants:
  - `MAX_FFI_STRING_LEN = 64 KiB`
  - `MAX_URL_LEN = 2 KiB`
  - `MAX_JSON_PAYLOAD_LEN = 16 KiB`
  - `MAX_RESPONSE_BUF_LEN = 1 MiB`
  - `MAX_TAB_LABEL_LEN = 256 B`
- Prevents OOB reads and allocation DoS

## Known Limitations / Residual Risk

| Risk | Mitigation | Residual |
|------|-----------|----------|
| WASI stubs return ENOSYS | Plugins expecting real syscalls fail gracefully | Low (breaks compat, not security) |
| Allowlist regex ReDoS | `regex` crate is linear-time (no backtracking) | Low |
| Registry key rotation | Not implemented; requires binary rebuild | Medium |
| Side-channel via `host_get_platform` | Now permission-gated | Low |
| TOCTOU in `write_atomic` cross-FS | Same-FS check + randomized temp name | Low |
| Malicious plugin DoS (infinite loop) | No fuel metering / epoch interruption | Medium |
| Memory exhaustion via large allocations | `MAX_FFI_STRING_LEN` bounds; Wasmer memory limit not set | Medium |

## Security Checklist for Plugin Review

- [ ] Manifest declares minimal permissions
- [ ] `allowed_commands` uses canonical paths, restrictive regex
- [ ] No WASI imports beyond allowlist (verify with `wasm-objdump -x plugin.wasm | grep wasi_snapshot_preview1`)
- [ ] `net_post` URLs are HTTPS, external domains only
- [ ] Plugin does not attempt `cd` traversal
- [ ] SHA256 in registry matches published WASM

## Incident Response

1. **Malicious plugin detected**: Revoke registry entry, rotate minisign key, rebuild host
2. **Registry compromise**: Rotate minisign key immediately, audit all plugins
3. **Sandbox escape**: Isolate host, analyze WASM module, patch Wasmer/import gate
