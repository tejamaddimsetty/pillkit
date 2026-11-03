#!/usr/bin/env bash
# Pill Kit for macOS and Linux:  ./install.sh
#
# Gets the host (built with cargo if you have Rust, otherwise downloaded from
# the latest GitHub release), starts it at login, and shows a demo pill.
# No server, account or Python needed. Safe to run again; ./stop.sh stops it.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")" && pwd)"
REPO="tejamaddimsetty/pillkit"
if [[ "$(uname)" == "Darwin" ]]; then DATA="$HOME/Library/Application Support/pillkit"; OS=macos; else DATA="${XDG_DATA_HOME:-$HOME/.local/share}/pillkit"; OS=linux; fi
mkdir -p "$DATA/bin"
step() { printf '\n\033[36m[%s/3] %s\033[0m\n' "$1" "$2"; }
fail() { printf '\n  \033[31m%s\033[0m\n' "$1"; exit 1; }

echo "Pill Kit installer"

step 1 "Getting pillkit-host"
DL="$DATA/bin/pillkit-host.download"
ARCH="$(uname -m)"; [[ "$ARCH" == "arm64" || "$ARCH" == "aarch64" ]] && ARCH=arm64 || ARCH=x64
if command -v cargo >/dev/null; then
  echo "  Building from source with cargo..."
  (cd "$ROOT" && cargo build -q --release -p pillkit-host)
  cp "$ROOT/target/release/pillkit-host" "$DL"
else
  URL="https://github.com/$REPO/releases/latest/download/pillkit-host-$OS-$ARCH"
  echo "  Downloading $URL"
  curl -fsSL "$URL" -o "$DL" || fail "Download failed. Install Rust (https://rustup.rs) to build it instead."
fi
chmod +x "$DL"

step 2 "Starting it (and at every login)"
"$ROOT/stop.sh" --quiet || true
"$DL" install >/dev/null          # copies itself to $DATA/bin and adds the login item
rm -f "$DL"
HOST="$DATA/bin/pillkit-host"
nohup "$HOST" run >/dev/null 2>&1 &
echo "  $("$HOST" --version)"

step 3 "Showing a demo pill. Look at your menu bar!"
"$HOST" show --app pillkit-demo --title "Pill Kit is running" --subtitle "Hover me, then try the SDK" \
  --label "Demo" --url "https://github.com/tejamaddimsetty/pillkit#readme"
echo
echo "  Try it yourself:   $HOST show --title \"Hello\" --subtitle \"from my terminal\""
echo "  Remove the demo:   $HOST clear --app pillkit-demo"
echo "  Stop everything:   ./stop.sh      Uninstall: $HOST uninstall"
echo "  Log:               $DATA/host.log"
