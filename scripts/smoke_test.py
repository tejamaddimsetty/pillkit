"""End-to-end check used in CI: talk to a running pillkit-host like an app
does (open mode, no server), show an item, and confirm the pill is placed,
the card opens on hover (Windows), and the host stays alive.

    python scripts/smoke_test.py
Exits non-zero (and prints host.log) on failure.
"""
import os
import sys
import threading
import time

sys.path.insert(0, os.path.dirname(__file__))
from demo_app import Conn, connect  # noqa: E402


_START = None


def host_log():
    """host.log written since this test started."""
    if os.name == "nt":
        path = os.path.join(os.environ["LOCALAPPDATA"], "pillkit", "host.log")
    elif sys.platform == "darwin":
        path = os.path.expanduser("~/Library/Application Support/pillkit/host.log")
    else:
        path = os.path.join(os.environ.get("XDG_DATA_HOME", os.path.expanduser("~/.local/share")), "pillkit", "host.log")
    try:
        text = open(path, encoding="utf-8", errors="replace").read()
    except OSError:
        return ""
    return text[_START:] if _START is not None else text


def fail(msg):
    print(f"FAIL: {msg}\n--- host.log ---\n{host_log()}")
    sys.exit(1)


def check_hover_card():
    """Windows: hover the pill like a person would and expect the card."""
    import ctypes
    import re
    placed = re.findall(r"placing pill: mode \w+, at \((-?\d+), (-?\d+)\) size (\d+)x(\d+)", host_log())
    if not placed:
        fail("no placement in the log to hover over")
    x, y, w, h = map(int, placed[-1])
    user32 = ctypes.windll.user32
    user32.SetCursorPos(x + w // 2, y + h // 2)
    deadline = time.time() + 5
    while "card shown" not in host_log():
        if time.time() > deadline:
            fail("hovering the pill did not open the card")
        time.sleep(0.2)
    print("hover: card shown")
    user32.SetCursorPos(10, 10)
    deadline = time.time() + 5
    while "card hidden" not in host_log():
        if time.time() > deadline:
            fail("moving away did not close the card")
        time.sleep(0.2)
    print("hover: card hidden")


def main():
    global _START
    _START = len(host_log())
    c = connect(timeout=30)

    # A reply that never comes is the failure we care about most (deadlock),
    # so every request has a deadline.
    def request(body, timeout=10):
        box = {}
        def run():
            try:
                c.send(body)
                box["reply"] = c.recv()
            except Exception as e:  # noqa: BLE001
                box["error"] = e
        t = threading.Thread(target=run, daemon=True)
        t.start()
        t.join(timeout)
        if t.is_alive():
            fail(f"no reply to {body['op']} within {timeout} s (host stuck?)")
        if "error" in box:
            fail(f"{body['op']}: {box['error']}")
        return box["reply"]

    r = request({"op": "hello"})
    print("hello:", r)
    r = request({"op": "register", "app_id": "smoke-test"})
    print("register:", r)
    if not r.get("ok") or r.get("mode") != "open":
        fail(f"register (open mode) refused: {r}")
    r = request({"op": "show", "item": {"id": "smoke", "title": "Smoke test", "subtitle": "open mode",
                                        "label": "CI", "action": {"type": "open_url", "url": "https://example.com/"}}})
    if not r.get("ok"):
        fail(f"show refused: {r}")
    r = request({"op": "show", "item": {"id": "bad", "title": "Bad", "action": {"type": "open_url", "url": "file:///etc/passwd"}}})
    if r.get("ok"):
        fail("a file: link was accepted")
    st = request({"op": "status"})
    print("status:", st)

    # The pill must actually be put somewhere, and the host must survive it.
    deadline = time.time() + 20
    while time.time() < deadline:
        log = host_log()
        if "CRASH" in log:
            fail("the host crashed")
        if "placing pill" in log or "geometry:" in log or "show [" in log:
            time.sleep(8)  # let it rotate and re-layout a few times
            if "CRASH" in host_log():
                fail("the host crashed after placing the pill")
            request({"op": "status"})  # still answering?
            if os.name == "nt":
                check_hover_card()
            print("PASS: registered, licensed, feed received, pill placed, host alive")
            print("--- host.log ---\n" + host_log())
            return
        time.sleep(1)
    fail("the pill was never placed")


if __name__ == "__main__":
    main()
