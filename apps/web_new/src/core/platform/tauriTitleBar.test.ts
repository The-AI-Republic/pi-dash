// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Title-bar / shell coherence (PIDESKAPP-192): the web_new header must match
// the shell the Tauri window builder actually creates. The OS window has a
// standard native title bar (main.rs sets only title/size/resizable, with no
// TitleBarStyle::Overlay or hidden title), so the frontend must not reserve a
// macOS traffic-light inset for lights drawn over the page, and must not
// claim a drag region the capability does not grant (start_dragging).
// Reads the real sources instead of the parity mock, which never enforces
// the Tauri ACL or the OS decoration choice.
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(here, "..", "..", "..", "..", "..");
const tauriSource = readFileSync(join(here, "tauri.ts"), "utf8");
const titleBarSource = readFileSync(join(here, "..", "..", "shared", "shell", "TitleBar.tsx"), "utf8");
const topBarSource = readFileSync(join(here, "..", "..", "shared", "shell", "TopBar.tsx"), "utf8");
const mainRust = readFileSync(join(repoRoot, "desktop", "src-tauri", "src", "main.rs"), "utf8");
const capability = JSON.parse(
  readFileSync(join(repoRoot, "desktop", "src-tauri", "capabilities", "default.json"), "utf8")
) as { permissions?: unknown };

const grants: string[] = Array.isArray(capability.permissions)
  ? capability.permissions.filter((entry): entry is string => typeof entry === "string")
  : [];

const shellIsOverlay =
  mainRust.includes("TitleBarStyle") ||
  mainRust.includes("title_bar_style") ||
  mainRust.includes("hidden_title") ||
  mainRust.includes("HiddenTitle");

describe("desktop title bar matches the shell window", () => {
  it("leaves no traffic-light inset when the shell keeps the native title bar", () => {
    if (shellIsOverlay) return;
    expect(tauriSource).toContain("trafficLightInset: false");
    expect(tauriSource).not.toContain("trafficLightInset: isMacOs()");
  });

  it("does not claim a drag region the capability does not grant", () => {
    const claimsDrag =
      titleBarSource.includes("data-tauri-drag-region") || topBarSource.includes("data-tauri-drag-region");
    expect(claimsDrag && !grants.includes("core:window:allow-start-dragging")).toBe(false);
  });

  it("does not make header text unselectable without a working drag", () => {
    const claimsDrag =
      titleBarSource.includes("data-tauri-drag-region") || topBarSource.includes("data-tauri-drag-region");
    const unselectable = titleBarSource.includes("select-none") || topBarSource.includes("select-none");
    const dragWorks = claimsDrag && grants.includes("core:window:allow-start-dragging");
    expect(unselectable && !dragWorks).toBe(false);
  });
});
