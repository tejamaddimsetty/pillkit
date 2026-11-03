// Sets one version on `pillkit`, its host dependencies and every
// pillkit-host-* package, so a release always ships matching pieces.
//   node scripts/set-npm-version.mjs 0.3.0
import { readFileSync, readdirSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const version = process.argv[2];
if (!/^\d+\.\d+\.\d+(-[\w.]+)?$/.test(version ?? "")) {
  console.error("usage: node scripts/set-npm-version.mjs <x.y.z>");
  process.exit(1);
}
const root = path.join(path.dirname(fileURLToPath(import.meta.url)), "..");
const update = (file, fn) => {
  const pkg = JSON.parse(readFileSync(file, "utf8"));
  fn(pkg);
  writeFileSync(file, JSON.stringify(pkg, null, 2) + "\n");
  console.log(`${pkg.name} -> ${version}`);
};

for (const dir of readdirSync(path.join(root, "npm"))) {
  update(path.join(root, "npm", dir, "package.json"), (p) => (p.version = version));
}
update(path.join(root, "sdks/node/package.json"), (p) => {
  p.version = version;
  for (const dep of Object.keys(p.optionalDependencies ?? {})) p.optionalDependencies[dep] = version;
});
