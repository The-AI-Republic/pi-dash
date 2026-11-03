// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Gate decisions (WSET-002/036, spec §1/§36): the address alone picks
// the section, the role answers allow/deny, and the caller never sees a
// redirect.
import { describe, expect, it } from "vitest";

import { ROLE_ADMIN, ROLE_GUEST, ROLE_MEMBER, selectPermissions } from "../../../core/session/index.js";
import { decideWorkspaceSettingsGate } from "./gate.js";

const ADMIN = selectPermissions(ROLE_ADMIN);
const MEMBER = selectPermissions(ROLE_MEMBER);
const GUEST = selectPermissions(ROLE_GUEST);

describe("decideWorkspaceSettingsGate", () => {
  it("ignores addresses outside the settings subtree", () => {
    expect(decideWorkspaceSettingsGate("/acme/projects", ADMIN)).toBeNull();
    expect(decideWorkspaceSettingsGate("/acme", ADMIN)).toBeNull();
    expect(decideWorkspaceSettingsGate("/", ADMIN)).toBeNull();
  });

  it("allows admins into every known section, nested addresses included", () => {
    expect(decideWorkspaceSettingsGate("/acme/settings", ADMIN)).toEqual({ status: "allowed", section: "general" });
    expect(decideWorkspaceSettingsGate("/acme/settings/billing/", ADMIN)).toEqual({
      status: "allowed",
      section: "billing",
    });
    expect(decideWorkspaceSettingsGate("/acme/settings/webhooks/abc", ADMIN)).toEqual({
      status: "allowed",
      section: "webhooks",
    });
  });

  it("allows members into their sections and denies admin-only ones on the same address", () => {
    expect(decideWorkspaceSettingsGate("/acme/settings/members/", MEMBER)).toEqual({
      status: "allowed",
      section: "members",
    });
    expect(decideWorkspaceSettingsGate("/acme/settings/billing/", MEMBER)).toEqual({
      status: "denied",
      section: "billing",
    });
    expect(decideWorkspaceSettingsGate("/acme/settings/webhooks/x", MEMBER)).toEqual({
      status: "denied",
      section: "webhooks",
    });
  });

  it("denies guests everywhere", () => {
    expect(decideWorkspaceSettingsGate("/acme/settings/", GUEST)).toEqual({ status: "denied", section: "general" });
    expect(decideWorkspaceSettingsGate("/acme/settings/exports/", GUEST)).toEqual({
      status: "denied",
      section: "exports",
    });
  });

  it("denies an unknown section with no section to title", () => {
    expect(decideWorkspaceSettingsGate("/acme/settings/projects/p1", ADMIN)).toEqual({
      status: "denied",
      section: null,
    });
  });
});
