# Official Plugins Catalog

This directory contains the index and detailed documentation for the official plugins built into the **plug** ecosystem.

## Plugin Index

Currently, the following official plugins are deployed and verified:

| Plugin Name | Version | Description | Documentation Link |
|---|---|---|---|
| **pTerm** | `1.0.0` | Default core terminal runner plugin bridging WASM container to host shells. | [pTerm Details](catalog/pTerm.md) |

## Core Integration Architecture

System plugins are compiled into WebAssembly binaries (`wasm32-unknown-unknown`) and reside in the `plugins/` directory.

Security and privilege routing is governed by:
1. **FFI Capability Gate**: Access to system resources (like spawning processes) requires requesting permissions (e.g. `host_exec`) in the plugin's `plugin.toml`.
2. **Cryptographic Integrity Validation**: Loaded plugins must match their `.integrity` sidecar SHA-256 value.
3. **Automated Trust Tier Bypass**: Core plugins like `pTerm` bypass sandbox interpreter restrictions if their SHA-256 hash matches the compile-time hardcoded `TRUSTED_PLUGIN_HASHES` array.
