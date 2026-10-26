#!/usr/bin/env bash
# pillkit local installer for macOS and Linux:  ./install.sh
#
# Sets up the server, gets the host (built with cargo if you have Rust,
# otherwise downloaded from the latest GitHub release), starts everything,
# registers a demo app and opens the portal. Safe to run again.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")" && pwd)"
SERVER="$ROOT/server"
REPO="tejamaddimsetty/pillkit"
if [[ "$(uname)" == "Darwin" ]]; then DATA="$HOME/Library/Application Support/pillkit"; OS=macos; else DATA="${XDG_DATA_HOME:-$HOME/.local/share}/pillkit"; OS=linux; fi
RUN="$DATA/run"; mkdir -p "$RUN" "$DATA/bin"
step() { printf '\n\033[36m[%s/6] %s\033[0m\n' "$1" "$2"; }
fail() { printf '\n  \033[31m%s\033[0m\n' "$1"; exit 1; }

echo "Pill Kit installer"

step 1 "Checking for Python 3.10+"
PY=""
for c in python3.13 python3.12 python3.11 python3.10 python3; do
  if command -v "$c" >/dev/null && "$c" -c 'import sys; sys.exit(sys.version_info < (3, 10))' 2>/dev/null; then PY="$(command -v "$c")"; break; fi
done
[[ -n "$PY" ]] || fail "Install Python 3.10+ (https://www.python.org/downloads/ or 'brew install python'), then run ./install.sh again."
echo "  Using $PY"

step 2 "Setting up the server"
[[ -x "$SERVER/.venv/bin/python" ]] || "$PY" -m venv "$SERVER/.venv"
"$SERVER/.venv/bin/python" -m pip install -q --disable-pip-version-check -r "$SERVER/requirements.txt"
if [[ ! -f "$SERVER/.env" ]]; then
  cat > "$SERVER/.env" <<ENV
DATABASE_URL=sqlite:///storage/pillkit.db
SECRET_KEY=$("$PY" -c 'import secrets; print(secrets.token_hex(32))')
SIGNING_KEY_PATH=storage/signing.key
PUBLIC_BASE_URL=http://127.0.0.1:5050
FEED_POLL_SECS=15
ENV
fi
SEED="$(cd "$SERVER" && .venv/bin/python dev_seed.py)"
APP_ID="$(sed -n 's/^APP_ID=//p' <<<"$SEED")"
KEY="$(sed -n 's/^KEY=//p' <<<"$SEED")"
echo "  Demo app: $APP_ID"

step 3 "Getting pillkit-host"
HOST="$DATA/bin/pillkit-host-dev"
HAVE="$([[ -x "$HOST" ]] && "$HOST" --version 2>/dev/null | sed 's/pillkit-host //' || true)"
LATEST="$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" 2>/dev/null | sed -n 's/.*"tag_name": *"v\{0,1\}\([^"]*\)".*/\1/p' | head -1 || true)"
[[ -n "$HAVE" ]] && echo "  Installed host: $HAVE   Latest release: ${LATEST:-unknown}"
if [[ "${1:-}" == "--update" || ! -x "$HOST" || ( -n "$LATEST" && "$HAVE" != "$LATEST" ) ]]; then
  if command -v cargo >/dev/null; then
    echo "  Building from source with cargo..."
    (cd "$ROOT" && cargo build -q -p pillkit-host)
    cp "$ROOT/target/debug/pillkit-host" "$HOST"
  else
    ARCH="$(uname -m)"; [[ "$ARCH" == "arm64" || "$ARCH" == "aarch64" ]] && ARCH=arm64 || ARCH=x64
    [[ "$OS-$ARCH" == "macos-arm64" || "$OS-$ARCH" == "linux-x64" ]] || fail "No prebuilt host for $OS-$ARCH. Install Rust (https://rustup.rs) and run again."
    URL="https://github.com/$REPO/releases/latest/download/pillkit-host-dev-$OS-$ARCH"
    echo "  Downloading $URL"
    curl -fsSL "$URL" -o "$HOST.download" || fail "Download failed. Install Rust (https://rustup.rs) to build it instead."
    mv "$HOST.download" "$HOST"; chmod +x "$HOST"
  fi
fi
echo "  Using $HOST"

step 4 "Starting the server and the host"
"$ROOT/stop.sh" --quiet || true
(cd "$SERVER" && nohup .venv/bin/python app.py >"$RUN/server.log" 2>&1 & echo $! >"$RUN/server.pid")
for _ in $(seq 60); do curl -fs http://127.0.0.1:5050/healthz >/dev/null && break; sleep 0.5; done
curl -fs http://127.0.0.1:5050/healthz >/dev/null || fail "The server didn't start. See $RUN/server.log"
export PILLKIT_PUBLIC_KEY="$(curl -fs http://127.0.0.1:5050/v1/public-key | "$PY" -c 'import sys, json; print(json.load(sys.stdin)["key"])')"
nohup "$HOST" run >"$RUN/host.out" 2>&1 & echo $! >"$RUN/host.pid"
echo "  Server: http://127.0.0.1:5050"

step 5 "Opening the portal"
(command -v open >/dev/null && open http://127.0.0.1:5050/login) || (command -v xdg-open >/dev/null && xdg-open http://127.0.0.1:5050/login) || true
echo "  Developer login: demo@pillkit.dev / demo-password"
echo "  Admin login:     admin@pillkit.dev / admin-password"

step 6 "Registering the demo app. Look at your menu bar!"
echo "  Host log: $DATA/host.log    Stop everything: ./stop.sh"
exec "$SERVER/.venv/bin/python" "$ROOT/scripts/demo_app.py" "$APP_ID" "$KEY" "http://127.0.0.1:5050"
