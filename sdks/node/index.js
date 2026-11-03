// @pillkit/sdk: put your app's notifications in the macOS menu bar and the
// Windows taskbar. Talks to the per-user pillkit-host over a local socket
// (docs/PROTOCOL.md §5), starting it on first use. Local only: no server,
// account or network.

import { spawn, execFileSync } from "node:child_process";
import { EventEmitter } from "node:events";
import { existsSync, readFileSync } from "node:fs";
import { createRequire } from "node:module";
import net from "node:net";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const require = createRequire(import.meta.url);
const pkg = JSON.parse(readFileSync(path.join(here, "package.json"), "utf8"));
const BUNDLED_HOST_VERSION = pkg.version;
const EXE = process.platform === "win32" ? "pillkit-host.exe" : "pillkit-host";
const APP_ID = /^[A-Za-z0-9_-]{1,64}$/;

export function dataDir() {
  if (process.platform === "darwin") return path.join(os.homedir(), "Library", "Application Support", "pillkit");
  if (process.platform === "win32") return path.join(process.env.LOCALAPPDATA || os.homedir(), "pillkit");
  return path.join(process.env.XDG_DATA_HOME || path.join(os.homedir(), ".local", "share"), "pillkit");
}

export function endpoint() {
  if (process.platform === "win32") {
    const user = (process.env.USERNAME || "user").replace(/[^A-Za-z0-9_-]/g, "");
    return `\\\\.\\pipe\\pillkit-host-${user}`;
  }
  return path.join(dataDir(), "host.sock");
}

/** The host binary for this computer, from @pillkit/host-<os>-<arch>. */
function bundledHost(opts) {
  const fromPackage = () => {
    try {
      return require.resolve(`@pillkit/host-${process.platform}-${process.arch}/bin/${EXE}`);
    } catch {
      return null;
    }
  };
  const candidates = [opts.hostPath, process.env.PILLKIT_HOST_PATH, fromPackage()]
    .filter(Boolean)
    // Electron: binaries must live outside the asar archive (asarUnpack).
    .map((p) => p.replace(`app.asar${path.sep}`, `app.asar.unpacked${path.sep}`));
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
      for (const p of this.pending.splice(0)) p.reject(err || new Error("pillkit-host disconnected"));
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
        reject(new Error(`pillkit-host did not answer "${body.op}"`));
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

function launch(hostPath, startAtLogin) {
  let exe = hostPath;
  if (startAtLogin) {
    // Copies the host into the user's data folder and adds a login item, so
    // pills keep showing after your app quits and after a restart.
    execFileSync(hostPath, ["install"], { stdio: "ignore", windowsHide: true });
    exe = path.join(dataDir(), "bin", EXE);
  }
  spawn(exe, ["run"], { detached: true, stdio: "ignore", windowsHide: true }).unref();
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
  if (!host) {
    throw new Error(
      `pillkit: no host for ${process.platform}-${process.arch}. Reinstall @pillkit/sdk, or set hostPath / PILLKIT_HOST_PATH.`,
    );
  }
  launch(host, opts.startAtLogin !== false);
  for (let i = 0; i < 50; i++) {
    await sleep(100);
    conn = await connect().catch(() => null);
    if (conn) return conn;
  }
  throw new Error("pillkit: the host did not start");
}

/** Turns the friendly item shape into the wire format. */
function toWire(item) {
  if (!item?.id || !item?.title) throw new Error("pillkit: an item needs an id and a title");
  const wire = {
    id: String(item.id),
    title: String(item.title),
    subtitle: item.subtitle ?? "",
    label: item.label ?? "",
    theme: item.theme ?? {
      background: item.background ?? "#E8F04A",
      foreground: item.foreground ?? "#2A2C05",
    },
    duration_secs: item.seconds ?? item.duration_secs ?? 20,
    weight: item.weight ?? 1,
  };
  if (item.action) wire.action = item.action;
  else if (item.url) {
    const url = String(item.url);
    wire.action = { type: /^https:\/\//i.test(url) ? "open_url" : "deep_link", url };
  } else if (item.onClick) wire.action = { type: "callback", name: String(item.onClick) };
  if (item.card || item.body) wire.card = item.card ?? { body: String(item.body), buttons: [] };
  if (typeof item.icon === "string") {
    const data = item.icon.endsWith(".png") ? readFileSync(item.icon).toString("base64") : item.icon;
    wire.icon = { type: "png", data };
  } else if (item.icon) wire.icon = item.icon;
  return wire;
}

export class PillKit extends EventEmitter {
  /** @param {import('./index').StartOptions} opts */
  static async start(opts) {
    if (!APP_ID.test(opts?.appId ?? "")) {
      throw new Error("pillkit: appId is required (letters, digits, - and _, up to 64), e.g. \"my-app\"");
    }
    const pill = new PillKit(opts);
    await pill.#open();
    return pill;
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
      op: "register",
      app_id: this.#opts.appId,
      link_schemes: this.#opts.linkSchemes ?? [],
    });
    if (!res.ok) throw new Error(`pillkit: ${res.error}`);
    this.#conn = conn;
    // Re-send items after a reconnect (the host restarted or was upgraded).
    for (const item of this.#local.values()) await conn.request({ op: "show", item });
  }

  async #reconnect() {
    for (let delay = 500; !this.#closed; delay = Math.min(delay * 2, 30000)) {
      await sleep(delay);
      try { await this.#open(); this.emit("reconnect"); return; } catch { /* keep trying */ }
    }
  }

  async #call(body) {
    if (!this.#conn) throw new Error("pillkit: not connected to the host");
    const res = await this.#conn.request(body);
    if (!res.ok) throw new Error(`pillkit: ${res.error}`);
    return res;
  }

  /** Shows an item next to the clock (or updates it, if the id exists). */
  async show(item) {
    const wire = toWire(item);
    const res = await this.#call({ op: "show", item: wire });
    this.#local.set(wire.id, wire);
    return res;
  }

  /** Removes one item. */
  async remove(id) {
    this.#local.delete(String(id));
    await this.#call({ op: "clear" });
    for (const item of this.#local.values()) await this.#call({ op: "show", item });
  }

  /** Removes all of this app's items. */
  async clear() {
    this.#local.clear();
    return this.#call({ op: "clear" });
  }

  async status() { return this.#call({ op: "status" }); }

  /** Removes the app and its items from the host for good. */
  async unregister() {
    const r = await this.#call({ op: "unregister" });
    this.close();
    return r;
  }

  /** Disconnects. Items already shown stay until cleared. */
  close() {
    this.#closed = true;
    this.#conn?.close();
  }
}

export default PillKit;
