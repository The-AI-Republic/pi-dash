// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-318): page-detail header actions (link copy,
// star, visibility, lock, shelved marker, options menu), the sync/offline/
// oversized indicators, socket-first persistence with its REST fallback, and
// the client-side export dialog. Rows: PAGE-055–PAGE-064 + PAGE-074.
//
// The options menu owns its shell here; entry behaviors owned by sibling
// rows (markup copy PAGE-044, display toggles PAGE-072, export PAGE-074,
// delete confirm PAGE-016) get shallow "it acted" assertions only — the
// deep behavior stays on the owning row. Socket-level primitives (badge,
// offline emulation, socket blocks, frame/close captures, lock/description
// REST helpers) are reused from the NEWFRONT-319 range; everything else is
// the pagesHeader* range this child owns.
// Pages are created in the seeded project (additive only; shared seed rows
// are never mutated) with per-attempt unique names so oracle retries converge.
import { test, expect } from "../fixtures";
import { createDriver } from "../drivers/index";
import type { Browser } from "@playwright/test";
import {
  pagesHeaderListPages,
  pagesHeaderReadPage,
  pagesHeaderSetArchive,
  pagesHeaderSetFavorite,
  pagesHeaderUserFavoriteEntities,
  pagesLivePatchDescriptionLenient,
  pagesLiveSetPageLock,
  requireGuestSeed,
  requireMentionMember,
  serverCreatePage,
  sessionBrowserCookies,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import type { PagesHeaderDownload, ParityDriver, ParitySeedFacts } from "../drivers/parity-driver";

/** Per-attempt unique marker so oracle retries never collide on page names. */
function stamp(): string {
  return Date.now().toString(36);
}

function detailPath(seed: ParitySeedFacts, projectId: string, pageId: string): string {
  return `/${seed.workspaceSlug}/projects/${projectId}/pages/${pageId}`;
}

async function seedSession(seed: ParitySeedFacts): Promise<string> {
  return signInSession(seed.email, seed.password);
}

async function memberSession(seed: ParitySeedFacts): Promise<string> {
  const member = requireMentionMember(seed);
  return signInSession(member.email, member.password);
}

async function guestSession(seed: ParitySeedFacts): Promise<string> {
  const guest = requireGuestSeed(seed);
  return signInSession(guest.email, guest.password);
}

/** Second browser context through the same driver interface (target-preserving). */
async function openPeer(
  browser: Browser,
  target: ParityDriver["target"],
  url: string,
  sessionCookie: string
): Promise<{ close: () => Promise<void>; driver: ParityDriver }> {
  const context = await browser.newContext();
  const peer = createDriver(await context.newPage(), target);
  await peer.openAuthenticated(url, sessionBrowserCookies(sessionCookie));
  return { close: () => context.close(), driver: peer };
}

/** Paste an image element into the focused live editor (REST-seeded images
 * race socket sync and serialize unreliably; pasted images are stable). */
async function pasteImage(driver: ParityDriver, imageSrc: string): Promise<void> {
  await driver.page.context().grantPermissions(["clipboard-read", "clipboard-write"]);
  await driver.page.evaluate((src) => {
    const blob = new Blob([`<img src="${src}">`], { type: "text/html" });
    return navigator.clipboard.write([new ClipboardItem({ "text/html": blob })]);
  }, imageSrc);
  await driver.page.keyboard.press("ControlOrMeta+V");
}

/** Poll the open menu for `label`, falling back to a reload when the live
 * update was stomped (bug NEWFRONT-369: on a slow stack a late-resolving
 * load-time record fetch overwrites the mutation's store update, leaving
 * the menu on pre-mutation entries until a reload renders server truth).
 * The live poll needs two consecutive sightings: out-of-order fetch
 * resolutions can flap the menu between pre- and post-mutation sets
 * (observed live). */
async function expectMenuItemOrReload(driver: ParityDriver, label: string): Promise<void> {
  let stable = false;
  try {
    await expect
      .poll(
        async () => {
          const present = (await driver.pagesHeaderMenuItems()).includes(label);
          const ok = present && stable;
          stable = present;
          return ok;
        },
        { timeout: 15_000 }
      )
      .toBe(true);
    return;
  } catch {
    // Live update lost to the stomp — recover via persisted server truth.
  }
  await driver.page.reload();
  await driver.pagesLiveWaitSynced();
  await driver.pagesHeaderOpenOptions();
  await expect.poll(() => driver.pagesHeaderMenuItems(), { timeout: 30_000 }).toContain(label);
}

/** Poll the open menu for the full shelved set (Restore + Delete, no Archive/visibility/toolbar), reloading once when the live menu flaps or sticks on pre-shelve entries (bug NEWFRONT-369 family). */
async function expectShelvedMenuOrReload(driver: ParityDriver): Promise<void> {
  const probe = async (): Promise<boolean> => {
    const items = await driver.pagesHeaderMenuItems();
    return (
      items.includes("Restore") &&
      items.includes("Delete") &&
      !items.includes("Archive") &&
      !items.includes("Make private") &&
      !items.includes("Make public") &&
      !items.includes("Sticky toolbar")
    );
  };
  try {
    await expect.poll(probe, { timeout: 15_000 }).toBe(true);
    return;
  } catch {
    // Flapping or stuck menu — recover via persisted server truth.
  }
  await driver.page.reload();
  await driver.pagesLiveWaitSynced();
  await driver.pagesHeaderOpenOptions();
  await expect.poll(probe, { timeout: 30_000 }).toBe(true);
}

/** Quiesce until no toast shows for a sustained window: a single null
 * sample can land in the gap between two stacked toasts and release the
 * next mutation early, whose notice is then swallowed (observed live). */
async function waitForToastQuiesce(driver: ParityDriver, timeoutMs = 30_000): Promise<void> {
  let clearSince = 0;
  await expect
    .poll(
      async () => {
        const toast = await driver.rulesLastToast();
        const now = Date.now();
        if (toast !== null) {
          clearSince = 0;
          return false;
        }
        if (clearSince === 0) clearSince = now;
        return now - clearSince >= 2000;
      },
      { timeout: timeoutMs }
    )
    .toBe(true);
}

/** Toggle the star, retrying once when nothing (notice, flip, or server
 * write) shows within 15s: under load the click can miss its handler
 * while the header re-renders (observed live). */
async function toggleFavoriteRobust(
  driver: ParityDriver,
  seed: ParitySeedFacts,
  projectId: string,
  pageId: string,
  session: string,
  expectAdded: boolean
): Promise<void> {
  const notice = expectAdded ? "Page added to favorites." : "Page removed from favorites.";
  const label = expectAdded ? "Remove favorite" : "Add to favorites";
  await driver.pagesHeaderToggleFavorite();
  try {
    await expect
      .poll(
        async () => {
          const toast = await driver.rulesLastToast();
          if (toast !== null && `${toast.title} ${toast.message}`.includes(notice)) return "toast";
          if ((await driver.pagesHeaderFavoriteLabel()) === label) return "flip";
          if ((await pagesHeaderReadPage(seed.workspaceSlug, projectId, pageId, session)).is_favorite === expectAdded)
            return "server";
          return "";
        },
        { timeout: 15_000 }
      )
      .not.toBe("");
  } catch {
    await driver.pagesHeaderToggleFavorite();
  }
}

/** Poll the editor's editable flag, falling back to a reload when the live
 * update was stomped (bug NEWFRONT-369 family: the archive mutation's
 * store update can be overwritten by a late-resolving load-time fetch,
 * leaving the editor stuck editable — or frozen — until a reload renders
 * server truth). */
async function expectEditableOrReload(driver: ParityDriver, editable: boolean): Promise<void> {
  try {
    await expect.poll(() => driver.pagesHeaderEditorEditable(), { timeout: 15_000 }).toBe(editable);
    return;
  } catch {
    // Live update lost to the stomp — recover via persisted server truth.
  }
  await driver.page.reload();
  await driver.pagesLiveWaitSynced();
  await expect.poll(() => driver.pagesHeaderEditorEditable(), { timeout: 30_000 }).toBe(editable);
}

/** D17 seats "Add sub-page" in the new app's options menu after "Version
 * history" for member and above (spec §12.2 PAGE-084); the oracle pins
 * the pre-nested order, so the new leg derives its expectation by
 * inserting the entry into the pinned list. */
function withSubPageEntry(pinned: readonly string[], leg: string): string[] {
  if (leg !== "new") return [...pinned];
  const at = pinned.indexOf("Version history");
  if (at < 0) return [...pinned];
  return [...pinned.slice(0, at + 1), "Add sub-page", ...pinned.slice(at + 1)];
}

/** Poll until a visible toast mentions `snippet` (toasts auto-dismiss; the box is loaded). */
async function waitForToast(driver: ParityDriver, snippet: string, timeoutMs = 60_000): Promise<void> {
  await expect
    .poll(
      async () => {
        const toast = await driver.rulesLastToast();
        if (toast === null) return "";
        return `${toast.title} ${toast.message}`;
      },
      { timeout: timeoutMs }
    )
    .toContain(snippet);
}

test(
  specTitle(["PAGE-055"], "link-copy writes the canonical address, confirms briefly, and pasting reopens the page"),
  { tag: specTags(["PAGE-055"]) },
  async ({ driver, seed }) => {
    const tag = stamp();
    const session = await seedSession(seed);
    const title = `Hdr055 ${tag}`;
    const pageId = await serverCreatePage(seed.workspaceSlug, seed.projectId, session, title);
    await driver.openAuthenticated(detailPath(seed, seed.projectId, pageId), sessionBrowserCookies(session));
    await driver.pagesLiveWaitSynced();

    expect(await driver.pagesHeaderCopyLinkLabel()).toBe("Copy link");
    await driver.pagesHeaderClickCopyLink();
    await expect.poll(() => driver.pagesHeaderCopyLinkLabel(), { timeout: 15_000 }).toBe("Copied link");
    const copiedAt = Date.now();
    const clipboard = await driver.pagesHeaderReadClipboard();
    expect(clipboard).toContain(`/${seed.workspaceSlug}/projects/${seed.projectId}/pages/${pageId}`);
    await waitForToast(driver, "Page link copied to clipboard.");
    // The confirmation state lasts about a second (live timing for the row).
    await expect.poll(() => driver.pagesHeaderCopyLinkLabel(), { timeout: 10_000 }).toBe("Copy link");
    const confirmMs = Date.now() - copiedAt;
    expect(confirmMs).toBeGreaterThan(300);
    expect(confirmMs).toBeLessThan(3000);

    // Pasting the address reopens the same page.
    await driver.page.goto(clipboard);
    await driver.pagesLiveWaitSynced();
    expect(await driver.pagesLiveTitleText()).toBe(title);
    expect(driver.page.url()).toContain(`/pages/${pageId}`);
  }
);

test(
  specTitle(["PAGE-056"], "detail star flips filled state, persists server-side, and surfaces in the favorites panel"),
  { tag: specTags(["PAGE-056"]) },
  async ({ driver, seed }) => {
    const tag = stamp();
    const session = await seedSession(seed);
    const title = `Hdr056 ${tag}`;
    const pageId = await serverCreatePage(seed.workspaceSlug, seed.projectId, session, title);
    await driver.openAuthenticated(detailPath(seed, seed.projectId, pageId), sessionBrowserCookies(session));
    await driver.pagesLiveWaitSynced();

    expect(await driver.pagesHeaderFavoriteLabel()).toBe("Add to favorites");
    expect(await driver.pagesHeaderFavoriteFilled()).toBe(false);
    expect(await driver.pagesHeaderFavoritesExpanded()).toBe(false);
    // Settle past the app's post-sync favorites revalidation: toggling in
    // the first ~second after sync-ready leaves the header star stale (the
    // revalidation stomps the optimistic flip; server write, toast, and
    // panel expansion still land). Observed live; see the row note.
    await driver.page.waitForTimeout(3_000);

    await toggleFavoriteRobust(driver, seed, seed.projectId, pageId, session, true);
    await waitForToast(driver, "Page added to favorites.");
    // The header re-render lags the toast under load: poll the flip.
    await expect.poll(() => driver.pagesHeaderFavoriteLabel(), { timeout: 60_000 }).toBe("Remove favorite");
    await expect.poll(() => driver.pagesHeaderFavoriteFilled(), { timeout: 15_000 }).toBe(true);
    // First use prompts the favorites panel, and the page surfaces in it.
    await expect.poll(() => driver.pagesHeaderFavoritesExpanded(), { timeout: 15_000 }).toBe(true);
    await expect.poll(() => driver.pagesHeaderFavoritesNames(), { timeout: 15_000 }).toContain(title);
    // Server state: the record flag flips and the generic user-favorites
    // row (the endpoint the star writes) carries the page.
    await expect
      .poll(async () => (await pagesHeaderReadPage(seed.workspaceSlug, seed.projectId, pageId, session)).is_favorite, {
        timeout: 15_000,
      })
      .toBe(true);
    expect(
      (await pagesHeaderUserFavoriteEntities(seed.workspaceSlug, "page", session)).map((row) => row.entity_identifier)
    ).toContain(pageId);
    // Quiesce: on a fast box the added notice is still showing when the
    // removal lands, and the removal notice would be swallowed by it.
    await waitForToastQuiesce(driver);

    await toggleFavoriteRobust(driver, seed, seed.projectId, pageId, session, false);
    await waitForToast(driver, "Page removed from favorites.");
    await expect.poll(() => driver.pagesHeaderFavoriteLabel(), { timeout: 60_000 }).toBe("Add to favorites");
    await expect.poll(() => driver.pagesHeaderFavoriteFilled(), { timeout: 15_000 }).toBe(false);
    await expect
      .poll(async () => (await pagesHeaderReadPage(seed.workspaceSlug, seed.projectId, pageId, session)).is_favorite, {
        timeout: 15_000,
      })
      .toBe(false);
    expect(
      (await pagesHeaderUserFavoriteEntities(seed.workspaceSlug, "page", session)).map((row) => row.entity_identifier)
    ).not.toContain(pageId);
  }
);

test(
  specTitle(["PAGE-056"], "star rolls back on write failure and stays hidden below member roles"),
  { tag: specTags(["PAGE-056"]) },
  async ({ driver, seed, browser }) => {
    const tag = stamp();
    const session = await seedSession(seed);
    const title = `Hdr056b ${tag}`;
    const pageId = await serverCreatePage(seed.workspaceSlug, seed.projectId, session, title);
    await driver.openAuthenticated(detailPath(seed, seed.projectId, pageId), sessionBrowserCookies(session));
    await driver.pagesLiveWaitSynced();

    // A member (non-owner) still sees the star.
    const peer = await openPeer(
      browser,
      driver.target,
      detailPath(seed, seed.projectId, pageId),
      await memberSession(seed)
    );
    try {
      await peer.driver.pagesLiveWaitSynced();
      await expect.poll(() => peer.driver.pagesHeaderFavoriteLabel(), { timeout: 15_000 }).toBe("Add to favorites");
    } finally {
      await peer.close();
    }

    // A failing write flips optimistically, then rolls back with an error notice.
    await driver.pagesHeaderFailFavoriteWrites();
    try {
      await driver.pagesHeaderToggleFavorite();
      await waitForToast(driver, "Page could not be added to favorites.");
      await expect.poll(() => driver.pagesHeaderFavoriteLabel(), { timeout: 15_000 }).toBe("Add to favorites");
      await expect.poll(() => driver.pagesHeaderFavoriteFilled(), { timeout: 15_000 }).toBe(false);
      expect((await pagesHeaderReadPage(seed.workspaceSlug, seed.projectId, pageId, session)).is_favorite).toBe(false);
      // Let the error notice dismiss: a toast fired while another shows is
      // swallowed (observed), which would eat the recovery toast below.
      await waitForToastQuiesce(driver);
    } finally {
      await driver.pagesHeaderUnfailFavoriteWrites();
    }
    // Writes flow again once the failure clears.
    await toggleFavoriteRobust(driver, seed, seed.projectId, pageId, session, true);
    await waitForToast(driver, "Page added to favorites.");
    await expect.poll(() => driver.pagesHeaderFavoriteLabel(), { timeout: 15_000 }).toBe("Remove favorite");
    await pagesHeaderSetFavorite(seed.workspaceSlug, seed.projectId, pageId, false, session);

    // Guests never reach the control: the detail refuses them outright.
    const guest = await openPeer(
      browser,
      driver.target,
      detailPath(seed, seed.projectId, pageId),
      await guestSession(seed)
    );
    try {
      await expect(guest.driver.page.getByText("Page not found")).toBeVisible({ timeout: 30_000 });
      expect(await guest.driver.pagesHeaderFavoriteLabel()).toBeNull();
    } finally {
      await guest.close();
    }
  }
);

test(
  specTitle(["PAGE-057"], "owners switch visibility both ways from the options menu"),
  { tag: specTags(["PAGE-057"]) },
  async ({ driver, seed }) => {
    const tag = stamp();
    const session = await seedSession(seed);
    const pageId = await serverCreatePage(seed.workspaceSlug, seed.projectId, session, `Hdr057 ${tag}`);
    await driver.openAuthenticated(detailPath(seed, seed.projectId, pageId), sessionBrowserCookies(session));
    await driver.pagesLiveWaitSynced();

    await driver.pagesHeaderOpenOptions();
    await expect.poll(() => driver.pagesHeaderMenuItems(), { timeout: 15_000 }).toContain("Make private");
    await driver.pagesHeaderChooseMenuItem("Make private");
    await waitForToast(driver, "marked private");
    await expect
      .poll(async () => (await pagesHeaderReadPage(seed.workspaceSlug, seed.projectId, pageId, session)).access, {
        timeout: 30_000,
      })
      .toBe(1);
    // Quiesce: a second notice fired while the first still shows is
    // swallowed, which would eat the make-public toast below.
    await waitForToastQuiesce(driver);

    await driver.pagesHeaderOpenOptions();
    await expectMenuItemOrReload(driver, "Make public");
    await driver.pagesHeaderChooseMenuItem("Make public");
    await waitForToast(driver, "marked public");
    await expect
      .poll(async () => (await pagesHeaderReadPage(seed.workspaceSlug, seed.projectId, pageId, session)).access, {
        timeout: 30_000,
      })
      .toBe(0);
  }
);

test(
  specTitle(["PAGE-057"], "visibility switching hides from non-privileged viewers and while shelved"),
  { tag: specTags(["PAGE-057"]) },
  async ({ driver, seed, browser }, testInfo) => {
    const tag = stamp();
    const session = await seedSession(seed);
    const pageId = await serverCreatePage(seed.workspaceSlug, seed.projectId, session, `Hdr057b ${tag}`);
    const path = detailPath(seed, seed.projectId, pageId);

    // A member (non-owner) gets the full menu minus every privileged entry.
    const peer = await openPeer(browser, driver.target, path, await memberSession(seed));
    try {
      await peer.driver.pagesLiveWaitSynced();
      await peer.driver.pagesHeaderOpenOptions();
      await expect
        .poll(() => peer.driver.pagesHeaderMenuItems(), { timeout: 15_000 })
        .toEqual(
          withSubPageEntry(
            ["Full width", "Sticky toolbar", "Copy markdown", "Version history", "Make a copy", "Export"],
            testInfo.project.name
          )
        );
    } finally {
      await peer.close();
    }

    // Shelving hides the switch even for the owner; restoring brings it back.
    await driver.openAuthenticated(path, sessionBrowserCookies(session));
    await driver.pagesLiveWaitSynced();
    await pagesHeaderSetArchive(seed.workspaceSlug, seed.projectId, pageId, true, session);
    try {
      await driver.page.reload();
      await driver.pagesLiveWaitSynced();
      await driver.pagesHeaderOpenOptions();
      await expect.poll(() => driver.pagesHeaderMenuItems(), { timeout: 15_000 }).toContain("Restore");
      const items = await driver.pagesHeaderMenuItems();
      expect(items).not.toContain("Make private");
      expect(items).not.toContain("Make public");
    } finally {
      await pagesHeaderSetArchive(seed.workspaceSlug, seed.projectId, pageId, false, session);
    }
    await driver.page.reload();
    await driver.pagesLiveWaitSynced();
    await driver.pagesHeaderOpenOptions();
    expect(await driver.pagesHeaderMenuItems()).toContain("Make private");
  }
);

test(
  specTitle(["PAGE-058"], "lock cycles resting, locked, and transitional phases and freezes editing for others"),
  { tag: specTags(["PAGE-058"]) },
  async ({ driver, seed, browser }) => {
    const tag = stamp();
    const session = await seedSession(seed);
    const pageId = await serverCreatePage(seed.workspaceSlug, seed.projectId, session, `Hdr058 ${tag}`);
    const path = detailPath(seed, seed.projectId, pageId);
    await driver.openAuthenticated(path, sessionBrowserCookies(session));
    await driver.pagesLiveWaitSynced();
    const peer = await openPeer(browser, driver.target, path, await memberSession(seed));
    try {
      await peer.driver.pagesLiveWaitSynced();
      await expect.poll(() => driver.pagesHeaderLockLabel(), { timeout: 15_000 }).toBe("Lock");
      // Settle the peer header on its copy control before asserting the
      // lock control is absent (a fresh render would also read null).
      await expect.poll(() => peer.driver.pagesHeaderCopyLinkLabel(), { timeout: 15_000 }).toBe("Copy link");
      expect(await peer.driver.pagesHeaderLockLabel()).toBeNull();
      await expect.poll(() => peer.driver.pagesHeaderEditorEditable(), { timeout: 30_000 }).toBe(true);

      await driver.pagesHeaderToggleLock();
      await waitForToast(driver, "Page locked successfully.");
      await expect.poll(() => driver.pagesHeaderLockLabel(), { timeout: 15_000 }).toBe("Locked");
      await expect
        .poll(async () => (await pagesHeaderReadPage(seed.workspaceSlug, seed.projectId, pageId, session)).is_locked, {
          timeout: 15_000,
        })
        .toBe(true);
      // Locking freezes editing for others — and, observed, for the
      // owner too (only the lock control itself stays usable).
      await expect.poll(() => peer.driver.pagesHeaderEditorEditable(), { timeout: 90_000 }).toBe(false);
      expect(await driver.pagesHeaderEditorEditable()).toBe(false);
      // The detail options menu carries no lock entry (observed): the row
      // menu owns the Lock/Unlock mirror instead.
      await driver.pagesHeaderOpenOptions();
      await expect.poll(() => driver.pagesHeaderMenuItems(), { timeout: 15_000 }).toContain("Export");
      const items = await driver.pagesHeaderMenuItems();
      expect(items.filter((item) => /^(un)?lock$/i.test(item))).toEqual([]);
      await driver.page.keyboard.press("Escape");
      // Verify the dismiss: a lingering overlay would swallow the unlock
      // click below (the click returns normally but never reaches the
      // button), leaving the test waiting on a toast that never comes.
      await expect.poll(() => driver.pagesHeaderMenuItems(), { timeout: 15_000 }).toEqual([]);
      // Quiesce: on a fast box the lock notice is still showing when the
      // unlock lands, and the unlock notice would be swallowed by it.
      await waitForToastQuiesce(driver);

      await driver.pagesHeaderToggleLock();
      // The store flips optimistically at click time, so the brief unlocked
      // phase starts immediately: sample it concurrently with the completion
      // wait instead of after, or the window is already gone. The breather
      // keeps the sampler from starving the page it samples (still ~24
      // reads of the 600ms window).
      const seen = new Set<string>();
      let stopSampling = false;
      const sampler = (async () => {
        while (!stopSampling) {
          const label = await driver.pagesHeaderLockLabel().catch(() => null);
          if (label !== null) seen.add(label);
          await driver.page.waitForTimeout(25);
        }
      })();
      await waitForToast(driver, "Page unlocked successfully.");
      await expect.poll(() => driver.pagesHeaderLockLabel(), { timeout: 60_000 }).toBe("Lock");
      stopSampling = true;
      await sampler;
      expect(seen.has("Unlocked")).toBe(true);
      await expect.poll(() => peer.driver.pagesHeaderEditorEditable(), { timeout: 90_000 }).toBe(true);
    } finally {
      await pagesLiveSetPageLock(seed.workspaceSlug, seed.projectId, pageId, false, session).catch(() => undefined);
      await peer.close();
    }
  }
);

test(
  specTitle(["PAGE-058"], "lock rolls back on write failure"),
  { tag: specTags(["PAGE-058"]) },
  async ({ driver, seed }) => {
    const tag = stamp();
    const session = await seedSession(seed);
    const pageId = await serverCreatePage(seed.workspaceSlug, seed.projectId, session, `Hdr058b ${tag}`);
    await driver.openAuthenticated(detailPath(seed, seed.projectId, pageId), sessionBrowserCookies(session));
    await driver.pagesLiveWaitSynced();

    await driver.pagesHeaderFailLockWrites();
    try {
      await driver.pagesHeaderToggleLock();
      await waitForToast(driver, "Page could not be locked.");
      await expect.poll(() => driver.pagesHeaderLockLabel(), { timeout: 15_000 }).toBe("Lock");
      expect((await pagesHeaderReadPage(seed.workspaceSlug, seed.projectId, pageId, session)).is_locked).toBe(false);
      // Let the error notice dismiss (a stacked toast would be swallowed).
      await waitForToastQuiesce(driver);
    } finally {
      await driver.pagesHeaderUnfailLockWrites();
    }
    await driver.pagesHeaderToggleLock();
    await waitForToast(driver, "Page locked successfully.");
    await expect.poll(() => driver.pagesHeaderLockLabel(), { timeout: 15_000 }).toBe("Locked");
    await pagesLiveSetPageLock(seed.workspaceSlug, seed.projectId, pageId, false, session);
  }
);

test(
  specTitle(["PAGE-059"], "shelving marks the header, freezes editing, and restores cleanly"),
  { tag: specTags(["PAGE-059"]) },
  async ({ driver, seed }) => {
    const tag = stamp();
    const session = await seedSession(seed);
    const pageId = await serverCreatePage(seed.workspaceSlug, seed.projectId, session, `Hdr059 ${tag}`);
    await driver.openAuthenticated(detailPath(seed, seed.projectId, pageId), sessionBrowserCookies(session));
    await driver.pagesLiveWaitSynced();

    expect(await driver.pagesHeaderArchivedMarker()).toBeNull();
    await expect.poll(() => driver.pagesHeaderEditorEditable(), { timeout: 30_000 }).toBe(true);

    await driver.pagesHeaderOpenOptions();
    await expect.poll(() => driver.pagesHeaderMenuItems(), { timeout: 15_000 }).toContain("Archive");
    await driver.pagesHeaderChooseMenuItem("Archive");
    await waitForToast(driver, "Page archived successfully.");
    await expect.poll(() => driver.pagesHeaderArchivedMarker(), { timeout: 15_000 }).not.toBeNull();
    expect(await driver.pagesHeaderArchivedMarker()).toMatch(/^Archived at [A-Z][a-z]{2} \d{1,2}, \d{4}$/);
    // The marker renders optimistically; the server write can lag it — poll.
    await expect
      .poll(async () => (await pagesHeaderReadPage(seed.workspaceSlug, seed.projectId, pageId, session)).archived_at, {
        timeout: 15_000,
      })
      .not.toBeNull();
    // Shelved content stays readable but frozen; visibility switching hides.
    await expectEditableOrReload(driver, false);
    await driver.pagesHeaderOpenOptions();
    await expectShelvedMenuOrReload(driver);
    await driver.page.keyboard.press("Escape");
    await expect.poll(() => driver.pagesHeaderMenuItems(), { timeout: 15_000 }).toEqual([]);
    // Let the archive notice dismiss so the restore notice is not swallowed.
    await waitForToastQuiesce(driver);

    await driver.pagesHeaderOpenOptions();
    await expectMenuItemOrReload(driver, "Restore");
    await driver.pagesHeaderChooseMenuItem("Restore");
    await waitForToast(driver, "Page restored successfully.");
    await expect.poll(() => driver.pagesHeaderArchivedMarker(), { timeout: 15_000 }).toBeNull();
    await expect
      .poll(async () => (await pagesHeaderReadPage(seed.workspaceSlug, seed.projectId, pageId, session)).archived_at, {
        timeout: 15_000,
      })
      .toBeNull();
    await expectEditableOrReload(driver, true);
  }
);

test(
  specTitle(["PAGE-059"], "shelved delete surfaces a naming dialog and cancel writes nothing"),
  { tag: specTags(["PAGE-059"]) },
  async ({ driver, seed }) => {
    const tag = stamp();
    const session = await seedSession(seed);
    const title = `Hdr059b ${tag}`;
    const pageId = await serverCreatePage(seed.workspaceSlug, seed.projectId, session, title);
    await driver.openAuthenticated(detailPath(seed, seed.projectId, pageId), sessionBrowserCookies(session));
    await driver.pagesLiveWaitSynced();

    // Live pages offer no delete entry at all.
    await driver.pagesHeaderOpenOptions();
    await expect.poll(() => driver.pagesHeaderMenuItems(), { timeout: 15_000 }).toContain("Archive");
    expect(await driver.pagesHeaderMenuItems()).not.toContain("Delete");
    await driver.page.keyboard.press("Escape");
    await expect.poll(() => driver.pagesHeaderMenuItems(), { timeout: 15_000 }).toEqual([]);

    await pagesHeaderSetArchive(seed.workspaceSlug, seed.projectId, pageId, true, session);
    try {
      await driver.page.reload();
      await driver.pagesLiveWaitSynced();
      await driver.pagesHeaderOpenOptions();
      await expect.poll(() => driver.pagesHeaderMenuItems(), { timeout: 15_000 }).toContain("Delete");
      await driver.pagesHeaderChooseMenuItem("Delete");
      await expect.poll(() => driver.pagesHeaderDeleteDialogVisible(), { timeout: 15_000 }).toBe(true);
      const text = await driver.pagesHeaderDeleteDialogText();
      expect(text).toContain("Delete page");
      expect(text).toContain(title);
      expect(text).toContain("permanently");
      expect(text).toContain("cannot be undone");
      await driver.pagesHeaderCancelDelete();
      await expect.poll(() => driver.pagesHeaderDeleteDialogVisible(), { timeout: 15_000 }).toBe(false);
      // Cancelling preserves the shelved page untouched.
      const record = await pagesHeaderReadPage(seed.workspaceSlug, seed.projectId, pageId, session);
      expect(record.archived_at).not.toBeNull();
      expect(await driver.pagesHeaderArchivedMarker()).not.toBeNull();
    } finally {
      await pagesHeaderSetArchive(seed.workspaceSlug, seed.projectId, pageId, false, session);
    }
  }
);

test(
  specTitle(["PAGE-060"], "options menu gathers every entry in order and each one acts"),
  { tag: specTags(["PAGE-060"]) },
  async ({ driver, seed }, testInfo) => {
    const tag = stamp();
    const session = await seedSession(seed);
    const title = `Hdr060 ${tag}`;
    const pageId = await serverCreatePage(seed.workspaceSlug, seed.projectId, session, title);
    await driver.openAuthenticated(detailPath(seed, seed.projectId, pageId), sessionBrowserCookies(session));
    await driver.pagesLiveWaitSynced();

    // The observed owner order (no new-tab, copy-link, lock, or move entry —
    // those live on the list row menu instead).
    await driver.pagesHeaderOpenOptions();
    await expect
      .poll(() => driver.pagesHeaderMenuItems(), { timeout: 15_000 })
      .toEqual(
        withSubPageEntry(
          [
            "Full width",
            "Sticky toolbar",
            "Copy markdown",
            "Version history",
            "Make a copy",
            "Archive",
            "Make private",
            "Export",
          ],
          testInfo.project.name
        )
      );
    await driver.page.keyboard.press("Escape");
    await expect.poll(() => driver.pagesHeaderMenuItems(), { timeout: 15_000 }).toEqual([]);

    await test.step("display toggles reflow the canvas", async () => {
      expect(await driver.pagesHeaderCanvasMaxWidth()).toBe("720px");
      await driver.pagesHeaderOpenOptions();
      await driver.pagesHeaderChooseMenuItem("Full width");
      expect(await driver.pagesHeaderCanvasMaxWidth()).toBe("1152px");
      await driver.pagesHeaderOpenOptions();
      await driver.pagesHeaderChooseMenuItem("Full width");
      expect(await driver.pagesHeaderCanvasMaxWidth()).toBe("720px");

      expect(await driver.pagesHeaderToolbarVisible()).toBe(true);
      await driver.pagesHeaderOpenOptions();
      await driver.pagesHeaderChooseMenuItem("Sticky toolbar");
      expect(await driver.pagesHeaderToolbarVisible()).toBe(false);
      await driver.pagesHeaderOpenOptions();
      await driver.pagesHeaderChooseMenuItem("Sticky toolbar");
      expect(await driver.pagesHeaderToolbarVisible()).toBe(true);
    });

    await test.step("markup copy toasts", async () => {
      await driver.pagesHeaderOpenOptions();
      await driver.pagesHeaderChooseMenuItem("Copy markdown");
      await waitForToast(driver, "Markdown copied to clipboard.");
    });

    await test.step("version history opens the pane on its info tab", async () => {
      await driver.pagesHeaderOpenOptions();
      await driver.pagesHeaderChooseMenuItem("Version history");
      await expect.poll(() => driver.pagesHeaderPaneTabParam(), { timeout: 15_000 }).toBe("info");
      // The pane shell animates open; poll past the slide transition.
      await expect.poll(() => driver.pagesHeaderPaneOpen(), { timeout: 15_000 }).toBe(true);
    });

    await test.step("duplication mints a standalone copy", async () => {
      // Quiesce: the markdown notice above may still show on a fast box.
      await waitForToastQuiesce(driver);
      const before = await pagesHeaderListPages(seed.workspaceSlug, seed.projectId, session);
      await driver.pagesHeaderOpenOptions();
      await driver.pagesHeaderChooseMenuItem("Make a copy");
      await waitForToast(driver, "Page duplicated successfully.");
      await expect
        .poll(async () => (await pagesHeaderListPages(seed.workspaceSlug, seed.projectId, session)).length, {
          timeout: 30_000,
        })
        .toBe(before.length + 1);
      const after = await pagesHeaderListPages(seed.workspaceSlug, seed.projectId, session);
      const copies = after.filter((row) => row.id !== pageId && row.name.includes(`Hdr060 ${tag}`));
      expect(copies.length).toBeGreaterThan(0);
    });

    await test.step("export opens its dialog", async () => {
      await driver.pagesHeaderOpenOptions();
      await driver.pagesHeaderChooseMenuItem("Export");
      await expect.poll(() => driver.pagesHeaderExportDialogVisible(), { timeout: 15_000 }).toBe(true);
      expect((await driver.pagesHeaderExportForm()).format).toBe("PDF");
      await driver.pagesHeaderExportCancel();
      await expect.poll(() => driver.pagesHeaderExportDialogVisible(), { timeout: 15_000 }).toBe(false);
    });
  }
);

test(
  specTitle(["PAGE-060"], "options menu degrades by role: member sees the reduced set, guests see no menu"),
  { tag: specTags(["PAGE-060"]) },
  async ({ driver, seed, browser }, testInfo) => {
    const tag = stamp();
    const session = await seedSession(seed);
    const pageId = await serverCreatePage(seed.workspaceSlug, seed.projectId, session, `Hdr060b ${tag}`);
    const path = detailPath(seed, seed.projectId, pageId);

    const peer = await openPeer(browser, driver.target, path, await memberSession(seed));
    try {
      await peer.driver.pagesLiveWaitSynced();
      await peer.driver.pagesHeaderOpenOptions();
      await expect
        .poll(() => peer.driver.pagesHeaderMenuItems(), { timeout: 15_000 })
        .toEqual(
          withSubPageEntry(
            ["Full width", "Sticky toolbar", "Copy markdown", "Version history", "Make a copy", "Export"],
            testInfo.project.name
          )
        );
    } finally {
      await peer.close();
    }

    const guest = await openPeer(browser, driver.target, path, await guestSession(seed));
    try {
      await expect(guest.driver.page.getByText("Page not found")).toBeVisible({ timeout: 30_000 });
      // No detail, so no header controls at all (copy, star, lock, or menu).
      expect(await guest.driver.pagesHeaderCopyLinkLabel()).toBeNull();
      expect(await guest.driver.pagesHeaderFavoriteLabel()).toBeNull();
      expect(await guest.driver.pagesHeaderLockLabel()).toBeNull();
    } finally {
      await guest.close();
    }
  }
);

test(
  specTitle(["PAGE-061"], "sync badge shows transitions and faults, then hides once synced"),
  { tag: specTags(["PAGE-061"]) },
  async ({ driver, seed }) => {
    test.slow();
    const tag = stamp();
    const session = await seedSession(seed);
    const pageId = await serverCreatePage(seed.workspaceSlug, seed.projectId, session, `Hdr061 ${tag}`);
    await driver.openAuthenticated(detailPath(seed, seed.projectId, pageId), sessionBrowserCookies(session));
    await driver.pagesLiveWaitSynced();
    // The badge pulses on background sync activity, so a single read can
    // catch a transient Syncing between quiet moments: poll for one.
    await expect.poll(() => driver.pagesLiveSyncBadge(), { timeout: 30_000 }).toBeNull();

    // A socket that can never connect drives syncing, then the fault badge.
    await driver.pagesLiveBlockCollaborationSocket();
    try {
      await driver.page.reload();
      const seen = new Set<string>();
      await expect
        .poll(
          async () => {
            const badge = await driver.pagesLiveSyncBadge();
            if (badge !== null) seen.add(badge);
            return badge;
          },
          { timeout: 180_000 }
        )
        .toBe("Connection lost");
      expect(seen.has("Syncing...")).toBe(true);
    } finally {
      await driver.pagesLiveUnblockCollaborationSocket();
    }
    // The client never retries a refused initial connection (no recovery
    // in 180s of sampling), so recovery here rides a fresh page switch —
    // which the row says resets the badge to syncing anyway.
    await driver.page.reload();
    await expect.poll(() => driver.pagesLiveSyncBadge(), { timeout: 180_000 }).toBeNull();
  }
);

test(
  specTitle(["PAGE-061"], "connection-lost badge explains that background saves continue"),
  { tag: specTags(["PAGE-061"]) },
  async ({ driver, seed }, testInfo) => {
    test.slow();
    const tag = stamp();
    const session = await seedSession(seed);
    const pageId = await serverCreatePage(seed.workspaceSlug, seed.projectId, session, `Hdr061b ${tag}`);
    await driver.openAuthenticated(detailPath(seed, seed.projectId, pageId), sessionBrowserCookies(session));
    await driver.pagesLiveWaitSynced();

    // Browser offline emulation would hide the badge behind the Offline
    // pill, so the fault badge is driven by a refused socket with the
    // browser online: the badge stays put for sampling.
    await driver.pagesLiveBlockCollaborationSocket();
    try {
      await driver.page.reload();
      await expect.poll(() => driver.pagesLiveSyncBadge(), { timeout: 180_000 }).toBe("Connection lost");
      expect(await driver.pagesHeaderOfflinePill()).toBeNull();
      const tooltip = await driver.pagesHeaderSyncBadgeTooltip();
      expect(tooltip).not.toBeNull();
      expect(tooltip ?? "").toContain("Connection lost");
      // The oracle pins the old stale ten-second wording (spec §15); the
      // new app states the real 30-second fallback cadence, which the
      // parity wiki allows as an intended fix, so branch only this leg.
      if (testInfo.project.name === "oracle") {
        expect(tooltip ?? "").toContain("synced and saved every 10 seconds");
      } else {
        expect(tooltip ?? "").toContain("every 30 seconds");
      }
    } finally {
      await driver.pagesLiveUnblockCollaborationSocket();
    }
  }
);

test(
  specTitle(["PAGE-062"], "offline pill shows while disconnected and typing syncs on reconnect"),
  { tag: specTags(["PAGE-062"]) },
  async ({ driver, seed }) => {
    test.slow();
    const tag = stamp();
    const session = await seedSession(seed);
    const pageId = await serverCreatePage(seed.workspaceSlug, seed.projectId, session, `Hdr062 ${tag}`);
    const path = detailPath(seed, seed.projectId, pageId);
    const cookies = sessionBrowserCookies(session);
    await driver.openAuthenticated(path, cookies);
    await driver.pagesLiveWaitSynced();
    expect(await driver.pagesHeaderOfflinePill()).toBeNull();

    // Attempts re-roll the offline cycle when a loaded full-file run blanks
    // the page mid-cycle (empty shell, editor never settles): the retry
    // reloads online and cycles again. The pill, the offline typing, and the
    // reconnect sync below still execute and assert on the passing attempt.
    let marker = "";
    for (let attempt = 0; attempt < 2; attempt += 1) {
      marker = `offline-062-${tag}-a${attempt}`;
      await driver.pagesLiveSetOffline(true);
      try {
        await expect.poll(() => driver.pagesHeaderOfflinePill(), { timeout: 60_000 }).toBe("Offline");
        // Disconnecting mid-edit never blocks typing. Type at once and keep
        // the offline window short: waiting here only gives a loaded
        // renderer time to blank the page. A stale-node click misfire gets
        // one immediate retry instead of a long settle wait.
        try {
          await driver.pagesLiveTypeInBody(`${marker} `);
        } catch {
          await driver.page.waitForTimeout(3000);
          await driver.pagesLiveTypeInBody(`${marker} `);
        }
        await expect.poll(() => driver.pagesLiveBodyText(), { timeout: 60_000 }).toContain(marker);
        break;
      } catch (error) {
        if (attempt > 0) throw error;
      } finally {
        await driver.pagesLiveSetOffline(false);
      }
      await driver.openAuthenticated(path, cookies);
      await driver.pagesLiveWaitSynced();
    }
    await driver.pagesLiveWaitSynced();
    expect(await driver.pagesHeaderOfflinePill()).toBeNull();
    // The offline typing reaches the server after the reconnect.
    await expect
      .poll(
        async () =>
          (await pagesHeaderReadPage(seed.workspaceSlug, seed.projectId, pageId, session)).description_html ?? "",
        { timeout: 120_000 }
      )
      .toContain(marker);
  }
);

test(
  specTitle(["PAGE-063"], "bug: NEWFRONT-338 oversized persist broadcasts error frames but the strip never shows"),
  { tag: specTags(["PAGE-063"]) },
  async ({ driver, seed }, testInfo) => {
    test.slow();
    const tag = stamp();
    const session = await seedSession(seed);
    const pageId = await serverCreatePage(seed.workspaceSlug, seed.projectId, session, `Hdr063 ${tag}`);
    // Seed just under the request-body ceiling: the seed PATCH fits, while
    // the socket's next all-forms persist exceeds it and the backend
    // refuses it as too large.
    const bulk = "Z".repeat(2_000_000);
    const seeded = await pagesLivePatchDescriptionLenient(seed.workspaceSlug, seed.projectId, pageId, session, {
      description_html: `<p>${bulk}</p>`,
    });
    expect(seeded.status).toBe(200);
    await driver.pagesLiveSocketCloseEvents();
    await driver.pagesLiveSocketReceivedFrames();
    await driver.openAuthenticated(detailPath(seed, seed.projectId, pageId), sessionBrowserCookies(session));
    await driver.pagesLiveWaitSynced();
    // No socket block yet: routing the live socket through the Playwright
    // proxy while megabytes stream across it blows the string limit, so the
    // oversized keystroke and the force-close below run on the direct
    // socket; the block lands right after, before the badge settles.
    await driver.pagesLiveTypeInBody("x");
    // The error and force-close notices land before the connection closes.
    let errorSeenAt = 0;
    let noticeSeenAt = 0;
    let closeSeenAt = 0;
    await expect
      .poll(
        async () => {
          const frames = await driver.pagesLiveSocketReceivedFrames();
          if (errorSeenAt === 0 && frames.some((frame) => frame.includes("content_too_large"))) {
            errorSeenAt = Date.now();
          }
          if (noticeSeenAt === 0 && frames.some((frame) => frame.includes("force_close"))) {
            noticeSeenAt = Date.now();
          }
          const closes = await driver.pagesLiveSocketCloseEvents();
          if (closeSeenAt === 0 && closes.some((entry) => entry.code === 4001)) closeSeenAt = Date.now();
          return errorSeenAt > 0 && noticeSeenAt > 0 && closeSeenAt > 0;
        },
        { timeout: 240_000 }
      )
      .toBe(true);
    const frames = await driver.pagesLiveSocketReceivedFrames();
    const errorFrame = frames.find((frame) => frame.includes("content_too_large")) ?? "";
    expect(errorFrame).toContain("should_disconnect");
    expect(errorSeenAt).toBeLessThanOrEqual(closeSeenAt);
    expect(noticeSeenAt).toBeLessThanOrEqual(closeSeenAt);
    const closes = await driver.pagesLiveSocketCloseEvents();
    expect(closes.map((entry) => entry.code)).toContain(4001);
    expect(closes.find((entry) => entry.code === 4001)?.reason).toBe("document_too_large");
    // The oracle pins the bug where the refused save stays invisible. The
    // new app implements the intended NEWFRONT-338 fix (spec §15 and the
    // parity wiki's bug rule), so its strip must render; the notice stays
    // absent on both legs. Bounded readers keep the oversized doc safe.
    if (testInfo.project.name === "oracle") {
      expect(await driver.pagesHeaderOversizedStrip()).toBeNull();
    } else {
      expect(await driver.pagesHeaderOversizedStrip()).toContain("Page too large to sync");
    }
    expect(await driver.pagesHeaderToastText()).toBeNull();
    // Now refuse reconnections: without the block the client would re-sync
    // the megabytes in a loop instead of settling on the terminal badge.
    // The reload comes first and is the circuit-breaker: it destroys the
    // megabyte socket and its buffers (the server snapshot is still the
    // small pre-oversized content, since the persist failed), so the block
    // below installs on a fresh quiet page — installing the socket route
    // while megabytes are buffered drains the buffers into the string
    // limit (observed live: installs hanging minutes, then crashing). A
    // second reload then drives a fresh socket through the installed
    // route, since the route only refuses new upgrades.
    await driver.page.reload();
    await driver.pagesLiveWaitSynced();
    await driver.pagesLiveBlockCollaborationSocket();
    await driver.page.reload();
    // The refused client works through reconnect attempts before settling
    // on the terminal badge. The trail (last minute of samples) rides
    // into any timeout so a stuck transitional state reads differently
    // from a slow settle.
    const badgeTrail: string[] = [];
    const badgeStart = Date.now();
    try {
      await expect
        .poll(
          async () => {
            const badge = await driver.pagesHeaderSyncBadgeText();
            badgeTrail.push(`t+${Math.round((Date.now() - badgeStart) / 1000)}s:${badge ?? "<hidden>"}`);
            if (badgeTrail.length > 90) badgeTrail.shift();
            return badge;
          },
          { timeout: 240_000 }
        )
        .toBe("Connection lost");
    } catch (error) {
      throw new Error(`badge trail: ${badgeTrail.join(" ")} :: ${(error as Error).message}`);
    }
    // The strip stays absent after the terminal settle too.
    expect(await driver.pagesHeaderOversizedStrip()).toBeNull();
    expect(await driver.pagesHeaderToastText()).toBeNull();
    await driver.pagesLiveUnblockCollaborationSocket();
  }
);

test(
  specTitle(["PAGE-064"], "connected typing rides the socket with no browser description write"),
  { tag: specTags(["PAGE-064"]) },
  async ({ driver, seed }) => {
    const tag = stamp();
    const session = await seedSession(seed);
    const pageId = await serverCreatePage(seed.workspaceSlug, seed.projectId, session, `Hdr064 ${tag}`);
    await driver.openAuthenticated(detailPath(seed, seed.projectId, pageId), sessionBrowserCookies(session));
    await driver.pagesLiveWaitSynced();

    await driver.pagesHeaderSpyDescriptionWrites();
    const marker = `socket-064-${tag}`;
    await driver.pagesLiveTypeInBody(`${marker} `);
    // The server converges through the socket persist path with the browser
    // issuing no REST description write at all.
    await expect
      .poll(
        async () =>
          (await pagesHeaderReadPage(seed.workspaceSlug, seed.projectId, pageId, session)).description_html ?? "",
        { timeout: 120_000 }
      )
      .toContain(marker);
    expect(await driver.pagesHeaderDescriptionWriteCount()).toBe(0);
  }
);

test(
  specTitle(["PAGE-064"], "disconnected fallback patches all forms and the save gesture forces one early"),
  { tag: specTags(["PAGE-064"]) },
  async ({ driver, seed }) => {
    test.slow();
    const tag = stamp();
    const session = await seedSession(seed);
    const pageId = await serverCreatePage(seed.workspaceSlug, seed.projectId, session, `Hdr064b ${tag}`);
    await driver.pagesHeaderSpyDescriptionWrites();
    // Socket down, REST up: the fallback (not the socket) must carry saves.
    await driver.pagesLiveBlockCollaborationSocket();
    try {
      await driver.openAuthenticated(detailPath(seed, seed.projectId, pageId), sessionBrowserCookies(session));
      await expect.poll(() => driver.pagesLiveSyncBadge(), { timeout: 180_000 }).toBe("Connection lost");
      // The disconnect itself triggers one fallback patch carrying all forms.
      await expect.poll(() => driver.pagesHeaderDescriptionWriteCount(), { timeout: 60_000 }).toBeGreaterThan(0);
      expect(await driver.pagesHeaderDescriptionWriteKeys()).toEqual(
        expect.arrayContaining(["description_binary", "description_html", "description_json"])
      );

      const marker = `fallback-064-${tag}`;
      await driver.pagesLiveTypeInBody(`${marker} `);
      // Let any typing-debounced autosave land before the gesture: pressing
      // immediately merges the two patches ~200ms apart and the gap below
      // would measure autosave-vs-gesture instead of gesture-vs-interval.
      await driver.page.waitForTimeout(8_000);
      const t0 = Date.now();
      const pressedAt = Date.now();
      await driver.pagesHeaderPressSave();
      const before = await driver.pagesHeaderDescriptionWriteCount();
      await expect.poll(() => driver.pagesHeaderDescriptionWriteCount(), { timeout: 30_000 }).toBeGreaterThan(before);
      // Live save-gesture debounce for the row (code: 500ms + latencies).
      const gestureMs = Date.now() - pressedAt;
      expect(gestureMs).toBeLessThan(15_000);
      const forcedCount = await driver.pagesHeaderDescriptionWriteCount();
      // The forced patch reaches the server with the typed content.
      await expect
        .poll(
          async () =>
            (await pagesHeaderReadPage(seed.workspaceSlug, seed.projectId, pageId, session)).description_html ?? "",
          { timeout: 60_000 }
        )
        .toContain(marker);
      // The gesture resets the interval: no second patch fires promptly after
      // the forced event settles. Settle first: a follow-up patch ~100-400ms
      // after the forced one appears intermittently (second save of the same
      // event), so the event ends at the last patch followed by 5s quiet.
      // The full half-minute cadence is not waited out, per the row note.
      let eventLen = forcedCount;
      let quietStart = Date.now();
      const absorbUntil = Date.now() + 20_000;
      while (Date.now() < absorbUntil) {
        await driver.page.waitForTimeout(1000);
        const len = (await driver.pagesHeaderDescriptionWriteTimes()).length;
        if (len > eventLen) {
          eventLen = len;
          quietStart = Date.now();
        } else if (Date.now() - quietStart >= 5000) {
          break;
        }
      }
      const settledCount = (await driver.pagesHeaderDescriptionWriteTimes()).length;
      await driver.page.waitForTimeout(8_000);
      const after = await driver.pagesHeaderDescriptionWriteTimes();
      const statuses = await driver.pagesHeaderDescriptionWriteStatuses();
      const timeline = after
        .map((time, index) => `${((time - t0) / 1000).toFixed(1)}:${statuses[index] ?? "?"}`)
        .join(",");
      expect(after.length, `patch timeline (s:status from gesture): ${timeline}`).toBe(settledCount);
    } finally {
      await driver.pagesLiveUnblockCollaborationSocket();
    }
  }
);

test(
  specTitle(["PAGE-064"], "bug: NEWFRONT-359 a refused fallback description save storms silently on the old client"),
  { tag: specTags(["PAGE-064"]) },
  async ({ driver, seed }, testInfo) => {
    test.slow();
    const tag = stamp();
    const session = await seedSession(seed);
    const pageId = await serverCreatePage(seed.workspaceSlug, seed.projectId, session, `Hdr359 ${tag}`);
    // Cache a body first: the fallback only runs with an editor handle, and
    // the reported wedge is a visible editor the user cannot click, not the
    // cold-document loader.
    await driver.openAuthenticated(detailPath(seed, seed.projectId, pageId), sessionBrowserCookies(session));
    await driver.pagesLiveWaitSynced();
    const cached = `cached-359-${tag}`;
    await driver.pagesLiveTypeInBody(`${cached} `);
    await expect.poll(() => driver.pagesLiveBodyText(), { timeout: 60_000 }).toContain(cached);
    // The socket persists after its debounce; wait for the server copy so
    // the fresh load below has a body to fall back onto rather than an
    // empty document whose loader could mask the save failure.
    await expect
      .poll(
        async () =>
          (await pagesHeaderReadPage(seed.workspaceSlug, seed.projectId, pageId, session)).description_html ?? "",
        { timeout: 120_000 }
      )
      .toContain(cached);
    await driver.pagesHeaderSpyDescriptionWrites();
    await driver.pagesHeaderFailDescriptionWrites();
    await driver.pagesLiveBlockCollaborationSocket();
    try {
      // Fresh load: the socket is refused, so the REST fallback owns body
      // writes — and every one of its PATCHes answers 500. Dirty local
      // edits are required: a clean reconcile saves byte-identical content
      // and stays quiet, while a refused dirty save re-triggers the
      // fallback on the old client (NEWFRONT-359).
      await driver.openAuthenticated(detailPath(seed, seed.projectId, pageId), sessionBrowserCookies(session));
      await expect.poll(() => driver.pagesLiveBodyText(), { timeout: 60_000 }).toContain(cached);
      const dirty = `dirty-359-${tag}`;
      await driver.pagesLiveTypeInBody(`${dirty} `);
      await expect.poll(() => driver.pagesLiveBodyText(), { timeout: 60_000 }).toContain(dirty);
      // The spy predates the reload, so slice the volley window below from
      // here: everything from this count on is a refused post-reload patch.
      const windowBaseline = await driver.pagesHeaderDescriptionWriteCount();
      // Both clients still reach the terminal disconnected state under the
      // refused socket: the 500s never wedge the badge (the Syncing wedge
      // from the issue does not reproduce — clean or dirty, the badge
      // settles within seconds; see the issue forensics). Polling here
      // guards the pin's premise instead of assuming it.
      const badgeTrail: string[] = [];
      let noticeSeen = false;
      const start = Date.now();
      const observe = async (): Promise<{ badge: string | null; patches: number }> => {
        const badge = await driver.pagesHeaderSyncBadgeText();
        const patches = await driver.pagesHeaderDescriptionWriteCount();
        badgeTrail.push(`t+${Math.round((Date.now() - start) / 1000)}s:${badge ?? "<hidden>"}/p${patches}`);
        if (badgeTrail.length > 140) badgeTrail.shift();
        const toast = await driver.pagesHeaderToastText();
        if (toast !== null && /could not be saved/i.test(toast)) noticeSeen = true;
        return { badge, patches };
      };
      await expect
        .poll(
          async () => {
            const { badge } = await observe();
            return badge;
          },
          { timeout: 60_000 }
        )
        .toBe("Connection lost");
      // Absorb the activation storm: the first refused dirty save refires
      // on the old client, so the gesture below measures a fresh burst,
      // not the activation one. The event ends at the last patch followed
      // by 5s quiet; a sustained storm only makes the delta below larger.
      let eventLen = await driver.pagesHeaderDescriptionWriteCount();
      let quietStart = Date.now();
      const absorbUntil = Date.now() + 25_000;
      while (Date.now() < absorbUntil) {
        await driver.page.waitForTimeout(1000);
        const { patches } = await observe();
        if (patches > eventLen) {
          eventLen = patches;
          quietStart = Date.now();
        } else if (Date.now() - quietStart >= 5000) {
          break;
        }
      }
      // The refusal must be the description PATCH, never a broken binary
      // read: the GET stays up in both apps.
      const statuses = await driver.pagesHeaderDescriptionWriteStatuses();
      expect(statuses, `description patch statuses: ${statuses.join(",")}`).toContain(500);
      const patchTimeline = async (): Promise<string> => {
        const times = await driver.pagesHeaderDescriptionWriteTimes();
        const seen = await driver.pagesHeaderDescriptionWriteStatuses();
        return times.map((time, index) => `${((time - start) / 1000).toFixed(1)}:${seen[index] ?? "?}"}`).join(",");
      };
      if (testInfo.project.name === "oracle") {
        // Old app pin (NEWFRONT-359): the refused dirty save is silent (no
        // failure notice) and refires as a volley — the activation save
        // spawns overlapping retries in the same beat, where the 30s
        // cadence allows a single patch — instead of settling into a calm
        // retry. The volley re-renders the fallback loader, so clicks
        // landing inside it meet an unstable editor.
        expect(noticeSeen, `badge trail: ${badgeTrail.join(" ")}`).toBe(false);
        const times = (await driver.pagesHeaderDescriptionWriteTimes()).slice(windowBaseline);
        const windowStatuses = statuses.slice(windowBaseline);
        const timeline = await patchTimeline();
        const context = `storm timeline (s:status from reload): ${timeline}; badge trail: ${badgeTrail.join(" ")}`;
        expect(times.length, context).toBeGreaterThanOrEqual(2);
        const gaps = times.slice(1).map((time, index) => time - times[index]);
        expect(Math.min(...gaps), context).toBeLessThan(1000);
        expect(
          windowStatuses.every((status) => status === 500),
          context
        ).toBe(true);
      } else {
        // New app: the refusal is visible, the client still reaches the
        // terminal disconnected state, and the page stays usable.
        expect(noticeSeen, `badge trail: ${badgeTrail.join(" ")}`).toBe(true);
        const typed = `typed-359-${tag}`;
        await driver.pagesLiveTypeInBody(`${typed} `);
        expect(await driver.pagesLiveBodyText()).toContain(typed);
        // The standard save gesture forces exactly one more fallback
        // attempt through the debounce, then the reset interval stays
        // quiet: no refire burst, retry on the next tick instead.
        const before = await driver.pagesHeaderDescriptionWriteCount();
        await driver.pagesHeaderPressSave();
        await expect.poll(() => driver.pagesHeaderDescriptionWriteCount(), { timeout: 30_000 }).toBe(before + 1);
        const forcedCount = await driver.pagesHeaderDescriptionWriteCount();
        await driver.page.waitForTimeout(8_000);
        await observe();
        const timeline = await patchTimeline();
        expect(
          await driver.pagesHeaderDescriptionWriteCount(),
          `storm timeline (s:status from reload): ${timeline}; badge trail: ${badgeTrail.join(" ")}`
        ).toBe(forcedCount);
      }
    } finally {
      await driver.pagesHeaderUnfailDescriptionWrites();
      await driver.pagesLiveUnblockCollaborationSocket();
    }
  }
);

test(
  specTitle(["PAGE-064"], "title saves ride the record endpoint outside the description path"),
  { tag: specTags(["PAGE-064"]) },
  async ({ driver, seed }) => {
    const tag = stamp();
    const session = await seedSession(seed);
    const pageId = await serverCreatePage(seed.workspaceSlug, seed.projectId, session, `Hdr064c ${tag}`);
    await driver.openAuthenticated(detailPath(seed, seed.projectId, pageId), sessionBrowserCookies(session));
    await driver.pagesLiveWaitSynced();

    await driver.pagesHeaderSpyDescriptionWrites();
    await driver.pagesHeaderSpyPageWrites();
    const title = `Hdr064c Renamed ${tag}`;
    await driver.pagesLiveSetTitle(title);
    await expect.poll(() => driver.pagesHeaderPageWriteCount(), { timeout: 30_000 }).toBeGreaterThan(0);
    expect(await driver.pagesHeaderDescriptionWriteCount()).toBe(0);
    await expect
      .poll(async () => (await pagesHeaderReadPage(seed.workspaceSlug, seed.projectId, pageId, session)).name, {
        timeout: 30_000,
      })
      .toBe(title);
  }
);

test(
  specTitle(["PAGE-074"], "markdown export downloads live editor content under the title file name"),
  { tag: specTags(["PAGE-074"]) },
  async ({ driver, seed }) => {
    test.slow();
    const tag = stamp();
    const session = await seedSession(seed);
    const title = `Export Me 074 ${tag}`;
    const pageId = await serverCreatePage(seed.workspaceSlug, seed.projectId, session, title);
    // The socket never connects (fresh load under the block), and a
    // request listener proves no export-shaped API call runs while the
    // file renders — so the download provably renders client-side from
    // live editor content. Attempts re-roll the load when the shared
    // editor reference loses its race (bug NEWFRONT-360: download empty
    // despite content). Concurrent fallback description patches are
    // allowed (they carry saves, not the export) — an earlier revision
    // required zero writes in the export window, which starves under
    // load when patches fire mid-dialog.
    await driver.pagesLiveBlockCollaborationSocket();
    const exportish: string[] = [];
    const onRequest = (request: { url(): string }): void => {
      const url = request.url();
      if (url.includes("/api/") && /export/i.test(url)) exportish.push(url);
    };
    driver.page.on("request", onRequest);
    try {
      let download: PagesHeaderDownload | null = null;
      let marker = "";
      const attempts: string[] = [];
      for (let attempt = 0; attempt < 5 && download === null; attempt += 1) {
        marker = `live-export-074-${tag}-a${attempt}`;
        await driver.openAuthenticated(detailPath(seed, seed.projectId, pageId), sessionBrowserCookies(session));
        await expect.poll(() => driver.pagesLiveSyncBadge(), { timeout: 180_000 }).toBe("Connection lost");
        await driver.pagesLiveTypeInBody(`${marker} `);
        expect(await driver.pagesLiveBodyText()).toContain(marker);

        await driver.pagesHeaderOpenOptions();
        await driver.pagesHeaderChooseMenuItem("Export");
        await expect.poll(() => driver.pagesHeaderExportDialogVisible(), { timeout: 15_000 }).toBe(true);
        expect(await driver.pagesHeaderExportForm()).toEqual({
          format: "PDF",
          scope: "Everything",
          pageFormat: "A4",
          pageFormatVisible: true,
        });
        await driver.pagesHeaderExportSetFormat("Markdown");
        expect((await driver.pagesHeaderExportForm()).pageFormatVisible).toBe(false);
        const candidate = await driver.pagesHeaderExportConfirm();
        const hasMarker = candidate.headText.includes(marker);
        attempts.push(`a${attempt}:marker=${hasMarker} bytes=${candidate.byteLength}`);
        if (hasMarker) download = candidate;
      }
      if (download === null) throw new Error(`074a attempts failed: ${attempts.join(" | ")}`);
      expect(download.headText).toContain(marker);
      // Let any confirm-triggered request land, then prove none of the
      // requests during the export window hit an export endpoint.
      await driver.page.waitForTimeout(2000);
      expect(exportish).toEqual([]);
      const slug = title
        .toLowerCase()
        .replace(/[^a-z0-9-_]/g, "-")
        .replace(/-+/g, "-");
      expect(download.fileName).toBe(`${slug}.md`);
      expect(download.byteLength).toBeGreaterThan(0);
      await waitForToast(driver, "Page exported successfully.");
      // Success closes the dialog once its exit transition finishes.
      await expect.poll(() => driver.pagesHeaderExportDialogVisible(), { timeout: 15_000 }).toBe(false);
    } finally {
      driver.page.off("request", onRequest);
      await driver.pagesLiveUnblockCollaborationSocket();
    }
  }
);

test(
  specTitle(["PAGE-074"], "printable export carries the page format in its file name"),
  { tag: specTags(["PAGE-074"]) },
  async ({ driver, seed }) => {
    const tag = stamp();
    const session = await seedSession(seed);
    const title = `Print Me 074 ${tag}`;
    const pageId = await serverCreatePage(seed.workspaceSlug, seed.projectId, session, title);
    await driver.openAuthenticated(detailPath(seed, seed.projectId, pageId), sessionBrowserCookies(session));
    await driver.pagesLiveWaitSynced();
    await driver.pagesLiveTypeInBody(`img074 ${tag} `);
    await pasteImage(driver, `https://example.com/shot074-${tag}.png`);
    await expect.poll(() => driver.pagesLiveBodyText(), { timeout: 60_000 }).toContain(`img074 ${tag}`);

    await driver.pagesHeaderOpenOptions();
    await driver.pagesHeaderChooseMenuItem("Export");
    await driver.pagesHeaderExportSetPageFormat("Letter");
    await driver.pagesHeaderExportSetScope("No images");
    const download = await driver.pagesHeaderExportConfirm();
    const slug = title
      .toLowerCase()
      .replace(/[^a-z0-9-_]/g, "-")
      .replace(/-+/g, "-");
    expect(download.fileName).toBe(`${slug}-letter.pdf`);
    expect(download.byteLength).toBeGreaterThan(1000);
    expect(download.headText.startsWith("%PDF")).toBe(true);
    await waitForToast(driver, "Page exported successfully.");
  }
);

test(
  specTitle(["PAGE-074"], "bug: NEWFRONT-356 markdown text-only scope keeps markdown-syntax images"),
  { tag: specTags(["PAGE-074"]) },
  async ({ driver, seed }, testInfo) => {
    const tag = stamp();
    const session = await seedSession(seed);
    const pageId = await serverCreatePage(seed.workspaceSlug, seed.projectId, session, `Hdr074b ${tag}`);
    // Attempts re-roll the load when the shared editor reference loses
    // its race (bug NEWFRONT-360: export empty despite visible content).
    let withImages: PagesHeaderDownload | null = null;
    let shot = "";
    let text = "";
    for (let attempt = 0; attempt < 3 && withImages === null; attempt += 1) {
      shot = `shot074-${tag}-a${attempt}`;
      text = `img074 ${tag} a${attempt}`;
      await driver.openAuthenticated(detailPath(seed, seed.projectId, pageId), sessionBrowserCookies(session));
      await driver.pagesLiveWaitSynced();
      await driver.pagesLiveTypeInBody(`${text} `);
      await pasteImage(driver, `https://example.com/${shot}.png`);
      await expect.poll(() => driver.pagesLiveBodyText(), { timeout: 60_000 }).toContain(text);

      await driver.pagesHeaderOpenOptions();
      await driver.pagesHeaderChooseMenuItem("Export");
      await driver.pagesHeaderExportSetFormat("Markdown");
      const candidate = await driver.pagesHeaderExportConfirm();
      if (candidate.headText.includes(shot)) withImages = candidate;
    }
    expect(withImages?.headText).toContain(shot);
    // The oracle pins the bug where text-only scope still emits markdown
    // image syntax. The new app implements the intended NEWFRONT-356 fix
    // (parity wiki bug rule), so its text-only scope drops that syntax
    // while keeping the prose.
    await driver.pagesHeaderOpenOptions();
    await driver.pagesHeaderChooseMenuItem("Export");
    await driver.pagesHeaderExportSetFormat("Markdown");
    await driver.pagesHeaderExportSetScope("No images");
    const textOnly = await driver.pagesHeaderExportConfirm();
    if (testInfo.project.name === "oracle") {
      expect(textOnly.headText).toContain(shot);
    } else {
      expect(textOnly.headText).not.toContain(shot);
    }
    expect(textOnly.headText).toContain(text);
  }
);

test(
  specTitle(["PAGE-074"], "cancelling exports nothing and the form resets after close"),
  { tag: specTags(["PAGE-074"]) },
  async ({ driver, seed }) => {
    const tag = stamp();
    const session = await seedSession(seed);
    const pageId = await serverCreatePage(seed.workspaceSlug, seed.projectId, session, `Hdr074c ${tag}`);
    await driver.openAuthenticated(detailPath(seed, seed.projectId, pageId), sessionBrowserCookies(session));
    await driver.pagesLiveWaitSynced();

    await driver.pagesHeaderOpenOptions();
    await driver.pagesHeaderChooseMenuItem("Export");
    await driver.pagesHeaderExportSetFormat("Markdown");
    await driver.pagesHeaderExportSetScope("No images");
    // Cancelling downloads nothing.
    const noDownload = driver.page.waitForEvent("download", { timeout: 3_000 }).then(
      () => false,
      () => true
    );
    await driver.pagesHeaderExportCancel();
    await expect.poll(() => driver.pagesHeaderExportDialogVisible(), { timeout: 15_000 }).toBe(false);
    expect(await noDownload).toBe(true);

    // The form resets shortly after close: reopening shows defaults.
    await driver.pagesHeaderOpenOptions();
    await driver.pagesHeaderChooseMenuItem("Export");
    expect(await driver.pagesHeaderExportForm()).toEqual({
      format: "PDF",
      scope: "Everything",
      pageFormat: "A4",
      pageFormatVisible: true,
    });
    await driver.pagesHeaderExportCancel();
  }
);
