// Minimal Electron app using Pill Kit: a window with a button that updates
// the pill, and a pill click that brings the window back.
import { app, BrowserWindow, ipcMain } from "electron";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { PillKit } from "pillkit";

const here = path.dirname(fileURLToPath(import.meta.url));
let win;
let pill;
let clicks = 0;

app.whenReady().then(async () => {
  win = new BrowserWindow({
    width: 420,
    height: 260,
    webPreferences: { preload: path.join(here, "preload.cjs") },
  });
  win.loadFile(path.join(here, "index.html"));

  // Only stays up while this demo runs; real apps usually keep the default.
  pill = await PillKit.start({ appId: "electron-hello", startAtLogin: false });
  await pill.show({
    id: "hello",
    title: "Hello from Electron",
    subtitle: "Click me to open the window",
    onClick: "open-window",
  });

  pill.on("callback", (e) => {
    if (e.name === "open-window") {
      win.show();
      win.focus();
    }
  });
});

ipcMain.handle("bump", async () => {
  clicks += 1;
  await pill.show({
    id: "hello",
    title: `Button pressed ${clicks}×`,
    subtitle: "Updated from the app",
    background: "#1F7A4D",
    foreground: "#FFFFFF",
    onClick: "open-window",
  });
  return clicks;
});

app.on("window-all-closed", async () => {
  await pill?.clear().catch(() => {});
  pill?.close();
  app.quit();
});
