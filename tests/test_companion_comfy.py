"""Exercise the actual supervisor with harmless local stand-in backends."""
import ctypes
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import unittest

LAUNCHER = Path(__file__).resolve().parents[1] / "src-tauri/resources/companion/managed-comfy.py"


def stopped(pid):
    if os.name != "nt":
        try:
            os.kill(pid, 0)
            return False
        except ProcessLookupError:
            return True
    kernel = ctypes.WinDLL("kernel32", use_last_error=True)
    kernel.OpenProcess.argtypes = [ctypes.c_ulong, ctypes.c_bool, ctypes.c_ulong]
    kernel.OpenProcess.restype = ctypes.c_void_p
    kernel.WaitForSingleObject.argtypes = [ctypes.c_void_p, ctypes.c_ulong]
    kernel.CloseHandle.argtypes = [ctypes.c_void_p]
    handle = kernel.OpenProcess(0x00100000, False, pid)
    if not handle:
        return True
    try:
        return kernel.WaitForSingleObject(handle, 0) == 0
    finally:
        kernel.CloseHandle(handle)


class SupervisorScenarios(unittest.TestCase):
    def test_backend_receives_literal_arguments_and_unicode_working_directory(self):
        with tempfile.TemporaryDirectory(prefix="penguin-comfy-中文 ") as folder:
            root = Path(folder)
            (root / "main.py").write_text("import json,sys; print(json.dumps(sys.argv))", encoding="utf-8")
            output = subprocess.run([sys.executable, "-s", str(LAUNCHER), str(os.getpid()), "--cpu", "literal ; & text"], cwd=root, capture_output=True, timeout=10, check=True)
            self.assertEqual(json.loads(output.stdout), ["main.py", "--cpu", "literal ; & text"])

    def test_invalid_owner_fails_before_backend_runs(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            (root / "main.py").write_text("raise RuntimeError('backend must not run')", encoding="utf-8")
            output = subprocess.run([sys.executable, str(LAUNCHER), "0"], cwd=root, capture_output=True, timeout=10)
            self.assertNotEqual(output.returncode, 0)
            self.assertIn(b"Invalid owning pet process", output.stderr)
            self.assertNotIn(b"backend must not run", output.stderr)

    @unittest.skipUnless(os.name == "nt", "Windows process handle lifecycle")
    def test_backend_exits_when_owning_pet_process_exits(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            (root / "main.py").write_text("from pathlib import Path\nimport time\nPath('started').write_text('yes')\ntime.sleep(30)\n", encoding="utf-8")
            parent = """import os,subprocess,sys,time
from pathlib import Path
p=subprocess.Popen([sys.executable,'-s',sys.argv[1],str(os.getpid())],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
for _ in range(100):
    if Path('started').exists(): break
    time.sleep(.05)
print(p.pid,flush=True)
"""
            output = subprocess.run([sys.executable, "-c", parent, str(LAUNCHER)], cwd=root, capture_output=True, timeout=12, check=True)
            self.assertTrue((root / "started").exists())
            pid = int(output.stdout)
            until = time.monotonic() + 5
            while not stopped(pid) and time.monotonic() < until:
                time.sleep(.1)
            self.assertTrue(stopped(pid), "backend outlived its owning pet")


@unittest.skipUnless(os.name == "nt", "Windows private runtime installer")
class InstallerSafetyScenarios(unittest.TestCase):
    def test_unowned_nonempty_directory_is_preserved_without_downloading(self):
        installer = LAUNCHER.with_name("install-comfy.ps1")
        with tempfile.TemporaryDirectory(prefix="penguin-installer-中文 ") as folder:
            root = Path(folder)
            sentinel = root / "existing-user-file.txt"
            sentinel.write_text("preserve me", encoding="utf-8")
            status = root / "install-status.json"
            result = subprocess.run(["powershell.exe", "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File", str(installer), "-Root", str(root), "-StatusFile", str(status)], capture_output=True, timeout=15)
            self.assertNotEqual(result.returncode, 0)
            report = json.loads(status.read_text(encoding="utf-8-sig"))
            self.assertEqual(report["state"], "failed")
            self.assertIn("not empty", report["message"])
            self.assertEqual(sentinel.read_text(encoding="utf-8"), "preserve me")
            self.assertFalse((root / "python").exists())
            self.assertFalse((root / ".penguinpal-runtime.json").exists())


if __name__ == "__main__":
    unittest.main()
