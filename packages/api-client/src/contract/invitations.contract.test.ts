// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: workspace invitations and join requests against live Django.
import { beforeAll, describe, expect, it } from "vitest";
import { ApiError } from "../errors.js";
import {
  acceptWorkspaceInvitations,
  answerWorkspaceInvitation,
  createWorkspaceJoinRequest,
  getWorkspaceInvitation,
  listWorkspaceInvitations,
  listWorkspaceJoinRequests,
  sendWorkspaceInvitations,
} from "../contracts/invitations.js";
import { listWorkspaces } from "../contracts/workspaces.js";
import { createWorkspace } from "../contracts/workspace-management.js";
import { signIn } from "../contracts/auth-entry.js";
import {
  CONTRACT_PASSWORD,
  CONTRACT_SECONDARY_EMAIL,
  CONTRACT_WORKSPACE,
  contractClient,
  contractEnabled,
  sharedSignedIn,
} from "./setup.js";
import type { ApiClient } from "../client.js";

// One session per user for the file: the scratch stack throttles anonymous
// hits (CSRF fetch + sign-in POST) to 30/minute per IP, so per-test
// sign-ins would 429 each other when files run together.
let inviter: ApiClient;
let invitee: ApiClient;

beforeAll(async () => {
  inviter = await sharedSignedIn();
  const probe = contractClient();
  const result = await signIn(probe, { email: CONTRACT_SECONDARY_EMAIL, password: CONTRACT_PASSWORD });
  expect(result.ok).toBe(true);
  invitee = probe;
});

/** Fresh slug per run: invites are unique per email and workspace. */
function inviteSlug(prefix: string): string {
  return `${prefix}-${Date.now().toString(36)}${Math.floor(Math.random() * 1296).toString(36)}`;
}

async function inviteProbe(prefix: string): Promise<{ slug: string; invitationId: string; token: string }> {
  const slug = inviteSlug(prefix);
  await createWorkspace(inviter, { name: `Contract ${slug}`, slug });
  const sent = await sendWorkspaceInvitations(inviter, slug, {
    emails: [{ email: CONTRACT_SECONDARY_EMAIL, role: 15 }],
  });
  expect(sent).toEqual({ message: "Emails sent successfully" });
  const invitations = await listWorkspaceInvitations(invitee);
  const invitation = invitations.find((row) => row.workspace.slug === slug);
  expect(invitation).toBeDefined();
  return { slug, invitationId: invitation?.id ?? "", token: invitation?.token ?? "" };
}

async function deleteWorkspace(slug: string): Promise<void> {
  await inviter.delete(`/api/workspaces/${slug}/`).catch(() => null);
}

describe.skipIf(!contractEnabled)("workspace invitations contract", () => {
  it("invites, lists and batch-accepts", async () => {
    const slug = inviteSlug("ct-inv");
    await createWorkspace(inviter, { name: `Contract ${slug}`, slug });
    try {
      const sent = await sendWorkspaceInvitations(inviter, slug, {
        emails: [{ email: CONTRACT_SECONDARY_EMAIL, role: 15 }],
      });
      expect(sent).toEqual({ message: "Emails sent successfully" });

      const invitations = await listWorkspaceInvitations(invitee);
      const invitation = invitations.find((row) => row.workspace.slug === slug);
      expect(invitation?.email).toBe(CONTRACT_SECONDARY_EMAIL);
      expect(invitation?.accepted).toBe(false);
      expect(invitation?.role).toBe(15);

      await acceptWorkspaceInvitations(invitee, [invitation?.id ?? ""]);

      const afterAccept = await listWorkspaceInvitations(invitee);
      expect(afterAccept.some((row) => row.workspace.slug === slug)).toBe(false);
      const joined = await listWorkspaces(invitee);
      expect(joined.some((workspace) => workspace.slug === slug)).toBe(true);
    } finally {
      await deleteWorkspace(slug);
    }
  });

  it("fetches one invitation and declines it, keeping the declined row", async () => {
    const { slug, invitationId, token } = await inviteProbe("ct-dec");
    try {
      const fetched = await getWorkspaceInvitation(invitee, slug, invitationId);
      expect(fetched.accepted).toBe(false);
      expect(fetched.responded_at).toBeNull();

      const declined = await answerWorkspaceInvitation(invitee, slug, invitationId, {
        accepted: false,
        token,
      });
      expect(declined).toEqual({ message: "Workspace Invitation was not accepted" });

      const kept = await getWorkspaceInvitation(invitee, slug, invitationId);
      expect(kept.accepted).toBe(false);
      expect(kept.responded_at).not.toBeNull();
    } finally {
      await deleteWorkspace(slug);
    }
  });

  it("accepts one invitation and deletes the row", async () => {
    const { slug, invitationId, token } = await inviteProbe("ct-acc");
    try {
      const accepted = await answerWorkspaceInvitation(invitee, slug, invitationId, {
        accepted: true,
        token,
      });
      expect(accepted).toEqual({ message: "Workspace Invitation Accepted" });

      const error = await getWorkspaceInvitation(invitee, slug, invitationId).then(
        () => null,
        (caught: unknown) => caught
      );
      expect(error).toBeInstanceOf(ApiError);
      expect((error as ApiError).status).toBe(404);

      const joined = await listWorkspaces(invitee);
      expect(joined.some((workspace) => workspace.slug === slug)).toBe(true);
    } finally {
      await deleteWorkspace(slug);
    }
  });

  it("rejects a single answer with a wrong token", async () => {
    const { slug, invitationId } = await inviteProbe("ct-bad");
    try {
      const error = await answerWorkspaceInvitation(invitee, slug, invitationId, {
        accepted: true,
        token: "wrong-token",
      }).then(
        () => null,
        (caught: unknown) => caught
      );
      expect(error).toBeInstanceOf(ApiError);
      expect((error as ApiError).status).toBe(403);
    } finally {
      await deleteWorkspace(slug);
    }
  });
});

describe.skipIf(!contractEnabled)("workspace join requests contract", () => {
  it("creates a join request and lists it back", async () => {
    const adminEmail = `nobody-${Date.now().toString(36)}@example.com`;
    const created = await createWorkspaceJoinRequest(inviter, { admin_email: adminEmail });
    expect(created).toEqual({ message: "Request sent" });

    const requests = await listWorkspaceJoinRequests(inviter);
    const mine = requests.find((row) => row.admin_email === adminEmail);
    expect(mine?.status).toBe("PENDING");
    expect(mine?.message).toBeNull();
    expect(mine?.responded_at).toBeNull();
  });

  it("names the workspace when the address resolves to a joined one", async () => {
    const shortcut = await createWorkspaceJoinRequest(inviter, { admin_email: CONTRACT_SECONDARY_EMAIL });
    expect(shortcut.message).toBe("You are already a member of this workspace");
    expect(shortcut.workspace_slug).toBe(CONTRACT_WORKSPACE);
  });
});
