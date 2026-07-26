#!/usr/bin/env node
/** Moved to clients/js/scripts/surfpool-lifecycle.mjs (needs @solana/kit resolution). */
import { spawnSync } from "node:child_process";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const r = spawnSync(
  process.execPath,
  ["--experimental-strip-types", resolve(root, "clients/js/scripts/surfpool-lifecycle.mjs")],
  { stdio: "inherit", cwd: resolve(root, "clients/js"), env: process.env },
);
process.exit(r.status ?? 1);
