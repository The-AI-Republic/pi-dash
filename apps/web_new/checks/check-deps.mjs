// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runtime dependency allowlist check (Stack and dependencies).
// apps/web_new and packages/kit have an allowlist in CI: adding a runtime
// dependency needs review, with the reason and gzipped size in the PR.
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..", "..", "..");
const allowlist = JSON.parse(readFileSync(join(root, "apps", "web_new", "allowlist.json"), "utf8"));

const packages = {
  web_new: join(root, "apps", "web_new", "package.json"),
  "@pidash/kit": join(root, "packages", "kit", "package.json"),
  "@pidash/api-client": join(root, "packages", "api-client", "package.json"),
};

const failures = [];

for (const [key, manifestPath] of Object.entries(packages)) {
  const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
  const allowed = new Set(allowlist[key] ?? []);
  for (const dep of Object.keys(manifest.dependencies ?? {})) {
    if (!allowed.has(dep)) {
      failures.push(`${key}: runtime dependency "${dep}" is not in allowlist.json`);
    }
  }
}

if (failures.length > 0) {
  console.error("Dependency allowlist check failed:");
  for (const failure of failures) console.error(`  - ${failure}`);
  process.exit(1);
}
console.log("Dependency allowlist check passed.");
