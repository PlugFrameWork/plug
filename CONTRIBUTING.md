# Contributing to plug

We welcome contributions to the plug framework! Whether you are fixing a bug, improving the docs, or adding a new feature, this guide will help you get started.

---

## Code of Conduct

By participating in this project, you agree to abide by our [Code of Conduct](CODE_OF_CONDUCT.md).

---

## Development Workflow

### 1. Fork and Clone
1. Fork the [plug repository](https://github.com/PlugFrameWork/plug) on GitHub.
2. Clone your fork locally:
   ```bash
   git clone https://github.com/your-username/plug.git
   cd plug
   ```

### 2. Set Up Your Environment
Follow the instructions in the [Build Guide](docs/BUILD.md) to set up Rust, CMake, Ninja, and your C++ compiler.

### 3. Create a Branch
Create a branch for your changes:
```bash
git checkout -b feature/your-feature-name
# or for bug fixes:
git checkout -b fix/your-bug-name
```

### 4. Implement Your Changes
- Ensure code adheres to our style guidelines:
  - **C++**: PascalCase for classes, camelCase for functions, and prefix variables with appropriate context (`g_` for globals, `m_` for members). Keep include directives clean.
  - **Rust**: Follow standard Rust formatting (`rustfmt`) and design guidelines.
  - **Python**: Adhere to PEP 8 standards.
- Run code formatting tools prior to committing:
  ```bash
  cargo fmt --all --manifest-path plug.app/rt/Cargo.toml
  ```

### 5. Add and Run Tests
- Every bug fix or feature should include tests.
- Run all test suites before submitting a PR:
  ```bash
  python tests/run_tests.py
  ```

### 6. Commit and Push
- Write clear, descriptive commit messages. We prefer conventional commits (e.g. `feat: add new CLI command`, `fix: resolve memory leak in Win32 scrollbar`).
- Push the branch to your fork:
  ```bash
  git push origin feature/your-feature-name
  ```

### 7. Open a Pull Request
1. Open a Pull Request on the upstream repository targeting the main development branch.
2. Fill out the [Pull Request Template](.github/PULL_REQUEST_TEMPLATE.md) completely.
3. Keep your PR focused on one specific change. If you have multiple unrelated changes, open separate PRs.
