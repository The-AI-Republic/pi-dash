// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import type { WorkspaceInvitation } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import {
  decideSingleInvitationView,
  INVITATION_IGNORE_LANDING,
  resolveSingleAcceptLanding,
  selectBatchLandingTarget,
  selectInvitationRoleLabel,
  toggleInvitationSelection,
} from "./invitations.js";

function invitation(overrides: Partial<WorkspaceInvitation> = {}): WorkspaceInvitation {
  return {
    id: "11111111-1111-4111-8111-111111111111",
    workspace: {
      id: "22222222-2222-4222-8222-222222222222",
      name: "Acme",
      slug: "acme",
      logo_url: null,
    },
    email: "ada@example.com",
    accepted: false,
    token: "token-1",
    message: null,
    responded_at: null,
    role: 15,
    invite_link: "https://app.test/workspace-invitations?invitation_id=1",
    created_by: null,
    updated_by: null,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    ...overrides,
  };
}

describe("selectInvitationRoleLabel", () => {
  it("labels the three workspace roles", () => {
    expect(selectInvitationRoleLabel(20)).toBe("Admin");
    expect(selectInvitationRoleLabel(15)).toBe("Member");
    expect(selectInvitationRoleLabel(5)).toBe("Guest");
  });

  it("folds unknown roles onto the nearest threshold", () => {
    expect(selectInvitationRoleLabel(99)).toBe("Admin");
    expect(selectInvitationRoleLabel(16)).toBe("Member");
    expect(selectInvitationRoleLabel(0)).toBe("Guest");
  });
});

describe("toggleInvitationSelection", () => {
  it("adds an unselected invite at the end", () => {
    expect(toggleInvitationSelection(["a"], "b")).toEqual(["a", "b"]);
  });

  it("removes a selected invite", () => {
    expect(toggleInvitationSelection(["a", "b"], "a")).toEqual(["b"]);
  });
});

describe("selectBatchLandingTarget", () => {
  const first = invitation({ id: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa" });
  const second = invitation({
    id: "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
    workspace: { id: "33333333-3333-4333-8333-333333333333", name: "Beta", slug: "beta", logo_url: null },
  });

  it("lands on the first selected invite in selection order", () => {
    expect(selectBatchLandingTarget([first, second], [second.id, first.id])).toEqual({
      id: "33333333-3333-4333-8333-333333333333",
      slug: "beta",
    });
  });

  it("skips selected ids the list no longer names", () => {
    expect(selectBatchLandingTarget([second], ["missing", second.id])).toEqual({
      id: "33333333-3333-4333-8333-333333333333",
      slug: "beta",
    });
  });

  it("resolves nothing when no selection matches", () => {
    expect(selectBatchLandingTarget([first], [])).toBeNull();
    expect(selectBatchLandingTarget([first], ["missing"])).toBeNull();
    expect(selectBatchLandingTarget([], ["missing"])).toBeNull();
  });
});

describe("resolveSingleAcceptLanding", () => {
  it("lands in the joined workspace on an address match", () => {
    expect(
      resolveSingleAcceptLanding({ inviteeEmail: "ada@example.com", viewerEmail: "ada@example.com", slug: "acme" })
    ).toBe("/acme");
  });

  it("lands home when the signed-in address differs", () => {
    expect(
      resolveSingleAcceptLanding({ inviteeEmail: "ada@example.com", viewerEmail: "grace@example.com", slug: "acme" })
    ).toBe("/");
  });

  it("lands home for signed-out viewers", () => {
    expect(resolveSingleAcceptLanding({ inviteeEmail: "ada@example.com", viewerEmail: undefined, slug: "acme" })).toBe(
      "/"
    );
  });

  it("ignoring always lands home", () => {
    expect(INVITATION_IGNORE_LANDING).toBe("/");
  });
});

describe("decideSingleInvitationView", () => {
  it("renders the actions for a pending invite", () => {
    const pending = invitation();
    expect(decideSingleInvitationView({ invitation: pending, failed: false })).toEqual({
      state: "pending",
      invitation: pending,
    });
  });

  it("renders the already-member card for an answered-plus-accepted invite", () => {
    const answered = invitation({ accepted: true, responded_at: "2026-01-02T00:00:00Z" });
    expect(decideSingleInvitationView({ invitation: answered, failed: false })).toEqual({
      state: "already-member",
      invitation: answered,
    });
  });

  it("renders the inactive card for an answered-plus-declined invite", () => {
    const declined = invitation({ accepted: false, responded_at: "2026-01-02T00:00:00Z" });
    expect(decideSingleInvitationView({ invitation: declined, failed: false })).toEqual({ state: "inactive" });
  });

  it("renders the inactive card when the fetch fails with nothing cached", () => {
    expect(decideSingleInvitationView({ invitation: null, failed: true })).toEqual({ state: "inactive" });
  });

  it("renders the inactive card when an answered row refetch fails", () => {
    const declined = invitation({ accepted: false, responded_at: "2026-01-02T00:00:00Z" });
    expect(decideSingleInvitationView({ invitation: declined, failed: true })).toEqual({ state: "inactive" });
  });

  it("renders the dead not-found branch for detail plus a fetch error at once", () => {
    const pending = invitation();
    expect(decideSingleInvitationView({ invitation: pending, failed: true })).toEqual({ state: "not-found" });
  });

  it("loads while the fetch is in flight", () => {
    expect(decideSingleInvitationView({ invitation: null, failed: false })).toEqual({ state: "loading" });
  });
});
