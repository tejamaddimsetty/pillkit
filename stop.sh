#!/usr/bin/env bash
# Stops the server and host started by install.sh.
if [[ "$(uname)" == "Darwin" ]]; then DATA="$HOME/Library/Application Support/pillkit"; else DATA="${XDG_DATA_HOME:-$HOME/.local/share}/pillkit"; fi
for f in "$DATA/run/server.pid" "$DATA/run/host.pid"; do
  [[ -f "$f" ]] && kill "$(cat "$f")" 2>/dev/null; rm -f "$f"
done
pkill -f "pillkit-host-dev run" 2>/dev/null || true
[[ "${1:-}" == "--quiet" ]] || echo "pillkit stopped."
