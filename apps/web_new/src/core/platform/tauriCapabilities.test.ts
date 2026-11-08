// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Capability coverage pin (PIDESKAPP-183): every `plugin:` command tauri.ts
// invokes must be granted by desktop/src-tauri/capabilities/default.json.
// The parity mock (e2e/parity/desktop/tauri-mock.ts) never enforces the
// Tauri ACL, so a call like `plugin:opener|open_url` stays green in tests
// while the real shell rejects it and sign-in can never open the browser.
// This test reads the real source and the real capability file instead.
// Native commands (open_in_browser, desktop_api_*, managed_*, chat_*) need
// no capability entry and are not checked here.
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(here, "..", "..", "..", "..", "..");
const tauriSource = readFileSync(join(here, "tauri.ts"), "utf8");
const capability = JSON.parse(
  readFileSync(join(repoRoot, "desktop", "src-tauri", "capabilities", "default.json"), "utf8")
) as { permissions?: unknown };

const grants: string[] = Array.isArray(capability.permissions)
  ? capability.permissions.filter((entry): entry is string => typeof entry === "string")
  : [];

// Tauri v2 permission each plugin invoke needs (plugin build.rs: mutation
// commands are off unless granted; `core:default` covers read-only state
// only). Only `plugin:` invokes are listed — anything new must add a row.
const REQUIRED: Record<string, string> = {
  "plugin:opener|open_url": "opener:allow-open-url",
  "plugin:window|set_title": "core:window:allow-set-title",
};

// Invokes the shell may deny without user-visible harm: the native window
// title is best-effort (document.title wins), so a denial is a silent no-op.
// Everything else must be granted. A separate follow-up owns the missing
// `core:window:allow-set-title` grant; this issue fixes the opener path.
const BEST_EFFORT = new Set(["plugin:window|set_title"]);

function pluginInvokes(source: string): string[] {
  const found = new Set<string>();
  for (const match of source.matchAll(/invoke\(\s*"([^"]*plugin:[^"]*)"/g)) {
    const command = match[1];
    if (command !== undefined) found.add(command);
  }
  return [...found].sort();
}

describe("tauri plugin commands vs desktop capabilities", () => {
  it("grants every plugin command the desktop platform invokes", () => {
    const denied: string[] = [];
    for (const command of pluginInvokes(tauriSource)) {
      if (BEST_EFFORT.has(command)) continue;
      const permission = REQUIRED[command];
      if (permission === undefined) {
        denied.push(`${command}: unmapped (add its required permission or document an exception)`);
      } else if (!grants.includes(permission)) {
        denied.push(`${command}: needs ${permission}`);
      }
    }
    expect(denied).toEqual([]);
  });

  it("keeps the best-effort exception list exact", () => {
    const used = pluginInvokes(tauriSource);
    for (const command of BEST_EFFORT) {
      expect(used).toContain(command);
    }
  });

  it("opens URLs through the capability-free native command", () => {
    expect(pluginInvokes(tauriSource)).not.toContain("plugin:opener|open_url");
    expect(tauriSource).toContain('invoke("open_in_browser"');
  });
});
