"""A tiny "developer app" using only the Python standard library. It does
what the SDK does: connect to pillkit-host, register (open mode: local only,
no server), and show an item.

    python scripts/demo_app.py [APP_ID]
"""
import json
import os
import re
import socket
import sys
import time


def endpoint():
    if os.name == "nt":
        user = re.sub(r"[^A-Za-z0-9_-]", "", os.environ.get("USERNAME", "user"))
        return rf"\\.\pipe\pillkit-host-{user}"
    base = os.path.expanduser("~/Library/Application Support/pillkit") if sys.platform == "darwin" else \
        os.path.join(os.environ.get("XDG_DATA_HOME", os.path.expanduser("~/.local/share")), "pillkit")
    return os.path.join(base, "host.sock")


class Conn:
    def __init__(self):
        path = endpoint()
        if os.name == "nt":
            self.f = open(path, "r+b", buffering=0)
            self.write = self.f.write
            self.lines = self._pipe_lines()
        else:
            s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            s.connect(path)
            self.f = s.makefile("rwb", buffering=0)
            self.write = self.f.write
            self.lines = iter(self.f.readline, b"")

    def _pipe_lines(self):
        buf = b""
        while True:
            chunk = self.f.read(1)
            if not chunk:
                return
            buf += chunk
            if chunk == b"\n":
                yield buf
                buf = b""

    def send(self, body):
        self.write((json.dumps(body) + "\n").encode())

    def recv(self):
        for line in self.lines:
            line = line.strip()
            if line:
                return json.loads(line)
        raise ConnectionError("pillkit-host closed the connection")


def connect(timeout=15):
    deadline = time.time() + timeout
    while True:
        try:
            return Conn()
        except OSError:
            if time.time() > deadline:
                raise SystemExit("Could not reach pillkit-host. Is it running? See host.log in the pillkit data folder.")
            time.sleep(0.5)


def main():
    app_id = sys.argv[1] if len(sys.argv) > 1 else "python-demo"

    c = connect()
    c.send({"op": "register", "app_id": app_id})
    reply = c.recv()
    if not reply.get("ok"):
        raise SystemExit(f"Register failed: {reply.get('error')}")
    c.send({"op": "show", "item": {
        "id": "demo-status", "title": "Build passed", "subtitle": "main · 2 minutes ago",
        "theme": {"background": "#1F7A4D", "foreground": "#FFFFFF"},
        "action": {"type": "callback", "name": "open-build"}, "duration_secs": 10,
    }})
    c.recv()
    print("Demo app registered. The widget should appear within a few seconds.", flush=True)
    print("Clicks on 'Build passed' show up here. Close this window to stop the demo app.", flush=True)
    try:
        while True:
            msg = c.recv()
            if msg.get("event") == "callback":
                print(f"  clicked: {msg.get('name')}", flush=True)
    except (ConnectionError, KeyboardInterrupt):
        pass


if __name__ == "__main__":
    main()
