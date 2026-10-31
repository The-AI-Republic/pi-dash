// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Parity scenarios (NEWFRONT-415): workspace-settings exports entry,
// instance full-screen states, deep links, and cross-cutting negatives.
// Rows WSET-033..040. Green on apps/web first.
//
// Observed on the running old app (oracle-prod):
// - The exports settings page mounts its guide (format/project form plus
//   history) for workspace admins and members; guests meet the
//   not-authorized view. Form/history behaviors belong to NEWFRONT-59
//   (EXP rows); this row pins the entry point only.
// - When the instance-info read fails, a themed full-screen maintenance
//   message replaces the app (light/dark illustrations, support link);
//   when setup is unfinished, a branded not-ready screen with the admin
//   entry replaces it. Both states are driven here by intercepting the
//   same info read the app shell performs (no server mutation; the
//   server stays healthy throughout, asserted via the info endpoint).
// - Every settings deep link loads its section for authorized roles and
//   the not-authorized view otherwise, without redirecting. Unknown
//   webhook ids 404 server-side while the detail page keeps spinning
//   (bug NEWFRONT-417); forbidden roles meet the denial view.
// - Escape closes the webhook-create (pre-reveal), remove-member, and
//   delete-webhook dialogs, while the invite and token-create dialogs
//   ignore it (bug NEWFRONT-418). Dialogs trap focus in a logical tab
//   order; no other settings shortcut exists; Escape leaves the members
//   search field untouched (a persistent inline input, nothing to
//   collapse).
// - Settings lists read on mount only: server-side mutations stay
//   invisible until reload, with zero list re-reads and zero sockets.
//   The exports history is the carved-out exception: it re-polls while
//   an export runs (EXP-009 owns that behavior).
// - No drag targets, import actions, or file inputs exist on any
//   settings landing page; browsing downloads nothing. (The general
//   page's logo control opens the area's only file picker, inside its
//   own dialog.)
// - Settings carry no desktop-only hooks; the desktop shell serves the
//   same web bundle with no settings route overrides in this checkout.
//   The native desktop build itself is not executed by the oracle; that
//   gap is recorded in WSET-040.
//
// Fixture shape: every scenario mints its own owner and workspace and
// seats whatever member/guest it needs, all through the public API, so
// scenarios never share server state.
import { test, expect } from "../fixtures";
import {
  ROLE,
  browserSessionCookies,
  createWorkspaceForProjects,
  exportsListHistory,
  instanceConfig,
  listInvitations,
  markOnboardedForProjects,
  seatFreshMember,
  serverCreateWorkspaceWebhook,
  serverDeleteApiToken,
  serverDeleteWorkspaceWebhook,
  serverListApiTokens,
  serverListWorkspaceWebhooks,
  serverMintApiToken,
  serverWorkspaceMemberRole,
  serverWorkspaceMembers,
  serverWorkspaceWebhookStatus,
  setLastWorkspaceForProjects,
  signUpAuthedSession,
  uniqueSuffixForProjects,
  type AuthedSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

const ROW_033 = ["WSET-033"];
const ROW_034 = ["WSET-034"];
const ROW_035 = ["WSET-035"];
const ROW_036 = ["WSET-036"];
const ROW_037 = ["WSET-037"];
const ROW_038 = ["WSET-038"];
const ROW_039 = ["WSET-039"];
const ROW_040 = ["WSET-040"];

type WsSection = "general" | "members" | "billing" | "exports" | "webhooks" | "integrations";

const SECTIONS: WsSection[] = ["general", "members", "billing", "exports", "webhooks", "integrations"];

/** Content marker proving a deep link landed on that section (admin view). */
const SECTION_MARKERS: Record<WsSection, string> = {
  general: "Update workspace",
  members: "Add member",
  billing: "Upgrade to Pro",
  exports: "Refresh status",
  webhooks: "Add webhook",
  integrations: "Connect",
};

/**
 * Member-visible equivalents: members see the general form without its
 * save control and the members list without the invite control.
 */
const MEMBER_MARKERS: Partial<Record<WsSection, string>> = {
  general: "Workspace name",
  members: "Filters",
  exports: "Refresh status",
};

function sectionPath(slug: string, section: WsSection): string {
  return section === "general" ? `/${slug}/settings` : `/${slug}/settings/${section}`;
}

interface WsWorld {
  owner: AuthedSession;
  member: AuthedSession;
  guest: AuthedSession;
  workspaceSlug: string;
  workspaceId: string;
}

/** Fresh owner plus workspace with a seated member and guest, ready to browse. */
async function setupWsWorld(prefix: string): Promise<WsWorld> {
  const owner = await signUpAuthedSession(prefix);
  const tag = uniqueSuffixForProjects();
  const ws = await createWorkspaceForProjects(owner, { name: `Wset WS ${tag}`, slug: `wset-${tag}` });
  await markOnboardedForProjects(owner);
  await setLastWorkspaceForProjects(owner, ws.id);
  const member = await seatFreshMember(owner, ws.slug, ROLE.MEMBER, `${prefix}-m`);
  await markOnboardedForProjects(member);
  await setLastWorkspaceForProjects(member, ws.id);
  const guest = await seatFreshMember(owner, ws.slug, ROLE.GUEST, `${prefix}-g`);
  await markOnboardedForProjects(guest);
  await setLastWorkspaceForProjects(guest, ws.id);
  return { owner, member, guest, workspaceSlug: ws.slug, workspaceId: ws.id };
}

test(
  specTitle(ROW_033, "exports settings entry mounts the guide form and history for admin/member, denial for guest"),
  { tag: specTags(ROW_033) },
  async ({ driver }) => {
    const world = await setupWsWorld("parity-wset033");

    await test.step("admin sees the guide form and the history section", async () => {
      await driver.exportsSettingsOpen(world.workspaceSlug, browserSessionCookies(world.owner));
      expect(await driver.exportsSettingsAccess()).toBe("form");
      expect((await driver.exportsSettingsHeading()).title).toBe("Exports");
      expect(await driver.exportsSettingsProjectPickerLabel()).toBe("All projects");
      expect(await driver.exportsSettingsFormatLabel()).toBe("CSV");
      expect(await driver.exportsSettingsSubmitLabel()).toBe("Export");
      const server = await exportsListHistory(world.owner, world.workspaceSlug);
      expect(await driver.exportsSettingsHistoryCount()).toBe(server.results.length);
    });

    await test.step("member reaches the same entry", async () => {
      await driver.exportsSettingsOpen(world.workspaceSlug, browserSessionCookies(world.member));
      expect(await driver.exportsSettingsAccess()).toBe("form");
    });

    await test.step("guest is blocked", async () => {
      await driver.exportsSettingsOpen(world.workspaceSlug, browserSessionCookies(world.guest));
      expect(await driver.exportsSettingsAccess()).toBe("not-authorized");
    });
  }
);

test(
  specTitle(ROW_034, "instance failure renders the themed maintenance screen and the app returns after"),
  { tag: specTags(ROW_034) },
  async ({ driver }) => {
    const world = await setupWsWorld("parity-wset034");
    const cookies = browserSessionCookies(world.owner);

    await test.step("server is healthy before the failure is simulated", async () => {
      expect(typeof (await instanceConfig())).toBe("object");
    });

    await test.step("light scheme shows the message, illustration, and support link", async () => {
      await driver.wsSettingsSetColorScheme("light");
      await driver.wsSettingsFailInstanceInfo();
      await driver.openAuthenticated(sectionPath(world.workspaceSlug, "members"), cookies);
      await expect.poll(() => driver.wsSettingsMaintenanceVisible(), { timeout: 60_000 }).toBe(true);
      expect(await driver.wsSettingsMaintenanceIllustration()).toContain("maintenance-mode-light");
      expect(await driver.wsSettingsMaintenanceSupportHref()).toBe("mailto:support@airepublic.com");
      expect(await driver.wsSettingsSidebarVisible()).toBe(false);
    });

    await test.step("dark scheme swaps the illustration", async () => {
      await driver.wsSettingsSetColorScheme("dark");
      await driver.openAuthenticated(sectionPath(world.workspaceSlug, "members"), cookies);
      await expect.poll(() => driver.wsSettingsMaintenanceVisible(), { timeout: 60_000 }).toBe(true);
      expect(await driver.wsSettingsMaintenanceIllustration()).toContain("maintenance-mode-dark");
      await driver.wsSettingsSetColorScheme("light");
    });

    await test.step("the app returns once the failure clears, server untouched", async () => {
      await driver.wsSettingsResetInstanceRoutes();
      await driver.wsSettingsOpenSection(world.workspaceSlug, "members", cookies);
      expect(await driver.wsSettingsMaintenanceVisible()).toBe(false);
      expect(await driver.wsSettingsNotAuthorized()).toBe(false);
      expect(await driver.wsSettingsSidebarVisible()).toBe(true);
      expect(typeof (await instanceConfig())).toBe("object");
    });
  }
);

test(
  specTitle(ROW_035, "unfinished setup renders the branded not-ready screen with admin entry and no workspace content"),
  { tag: specTags(ROW_035) },
  async ({ driver }) => {
    const world = await setupWsWorld("parity-wset035");
    const cookies = browserSessionCookies(world.owner);

    await driver.wsSettingsServeInstanceNotReady();
    await driver.openAuthenticated(sectionPath(world.workspaceSlug, "members"), cookies);
    await expect
      .poll(() => driver.wsSettingsNotReadyFacts().then((facts) => facts.heading), { timeout: 60_000 })
      .toBe("Welcome to Pi Dash");

    const facts = await driver.wsSettingsNotReadyFacts();
    expect(facts.getStartedHref).toContain("/god-mode");
    expect(facts.workspaceContent).toBe(false);
    expect(await driver.wsSettingsSidebarVisible()).toBe(false);

    await test.step("the app returns once setup reads finished, server untouched", async () => {
      await driver.wsSettingsResetInstanceRoutes();
      await driver.wsSettingsOpenSection(world.workspaceSlug, "members", cookies);
      expect(await driver.wsSettingsSidebarVisible()).toBe(true);
      expect((await driver.wsSettingsNotReadyFacts()).heading).not.toBe("Welcome to Pi Dash");
      expect(typeof (await instanceConfig())).toBe("object");
    });
  }
);

test(
  specTitle(ROW_036, "settings deep links resolve per section and role without redirecting"),
  { tag: specTags(ROW_036) },
  async ({ driver }) => {
    const world = await setupWsWorld("parity-wset036");

    await test.step("server roles match the seats", async () => {
      expect(await serverWorkspaceMemberRole(world.owner, world.workspaceSlug)).toBe(ROLE.ADMIN);
      expect(await serverWorkspaceMemberRole(world.member, world.workspaceSlug)).toBe(ROLE.MEMBER);
      expect(await serverWorkspaceMemberRole(world.guest, world.workspaceSlug)).toBe(ROLE.GUEST);
    });

    const adminOnly: WsSection[] = ["billing", "webhooks", "integrations"];
    const cases: Array<{ session: AuthedSession; denied: WsSection[]; memberView: boolean }> = [
      { session: world.owner, denied: [], memberView: false },
      { session: world.member, denied: adminOnly, memberView: true },
      { session: world.guest, denied: SECTIONS, memberView: false },
    ];
    for (const { session, denied, memberView } of cases) {
      const cookies = browserSessionCookies(session);
      for (const section of SECTIONS) {
        const expectedPath = sectionPath(world.workspaceSlug, section);
        await driver.wsSettingsOpenSection(world.workspaceSlug, section, cookies);
        expect(await driver.wsSettingsCurrentPath()).toBe(expectedPath);
        if (denied.includes(section)) {
          expect(await driver.wsSettingsNotAuthorized()).toBe(true);
          expect(await driver.wsSettingsSidebarVisible()).toBe(false);
        } else {
          expect(await driver.wsSettingsNotAuthorized()).toBe(false);
          expect(await driver.wsSettingsSidebarVisible()).toBe(true);
          const marker = memberView ? (MEMBER_MARKERS[section] ?? SECTION_MARKERS[section]) : SECTION_MARKERS[section];
          // Section bodies hydrate after the shell (integrations waits on
          // provider checks), so poll for the marker, not the shell.
          await expect.poll(() => driver.wsSettingsOutletText(), { timeout: 60_000 }).toContain(marker);
        }
      }
    }
  }
);

test(
  specTitle(ROW_036, "bug:417 unknown webhook ids 404 server-side while the detail page keeps spinning"),
  { tag: specTags(ROW_036) },
  async ({ driver }) => {
    test.setTimeout(420_000);
    const world = await setupWsWorld("parity-wset036b");
    const unknownId = "00000000-0000-0000-0000-000000000000";

    await test.step("unknown id 404s on the server", async () => {
      expect(await serverWorkspaceWebhookStatus(world.owner, world.workspaceSlug, unknownId)).toBe(404);
    });

    await test.step("admin sees loading that never resolves, URL unchanged", async () => {
      await driver.wsSettingsOpenWebhookDetail(world.workspaceSlug, unknownId, browserSessionCookies(world.owner));
      expect(await driver.wsSettingsWebhookDetailState()).toBe("loading");
      expect(await driver.wsSettingsSidebarVisible()).toBe(true);
      expect(await driver.wsSettingsCurrentPath()).toBe(`/${world.workspaceSlug}/settings/webhooks/${unknownId}`);
      await driver.page.waitForTimeout(10_000);
      expect(await driver.wsSettingsWebhookDetailState()).toBe("loading");
    });

    await test.step("forbidden roles meet the denial view", async () => {
      for (const session of [world.member, world.guest]) {
        await driver.wsSettingsOpenWebhookDetail(world.workspaceSlug, unknownId, browserSessionCookies(session));
        expect(await driver.wsSettingsWebhookDetailState()).toBe("denied");
        expect(await driver.wsSettingsCurrentPath()).toBe(`/${world.workspaceSlug}/settings/webhooks/${unknownId}`);
      }
    });
  }
);

test(
  specTitle(ROW_037, "Escape closes the webhook-create, remove-member, and delete-webhook dialogs"),
  { tag: specTags(ROW_037) },
  async ({ driver }) => {
    const world = await setupWsWorld("parity-wset037a");

    await test.step("webhook create closes, nothing created server-side", async () => {
      await driver.wsSettingsOpenWebhookCreateDialog(world.workspaceSlug, browserSessionCookies(world.owner));
      expect(await driver.wsSettingsDialogOpen()).toBe(true);
      await driver.wsSettingsPressEscape();
      expect(await driver.wsSettingsDialogOpen()).toBe(false);
      expect(await serverListWorkspaceWebhooks(world.owner, world.workspaceSlug)).toHaveLength(0);
    });

    await test.step("remove-member confirm closes, member kept server-side", async () => {
      const victim = await seatFreshMember(world.owner, world.workspaceSlug, ROLE.MEMBER, "parity-wset037a-v");
      await markOnboardedForProjects(victim);
      await driver.wsSettingsOpenRemoveMemberDialog(
        world.workspaceSlug,
        victim.email,
        browserSessionCookies(world.owner)
      );
      expect(await driver.wsSettingsDialogOpen()).toBe(true);
      expect((await driver.wsSettingsDialogButtons()).length).toBeGreaterThanOrEqual(2);
      await driver.wsSettingsPressEscape();
      expect(await driver.wsSettingsDialogOpen()).toBe(false);
      expect(await serverWorkspaceMemberRole(victim, world.workspaceSlug)).toBe(ROLE.MEMBER);
    });

    await test.step("delete-webhook confirm closes, webhook kept server-side", async () => {
      const hook = await serverCreateWorkspaceWebhook(world.owner, world.workspaceSlug, {
        url: "https://example.com/wset037a",
      });
      try {
        await driver.wsSettingsOpenDeleteWebhookDialog(
          world.workspaceSlug,
          hook.id,
          browserSessionCookies(world.owner)
        );
        expect(await driver.wsSettingsDialogButtons()).toEqual(["Cancel", "Delete"]);
        await driver.wsSettingsPressEscape();
        expect(await driver.wsSettingsDialogOpen()).toBe(false);
        expect(await serverWorkspaceWebhookStatus(world.owner, world.workspaceSlug, hook.id)).toBe(200);
      } finally {
        await serverDeleteWorkspaceWebhook(world.owner, world.workspaceSlug, hook.id);
      }
    });
  }
);

test(
  specTitle(
    ROW_037,
    "bug:418 invite and token-create dialogs need explicit dismissal on the old app (the new app closes them on Escape, §37)"
  ),
  { tag: specTags(ROW_037) },
  async ({ driver }) => {
    const world = await setupWsWorld("parity-wset037b");
    // §37 target: the new frontend closes the invite and token-create
    // dialogs on Escape like their siblings. The old app ignores it
    // (NEWFRONT-418), so that leg is pinned for the oracle only.
    const escapeCloses = driver.target === "web_new";
    // The invite dialog's submit control is named for its action; the
    // wording is not pinned text, so accept either label.
    const sendLabel = /^(Send invitations|Send invites)$/;

    await test.step("invite dialog Escape behavior, no invitation created either way", async () => {
      const before = await listInvitations(world.workspaceSlug, world.owner.cookie);
      await driver.wsSettingsOpenInviteDialog(world.workspaceSlug, browserSessionCookies(world.owner));
      const buttons = await driver.wsSettingsDialogButtons();
      expect(buttons).toContain("Cancel");
      expect(buttons.some((label) => sendLabel.test(label))).toBe(true);
      await driver.wsSettingsPressEscape();
      expect(await driver.wsSettingsDialogOpen()).toBe(!escapeCloses);
      if (await driver.wsSettingsDialogOpen()) {
        await driver.wsSettingsCloseDialogExplicit();
      }
      expect(await driver.wsSettingsDialogOpen()).toBe(false);
      const after = await listInvitations(world.workspaceSlug, world.owner.cookie);
      expect(after.map((inv) => inv.id).sort()).toEqual(before.map((inv) => inv.id).sort());
    });

    await test.step("token-create dialog Escape behavior, no token minted either way", async () => {
      await driver.wsSettingsOpenTokenCreateDialog(browserSessionCookies(world.owner));
      expect(await driver.wsSettingsDialogOpen()).toBe(true);
      await driver.wsSettingsPressEscape();
      expect(await driver.wsSettingsDialogOpen()).toBe(!escapeCloses);
      if (await driver.wsSettingsDialogOpen()) {
        await driver.wsSettingsCloseDialogExplicit();
      }
      expect(await driver.wsSettingsDialogOpen()).toBe(false);
      expect(await serverListApiTokens(world.owner)).toHaveLength(0);
    });
  }
);

test(
  specTitle(ROW_037, "settings dialogs trap focus in a logical tab order"),
  { tag: specTags(ROW_037) },
  async ({ driver }) => {
    const world = await setupWsWorld("parity-wset037c");

    await test.step("invite dialog cycles its controls without leaving", async () => {
      await driver.wsSettingsOpenInviteDialog(world.workspaceSlug, browserSessionCookies(world.owner));
      const { order, escaped } = await driver.wsSettingsDialogTabOrder(8);
      expect(escaped).toBe(false);
      expect(order).toHaveLength(8);
      expect(order.every((label) => label !== "" && label !== "<none>")).toBe(true);
      expect(order).toContain("input:name@company.com");
      expect(order.some((label) => label === "button:Send invitations" || label === "button:Send invites")).toBe(true);
      await driver.wsSettingsCloseDialogExplicit();
    });

    await test.step("webhook-create dialog keeps focus inside", async () => {
      await driver.wsSettingsOpenWebhookCreateDialog(world.workspaceSlug, browserSessionCookies(world.owner));
      const { order, escaped } = await driver.wsSettingsDialogTabOrder(6);
      expect(escaped).toBe(false);
      expect(order).toHaveLength(6);
      expect(order.every((label) => label !== "" && label !== "<none>")).toBe(true);
      await driver.wsSettingsPressEscape();
    });
  }
);

test(
  specTitle(ROW_037, "no other keyboard shortcut acts on settings; Escape leaves the members search untouched"),
  { tag: specTags(ROW_037) },
  async ({ driver }) => {
    const world = await setupWsWorld("parity-wset037d");
    const cookies = browserSessionCookies(world.owner);

    await test.step("representative keys open nothing and navigate nowhere", async () => {
      await driver.wsSettingsOpenSection(world.workspaceSlug, "members", cookies);
      const path = await driver.wsSettingsCurrentPath();
      await driver.wsSettingsPressKeys(["e", "m", "?", "/", "Enter", "n", "c", "Delete"]);
      expect(await driver.wsSettingsDialogOpen()).toBe(false);
      expect(await driver.wsSettingsCurrentPath()).toBe(path);
      expect(await driver.wsSettingsOutletText()).toContain("Add member");
    });

    await test.step("Escape neither clears nor collapses the members search", async () => {
      const probe = await driver.wsSettingsMembersSearchEscapeProbe("zzz-no-match");
      expect(probe).toEqual({ value: "zzz-no-match", visible: true });
    });
  }
);

test(
  specTitle(ROW_038, "settings lists never refresh without navigation: mutations stay invisible until reload"),
  { tag: specTags(ROW_038) },
  async ({ driver }) => {
    test.setTimeout(420_000);
    const world = await setupWsWorld("parity-wset038");

    await test.step("members: a seated member appears only after reload, with no re-reads or sockets", async () => {
      // Sockets must never fire at all, so this watcher arms before the
      // page even loads and is never re-armed. The list watcher also
      // arms early; the page fires its mount reads in a burst, so after
      // the rows render, drain and re-arm at zero for the window.
      await driver.wsSettingsWatchSockets();
      await driver.wsSettingsWatchListRequests();
      await driver.wsSettingsOpenSection(world.workspaceSlug, "members", browserSessionCookies(world.owner));
      await expect
        .poll(() => driver.wsSettingsMembersListText().then((rows) => rows.join("\n")), { timeout: 60_000 })
        .toContain(world.owner.email);
      await driver.page.waitForTimeout(5000);
      await driver.wsSettingsWatchListRequests();
      const before = await driver.wsSettingsMembersListText();
      const late = await seatFreshMember(world.owner, world.workspaceSlug, ROLE.MEMBER, "parity-wset038-l");
      expect(await serverWorkspaceMemberRole(late, world.workspaceSlug)).toBe(ROLE.MEMBER);
      await driver.page.waitForTimeout(10_000);
      expect(await driver.wsSettingsMembersListText()).toEqual(before);
      expect(await driver.wsSettingsListRequestCount()).toBe(0);
      expect(await driver.wsSettingsSocketCount()).toBe(0);
      await driver.wsSettingsOpenSection(world.workspaceSlug, "members", browserSessionCookies(world.owner));
      await expect
        .poll(() => driver.wsSettingsMembersListText().then((rows) => rows.join("\n")), { timeout: 60_000 })
        .toContain(late.email);
      const after = await driver.wsSettingsMembersListText();
      expect(after.length).toBe(before.length + 1);
    });

    await test.step("webhooks: a created webhook appears only after reload", async () => {
      // Arm before navigating: the empty list's mount read can land
      // before the shell settles, so observe the read itself, drain,
      // then re-arm at zero for the window.
      await driver.wsSettingsWatchListRequests();
      await driver.wsSettingsOpenSection(world.workspaceSlug, "webhooks", browserSessionCookies(world.owner));
      await expect.poll(() => driver.wsSettingsListRequestCount(), { timeout: 60_000 }).toBeGreaterThanOrEqual(1);
      await driver.page.waitForTimeout(5000);
      await driver.wsSettingsWatchListRequests();
      const before = await driver.wsSettingsWebhooksListText();
      const hook = await serverCreateWorkspaceWebhook(world.owner, world.workspaceSlug, {
        url: "https://example.com/wset038",
      });
      try {
        await driver.page.waitForTimeout(10_000);
        expect(await driver.wsSettingsWebhooksListText()).toEqual(before);
        expect(await driver.wsSettingsListRequestCount()).toBe(0);
        await driver.wsSettingsOpenSection(world.workspaceSlug, "webhooks", browserSessionCookies(world.owner));
        await expect
          .poll(() => driver.wsSettingsWebhooksListText(), { timeout: 60_000 })
          .toContain("https://example.com/wset038");
      } finally {
        await serverDeleteWorkspaceWebhook(world.owner, world.workspaceSlug, hook.id);
      }
    });

    await test.step("tokens: a minted token triggers no re-read", async () => {
      await driver.wsSettingsWatchListRequests();
      await driver.openAuthenticated("/settings/profile/api-tokens", browserSessionCookies(world.owner));
      await expect.poll(() => driver.wsSettingsListRequestCount(), { timeout: 60_000 }).toBeGreaterThanOrEqual(1);
      await driver.page.waitForTimeout(5000);
      await driver.wsSettingsWatchListRequests();
      const minted = await serverMintApiToken(world.owner.cookie, "wset038probe");
      try {
        expect((await serverListApiTokens(world.owner)).map((t) => t.id)).toContain(minted.id);
        await driver.page.waitForTimeout(10_000);
        expect(await driver.wsSettingsListRequestCount()).toBe(0);
      } finally {
        await serverDeleteApiToken(minted.id, world.owner.cookie);
      }
    });
  }
);

test(
  specTitle(ROW_039, "settings lists offer no drag targets, import actions, or downloads while browsing"),
  { tag: specTags(ROW_039) },
  async ({ driver }) => {
    const world = await setupWsWorld("parity-wset039");
    const cookies = browserSessionCookies(world.owner);

    await test.step("no section landing page carries drag, file, or import affordances", async () => {
      for (const section of SECTIONS) {
        await driver.wsSettingsOpenSection(world.workspaceSlug, section, cookies);
        expect(await driver.wsSettingsDraggableCount()).toBe(0);
        expect(await driver.wsSettingsFileInputCount()).toBe(0);
        expect(await driver.wsSettingsImportMentions()).toEqual([]);
      }
    });

    await test.step("dragging a member row reorders nothing, server order kept", async () => {
      await driver.wsSettingsOpenSection(world.workspaceSlug, "members", cookies);
      const serverBefore = (await serverWorkspaceMembers(world.workspaceSlug, world.owner.cookie)).map(
        (m) => m.membershipId
      );
      // The roster replaces its loader once the read lands; this
      // section was opened behind the surface check, not the roster one.
      await expect
        .poll(() => driver.wsSettingsMembersListText().then((rows) => rows.length), { timeout: 60_000 })
        .toBeGreaterThanOrEqual(2);
      const before = await driver.wsSettingsMembersListText();
      expect(before.length).toBeGreaterThanOrEqual(2);
      await driver.wsSettingsDragFirstMemberRowToSecond();
      expect(await driver.wsSettingsMembersListText()).toEqual(before);
      const serverAfter = (await serverWorkspaceMembers(world.workspaceSlug, world.owner.cookie)).map(
        (m) => m.membershipId
      );
      expect(serverAfter).toEqual(serverBefore);
    });

    await test.step("browsing sections downloads nothing", async () => {
      await driver.wsSettingsWatchDownloads();
      for (const section of ["members", "webhooks", "exports"] as const) {
        await driver.wsSettingsOpenSection(world.workspaceSlug, section, cookies);
      }
      await driver.openAuthenticated("/settings/profile/api-tokens", cookies);
      await expect
        .poll(() => driver.wsSettingsOutletText(), { timeout: 60_000 })
        .toContain("Add personal access token");
      expect(await driver.wsSettingsDownloadCount()).toBe(0);
    });
  }
);

test(
  specTitle(ROW_040, "settings sections carry no desktop-only hooks; the desktop shell serves the same pages"),
  { tag: specTags(ROW_040) },
  async ({ driver }) => {
    const world = await setupWsWorld("parity-wset040");
    const cookies = browserSessionCookies(world.owner);

    for (const section of SECTIONS) {
      await driver.wsSettingsOpenSection(world.workspaceSlug, section, cookies);
      expect(await driver.wsSettingsSidebarVisible()).toBe(true);
      await expect.poll(() => driver.wsSettingsOutletText(), { timeout: 60_000 }).toContain(SECTION_MARKERS[section]);
      expect(await driver.wsSettingsDesktopOnlyCount()).toBe(0);
    }

    await test.step("token page likewise carries no desktop-only hooks", async () => {
      await driver.openAuthenticated("/settings/profile/api-tokens", cookies);
      await expect
        .poll(() => driver.wsSettingsOutletText(), { timeout: 60_000 })
        .toContain("Add personal access token");
      expect(await driver.wsSettingsDesktopOnlyCount()).toBe(0);
    });
  }
);
