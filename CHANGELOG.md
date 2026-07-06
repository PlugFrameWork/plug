# Changelog

---

## [0.1.2a] - 2026-07-06

### Added
* Shared WASM compilation module `tests/build_utils.py` to enforce allow-undefined flags globally.
* Automated target binary copying and caching for Linux production builds.
* Dependabot integration under `.github/dependabot.yml` tracking Rust cargo packages and GitHub Actions.
* Custom structured issue templates (Bug Report, Feature Request, Plugin Submission) and a Pull Request template in `.github/`.

### Fixed
* Stale cache build errors by unlinking old host binaries if `pterm_hash.txt` has changed.
* Registry trusted validation check by trimming white-spaces inside the compile-time `TRUSTED_PLUGIN_HASHES` comparison array.
* Error reporting inside `verify_production_binary_clean` to distinguish missing binaries from symbol leaks.

---

## [0.1.1a] - 2026-07-05

### Added
* End-to-end interactive mock HTTP server registry download tests (Test Case 8, 9, and 10) in `test_sandbox_rules.py`.

### Fixed
* MSVC Windows entrypoint linker failures (`WinMain` unresolved) in `CMakeLists.txt` using `/ENTRY:mainCRTStartup`.
* Linux CI Rust library detection failures by introducing auto-scanning inside the orchestrator.
* GHA Windows runners multi-line backslash parsing errors.
* GHA `cargo-audit` workflow check permissions and updated deprecated Node configurations.

---

## [0.1.0a] - 2026-07-04

### Added
* **Native/FFI Interface**: C++ application shell linked to a Rust runtime staticlib (`tm_main`).
* **WASM Plugin Runtime**: Built-in integration with the Wasmer 4.3 runtime using the Cranelift compiler.
* **Multitab Layout Subsystem**: High-fidelity terminal emulator window manager supporting multiple tab contexts, carets, scrollbars, text highlighting, clipboard manipulation, and custom hover transitions.
* **Unified Plugins Manifest**: Added `plugin.toml` tracking configurations, authors, and security permissions.
* **`pTerm` Plugin**: Compiled terminal tool supporting shell delegation (CMD and PowerShell) inside sandboxed tabs.
* **CLI Commands Support**: Added command parser supporting `/a`, `/?`, `/e`, `/tab`, `/plug*`, `/plug`, and `/plug-`.
* **CI/CD Workflows**: Added test runners, document formatting validators, and release builder templates.
* **Comprehensive Docs Suite**: Added Architecture, Build, and Plugin Development specifications.
