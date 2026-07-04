# Compilation and Build Guide

This document describes how to compile the C++ application shell, compile the Rust core runtime library, and compile the default WASM plugins from source on both Windows and Linux.

---

## Toolchain Dependencies

Ensure you have the following prerequisites installed:

- **Rust**: stable toolchain.
- **Python 3**: For build scripts execution.
- **CMake**: version 3.20 or newer.
- **Ninja**: Build engine.
- **C++ Compilers**:
  - **Windows**: MSVC 2022 (Community/Professional/Enterprise) or Clang/LLVM.
  - **Linux**: GCC or Clang.
  - **GTK4 Development Headers** (Linux only): required for window rendering.

---

## Directory Build Map

```
plug/
├── plug.app/
│   └── rt/               <-- Rust Runtime Project (compile this first)
│
├── plug.cross/
│   ├── windows/          <-- Windows build workspace
│   │   ├── build/        <-- Target directory for CMake compiler output
│   │   └── scripts/      <-- builder.py script wrapper
│   │
│   └── linux/            <-- Linux build workspace
│       ├── build/        <-- Target directory for CMake compiler output
│       └── scripts/      <-- builder.py script wrapper
```

---

## Windows Compilation

### Automatic Build via Python Wrapper

The python builder parses `plug.cross/windows/config/toolchains.json` to detect MSVC/LLVM paths, builds the Rust library, builds the WASM plugins, generates resource manifests, and links everything using CMake and Ninja.

1. Navigate to the Windows cross directory:
   ```powershell
   cd plug.cross\windows
   ```
2. Execute the build wrapper script:
   ```powershell
   .\make.bat
   ```
   *Note: This bat file executes `python scripts/builder.py clang-x64` by default.*
3. Find compiled binaries inside:
   `release/x64/plug.exe` and `plugins/pTerm/`

---

## Linux Compilation

### Installing Linux Dependencies

On Debian/Ubuntu:
```bash
sudo apt-get update
sudo apt-get install -y libgtk-4-dev pkg-config ninja-build libx11-dev build-essential
```

### Automatic Build via Python Wrapper

1. Navigate to the Linux cross directory:
   ```bash
   cd plug.cross/linux
   ```
2. Execute the shell build script:
   ```bash
   chmod +x make.sh
   ./make.sh
   ```
   *Note: This shell file executes `python3 scripts/builder.py gcc-x64` by default.*
3. Find compiled binaries inside:
   `release_linux_x64/plug` and `plugins/pTerm/`

---

## Manual Step-by-Step Compilation

If you prefer to compile manually without the Python scripts:

### Step 1: Compile the Rust Core Runtime
Compile the Rust project in release mode:
```bash
cd plug.app/rt
cargo build --release --lib
```
This generates the static library file:
- Windows: `plug.app/rt/target/release/tm_main.lib`
- Linux: `plug.app/rt/target/release/libtm_main.a`

### Step 2: Compile WASM Plugins
Compile plugins individually using `rustc` targeting WASM:
```bash
rustc --target wasm32-unknown-unknown --crate-type cdylib \
      plug.app/rt/src_plugins/pTerm/pTerm.rs \
      -o plugins/pTerm.wasm \
      -C opt-level=z -C lto=true
```

### Step 3: Run CMake Build
Point CMake to the compiled Rust static library and build:

#### Windows (MSVC developer console / CMD)
```cmd
cd plug.cross\windows
mkdir build
cd build
cmake -G Ninja -DRUST_LIB_PATH=../../../plug.app/rt/target/release/tm_main.lib -DCMAKE_BUILD_TYPE=Release ..
ninja
```

#### Linux
```bash
cd plug.cross/linux
mkdir build
cd build
cmake -G Ninja -DRUST_LIB_PATH=../../../plug.app/rt/target/release/libtm_main.a -DCMAKE_BUILD_TYPE=Release ..
ninja
```
