// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-319): pages realtime collaboration and the live
// server — conflict-free convergence, session identity, the reconnect state
// machine, socket handshake gating, structured error events with force-close,
// single-instance loss-free reconnect, and the live HTTP operations surface.
// Rows: PAGE-065–PAGE-071.
//
// Two-peer scenarios drive a second browser context through the same driver
// interface (created with the run's own target), so the area gate can replay
// them against apps/web_new unchanged. Live HTTP goes through the slot proxy
// origin (/live/*), the same origin the editor's same-origin fallback dials.
// Pages are created in the seeded project (additive only; shared seed rows
// are never mutated) with per-attempt unique names so oracle retries converge.
import { test, expect } from "../fixtures";
import { createDriver } from "../drivers/index";
import type { Browser } from "@playwright/test";
import {
  pagesLiveConvertDocumentLenient,
  pagesLiveGetLenient,
  pagesLiveHealth,
  pagesLiveOracleBase,
  pagesLivePatchDescriptionLenient,
  pagesLivePdfExportLenient,
  pagesLiveReadPage,
  pagesLiveSetPageLock,
  requireMentionMember,
  serverCreatePage,
  serverDeletePage,
  sessionBrowserCookies,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import type { ParityDriver, ParitySeedFacts } from "../drivers/parity-driver";

/** Per-attempt unique marker so oracle retries never collide on page names. */
function stamp(): string {
  return Date.now().toString(36);
}

function detailUrl(seed: ParitySeedFacts, pageId: string): string {
  return `/${seed.workspaceSlug}/projects/${seed.projectId}/pages/${pageId}`;
}

/** Balanced-JSON slice starting at `start` (an event payload inside frame bytes). */
function sliceEventJson(bytes: string, start: number): string | null {
  let depth = 0;
  let inString = false;
  let escaped = false;
  for (let i = start; i < bytes.length; i++) {
    const char = bytes[i] ?? "";
    if (inString) {
      if (escaped) escaped = false;
      else if (char === "\\") escaped = true;
      else if (char === '"') inString = false;
      continue;
    }
    if (char === '"') inString = true;
    else if (char === "{") depth += 1;
    else if (char === "}") {
      depth -= 1;
      if (depth === 0) return bytes.slice(start, i + 1);
    }
  }
  return null;
}

/** First received frame containing `marker`, polled until one arrives. */
async function waitForFrame(driver: ParityDriver, marker: string, timeoutMs = 120_000): Promise<string> {
  let found = "";
  await expect
    .poll(
      async () => {
        const frames = await driver.pagesLiveSocketReceivedFrames();
        found = frames.find((frame) => frame.includes(marker)) ?? "";
        return found;
      },
      { timeout: timeoutMs }
    )
    .not.toBe("");
  return found;
}

interface LiveErrorData {
  error_message?: unknown;
  error_type?: unknown;
  error_code?: unknown;
  should_disconnect?: unknown;
}

interface LiveErrorEvent {
  action?: unknown;
  affectedPages?: { currentPage?: unknown };
  data?: LiveErrorData;
}

/** Parse the error event JSON out of a frame's bytes (the stateless layer double-encodes it). */
function parseErrorEvent(frame: string): LiveErrorEvent {
  const unescaped = frame.replace(/\\"/g, '"');
  const start = unescaped.indexOf("{");
  if (start < 0) throw new Error("[parity] error frame carried no action payload.");
  const slice = sliceEventJson(unescaped, start);
  if (slice === null) throw new Error("[parity] error frame payload was not balanced JSON.");
  return JSON.parse(slice) as LiveErrorEvent;
}

async function openPeer(
  browser: Browser,
  target: ParityDriver["target"],
  url: string,
  sessionCookie: string
): Promise<{ close: () => Promise<void>; driver: ParityDriver }> {
  const context = await browser.newContext();
  const peer = createDriver(await context.newPage(), target);
  // Install socket captures before navigating so the first handshake is observed.
  await peer.pagesLiveSocketUrls();
  await peer.pagesLiveSocketAuthTokens();
  await peer.pagesLiveSocketCloseEvents();
  await peer.pagesLiveSocketReceivedFrames();
  await peer.openAuthenticated(url, sessionBrowserCookies(sessionCookie));
  return { close: () => context.close(), driver: peer };
}

async function seedSession(seed: ParitySeedFacts): Promise<string> {
  return signInSession(seed.email, seed.password);
}

async function memberSession(seed: ParitySeedFacts): Promise<string> {
  const member = requireMentionMember(seed);
  return signInSession(member.email, member.password);
}

test(
  specTitle(
    ["PAGE-065"],
    "concurrent keystrokes converge with no merge dialog and reload shows merged content and titles"
  ),
  { tag: specTags(["PAGE-065"]) },
  async ({ driver, seed, browser }) => {
    const tag = stamp();
    const session = await seedSession(seed);
    const peerSession = await memberSession(seed);
    const pageId = await serverCreatePage(seed.workspaceSlug, seed.projectId, session, `Live065 ${tag}`);
    const url = detailUrl(seed, pageId);
    await driver.openAuthenticated(url, sessionBrowserCookies(session));
    await driver.pagesLiveWaitSynced();
    const peer = await openPeer(browser, driver.target, url, peerSession);
    try {
      await peer.driver.pagesLiveWaitSynced();
      const markerA = `alpha-065-${tag}`;
      const markerB = `beta-065-${tag}`;
      await driver.pagesLiveTypeInBody(`${markerA} `);
      await peer.driver.pagesLiveTypeInBody(`${markerB} `);
      await expect.poll(() => driver.pagesLiveBodyText(), { timeout: 30_000 }).toContain(markerB);
      await expect.poll(() => peer.driver.pagesLiveBodyText(), { timeout: 30_000 }).toContain(markerA);
      const title = `Live065 Title ${tag}`;
      await driver.pagesLiveSetTitle(title);
      await expect.poll(() => peer.driver.pagesLiveTitleText(), { timeout: 30_000 }).toBe(title);
      // The server persists a debounced snapshot: poll the record until both
      // markers and the synced title land.
      await expect
        .poll(
          async () => {
            const record = await pagesLiveReadPage(seed.workspaceSlug, seed.projectId, pageId, session);
            return `${record.name} ${record.description_html ?? ""}`;
          },
          { timeout: 90_000 }
        )
        .toContain(markerA);
      const record = await pagesLiveReadPage(seed.workspaceSlug, seed.projectId, pageId, session);
      expect(record.description_html ?? "").toContain(markerB);
      expect(record.name).toBe(title);
      // Reload converges on the merged snapshot; no merge dialog ever appears.
      await driver.pagesLiveOpenPageDetail(seed.workspaceSlug, seed.projectId, pageId);
      await driver.pagesLiveWaitSynced();
      await expect.poll(() => driver.pagesLiveBodyText(), { timeout: 30_000 }).toContain(markerA);
      expect(await driver.pagesLiveBodyText()).toContain(markerB);
      expect(await driver.pagesLiveTitleText()).toBe(title);
      expect(await driver.page.getByRole("dialog").count()).toBe(0);
    } finally {
      await peer.close();
    }
  }
);

test(
  specTitle(["PAGE-066"], "sessions open with user identity and color while socket stages map onto the sync badge"),
  { tag: specTags(["PAGE-066"]) },
  async ({ driver, seed, browser }, testInfo) => {
    const tag = stamp();
    const member = requireMentionMember(seed);
    const session = await seedSession(seed);
    const peerSession = await memberSession(seed);
    const pageId = await serverCreatePage(seed.workspaceSlug, seed.projectId, session, `Live066 ${tag}`);
    const url = detailUrl(seed, pageId);
    // Install captures before navigating so the first handshake is observed.
    await driver.pagesLiveSocketUrls();
    await driver.pagesLiveSocketAuthTokens();
    await driver.openAuthenticated(url, sessionBrowserCookies(session));
    await driver.pagesLiveWaitSynced();
    const peer = await openPeer(browser, driver.target, url, peerSession);
    try {
      await peer.driver.pagesLiveWaitSynced();
      const tokens = await driver.pagesLiveSocketAuthTokens();
      expect(tokens.length).toBeGreaterThan(0);
      const identity = JSON.parse(tokens[0] ?? "{}") as { id?: unknown; name?: unknown; color?: unknown };
      expect(typeof identity.id).toBe("string");
      expect((identity.id as string).length).toBeGreaterThan(0);
      expect(typeof identity.name).toBe("string");
      expect((identity.name as string).length).toBeGreaterThan(0);
      expect(identity.color).toMatch(/^#[0-9a-fA-F]{6}$/);
      const peerTokens = await peer.driver.pagesLiveSocketAuthTokens();
      expect(peerTokens.length).toBeGreaterThan(0);
      const peerIdentity = JSON.parse(peerTokens[0] ?? "{}") as { id?: unknown; name?: unknown; color?: unknown };
      expect(peerIdentity.name).toBe(member.displayName);
      expect(peerIdentity.id).not.toBe(identity.id);
      // The cursor color derives deterministically from the user: a fresh
      // session for the same user re-sends the identical color.
      const firstColor = identity.color as string;
      await driver.pagesLiveOpenPageDetail(seed.workspaceSlug, seed.projectId, pageId);
      await driver.pagesLiveWaitSynced();
      const laterTokens = await driver.pagesLiveSocketAuthTokens();
      const laterColors = laterTokens.map((raw) => (JSON.parse(raw) as { color?: unknown }).color);
      expect(laterColors).toContain(firstColor);
      // Every handshake carries the document context in its query.
      const urls = await driver.pagesLiveSocketUrls();
      expect(urls.length).toBeGreaterThan(0);
      const query = new URL(urls[0] ?? "").searchParams;
      expect(query.get("documentType")).toBe("project_page");
      expect(query.get("projectId")).toBe(seed.projectId);
      expect(query.get("workspaceSlug")).toBe(seed.workspaceSlug);
      // Socket stages surface on the badge: dropping the connection shows a
      // transitional label, and restoring it re-syncs the badge away. A
      // plain load's connecting stage often syncs before first paint, so
      // the drop drives the transitional stage deterministically.
      await driver.pagesLiveSetOffline(true);
      try {
        if (testInfo.project.name === "new") {
          // The new app surfaces the drop as the offline pill, not the
          // badge (area spec §8.3: the badge hides while offline); the
          // drop still surfaces in the header, which is what this step
          // proves. The oracle leg below keeps pinning the old wording.
          await driver.page.getByTestId("page-offline-pill").waitFor({ state: "visible", timeout: 60_000 });
        } else {
          await expect.poll(() => driver.pagesLiveSyncBadge(), { timeout: 60_000 }).not.toBeNull();
        }
      } finally {
        await driver.pagesLiveSetOffline(false).catch(() => undefined);
      }
      // Multi-user editing converges over the socket; converging first also
      // lets the restored sync settle before the badge below is read, since
      // the badge can blip through hidden mid-resync.
      const marker = `gamma-066-${tag}`;
      await peer.driver.pagesLiveTypeInBody(`${marker} `);
      // The restored socket can ride out reconnect backoff before the first
      // receive lands; the budget covers a loaded full-file run.
      await expect.poll(() => driver.pagesLiveBodyText(), { timeout: 120_000 }).toContain(marker);
      await driver.pagesLiveWaitSynced();
      expect(await driver.pagesLiveSyncBadge()).toBeNull();
    } finally {
      await peer.close();
    }
  }
);

test(
  specTitle(["PAGE-067"], "a transient drop reconnects and re-syncs without user action"),
  { tag: specTags(["PAGE-067"]) },
  async ({ driver, seed, browser }, testInfo) => {
    const tag = stamp();
    const session = await seedSession(seed);
    const peerSession = await memberSession(seed);
    const pageId = await serverCreatePage(seed.workspaceSlug, seed.projectId, session, `Live067 ${tag}`);
    const url = detailUrl(seed, pageId);
    await driver.openAuthenticated(url, sessionBrowserCookies(session));
    await driver.pagesLiveWaitSynced();
    const peer = await openPeer(browser, driver.target, url, peerSession);
    try {
      await peer.driver.pagesLiveWaitSynced();
      await driver.pagesLiveSetOffline(true);
      // The reconnecting stage surfaces as a transitional badge — as the
      // offline pill on the new app (area spec §8.3 hides the badge while
      // offline). The oracle leg keeps pinning the old wording.
      if (testInfo.project.name === "new") {
        await driver.page.getByTestId("page-offline-pill").waitFor({ state: "visible", timeout: 60_000 });
      } else {
        await expect.poll(() => driver.pagesLiveSyncBadge(), { timeout: 60_000 }).not.toBeNull();
      }
      const marker = `offline-067-${tag}`;
      await driver.pagesLiveTypeInBody(`${marker} `);
      await driver.pagesLiveSetOffline(false);
      await driver.pagesLiveWaitSynced();
      await expect.poll(() => peer.driver.pagesLiveBodyText(), { timeout: 60_000 }).toContain(marker);
    } finally {
      await driver.pagesLiveSetOffline(false).catch(() => undefined);
      await peer.close();
    }
  }
);

test(
  specTitle(["PAGE-067"], "an unreachable socket shows the sync loader, then the binary fallback renders content"),
  { tag: specTags(["PAGE-067"]) },
  async ({ driver, seed }) => {
    const tag = stamp();
    const session = await seedSession(seed);
    const pageId = await serverCreatePage(seed.workspaceSlug, seed.projectId, session, `Live067b ${tag}`);
    const marker = `fallback-067-${tag}`;
    const seeded = await pagesLivePatchDescriptionLenient(seed.workspaceSlug, seed.projectId, pageId, session, {
      description_html: `<p>${marker}</p>`,
    });
    expect(seeded.status).toBe(200);
    // Fresh context, so the local cache is known-empty; the socket can never
    // connect while REST stays up.
    await driver.pagesLiveBlockCollaborationSocket();
    await driver.pagesLiveDescriptionFetchCount();
    try {
      await driver.openAuthenticated(detailUrl(seed, pageId), sessionBrowserCookies(session));
      await expect.poll(() => driver.pagesLiveSyncLoaderVisible(), { timeout: 60_000 }).toBe(true);
      // Retries exhaust into the terminal disconnected state: the fault badge.
      await expect.poll(() => driver.pagesLiveSyncBadge(), { timeout: 180_000 }).toBe("Connection lost");
      // Disconnected without cache, the client fetches the binary fallback
      // separately and renders it.
      await expect.poll(() => driver.pagesLiveDescriptionFetchCount(), { timeout: 60_000 }).toBeGreaterThan(0);
      await expect.poll(() => driver.pagesLiveBodyText(), { timeout: 60_000 }).toContain(marker);
    } finally {
      await driver.pagesLiveUnblockCollaborationSocket();
    }
  }
);

test(
  specTitle(["PAGE-068", "PAGE-069"], "a session whose token user mismatches the cookie is terminated"),
  { tag: specTags(["PAGE-068", "PAGE-069"]) },
  async ({ driver, seed, browser }) => {
    const tag = stamp();
    const session = await seedSession(seed);
    const peerSession = await memberSession(seed);
    const pageId = await serverCreatePage(seed.workspaceSlug, seed.projectId, session, `Live068 ${tag}`);
    const url = detailUrl(seed, pageId);
    // Open a valid session first (the token identity is fixed), then swap the
    // session cookie underneath it and force a reconnect: the new handshake
    // carries the owner's token with the member's cookie.
    await driver.pagesLiveSocketCloseEvents();
    await driver.openAuthenticated(url, sessionBrowserCookies(session));
    await driver.pagesLiveWaitSynced();
    await driver.page.context().clearCookies();
    await driver.page.context().addCookies(sessionBrowserCookies(peerSession));
    const baselineCloses = (await driver.pagesLiveSocketCloseEvents()).length;
    await driver.pagesLiveSetOffline(true);
    await driver.page.waitForTimeout(3_000);
    await driver.pagesLiveSetOffline(false);
    await expect.poll(() => driver.pagesLiveSyncBadge(), { timeout: 60_000 }).toBe("Connection lost");
    await expect
      .poll(async () => (await driver.pagesLiveSocketCloseEvents()).length, { timeout: 60_000 })
      .toBeGreaterThan(baselineCloses);
    // The terminated session never converges: a healthy peer stays clean.
    const peer = await openPeer(browser, driver.target, url, peerSession);
    try {
      await peer.driver.pagesLiveWaitSynced();
      const marker = `nomerge-068-${tag}`;
      await driver.pagesLiveTypeInBody(`${marker} `);
      await driver.page.waitForTimeout(5_000);
      expect(await peer.driver.pagesLiveBodyText()).not.toContain(marker);
    } finally {
      await peer.close();
    }
  }
);

test(
  specTitle(["PAGE-068"], "missing credentials open no socket and no secret-gated admin route answers"),
  { tag: specTags(["PAGE-068"]) },
  async ({ driver, seed }) => {
    const tag = stamp();
    const session = await seedSession(seed);
    const pageId = await serverCreatePage(seed.workspaceSlug, seed.projectId, session, `Live068b ${tag}`);
    await driver.pagesLiveSocketUrls();
    await driver.page.goto(detailUrl(seed, pageId), { waitUntil: "domcontentloaded" });
    // Signed-out visitors land on the entry card with the target preserved as
    // next_path; no collaboration socket ever opens.
    await expect.poll(() => driver.currentPath(), { timeout: 30_000 }).toMatch(/^\/(?:\?|$)/);
    await driver.page.waitForTimeout(5_000);
    expect(await driver.pagesLiveSocketUrls()).toHaveLength(0);
    // No privileged admin HTTP route is wired in this build: a representative
    // admin path answers the generic not-found envelope with and without the
    // shared secret header.
    const plain = await pagesLiveGetLenient("/live/admin/");
    expect(plain.status).toBe(404);
    const withSecret = await pagesLiveGetLenient("/live/admin/", {
      "live-server-secret-key": "parity19-scratch-live-secret",
    });
    expect(withSecret.status).toBe(plain.status);
    expect(withSecret.bodyText).toBe(plain.bodyText);
  }
);

test(
  specTitle(
    ["PAGE-069"],
    "bug: NEWFRONT-338 locked persist fails silently; the store error frame arrives but no toast shows"
  ),
  { tag: specTags(["PAGE-069"]) },
  async ({ driver, seed }, testInfo) => {
    const tag = stamp();
    const session = await seedSession(seed);
    const pageId = await serverCreatePage(seed.workspaceSlug, seed.projectId, session, `Live069 ${tag}`);
    await driver.pagesLiveSocketReceivedFrames();
    await driver.openAuthenticated(detailUrl(seed, pageId), sessionBrowserCookies(session));
    await driver.pagesLiveWaitSynced();
    await pagesLiveSetPageLock(seed.workspaceSlug, seed.projectId, pageId, true, session);
    try {
      await driver.pagesLiveTypeInBody(`locked-069-${tag} `);
      const event = parseErrorEvent(await waitForFrame(driver, "error_message"));
      expect(event.action).toBe("error");
      expect(event.affectedPages?.currentPage).toBe(pageId);
      const errorMessage = event.data?.error_message;
      expect(typeof errorMessage).toBe("string");
      expect(errorMessage?.length).toBeGreaterThan(0);
      expect(event.data?.error_type).toBe("store");
      // The bug: the failure stays invisible — no toast, badge still synced.
      if (testInfo.project.name === "new") {
        // The new app implements the fix (Parity wiki "Old bugs" allows
        // it): the persist failure surfaces as an error notice while the
        // badge stays synced. The notice auto-dismisses, so it is polled
        // for while fresh; the oracle leg keeps pinning the silence.
        let toast: { title: string; message: string } | null = null;
        await expect
          .poll(
            async () => {
              toast = await driver.rulesLastToast();
              return toast;
            },
            { timeout: 30_000 }
          )
          .not.toBeNull();
        expect(toast?.title).toBe("Could not save to the live document.");
      } else {
        await driver.page.waitForTimeout(10_000);
        expect(await driver.rulesLastToast()).toBeNull();
      }
      expect(await driver.pagesLiveSyncBadge()).toBeNull();
    } finally {
      await pagesLiveSetPageLock(seed.workspaceSlug, seed.projectId, pageId, false, session);
    }
  }
);

test(
  specTitle(
    ["PAGE-069"],
    "bug: NEWFRONT-338 oversized persist force-closes with the dedicated code; the error frames arrive but no toast shows"
  ),
  { tag: specTags(["PAGE-069"]) },
  async ({ driver, seed }, testInfo) => {
    const tag = stamp();
    const session = await seedSession(seed);
    const pageId = await serverCreatePage(seed.workspaceSlug, seed.projectId, session, `Live069b ${tag}`);
    // Seed just under the request-body ceiling: the seed PATCH fits, while the
    // socket's next all-forms persist (binary plus markup plus structured)
    // exceeds it and the backend refuses it as too large.
    const bulk = "Z".repeat(2_000_000);
    const seeded = await pagesLivePatchDescriptionLenient(seed.workspaceSlug, seed.projectId, pageId, session, {
      description_html: `<p>${bulk}</p>`,
    });
    expect(seeded.status).toBe(200);
    await driver.pagesLiveSocketCloseEvents();
    await driver.pagesLiveSocketReceivedFrames();
    await driver.openAuthenticated(detailUrl(seed, pageId), sessionBrowserCookies(session));
    await driver.pagesLiveWaitSynced();
    // Pre-place the socket block before the oversized keystroke: the current
    // socket stays up (only new upgrades are refused), so the force-close
    // sequence below is observed exactly once while the reconnect loop it
    // would otherwise start can never re-sync the megabytes — a loop that
    // crashes the renderer under a loaded full-file run.
    await driver.pagesLiveBlockCollaborationSocket();
    await driver.pagesLiveTypeInBody("x");
    // The error and force-close notices land before the connection closes.
    let errorSeenAt = 0;
    let noticeSeenAt = 0;
    let closeSeenAt = 0;
    let errorFrame = "";
    await expect
      .poll(
        async () => {
          const frames = await driver.pagesLiveSocketReceivedFrames();
          if (errorSeenAt === 0 && frames.some((frame) => frame.includes("content_too_large"))) {
            errorFrame = frames.find((frame) => frame.includes("content_too_large")) ?? "";
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
    const event = parseErrorEvent(errorFrame);
    expect(event.action).toBe("error");
    expect(event.data?.error_type).toBe("store");
    expect(event.data?.error_code).toBe("content_too_large");
    expect(event.data?.should_disconnect).toBe(true);
    expect(errorSeenAt).toBeLessThanOrEqual(closeSeenAt);
    expect(noticeSeenAt).toBeLessThanOrEqual(closeSeenAt);
    const closes = await driver.pagesLiveSocketCloseEvents();
    expect(closes.map((entry) => entry.code)).toContain(4001);
    expect(closes.find((entry) => entry.code === 4001)?.reason).toBe("document_too_large");
    // The force-closed client works through refused reconnect attempts before
    // settling on the terminal badge; under a loaded full-file run that
    // takes a while. (The socket was already blocked before the keystroke,
    // so no reconnect can re-sync the megabytes while this settles.)
    await expect.poll(() => driver.pagesLiveSyncBadge(), { timeout: 180_000 }).toBe("Connection lost");
    // The bug: neither notice surfaces — no toast for the refused save.
    if (testInfo.project.name === "new") {
      // The new app implements the fix (Parity wiki "Old bugs" allows
      // it): the refused save surfaces — the oversized strip owns the
      // incident (PAGE-063) while the failing fallback save additionally
      // toasts. The strip is sticky, so it pins the fix
      // deterministically; the oracle leg keeps pinning the silence.
      await driver.page.getByTestId("page-oversized-strip").waitFor({ state: "visible", timeout: 60_000 });
    } else {
      expect(await driver.rulesLastToast()).toBeNull();
    }
  }
);

test(
  specTitle(
    ["PAGE-069"],
    "bug: NEWFRONT-338 reconnecting to a removed page ends disconnected with no fetch frame or toast"
  ),
  { tag: specTags(["PAGE-069"]) },
  async ({ driver, seed }, testInfo) => {
    const tag = stamp();
    const session = await seedSession(seed);
    const pageId = await serverCreatePage(seed.workspaceSlug, seed.projectId, session, `Live069c ${tag}`);
    // Let the record load while the socket waits, then remove the page: the
    // server-side fetch fails once the socket connects. The blocked attempts
    // pause the client, so the fetch is triggered afterwards through an
    // offline toggle, which re-arms the connection.
    await driver.pagesLiveBlockCollaborationSocket();
    await driver.pagesLiveSocketReceivedFrames();
    await driver.pagesLiveSocketCloseEvents();
    await driver.pagesLiveSocketUrls();
    await driver.openAuthenticated(detailUrl(seed, pageId), sessionBrowserCookies(session));
    // Wait until the editor mounts against the held socket; only then remove
    // the page, so the record is cached client-side before it disappears.
    await expect.poll(() => driver.pagesLiveSyncLoaderVisible(), { timeout: 60_000 }).toBe(true);
    await serverDeletePage(seed.workspaceSlug, seed.projectId, pageId, session);
    await driver.pagesLiveUnblockCollaborationSocket();
    const baselineCloses = (await driver.pagesLiveSocketCloseEvents()).length;
    await driver.pagesLiveSetOffline(true);
    await driver.page.waitForTimeout(3_000);
    await driver.pagesLiveSetOffline(false);
    // The failed load drops the connection and the client exhausts into the
    // terminal fault badge.
    await expect
      .poll(async () => (await driver.pagesLiveSocketCloseEvents()).length, { timeout: 120_000 })
      .toBeGreaterThan(baselineCloses);
    // The terminal badge can take a while under load (matches the PAGE-067
    // disconnected-state budget): each refused reconnect backs off before
    // the client exhausts into the fault badge.
    await expect.poll(() => driver.pagesLiveSyncBadge(), { timeout: 180_000 }).toBe("Connection lost");
    // The bug, doubled: the fetch error frame has no attached recipient (the
    // failing loader is never attached at broadcast time), and nothing would
    // surface it anyway — no toast for the failed load.
    expect(await driver.pagesLiveSocketReceivedFrames()).toHaveLength(0);
    if (testInfo.project.name === "new") {
      // The new app implements the fix (Parity wiki "Old bugs" allows
      // it): with the page gone, the fallback save the terminal state
      // activates fails and surfaces instead of wedging silently. The
      // notice auto-dismisses, so it is polled for while fresh; the
      // oracle leg keeps pinning the silence.
      let toast: { title: string; message: string } | null = null;
      await expect
        .poll(
          async () => {
            toast = await driver.rulesLastToast();
            return toast;
          },
          { timeout: 60_000 }
        )
        .not.toBeNull();
      expect(toast?.title).toBe("Page content could not be saved.");
    } else {
      expect(await driver.rulesLastToast()).toBeNull();
    }
  }
);

test(
  specTitle(["PAGE-070"], "work typed while disconnected survives the reconnect with nothing lost or duplicated"),
  { tag: specTags(["PAGE-070"]) },
  async ({ driver, seed, browser }) => {
    const tag = stamp();
    const session = await seedSession(seed);
    const peerSession = await memberSession(seed);
    const pageId = await serverCreatePage(seed.workspaceSlug, seed.projectId, session, `Live070 ${tag}`);
    const url = detailUrl(seed, pageId);
    await driver.openAuthenticated(url, sessionBrowserCookies(session));
    await driver.pagesLiveWaitSynced();
    const peer = await openPeer(browser, driver.target, url, peerSession);
    try {
      await peer.driver.pagesLiveWaitSynced();
      const lines = [`queued-070-a-${tag}`, `queued-070-b-${tag}`, `queued-070-c-${tag}`];
      await driver.pagesLiveSetOffline(true);
      // One focused pass: re-clicking between lines would reset the cursor and
      // scramble positions, which is a test artifact, not sync behavior.
      await driver.pagesLiveTypeInBody(`${lines.join(" ")} `);
      await driver.page.waitForTimeout(10_000);
      await driver.pagesLiveSetOffline(false);
      await driver.pagesLiveWaitSynced();
      for (const line of lines) {
        await expect.poll(() => peer.driver.pagesLiveBodyText(), { timeout: 60_000 }).toContain(line);
      }
      // Both peers hold exactly the same merged text: nothing lost, nothing
      // duplicated across the reconnect.
      await expect
        .poll(() => peer.driver.pagesLiveBodyText(), { timeout: 30_000 })
        .toBe(await driver.pagesLiveBodyText());
    } finally {
      await driver.pagesLiveSetOffline(false).catch(() => undefined);
      await peer.close();
    }
  }
);

test(
  specTitle(["PAGE-071"], "the live operations endpoints probe, convert, and export with mapped failures"),
  { tag: specTags(["PAGE-071"]) },
  async ({ seed }) => {
    const tag = stamp();
    const session = await seedSession(seed);
    const oracleBase = pagesLiveOracleBase();
    const health = await pagesLiveHealth(oracleBase);
    expect(health.status).toBe("OK");
    expect(Number.isNaN(Date.parse(health.timestamp))).toBe(false);
    expect(health.version.length).toBeGreaterThan(0);

    const converted = await pagesLiveConvertDocumentLenient(
      { description_html: `<p>Live071 ${tag}</p>`, variant: "rich", title: `Live071 ${tag}` },
      oracleBase
    );
    expect(converted.status).toBe(200);
    const forms = converted.payload as {
      description_json?: unknown;
      description_binary?: unknown;
      description_html?: unknown;
    };
    expect(typeof forms.description_json).toBe("object");
    expect(typeof forms.description_binary).toBe("string");
    expect((forms.description_binary as string).length).toBeGreaterThan(0);
    expect(String(forms.description_html ?? "")).toContain(`Live071 ${tag}`);

    for (const invalid of [
      {},
      { description_html: "", variant: "rich" },
      { description_html: "   ", variant: "rich" },
      { description_html: "no tags here", variant: "rich" },
      { description_html: "<p>x</p>", variant: "bogus" },
      { description_html: "<p>x</p>", variant: "rich", description_binary: "!!!not-base64!!!" },
    ]) {
      const rejected = await pagesLiveConvertDocumentLenient(invalid, oracleBase);
      expect(rejected.status).toBe(400);
    }

    const pageId = await serverCreatePage(seed.workspaceSlug, seed.projectId, session, `Live071 ${tag}`);
    // Export renders from the stored binary: mint one through conversion and persist all three forms first.
    const minted = await pagesLiveConvertDocumentLenient(
      { description_html: `<p>Live071 export ${tag}</p>`, variant: "rich" },
      oracleBase
    );
    expect(minted.status).toBe(200);
    const mintedForms = minted.payload as {
      description_json?: unknown;
      description_binary?: unknown;
      description_html?: unknown;
    };
    const persisted = await pagesLivePatchDescriptionLenient(seed.workspaceSlug, seed.projectId, pageId, session, {
      description_binary: mintedForms.description_binary as string,
      description_html: mintedForms.description_html as string,
      description_json: mintedForms.description_json as object,
    });
    expect(persisted.status).toBe(200);
    const exported = await pagesLivePdfExportLenient(
      { pageId, workspaceSlug: seed.workspaceSlug, projectId: seed.projectId, title: `Live071 ${tag}`, noAssets: true },
      session,
      oracleBase
    );
    expect(exported.status).toBe(200);
    expect(exported.contentType).toContain("application/pdf");
    expect(exported.head.startsWith("%PDF")).toBe(true);
    expect(exported.byteLength).toBeGreaterThan(1000);

    const unauthenticated = await pagesLivePdfExportLenient(
      { pageId, workspaceSlug: seed.workspaceSlug, projectId: seed.projectId },
      "",
      oracleBase
    );
    expect(unauthenticated.status).toBe(401);
    const invalidBody = await pagesLivePdfExportLenient({ bogus: true }, session, oracleBase);
    expect(invalidBody.status).toBe(400);
    const missingPage = await pagesLivePdfExportLenient(
      {
        pageId: "00000000-0000-0000-0000-000000000000",
        workspaceSlug: seed.workspaceSlug,
        projectId: seed.projectId,
      },
      session,
      oracleBase
    );
    expect([404, 502]).toContain(missingPage.status);
  }
);
