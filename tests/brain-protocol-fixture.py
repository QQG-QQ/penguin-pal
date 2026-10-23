"""Synthetic input only: used by Rust to compare both protocol implementations."""
import importlib.util
import json
import sys
from pathlib import Path

spec = importlib.util.spec_from_file_location("sync_server", Path(__file__).parents[1] / "src-tauri/resources/companion/sync-server.py")
server = importlib.util.module_from_spec(spec)
spec.loader.exec_module(server)
value = json.loads(sys.stdin.buffer.read())
document = value["document"]
for operation in value["operations"]:
    server.apply(document, operation, value["now"])
server.validate(document)
sys.stdout.buffer.write(json.dumps(document, ensure_ascii=False).encode("utf-8"))
