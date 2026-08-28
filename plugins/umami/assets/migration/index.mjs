import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { readFile, stat } from "node:fs/promises";
import path from "node:path";

const MAX_OUTPUT = 32_000;
const TIMEOUT_MS = 14 * 60 * 1000;

function digest(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

async function verifyFile(relative, expected) {
  const bytes = await readFile(path.join(process.cwd(), relative));
  if (digest(bytes) !== expected) throw new Error(`controlled migration file failed verification: ${relative}`);
  return bytes;
}

async function verifyArtifact() {
  const control = JSON.parse(await readFile(new URL("./control.json", import.meta.url), "utf8"));
  await verifyFile("prisma/schema.prisma", control.schemaSha256);
  await verifyFile("prisma.config.ts", control.configSha256);
  for (const file of control.files) await verifyFile(file.path, file.sha256);
  const engine = await verifyFile("schema-engine", control.engine.sha256);
  if (engine.length !== control.engine.size) throw new Error("controlled migration engine has the wrong size");
  const metadata = await stat(path.join(process.cwd(), "schema-engine"));
  if ((metadata.mode & 0o111) === 0) throw new Error("controlled migration engine is not executable");
  return control;
}

export async function handler() {
  const databaseUrl = process.env.DIRECT_DATABASE_URL || process.env.DATABASE_URL;
  if (!databaseUrl) throw new Error("DATABASE_URL is required");
  await verifyArtifact();

  const chunks = [];
  let outputLength = 0;
  const append = chunk => {
    if (outputLength >= MAX_OUTPUT) return;
    const text = chunk.toString();
    chunks.push(text.slice(0, MAX_OUTPUT - outputLength));
    outputLength += text.length;
  };

  const code = await new Promise((resolve, reject) => {
    const child = spawn(process.execPath, ["node_modules/prisma/build/index.js", "migrate", "deploy", "--config", "prisma.config.ts"], {
      cwd: process.cwd(),
      env: {
        ...process.env,
        DATABASE_URL: databaseUrl,
        PRISMA_SCHEMA_ENGINE_BINARY: path.join(process.cwd(), "schema-engine"),
      },
      stdio: ["ignore", "pipe", "pipe"],
    });
    child.stdout.on("data", append);
    child.stderr.on("data", append);
    child.once("error", reject);
    const timeout = setTimeout(() => {
      child.kill("SIGTERM");
      reject(new Error("Prisma migration exceeded 14 minutes"));
    }, TIMEOUT_MS);
    timeout.unref();
    child.once("close", exitCode => {
      clearTimeout(timeout);
      resolve(exitCode);
    });
  });

  const output = chunks.join("").trim();
  if (output) console.log(output);
  if (code !== 0) throw new Error(`prisma migrate deploy exited ${code}`);
  return { ok: true };
}
