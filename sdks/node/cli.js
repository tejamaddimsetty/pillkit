#!/usr/bin/env node
// `npx pillkit show --title "..."`: forwards to the bundled pillkit-host CLI.
import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const exe = process.platform === "win32" ? "pillkit-host.exe" : "pillkit-host";
let host = process.env.PILLKIT_HOST_PATH;
if (!host) {
  try {
    host = require.resolve(`@pillkit/host-${process.platform}-${process.arch}/bin/${exe}`);
  } catch {
    host = null;
  }
}
if (!host || !existsSync(host)) {
  console.error(`pillkit: no host for ${process.platform}-${process.arch}. Reinstall @pillkit/sdk.`);
  process.exit(1);
}
const r = spawnSync(host, process.argv.slice(2), { stdio: "inherit", windowsHide: true });
process.exit(r.status ?? 1);
