# RFC 0001: One shared host per user

- Status: accepted

## Problem

If each app drew its own widget, several apps on one machine would fight over
the single taskbar slot, and the widget would vanish whenever its app closed.

## Proposal

The SDK installs one `pillkit-host` per user and starts it at login. It owns the
slot, verifies licences and feeds, and rotates between registered apps (round
robin across apps, weighted within an app). SDKs talk to it over a local
socket (docs/PROTOCOL.md §5). The newest host version wins: an SDK that bundles
a newer host asks the running one to shut down and replaces it.

## Alternatives considered

- **Library only, drawn by each app**: simplest, but collisions and no
  persistence.
- **A separate end-user installer**: nobody installs an ad bar on purpose.

## Security and privacy impact

The host is the trust boundary: it holds the server's public key and enforces
signatures and the URL allowlist, so a compromised app can't show unsigned
content or open arbitrary URLs.

## Migration

None (initial design).
