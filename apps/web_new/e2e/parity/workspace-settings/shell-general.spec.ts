// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-410): workspace-settings shell and general
// page. Rows WSET-001 (grouped sidebar shell, active highlight,
// narrow-screen drawer), WSET-002 (per-section permission gate with the
// not-authorized view), WSET-003 (edit name, organization size and
// timezone), WSET-004 (logo upload, replace and removal with the
// initial-letter fallback), WSET-005 (copy the workspace URL id),
// WSET-006 (delete-workspace danger zone) and WSET-007 (tab titles).
//
// Observed behavior notes (the inventory rows carry them at update
// time): the sidebar groups entries under Administration (general,
// members, billing, exports) and Developer (webhooks, integrations),
// hides groups a role cannot use, and highlights the current section;
// narrow screens swap it for a hamburger drawer with the same entries
// plus an active-path crumb; general, members and exports admit admins
// and members while billing, webhooks and integrations are admin-only
// and guests are refused everywhere with the not-authorized view and an
// empty tab title (reads stay server-allowed, so the gate is UI-only);
// the general form saves name, size and timezone behind an Updating
// state with a success notice, validates the name inline, and leaves
// the form and server state intact on validation and server failures
// (Escape-closing the size dropdown wedges reopening until an outside
// click lands: bug NEWFRONT-420, recorded in the row);
// logo upload and replace close the dialog with no notice while removal
// shows a success notice but leaves the old image rendered until a
// reload (bug NEWFRONT-416); copying the workspace URL fills the
// clipboard with the full workspace address plus a confirmation;
// deleting needs the workspace name plus a fixed phrase, navigates to
// the remaining workspace on success, and leaves everything untouched
// on cancel; every section titles its tab "<workspace> - <section>".
import { fileURLToPath } from "node:url";
import { test, expect } from "../fixtures";
import {
  parityApiBase,
  serverCleanupWorkspaceMember,
  serverCompleteOnboarding,
  serverCreateWorkspace,
  serverDeleteWorkspace,
  serverProvisionWorkspaceMember,
  serverRequestStatus,
  serverWorkspace,
  serverWorkspaceMembers,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import type { ParityDriver, ParitySeedFacts } from "../drivers/parity-driver";

const LOGO_A = fileURLToPath(new URL("./logo-a.png", import.meta.url));
const LOGO_B = fileURLToPath(new URL("./logo-b.png", import.meta.url));

/** Full admin sidebar in display order: every entry the shell defines. */
const ADMIN_ITEMS = ["General", "Members", "Billing & Plans", "Exports", "Webhooks", "Integrations (github)"];

/** Member-visible sidebar: the administration entries a member may open. */
const MEMBER_ITEMS = ["General", "Members", "Exports"];

/** Sections denied to members (admin-only) plus the guest-denied-all set. */
const ADMIN_ONLY: string[] = ["billing", "webhooks", "integrations"];
const ALL_SECTIONS: string[] = ["", "members", "billing", "exports", "webhooks", "integrations"];

/** Seeded guest identity; throws when the seed lacks it. */
function requireGuest(seed: ParitySeedFacts): { email: string; password: string } {
  if (!seed.guestEmail || !seed.guestPassword)
    throw new Error("[parity] seed has no guest identity; rerun parity-up.sh from a clean checkout.");
  return { email: seed.guestEmail, password: seed.guestPassword };
}

/** Provision a workspace member with onboarding completed, for UI sign-in. */
async function provisionMember(
  seed: ParitySeedFacts,
  ownerSession: string,
  prefix: string
): Promise<{ userId: string; session: string; email: string; password: string }> {
  const member = await serverProvisionWorkspaceMember(seed.workspaceSlug, prefix, ownerSession);
  await serverCompleteOnboarding(member.session);
  return member;
}

/**
 * Switch the signed-in UI user. Sign-in is a no-op while a session
 * lives, so the current user signs out first.
 */
async function switchUser(driver: ParityDriver, email: string, password: string, workspaceSlug: string): Promise<void> {
  await driver.signOutViaCommandPalette();
  await driver.rulesEnsureSignedIn(email, password, workspaceSlug);
}

/** Workspace membership id for cleanup, resolved by provisioned user id. */
async function membershipIdFor(seed: ParitySeedFacts, ownerSession: string, userId: string): Promise<string> {
  const rows = await serverWorkspaceMembers(seed.workspaceSlug, ownerSession);
  const row = rows.find((entry) => entry.userId === userId);
  if (!row) throw new Error("[parity] provisioned member missing from workspace list.");
  return row.membershipId;
}

/** Most recent toast, waiting briefly for one to appear; null when none does. */
async function pollToast(driver: ParityDriver): Promise<{ title: string; message: string } | null> {
  try {
    let toast: { title: string; message: string } | null = null;
    await expect
      .poll(
        async () => {
          toast = await driver.rulesLastToast();
          return toast === null ? "none" : "toast";
        },
        { timeout: 10_000 }
      )
      .toBe("toast");
    return toast;
  } catch {
    return null;
  }
}

/** Wait until no toast is visible so a later notice can be attributed. */
async function waitForNoToast(driver: ParityDriver): Promise<void> {
  await expect.poll(async () => (await driver.rulesLastToast()) === null, { timeout: 20_000 }).toBe(true);
}

/**
 * Convergent fallback preamble for the logo scenarios: a logo left by an
 * interrupted run is removed through the UI (plus a re-open for bug
 * NEWFRONT-416), a stale render is cleared by re-opening, and both paths
 * settle on the fallback with no stored logo.
 */
async function ensureFallbackLogo(driver: ParityDriver, seed: ParitySeedFacts, session: string): Promise<void> {
  await driver.wsSettingsOpen(seed.workspaceSlug, "");
  const facts = await serverWorkspace(seed.workspaceSlug, session);
  if (facts.logoUrl !== null) {
    await driver.wsSettingsOpenLogoDialog();
    await driver.wsSettingsRemoveLogo();
    if (await driver.wsSettingsLogoDialogVisible()) await driver.wsSettingsCloseLogoDialog();
    await driver.wsSettingsOpen(seed.workspaceSlug, "");
  } else if ((await driver.wsSettingsLogoState()) === "image") {
    await driver.wsSettingsOpen(seed.workspaceSlug, "");
  }
  expect(await driver.wsSettingsLogoState()).toBe("fallback");
  expect(await driver.wsSettingsLogoSrc()).toBeNull();
  expect((await serverWorkspace(seed.workspaceSlug, session)).logoUrl).toBeNull();
  await waitForNoToast(driver);
}

test(
  specTitle(["WSET-001"], "settings shell groups entries and highlights the current section"),
  { tag: specTags(["WSET-001"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);

    await test.step("admin sees both groups with every entry", async () => {
      await driver.wsSettingsOpen(seed.workspaceSlug, "");
      expect(await driver.wsSettingsGroups()).toEqual(["Administration", "Developer"]);
      expect(await driver.wsSettingsItems()).toEqual(ADMIN_ITEMS);
      expect(await driver.wsSettingsActiveItem()).toBe("General");
      const facts = await serverWorkspace(seed.workspaceSlug, session);
      expect(facts.slug).toBe(seed.workspaceSlug);
    });

    await test.step("following entries moves the highlight", async () => {
      await driver.wsSettingsFollowItem("Members");
      expect(await driver.wsSettingsPath()).toBe(`/${seed.workspaceSlug}/settings/members/`);
      expect(await driver.wsSettingsActiveItem()).toBe("Members");
      await driver.wsSettingsFollowItem("Webhooks");
      expect(await driver.wsSettingsPath()).toBe(`/${seed.workspaceSlug}/settings/webhooks/`);
      expect(await driver.wsSettingsActiveItem()).toBe("Webhooks");
      await driver.wsSettingsFollowItem("General");
      expect(await driver.wsSettingsActiveItem()).toBe("General");
    });
  }
);

test(
  specTitle(["WSET-001"], "members see the filtered sidebar with no developer group"),
  { tag: specTags(["WSET-001"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const member = await provisionMember(seed, session, "nf410-shell");
    try {
      await driver.rulesEnsureSignedIn(member.email, member.password, seed.workspaceSlug);
      await driver.wsSettingsOpen(seed.workspaceSlug, "");
      expect(await driver.wsSettingsGroups()).toEqual(["Administration"]);
      expect(await driver.wsSettingsItems()).toEqual(MEMBER_ITEMS);
      expect(await driver.wsSettingsActiveItem()).toBe("General");
      await driver.wsSettingsFollowItem("Exports");
      expect(await driver.wsSettingsActiveItem()).toBe("Exports");
      const facts = await serverWorkspace(seed.workspaceSlug, member.session);
      expect(facts.slug).toBe(seed.workspaceSlug);
    } finally {
      await serverCleanupWorkspaceMember(
        seed.workspaceSlug,
        await membershipIdFor(seed, session, member.userId),
        session
      );
    }
  }
);

test(
  specTitle(["WSET-001"], "narrow screens swap the sidebar for a drawer with the same entries"),
  { tag: specTags(["WSET-001"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const member = await provisionMember(seed, session, "nf410-drawer");
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.wsSettingsOpen(seed.workspaceSlug, "");

      await test.step("the drawer trigger hides on wide screens", async () => {
        expect(await driver.wsSettingsNarrowTriggerVisible()).toBe(false);
      });

      await test.step("the admin drawer lists every entry with the active crumb", async () => {
        try {
          await driver.wsSettingsOpenDrawer();
          expect(await driver.wsSettingsNarrowTriggerVisible()).toBe(true);
          expect(await driver.wsSettingsDrawerItems()).toEqual(ADMIN_ITEMS);
          expect(await driver.wsSettingsMobileCrumb()).toBe("General");
        } finally {
          await driver.wsSettingsWideViewport();
        }
        expect(await driver.wsSettingsNarrowTriggerVisible()).toBe(false);
      });

      await test.step("the member drawer lists the filtered entries", async () => {
        await switchUser(driver, member.email, member.password, seed.workspaceSlug);
        await driver.wsSettingsOpen(seed.workspaceSlug, "");
        try {
          await driver.wsSettingsOpenDrawer();
          expect(await driver.wsSettingsDrawerItems()).toEqual(MEMBER_ITEMS);
          expect(await driver.wsSettingsMobileCrumb()).toBe("General");
        } finally {
          await driver.wsSettingsWideViewport();
        }
      });
    } finally {
      await serverCleanupWorkspaceMember(
        seed.workspaceSlug,
        await membershipIdFor(seed, session, member.userId),
        session
      );
    }
  }
);

test(
  specTitle(["WSET-002"], "section gates admit each role and refuse the rest with the not-authorized view"),
  { tag: specTags(["WSET-002"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const guest = requireGuest(seed);
    const member = await provisionMember(seed, session, "nf410-gates");
    try {
      await test.step("admins open every section", async () => {
        await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
        for (const section of ALL_SECTIONS) {
          await driver.wsSettingsOpen(seed.workspaceSlug, section);
          expect(await driver.wsSettingsDeniedVisible()).toBe(false);
        }
      });

      await test.step("members open general, members and exports and are refused the admin sections", async () => {
        await switchUser(driver, member.email, member.password, seed.workspaceSlug);
        for (const section of ["", "members", "exports"]) {
          await driver.wsSettingsOpen(seed.workspaceSlug, section);
          expect(await driver.wsSettingsDeniedVisible()).toBe(false);
        }
        for (const section of ADMIN_ONLY) {
          await driver.wsSettingsOpen(seed.workspaceSlug, section);
          expect(await driver.wsSettingsDeniedVisible()).toBe(true);
        }
      });

      await test.step("members see the general page dimmed and disabled", async () => {
        await driver.wsSettingsOpen(seed.workspaceSlug, "");
        expect(await driver.wsSettingsGeneralVisible()).toBe(true);
        expect(await driver.wsSettingsNameDisabled()).toBe(true);
        expect(await driver.wsSettingsSaveVisible()).toBe(false);
        expect(await driver.wsSettingsLogoAction()).toBe("none");
        expect(await driver.wsSettingsDeleteSectionVisible()).toBe(false);
      });

      await test.step("guests are refused every section", async () => {
        await switchUser(driver, guest.email, guest.password, seed.workspaceSlug);
        for (const section of ALL_SECTIONS) {
          await driver.wsSettingsOpen(seed.workspaceSlug, section);
          expect(await driver.wsSettingsDeniedVisible()).toBe(true);
        }
        expect(await driver.wsSettingsItems()).toEqual([]);
      });

      await test.step("the server refuses out-of-role writes while reads stay allowed", async () => {
        const apiBase = parityApiBase();
        const guestSession = await signInSession(guest.email, guest.password);
        const workspaceUrl = `${apiBase}/api/workspaces/${seed.workspaceSlug}/`;
        const memberPatch = await serverRequestStatus("PATCH", workspaceUrl, member.session, { name: "member rename" });
        expect(memberPatch.status).toBe(403);
        const guestPatch = await serverRequestStatus("PATCH", workspaceUrl, guestSession, { name: "guest rename" });
        expect(guestPatch.status).toBe(403);
        const memberDelete = await serverRequestStatus("DELETE", workspaceUrl, member.session);
        expect(memberDelete.status).toBe(403);
        // Reads stay open where writes are refused: members and guests
        // may read the workspace through the API while the settings
        // pages deny them, so the page gates are UI-only.
        const memberGet = await serverRequestStatus("GET", workspaceUrl, member.session);
        expect(memberGet.status).toBe(200);
        const guestGet = await serverRequestStatus("GET", workspaceUrl, guestSession);
        expect(guestGet.status).toBe(200);
        const facts = await serverWorkspace(seed.workspaceSlug, session);
        expect(facts.slug).toBe(seed.workspaceSlug);
      });
    } finally {
      await serverCleanupWorkspaceMember(
        seed.workspaceSlug,
        await membershipIdFor(seed, session, member.userId),
        session
      );
    }
  }
);

test(
  specTitle(["WSET-003"], "admins edit name, size and timezone with a saving state and a success notice"),
  { tag: specTags(["WSET-003"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
    const before = await serverWorkspace(seed.workspaceSlug, session);
    const nextName = "Parity Workspace NF410";
    try {
      await driver.wsSettingsOpen(seed.workspaceSlug, "");

      await test.step("the form prefills the stored details with the full preset list", async () => {
        expect(await driver.wsSettingsGeneralVisible()).toBe(true);
        expect(await driver.wsSettingsNameValue()).toBe(before.name);
        expect(await driver.wsSettingsSizeOptions()).toEqual([
          "Just myself",
          "2-10",
          "11-50",
          "51-200",
          "201-500",
          "500+",
        ]);
        expect(await driver.wsSettingsUrlValue()).toContain(seed.workspaceSlug);
      });

      await test.step("saving shows the saving state, then the success notice and stored details", async () => {
        await driver.wsSettingsFillName(nextName);
        await driver.wsSettingsSelectSize("11-50");
        await driver.wsSettingsSelectTimezone("Auckland", "Auckland, Wellington");
        await driver.wsSettingsDelaySave(2500);
        try {
          const saving = driver.wsSettingsSave();
          await expect.poll(() => driver.wsSettingsSaveLabel()).toBe("Updating");
          await saving;
        } finally {
          await driver.wsSettingsClearSaveRoutes();
        }
        const toast = await pollToast(driver);
        expect(toast?.title).toBe("Success!");
        expect(toast?.message).toBe("Workspace updated successfully");
        expect(await driver.wsSettingsNameValue()).toBe(nextName);
        expect(await driver.wsSettingsSizeLabel()).toBe("11-50");
        expect(await driver.wsSettingsTimezoneLabel()).toContain("Auckland");
        const facts = await serverWorkspace(seed.workspaceSlug, session);
        expect(facts.name).toBe(nextName);
        expect(facts.organizationSize).toBe("11-50");
        expect(facts.timezone).toBe("Pacific/Auckland");
      });
    } finally {
      const restore = await serverRequestStatus(
        "PATCH",
        `${parityApiBase()}/api/workspaces/${seed.workspaceSlug}/`,
        session,
        {
          name: before.name,
          organization_size: before.organizationSize,
          timezone: before.timezone,
        }
      );
      expect(restore.status).toBe(200);
      const facts = await serverWorkspace(seed.workspaceSlug, session);
      expect(facts.name).toBe(before.name);
    }
  }
);

test(
  specTitle(["WSET-003"], "invalid names and failed saves leave the form and server state intact"),
  { tag: specTags(["WSET-003"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
    const before = await serverWorkspace(seed.workspaceSlug, session);
    await driver.wsSettingsOpen(seed.workspaceSlug, "");

    await test.step("an empty name is rejected inline with no write", async () => {
      await driver.wsSettingsFillName("");
      await driver.wsSettingsSave();
      expect(await driver.wsSettingsNameError()).toBe("Workspace name is required");
      expect(await driver.wsSettingsNameValue()).toBe("");
      expect((await serverWorkspace(seed.workspaceSlug, session)).name).toBe(before.name);
    });

    await test.step("a name with special characters is rejected inline with no write", async () => {
      await driver.wsSettingsFillName("Bad{Name}");
      await driver.wsSettingsSave();
      expect(await driver.wsSettingsNameError()).toContain("special characters");
      expect((await serverWorkspace(seed.workspaceSlug, session)).name).toBe(before.name);
    });

    await test.step("a failed save keeps the form intact with no success notice", async () => {
      const attempt = "Parity Workspace Attempt NF410";
      await driver.wsSettingsFillName(attempt);
      await driver.wsSettingsFailSave();
      try {
        await driver.wsSettingsSave();
      } finally {
        await driver.wsSettingsClearSaveRoutes();
      }
      expect(await pollToast(driver)).toBeNull();
      expect(await driver.wsSettingsNameValue()).toBe(attempt);
      expect(await driver.wsSettingsNameError()).toBeNull();
      expect((await serverWorkspace(seed.workspaceSlug, session)).name).toBe(before.name);
    });
  }
);

test(
  specTitle(["WSET-004"], "logo lifecycle: fallback, upload, replace and removal with notices"),
  { tag: specTags(["WSET-004"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
    await ensureFallbackLogo(driver, seed, session);

    await test.step("the fallback shows with the upload action", async () => {
      expect(await driver.wsSettingsLogoState()).toBe("fallback");
      expect(await driver.wsSettingsLogoAction()).toBe("upload");
    });

    await test.step("uploading closes the dialog with no notice and stores the logo", async () => {
      await driver.wsSettingsOpenLogoDialog();
      expect(await driver.wsSettingsLogoDialogVisible()).toBe(true);
      await driver.wsSettingsUploadLogo(LOGO_A);
      expect(await driver.wsSettingsLogoDialogVisible()).toBe(false);
      expect(await driver.wsSettingsLogoState()).toBe("image");
      expect(await driver.wsSettingsLogoAction()).toBe("edit");
      const src = await driver.wsSettingsLogoSrc();
      expect(src).not.toBeNull();
      await expect.poll(async () => (await serverWorkspace(seed.workspaceSlug, session)).logoUrl).not.toBeNull();
      expect(await pollToast(driver)).toBeNull();
    });

    await test.step("replacing swaps the stored logo with no notice", async () => {
      const firstSrc = await driver.wsSettingsLogoSrc();
      await driver.wsSettingsOpenLogoDialog();
      await driver.wsSettingsUploadLogo(LOGO_B);
      expect(await driver.wsSettingsLogoDialogVisible()).toBe(false);
      const secondSrc = await driver.wsSettingsLogoSrc();
      expect(secondSrc).not.toBeNull();
      expect(secondSrc).not.toBe(firstSrc);
      await expect.poll(async () => (await serverWorkspace(seed.workspaceSlug, session)).logoUrl).not.toBeNull();
      expect(await pollToast(driver)).toBeNull();
    });

    await test.step("removal clears the stored logo with a success notice", async () => {
      await driver.wsSettingsOpenLogoDialog();
      await driver.wsSettingsRemoveLogo();
      expect(await driver.wsSettingsLogoDialogVisible()).toBe(false);
      const toast = await pollToast(driver);
      expect(toast?.title).toBe("Success!");
      expect(toast?.message).toBe("Workspace picture removed successfully.");
      await expect.poll(async () => (await serverWorkspace(seed.workspaceSlug, session)).logoUrl).toBeNull();
      // Bug NEWFRONT-416 pins the stale render without a reload; the
      // re-open here settles the page back on the proven fallback.
      await driver.wsSettingsOpen(seed.workspaceSlug, "");
      expect(await driver.wsSettingsLogoState()).toBe("fallback");
      expect(await driver.wsSettingsLogoAction()).toBe("upload");
    });
  }
);

test(
  specTitle(["WSET-004"], "bug: NEWFRONT-416 removed logo stays rendered until reload"),
  { tag: specTags(["WSET-004"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
    await ensureFallbackLogo(driver, seed, session);
    // Observed on the old app: the success notice shows and the stored
    // logo clears, but the old image stays rendered until the page is
    // reloaded. The new frontend ships the intended behavior (spec §4,
    // §43): the avatar falls back to the initial as soon as removal
    // succeeds, with no reload — the same branching the other bug: rows
    // use where web_new carries the fix.
    await driver.wsSettingsOpenLogoDialog();
    await driver.wsSettingsUploadLogo(LOGO_A);
    expect(await driver.wsSettingsLogoState()).toBe("image");
    await driver.wsSettingsOpenLogoDialog();
    await driver.wsSettingsRemoveLogo();
    const toast = await pollToast(driver);
    expect(toast?.title).toBe("Success!");
    await expect.poll(async () => (await serverWorkspace(seed.workspaceSlug, session)).logoUrl).toBeNull();
    if (driver.target === "web_new") {
      await expect.poll(() => driver.wsSettingsLogoState(), { timeout: 30_000 }).toBe("fallback");
      expect(await driver.wsSettingsLogoSrc()).toBeNull();
      expect(await driver.wsSettingsLogoAction()).toBe("upload");
      return;
    }
    expect(await driver.wsSettingsLogoState()).toBe("image");
    await driver.wsSettingsOpen(seed.workspaceSlug, "");
    expect(await driver.wsSettingsLogoState()).toBe("fallback");
  }
);

test(
  specTitle(["WSET-005"], "copying the workspace URL fills the clipboard and confirms"),
  { tag: specTags(["WSET-005"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const member = await provisionMember(seed, session, "nf410-copy");
    const before = await serverWorkspace(seed.workspaceSlug, session);
    try {
      await test.step("admins copy the full workspace address", async () => {
        await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
        await driver.wsSettingsOpen(seed.workspaceSlug, "");
        await driver.wsSettingsCopyUrl();
        const origin = new URL(driver.page.url()).origin;
        expect(await driver.wsSettingsReadClipboard()).toBe(`${origin}/${seed.workspaceSlug}`);
        const toast = await pollToast(driver);
        expect(toast?.title).toBe("Workspace URL copied to the clipboard.");
      });

      await test.step("members copy the same address", async () => {
        await switchUser(driver, member.email, member.password, seed.workspaceSlug);
        await driver.wsSettingsOpen(seed.workspaceSlug, "");
        await driver.wsSettingsCopyUrl();
        const origin = new URL(driver.page.url()).origin;
        expect(await driver.wsSettingsReadClipboard()).toBe(`${origin}/${seed.workspaceSlug}`);
        const toast = await pollToast(driver);
        expect(toast?.title).toBe("Workspace URL copied to the clipboard.");
      });

      await test.step("copying writes nothing server-side", async () => {
        const facts = await serverWorkspace(seed.workspaceSlug, session);
        expect(facts.name).toBe(before.name);
        expect(facts.slug).toBe(before.slug);
      });
    } finally {
      await serverCleanupWorkspaceMember(
        seed.workspaceSlug,
        await membershipIdFor(seed, session, member.userId),
        session
      );
    }
  }
);

test(
  specTitle(["WSET-006"], "delete danger zone confirms twice and removes the workspace"),
  { tag: specTags(["WSET-006"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const apiBase = parityApiBase();
    const stamp = Date.now().toString(36);
    const slug = `nf410-del-${stamp}`;
    const name = `NF410 Delete ${stamp}`;
    await serverCreateWorkspace(session, name, slug);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.wsSettingsOpen(slug, "");

      await test.step("the danger section names the workspace with a double confirm", async () => {
        expect(await driver.wsSettingsDeleteSectionVisible()).toBe(true);
        await driver.wsSettingsOpenDeleteDialog();
        expect(await driver.wsSettingsDeleteDialogVisible()).toBe(true);
        const text = await driver.wsSettingsDeleteDialogText();
        expect(text).toContain(name);
        expect(text).toContain("delete my workspace");
        expect(await driver.wsSettingsConfirmDeleteEnabled()).toBe(false);
        await driver.wsSettingsFillDeleteName("wrong name");
        await driver.wsSettingsFillDeletePhrase("delete my workspace");
        expect(await driver.wsSettingsConfirmDeleteEnabled()).toBe(false);
        await driver.wsSettingsFillDeleteName(name);
        await driver.wsSettingsFillDeletePhrase("wrong phrase");
        expect(await driver.wsSettingsConfirmDeleteEnabled()).toBe(false);
        await driver.wsSettingsFillDeletePhrase("delete my workspace");
        expect(await driver.wsSettingsConfirmDeleteEnabled()).toBe(true);
      });

      await test.step("cancelling leaves the workspace untouched", async () => {
        await driver.wsSettingsCancelDelete();
        expect(await driver.wsSettingsDeleteDialogVisible()).toBe(false);
        expect((await serverRequestStatus("GET", `${apiBase}/api/workspaces/${slug}/`, session)).status).toBe(200);
      });

      await test.step("confirming deletes the workspace and navigates away", async () => {
        await driver.wsSettingsOpenDeleteDialog();
        await driver.wsSettingsFillDeleteName(name);
        await driver.wsSettingsFillDeletePhrase("delete my workspace");
        await driver.wsSettingsConfirmDelete();
        expect(await driver.wsSettingsPath()).toBe(`/${seed.workspaceSlug}/`);
        const toast = await pollToast(driver);
        expect(toast?.title).toBe("Workspace deleted.");
        expect((await serverRequestStatus("GET", `${apiBase}/api/workspaces/${slug}/`, session)).status).toBe(404);
      });
    } finally {
      await serverDeleteWorkspace(slug, session);
    }
  }
);

test(
  specTitle(["WSET-007"], "tab titles carry the workspace and section names"),
  { tag: specTags(["WSET-007"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const guest = requireGuest(seed);
    const member = await provisionMember(seed, session, "nf410-titles");
    const titled: [string, string][] = [
      ["", "General settings"],
      ["members", "Members"],
      ["billing", "Billing & Plans"],
      ["exports", "Exports"],
      ["webhooks", "Webhooks"],
      ["integrations", "Integrations"],
    ];
    try {
      await test.step("every admin section titles its tab and updates on navigation", async () => {
        await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
        for (const [section, label] of titled) {
          const workspaceName = (await serverWorkspace(seed.workspaceSlug, session)).name;
          await driver.wsSettingsOpen(seed.workspaceSlug, section);
          expect(await driver.wsSettingsTitle()).toBe(`${workspaceName} - ${label}`);
        }
      });

      await test.step("members see titles where admitted and an empty title where refused", async () => {
        await switchUser(driver, member.email, member.password, seed.workspaceSlug);
        for (const [section, label] of titled) {
          const workspaceName = (await serverWorkspace(seed.workspaceSlug, session)).name;
          await driver.wsSettingsOpen(seed.workspaceSlug, section);
          if (ADMIN_ONLY.includes(section)) {
            expect(await driver.wsSettingsDeniedVisible()).toBe(true);
            expect(await driver.wsSettingsTitle()).toBe("");
          } else {
            expect(await driver.wsSettingsDeniedVisible()).toBe(false);
            expect(await driver.wsSettingsTitle()).toBe(`${workspaceName} - ${label}`);
          }
        }
      });

      await test.step("guests see no titles anywhere", async () => {
        await switchUser(driver, guest.email, guest.password, seed.workspaceSlug);
        for (const [section] of titled) {
          await driver.wsSettingsOpen(seed.workspaceSlug, section);
          expect(await driver.wsSettingsDeniedVisible()).toBe(true);
          expect(await driver.wsSettingsTitle()).toBe("");
        }
      });
    } finally {
      await serverCleanupWorkspaceMember(
        seed.workspaceSlug,
        await membershipIdFor(seed, session, member.userId),
        session
      );
    }
  }
);
