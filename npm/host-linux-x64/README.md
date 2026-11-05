# pillkit-host-linux-x64

The Pill Kit host binary for **linux-x64**. You don't need to install this yourself:
it is installed automatically as an optional dependency of
[`pillkit`](https://www.npmjs.com/package/pillkit), and npm only downloads the
one that matches your computer.

```bash
npm install pillkit
```

```js
import { PillKit } from "pillkit";

const pill = await PillKit.start({ appId: "my-app" });
await pill.show({ id: "hello", title: "Hello from my app", url: "https://example.com" });
```

Guide: [docs/INTEGRATION.md](https://github.com/tejamaddimsetty/pillkit/blob/main/docs/INTEGRATION.md)
· Source: [github.com/tejamaddimsetty/pillkit](https://github.com/tejamaddimsetty/pillkit)
· License: Apache-2.0
