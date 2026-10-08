// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-125): the workspace switcher lists workspaces
// with role and member counts, marks the active one, and offers settings,
// invite, creation, invites inbox and sign-out shortcuts.
// Observed on the running old app: the top-bar switcher button opens a
// menu naming every workspace with its role and member count; each row
// offers workspace settings and member invites, and the menu footer
// offers workspace creation, the invites inbox and sign-out; picking
// another workspace navigates there and remembers it as the last
// workspace, so the next entry lands there; disabled creation hides its
// row; a failed sign-out toasts instead of signing out. Row: SHELL-058.
import { test, expect } from "../../fixtures";
import {
  deleteWorkspace,
  ensureWorkspace,
  fetchLastWorkspaceId,
  fetchWorkspaceTotalMembers,
  ownerSession,
  workspaceMemberRoles,
} from "../../helpers/api";
import { setWorkspaceCreationDisabled } from "../../helpers/stack";
import { specTags, specTitle } from "../../helpers/tags";

const ROWS = ["SHELL-058"];
const SECOND_NAME = "Parity Second";
const SECOND_SLUG = "parity-second";

test(
  specTitle(ROWS, "workspace switcher lists, marks and switches workspaces"),
  {
    tag: specTags(ROWS),
  },
  async ({ driver, seed }) => {
    const session = await test.step("prepare server session", async () => ownerSession(seed));
    const home = await test.step("resolve the seeded workspace", async () =>
      ensureWorkspace(session, seed.workspaceName, seed.workspaceSlug));
    const second = await test.step("provision a second workspace", async () => {
      return ensureWorkspace(session, SECOND_NAME, SECOND_SLUG);
    });

    await test.step("sign in through the UI", async () => {
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
    });

    await test.step("the switcher lists workspaces with roles, counts and shortcuts", async () => {
      const members = await workspaceMemberRoles(seed.workspaceSlug, session);
      const owner = members.find((member) => member.email === seed.email);
      expect(owner?.role).toBe(20);
      // The row counts from the workspace's own total, not the members
      // list length (which can carry extra rows).
      const total = await fetchWorkspaceTotalMembers(seed.workspaceSlug, session);
      const countLabel = `${total} member${total === 1 ? "" : "s"}`;
      await driver.openWorkspaceHome(seed.workspaceSlug);
      await expect.poll(() => driver.sidebarLinkTexts(), { timeout: 60_000 }).toContain("Home");
      await driver.openWorkspaceSwitcher();
      await expect.poll(() => driver.workspaceSwitcherTexts(), { timeout: 30_000 }).not.toEqual([]);
      const texts = (await driver.workspaceSwitcherTexts()).join(" ");
      expect(texts).toContain(seed.workspaceName);
      expect(texts).toContain(SECOND_NAME);
      expect(texts).toContain("admin");
      expect(texts).toContain(countLabel);
      for (const shortcut of ["Settings", "Invite members", "Create workspace", "Workspace invites", "Sign out"]) {
        expect(texts).toContain(shortcut);
      }
    });

    await test.step("switching navigates and remembers the workspace", async () => {
      await driver.switchWorkspace(SECOND_NAME);
      await expect
        .poll(() => Promise.resolve(new URL(driver.page.url()).pathname), { timeout: 30_000 })
        .toBe(`/${SECOND_SLUG}/`);
      await expect.poll(() => fetchLastWorkspaceId(session), { timeout: 30_000 }).toBe(second.id);
      // The remembered workspace is where the next entry lands. The last
      // cell is shared with sibling runs on the same seed user, so a round
      // that finds it flipped re-switches and re-enters, bounded.
      let landed = false;
      for (let round = 0; round < 3 && !landed; round += 1) {
        if (round > 0) {
          await driver.openWorkspaceHome(seed.workspaceSlug);
          await expect.poll(() => driver.sidebarLinkTexts(), { timeout: 60_000 }).toContain("Home");
          await driver.openWorkspaceSwitcher();
          await driver.switchWorkspace(SECOND_NAME);
          await expect.poll(() => fetchLastWorkspaceId(session), { timeout: 30_000 }).toBe(second.id);
        }
        await driver.openEntry();
        const deadline = Date.now() + 30_000;
        while (Date.now() < deadline && !landed) {
          if (new URL(driver.page.url()).pathname.includes(`/${SECOND_SLUG}/`)) landed = true;
          else await driver.page.waitForTimeout(1000);
        }
      }
      expect(landed).toBe(true);
      // Restore the seeded baseline: later steps and sibling runs assume
      // the owner lands on the seeded workspace.
      await driver.openWorkspaceHome(seed.workspaceSlug);
      await expect.poll(() => driver.sidebarLinkTexts(), { timeout: 60_000 }).toContain("Home");
      await driver.openWorkspaceSwitcher();
      await driver.switchWorkspace(seed.workspaceName);
      await expect
        .poll(() => Promise.resolve(new URL(driver.page.url()).pathname), { timeout: 30_000 })
        .toBe(`/${seed.workspaceSlug}/`);
      await expect.poll(() => fetchLastWorkspaceId(session), { timeout: 30_000 }).toBe(home.id);
    });

    await test.step("disabled creation hides its row", async () => {
      // The flag is instance-wide and cache-backed: flip it, assert, and
      // always restore it, keeping the disabled window as small as possible
      // so sibling runs creating workspaces are unaffected (the API gates
      // creation too). /api/instances/ carries a 12s browser max-age, so
      // wipe the HTTP cache at each flip edge: without it a fast reload
      // replays the pre-flip value.
      const clearHttpCache = async (): Promise<void> => {
        const cdp = await driver.page.context().newCDPSession(driver.page);
        await cdp.send("Network.clearBrowserCache").catch(() => {});
        await cdp.detach().catch(() => {});
      };
      try {
        setWorkspaceCreationDisabled(true);
        await clearHttpCache();
        await driver.openWorkspaceHome(seed.workspaceSlug);
        await expect.poll(() => driver.sidebarLinkTexts(), { timeout: 60_000 }).toContain("Home");
        await driver.openWorkspaceSwitcher();
        await expect.poll(() => driver.workspaceSwitcherTexts(), { timeout: 30_000 }).not.toEqual([]);
        const texts = (await driver.workspaceSwitcherTexts()).join(" ");
        expect(texts).toContain("Workspace invites");
        expect(texts).not.toContain("Create workspace");
      } finally {
        setWorkspaceCreationDisabled(false);
      }
      await clearHttpCache();
      await driver.openWorkspaceHome(seed.workspaceSlug);
      await driver.openWorkspaceSwitcher();
      await expect
        .poll(async () => (await driver.workspaceSwitcherTexts()).join(" "), { timeout: 30_000 })
        .toContain("Create workspace");
    });

    await test.step("a failed sign-out toasts and stays signed in", async () => {
      // Sign-out is a fire-and-forget form POST, so its failure path runs
      // through the CSRF fetch that precedes it: starve that fetch and the
      // sign-out throws, toasts, and leaves the session in place. Load the
      // page before installing the block so the load itself stays clean.
      await driver.openWorkspaceHome(seed.workspaceSlug);
      await expect.poll(() => driver.sidebarLinkTexts(), { timeout: 60_000 }).toContain("Home");
      const pattern = "**/auth/get-csrf-token/**";
      await driver.page.route(pattern, async (route) => {
        await route.abort();
      });
      try {
        await driver.openWorkspaceSwitcher();
        await driver.switchWorkspace("Sign out");
        await expect
          .poll(() => driver.isToastVisible("Failed to sign out. Please try again."), { timeout: 30_000 })
          .toBe(true);
        expect(new URL(driver.page.url()).pathname).toContain(`/${seed.workspaceSlug}/`);
        await expect.poll(() => driver.sidebarLinkTexts(), { timeout: 60_000 }).toContain("Home");
      } finally {
        await driver.page.unroute(pattern);
      }
    });

    await test.step("restore the seeded baseline", async () => {
      await deleteWorkspace(session, SECOND_SLUG);
    });
  }
);
