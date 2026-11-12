// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import type { WorkspaceJoinRequestList } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import {
  ADMIN_REQUEST_SUBJECT,
  buildAdminRequestMailto,
  deriveWorkspaceSlug,
  INVITE_INITIAL_ROW_COUNT,
  isReservedWorkspaceSlug,
  isSoloTeamSize,
  RESERVED_WORKSPACE_SLUGS,
  resolvePendingEmail,
  resolveWorkspaceDefaultView,
  selectPendingJoinRequest,
  SLUG_FORMAT_MESSAGE,
  SLUG_TAKEN_MESSAGE,
  SOLO_TEAM_SIZE,
  validateWorkspaceName,
  validateWorkspaceSlug,
  WORKSPACE_INVITE_ROLES,
  WORKSPACE_NAME_MAX_LENGTH,
  WORKSPACE_SLUG_MAX_LENGTH,
  WORKSPACE_TEAM_SIZES,
  workspaceRoleLabel,
} from "./workspace.js";

function joinRequest(status: "PENDING" | "APPROVED" | "DENIED", adminEmail: string) {
  return {
    id: "123e4567-e89b-12d3-a456-426614174000",
    requester: {
      id: "123e4567-e89b-12d3-a456-426614174000",
      first_name: "Ada",
      last_name: "Lovelace",
      avatar: "",
      avatar_url: null,
      display_name: "Ada Lovelace",
      is_bot: false,
    },
    admin_email: adminEmail,
    message: null,
    status,
    responded_at: null,
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-01-02T03:04:05Z",
  };
}

describe("validateWorkspaceName", () => {
  it("requires a name", () => {
    expect(validateWorkspaceName("")).toBe("Enter a workspace name.");
    expect(validateWorkspaceName("   ")).toBe("Enter a workspace name.");
  });

  it("caps the name at eighty characters", () => {
    expect(validateWorkspaceName("a".repeat(WORKSPACE_NAME_MAX_LENGTH))).toBeNull();
    expect(validateWorkspaceName("a".repeat(WORKSPACE_NAME_MAX_LENGTH + 1))).toBe(
      "Keep the workspace name within 80 characters."
    );
  });

  it("rejects injection-risk characters", () => {
    expect(validateWorkspaceName("Acme <Corp>")).toContain("cannot contain symbols");
    expect(validateWorkspaceName("O'Brien")).toContain("cannot contain symbols");
  });

  it("rejects characters outside the name charset", () => {
    expect(validateWorkspaceName("Acme.Corp")).toBe("Use only letters, numbers, spaces, hyphens, and underscores.");
  });

  it("accepts letters, numbers, spaces, hyphens, and underscores", () => {
    expect(validateWorkspaceName("Acme Corp-123_4")).toBeNull();
    expect(validateWorkspaceName("Müller GmbH")).toBeNull();
  });
});

describe("validateWorkspaceSlug", () => {
  it("requires a slug", () => {
    expect(validateWorkspaceSlug("")).toBe("Enter a workspace URL.");
    expect(validateWorkspaceSlug("   ")).toBe("Enter a workspace URL.");
  });

  it("caps the slug at forty-eight characters", () => {
    expect(validateWorkspaceSlug("a".repeat(WORKSPACE_SLUG_MAX_LENGTH))).toBeNull();
    expect(validateWorkspaceSlug("a".repeat(WORKSPACE_SLUG_MAX_LENGTH + 1))).toBe(
      "Keep the workspace URL within 48 characters."
    );
  });

  it("rejects malformed slugs with the parity-pinned message", () => {
    expect(SLUG_FORMAT_MESSAGE).toContain("URLs can contain only");
    expect(validateWorkspaceSlug("bad@slug")).toBe(SLUG_FORMAT_MESSAGE);
    expect(validateWorkspaceSlug("has space")).toBe(SLUG_FORMAT_MESSAGE);
    expect(validateWorkspaceSlug("bad<slug")).toBe(SLUG_FORMAT_MESSAGE);
  });

  it("accepts letters, numbers, hyphens, and underscores", () => {
    expect(validateWorkspaceSlug("acme-corp_123")).toBeNull();
  });

  it("keeps the taken message parity-pinned", () => {
    expect(SLUG_TAKEN_MESSAGE).toContain("already taken");
  });
});

describe("deriveWorkspaceSlug", () => {
  it("lowercases, trims, and turns spaces into dashes", () => {
    expect(deriveWorkspaceSlug("My Cool Team")).toBe("my-cool-team");
    expect(deriveWorkspaceSlug("  Padded  ")).toBe("padded");
    expect(deriveWorkspaceSlug("already-slug")).toBe("already-slug");
    expect(deriveWorkspaceSlug("")).toBe("");
  });
});

describe("isReservedWorkspaceSlug", () => {
  it("holds the full spec list", () => {
    expect(RESERVED_WORKSPACE_SLUGS).toHaveLength(62);
    expect(RESERVED_WORKSPACE_SLUGS).toContain("api");
    expect(RESERVED_WORKSPACE_SLUGS).toContain("onboarding");
    expect(RESERVED_WORKSPACE_SLUGS).toContain("create-workspace");
    expect(RESERVED_WORKSPACE_SLUGS).toContain("instance");
  });

  it("matches case-insensitively", () => {
    expect(isReservedWorkspaceSlug("api")).toBe(true);
    expect(isReservedWorkspaceSlug("API")).toBe(true);
    expect(isReservedWorkspaceSlug("acme")).toBe(false);
  });
});

describe("team sizes", () => {
  it("lists the six stored sizes in order", () => {
    expect(WORKSPACE_TEAM_SIZES).toEqual(["Just myself", "2-10", "11-50", "51-200", "201-500", "500+"]);
  });

  it("recognizes only the solo size", () => {
    expect(SOLO_TEAM_SIZE).toBe("Just myself");
    expect(isSoloTeamSize("Just myself")).toBe(true);
    expect(isSoloTeamSize("2-10")).toBe(false);
    expect(isSoloTeamSize("")).toBe(false);
  });
});

describe("resolveWorkspaceDefaultView", () => {
  it("forces the holding view for a pending request", () => {
    expect(resolveWorkspaceDefaultView({ hasPendingRequest: true, hasInvitations: true })).toBe("pending");
    expect(resolveWorkspaceDefaultView({ hasPendingRequest: true, hasInvitations: false })).toBe("pending");
  });

  it("defaults to invites when invites exist", () => {
    expect(resolveWorkspaceDefaultView({ hasPendingRequest: false, hasInvitations: true })).toBe("invites");
  });

  it("defaults to create otherwise", () => {
    expect(resolveWorkspaceDefaultView({ hasPendingRequest: false, hasInvitations: false })).toBe("create");
  });
});

describe("selectPendingJoinRequest", () => {
  it("returns the first pending request", () => {
    const requests = [
      joinRequest("DENIED", "denied@example.com"),
      joinRequest("PENDING", "first@example.com"),
      joinRequest("PENDING", "second@example.com"),
    ] as unknown as WorkspaceJoinRequestList;
    expect(selectPendingJoinRequest(requests)?.admin_email).toBe("first@example.com");
  });

  it("returns null without a pending request", () => {
    const requests = [joinRequest("APPROVED", "done@example.com")] as unknown as WorkspaceJoinRequestList;
    expect(selectPendingJoinRequest(requests)).toBeNull();
    expect(selectPendingJoinRequest([])).toBeNull();
  });
});

describe("resolvePendingEmail", () => {
  it("prefers the just-requested address", () => {
    expect(resolvePendingEmail("fresh@example.com", { admin_email: "old@example.com" })).toBe("fresh@example.com");
  });

  it("falls back to the outstanding request's address", () => {
    expect(resolvePendingEmail(null, { admin_email: "old@example.com" })).toBe("old@example.com");
  });

  it("falls back to empty without either", () => {
    expect(resolvePendingEmail(null, null)).toBe("");
  });
});

describe("invite roles", () => {
  it("opens with three blank rows", () => {
    expect(INVITE_INITIAL_ROW_COUNT).toBe(3);
  });

  it("offers admin, member, and guest", () => {
    expect(WORKSPACE_INVITE_ROLES).toEqual([
      { value: 20, label: "Admin" },
      { value: 15, label: "Member" },
      { value: 5, label: "Guest" },
    ]);
  });

  it("labels known roles and reads unknown ones as member", () => {
    expect(workspaceRoleLabel(20)).toBe("Admin");
    expect(workspaceRoleLabel(15)).toBe("Member");
    expect(workspaceRoleLabel(5)).toBe("Guest");
    expect(workspaceRoleLabel(99)).toBe("Member");
  });
});

describe("buildAdminRequestMailto", () => {
  it("drafts an unaddressed request carrying the subject and template", () => {
    const href = buildAdminRequestMailto({ firstName: "Ada", lastName: "Lovelace", email: "ada@example.com" });
    expect(href.startsWith("mailto:?")).toBe(true);
    const query = new URLSearchParams(href.slice("mailto:?".length));
    expect(query.get("subject")).toBe(ADMIN_REQUEST_SUBJECT);
    const body = query.get("body") ?? "";
    expect(body).toContain("[/workspace-name]");
    expect(body).toContain("ada@example.com");
    expect(body).toContain("Ada Lovelace");
  });

  it("signs with the bare address when the name is blank", () => {
    const href = buildAdminRequestMailto({ firstName: "", lastName: "  ", email: "ada@example.com" });
    const body = new URLSearchParams(href.slice("mailto:?".length)).get("body") ?? "";
    expect(body.endsWith("ada@example.com")).toBe(true);
  });

  it("encodes reserved characters in the draft", () => {
    const href = buildAdminRequestMailto({ firstName: "Ada&Co", lastName: "", email: "ada+tour@example.com" });
    expect(href).toContain(encodeURIComponent("Ada&Co"));
    expect(href).toContain(encodeURIComponent("ada+tour@example.com"));
  });
});
