# Contributing

Thanks for helping. A few ground rules keep this codebase healthy.

## Workflow

1. Open an issue first for anything bigger than a bug fix.
2. Changes to `docs/PROTOCOL.md` need an RFC in `rfcs/` (copy `rfcs/0000-template.md`).
3. One topic per pull request. CI must pass on macOS, Windows and Linux.
4. Use [Conventional Commits](https://www.conventionalcommits.org/): `feat(host): …`, `fix(server): …`.

## Running checks

```bash
cargo test -p pillkit-core
cargo build -p pillkit-host
cargo check -p pillkit-host --target x86_64-pc-windows-msvc   # from macOS/Linux
cd server && .venv/bin/python -m pytest -q tests
```

## Code style

- Rust: `cargo fmt`, `cargo clippy`. No `unwrap()` on anything that comes from
  the network, the socket or the disk.
- Python: follow the surrounding code; keep handlers small.
- Comments explain *why*, not what.

## Versioning

Semantic versioning. The host and SDKs share a minor version; the protocol
version (`PROTOCOL_VERSION`) only changes with an accepted RFC.
