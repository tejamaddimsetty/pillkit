// @barkit/sdk: talks to the per-user barkit-host over a local socket
// (docs/PROTOCOL.md §5). Installs and starts the host on first use.

import { spawn, execFileSync } from "node:child_process";
import { EventEmitter } from "node:events";
import { existsSync, readFileSync } from "node:fs";
import net from "node:net";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const pkg = JSON.parse(readFileSync(path.join(here, "package.json"), "utf8"));
const BUNDLED_HOST_VERSION = pkg.barkit.hostVersion;
const EXE = process.platform === "win32" ? "barkit-host.exe" : "barkit-host";

export function dataDir() {
  if (process.platform === "darwin") return path.join(os.homedir(), "Library", "Application Support", "barkit");
  if (process.platform === "win32") return path.join(process.env.LOCALAPPDATA || os.homedir(), "barkit");
  return path.join(process.env.XDG_DATA_HOME || path.join(os.homedir(), ".local", "share"), "barkit");
}

export function endpoint() {
  if (process.platform === "win32") {
    const user = (process.env.USERNAME || "user").replace(/[^A-Za-z0-9_-]/g, "");
    return `\\\\.\\pipe\\barkit-host-${user}`;
  }
  return path.join(dataDir(), "host.sock");
}

function bundledHost(opts) {
  const candidates = [
    opts.hostPath,
    process.env.BARKIT_HOST_PATH,
    // Electron: ship the binary outside the asar archive (asarUnpack).
    path.join(here, "bin", `${process.platform}-${process.arch}`, EXE).replace("app.asar", "app.asar.unpacked"),
  ].filter(Boolean);
  return candidates.find((p) => existsSync(p));
}

function newer(a, b) {
  const pa = a.split(".").map(Number), pb = b.split(".").map(Number);
  for (let i = 0; i < 3; i++) if ((pa[i] || 0) !== (pb[i] || 0)) return (pa[i] || 0) > (pb[i] || 0);
  return false;
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

class Connection extends EventEmitter {
  constructor(socket) {
    super();
    this.socket = socket;
    this.pending = [];
    let buf = "";
    socket.setEncoding("utf8");
    socket.on("data", (chunk) => {
      buf += chunk;
      let i;
      while ((i = buf.indexOf("\n")) >= 0) {
        const line = buf.slice(0, i).trim();
        buf = buf.slice(i + 1);
        if (!line) continue;
        let msg;
        try { msg = JSON.parse(line); } catch { continue; }
        if (msg.event) this.emit("event", msg);
        else this.pending.shift()?.resolve(msg);
      }
    });
    let closed = false;
    const fail = (err) => {
      if (closed) return;
      closed = true;
      for (const p of this.pending.splice(0)) p.reject(err || new Error("barkit-host disconnected"));
      this.emit("close");
    };
    socket.on("error", fail);
    socket.on("close", () => fail());
  }

  request(body, timeoutMs = 5000) {
    return new Promise((resolve, reject) => {
      const entry = { resolve, reject };
      const timer = setTimeout(() => {
        this.pending = this.pending.filter((p) => p !== entry);
        reject(new Error(`barkit-host did not answer "${body.op}"`));
      }, timeoutMs);
      entry.resolve = (v) => { clearTimeout(timer); resolve(v); };
      entry.reject = (e) => { clearTimeout(timer); reject(e); };
      this.pending.push(entry);
      this.socket.write(JSON.stringify(body) + "\n");
    });
  }

  close() { this.socket.end(); }
}

function connect() {
  return new Promise((resolve, reject) => {
    const s = net.connect(endpoint());
    s.once("connect", () => { s.removeAllListeners("error"); resolve(new Connection(s)); });
    s.once("error", reject);
  });
}

function installAndLaunch(hostPath) {
  // `install` copies the binary into the user's data dir and adds the login entry.
  const out = execFileSync(hostPath, ["install"], { encoding: "utf8" });
  const installed = out.trim().replace(/^installed to /, "");
  const child = spawn(installed, ["run"], { detached: true, stdio: "ignore", windowsHide: true });
  child.unref();
}

async function connectOrLaunch(opts) {
  const host = bundledHost(opts);
  let conn = await connect().catch(() => null);

  if (conn && host) {
    // Newest host wins: replace an older one that another app installed.
    const hello = await conn.request({ op: "hello" }).catch(() => null);
    if (hello?.version && newer(BUNDLED_HOST_VERSION, hello.version)) {
      await conn.request({ op: "shutdown" }).catch(() => {});
      conn.close();
      conn = null;
      await sleep(300);
    }
  }
  if (conn) return conn;
  if (!host) throw new Error("barkit-host is not running and no bundled host binary was found (set hostPath or BARKIT_HOST_PATH)");

  installAndLaunch(host);
  for (let i = 0; i < 50; i++) {
    await sleep(100);
    conn = await connect().catch(() => null);
    if (conn) return conn;
  }
  throw new Error("barkit-host did not start");
}

export class Barkit extends EventEmitter {
  /** @param {import('./index').StartOptions} opts */
  static async start(opts) {
    if (!opts?.appId || !opts?.key) throw new Error("Barkit.start needs appId and key");
    const bar = new Barkit(opts);
    await bar.#open();
    return bar;
  }

  #opts; #conn = null; #closed = false; #local = new Map();

  constructor(opts) {
    super();
    this.#opts = opts;
  }

  async #open() {
    const conn = await connectOrLaunch(this.#opts);
    conn.on("event", (e) => this.emit(e.event, e));
    conn.on("close", () => {
      this.#conn = null;
      if (!this.#closed) this.#reconnect();
    });
    const res = await conn.request({
      op: "register", app_id: this.#opts.appId, key: this.#opts.key,
      bundle_id: this.#opts.bundleId || "", server: this.#opts.server || "",
    });
    if (!res.ok) throw new Error(`barkit: ${res.error}`);
    this.#conn = conn;
    this.licensed = !!res.licensed;
    // Re-send local items after a reconnect (host restarted or was upgraded).
    for (const item of this.#local.values()) await conn.request({ op: "show", item });
  }

  async #reconnect() {
    for (let delay = 500; !this.#closed; delay = Math.min(delay * 2, 30000)) {
      await sleep(delay);
      try { await this.#open(); this.emit("reconnect"); return; } catch { /* keep trying */ }
    }
  }

  async #call(body) {
    if (!this.#conn) throw new Error("barkit-host is not connected");
    const res = await this.#conn.request(body);
    if (!res.ok) throw new Error(`barkit: ${res.error}`);
    return res;
  }

  /** Show your own item (status, news). Needs a valid licence to appear. */
  async show(item) {
    this.#local.set(item.id, item);
    return this.#call({ op: "show", item });
  }

  async clear() {
    this.#local.clear();
    return this.#call({ op: "clear" });
  }

  async status() { return this.#call({ op: "status" }); }

  /** Stop showing anything for this app, even after restarts. */
  async unregister() {
    const r = await this.#call({ op: "unregister" });
    this.close();
    return r;
  }

  /** Disconnect. The host keeps showing this app's server-managed items. */
  close() {
    this.#closed = true;
    this.#conn?.close();
  }
}

export default Barkit;
