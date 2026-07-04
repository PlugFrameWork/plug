# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [1.0.1a] - 2026-07-04

This is the initial release of the **plug** framework, establishing a robust multitab desktop UI shell linked with a sandboxed WebAssembly execution engine.

### Added
- **Hybrid Native/FFI Interface**: C++ application shell linked to a Rust runtime staticlib (`tm_main`).
- **WASM Plugin Runtime**: Built-in integration with the Wasmer 4.3 runtime using the Cranelift compiler.
- **Multitab Layout Subsystem**: High-fidelity terminal emulator window manager supporting multiple tab contexts, carets, scrollbars, text highlighting, clipboard manipulation, and custom hover transitions.
- **Unified Plugins Manifest**: Added `plugin.toml` tracking configurations, authors, and security permissions.
- **`pTerm` Plugin**: Compiled terminal tool supporting shell delegation (CMD and PowerShell) inside sandboxed tabs.
- **CLI Commands Support**: Added command parser supporting:
  - `/a` (about/system status)
  - `/?` (help menu)
  - `/e` (application exit)
  - `/tab` (new tab creation)
  - `/plug*` (official plugin repository listing)
  - `/plug` (manual plugin installation)
  - `/plug-` (plugin removal)
- **CI/CD Workflows**: Added test runners, document formatting validators, and release builder templates.
- **Comprehensive Docs Suite**: Added Architecture, Build, and Plugin Development specifications.
