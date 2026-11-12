// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Capability coverage pin (PIDESKAPP-188): the native window title invoke
// must be granted by desktop/src-tauri/capabilities/default.json.
// desktopWindow.setTitle calls `plugin:window|set_title` and swallows
// failures with .catch(() => undefined), so an ACL denial is invisible in
// tests (the parity mock never enforces the Tauri ACL) but the OS title
// stays "Pi Dash" in the real desktop build. This test reads the real
// source and the real capability file instead of relying on the mock.
// Scoped to set_title only: the sibling plugin:opener|open_url gap belongs
// to PIDESKAPP-183, which routes opens through the capability-free
// open_in_browser command.
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(here, "..", "..", "..", "..", "..");
const tauriSource = readFileSync(join(here, "tauri.ts"), "utf8");
const capability = JSON.parse(
  readFileSync(join(repoRoot, "desktop", "src-tauri", "capabilities", "default.json"), "utf8")
) as { permissions?: unknown; windows?: unknown };

const grants: string[] = Array.isArray(capability.permissions)
  ? capability.permissions.filter((entry): entry is string => typeof entry === "string")
  : [];

// Tauri v2 permission the window title mutation requires (plugin build.rs:
// mutation commands are off unless granted; `core:default` covers read-only
// state only — allow-title is granted, allow-set-title is not).
const REQUIRED = "core:window:allow-set-title";

describe("tauri window title capability", () => {
  it("invokes the native window title from the desktop platform", () => {
    expect(tauriSource).toContain('invoke("plugin:window|set_title"');
  });

  it("grants the set_title command for the main window", () => {
    expect(grants).toContain(REQUIRED);
    const windows = Array.isArray(capability.windows) ? capability.windows : [];
    expect(windows).toContain("main");
  });
});
