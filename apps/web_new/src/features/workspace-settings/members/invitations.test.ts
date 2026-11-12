// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Pending-invitation rules (WSET-010, spec §11): search narrows by
// address, the count follows the search, rows order oldest first, copy
// links resolve against the running origin only when a link exists, and
// the avatar starts from the first address character.
import type { WorkspaceInvitation } from "@pidash/api-client";
import { describe, expect, it, vi } from "vitest";

import {
  copyInvitationLink,
  invitationInitial,
  invitationLink,
  invitationWithdrawTitle,
  pendingInvitationCount,
  PENDING_GROUP_LABEL,
  selectPendingInvitations,
} from "./invitations.js";

function invitation(email: string, created: string, link = "https://app.test/invite/tok"): WorkspaceInvitation {
  return {
    id: `inv-${email}`,
    workspace: { id: "ws", name: "Acme", slug: "acme", logo_url: null },
    email,
    accepted: false,
    token: "tok",
    message: null,
    responded_at: null,
    role: 15,
    invite_link: link,
    created_by: null,
    updated_by: null,
    created_at: created,
    updated_at: created,
  };
}

const ADA = invitation("ada@example.com", "2026-03-02T00:00:00Z");
const GRACE = invitation("grace@example.com", "2026-01-02T00:00:00Z");
const LINUS = invitation("linus@example.com", "2026-02-02T00:00:00Z");

describe("pending invitations", () => {
  it("names the group heading", () => {
    expect(PENDING_GROUP_LABEL).toBe("Pending invites");
  });

  it("orders matching invitations oldest first and ignores the input order", () => {
    expect(selectPendingInvitations([ADA, GRACE, LINUS], "").map((row) => row.email)).toEqual([
      "grace@example.com",
      "linus@example.com",
      "ada@example.com",
    ]);
    expect(selectPendingInvitations([], "")).toEqual([]);
  });

  it("matches by address, case-insensitively and by substring", () => {
    expect(selectPendingInvitations([ADA, GRACE], "GRACE@").map((row) => row.email)).toEqual(["grace@example.com"]);
    expect(selectPendingInvitations([ADA, GRACE], "  example  ")).toHaveLength(2);
    expect(selectPendingInvitations([ADA, GRACE], "nobody")).toEqual([]);
  });

  it("leaves the caller's list in place", () => {
    const list = [ADA, GRACE];
    selectPendingInvitations(list, "");
    expect(list.map((row) => row.email)).toEqual(["ada@example.com", "grace@example.com"]);
  });

  it("counts what the search leaves", () => {
    expect(pendingInvitationCount([ADA, GRACE, LINUS], "")).toBe(3);
    expect(pendingInvitationCount([ADA, GRACE, LINUS], "grace")).toBe(1);
    expect(pendingInvitationCount([ADA], "nobody")).toBe(0);
  });

  it("resolves a stored link against the running origin", () => {
    expect(invitationLink(ADA, "https://new.test")).toBe("https://app.test/invite/tok");
    expect(invitationLink(invitation("a@example.com", "2026-01-01T00:00:00Z", "/invite/tok"), "https://new.test")).toBe(
      "https://new.test/invite/tok"
    );
  });

  it("offers no link when the row carries none", () => {
    expect(invitationLink(invitation("a@example.com", "2026-01-01T00:00:00Z", "  "), "https://new.test")).toBeNull();
  });

  it("treats an unresolvable link as absent", () => {
    const base = invitation("a@example.com", "2026-01-01T00:00:00Z", "http://[bad");
    expect(invitationLink(base, "not a base")).toBeNull();
  });

  it("starts the avatar from the first address character", () => {
    expect(invitationInitial("ada@example.com")).toBe("A");
    expect(invitationInitial("  grace@example.com")).toBe("G");
    expect(invitationInitial("   ")).toBe("?");
  });

  it("titles the withdrawal with the address", () => {
    expect(invitationWithdrawTitle(ADA)).toBe("Remove ada@example.com?");
  });
});

describe("copyInvitationLink", () => {
  it("writes the absolute link and confirms once it lands", async () => {
    const writeText = vi.fn(async () => undefined);
    const copied = vi.fn();
    copyInvitationLink(ADA, "https://new.test", { writeText }, copied);
    expect(writeText).toHaveBeenCalledWith("https://app.test/invite/tok");
    await vi.waitFor(() => expect(copied).toHaveBeenCalledTimes(1));
  });

  it("asks for nothing when the row carries no link", () => {
    const writeText = vi.fn(async () => undefined);
    const copied = vi.fn();
    copyInvitationLink(
      invitation("a@example.com", "2026-01-01T00:00:00Z", "  "),
      "https://new.test",
      { writeText },
      copied
    );
    expect(writeText).not.toHaveBeenCalled();
    expect(copied).not.toHaveBeenCalled();
  });

  it("stays silent when the clipboard refuses", async () => {
    const writeText = vi.fn(async () => Promise.reject(new Error("denied")));
    const copied = vi.fn();
    copyInvitationLink(ADA, "https://new.test", { writeText }, copied);
    await Promise.resolve();
    await Promise.resolve();
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(copied).not.toHaveBeenCalled();
  });
});
