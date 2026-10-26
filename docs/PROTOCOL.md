# Pill Kit protocol v1

This is the contract between the four moving parts. Anything that changes it
needs an RFC in `rfcs/`.

```
 developer app ──(SDK, local IPC)──▶ pillkit-host ──(HTTPS)──▶ Pill Kit server
                                          │
                                          └── draws the widget in the OS bar
```

## 1. Signed envelopes

Everything the server sends the host (licences, feeds) is a signed envelope:

```
pillkit1.<base64url(payload JSON)>.<base64url(ed25519 signature)>
```

The signature covers the ASCII bytes `pillkit1.<payload part>`. The host ships
with the server's public key and refuses anything that does not verify, has
expired, or is for a different app. This is what stops a hijacked network or a
hostile local process from putting arbitrary content in the user's taskbar.

## 2. Licence

`POST /v1/license` with `Authorization: Bearer <publishable key>` and body
`{"app_id": "...", "bundle_id": "...", "platform": "macos|windows|linux"}`.

Payload:

```json
{
  "typ": "license",
  "lic": "lic_...",
  "dev": "dev_...",
  "app_id": "app_...",
  "bundle_id": "com.acme.clip",
  "plan": "lifetime",
  "entitlements": {"watermark": false, "max_items": 20},
  "iat": 1761350400,
  "exp": 1763942400
}
```

`exp` is a refresh deadline (30 days), not the end of the licence. The host
refreshes daily and keeps working offline until `exp`. A revoked licence stops
refreshing, so the widget switches off within 30 days at most (immediately if
the host is online, because the feed endpoint also refuses revoked apps).

## 3. Feed

`GET /v1/feed?app_id=...` with the publishable key. Payload:

```json
{
  "typ": "feed",
  "app_id": "app_...",
  "iat": 1761350400,
  "exp": 1761354000,
  "poll_secs": 300,
  "allowed_url_prefixes": ["https://acme.com/", "acmeclip://"],
  "items": [ ITEM, ... ]
}
```

## 4. Item

```json
{
  "id": "itm_...",
  "title": "Acme Cloud",
  "subtitle": "Deploy in 30s. Try free.",
  "label": "Sponsored",
  "icon": {"type": "monogram"} | {"type": "png", "data": "<base64>"},
  "theme": {"background": "#E8F04A", "foreground": "#2A2C05"},
  "action": ACTION,
  "card": {"body": "...", "buttons": [{"label": "Open site", "action": ACTION}]},
  "weight": 1,
  "duration_secs": 20,
  "starts_at": null,
  "ends_at": null
}
```

`label` is required to be non-empty for sponsored items; the server enforces it.

### Actions

| type        | fields  | effect                                                    |
|-------------|---------|-----------------------------------------------------------|
| `open_url`  | `url`   | open in the default browser (must match an allowed prefix) |
| `deep_link` | `url`   | open a custom-scheme URL (must match an allowed prefix)    |
| `callback`  | `name`  | sent back to the registering app over IPC                  |
| `card`      | —       | show the item's card (popover)                             |
| `none`      | —       | no-op                                                      |

The host checks every URL against `allowed_url_prefixes` before opening it.

## 5. Local IPC (SDK ⇄ host)

Transport: newline-delimited JSON.

- macOS / Linux: Unix socket at `<data dir>/host.sock`
- Windows: named pipe `\\.\pipe\pillkit-host-<username>`

Data dir: `~/Library/Application Support/pillkit` (macOS),
`%LOCALAPPDATA%\pillkit` (Windows), `$XDG_DATA_HOME/pillkit` (Linux).

SDK → host:

```json
{"op": "hello"}
{"op": "register", "app_id": "app_...", "key": "pk_...", "bundle_id": "...", "server": "https://..."}
{"op": "show", "item": ITEM}          // local item, e.g. a status message
{"op": "clear"}                        // drop this app's local items
{"op": "unregister"}
{"op": "shutdown"}                     // used by the SDK to replace an older host
```

Host → SDK:

```json
{"ok": true, "version": "0.1.0"}
{"ok": false, "error": "license_invalid"}
{"event": "callback", "item_id": "itm_...", "name": "open_settings"}
```

Local items are allowed only while the app holds a valid licence, and are
shown with the same rotation and labelling rules as feed items.

## 6. Events

`POST /v1/events` with the publishable key, batched every few minutes:

```json
{"app_id": "app_...", "events": [{"item_id": "itm_...", "type": "impression|click", "count": 3}]}
```

No user identifiers are sent.
