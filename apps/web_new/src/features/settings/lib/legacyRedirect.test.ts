// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Legacy forwards land inside a workspace (parity/specs/profile.md §5,
// PROF-026; main#670): the current workspace for the workspace-less
// families, the named one for the workspace families, the query string
// kept in both, and the creation flow when the reader belongs nowhere.
import type { WorkspaceList } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import { CREATE_WORKSPACE_PATH } from "../../../core/session/index.js";
import { legacyCurrentWorkspaceSlug, legacySettingsRedirect } from "./legacyRedirect.js";

function workspace(slug: string): WorkspaceList[number] {
  return { id: `id-${slug}`, name: slug, slug, logo_url: null, total_members: 1, role: 20 };
}

const WORKSPACES: WorkspaceList = [workspace("acme"), workspace("globex")];

describe("legacyCurrentWorkspaceSlug", () => {
  it("prefers the last workspace while the reader still belongs to it", () => {
    expect(legacyCurrentWorkspaceSlug({ lastSlug: "globex", fallbackSlug: "acme", workspaces: WORKSPACES })).toBe(
      "globex"
    );
  });

  it("falls back to the remembered workspace when the last one is gone", () => {
    expect(legacyCurrentWorkspaceSlug({ lastSlug: "initech", fallbackSlug: "acme", workspaces: WORKSPACES })).toBe(
      "acme"
    );
    expect(legacyCurrentWorkspaceSlug({ lastSlug: null, fallbackSlug: "globex", workspaces: WORKSPACES })).toBe(
      "globex"
    );
  });

  it("answers null when neither candidate is a workspace the reader belongs to", () => {
    expect(legacyCurrentWorkspaceSlug({ lastSlug: "initech", fallbackSlug: null, workspaces: WORKSPACES })).toBeNull();
    expect(legacyCurrentWorkspaceSlug({ lastSlug: "acme", fallbackSlug: null, workspaces: undefined })).toBeNull();
  });
});

describe("legacySettingsRedirect", () => {
  const current = { lastSlug: "acme", fallbackSlug: null, workspaces: WORKSPACES };

  it("forwards the bare settings root to the current workspace, keeping the query", () => {
    expect(legacySettingsRedirect("/settings", "", current)).toBe("/acme/settings/account/general/");
    expect(legacySettingsRedirect("/settings/", "?keep=1&other=2", current)).toBe(
      "/acme/settings/account/general/?keep=1&other=2"
    );
  });

  it("forwards the legacy profile settings family per tab", () => {
    expect(legacySettingsRedirect("/settings/profile/security", "", current)).toBe("/acme/settings/account/security/");
    // Only a real tab is forwarded as one.
    expect(legacySettingsRedirect("/settings/profile/bogus-tab-id", "", current)).toBe(
      "/acme/settings/account/general/"
    );
    expect(legacySettingsRedirect("/settings/profile/bogus-tab-id", "?x=1", current)).toBe(
      "/acme/settings/account/general/?x=1"
    );
  });

  it("forwards the older /profile family per tab", () => {
    expect(legacySettingsRedirect("/profile/preferences", "?keep=4", current)).toBe(
      "/acme/settings/account/preferences/?keep=4"
    );
    expect(legacySettingsRedirect("/profile", "", current)).toBe("/acme/settings/account/general/");
  });

  it("forwards a workspace account address inside that same workspace", () => {
    expect(legacySettingsRedirect("/globex/settings/account/security", "?keep=2", current)).toBe(
      "/globex/settings/account/security/?keep=2"
    );
    // The bare address lands on that workspace's general tab even when it
    // is not the current one, and unknown ids default there too.
    expect(legacySettingsRedirect("/globex/settings/account", "", current)).toBe("/globex/settings/account/general/");
    expect(legacySettingsRedirect("/globex/settings/account/bogus", "", current)).toBe(
      "/globex/settings/account/general/"
    );
  });

  it("forwards the workspace tokens address with its parameters (NEWFRONT-431 target)", () => {
    expect(legacySettingsRedirect("/globex/settings/api-tokens", "?keep=3", current)).toBe(
      "/globex/settings/account/api-tokens/?keep=3"
    );
  });

  it("sends a reader who belongs nowhere to workspace creation", () => {
    expect(legacySettingsRedirect("/settings", "", { lastSlug: null, fallbackSlug: null, workspaces: [] })).toBe(
      CREATE_WORKSPACE_PATH
    );
  });

  it("answers null outside the families", () => {
    expect(legacySettingsRedirect("/acme/projects/p1", "", current)).toBeNull();
    expect(legacySettingsRedirect("/profile/a/b", "", current)).toBeNull();
    expect(legacySettingsRedirect("/acme/settings/members", "", current)).toBeNull();
  });
});
