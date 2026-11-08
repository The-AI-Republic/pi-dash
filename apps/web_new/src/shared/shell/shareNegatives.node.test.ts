// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Source-level absence pin (PIDESKAPP-187, DESK-019): no copy/display
// surface may read the window's origin or full address to build a
// shareable link. On the desktop bundle that origin is the private
// tauri origin, so every emitted address in these files must go
// through the platform (`platform.shareableUrl` via shareLink.js).
// Fixtures below prove each token family flags real usage.
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const here = dirname(fileURLToPath(import.meta.url));
const SRC = join(here, "..", "..", "..", "src");

/** Comment-free body: prose may name the absence, code may not. */
function scanBody(source: string): string {
  return source.replace(/\/\*[\s\S]*?\*\//g, "").replace(/(^|[\s;])\/\/[^\n]*/g, "$1");
}

function hits(source: string, tokens: RegExp[]): string[] {
  return tokens.filter((token) => token.test(source)).map((token) => token.source);
}

/**
 * Reads of the window's origin or full address. Path-only reads
 * (`location.pathname`, `search`, `hash`) are allowed: a path alone
 * cannot emit a dead absolute address once the origin comes from the
 * platform.
 */
const WINDOW_ADDRESS_TOKENS = [/\blocation\.origin\b/, /\blocation\.href\b/];

const FILES = [
  "features/pages/lib/share.ts",
  "features/issues/widgets/RelationRow.tsx",
  "shared/shell/clipboard.ts",
  "shared/shell/contextual/hooks.tsx",
  "features/spaces/peek/SpaceIssuePeek.tsx",
  "features/workspace-settings/members/MembersPage.tsx",
  "features/workspace-settings/members/PendingInvitations.tsx",
  "features/workspace-settings/general/WorkspaceGeneralPage.tsx",
  "features/workspace-settings/general/components/WorkspaceIdentityCard.tsx",
  "features/workspace-settings/general/components/WorkspaceDetailsForm.tsx",
  "features/admin/components/WorkspaceCreateForm.tsx",
  "features/projects/api/publish.ts",
  "features/projects/components/PublishProjectDialog.tsx",
];

describe("share negatives (PIDESKAPP-187)", () => {
  it("flags a window-origin read in a fixture", () => {
    const fixture = "const address = new URL(path, window.location.origin).toString();";
    expect(hits(scanBody(fixture), WINDOW_ADDRESS_TOKENS).length).toBeGreaterThan(0);
  });

  it("flags a window-href read in a fixture", () => {
    const fixture = "await navigator.clipboard.writeText(window.location.href);";
    expect(hits(scanBody(fixture), WINDOW_ADDRESS_TOKENS).length).toBeGreaterThan(0);
  });

  it("passes path-only reads", () => {
    const fixture = "const path = `${location.pathname}${location.search}`; platform.shareableUrl(path);";
    expect(hits(scanBody(fixture), WINDOW_ADDRESS_TOKENS)).toEqual([]);
  });

  for (const file of FILES) {
    it(`${file} emits no window-origin address`, () => {
      expect(hits(scanBody(readFileSync(join(SRC, file), "utf8")), WINDOW_ADDRESS_TOKENS)).toEqual([]);
    });
  }
});
