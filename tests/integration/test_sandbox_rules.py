import os
import sys
import shutil
import platform
import subprocess
import json
import hashlib
from pathlib import Path

def setup_plugin_sandbox(plugin_name: str, src_wasm: Path, src_toml: Path, dest_dir: Path):
    import hashlib
    # Ensure dest_dir exists
    dest_dir.mkdir(parents=True, exist_ok=True)
    
    # Hash value for test
    hash_val = f"test_{plugin_name}_hash"
    
    # Target file paths
    hash_file = dest_dir / f"{plugin_name}.hash"
    wasm_file = dest_dir / f"{plugin_name}.{hash_val}"
    toml_file = dest_dir / f"{plugin_name}.toml"
    integrity_file = dest_dir / f"{plugin_name}.integrity"
    
    # Write hash file
    with open(hash_file, "w", encoding="utf-8") as f:
        f.write(hash_val)
        
    # Copy wasm and toml files
    shutil.copy2(src_wasm, wasm_file)
    if src_toml.resolve() != toml_file.resolve():
        shutil.copy2(src_toml, toml_file)

    # Compute SHA-256 and write integrity file
    wasm_bytes = wasm_file.read_bytes()
    sha = hashlib.sha256(wasm_bytes).hexdigest()
    with open(integrity_file, "w", encoding="utf-8") as f:
        f.write(sha)

def main():
    try:
        sys.stdout.reconfigure(encoding='utf-8')
        sys.stderr.reconfigure(encoding='utf-8')
    except AttributeError:
        pass
        
    print("[SANDBOX] Running Sandbox Rules integration tests...")
    
    # 1. Load env context
    env_ctx = {}
    if "ENV_CONTEXT" in os.environ:
        try:
            env_ctx = json.loads(os.environ["ENV_CONTEXT"])
        except Exception:
            pass
            
    project_root = Path(__file__).parent.parent.parent.resolve()
    target_bin = env_ctx.get("target_bin")
    
    if not target_bin or not Path(target_bin).exists():
        system = platform.system()
        tc_cfg = project_root / "plug.cross" / "windows" / "config" / "toolchains.json"
        arch = "x64"
        if tc_cfg.exists():
            try:
                import json as _json
                tc_data = _json.loads(tc_cfg.read_text(encoding="utf-8"))
                active_tc = tc_data.get("active_toolchain", "clang-x64")
                arch = tc_data["toolchains"][active_tc].get("arch", "x64")
            except Exception:
                pass
        binary_name = f"plug-{arch}.exe" if system == "Windows" else f"plug-{arch}"
        release_dir = project_root / "plug.cross" / "release" / (arch if system == "Windows" else ("x86_64" if arch == "x64" else arch))
        target_bin = str((release_dir / binary_name).resolve())
        
    # Determine the plugin directory to copy mock files to
    system = platform.system()
    if system == "Windows":
        sys_drive = os.environ.get("SystemDrive", "C:")
        if not sys_drive.endswith("\\"):
            sys_drive += "\\"
        plug_dir = Path(sys_drive) / ".plug" / "plugins"
    else:
        plug_dir = Path("/.plug/plugins")
        
    # Backup existing plugin folder if it exists
    backup_dir = plug_dir.parent / "plugins_backup"
    if plug_dir.exists():
        print(f"[SANDBOX] Backing up existing plugin folder to {backup_dir}...")
        if backup_dir.exists():
            shutil.rmtree(backup_dir)
        shutil.move(plug_dir, backup_dir)
        
    plug_dir.mkdir(parents=True, exist_ok=True)
    
    # Setup Popen creation flags to run headless
    startupinfo = None
    if system == "Windows":
        startupinfo = subprocess.STARTUPINFO()
        startupinfo.dwFlags |= subprocess.STARTF_USESHOWWINDOW
        creationflags = subprocess.CREATE_NO_WINDOW
    else:
        creationflags = 0

    fixtures_build = project_root / "tests" / "fixtures" / "build"
    manifests_dir = project_root / "tests" / "fixtures" / "manifests"
    
    failed = False
    
    try:
        # TEST CASE 1: ok_plugin should run and add a tab normally (positive baseline)
        print("\n[SANDBOX] Test Case 1: ok_plugin baseline check...")
        shutil.rmtree(plug_dir)
        plug_dir.mkdir()
        
        setup_plugin_sandbox(
            "ok_plugin",
            fixtures_build / "ok_plugin.wasm",
            manifests_dir / "ok_plugin.toml",
            plug_dir
        )
        
        proc = subprocess.Popen(
            [target_bin],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            encoding="utf-8",
            startupinfo=startupinfo,
            creationflags=creationflags,
            env={**os.environ, "PLUG_HEADLESS": "1"}
        )
        import time
        proc.stdin.write("ok_plugin\n")
        proc.stdin.flush()
        time.sleep(1.0)
        proc.stdin.write("/e\n")
        proc.stdin.flush()
        stdout, stderr = proc.communicate(timeout=10)
        
        if proc.returncode != 0:
            print(f"[FAIL] ok_plugin baseline test failed with exit code: {proc.returncode}")
            failed = True
        else:
            print("[PASS] ok_plugin baseline test passed successfully.")

        # TEST CASE 2: rogue_plugin should be rejected at load-time (unauthorized imports)
        print("\n[SANDBOX] Test Case 2: rogue_plugin load-time imports rejection check...")
        shutil.rmtree(plug_dir)
        plug_dir.mkdir()
        
        setup_plugin_sandbox(
            "rogue_plugin",
            fixtures_build / "rogue_plugin.wasm",
            manifests_dir / "rogue_plugin.toml",
            plug_dir
        )
        
        proc = subprocess.Popen(
            [target_bin],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            encoding="utf-8",
            startupinfo=startupinfo,
            creationflags=creationflags,
            env={**os.environ, "PLUG_HEADLESS": "1"}
        )
        stdout, stderr = proc.communicate(input="/e\n", timeout=10)
        
        # Verify stderr contains load-time validation rejection notice
        # (e.g. "[WASM] Failed to load ...: [SECURITY] Unauthorized import: main_w_add_tab")
        if "Unauthorized import" in stderr or "Unauthorized import" in stdout:
            print("[PASS] rogue_plugin load-time imports rejection verified successfully.")
        else:
            print("[FAIL] rogue_plugin load-time rejection was not captured in stderr/stdout!")
            print(f"Stdout:\n{stdout}\nStderr:\n{stderr}")
            failed = True

        # TEST CASE 3: rogue_plugin_runtime should be trapped at runtime (unauthorized call)
        print("\n[SANDBOX] Test Case 3: rogue_plugin_runtime runtime sandbox trap check...")
        shutil.rmtree(plug_dir)
        plug_dir.mkdir()
        
        setup_plugin_sandbox(
            "rogue_plugin_runtime",
            fixtures_build / "rogue_plugin_runtime.wasm",
            manifests_dir / "rogue_plugin_runtime.toml",
            plug_dir
        )
        
        proc = subprocess.Popen(
            [target_bin],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            encoding="utf-8",
            startupinfo=startupinfo,
            creationflags=creationflags,
            env={**os.environ, "PLUG_HEADLESS": "1"}
        )
        proc.stdin.write("rogue_plugin_runtime\n")
        proc.stdin.flush()
        time.sleep(1.0)
        proc.stdin.write("/e\n")
        proc.stdin.flush()
        stdout, stderr = proc.communicate(timeout=10)
        
        # Verify stdout/stderr contains runtime trap errors for both sandbox violations:
        #   - "Blocked execution of unauthorized binary" (path not in allowlist)
        #   - "Blocked execution of banned binary" (LOLBin/interpreter ban, plugin_mgr.rs)
        # NOTE: these string literals are coupled to plugin_mgr.rs log messages.
        # If the messages change there, update this assertion to match — do NOT add
        # OR-fallbacks to accept old strings; force a conscious sync instead.
        has_unauth_block = (
            "Blocked execution of unauthorized binary" in stderr
            or "Blocked execution of unauthorized binary" in stdout
        )
        has_banned_block = (
            "Blocked execution of banned binary" in stderr
            or "Blocked execution of banned binary" in stdout
        )

        if has_unauth_block and has_banned_block:
            print("[PASS] rogue_plugin_runtime runtime trap verified successfully.")
        else:
            print("[FAIL] rogue_plugin_runtime runtime trap was not triggered!")
            print(f"  has_unauth_block={has_unauth_block}, has_banned_block={has_banned_block}")
            print(f"Stdout:\n{stdout}\nStderr:\n{stderr}")
            failed = True

        # TEST CASE 4: Integrity mismatch (modified wasm file bytes after installation)
        print("\n[SANDBOX] Test Case 4: ok_plugin integrity mismatch check...")
        shutil.rmtree(plug_dir)
        plug_dir.mkdir()
        
        setup_plugin_sandbox(
            "ok_plugin",
            fixtures_build / "ok_plugin.wasm",
            manifests_dir / "ok_plugin.toml",
            plug_dir
        )
        
        # Modify ok_plugin wasm bytes to cause mismatch
        wasm_file = list(plug_dir.glob("ok_plugin.test_ok_plugin_hash"))[0]
        with open(wasm_file, "r+b") as f:
            f.seek(0)
            f.write(b"\x00\x00\x00\x00") # corrupt signature
            
        proc = subprocess.Popen(
            [target_bin],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            encoding="utf-8",
            startupinfo=startupinfo,
            creationflags=creationflags,
            env={**os.environ, "PLUG_HEADLESS": "1"}
        )
        stdout, stderr = proc.communicate(input="/e\n", timeout=10)
        if "Integrity mismatch" in stderr or "Integrity mismatch" in stdout:
            print("[PASS] ok_plugin integrity mismatch block verified successfully.")
        else:
            print("[FAIL] ok_plugin integrity mismatch was not caught by host runtime!")
            print(f"Stdout:\n{stdout}\nStderr:\n{stderr}")
            failed = True

        # TEST CASE 5: Migration and post-migration bypass block (Phase 4.3 Case A and B)
        print("\n[SANDBOX] Test Case 5: Global migration and bypass blocking...")
        shutil.rmtree(plug_dir)
        plug_dir.mkdir()
        
        # Determine global migration marker path
        marker_path = plug_dir.parent / ".integrity_migration_v1_done"
        if marker_path.exists():
            marker_path.unlink()
            
        # Set up a plugin WITHOUT .integrity file on disk
        setup_plugin_sandbox(
            "ok_plugin",
            fixtures_build / "ok_plugin.wasm",
            manifests_dir / "ok_plugin.toml",
            plug_dir
        )
        # Delete the .integrity file so we simulate a pre-existing plugin before patch
        integrity_file = plug_dir / "ok_plugin.integrity"
        if integrity_file.exists():
            integrity_file.unlink()
            
        # Run plug. It should perform migration, create the .integrity file, and create the marker.
        proc = subprocess.Popen(
            [target_bin],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            encoding="utf-8",
            startupinfo=startupinfo,
            creationflags=creationflags,
            env={**os.environ, "PLUG_HEADLESS": "1"}
        )
        stdout, stderr = proc.communicate(input="/e\n", timeout=10)
        
        # Verify migration occurred (marker created, integrity file created)
        if marker_path.exists() and integrity_file.exists():
            print("[PASS] Global migration backfilled integrity file and created marker successfully.")
        else:
            print("[FAIL] Global migration did not execute or create expected sidecars!")
            failed = True
            
        # Case B: Now with the marker present, delete the .integrity file and try loading again.
        # This simulates an attacker deleting the integrity sidecar.
        if integrity_file.exists():
            integrity_file.unlink()
            
        proc = subprocess.Popen(
            [target_bin],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            encoding="utf-8",
            startupinfo=startupinfo,
            creationflags=creationflags,
            env={**os.environ, "PLUG_HEADLESS": "1"}
        )
        stdout, stderr = proc.communicate(input="/e\n", timeout=10)
        
        # It must be rejected with "Missing integrity sidecar"
        if "Missing integrity sidecar" in stderr or "Missing integrity sidecar" in stdout:
            print("[PASS] Post-migration bypass block (deleting sidecar) verified successfully.")
        else:
            print("[FAIL] Post-migration missing integrity file was not blocked!")
            print(f"Stdout:\n{stdout}\nStderr:\n{stderr}")
            failed = True

        # TEST CASE 6: Trusted plugin bypass checks (Case C)
        print("\n[SANDBOX] Test Case 6: Trusted plugin bypass integrity file check...")
        shutil.rmtree(plug_dir)
        plug_dir.mkdir()
        
        # Determine global migration marker path
        marker_path = plug_dir.parent / ".integrity_migration_v1_done"
        if not marker_path.exists():
            with open(marker_path, "w") as f:
                f.write("v1_done")
                
        # Find the official pTerm wasm file from the freshly built workspace plugins directory
        pterm_src = list((project_root / "plugins" / "pTerm").glob("pTerm.*"))
        pterm_wasm = [p for p in pterm_src if p.suffix not in (".hash", ".integrity", ".tmp")][0]
        
        # Write pTerm.toml containing permissions = ["host_exec", "main_w_add_tab", "host_set_tab_owner", "host_get_tab_label"]
        pterm_toml_path = plug_dir / "pTerm.toml"
        with open(pterm_toml_path, "w", encoding="utf-8") as f:
            f.write("""[plugin]
name = "pTerm"
version = "1.0.0"
author = "plug"
api_version = "0.1.0a"
permissions = ["host_exec", "main_w_add_tab", "host_set_tab_owner", "host_get_tab_label"]
""")

        # Setup pTerm plugin in sandbox without .integrity file
        setup_plugin_sandbox(
            "pTerm",
            pterm_wasm,
            pterm_toml_path,
            plug_dir
        )
        # Delete pTerm's .integrity file
        pterm_integrity = plug_dir / "pTerm.integrity"
        if pterm_integrity.exists():
            pterm_integrity.unlink()
            
        # Run plug. It should load successfully even without the .integrity file.
        proc = subprocess.Popen(
            [target_bin],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            encoding="utf-8",
            startupinfo=startupinfo,
            creationflags=creationflags,
            env={**os.environ, "PLUG_HEADLESS": "1"}
        )
        
        # Send /cmd dir command to execute via pTerm and verify output
        proc.stdin.write("/cmd dir\n")
        proc.stdin.flush()
        import time
        time.sleep(1.0)
        
        proc.stdin.write("/e\n")
        proc.stdin.flush()
        stdout, stderr = proc.communicate(timeout=10)
        
        # Check that it didn't fail to load pTerm:
        has_load_fail = (
            "Failed to load pTerm" in stderr or "Failed to load pTerm" in stdout or
            "Missing integrity sidecar for pTerm" in stderr or "Missing integrity sidecar for pTerm" in stdout or
            "Invalid manifest format" in stderr or "Missing manifest entry" in stderr
        )
        
        # Check if the execution output contains typical repository files/folders (dir command results)
        has_execution_output = "test_sandbox_rules.py" in stdout or "Cargo.toml" in stdout or "plugins" in stdout or "plug.app" in stdout
        
        if not has_load_fail and has_execution_output:
            print("[PASS] Trusted plugin successfully bypassed integrity sidecar check and executed host command.")
        else:
            print("[FAIL] Trusted plugin execution or load bypass failed!")
            print(f"Stdout:\n{stdout}\nStderr:\n{stderr}")
            failed = True

        # TEST CASE 7: Tampered trusted plugin block (Case D)
        print("\n[SANDBOX] Test Case 7: Tampered trusted plugin block check...")
        shutil.rmtree(plug_dir)
        plug_dir.mkdir()
        
        # With the global marker present
        if not marker_path.exists():
            with open(marker_path, "w") as f:
                f.write("v1_done")
                
        # Write pTerm.toml containing permissions = ["host_exec", "main_w_add_tab", "host_set_tab_owner", "host_get_tab_label"]
        pterm_toml_path = plug_dir / "pTerm.toml"
        with open(pterm_toml_path, "w", encoding="utf-8") as f:
            f.write("""[plugin]
name = "pTerm"
version = "1.0.0"
author = "plug"
api_version = "0.1.0a"
permissions = ["host_exec", "main_w_add_tab", "host_set_tab_owner", "host_get_tab_label"]
""")

        # Setup pTerm in sandbox but using corrupted/tampered bytes (using ok_plugin.wasm)
        setup_plugin_sandbox(
            "pTerm",
            fixtures_build / "ok_plugin.wasm", # mismatched hash
            pterm_toml_path,
            plug_dir
        )
        # Delete pTerm's .integrity file
        pterm_integrity = plug_dir / "pTerm.integrity"
        if pterm_integrity.exists():
            pterm_integrity.unlink()
            
        # Run plug. It should fail-closed (blocked completely).
        proc = subprocess.Popen(
            [target_bin],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            encoding="utf-8",
            startupinfo=startupinfo,
            creationflags=creationflags,
            env={**os.environ, "PLUG_HEADLESS": "1"}
        )
        stdout, stderr = proc.communicate(input="/e\n", timeout=10)
        
        # Verify it is blocked with "Missing integrity sidecar"
        if "Missing integrity sidecar for pTerm" in stderr or "Missing integrity sidecar for pTerm" in stdout:
            print("[PASS] Tampered trusted plugin failed-closed (blocked) successfully.")
        else:
            print("[FAIL] Tampered trusted plugin was not blocked (allowed to load)!")
            print(f"Stdout:\n{stdout}\nStderr:\n{stderr}")
            failed = True

        # TEST CASE 8/9/10: E2E Registry download flow and downgrade attack checks
        print("\n[SANDBOX] Test Cases 8, 9, 10: E2E Registry download & security checks...")
        import http.server
        import socketserver
        import threading
        
        # We need a directory to serve files from
        mock_web_dir = Path("tests/.artifacts/mock_web")
        if mock_web_dir.exists():
            shutil.rmtree(mock_web_dir)
        mock_web_dir.mkdir(parents=True)
        
        # Define the handler serving from mock_web_dir
        class QuietHTTPRequestHandler(http.server.SimpleHTTPRequestHandler):
            def __init__(self, *args, **kwargs):
                super().__init__(*args, directory=str(mock_web_dir.resolve()), **kwargs)
            def log_message(self, format, *args):
                pass # suppress printing to stdout
                
        # Start the server on an ephemeral port
        PORT = 8081
        handler = QuietHTTPRequestHandler
        for p in range(8081, 8099):
            try:
                httpd = socketserver.TCPServer(("127.0.0.1", p), handler)
                PORT = p
                break
            except OSError:
                continue
                
        server_thread = threading.Thread(target=httpd.serve_forever)
        server_thread.daemon = True
        server_thread.start()
        
        try:
            # Let's write the mock pTerm.wasm file
            mock_wasm_content = b"mock WASM binary content for pTerm"
            mock_wasm_hash = hashlib.sha256(mock_wasm_content).hexdigest()
            
            # The client expects to fetch plugins/plugin.toml
            # and plugins/pTerm (which is the WASM file)
            plugins_web_dir = mock_web_dir / "plugins"
            plugins_web_dir.mkdir()
            
            # Write plugin.toml (manifest)
            with open(plugins_web_dir / "plugin.toml", "w", encoding="utf-8") as f:
                f.write("""[plugin]
name = "pTerm"
version = "1.0.0"
author = "plug"
api_version = "0.1.0a"
permissions = ["host_exec"]
""")
            
            # Write the WASM file inside a pTerm subfolder to match new URL pattern
            pterm_web_dir = plugins_web_dir / "pTerm"
            pterm_web_dir.mkdir()
            with open(pterm_web_dir / "pTerm", "wb") as f:
                f.write(mock_wasm_content)
                
            # Write registry file pluglists.json
            registry_data = {
                "repo_version": "1.0.0",
                "plugins": [
                    {
                        "name": "pTerm",
                        "author": "plug",
                        "version": "1.0.0",
                        "description": "Terminal environment",
                        "official": True,
                        "sha256": mock_wasm_hash
                    }
                ]
            }
            with open(mock_web_dir / "pluglists.json", "w", encoding="utf-8") as f:
                json.dump(registry_data, f)
                
            # Configure environment variables to point to this mock server
            test_env = {
                **os.environ,
                "PLUG_HEADLESS": "1",
                "PLUG_REGISTRY_URL": f"http://127.0.0.1:{PORT}/pluglists.json",
                "PLUG_WASM_BASE_URL": f"http://127.0.0.1:{PORT}/plugins"
            }
            
            # Local helper to run registry query and download in the SAME running process session
            def run_interactive_install(target_bin, env, use_wrong_hash=False):
                p = subprocess.Popen(
                    [target_bin],
                    stdin=subprocess.PIPE,
                    stdout=subprocess.PIPE,
                    stderr=subprocess.PIPE,
                    text=True,
                    encoding="utf-8",
                    startupinfo=startupinfo,
                    creationflags=creationflags,
                    env=env
                )
                
                # 1. Query registry to populate session list
                p.stdin.write("/plug*\n")
                p.stdin.flush()
                
                # Read stdout line by line until we find the session hash
                session_hash = None
                import re
                
                # Consume lines to avoid block
                lines = []
                for _ in range(50):
                    line = p.stdout.readline()
                    if not line:
                        break
                    lines.append(line)
                    match = re.search(r"\[([a-f0-9]{6})\] pTerm", line)
                    if match:
                        session_hash = match.group(1)
                        break
                
                if not session_hash:
                    p.kill()
                    stdout_rem, stderr_rem = p.communicate()
                    return False, f"Session hash not found in registry. Stdout:\n{''.join(lines)}\n{stdout_rem}\nStderr:\n{stderr_rem}"
                
                # 2. Install the plugin using either correct or modified hash
                install_hash = "wrong_hash_12345" if use_wrong_hash else session_hash
                p.stdin.write(f"/plug {install_hash}\n")
                p.stdin.flush()
                
                # 3. Exit the session
                p.stdin.write("/e\n")
                p.stdin.flush()
                
                stdout_rem, stderr_rem = p.communicate(timeout=10)
                stdout_full = "".join(lines) + stdout_rem
                return True, (stdout_full, stderr_rem)

            # Sub-test 8: Successful installation with matching hash
            print("  - Test Case 8: Successful installation with matching hash...")
            if plug_dir.exists():
                shutil.rmtree(plug_dir)
            plug_dir.mkdir()
            
            ok, res = run_interactive_install(target_bin, test_env, use_wrong_hash=False)
            if ok:
                stdout, stderr = res
                if "successfully installed" in stdout or "successfully installed" in stderr:
                    print("    [PASS] Installed successfully under correct matching hash.")
                else:
                    print("    [FAIL] Failed to install with matching hash!")
                    print(f"Stdout:\n{stdout}\nStderr:\n{stderr}")
                    failed = True
            else:
                print(f"    [FAIL] E2E interactive run failed: {res}")
                failed = True
                
            # Sub-test 9: Tampered hash mismatch failure
            print("  - Test Case 9: Prevent tampered hash mismatch installation (fail-closed)...")
            # Modify pluglists.json to have a mismatched hash
            registry_data["plugins"][0]["sha256"] = "wrong_hash_12345"
            with open(mock_web_dir / "pluglists.json", "w", encoding="utf-8") as f:
                json.dump(registry_data, f)
                
            # Run installation. Since we ask it to install the session_hash, but the registry hash has been tampered,
            # it should verify and fail-closed due to hash mismatch.
            ok, res = run_interactive_install(target_bin, test_env, use_wrong_hash=False)
            if ok:
                stdout, stderr = res
                if "[SECURITY] Pinned registry hash mismatch" in stdout or "[SECURITY] Pinned registry hash mismatch" in stderr:
                    print("    [PASS] Prevented installation of tampered plugin successfully.")
                else:
                    print("    [FAIL] Allowed installation of tampered plugin!")
                    print(f"Stdout:\n{stdout}\nStderr:\n{stderr}")
                    failed = True
            else:
                print(f"    [FAIL] E2E interactive run failed for tampered case: {res}")
                failed = True
                
            # Sub-test 10: Missing hash in registry block
            print("  - Test Case 10: Prevent missing hash in registry installation (fail-closed)...")
            # Modify pluglists.json to completely omit the sha256 key
            del registry_data["plugins"][0]["sha256"]
            with open(mock_web_dir / "pluglists.json", "w", encoding="utf-8") as f:
                json.dump(registry_data, f)
                
            ok, res = run_interactive_install(target_bin, test_env, use_wrong_hash=False)
            if ok:
                stdout, stderr = res
                if "Missing pinned hash in registry" in stdout or "Missing pinned hash in registry" in stderr:
                    print("    [PASS] Prevented installation of plugin with missing hash successfully.")
                else:
                    print("    [FAIL] Allowed installation of plugin with missing hash!")
                    print(f"Stdout:\n{stdout}\nStderr:\n{stderr}")
                    failed = True
            else:
                print(f"    [FAIL] E2E interactive run failed for missing hash case: {res}")
                failed = True

        finally:
            # Stop the HTTP server
            httpd.shutdown()
            httpd.server_close()
            server_thread.join()
            # Clean up temp folder
            if mock_web_dir.exists():
                shutil.rmtree(mock_web_dir)


    finally:
        # Restore backup
        if plug_dir.exists():
            shutil.rmtree(plug_dir)
        if backup_dir.exists():
            print("[SANDBOX] Restoring original plugin folder from backup...")
            shutil.move(backup_dir, plug_dir)
            
    sys.exit(1 if failed else 0)

if __name__ == "__main__":
    main()
