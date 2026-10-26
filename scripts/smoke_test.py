"""End-to-end check used in CI: talk to a running pillkit-host like an app
does, and confirm it registers and gets a licence from the local server.

    python scripts/smoke_test.py APP_ID KEY [SERVER]
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


def main():
    global _START
    _START = len(host_log())
    app_id, key = sys.argv[1], sys.argv[2]
    server = sys.argv[3] if len(sys.argv) > 3 else "http://127.0.0.1:5050"
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
    r = request({"op": "register", "app_id": app_id, "key": key, "bundle_id": "", "server": server})
    print("register:", r)
    if not r.get("ok"):
        fail(f"register refused: {r}")
    deadline = time.time() + 45
    while time.time() < deadline:
        st = request({"op": "status"})
        if st.get("licensed") and st.get("feed_items", 0) > 0:
            print("status:", st)
            break
        time.sleep(2)
    else:
        fail(f"not licensed after 45 s: {st}")

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
            print("PASS: registered, licensed, feed received, pill placed, host alive")
            print("--- host.log ---\n" + host_log())
            return
        time.sleep(1)
    fail("the pill was never placed")


if __name__ == "__main__":
    main()
