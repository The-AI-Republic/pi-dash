// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Member row actions and shared dialog copy (WSET-012/013, spec §13):
// admins remove others, anyone leaves their own row, suspended rows offer
// nothing, only an admin sees another's role control, and each target
// gets its own confirmation wording.
import type { WorkspaceRosterMember } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import { memberDisplayName, memberMenuAction, memberRoleEditable, removeDialogCopy } from "./memberRules.js";

function member(userId: string, active = true, displayName = "Ada Lovelace"): WorkspaceRosterMember {
  return {
    id: `m-${userId}`,
    member: {
      id: userId,
      first_name: "Ada",
      last_name: "Lovelace",
      avatar: "",
      avatar_url: null,
      display_name: displayName,
      is_bot: false,
      email: `${userId}@example.com`,
      last_login_medium: "email",
    },
    role: 15,
    is_active: active,
    created_at: "2026-01-01T00:00:00Z",
  };
}

const OTHER = member("other");
const SELF = member("self");

describe("member menu entries", () => {
  it("lets an admin remove somebody else and leave their own row", () => {
    expect(memberMenuAction(OTHER, "self", true)).toBe("remove");
    expect(memberMenuAction(SELF, "self", true)).toBe("leave");
  });

  it("offers a non-admin only their own leave entry", () => {
    expect(memberMenuAction(OTHER, "self", false)).toBeNull();
    expect(memberMenuAction(SELF, "self", false)).toBe("leave");
  });

  it("offers nobody anything on a suspended row", () => {
    const gone = member("gone", false);
    expect(memberMenuAction(gone, "self", true)).toBeNull();
    expect(memberMenuAction(gone, "gone", true)).toBeNull();
  });

  it("treats an unknown actor as somebody else", () => {
    expect(memberMenuAction(SELF, null, true)).toBe("remove");
    expect(memberMenuAction(OTHER, null, false)).toBeNull();
  });
});

describe("account-type control", () => {
  it("renders for admins on somebody else's active row", () => {
    expect(memberRoleEditable(OTHER, "self", true)).toBe(true);
    expect(memberRoleEditable(OTHER, null, true)).toBe(true);
  });

  it("never renders for the actor's own row, a non-admin or a suspended row", () => {
    expect(memberRoleEditable(SELF, "self", true)).toBe(false);
    expect(memberRoleEditable(OTHER, "self", false)).toBe(false);
    expect(memberRoleEditable(member("gone", false), "self", true)).toBe(false);
  });
});

describe("confirmation copy", () => {
  it("names another member and warns about losing access", () => {
    const copy = removeDialogCopy({ kind: "member", row: OTHER });
    expect(copy.title).toBe("Remove Ada Lovelace?");
    expect(copy.body).toContain("Ada Lovelace");
    expect(copy.body).toContain("no longer have access");
    expect(copy.confirm).toBe("Remove");
  });

  it("falls back to the address when the display name is blank", () => {
    const copy = removeDialogCopy({ kind: "member", row: member("other", true, "  ") });
    expect(copy.title).toBe("Remove other@example.com?");
  });

  it("asks the leaver before leaving", () => {
    const copy = removeDialogCopy({ kind: "self" });
    expect(copy.title).toBe("Leave workspace?");
    expect(copy.body).toContain("lose access");
    expect(copy.confirm).toBe("Leave");
  });

  it("names the invitation address and says it can be re-invited", () => {
    const copy = removeDialogCopy({
      kind: "invitation",
      invitation: {
        id: "inv-1",
        workspace: { id: "ws", name: "Acme", slug: "acme", logo_url: null },
        email: "ada@example.com",
        accepted: false,
        token: "tok",
        message: null,
        responded_at: null,
        role: 15,
        invite_link: "/invite/tok",
        created_by: null,
        updated_by: null,
        created_at: "2026-01-01T00:00:00Z",
        updated_at: "2026-01-01T00:00:00Z",
      },
    });
    expect(copy.title).toBe("Remove ada@example.com?");
    expect(copy.body).toContain("ada@example.com");
    expect(copy.confirm).toBe("Remove");
  });

  it("reads the display name with an address fallback", () => {
    expect(memberDisplayName(OTHER)).toBe("Ada Lovelace");
    expect(memberDisplayName(member("other", true, ""))).toBe("other@example.com");
  });
});
