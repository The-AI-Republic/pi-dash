// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: workspace settings (details, logo assets, roster, member
// writes, invitation admin) against live Django.
import { beforeAll, describe, expect, it } from "vitest";
import { ApiError } from "../errors.js";
import {
  createWorkspace,
  deleteWorkspace,
  getWorkspace,
  leaveWorkspace,
  listWorkspaceRoster,
  removeWorkspaceMember,
  updateWorkspace,
  updateWorkspaceMemberRole,
} from "../contracts/workspace-management.js";
import {
  answerWorkspaceInvitation,
  listPendingWorkspaceInvites,
  listWorkspaceInvitations,
  sendWorkspaceInvitations,
  updateWorkspaceInviteRole,
  withdrawWorkspaceInvite,
} from "../contracts/invitations.js";
import { SignedUserAsset, deleteLegacyWorkspaceFileAsset, deleteWorkspaceLogoAsset } from "../contracts/assets.js";
import { listWorkspaces } from "../contracts/workspaces.js";
import { getMe } from "../contracts/users.js";
import { signIn } from "../contracts/auth-entry.js";
import {
  CONTRACT_EMAIL,
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
let admin: ApiClient;
let probe: ApiClient;

beforeAll(async () => {
  admin = await sharedSignedIn();
  const secondary = contractClient();
  const result = await signIn(secondary, { email: CONTRACT_SECONDARY_EMAIL, password: CONTRACT_PASSWORD });
  expect(result.ok).toBe(true);
  probe = secondary;
});

/** Fresh slug per test: the backend reserves slugs of deleted rows. */
function settingsSlug(prefix: string): string {
  return `${prefix}-${Date.now().toString(36)}${Math.floor(Math.random() * 1296).toString(36)}`;
}

async function deleteQuiet(slug: string): Promise<void> {
  await deleteWorkspace(admin, slug).catch(() => 0);
}

/** Invite the probe user to a workspace and have them accept at a role. */
async function acceptProbeInvite(slug: string, role: number): Promise<void> {
  const sent = await sendWorkspaceInvitations(admin, slug, {
    emails: [{ email: CONTRACT_SECONDARY_EMAIL, role }],
  });
  expect(sent).toEqual({ message: "Emails sent successfully" });
  const invitations = await listWorkspaceInvitations(probe);
  const invitation = invitations.find((row) => row.workspace.slug === slug);
  expect(invitation).toBeDefined();
  const accepted = await answerWorkspaceInvitation(probe, slug, invitation?.id ?? "", {
    accepted: true,
    token: invitation?.token ?? "",
  });
  expect(accepted).toEqual({ message: "Workspace Invitation Accepted" });
}

async function probeMembershipId(slug: string): Promise<string> {
  const me = await getMe(probe);
  const roster = await listWorkspaceRoster(admin, slug);
  const row = roster.find((membership) => membership.member.id === me.id);
  expect(row).toBeDefined();
  return row?.id ?? "";
}

async function adminMembershipId(slug: string): Promise<string> {
  const me = await getMe(admin);
  const roster = await listWorkspaceRoster(admin, slug);
  const row = roster.find((membership) => membership.member.id === me.id);
  expect(row).toBeDefined();
  return row?.id ?? "";
}

describe.skipIf(!contractEnabled)("workspace details contract", () => {
  it("reads the stored details", async () => {
    const slug = settingsSlug("ct-wsdet");
    await createWorkspace(admin, { name: `Contract ${slug}`, slug, organization_size: "2-10" });
    try {
      const details = await getWorkspace(admin, slug);
      expect(details.slug).toBe(slug);
      expect(details.name).toBe(`Contract ${slug}`);
      expect(details.organization_size).toBe("2-10");
      expect(details.timezone).toBe("UTC");
      expect(details.total_members).toBe(1);
      expect(details.logo).toBeNull();
      expect(details.logo_asset).toBeNull();
      expect(details.logo_url).toBeNull();
    } finally {
      await deleteQuiet(slug);
    }
  });

  it("patches name, size and timezone and reads them back", async () => {
    const slug = settingsSlug("ct-wspatch");
    await createWorkspace(admin, { name: `Contract ${slug}`, slug });
    try {
      const updated = await updateWorkspace(admin, slug, {
        name: `Renamed ${slug}`,
        organization_size: "11-50",
        timezone: "Pacific/Auckland",
      });
      expect(updated.name).toBe(`Renamed ${slug}`);
      expect(updated.organization_size).toBe("11-50");
      expect(updated.timezone).toBe("Pacific/Auckland");

      const reread = await getWorkspace(admin, slug);
      expect(reread.name).toBe(`Renamed ${slug}`);
      expect(reread.organization_size).toBe("11-50");
      expect(reread.timezone).toBe("Pacific/Auckland");
    } finally {
      await deleteQuiet(slug);
    }
  });

  it("patches the logo URL and clears it", async () => {
    const slug = settingsSlug("ct-wslogo");
    await createWorkspace(admin, { name: `Contract ${slug}`, slug });
    try {
      const url = "https://cdn.test/logos/ct-logo.png";
      const withLogo = await updateWorkspace(admin, slug, { logo: url });
      expect(withLogo.logo).toBe(url);
      expect(withLogo.logo_url).toBe(url);

      const cleared = await updateWorkspace(admin, slug, { logo: "" });
      expect(cleared.logo).toBe("");
      const reread = await getWorkspace(admin, slug);
      expect(reread.logo).toBe("");
      expect(reread.logo_url).toBeNull();
    } finally {
      await deleteQuiet(slug);
    }
  });

  it("refuses member and guest writes with 403", async () => {
    const slug = settingsSlug("ct-wsperm");
    await createWorkspace(admin, { name: `Contract ${slug}`, slug });
    try {
      await acceptProbeInvite(slug, 15);

      const memberPatch = await updateWorkspace(probe, slug, { name: "Nope" }).then(
        () => null,
        (caught: unknown) => caught
      );
      expect(memberPatch).toBeInstanceOf(ApiError);
      expect((memberPatch as ApiError).status).toBe(403);

      const memberDelete = await deleteWorkspace(probe, slug).then(
        () => null,
        (caught: unknown) => caught
      );
      expect(memberDelete).toBeInstanceOf(ApiError);
      expect((memberDelete as ApiError).status).toBe(403);

      await updateWorkspaceMemberRole(admin, slug, await probeMembershipId(slug), { role: 5 });
      const guestPatch = await updateWorkspace(probe, slug, { name: "Nope" }).then(
        () => null,
        (caught: unknown) => caught
      );
      expect(guestPatch).toBeInstanceOf(ApiError);
      expect((guestPatch as ApiError).status).toBe(403);
    } finally {
      await deleteQuiet(slug);
    }
  });

  it("deletes a workspace and it leaves the list", async () => {
    const slug = settingsSlug("ct-wsdel");
    await createWorkspace(admin, { name: `Contract ${slug}`, slug });
    try {
      await expect(deleteWorkspace(admin, slug)).resolves.toBe(204);
      const workspaces = await listWorkspaces(admin);
      expect(workspaces.some((workspace) => workspace.slug === slug)).toBe(false);

      const missing = await getWorkspace(admin, slug).then(
        () => null,
        (caught: unknown) => caught
      );
      expect(missing).toBeInstanceOf(ApiError);
      expect((missing as ApiError).status).toBe(404);
    } finally {
      await deleteQuiet(slug);
    }
  });
});

describe.skipIf(!contractEnabled)("workspace roster contract", () => {
  it("lists the admin-lens roster on the seeded workspace", async () => {
    const roster = await listWorkspaceRoster(admin, CONTRACT_WORKSPACE);
    expect(roster.length).toBeGreaterThanOrEqual(2);
    const mine = roster.find((membership) => membership.member.email === CONTRACT_EMAIL);
    expect(mine?.role).toBe(20);
    expect(mine?.member.last_login_medium).toBe("email");
    expect(mine?.member.display_name.length).toBeGreaterThan(0);
    expect(mine?.is_active).toBe(true);
    expect(typeof mine?.created_at).toBe("string");
  });

  it("keeps removed members listed as suspended", async () => {
    const slug = settingsSlug("ct-wssusp");
    await createWorkspace(admin, { name: `Contract ${slug}`, slug });
    try {
      await acceptProbeInvite(slug, 15);
      const membershipId = await probeMembershipId(slug);
      await expect(removeWorkspaceMember(admin, slug, membershipId)).resolves.toBe(204);

      const roster = await listWorkspaceRoster(admin, slug);
      expect(roster.find((row) => row.id === membershipId)?.is_active).toBe(false);
      const adminRowId = await adminMembershipId(slug);
      expect(roster.find((row) => row.id === adminRowId)?.is_active).toBe(true);
    } finally {
      await deleteQuiet(slug);
    }
  });
});

describe.skipIf(!contractEnabled)("workspace member write contract", () => {
  it("changes a member's role and reads it back", async () => {
    const slug = settingsSlug("ct-wsrole");
    await createWorkspace(admin, { name: `Contract ${slug}`, slug });
    try {
      await acceptProbeInvite(slug, 15);
      const membershipId = await probeMembershipId(slug);
      const updated = await updateWorkspaceMemberRole(admin, slug, membershipId, { role: 5 });
      expect(updated.id).toBe(membershipId);
      expect(updated.role).toBe(5);

      const roster = await listWorkspaceRoster(admin, slug);
      expect(roster.find((row) => row.id === membershipId)?.role).toBe(5);
    } finally {
      await deleteQuiet(slug);
    }
  });

  it("refuses self role-change with 400", async () => {
    const membershipId = await adminMembershipId(CONTRACT_WORKSPACE);
    const error = await updateWorkspaceMemberRole(admin, CONTRACT_WORKSPACE, membershipId, {
      role: 15,
    }).then(
      () => null,
      (caught: unknown) => caught
    );
    expect(error).toBeInstanceOf(ApiError);
    expect((error as ApiError).status).toBe(400);
  });

  it("refuses member-actor role writes with 403", async () => {
    const slug = settingsSlug("ct-wsrole403");
    await createWorkspace(admin, { name: `Contract ${slug}`, slug });
    try {
      await acceptProbeInvite(slug, 15);
      const error = await updateWorkspaceMemberRole(probe, slug, await adminMembershipId(slug), {
        role: 5,
      }).then(
        () => null,
        (caught: unknown) => caught
      );
      expect(error).toBeInstanceOf(ApiError);
      expect((error as ApiError).status).toBe(403);
    } finally {
      await deleteQuiet(slug);
    }
  });

  it("refuses self-removal with 400", async () => {
    const membershipId = await adminMembershipId(CONTRACT_WORKSPACE);
    const error = await removeWorkspaceMember(admin, CONTRACT_WORKSPACE, membershipId).then(
      () => null,
      (caught: unknown) => caught
    );
    expect(error).toBeInstanceOf(ApiError);
    expect((error as ApiError).status).toBe(400);
  });

  it("leaves a workspace", async () => {
    const slug = settingsSlug("ct-wsleave");
    await createWorkspace(admin, { name: `Contract ${slug}`, slug });
    try {
      await acceptProbeInvite(slug, 15);
      await expect(leaveWorkspace(probe, slug)).resolves.toBe(204);

      const me = await getMe(probe);
      const roster = await listWorkspaceRoster(admin, slug);
      expect(roster.find((row) => row.member.id === me.id)?.is_active).toBe(false);
    } finally {
      await deleteQuiet(slug);
    }
  });

  it("refuses the only admin's leave with 400", async () => {
    const slug = settingsSlug("ct-wsleave1");
    await createWorkspace(admin, { name: `Contract ${slug}`, slug });
    try {
      const error = await leaveWorkspace(admin, slug).then(
        () => null,
        (caught: unknown) => caught
      );
      expect(error).toBeInstanceOf(ApiError);
      expect((error as ApiError).status).toBe(400);
    } finally {
      await deleteQuiet(slug);
    }
  });
});

describe.skipIf(!contractEnabled)("workspace invitation admin contract", () => {
  it("lists, re-roles and withdraws a pending invite", async () => {
    const slug = settingsSlug("ct-wsinv");
    await createWorkspace(admin, { name: `Contract ${slug}`, slug });
    try {
      const email = `ct-ws-${Date.now().toString(36)}${Math.floor(Math.random() * 1296).toString(36)}@example.com`;
      const sent = await sendWorkspaceInvitations(admin, slug, { emails: [{ email, role: 15 }] });
      expect(sent).toEqual({ message: "Emails sent successfully" });

      const invites = await listPendingWorkspaceInvites(admin, slug);
      const invite = invites.find((row) => row.email === email);
      expect(invite?.role).toBe(15);
      expect(invite?.invite_link.length).toBeGreaterThan(0);

      const rerole = await updateWorkspaceInviteRole(admin, slug, invite?.id ?? "", { role: 5 });
      expect(rerole.role).toBe(5);
      const relisted = await listPendingWorkspaceInvites(admin, slug);
      expect(relisted.find((row) => row.id === invite?.id)?.role).toBe(5);

      await expect(withdrawWorkspaceInvite(admin, slug, invite?.id ?? "")).resolves.toBe(204);
      const after = await listPendingWorkspaceInvites(admin, slug);
      expect(after.some((row) => row.id === invite?.id)).toBe(false);
    } finally {
      await deleteQuiet(slug);
    }
  });

  it("lets members manage invites while guests are refused", async () => {
    const slug = settingsSlug("ct-wsinv403");
    await createWorkspace(admin, { name: `Contract ${slug}`, slug });
    try {
      await acceptProbeInvite(slug, 15);
      const email = `ct-wsm-${Date.now().toString(36)}${Math.floor(Math.random() * 1296).toString(36)}@example.com`;
      await sendWorkspaceInvitations(admin, slug, { emails: [{ email, role: 15 }] });

      const visible = await listPendingWorkspaceInvites(probe, slug);
      const invite = visible.find((row) => row.email === email);
      expect(invite?.role).toBe(15);

      const rerole = await updateWorkspaceInviteRole(probe, slug, invite?.id ?? "", { role: 5 });
      expect(rerole.role).toBe(5);
      await expect(withdrawWorkspaceInvite(probe, slug, invite?.id ?? "")).resolves.toBe(204);
      const after = await listPendingWorkspaceInvites(admin, slug);
      expect(after.some((row) => row.id === invite?.id)).toBe(false);

      await updateWorkspaceMemberRole(admin, slug, await probeMembershipId(slug), { role: 5 });
      const refused = await listPendingWorkspaceInvites(probe, slug).then(
        () => null,
        (caught: unknown) => caught
      );
      expect(refused).toBeInstanceOf(ApiError);
      expect((refused as ApiError).status).toBe(403);
    } finally {
      await deleteQuiet(slug);
    }
  });
});

describe.skipIf(!contractEnabled)("workspace logo asset contract", () => {
  it("grants, confirms and deletes a logo asset", async () => {
    const slug = settingsSlug("ct-wsasset");
    const created = await createWorkspace(admin, { name: `Contract ${slug}`, slug });
    try {
      const grantPath = `/api/assets/v2/workspaces/${slug}/`;
      const envelope = await admin.sendJson<unknown>("POST", grantPath, {
        json: {
          entity_identifier: created.id,
          entity_type: "WORKSPACE_LOGO",
          name: "logo.png",
          size: 1234,
          type: "image/png",
        },
      });
      const grant = admin.parse(SignedUserAsset, envelope.data, `POST ${grantPath}`);
      expect(grant.asset_id.length).toBeGreaterThan(0);
      expect(grant.asset_url).toBe(`/api/assets/v2/static/${grant.asset_id}/`);
      expect(grant.upload_data.url.length).toBeGreaterThan(0);

      // Confirm wires the logo; the byte leg between grant and confirm is
      // stub-proven (no object storage in the contract stack).
      const confirmed = await admin.patch(`/api/assets/v2/workspaces/${slug}/${grant.asset_id}/`);
      expect(confirmed.status).toBe(204);
      const wired = await getWorkspace(admin, slug);
      expect(wired.logo_asset).toBe(grant.asset_id);
      expect(wired.logo).toBe("");
      expect(wired.logo_url).toBe(grant.asset_url);

      await expect(deleteWorkspaceLogoAsset(admin, slug, grant.asset_id)).resolves.toBe(204);
      const unwired = await getWorkspace(admin, slug);
      expect(unwired.logo_asset).toBeNull();
    } finally {
      await deleteQuiet(slug);
    }
  });

  it("deletes a legacy file asset", async () => {
    const slug = settingsSlug("ct-wslegacy");
    const created = await createWorkspace(admin, { name: `Contract ${slug}`, slug });
    try {
      const grantPath = `/api/assets/v2/workspaces/${slug}/`;
      const envelope = await admin.sendJson<unknown>("POST", grantPath, {
        json: {
          entity_identifier: created.id,
          entity_type: "WORKSPACE_LOGO",
          name: "legacy.png",
          size: 1234,
          type: "image/png",
        },
      });
      const grant = admin.parse(SignedUserAsset, envelope.data, `POST ${grantPath}`);
      const storedKey = grant.upload_data.fields["key"] ?? "";
      expect(storedKey.startsWith(`${created.id}/`)).toBe(true);
      const assetKey = storedKey.slice(created.id.length + 1);
      await expect(deleteLegacyWorkspaceFileAsset(admin, created.id, assetKey)).resolves.toBe(204);
    } finally {
      await deleteQuiet(slug);
    }
  });
});
