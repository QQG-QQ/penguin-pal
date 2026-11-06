"""Trusted local backend launcher. Exit when the owning pet process exits."""
import ctypes
import os
import runpy
import sys
import threading
import time


def watch_parent(parent):
    if os.name == "nt":
        kernel = ctypes.WinDLL("kernel32", use_last_error=True)
        kernel.OpenProcess.argtypes = [ctypes.c_ulong, ctypes.c_bool, ctypes.c_ulong]
        kernel.OpenProcess.restype = ctypes.c_void_p
        kernel.WaitForSingleObject.argtypes = [ctypes.c_void_p, ctypes.c_ulong]
        kernel.CloseHandle.argtypes = [ctypes.c_void_p]
        handle = kernel.OpenProcess(0x00100000, False, parent)
        if not handle:
            raise RuntimeError("Cannot watch the owning pet process")

        def wait():
            kernel.WaitForSingleObject(handle, 0xFFFFFFFF)
            kernel.CloseHandle(handle)
            os._exit(0)
    else:
        if os.getppid() != parent:
            raise RuntimeError("Owning pet process changed")

        def wait():
            while os.getppid() == parent:
                time.sleep(0.2)
            os._exit(0)
    threading.Thread(target=wait, daemon=True).start()


if __name__ == "__main__":
    parent = int(sys.argv.pop(1))
    if parent <= 0 or parent == os.getpid():
        raise RuntimeError("Invalid owning pet process")
    watch_parent(parent)
    # The working directory and arguments are set by native code, never chat text.
    sys.path.insert(0, os.getcwd())
    sys.argv[0] = "main.py"
    runpy.run_path("main.py", run_name="__main__")
