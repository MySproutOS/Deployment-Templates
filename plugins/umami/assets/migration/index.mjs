import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { readFile, stat } from "node:fs/promises";
import path from "node:path";
import bcrypt from "bcryptjs";
import pg from "pg";

const MAX_OUTPUT = 32_000;
const TIMEOUT_MS = 14 * 60 * 1000;
const SEEDED_ADMIN_ID = "41e2b680-648e-4b09-bcd7-3e2b10c06264";
const SEEDED_ADMIN_PASSWORD_HASH = "$2b$10$BUli0c.muyCW1ErNJc3jL.vFRFtFJWrT8/GcR4A.sUdCznaXiqFXa";
// Umami 3.3.1 writes cost 10; the controlled initial rotation writes cost 12.
const ACCEPTED_BCRYPT_HASH = /^\$2[ab]\$(?:10|12)\$[./A-Za-z0-9]{53}$/;
const { Client } = pg;

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

function initialAdminPassword() {
  const password = process.env.UMAMI_ADMIN_PASSWORD;
  if (!password || Buffer.byteLength(password, "utf8") < 32 || password === "umami") {
    throw new Error("UMAMI_ADMIN_PASSWORD must contain at least 32 bytes and cannot be the upstream default");
  }
  return password;
}

async function secureSeededAdministrator(databaseUrl, configuredPassword) {
  const client = new Client({ connectionString: databaseUrl });
  await client.connect();
  try {
    await client.query("BEGIN");
    const found = await client.query(
      'SELECT "password" FROM "user" WHERE "user_id" = $1 FOR UPDATE',
      [SEEDED_ADMIN_ID],
    );
    if (found.rowCount === 0) {
      // An owner may deliberately remove the seeded account after creating another administrator.
      await client.query("COMMIT");
      return;
    }
    if (found.rowCount !== 1 || typeof found.rows[0]?.password !== "string") {
      throw new Error("the seeded Umami administrator is in an unexpected state");
    }

    const currentHash = found.rows[0].password;
    if (currentHash !== SEEDED_ADMIN_PASSWORD_HASH) {
      if (!ACCEPTED_BCRYPT_HASH.test(currentHash)) {
        throw new Error("the Umami administrator has an unsupported password hash state");
      }
      // Never reset a valid owner-changed password on a later deployment. Still fail closed if
      // the owner explicitly changed it back to Umami's public default under a different salt.
      if (await bcrypt.compare("umami", currentHash)) {
        throw new Error("the Umami administrator still uses the public upstream default password");
      }
      await client.query("COMMIT");
      return;
    }

    const replacementHash = await bcrypt.hash(configuredPassword, 12);
    const updated = await client.query(
      'UPDATE "user" SET "password" = $1 WHERE "user_id" = $2 AND "password" = $3',
      [replacementHash, SEEDED_ADMIN_ID, SEEDED_ADMIN_PASSWORD_HASH],
    );
    if (updated.rowCount !== 1) {
      throw new Error("the seeded Umami administrator changed during credential rotation");
    }
    if (!(await bcrypt.compare(configuredPassword, replacementHash))) {
      throw new Error("the generated Umami administrator credential failed verification");
    }
    await client.query("COMMIT");
  } catch (error) {
    await client.query("ROLLBACK").catch(() => {});
    throw error;
  } finally {
    await client.end();
  }
}

export async function handler() {
  const databaseUrl = process.env.DIRECT_DATABASE_URL || process.env.DATABASE_URL;
  if (!databaseUrl) throw new Error("DATABASE_URL is required");
  const adminPassword = initialAdminPassword();
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
  await secureSeededAdministrator(databaseUrl, adminPassword);
  return { ok: true };
}
