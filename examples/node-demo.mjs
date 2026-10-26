// node examples/node-demo.mjs APP_ID KEY [SERVER]
import { PillKit } from "../sdks/node/index.js";

const [appId, key, server = "http://127.0.0.1:5050"] = process.argv.slice(2);
const bar = await PillKit.start({ appId, key, server, hostPath: process.env.PILLKIT_HOST_PATH });
console.log("registered; licensed so far:", bar.licensed);

await bar.show({
  id: "build-status",
  title: "Build passed",
  subtitle: "main · 2 minutes ago",
  label: "",
  theme: { background: "#1F7A4D", foreground: "#FFFFFF" },
  action: { type: "callback", name: "open-build" },
  duration_secs: 10,
});
bar.on("callback", (e) => console.log("user clicked:", e.name));

setInterval(() => bar.status().then(console.log, (e) => console.log("waiting for host:", e.message)), 5000);
