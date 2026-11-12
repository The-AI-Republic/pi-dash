// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runners access rule (NEWFRONT-540, RUN-001). The rule is a pure function
// over the role records: loading keeps the screen blank, admin/member pass,
// guests and unknowns are denied, and workspace admins always reach a
// project regardless of their project-level entry.
import { describe, expect, it } from "vitest";

import { ROLE_ADMIN, ROLE_GUEST, ROLE_MEMBER } from "../../core/session/permissions.js";
import { decideProjectRunnersAccess, decideWorkspaceRunnersAccess } from "./access.js";

describe("decideWorkspaceRunnersAccess", () => {
  it("stays loading until the role record settles", () => {
    expect(decideWorkspaceRunnersAccess({ role: null, isLoading: true })).toBe("loading");
    // A loaded role wins even while another read keeps loading.
    expect(decideWorkspaceRunnersAccess({ role: ROLE_MEMBER, isLoading: true })).toBe("loading");
  });

  it("admits admins and members", () => {
    expect(decideWorkspaceRunnersAccess({ role: ROLE_ADMIN, isLoading: false })).toBe("allowed");
    expect(decideWorkspaceRunnersAccess({ role: ROLE_MEMBER, isLoading: false })).toBe("allowed");
  });

  it("denies guests and callers without a membership", () => {
    expect(decideWorkspaceRunnersAccess({ role: ROLE_GUEST, isLoading: false })).toBe("denied");
    expect(decideWorkspaceRunnersAccess({ role: null, isLoading: false })).toBe("denied");
  });
});

describe("decideProjectRunnersAccess", () => {
  const base = {
    workspaceLoading: false,
    projectRolesLoading: false,
    projectRoles: { "project-1": ROLE_MEMBER } as Record<string, number>,
    projectId: "project-1",
  };

  it("waits for either role record", () => {
    expect(decideProjectRunnersAccess({ ...base, workspaceRole: ROLE_MEMBER, workspaceLoading: true })).toBe("loading");
    expect(decideProjectRunnersAccess({ ...base, workspaceRole: ROLE_MEMBER, projectRolesLoading: true })).toBe(
      "loading"
    );
  });

  it("admits project admins and members", () => {
    expect(
      decideProjectRunnersAccess({ ...base, workspaceRole: ROLE_GUEST, projectRoles: { "project-1": ROLE_ADMIN } })
    ).toBe("allowed");
    expect(decideProjectRunnersAccess({ ...base, workspaceRole: ROLE_GUEST })).toBe("allowed");
  });

  it("admits workspace admins without a project entry", () => {
    expect(decideProjectRunnersAccess({ ...base, workspaceRole: ROLE_ADMIN, projectRoles: {} })).toBe("allowed");
  });

  it("denies guests, project guests and members of other projects", () => {
    expect(decideProjectRunnersAccess({ ...base, workspaceRole: ROLE_GUEST, projectRoles: {} })).toBe("denied");
    expect(
      decideProjectRunnersAccess({ ...base, workspaceRole: ROLE_GUEST, projectRoles: { "project-1": ROLE_GUEST } })
    ).toBe("denied");
    expect(
      decideProjectRunnersAccess({ ...base, workspaceRole: ROLE_GUEST, projectRoles: { "project-2": ROLE_MEMBER } })
    ).toBe("denied");
    expect(decideProjectRunnersAccess({ ...base, workspaceRole: null, projectRoles: undefined })).toBe("denied");
  });
});
