// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-446): instance-admin dashboard shell and
// negative rows. Rows ADM-062 (sidebar navigation), ADM-063 (sidebar
// collapse, persistence, responsive), ADM-064 (account dropdown
// identity), ADM-065 (theme toggle), ADM-066 (header breadcrumbs),
// ADM-067 (help section plus version), ADM-068 (mobile hamburger),
// ADM-069 (no realtime), ADM-070 (no drag-drop/import/export),
// ADM-071 (no custom shortcuts), ADM-072 (web-only, no desktop) and
// ADM-073 (delete-confirmation gaps).
//
// Observed behavior notes (paraphrased; the inventory rows carry them
// at update time): the sidebar lists the settings areas in a fixed
// order with a name, a one-line description and a route each, and the
// entry matching the current path carries the highlight; the oracle
// still lists the image area and has no runners entry (bug:
// NEWFRONT-493, NEWFRONT-494), while the new console builds the
// intended rail (image gone, runners present). A footer control
// collapses the rail to
// icons with hover labels and persists the choice locally; narrow
// viewports collapse it one-way and an outside tap on small screens
// hides the off-canvas rail again. The sidebar head shows the admin
// label plus an avatar control opening a menu with the account email,
// the theme switch and the sign-out; collapsed, the same menu hangs off
// an icon control. The theme item names the mode it switches to, the
// choice persists, and signing out resets it to the system default. The
// header leads with a settings crumb back to general settings, then one
// crumb per path segment (last dropped) from a fixed label map with an
// uppercased fallback, every crumb a link; long labels clip with the
// full text on hover. The footer also links out to the main app and
// opens a help popover with external documentation, forum and
// bug-report links plus the running instance version. Below the tablet
// breakpoint a header hamburger opens the off-canvas rail (the open rail
// covers the control, so closing goes through the footer control or an
// outside tap). Nothing updates live: the settings pages share
// one cached configurations read with no sockets, streams or polling,
// while list mutations revalidate their read explicitly.
// The console has no drag sources, file inputs, import/export or
// desktop affordances, no bespoke shortcuts, and loop deletion goes
// through immediately while workspaces offer no deletion at all.
import { readdirSync, readFileSync, statSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { test, expect } from "../fixtures";
import {
  serverAdminCreateLoopJob,
  serverAdminDeleteLoopJobsBySlugPrefix,
  serverAdminGetLoopJob,
  serverAdminInstanceInfo,
  serverAdminInstanceVersion,
  serverAdminListLoopJobs,
  serverAdminListWorkspaces,
  serverAdminMe,
  serverAdminSession,
  uniqueSlug,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import type { ParityDriver, ParitySeedFacts, ParityTarget } from "../drivers/parity-driver";

/**
 * Sidebar entries in render order: name, route slug. The oracle still
 * lists the image area and lacks the runners page; the new console
 * builds the intended order (no image per main#523, runners per
 * main#534 — NEWFRONT-493/494), so each target reads its own rail.
 */
const SIDEBAR_ORDER: Record<ParityTarget, Array<[string, string]>> = {
  web: [
    ["General", "general"],
    ["Email", "email"],
    ["Authentication", "authentication"],
    ["Workspaces", "workspace"],
    ["Artificial intelligence", "ai"],
    ["Loop", "loop"],
    ["Images in Pi Dash", "image"],
  ],
  web_new: [
    ["General", "general"],
    ["Email", "email"],
    ["Authentication", "authentication"],
    ["Workspaces", "workspace"],
    ["Artificial intelligence", "ai"],
    ["Loop", "loop"],
    ["Runners", "runners"],
  ],
};

/** The rail this run must show. */
function sidebarOrder(driver: ParityDriver): Array<[string, string]> {
  return SIDEBAR_ORDER[driver.target];
}

/** Local sleep for proving an absence (no refetch, no transport) over a window. */
function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

/** Wait until the dashboard sidebar renders its entries (works collapsed too). */
async function waitForSidebar(driver: ParityDriver): Promise<void> {
  // The gate branch reads the "Instance admin" label, which unmounts
  // when collapsed — entry presence settles in either state.
  const deadline = Date.now() + 30_000;
  for (;;) {
    if ((await driver.adminSidebarFacts()).entries.length > 0) return;
    if (Date.now() >= deadline) throw new Error("[parity] dashboard sidebar never rendered.");
    await sleep(250);
  }
}

/** Wait until the sidebar rail settles open (x ~= 0) or off-canvas. */
async function waitForSidebarX(driver: ParityDriver, want: "open" | "closed"): Promise<void> {
  // The collapse flag flips on click but the rail animates over 300ms,
  // so geometry assertions wait out the transition.
  const deadline = Date.now() + 10_000;
  for (;;) {
    const x = (await driver.adminSidebarFacts()).x ?? 0;
    if (want === "open" ? x > -1 : x < -200) return;
    if (Date.now() >= deadline) throw new Error(`[parity] sidebar rail never settled ${want} (x=${x}).`);
    await sleep(100);
  }
}

/** Sign in and settle on the dashboard entry page. */
async function openDashboard(driver: ParityDriver, seed: ParitySeedFacts): Promise<void> {
  await driver.adminEnsureSignedIn(seed.email, seed.password);
  await driver.adminOpenPath("/general/");
  await waitForSidebar(driver);
}

/** Expand the sidebar when a scenario needs the labeled rail. */
async function ensureExpanded(driver: ParityDriver): Promise<void> {
  if ((await driver.adminSidebarFacts()).collapsed) await driver.adminSidebarToggleCollapse();
}

test(
  specTitle(["ADM-062"], "sidebar lists every settings area with name, description and route"),
  { tag: specTags(["ADM-062"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    await openDashboard(driver, seed);
    const facts = await driver.adminSidebarFacts();
    expect(facts.collapsed).toBe(false);
    expect(facts.entries.map((entry) => entry.name)).toEqual(sidebarOrder(driver).map(([name]) => name));
    for (const [name, slug] of sidebarOrder(driver)) {
      const entry = facts.entries.find((row) => row.name === name);
      expect(entry?.description).not.toBe("");
      expect(entry?.href).toBe(`/god-mode/${slug}/`);
    }
    // No per-item gating: every entry shows for the signed-in admin.
    expect(facts.entries).toHaveLength(sidebarOrder(driver).length);
    const info = await serverAdminInstanceInfo();
    expect(info.isSetupDone).toBe(true);
    expect(info.instanceId).not.toBe("");
  }
);

test(
  specTitle(["ADM-062"], "bug: NEWFRONT-493 NEWFRONT-494 image entry present, runners entry absent"),
  { tag: specTags(["ADM-062"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    await openDashboard(driver, seed);
    const facts = await driver.adminSidebarFacts();
    // Intended: no image entry (removal target per main#523) and a
    // runners entry (target per main#534). The oracle shows the reverse;
    // the new console builds the intended rail.
    const image = facts.entries.some((entry) => entry.name === "Images in Pi Dash");
    const runners = facts.entries.some((entry) => entry.name === "Runners");
    if (driver.target === "web_new") {
      expect(image).toBe(false);
      expect(runners).toBe(true);
    } else {
      expect(image).toBe(true);
      expect(runners).toBe(false);
    }
  }
);

test(
  specTitle(["ADM-062"], "following each entry routes to its page and moves the active highlight"),
  { tag: specTags(["ADM-062"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    await openDashboard(driver, seed);
    const before = await serverAdminInstanceInfo();
    for (const [name, slug] of sidebarOrder(driver)) {
      await driver.adminSidebarFollowEntry(name);
      expect(await driver.adminCurrentPath()).toBe(`/god-mode/${slug}/`);
      const facts = await driver.adminSidebarFacts();
      for (const entry of facts.entries) {
        expect(entry.active).toBe(entry.name === name);
      }
    }
    // Navigation reads; it mutates no server state.
    expect(await serverAdminInstanceInfo()).toEqual(before);
  }
);

test(
  specTitle(["ADM-063"], "collapse control narrows to icons, persists, and restores after reload"),
  { tag: specTags(["ADM-063"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    await openDashboard(driver, seed);
    await ensureExpanded(driver);
    await driver.adminSidebarToggleCollapse();
    let facts = await driver.adminSidebarFacts();
    expect(facts.collapsed).toBe(true);
    expect(facts.persistedCollapsed).toBe(true);
    // Icons keep their routes; only the labels hide.
    expect(facts.entries.map((entry) => entry.href)).toEqual(
      sidebarOrder(driver).map(([, slug]) => `/god-mode/${slug}/`)
    );
    expect(facts.entries.every((entry) => entry.name === "" && entry.description === "")).toBe(true);
    await driver.adminOpenPath("/general/");
    await waitForSidebar(driver);
    facts = await driver.adminSidebarFacts();
    expect(facts.collapsed).toBe(true);
    expect(facts.persistedCollapsed).toBe(true);
    await driver.adminSidebarToggleCollapse();
    facts = await driver.adminSidebarFacts();
    expect(facts.collapsed).toBe(false);
    expect(facts.persistedCollapsed).toBe(false);
    expect(facts.entries.map((entry) => entry.name)).toEqual(sidebarOrder(driver).map(([name]) => name));
  }
);

test(
  specTitle(["ADM-063"], "narrow viewport auto-collapses the sidebar one-way"),
  { tag: specTags(["ADM-063"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    await driver.setViewportSize(1440, 900);
    await openDashboard(driver, seed);
    await ensureExpanded(driver);
    await driver.setViewportSize(700, 900);
    const deadline = Date.now() + 10_000;
    for (;;) {
      if ((await driver.adminSidebarFacts()).collapsed) break;
      if (Date.now() >= deadline) throw new Error("[parity] narrow viewport never auto-collapsed the sidebar.");
      await sleep(250);
    }
    expect((await driver.adminSidebarFacts()).persistedCollapsed).toBe(true);
    // Widening back never re-expands: the resize rule only collapses.
    await driver.setViewportSize(1440, 900);
    await sleep(500);
    const facts = await driver.adminSidebarFacts();
    expect(facts.collapsed).toBe(true);
    expect(facts.persistedCollapsed).toBe(true);
  }
);

test(
  specTitle(["ADM-063"], "outside click on small screens collapses the open sidebar"),
  { tag: specTags(["ADM-063"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    await openDashboard(driver, seed);
    // Narrow first, then reload: the small-screen mount auto-collapses
    // the rail off-canvas.
    await driver.setViewportSize(700, 900);
    await driver.adminOpenPath("/general/");
    await waitForSidebar(driver);
    let facts = await driver.adminSidebarFacts();
    expect(facts.collapsed).toBe(true);
    await waitForSidebarX(driver, "closed");
    await driver.adminHamburgerToggle();
    await waitForSidebarX(driver, "open");
    facts = await driver.adminSidebarFacts();
    expect(facts.collapsed).toBe(false);
    await driver.adminSidebarOutsideClick();
    const deadline = Date.now() + 10_000;
    for (;;) {
      if ((await driver.adminSidebarFacts()).collapsed) break;
      if (Date.now() >= deadline) throw new Error("[parity] outside click never collapsed the sidebar.");
      await sleep(250);
    }
    await waitForSidebarX(driver, "closed");
  }
);

test(
  specTitle(["ADM-063"], "collapsed icons show hover labels, expanded entries show none"),
  { tag: specTags(["ADM-063"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    await openDashboard(driver, seed);
    await ensureExpanded(driver);
    // Labeled entries carry no hover tooltip.
    expect(await driver.adminSidebarEntryTooltip("/god-mode/general/", "General")).toBe(null);
    await driver.adminSidebarToggleCollapse();
    for (const [name, slug] of sidebarOrder(driver)) {
      expect(await driver.adminSidebarEntryTooltip(`/god-mode/${slug}/`, name)).toBe(name);
    }
  }
);

test(
  specTitle(["ADM-064"], "dropdown shows identity, theme switch and sign-out"),
  { tag: specTags(["ADM-064"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    await openDashboard(driver, seed);
    await ensureExpanded(driver);
    await driver.adminAccountOpen();
    const facts = await driver.adminAccountFacts();
    expect(facts.labelVisible).toBe(true);
    expect(facts.avatarVisible).toBe(true);
    expect(facts.menuOpen).toBe(true);
    expect(facts.email).toBe(seed.email);
    expect(facts.themeItemLabel ?? "").toMatch(/^Switch to (light|dark) mode$/);
    expect(facts.signOutVisible).toBe(true);
    expect(facts.signOutAction ?? "").toContain("/api/instances/admins/sign-out/");
    expect(facts.csrfTokenPresent).toBe(true);
    // The menu identity agrees with the server session.
    const session = await serverAdminSession(seed.email, seed.password);
    expect((await serverAdminMe(session)).email).toBe(seed.email);
  }
);

test(
  specTitle(["ADM-064"], "dropdown works collapsed through the icon trigger"),
  { tag: specTags(["ADM-064"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    await openDashboard(driver, seed);
    await ensureExpanded(driver);
    await driver.adminSidebarToggleCollapse();
    await driver.adminAccountOpen();
    const facts = await driver.adminAccountFacts();
    expect(facts.labelVisible).toBe(false);
    expect(facts.avatarVisible).toBe(true);
    expect(facts.menuOpen).toBe(true);
    expect(facts.email).toBe(seed.email);
    expect(facts.themeItemLabel ?? "").toMatch(/^Switch to (light|dark) mode$/);
    expect(facts.signOutVisible).toBe(true);
    expect(facts.csrfTokenPresent).toBe(true);
  }
);

test(
  specTitle(["ADM-065"], "theme menu item flips the mode and persists it"),
  { tag: specTags(["ADM-065"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    await openDashboard(driver, seed);
    await ensureExpanded(driver);
    await driver.adminAccountOpen();
    const label = (await driver.adminAccountFacts()).themeItemLabel ?? "";
    const target = label.includes("dark") ? "dark" : "light";
    await driver.adminSetTheme(target);
    // The theme applies a beat after the menu closes.
    await expect.poll(async () => (await driver.adminThemeFacts()).resolved, { timeout: 10_000 }).toBe(target);
    expect((await driver.adminThemeFacts()).stored).toBe(target);
    // The label now names the way back.
    await driver.adminAccountOpen();
    expect((await driver.adminAccountFacts()).themeItemLabel).toBe(
      `Switch to ${target === "dark" ? "light" : "dark"} mode`
    );
    // The choice survives a reload.
    await driver.adminOpenPath("/general/");
    await waitForSidebar(driver);
    expect((await driver.adminThemeFacts()).resolved).toBe(target);
  }
);

test(
  specTitle(["ADM-065"], "sign-out resets the theme to system"),
  { tag: specTags(["ADM-065"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    await openDashboard(driver, seed);
    await ensureExpanded(driver);
    await driver.adminSetTheme("dark");
    expect((await driver.adminThemeFacts()).stored).toBe("dark");
    await driver.adminSidebarSignOut();
    await driver.adminWaitForRootBranch("sign-in");
    expect((await driver.adminThemeFacts()).stored).toBe("system");
  }
);

test(
  specTitle(["ADM-066"], "trail leads with settings and follows the path with mapped labels"),
  { tag: specTags(["ADM-066"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    await openDashboard(driver, seed);
    const cases: Array<[string, Array<[string, string]>]> = [
      [
        "/general/",
        [
          ["Settings", "/god-mode/general/"],
          ["General", "/god-mode/general/"],
        ],
      ],
      [
        "/workspace/create/",
        [
          ["Settings", "/god-mode/general/"],
          ["Workspace", "/god-mode/workspace/"],
          ["Create", "/god-mode/workspace/create/"],
        ],
      ],
      [
        "/authentication/github/",
        [
          ["Settings", "/god-mode/general/"],
          ["Authentication", "/god-mode/authentication/"],
          ["GitHub", "/god-mode/authentication/github/"],
        ],
      ],
    ];
    for (const [path, want] of cases) {
      await driver.adminOpenPath(path);
      await waitForSidebar(driver);
      const facts = await driver.adminBreadcrumbFacts();
      expect(facts.items.map((item) => [item.label, item.href])).toEqual(want);
    }
  }
);

test(
  specTitle(["ADM-066"], "unmapped segments fall back to uppercase and long labels carry a tooltip"),
  { tag: specTags(["ADM-066"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    const session = await serverAdminSession(seed.email, seed.password);
    const jobs = await serverAdminListLoopJobs(session);
    expect(jobs.length).toBeGreaterThan(0);
    const id = jobs[0]?.id ?? "";
    expect(id).not.toBe("");
    await openDashboard(driver, seed);
    await driver.adminOpenPath(`/loop/${id}/`);
    await waitForSidebar(driver);
    const facts = await driver.adminBreadcrumbFacts();
    // "loop" has no label-map entry, so it uppercases like the raw id.
    expect(facts.items.map((item) => [item.label, item.href])).toEqual([
      ["Settings", "/god-mode/general/"],
      ["LOOP", "/god-mode/loop/"],
      [id.toUpperCase(), `/god-mode/loop/${id}/`],
    ]);
    expect(await driver.adminBreadcrumbTooltip(id.toUpperCase())).toBe(id.toUpperCase());
  }
);

test(
  specTitle(["ADM-067"], "help popover lists external links and the running version"),
  { tag: specTags(["ADM-067"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    await openDashboard(driver, seed);
    await ensureExpanded(driver);
    await driver.adminHelpOpen();
    const facts = await driver.adminHelpFacts();
    expect(facts.popoverOpen).toBe(true);
    // The router normalizes trailing slashes onto the external targets.
    expect(facts.links).toEqual([
      {
        name: "Documentation",
        href: "https://github.com/The-AI-Republic/pi-dash/#readme",
        target: "_blank",
      },
      {
        name: "Join our Forum",
        href: "https://github.com/The-AI-Republic/pi-dash/discussions/",
        target: "_blank",
      },
      {
        name: "Report a bug",
        href: "https://github.com/The-AI-Republic/pi-dash/issues/new/choose/",
        target: "_blank",
      },
    ]);
    expect(facts.versionText).toBe(`Version: v${await serverAdminInstanceVersion()}`);
  }
);

test(
  specTitle(["ADM-067"], "redirect control targets the main app and the footer restacks collapsed"),
  { tag: specTags(["ADM-067"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    await openDashboard(driver, seed);
    await ensureExpanded(driver);
    expect((await driver.adminHelpFacts()).redirectHref).toBe("/");
    await driver.adminSidebarToggleCollapse();
    await driver.adminHelpOpen();
    const facts = await driver.adminHelpFacts();
    expect(facts.redirectHref).toBe("/");
    expect(facts.popoverOpen).toBe(true);
    expect(facts.links.map((link) => link.name)).toEqual(["Documentation", "Join our Forum", "Report a bug"]);
    expect(facts.versionText).toBe(`Version: v${await serverAdminInstanceVersion()}`);
  }
);

test(
  specTitle(["ADM-067"], "collapsed footer controls show hover labels, expanded redirect shows none"),
  { tag: specTags(["ADM-067"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    await openDashboard(driver, seed);
    await ensureExpanded(driver);
    // Expanded, the redirect link carries its own text, so its tooltip
    // stays disabled.
    expect(await driver.adminFooterTooltip("redirect", "Redirect to Pi Dash")).toBe(null);
    await driver.adminSidebarToggleCollapse();
    expect(await driver.adminFooterTooltip("redirect", "Redirect to Pi Dash")).toBe("Redirect to Pi Dash");
    expect(await driver.adminFooterTooltip("help", "Help")).toBe("Help");
    expect(await driver.adminFooterTooltip("toggle", "Toggle sidebar")).toBe("Toggle sidebar");
  }
);

test(
  specTitle(["ADM-068"], "hamburger shows on small screens and opens the off-canvas sidebar"),
  { tag: specTags(["ADM-068"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    await driver.setViewportSize(1440, 900);
    await openDashboard(driver, seed);
    expect((await driver.adminHamburgerFacts()).visible).toBe(false);
    await driver.setViewportSize(700, 900);
    const deadline = Date.now() + 10_000;
    for (;;) {
      if ((await driver.adminSidebarFacts()).collapsed) break;
      if (Date.now() >= deadline) throw new Error("[parity] narrow viewport never auto-collapsed the sidebar.");
      await sleep(250);
    }
    expect((await driver.adminHamburgerFacts()).visible).toBe(true);
    await waitForSidebarX(driver, "closed");
    // The hamburger opens the rail; the open rail overlays the header
    // and covers the hamburger, so the footer control closes it again.
    await driver.adminHamburgerToggle();
    await waitForSidebarX(driver, "open");
    expect((await driver.adminSidebarFacts()).collapsed).toBe(false);
    await driver.adminSidebarToggleCollapse();
    await waitForSidebarX(driver, "closed");
    expect((await driver.adminSidebarFacts()).collapsed).toBe(true);
  }
);

test(
  specTitle(["ADM-069"], "ai and email share one cached configurations fetch"),
  { tag: specTags(["ADM-069"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    await openDashboard(driver, seed);
    await driver.adminStartConfigFetchSpy();
    await driver.adminOpenPath("/ai/");
    await waitForSidebar(driver);
    await sleep(1000);
    expect(await driver.adminConfigFetchCount()).toBe(1);
    // Client-side hops reuse the mounted cache: no refetch.
    await driver.adminSidebarFollowEntry("Email");
    await driver.adminSidebarFollowEntry("Artificial intelligence");
    await sleep(2000);
    expect(await driver.adminConfigFetchCount()).toBe(1);
  }
);

test(
  specTitle(["ADM-069"], "no realtime transports over a dashboard window"),
  { tag: specTags(["ADM-069"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    await openDashboard(driver, seed);
    await driver.adminStartTransportSpy();
    await driver.adminStartConfigFetchSpy();
    await driver.adminSidebarFollowEntry("Artificial intelligence");
    await driver.adminSidebarFollowEntry("Email");
    const fetches = await driver.adminConfigFetchCount();
    await sleep(5000);
    expect(await driver.adminConfigFetchCount()).toBe(fetches);
    expect(await driver.adminTransportFacts()).toEqual({ websockets: 0, eventStreams: 0 });
  }
);

test(
  specTitle(["ADM-069"], "loop modal save explicitly revalidates the jobs list read"),
  { tag: specTags(["ADM-069"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    const session = await serverAdminSession(seed.email, seed.password);
    const slug = uniqueSlug("parity-446-reval");
    try {
      await openDashboard(driver, seed);
      await driver.adminOpenPath("/loop/");
      await waitForSidebar(driver);
      await expect
        .poll(
          async () => {
            const facts = await driver.adminLoopListFacts();
            return facts.tableVisible || facts.emptyVisible;
          },
          { timeout: 15_000 }
        )
        .toBe(true);
      await driver.adminStartLoopJobsSpy();
      await sleep(1000);
      const baseline = (await driver.adminLoopJobsUrls()).length;
      await driver.adminLoopOpenCreate();
      await driver.adminLoopModalFill({
        slug,
        name: `Probe job ${slug}`,
        publicName: `Probe public ${slug}`,
        publicDescription: `Probe description ${slug}`,
        prompt: `Probe prompt ${slug}`,
        minRole: 20,
        tzid: "UTC",
        rrule: "FREQ=DAILY;BYHOUR=3;BYMINUTE=0",
      });
      await driver.adminLoopModalSave();
      // The saved row appears through one explicit list refetch — and
      // nothing else refires the read over the window.
      await expect.poll(async () => (await driver.adminLoopModalFacts()).open, { timeout: 15_000 }).toBe(false);
      await expect
        .poll(async () => (await driver.adminLoopRows()).some((row) => row.slug === slug), { timeout: 15_000 })
        .toBe(true);
      expect((await driver.adminLoopJobsUrls()).length).toBe(baseline + 1);
      await sleep(2000);
      expect((await driver.adminLoopJobsUrls()).length).toBe(baseline + 1);
      expect((await serverAdminListLoopJobs(session)).some((job) => job.slug === slug)).toBe(true);
    } finally {
      await serverAdminDeleteLoopJobsBySlugPrefix("parity-446-reval");
    }
  }
);

test(
  specTitle(["ADM-070"], "no drag sources, file inputs or transfer controls on any dashboard page"),
  { tag: specTags(["ADM-070"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    await openDashboard(driver, seed);
    for (const [name] of sidebarOrder(driver)) {
      await driver.adminSidebarFollowEntry(name);
      expect(await driver.adminAbsenceFacts()).toEqual({
        draggable: 0,
        fileInputs: 0,
        transferControls: 0,
        desktopMentions: 0,
      });
    }
  }
);

test(
  specTitle(["ADM-071"], "representative keys navigate nowhere and open nothing"),
  { tag: specTags(["ADM-071"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    await openDashboard(driver, seed);
    for (const name of ["General", "Loop"]) {
      await driver.adminSidebarFollowEntry(name);
      const probe = await driver.adminShortcutProbe(["/", "?", "g", "t", "Escape"]);
      expect(probe.pathAfter).toBe(probe.pathBefore);
      expect(probe.dialogsOpened).toBe(0);
    }
  }
);

/** Repo root from this spec file (apps/web_new/e2e/parity/admin). */
function repoRoot(): string {
  return join(dirname(fileURLToPath(import.meta.url)), "..", "..", "..", "..", "..");
}

/** Source files under `dir`, skipping build output and vendored trees. */
function sourceFiles(dir: string): string[] {
  const out: string[] = [];
  for (const name of readdirSync(dir)) {
    if (name === "node_modules" || name === "dist" || name === "target" || name === ".dev-tree") continue;
    const full = join(dir, name);
    if (statSync(full).isDirectory()) out.push(...sourceFiles(full));
    else out.push(full);
  }
  return out;
}

test(
  specTitle(["ADM-072"], "console carries no desktop affordances and the desktop shell mounts no admin route"),
  { tag: specTags(["ADM-072"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    await openDashboard(driver, seed);
    for (const [name] of sidebarOrder(driver)) {
      await driver.adminSidebarFollowEntry(name);
      expect((await driver.adminAbsenceFacts()).desktopMentions).toBe(0);
    }
    // Packaging half: the desktop shell layers the web SPA only — no
    // admin tree ships and no admin mount is referenced anywhere.
    const root = repoRoot();
    expect(readdirSync(join(root, "desktop-overlay", "apps"))).not.toContain("admin");
    const mount = /god-mode|godmode|god_mode|apps\/admin/;
    const hits: string[] = [];
    for (const dir of [
      join(root, "desktop", "src-tauri", "src"),
      join(root, "desktop", "src-tauri", "capabilities"),
      join(root, "desktop", "scripts"),
      join(root, "desktop-overlay"),
    ]) {
      for (const file of sourceFiles(dir)) {
        if (mount.test(readFileSync(file, "utf8"))) hits.push(file);
      }
    }
    const conf = readFileSync(join(root, "desktop", "src-tauri", "tauri.conf.json"), "utf8");
    if (mount.test(conf)) hits.push("tauri.conf.json");
    expect(hits).toEqual([]);
  }
);

test(
  specTitle(["ADM-073"], "loop delete completes with no confirmation"),
  { tag: specTags(["ADM-073"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    const session = await serverAdminSession(seed.email, seed.password);
    const slug = uniqueSlug("parity-446-del");
    const job = await serverAdminCreateLoopJob(session, {
      slug,
      name: "Parity 446 delete watch",
      publicName: "Parity 446 delete watch",
      publicDescription: "Parity oracle fixture: delete-watch probe.",
      prompt: "Parity oracle fixture: delete-watch probe.",
      minRole: 15,
      rrule: "FREQ=DAILY;BYHOUR=3;BYMINUTE=0",
      tzid: "UTC",
    });
    try {
      await openDashboard(driver, seed);
      await driver.adminOpenPath(`/loop/${job.id}/`);
      await waitForSidebar(driver);
      await expect.poll(async () => (await driver.adminLoopDetailFacts()).editVisible, { timeout: 15_000 }).toBe(true);
      const watch = await driver.adminLoopDeleteWatchDialogs();
      expect(watch).toEqual({ nativeDialogs: 0, modalDialogs: 0 });
      expect(await serverAdminGetLoopJob(session, job.id)).toBe(null);
    } finally {
      await serverAdminDeleteLoopJobsBySlugPrefix("parity-446-del");
    }
  }
);

test(
  specTitle(["ADM-073"], "workspace page offers no deletion"),
  { tag: specTags(["ADM-073"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    await openDashboard(driver, seed);
    await driver.adminOpenPath("/workspace/");
    await waitForSidebar(driver);
    expect(await driver.adminWorkspaceDeleteFacts()).toEqual({ deleteControls: 0 });
    // The seeded workspace is untouched: the page deletes nothing.
    const session = await serverAdminSession(seed.email, seed.password);
    const list = await serverAdminListWorkspaces(session);
    expect(list.results.length).toBeGreaterThan(0);
  }
);
