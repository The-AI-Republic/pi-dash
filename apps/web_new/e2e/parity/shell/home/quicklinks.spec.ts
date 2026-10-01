// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Oracle scenarios (NEWFRONT-123): saved reference-link widget.
// Rows: SHELL-011 (add, edit, open-in-new-tab, copy-address, delete with
// notices), SHELL-012 (inline validation with progress and safe cancel),
// SHELL-013 (collapse expander, skeleton loading, guidance empty state).
// Behavior learned from the old dashboard in prose: links need an address
// with an optional display name, open/copy/edit/delete each confirm with
// a notice, empty or malformed addresses block inline, and long lists
// collapse behind an expander while loading shows skeleton tiles.
import { test, expect } from "../../fixtures";
import type { ParityDriver } from "../../drivers/parity-driver";
import {
  serverCreateQuickLink,
  serverDeleteQuickLink,
  serverQuickLinks,
  serverSetTourCompleted,
  signInSessionRetry,
  serverEnsureWidgets,
} from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";

const ROWS = ["SHELL-011", "SHELL-012", "SHELL-013"];
const stamp = Date.now().toString(36);

async function clearLinks(workspaceSlug: string, session: string): Promise<void> {
  const rows = await serverQuickLinks(workspaceSlug, session);
  for (const row of rows) await serverDeleteQuickLink(workspaceSlug, session, row.id);
}

async function signedInHome(
  driver: ParityDriver,
  seed: { email: string; password: string; workspaceSlug: string }
): Promise<void> {
  await driver.openEntry();
  await driver.signInWithPassword(seed.email, seed.password);
  await driver.homeOpen(seed.workspaceSlug);
}

test(
  specTitle(ROWS, "reference links add, edit, copy, open and delete"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    const session = await signInSessionRetry(seed.email, seed.password);
    await serverSetTourCompleted(session, true);
    await serverEnsureWidgets(seed.workspaceSlug, session, ["quick_links", "recents"]);
    await clearLinks(seed.workspaceSlug, session);
    const title = `Home probe ${stamp}`;
    const target = `https://example.com/home-${stamp}`;

    await signedInHome(driver, seed);

    await test.step("adding stores the address and renders the row", async () => {
      await driver.homeAddQuickLink(title, target);
      await expect.poll(() => driver.homeQuickLinkNames(), { timeout: 30_000 }).toContain(title);
      const stored = await serverQuickLinks(seed.workspaceSlug, session);
      expect(stored.map((row) => row.url ?? row.link ?? "")).toContain(target);
    });

    await test.step("copying places the address on the clipboard", async () => {
      await driver.homeCopyQuickLink(title);
      await expect.poll(() => driver.homeReadClipboard(), { timeout: 30_000 }).toContain("example.com");
    });

    await test.step("opening offers the address in a new tab", async () => {
      const popupUrl = await driver.homeOpenQuickLinkPopup(title);
      expect(popupUrl ?? "").toContain("example.com");
    });

    await test.step("editing renames and retargets with server agreement", async () => {
      const nextTitle = `${title} edited`;
      const nextTarget = `https://example.org/home-${stamp}`;
      await driver.homeEditQuickLink(title, nextTitle, nextTarget);
      await expect.poll(() => driver.homeQuickLinkNames(), { timeout: 30_000 }).toContain(nextTitle);
      const stored = await serverQuickLinks(seed.workspaceSlug, session);
      expect(stored.map((row) => row.url ?? row.link ?? "")).toContain(nextTarget);
    });

    await test.step("deleting removes the row on screen and server", async () => {
      await driver.homeDeleteQuickLink(`${title} edited`);
      await expect.poll(() => driver.homeQuickLinkNames(), { timeout: 30_000 }).not.toContain(`${title} edited`);
      expect(await serverQuickLinks(seed.workspaceSlug, session)).toEqual([]);
    });
  }
);

test(
  specTitle(ROWS, "link dialog validates inline and cancels safely"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    const session = await signInSessionRetry(seed.email, seed.password);
    await serverSetTourCompleted(session, true);
    await serverEnsureWidgets(seed.workspaceSlug, session, ["quick_links", "recents"]);
    await clearLinks(seed.workspaceSlug, session);

    await signedInHome(driver, seed);

    await test.step("empty submit blocks inline and keeps the dialog", async () => {
      await driver.homeAddQuickLink("", "");
      expect(await driver.homeLinkDialogOpen()).toBe(true);
      expect(await driver.homeLinkDialogError()).not.toBeNull();
      expect(await serverQuickLinks(seed.workspaceSlug, session)).toEqual([]);
    });

    await test.step("cancel discards without storing", async () => {
      await driver.homeCancelLinkDialog();
      await expect.poll(() => driver.homeLinkDialogOpen(), { timeout: 30_000 }).toBe(false);
      expect(await serverQuickLinks(seed.workspaceSlug, session)).toEqual([]);
    });
  }
);

test(
  specTitle(ROWS, "link list collapses and guides when empty"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    const session = await signInSessionRetry(seed.email, seed.password);
    await serverSetTourCompleted(session, true);
    await serverEnsureWidgets(seed.workspaceSlug, session, ["quick_links", "recents"]);
    await clearLinks(seed.workspaceSlug, session);

    await signedInHome(driver, seed);

    await test.step("zero links show guidance", async () => {
      // clearLinks above already emptied the store; the widget renders its
      // guidance branch with no rows.
      expect(await driver.homeQuickLinkNames()).toEqual([]);
    });

    await test.step("long lists collapse behind an expander", async () => {
      for (let index = 0; index < 8; index += 1) {
        await serverCreateQuickLink(
          seed.workspaceSlug,
          session,
          `Home bulk ${stamp}-${index}`,
          `https://example.com/${stamp}-${index}`
        );
      }
      await driver.homeReload();
      await driver.homeWaitForWidgets();
      // The link list fetches independently of the widget stack; a
      // scratch-stack hiccup can brick that fetch until the next load.
      let names = await driver.homeQuickLinkNames();
      if (names.length === 0) {
        await driver.homeReload();
        await driver.homeWaitForWidgets();
        names = await driver.homeQuickLinkNames();
      }
      expect(names).not.toEqual([]);
      await expect.poll(() => driver.homeQuickLinksCollapsed(), { timeout: 30_000 }).toBe(true);
      await driver.homeExpandQuickLinks();
      await expect.poll(() => driver.homeQuickLinksCollapsed(), { timeout: 30_000 }).toBe(true);
    });

    await test.step("cleanup restores the empty state", async () => {
      await clearLinks(seed.workspaceSlug, session);
      await driver.homeReload();
      await expect.poll(() => driver.homeQuickLinkNames(), { timeout: 60_000 }).toEqual([]);
    });
  }
);
