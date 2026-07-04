import os
import sys
import json
import subprocess
import shutil

SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))
CROSS_DIR = os.path.dirname(SCRIPT_DIR)
ROOT_DIR = os.path.dirname(os.path.dirname(CROSS_DIR))
RUST_APP_DIR = os.path.join(ROOT_DIR, "plug.app", "rt")

def load_config():
    config_path = os.path.join(CROSS_DIR, "config", "toolchains.json")
    with open(config_path, "r") as f:
        return json.load(f)

def run_command(cmd, cwd, env=None):
    try:
        subprocess.run(cmd, cwd=cwd, env=env, check=True)
    except subprocess.CalledProcessError as e:
        print(f"[ERROR] Command failed with exit code {e.returncode}")
        sys.exit(1)

def build(toolchain_name=None):
    config = load_config()
    
    if toolchain_name is None:
        toolchain_name = config.get("active_toolchain", "gcc-x64")
        
    toolchains = config.get("toolchains", {})
    if toolchain_name not in toolchains:
        print(f"[ERROR] Toolchain '{toolchain_name}' not found.")
        sys.exit(1)
        
    tc = toolchains[toolchain_name]
    tools = config.get("tools", {})
    build_options = config.get("build_options", {})
    
    arch = tc.get("arch", "x64")
    
    release_dir = os.path.join(ROOT_DIR, f"release_linux_{arch}")
    build_dir = os.path.join(CROSS_DIR, "build", toolchain_name)
    
    os.makedirs(release_dir, exist_ok=True)
    os.makedirs(build_dir, exist_ok=True)
    
    cargo_cmd = tools.get("cargo", "cargo")
    cmake_cmd_bin = tools.get("cmake", "cmake")
    ninja_cmd = tools.get("ninja", "ninja")
    upx_cmd = tools.get("upx", "upx")
    
    env = os.environ.copy()
    
    # Tự động bổ sung ~/.cargo/bin vào PATH nếu chưa có (Phổ biến trên Linux/rustup)
    cargo_home_bin = os.path.expanduser("~/.cargo/bin")
    if os.path.exists(cargo_home_bin) and cargo_home_bin not in env.get("PATH", ""):
        env["PATH"] = f"{cargo_home_bin}:{env.get('PATH', '')}"

    # Check and set C/C++ compilers
    if tc.get("c_compiler"):
        resolved_cc = shutil.which(tc["c_compiler"], path=env.get("PATH"))
        if not resolved_cc:
            print(f"[ERROR] C compiler '{tc['c_compiler']}' is not found. Please install build-essential or gcc/clang.")
            sys.exit(1)
        env["CC"] = resolved_cc
        
        # Ép Rust sử dụng đúng linker này thay vì dùng mặc định 'cc'
        rust_target_env = tc["rust_target"].upper().replace("-", "_")
        env[f"CARGO_TARGET_{rust_target_env}_LINKER"] = resolved_cc
        
    if tc.get("cxx_compiler"):
        resolved_cxx = shutil.which(tc["cxx_compiler"], path=env.get("PATH"))
        if not resolved_cxx:
            print(f"[ERROR] C++ compiler '{tc['cxx_compiler']}' is not found. Please install g++ or clang++.")
            sys.exit(1)
        env["CXX"] = resolved_cxx
    
    # Resolve the absolute path of cargo to avoid FileNotFoundError
    resolved_cargo = shutil.which(cargo_cmd, path=env.get("PATH"))
    if not resolved_cargo:
        print("[ERROR] 'cargo' is not found. Please install Rust (https://rustup.rs/)")
        sys.exit(1)

    # 1. Build Rust
    print(f"\n[1] Building Rust Runtime ({toolchain_name})...")
    rust_target = tc["rust_target"]
    run_command([resolved_cargo, "build", "--release", "--target", rust_target], cwd=RUST_APP_DIR, env=env)
    rust_lib_path = os.path.join(RUST_APP_DIR, "target", rust_target, "release", "libtm_main.a")
    
    # 2. Build C/C++ with CMake
    print(f"\n[2] Configuring C/C++ Engine ({toolchain_name})...")
    
    # GTK4 requires pkg-config to resolve library paths
    if not shutil.which("pkg-config", path=env.get("PATH")):
        print("[ERROR] 'pkg-config' is not found. Please install it (e.g. sudo apt install pkg-config libgtk-4-dev).")
        sys.exit(1)

    resolved_cmake = shutil.which(cmake_cmd_bin, path=env.get("PATH"))
    if not resolved_cmake:
        print("[ERROR] 'cmake' is not found. Please install CMake.")
        sys.exit(1)

    cmake_cmd = [
        resolved_cmake, 
        "-G", "Ninja",
        f"-DRUST_LIB_PATH={rust_lib_path}",
        "-DCMAKE_BUILD_TYPE=Release"
    ]
    
    if tc.get("extra_flags"):
        cmake_cmd.append(f"-DCMAKE_C_FLAGS={tc['extra_flags']}")
        cmake_cmd.append(f"-DCMAKE_CXX_FLAGS={tc['extra_flags']}")
        
    if build_options.get("hide_console", False):
        # Console hide on Linux typically means launching detached or avoiding stdout
        pass 
        
    cmake_cmd.append(CROSS_DIR)
    run_command(cmake_cmd, cwd=build_dir, env=env)
    
    print(f"\n[3] Compiling Binary ({toolchain_name})...")
    resolved_ninja = shutil.which(ninja_cmd, path=env.get("PATH"))
    if not resolved_ninja:
        print("[ERROR] 'ninja' is not found. Please install Ninja build system.")
        sys.exit(1)
    run_command([resolved_ninja], cwd=build_dir, env=env)
    
    # 4. Strip & Compress
    exe_name = "plug"
    exe_path = os.path.join(build_dir, exe_name)
    release_exe = os.path.join(release_dir, exe_name)
    
    print("\n[4] Stripping debug symbols...")
    run_command(["strip", exe_path], cwd=build_dir)
    
    if build_options.get("upx_compress", False):
        print("\n[5] Compressing with UPX...")
        resolved_upx = shutil.which(upx_cmd, path=env.get("PATH"))
        if resolved_upx:
            upx_level = build_options.get("upx_level", "--best").split()
            run_command([resolved_upx] + upx_level + [exe_path], cwd=build_dir)
        else:
            print("[WARNING] UPX not found. Skipping compression.")
        
    shutil.copy(exe_path, release_exe)
    print(f"\n[OK] Build completed! Output: {release_exe}")

if __name__ == "__main__":
    if len(sys.argv) >= 2:
        build(sys.argv[1])
    else:
        build()
