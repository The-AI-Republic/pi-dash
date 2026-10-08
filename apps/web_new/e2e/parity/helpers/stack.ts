// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Instance-level stack controls for parity scenarios (NEWFRONT-111).
//
// A few rows (AUTH-041) exercise behavior gated on an instance-wide config
// flag that has no per-user or HTTP override in the OSS backend and whose
// read is served from a long-lived response cache. The only faithful way to
// drive the real UI for those rows is to flip the backing InstanceConfiguration
// row and bust the cache, then restore it. That is exactly what the seeded
// stack already does for its own bootstrap (manage.py shell inside the api
// container), so this helper does the same through docker compose. It lives in
// its own file so the HTTP-only helpers in api.ts stay free of any docker
// coupling.
//
// The flag is global, so a scenario using it must flip, assert, and restore
// within the smallest possible window (parity runs are serial), and always
// restore in a finally block.
import { execFileSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

function composeFile(): string {
  const override = process.env["PARITY_STACK_COMPOSE"];
  if (override && override.length > 0) return override;
  const here = dirname(fileURLToPath(import.meta.url));
  return join(here, "..", "stack", "docker-compose.yml");
}

function runInApi(python: string): string {
  try {
    return execFileSync(
      "docker",
      ["compose", "-f", composeFile(), "exec", "-T", "api", "python", "manage.py", "shell", "-c", python],
      { encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] }
    );
  } catch (error) {
    const detail = error instanceof Error ? error.message : String(error);
    throw new Error(`[parity] stack control failed (is the seeded stack up?): ${detail}`);
  }
}

/**
 * Set an instance configuration value and clear the response cache so the very
 * next `/api/instances/` read reflects it. Returns nothing; throws on failure.
 */
export function setInstanceConfig(key: string, value: string): void {
  const python = [
    "from pi_dash.license.models import InstanceConfiguration",
    "from django.core.cache import cache",
    `InstanceConfiguration.objects.update_or_create(key=${JSON.stringify(key)}, defaults={"value": ${JSON.stringify(value)}, "is_encrypted": False})`,
    "cache.clear()",
    'print("PARITY_STACK_OK")',
  ].join("\n");
  const out = runInApi(python);
  if (!out.includes("PARITY_STACK_OK")) {
    throw new Error(`[parity] setting ${key}=${value} did not confirm; output: ${out}`);
  }
}

/** Turn workspace creation off (true) or back on (false) instance-wide. */
export function setWorkspaceCreationDisabled(disabled: boolean): void {
  setInstanceConfig("DISABLE_WORKSPACE_CREATION", disabled ? "1" : "0");
}
