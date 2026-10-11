// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the NEWFRONT-265 seed-settling block appended to ./api.ts:
// the seed-project match predicate and the Django cleanup-snippet builder.
// Both are pure, so these tests pin them without a stack; the polling and
// drain helpers themselves are exercised by the oracle specs that call them.
import { describe, expect, it } from "vitest";

import { isWorkspaceSeedProject, workspaceSeedCleanupPython } from "./api.js";

describe("isWorkspaceSeedProject", () => {
  it("matches the project named after the workspace", () => {
    expect(isWorkspaceSeedProject("Empty WS abc123", "Empty WS abc123")).toBe(true);
  });

  it("does not match the seed file's demo title", () => {
    // The seed file calls its project "Pi Dash Demo Project", but the seed
    // task discards that title and names the created project after the
    // workspace instead: matching the demo title would never drain.
    expect(isWorkspaceSeedProject("Pi Dash Demo Project", "Empty WS abc123")).toBe(false);
  });

  it("does not match scenario projects", () => {
    expect(isWorkspaceSeedProject("Real Project", "Empty WS abc123")).toBe(false);
  });

  it("is exact: case and whitespace variants do not match", () => {
    expect(isWorkspaceSeedProject("empty ws abc123", "Empty WS abc123")).toBe(false);
    expect(isWorkspaceSeedProject("Empty WS abc123 ", "Empty WS abc123")).toBe(false);
  });
});

describe("workspaceSeedCleanupPython", () => {
  const snippet = workspaceSeedCleanupPython("empty-abc123", "11111111-2222-3333-4444-555555555555");

  it("soft-deletes the seed project by id scoped to the workspace", () => {
    expect(snippet).toContain(
      'proj = Project.objects.filter(id="11111111-2222-3333-4444-555555555555", workspace=ws).first()'
    );
    expect(snippet).toContain("proj.delete()");
  });

  it("removes the bot's workspace-level pages, membership and user", () => {
    expect(snippet).toContain("Page.objects.filter(workspace=ws)");
    expect(snippet).toContain("members = WorkspaceMember.objects.filter(workspace=ws, member=bot).delete()");
    // The bot user has no soft-delete: a plain queryset delete returning
    // the (total, per-model) tuple, which the snippet unpacks.
    expect(snippet).toContain("users, _ = User.objects.filter(id=bot.id).delete()");
  });

  it("prints the drain marker with numeric counts", () => {
    expect(snippet).toContain("PARITY_SEED_DRAIN:");
  });

  it("escapes its inputs", () => {
    const nasty = workspaceSeedCleanupPython('a"b\\c', "id");
    expect(nasty).toContain('Workspace.objects.get(slug="a\\"b\\\\c")');
    expect(nasty).not.toContain('slug="a"b');
  });
});
