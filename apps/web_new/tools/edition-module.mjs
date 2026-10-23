// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Edition module resolution (maintainer decision 2026-10-18, NEWFRONT-662).
// The cloud build points the single @pidash/edition alias at the private
// module (ee-overlay/apps/web_new/src/core/edition/cloud.ts) through
// PIDASH_EDITION_MODULE; unset means the OSS module in this repo. Both
// vite.config.ts and vitest.config.ts resolve through here so dev, build,
// test and the prerender step (which boots its own Vite server) agree.
import { existsSync } from "node:fs";
import { dirname, resolve, sep } from "node:path";

/** The OSS edition module every build uses unless overridden. */
export function defaultEditionModule(appDir) {
  return resolve(appDir, "src/core/edition/oss.ts");
}

/**
 * Resolve the @pidash/edition alias target for an app dir.
 *
 * The override may be absolute or relative to the app dir, and must name
 * an existing file — a typo fails here with the resolved path instead of
 * a cryptic resolver error deep in the build.
 *
 * `fsAllow` is null when the module lives inside the repo (Vite serves
 * the workspace root by default) or `[repoRoot, moduleDir]` when it
 * lives outside it (the private checkout), so `vite dev` may serve it.
 */
export function resolveEditionModule(env, appDir) {
  const override = (env.PIDASH_EDITION_MODULE ?? "").trim();
  const modulePath = override === "" ? defaultEditionModule(appDir) : resolve(appDir, override);
  if (override !== "" && !existsSync(modulePath)) {
    throw new Error(`[edition] PIDASH_EDITION_MODULE points at a missing file: ${modulePath}`);
  }
  const repoRoot = resolve(appDir, "..", "..");
  const inside = modulePath === repoRoot || modulePath.startsWith(repoRoot + sep);
  return { modulePath, fsAllow: inside ? null : [repoRoot, dirname(modulePath)] };
}
