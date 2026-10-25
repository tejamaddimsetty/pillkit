"""A stand-in "developer app" for local testing, using only the Python standard
library. It does what the SDK does: connect to barkit-host, register, and show
a status item next to the portal's items.

    python scripts/demo_app.py APP_ID KEY [SERVER]
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
        return rf"\\.\pipe\barkit-host-{user}"
    base = os.path.expanduser("~/Library/Application Support/barkit") if sys.platform == "darwin" else \
        os.path.join(os.environ.get("XDG_DATA_HOME", os.path.expanduser("~/.local/share")), "barkit")
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
        raise ConnectionError("barkit-host closed the connection")


def connect(timeout=15):
    deadline = time.time() + timeout
    while True:
        try:
            return Conn()
        except OSError:
            if time.time() > deadline:
                raise SystemExit("Could not reach barkit-host. Is it running? See host.log in the barkit data folder.")
            time.sleep(0.5)


def main():
    if len(sys.argv) < 3:
        raise SystemExit(__doc__)
    app_id, key = sys.argv[1], sys.argv[2]
    server = sys.argv[3] if len(sys.argv) > 3 else "http://localhost:5050"

    c = connect()
    c.send({"op": "register", "app_id": app_id, "key": key, "bundle_id": "", "server": server})
    reply = c.recv()
    if not reply.get("ok"):
        raise SystemExit(f"Register failed: {reply.get('error')}")
    c.send({"op": "show", "item": {
        "id": "demo-status", "title": "Build passed", "subtitle": "main · 2 minutes ago",
        "theme": {"background": "#1F7A4D", "foreground": "#FFFFFF"},
        "action": {"type": "callback", "name": "open-build"}, "duration_secs": 10,
    }})
    c.recv()
    print("Demo app registered. The widget should appear within a few seconds.")
    print("Clicks on 'Build passed' show up here. Close this window to stop the demo app.")
    try:
        while True:
            msg = c.recv()
            if msg.get("event") == "callback":
                print(f"  clicked: {msg.get('name')}")
    except (ConnectionError, KeyboardInterrupt):
        pass


if __name__ == "__main__":
    main()
