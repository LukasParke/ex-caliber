#!/usr/bin/env python3
"""Drive `xc mcp` over stdio with raw JSON-RPC: handshake, tool calls, screenshot."""
import json, subprocess, sys, os, base64

SCENE = os.path.expanduser("~/exval/mcp-demo.excalidraw")
if os.path.exists(SCENE):
    os.remove(SCENE)

proc = subprocess.Popen(
    ["/home/luke/github/ex-caliber/target/debug/xc", "mcp", "--file", SCENE],
    stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
)

next_id = 1
def send(method, params=None, notify=False):
    global next_id
    msg = {"jsonrpc": "2.0", "method": method}
    if params is not None:
        msg["params"] = params
    if not notify:
        msg["id"] = next_id
        next_id += 1
    proc.stdin.write((json.dumps(msg) + "\n").encode())
    proc.stdin.flush()
    if notify:
        return None
    while True:
        line = proc.stdout.readline()
        if not line:
            err = proc.stderr.read().decode()
            raise SystemExit(f"server died. stderr:\n{err[-2000:]}")
        resp = json.loads(line)
        if resp.get("id") == msg["id"]:
            return resp

# 1. Handshake
r = send("initialize", {
    "protocolVersion": "2025-11-25",
    "capabilities": {},
    "clientInfo": {"name": "xc-e2e", "version": "0.0.1"},
})
info = r["result"]
print(f"handshake ok: server={info['serverInfo']['name']} v{info['serverInfo']['version']} proto={info['protocolVersion']}")
send("notifications/initialized", {}, notify=True)

# 2. Tool discovery
r = send("tools/list")
tools = [t["name"] for t in r["result"]["tools"]]
print(f"tools ({len(tools)}):", ", ".join(sorted(tools)))
def call(name, arguments):
    r = send("tools/call", {"name": name, "arguments": arguments})
    if r["result"].get("isError"):
        raise SystemExit(f"{name} errored: {r['result']['content']}")
    return r["result"]["content"]

# 3. Build a small architecture diagram: three boxes + arrows + labels
r = call("create_elements", {"elements": [
    {"type": "rectangle", "x": 0,   "y": 0,   "width": 160, "height": 70,
     "backgroundColor": "#a5d8ff"},
    {"type": "rectangle", "x": 320, "y": 0,   "width": 160, "height": 70,
     "backgroundColor": "#b2f2bb"},
    {"type": "ellipse",   "x": 340, "y": 180, "width": 120, "height": 60,
     "backgroundColor": "#ffc9c9"},
]})
ids = json.loads(r[0]["text"])["ids"]
client_id, server_id, db_id = ids
print(f"created: client={client_id} server={server_id} db={db_id}")

# Unknown types must be rejected cleanly (tool error, not crash).
r = send("tools/call", {"name": "create_elements", "arguments": {"elements": [
    {"type": "cylinder", "x": 0, "y": 0}]}})
assert r["result"].get("isError"), "unknown type should be a tool error"
print("unknown-type rejection ok")

# 4. Labels + connections
call("add_text", {"text": "Client", "container_id": client_id})
call("add_text", {"text": "Server", "container_id": server_id})
call("add_text", {"text": "Database", "container_id": db_id})
a1 = json.loads(call("connect", {"from_id": client_id, "to_id": server_id,
                                  "label": "HTTPS"})[0]["text"])["arrow_id"]
a2 = json.loads(call("connect", {"from_id": server_id, "to_id": db_id,
                                  "label": "SQL"})[0]["text"])["arrow_id"]
print(f"connected: {a1}, {a2}")

# 5. Scene reads back what we built
r = call("get_scene", {})
scene = json.loads(r[0]["text"])
assert scene["count"] == 10, f"expected 10 elements, got {scene['count']}"
kinds = sorted(e["type"] for e in scene["elements"])
print(f"scene: {scene['count']} elements {kinds}")

# 6. Screenshot returns a real PNG
r = call("screenshot", {})
img = next(c for c in r if c.get("type") == "image")
png = base64.b64decode(img["data"])
assert png[1:4] == b"PNG" and len(png) > 1000, "screenshot must be a real PNG"
print(f"screenshot: {len(png)} bytes PNG")

# 7. Undo removes the last connect (arrow + label); redo restores.
call("undo", {})
scene = json.loads(call("get_scene", {})[0]["text"])
assert scene["count"] == 8, f"after undo expected 8, got {scene['count']}"
call("redo", {})
scene = json.loads(call("get_scene", {})[0]["text"])
assert scene["count"] == 10, f"after redo expected 10, got {scene['count']}"
print("undo/redo ok")

# 8. Export excalidraw JSON and validate the persisted file.
call("export_file", {"format": "excalidraw", "path": os.path.expanduser("~/exval/mcp-export.excalidraw")})
proc.stdin.close()
proc.wait(timeout=5)

saved = json.load(open(SCENE))
assert saved["type"] == "excalidraw" and saved["version"] == 2
live = [e for e in saved["elements"] if not e.get("isDeleted")]
assert len(live) == 10, f"file should hold 10 live elements, got {len(live)}"
arrow = next(e for e in live if e["type"] == "arrow" and e["id"] == a1)
assert arrow["startBinding"]["elementId"] == client_id
assert "fixedPoint" in arrow["startBinding"]
print("persistence: file valid, bindings intact")
print("M2 E2E: ALL CHECKS PASSED")
