const { contextBridge, ipcRenderer } = require("electron");
contextBridge.exposeInMainWorld("pillkit", { bump: () => ipcRenderer.invoke("bump") });
