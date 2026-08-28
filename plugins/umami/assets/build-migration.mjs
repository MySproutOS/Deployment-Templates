import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { chmod, copyFile, cp, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import path from "node:path";
import { gunzipSync } from "node:zlib";

const source = "sproutos/migration";
const output = ".sproutos/build/migration";

function digest(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

async function verified(relative, expected) {
  const bytes = await readFile(relative);
  if (digest(bytes) !== expected) throw new Error(`pinned migration source changed: ${relative}`);
  return bytes;
}

if (process.platform !== "linux" || process.arch !== "arm64") {
  throw new Error(`Umami migration artifact must be built on Linux ARM64, received ${process.platform}/${process.arch}`);
}

const control = JSON.parse(await readFile(`${source}/control.json`, "utf8"));
if (control.upstreamCommit !== "ca661c7057984aa98ed4f7083d84dae2f65bfcb0" || control.prismaVersion !== "7.9.1") {
  throw new Error("migration control manifest is incompatible with this recipe");
}
await verified("prisma/schema.prisma", control.schemaSha256);
await verified("prisma.config.ts", control.configSha256);
for (const file of control.files) await verified(file.path, file.sha256);

await rm(output, { recursive: true, force: true });
await mkdir(output, { recursive: true });
for (const name of ["index.mjs", "control.json", "npm-shrinkwrap.json"]) {
  await copyFile(`${source}/${name}`, `${output}/${name}`);
}
// Keep this npm-only manifest out of Umami's `packages: ['**']` pnpm workspace.
// It becomes package.json only inside the isolated migration build artifact.
await copyFile(`${source}/package.build.json`, `${output}/package.json`);
await copyFile("prisma.config.ts", `${output}/prisma.config.ts`);
await cp("prisma", `${output}/prisma`, { recursive: true });

const install = spawnSync("npm", ["ci", "--omit=dev", "--ignore-scripts", "--no-audit", "--no-fund"], {
  cwd: output,
  stdio: "inherit",
});
if (install.error) throw install.error;
if (install.status !== 0) throw new Error(`npm ci exited ${install.status}`);

const response = await fetch(control.engine.url);
if (!response.ok) throw new Error(`Prisma engine download returned ${response.status}`);
const compressed = Buffer.from(await response.arrayBuffer());
if (digest(compressed) !== control.engine.compressedSha256) throw new Error("compressed Prisma engine failed verification");
const engine = gunzipSync(compressed);
if (digest(engine) !== control.engine.sha256 || engine.length !== control.engine.size) {
  throw new Error("Prisma engine failed verification");
}
await writeFile(`${output}/schema-engine`, engine, { mode: 0o755 });
await chmod(`${output}/schema-engine`, 0o755);

console.log(`built controlled Umami migrator in ${path.resolve(output)}`);
