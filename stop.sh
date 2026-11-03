#!/usr/bin/env bash
# Stops the Pill Kit host (it starts again at the next login; ./install.sh starts it now).
pkill -f "pillkit-host run" 2>/dev/null || true
pkill -f "pillkit-host-dev run" 2>/dev/null || true
[[ "${1:-}" == "--quiet" ]] || echo "Pill Kit stopped."
