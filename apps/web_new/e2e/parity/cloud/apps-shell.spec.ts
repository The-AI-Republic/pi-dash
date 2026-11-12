// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-439): Apps sidebar entry + page shell,
// rows CLOUD-021 (sidebar entry for every role, release-gated) and
// CLOUD-022 (workspace chrome, viewer's own apps), green on the
// cloud-edition old app first. The same file must go green on
// apps/web_new once the cloud Apps area lands there.
//
// Observed behavior notes (cloud oracle): with the catalog flag on,
// every workspace role sees a sidebar Apps entry with its own icon,
// highlighted on its route, pointing at /<slug>/apps/; the flag-off
// shape is the OSS sidebar (no entry, no placeholder) with the route
// unknown. The Apps page mounts the standard workspace shell — sidebar
// plus command-palette provider, so the sidebar create control opens
// the create dialog — with the catalog as its content, and states that
// the list belongs to the viewer: two people on the same URL see
// different apps.
//
// The flag-off scenario needs a second, OSS-edition stack beside the
// cloud one (see "Cloud oracle" in stack/README.md): bring it up with
// its own namespace, then export PARITY_OSS_NS=<ns> before running.
// The scenario derives its oracle URL, API URL, and seed file from
// that namespace and fails fast when it is unset.
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { test, expect } from "../fixtures";
import type { ParitySeedFacts } from "../drivers/parity-driver";
import {
  cloudBrowserCookies,
  cloudDevLogin,
  cloudProvisionWorkspaceMember,
  sessionBrowserCookies,
  signInSession,
  type BrowserCookie,
} from "../helpers/api";
import { parityApiUrl, parityOracleUrl } from "../helpers/parity-env";
import { specTags, specTitle } from "../helpers/tags";
import { installOpenhubStub, marketplacePage, stubApp } from "./openhub-stub";

const MEMBER_EMAIL = "apps-member@example.com";
const GUEST_EMAIL = "apps-guest@example.com";

async function openAs(
  driver: { openAuthenticated: (path: string, cookies: BrowserCookie[]) => Promise<void> },
  path: string,
  cookie: string
): Promise<void> {
  await driver.openAuthenticated(path, cloudBrowserCookies(cookie));
}

/** Resolve the OSS stack's oracle URL, API URL, and seed facts from PARITY_OSS_NS. */
function ossStack(): { oracle: string; apiBase: string; seed: ParitySeedFacts } {
  const ns = (process.env["PARITY_OSS_NS"] ?? "").trim();
  if (ns === "") {
    throw new Error(
      "[parity] PARITY_OSS_NS is not set; bring up an OSS stack with its own namespace " +
        "(PARITY_NS=<ns> parity-up.sh, no PARITY_ORACLE_EDITION) and export PARITY_OSS_NS=<ns>."
    );
  }
  // Explicit PARITY_*_URL settings win over the namespace derivation, so
  // drop them: the OSS stack resolves purely from its own namespace.
  const env = { ...process.env, PARITY_NS: ns };
  delete env["PARITY_API_URL"];
  delete env["PARITY_ORACLE_URL"];
  delete env["PARITY_ORACLE_ADMIN_URL"];
  const seedFile = join(dirname(fileURLToPath(import.meta.url)), "..", `.seed-${ns.toLowerCase()}.json`);
  return {
    oracle: parityOracleUrl(env),
    apiBase: parityApiUrl(env),
    seed: JSON.parse(readFileSync(seedFile, "utf8")) as ParitySeedFacts,
  };
}

test(
  specTitle(["CLOUD-021"], "sidebar shows the Apps entry with its icon for every workspace role"),
  { tag: specTags(["CLOUD-021"]) },
  async ({ driver, seed }) => {
    const owner = await cloudDevLogin(seed.email);
    const member = await cloudProvisionWorkspaceMember(seed.workspaceSlug, MEMBER_EMAIL, 15, owner.cookie);
    const guest = await cloudProvisionWorkspaceMember(seed.workspaceSlug, GUEST_EMAIL, 5, owner.cookie);
    for (const session of [owner.cookie, member.cookie, guest.cookie]) {
      await driver.page.context().clearCookies();
      await openAs(driver, `/${seed.workspaceSlug}/apps`, session);
      await expect.poll(() => driver.cloudAppsSidebarHref(), { timeout: 60_000 }).toBe(`/${seed.workspaceSlug}/apps/`);
      expect(await driver.cloudAppsSidebarIconPresent()).toBe(true);
      expect(await driver.sidebarEntryVisible("Apps")).toBe(true);
    }
  }
);

test(
  specTitle(["CLOUD-021"], "Apps entry highlights on its route and reaches the page"),
  { tag: specTags(["CLOUD-021"]) },
  async ({ driver, seed }) => {
    const session = await cloudDevLogin(seed.email);
    await openAs(driver, `/${seed.workspaceSlug}/projects/${seed.projectId}/issues`, session.cookie);
    await driver.dismissWelcomeDialog();
    await expect.poll(() => driver.cloudAppsSidebarHref(), { timeout: 60_000 }).toBe(`/${seed.workspaceSlug}/apps/`);
    const away = await driver.sidebarRowTone("Apps");
    await driver.cloudAppsOpenFromSidebar();
    expect(await driver.currentPath()).toBe(`/${seed.workspaceSlug}/apps/`);
    const active = await driver.sidebarRowTone("Apps");
    expect(active).not.toEqual(away);
  }
);

test(
  specTitle(["CLOUD-021"], "flag-off builds carry no Apps entry and no apps route"),
  { tag: specTags(["CLOUD-021"]) },
  async ({ driver }) => {
    const oss = ossStack();
    const cookie = await signInSession(oss.seed.email, oss.seed.password, oss.apiBase);
    await driver.page.context().clearCookies();
    await driver.page.context().addCookies(sessionBrowserCookies(cookie));
    await driver.page.goto(`${oss.oracle}/${oss.seed.workspaceSlug}/projects/${oss.seed.projectId}/issues`, {
      waitUntil: "domcontentloaded",
    });
    await expect.poll(() => driver.sidebarPresent(), { timeout: 60_000 }).toBe(true);
    // No entry, no placeholder, no disabled row: nothing apps-shaped at all.
    expect(await driver.sidebarEntryVisible("Apps")).toBe(false);
    expect(await driver.cloudAppsSidebarHref()).toBeNull();
    const sidebar = await driver.page.locator("#main-sidebar").innerText();
    expect(sidebar.includes("Apps")).toBe(false);
    expect(await driver.page.locator('#main-sidebar a[href*="apps"]').count()).toBe(0);

    // The route itself is unknown there.
    await driver.page.goto(`${oss.oracle}/${oss.seed.workspaceSlug}/apps`, { waitUntil: "domcontentloaded" });
    await expect
      .poll(
        async () =>
          (
            (await driver.page
              .locator("body")
              .innerText()
              .catch(() => "")) ?? ""
          ).toLowerCase(),
        { timeout: 60_000 }
      )
      .toMatch(/cannot be found|not found|something went wrong/);
  }
);

test(
  specTitle(["CLOUD-022"], "apps page mounts workspace chrome with the catalog as content"),
  { tag: specTags(["CLOUD-022"]) },
  async ({ driver, seed }) => {
    const session = await cloudDevLogin(seed.email);
    const stub = await installOpenhubStub(driver.page.context(), {
      marketplace: () =>
        Promise.resolve({
          status: 200,
          body: marketplacePage([stubApp({ appId: "app-notes", slug: "notes", name: "Notes Pro" })]),
        }),
    });
    try {
      await openAs(driver, `/${seed.workspaceSlug}/apps`, session.cookie);
      await driver.dismissWelcomeDialog();
      await expect.poll(() => driver.cloudAppsCatalogVisible(), { timeout: 60_000 }).toBe(true);
      // Workspace chrome around the catalog: sidebar, ownership statement,
      // toggle, search — and the status read that gates it all.
      expect(await driver.sidebarPresent()).toBe(true);
      expect(await driver.sidebarEntryVisible("Apps")).toBe(true);
      const intro = await driver.cloudAppsIntroText();
      expect(intro.includes("yours")).toBe(true);
      expect(intro.includes("teammates")).toBe(true);
      expect(await driver.cloudAppsToggleState()).not.toBeNull();
      expect(await driver.cloudAppsSearchVisible()).toBe(true);
      expect(stub.log.some((entry) => entry.path === "/api/openhub/status/")).toBe(true);
      // The command-palette provider: the sidebar create control works here.
      expect(await driver.cloudAppsSidebarCreateOpensDialog()).toBe(true);
    } finally {
      await stub.uninstall();
    }
  }
);

test(
  specTitle(["CLOUD-022"], "two viewers see their own lists on the same URL"),
  { tag: specTags(["CLOUD-022"]) },
  async ({ driver, seed }) => {
    const owner = await cloudDevLogin(seed.email);
    const member = await cloudProvisionWorkspaceMember(seed.workspaceSlug, MEMBER_EMAIL, 15, owner.cookie);
    const ownerToken = /pidash_access=([^;]+)/.exec(owner.cookie)?.[1] ?? "";
    const memberToken = /pidash_access=([^;]+)/.exec(member.cookie)?.[1] ?? "";
    expect(ownerToken === "").toBe(false);
    expect(memberToken === "").toBe(false);
    const stub = await installOpenhubStub(driver.page.context(), {
      marketplace: (_route, logged) => {
        const mine = logged.cookie.includes(ownerToken)
          ? [stubApp({ appId: "app-owner-only", slug: "owner-only", name: "Owner Vault" })]
          : [stubApp({ appId: "app-member-only", slug: "member-only", name: "Member Vault" })];
        return Promise.resolve({ status: 200, body: marketplacePage(mine) });
      },
    });
    try {
      await openAs(driver, `/${seed.workspaceSlug}/apps`, owner.cookie);
      await expect.poll(() => driver.cloudAppsCardNames(), { timeout: 60_000 }).toEqual(["Owner Vault"]);
      await driver.page.context().clearCookies();
      await openAs(driver, `/${seed.workspaceSlug}/apps`, member.cookie);
      await expect.poll(() => driver.cloudAppsCardNames(), { timeout: 60_000 }).toEqual(["Member Vault"]);
    } finally {
      await stub.uninstall();
    }
  }
);
