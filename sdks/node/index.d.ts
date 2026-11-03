import { EventEmitter } from "node:events";

export interface StartOptions {
  /** Names your app on this computer: letters, digits, - and _. e.g. "my-app". */
  appId: string;
  /** Your app's own link schemes, so items can deep-link into it, e.g. ["myapp"]. */
  linkSchemes?: string[];
  /** Keep the host (and your pills) running after your app quits and across restarts. Default true. */
  startAtLogin?: boolean;
  /** Use a specific pillkit-host binary instead of the bundled one. */
  hostPath?: string;
}

export type Action =
  | { type: "open_url"; url: string }
  | { type: "deep_link"; url: string }
  | { type: "callback"; name: string }
  | { type: "card" }
  | { type: "none" };

export interface Item {
  /** Stable id; showing the same id again updates the item. */
  id: string;
  /** Bold first line, up to 40 characters. */
  title: string;
  /** Second line, up to 60 characters. */
  subtitle?: string;
  /** Small tag on the right, e.g. "Sponsored". Up to 16 characters. */
  label?: string;
  /** A .png file path, or base64 PNG data. Square works best. */
  icon?: string | { type: "png"; data: string } | { type: "monogram" };
  background?: string;
  foreground?: string;
  /** Opens on click: an https:// page, or a link into your app (declare its scheme in linkSchemes). */
  url?: string;
  /** Instead of a link: emits a "callback" event with this name when clicked. */
  onClick?: string;
  /** Text in the card that opens on hover. Defaults to the subtitle. */
  body?: string;
  /** Seconds on screen before the next item (5–300). Default 20. */
  seconds?: number;
  /** Higher shows more often (1–100). Default 1. */
  weight?: number;
  /** Full forms, if you prefer them to the shorthands above. */
  action?: Action;
  theme?: { background: string; foreground: string };
  card?: { body?: string; buttons?: { label: string; action: Action }[] };
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
  show(item: Item): Promise<{ ok: true }>;
  remove(id: string): Promise<void>;
  clear(): Promise<{ ok: true }>;
  status(): Promise<{ ok: true; local_items: number }>;
  unregister(): Promise<{ ok: true }>;
  close(): void;
  on(event: "callback", listener: (e: CallbackEvent) => void): this;
  on(event: "reconnect", listener: () => void): this;
}

export declare function dataDir(): string;
export declare function endpoint(): string;
export default PillKit;
