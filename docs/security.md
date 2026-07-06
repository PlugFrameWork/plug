# Security Architecture Policy

This document outlines the threat model, mitigation strategies, and security policies governing the **plug** WebAssembly plugin framework.

---

## 1. Threat Model & Sandbox Isolation

The plug architecture enforces a strict boundary between untrusted user-developed plugins and the host operating system.

```
                  +--------------------------------+
                  |      WASM Sandbox (Wasmer)     |
                  |  Memory-Isolated Plugin Code   |
                  +---------------+----------------+
                                  |
                                  | FFI Capability Requests
                                  v
                  +---------------+----------------+
                  |       Host FFI Gatekeeper      |
                  | Checks manifest (plugin.toml)  |
                  +---------------+----------------+
                                  |
                                  | Sanitized Requests
                                  v
                  +---------------+----------------+
                  |         Host OS Kernel         |
                  +--------------------------------+
```

### 1.1 Memory Isolation
Plugins are executed inside a sandboxed environment managed by the **Wasmer** compilation engine. This guarantees memory-space isolation, preventing untrusted WebAssembly code from directly accessing or mutating host system memory.

### 1.2 FFI Capability Gatekeeper
Plugins do not possess native OS execution rights. Any interaction with the host (e.g., UI tab creation, command execution, or networking) must be explicitly requested in the plugin's `plugin.toml` manifest permissions array (e.g., `permissions = ["host_exec"]`). The host runtime rejects any unauthorized FFI import invocation at compile/load time.

### 1.3 Subprocess Execution Constraints (`host_exec`)
Native command execution requested via `host_exec` by untrusted plugins is heavily constrained:
- Commands must be resolved against a canonical absolute path allowlist.
- Arguments are validated against restrictive regular expressions to block shell command injections.
- Standard shell command interpreters (such as `cmd.exe`, `powershell.exe`, `/bin/sh`) are strictly prohibited for untrusted tiers.

---

## 2. Cryptographic Integrity Validation & TOCTOU Mitigations

To prevent filesystem manipulation, Man-in-the-Middle (MITM) hijacking, and Time-of-Check to Time-of-Use (TOCTOU) attacks, we implement the following cryptographic checks.

### 2.1 Single-Read Load-Time Verification
When loading a plugin, the host runtime performs a single read operation (`fs::read`) into a memory buffer.
1. The SHA-256 hash of this memory buffer is calculated.
2. The hash is validated against the global `TRUSTED_PLUGIN_HASHES` array and the local `.integrity` sidecar file.
3. The verified memory buffer is compiled directly. This ensures the file cannot be replaced on disk between validation and execution.

### 2.2 Atomic Sidecar Writing
To prevent partial or interrupted filesystem writes that could leave the plugin directory in a corrupted or insecure state:
1. WASM payload and `.integrity` hash strings are written to temporary files on the same filesystem partition.
2. The runtime calls atomic renaming (`fs::rename`) to update the files in place, ensuring writes are fully complete or not written at all.

### 2.3 Registry Hash Pinning (MITM & Downgrade Prevention)
Online plugin installation requires SHA-256 hash matching:
- The registry schema `pluglists.json` includes a `"sha256"` field mapping the release binary.
- During `/plug [hash]` execution, the client matches the SHA-256 of the downloaded bytes against the registry pin hash.
- **Fail-Closed Policy**: If the registry lacks a `"sha256"` field, or if the downloaded hash is mismatched, the installation fails immediately with `[SECURITY] Missing pinned hash in registry` to prevent downgrade attacks.

---

## 3. Automated Trusted Tier Routing

For system-level utility plugins (such as `pTerm`) requiring access to native shells, plug implements a cryptographically managed trust routing system.

### 3.1 Hardcoded Compile-Time Verification
- During compilation, the builder script computes the SHA-256 of the official `pTerm.wasm` binary and writes it to `rt/pterm_hash.txt`.
- The Rust core loads this hash at compile time:
  ```rust
  static TRUSTED_PLUGIN_HASHES: &[&str] = &[
      include_str!("../../pterm_hash.txt")
  ];
  ```
- Plugins matching this hash are loaded with `is_trusted = true` and can bypass execution checks to run shell command emulators.
- Any tampering with `pTerm.wasm` breaks the hash match, forcing it to fall back to untrusted validation rules, where it fails-closed and is blocked.

### 3.2 Build-Order Guard
To prevent out-of-order build issues where a stale or missing `pterm_hash.txt` is compiled into the binary, the builder script records compilation timestamps and halts if `pterm_hash.txt` is missing or has not been freshly updated in the current build sequence.

---

## 4. Vulnerability Disclosure & Audits

### 4.1 Dependency Audit Gate
To secure the supply chain, a static analysis gate is run in the CI pipeline using `cargo-audit`. It enforces a strict `--deny warnings` compile policy.

### 4.2 Remediation
- **rustls-webpki**: Remediated 3 vulnerabilities (RUSTSEC-2026-0098, RUSTSEC-2026-0099, RUSTSEC-2026-0104) by upgrading `rustls-webpki` to `0.103.13`.
- **RUSTSEC-2026-0186 (memmap2)**: Confirmed unsoundness warning in `advise_range`/`flush_range` is inactive. The plug runtime only utilizes `Mmap::map` for compiling bytecode, and does not invoke the unsound range management APIs, reducing practical security risk to zero.
