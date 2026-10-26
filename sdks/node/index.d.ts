import { EventEmitter } from "node:events";

export interface StartOptions {
  appId: string;
  /** Publishable key (pk_...). Safe to ship in your app. */
  key: string;
  /** Must match the bundle ID registered in the portal, if you set one there. */
  bundleId?: string;
  /** pillkit server, defaults to https://api.pillkit.dev */
  server?: string;
  /** Path to a pillkit-host binary, if you don't use the bundled one. */
  hostPath?: string;
}

export type Action =
  | { type: "open_url"; url: string }
  | { type: "deep_link"; url: string }
  | { type: "callback"; name: string }
  | { type: "card" }
  | { type: "none" };

export interface Item {
  id: string;
  title: string;
  subtitle?: string;
  label?: string;
  icon?: { type: "monogram" } | { type: "png"; data: string };
  theme?: { background: string; foreground: string };
  action?: Action;
  card?: { body?: string; buttons?: { label: string; action: Action }[] };
  weight?: number;
  duration_secs?: number;
  starts_at?: number | null;
  ends_at?: number | null;
}

export interface CallbackEvent {
  event: "callback";
  item_id: string;
  name: string;
}

export declare class PillKit extends EventEmitter {
  static start(opts: StartOptions): Promise<PillKit>;
  licensed: boolean;
  show(item: Item): Promise<{ ok: true; licensed: boolean }>;
  clear(): Promise<{ ok: true }>;
  status(): Promise<{ ok: true; licensed: boolean; plan: string | null; feed_items: number; local_items: number; error: string | null }>;
  unregister(): Promise<{ ok: true }>;
  close(): void;
  on(event: "callback", listener: (e: CallbackEvent) => void): this;
  on(event: "reconnect", listener: () => void): this;
}

export declare function dataDir(): string;
export declare function endpoint(): string;
export default PillKit;
