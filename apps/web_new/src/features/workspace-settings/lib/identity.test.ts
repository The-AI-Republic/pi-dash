// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Identity fallback chain (WSET-001): details, then the listed row,
// then the slug; the first logo leg that is present wins.
import { describe, expect, it } from "vitest";

import { resolveWorkspaceSettingsIdentity } from "./identity.js";

describe("resolveWorkspaceSettingsIdentity", () => {
  it("prefers the details read", () => {
    expect(
      resolveWorkspaceSettingsIdentity({
        workspaceSlug: "acme",
        details: { name: "Acme Details", logo_url: "/details.png" },
        listed: { name: "Acme Listed", logo_url: "/listed.png" },
      })
    ).toEqual({ workspaceName: "Acme Details", logoLeg: "/details.png" });
  });

  it("falls back to the listed row while the details read is absent", () => {
    expect(
      resolveWorkspaceSettingsIdentity({
        workspaceSlug: "acme",
        details: undefined,
        listed: { name: "Acme Listed", logo_url: "/listed.png" },
      })
    ).toEqual({ workspaceName: "Acme Listed", logoLeg: "/listed.png" });
  });

  it("uses the slug and no logo when neither read provides one", () => {
    expect(resolveWorkspaceSettingsIdentity({ workspaceSlug: "acme", details: undefined, listed: undefined })).toEqual({
      workspaceName: "acme",
      logoLeg: null,
    });
  });
});
