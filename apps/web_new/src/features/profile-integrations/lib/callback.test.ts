// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Install-callback parsing (parity/specs/profile.md §11, PROF-025): the
// host's result marker becomes a notice, the named workspace becomes the
// selection, and only the marker keys leave the address.
import { describe, expect, it } from "vitest";

import { CALLBACK_FAILED_FALLBACK } from "./copy.js";
import { readInstallCallback, scrubInstallCallback, scrubbedAddress } from "./callback.js";

describe("readInstallCallback", () => {
  it("ignores an address with no marker", () => {
    expect(readInstallCallback("")).toBeNull();
    expect(readInstallCallback("?tab=integrations")).toBeNull();
    expect(readInstallCallback("?github_app=")).toBeNull();
  });

  it("reads a success with the named workspace", () => {
    expect(readInstallCallback("?github_app=connected&workspace_slug=acme")).toEqual({
      ok: true,
      workspaceSlug: "acme",
      message: "",
    });
  });

  it("treats a blank workspace as unnamed", () => {
    expect(readInstallCallback("?github_app=connected&workspace_slug=")).toEqual({
      ok: true,
      workspaceSlug: null,
      message: "",
    });
    expect(readInstallCallback("?github_app=connected")?.workspaceSlug).toBeNull();
  });

  it("words each known failure code", () => {
    expect(readInstallCallback("?github_app=failed&error=actor_mismatch")).toEqual({
      ok: false,
      workspaceSlug: null,
      message: "This install session was started by a different Pi Dash user.",
    });
    expect(readInstallCallback("?github_app=failed&error=workspace_admin_required")?.message).toBe(
      "You must be a workspace admin to connect GitHub."
    );
    expect(readInstallCallback("?github_app=failed&error=login_required")?.message).toBe(
      "Sign in to Pi Dash and start the GitHub App install again."
    );
  });

  it("falls back for an unknown code, and reads the result itself as the code", () => {
    expect(readInstallCallback("?github_app=failed&error=other")?.message).toBe(CALLBACK_FAILED_FALLBACK);
    expect(readInstallCallback("?github_app=cancelled")).toEqual({
      ok: false,
      workspaceSlug: null,
      message: CALLBACK_FAILED_FALLBACK,
    });
  });
});

describe("scrubInstallCallback", () => {
  it("removes the marker keys and keeps everything else", () => {
    expect(scrubInstallCallback("?github_app=connected&workspace_slug=acme&tab=integrations&keep=1")).toBe(
      "tab=integrations&keep=1"
    );
  });

  it("removes the error key too", () => {
    expect(scrubInstallCallback("?error=login_required&github_app=failed")).toBe("");
  });
});

describe("scrubbedAddress", () => {
  it("returns the bare path when nothing else was there", () => {
    expect(scrubbedAddress("/settings/integrations", "?github_app=connected&workspace_slug=acme")).toBe(
      "/settings/integrations"
    );
  });

  it("keeps the remaining query and the hash", () => {
    expect(scrubbedAddress("/settings/integrations", "?github_app=failed&error=x&tab=integrations", "#github")).toBe(
      "/settings/integrations?tab=integrations#github"
    );
  });

  it("keeps an empty search as an empty search", () => {
    expect(scrubbedAddress("/a", "")).toBe("/a");
  });
});
