// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-210): desktop share links, transport, CLI,
// native shell — web-observable sides. Rows: DESK-019–DESK-025.
//
// The oracle runs on apps/web, where the native halves are absent by
// design: copied links resolve against the page origin instead of a
// configured hosted origin, API traffic flows over the browser fetch
// adapter (the native transport needs both window flags the shell sets),
// the CLI signs in through the human-typed device page rather than an
// in-app self-approving flow, closing ends the page with no tray to park
// in, zoom is the browser's alone, server-host pages load in place, and
// the entry routes render their marketing-framed content.
//
// Halves with no UI path on web are recorded here, not weakened: the
// copied work-item path carries no query or fragment to keep (the
// preservation the row names is the shared new-URL construction the web
// list/peek call sites use, and the desktop refusal without a configured
// origin is pinned by the resolver's own unit test); live-stream resume
// on web is webview-EventSource semantics by construction (no native
// stream command exists without the bridge); the tray/window size
// defaults are native-only; and both route-extension tables are empty in
// this build, so no other web-only route exists to pin beyond the entry.
//
// Overlap: ISS-161 owns the detail copy toast, ISS-187–190 own the
// attachment widget flows, DESK-011 (NEWFRONT-208) owns entry-route
// equivalence including /login, and AUTH-020 owns the signed-out bounce.
// These scenarios assert only the DESK seam angles: the hosted-origin
// shape across all three copy call sites, fetch transport with working
// uploads/downloads plus browser abort semantics, the full device grant
// through the web page, close-and-reopen cookie semantics, zoom-menu
// absence, same-origin navigation, and the rendered entry plus the
// healthy catch-all.
//
// Fixtures: scenario-owned issues created over the API (deleted in a
// finally); device grants are single-use by design.
import { test, expect } from "../../fixtures";
import type { ParityDriver } from "../../drivers/parity-driver";
import {
  createIssue,
  deleteIssue,
  issueAttachments,
  serverDeskShellDeviceApprove,
  serverDeskShellDeviceApproveStatus,
  serverDeskShellDeviceStart,
  serverDeskShellDeviceToken,
  serverDeskShellDeviceTokenError,
  serverIssues,
  serverProjectIdentifier,
  sessionValid,
  signInSession,
} from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";

// Uploads run the full bytes round trip, and cold dev servers compile
// route bundles on first load; each step still carries its own poll.
test.setTimeout(600_000);

/** Distinctive label of the desktop zoom-reset menu entry (main.rs). Absent on web. */
const ZOOM_RESET_LABEL = "Actual Size";

/** Desktop View-menu zoom entries (main.rs). Absent on web. */
const ZOOM_LABELS = ["Zoom In", "Zoom Out", ZOOM_RESET_LABEL];

/** Heading of the web device-code approval page. Renders on web. */
const DEVICE_PAGE_HEADING = "Authorize Pi Dash CLI";

/** Heading of the app's catch-all not-found page. Renders on web. */
const NOT_FOUND_HEADING = "Oops! Something went wrong.";

/** Home action of the app's catch-all not-found page. */
const NOT_FOUND_HOME = "Go to Home";

// A 2x2 solid-red PNG, generated for this spec: the server enforces a
// MIME whitelist, so uploads use an allowed image type.
const PIXEL_PNG = Buffer.from(
  "iVBORw0KGgoAAAANSUhEUgAAAAIAAAACCAIAAAD91JpzAAAAEElEQVR4nGP4z8AARAwQCgAf7gP9i18U1AAAAABJRU5ErkJggg==",
  "base64"
);

const POLL_60 = { timeout: 60_000 };

/** Origin of the loaded page (the hosted app origin on web). */
function pageOrigin(driver: ParityDriver): string {
  return new URL(driver.page.url()).origin;
}

/** Session cookie names a signed-out visit must never plant. */
async function sessionCookieNames(driver: ParityDriver): Promise<string[]> {
  const cookies = await driver.page.context().cookies();
  return cookies.map((cookie) => cookie.name);
}

test(
  specTitle(["DESK-019"], "copy-link actions produce hosted same-origin work-item URLs"),
  { tag: specTags(["DESK-019"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const identifier = await serverProjectIdentifier(seed.workspaceSlug, seed.projectId, session);
    const name = `Shell link ${Date.now()}`;
    const created = await createIssue(seed.workspaceSlug, seed.projectId, session, name);
    const seq = `${identifier}-${created.sequence_id}`;
    const expectedPath = `/${seed.workspaceSlug}/browse/${seq}/`;
    try {
      await test.step("sign in and grant the clipboard", async () => {
        await driver.openEntry();
        await driver.signInWithPassword(seed.email, seed.password);
        await driver.openWorkspaceHome(seed.workspaceSlug);
        await expect.poll(() => driver.sidebarLinkTexts(), POLL_60).toContain("Home");
        await driver.page.context().grantPermissions(["clipboard-read", "clipboard-write"]);
      });

      await test.step("the detail header copies the hosted absolute URL", async () => {
        await driver.openIssueDetail(seed.workspaceSlug, seq);
        await expect.poll(() => driver.currentPath(), POLL_60).toContain(`/browse/${seq}`);
        await driver.copyIssueLink();
        const origin = pageOrigin(driver);
        expect(origin).toMatch(/^https?:\/\//);
        expect(await driver.readClipboard()).toBe(`${origin}${expectedPath}`);
      });

      await test.step("the list menu copies the same hosted URL", async () => {
        await driver.openProjectIssues(seed.workspaceSlug, seed.projectId);
        await expect.poll(() => driver.visibleIssueNames(), POLL_60).toContain(name);
        await driver.layoutsRowMenuChoose(name, "Copy link");
        expect(await driver.readClipboard()).toBe(`${pageOrigin(driver)}${expectedPath}`);
      });

      await test.step("the peek header copies the same hosted URL", async () => {
        await driver.openPeek(seed.workspaceSlug, seed.projectId, created.id);
        await expect.poll(() => driver.peekTitle(), POLL_60).toBe(name);
        await driver.copyPeekLink();
        const copied = await driver.readClipboard();
        expect(copied).toBe(`${pageOrigin(driver)}${expectedPath}`);
        expect(copied).not.toContain("tauri://");
      });
    } finally {
      await deleteIssue(seed.workspaceSlug, seed.projectId, created.id, session);
    }

    await test.step("the server session stays valid throughout", async () => {
      expect(await sessionValid(await signInSession(seed.email, seed.password))).toBe(true);
    });
  }
);

test(
  specTitle(["DESK-020"], "API transport on web is browser fetch with working uploads and downloads"),
  { tag: specTags(["DESK-020"]) },
  async ({ driver, seed }) => {
    await test.step("entry carries no native transport bridge", async () => {
      await driver.visit("/");
      await expect.poll(() => driver.authStep(), POLL_60).toBe("email");
      expect(await driver.desktopRuntimeIsTauriPresent()).toBe(false);
      expect(await driver.deskShellNativeHttpPresent()).toBe(false);
    });

    const session = await signInSession(seed.email, seed.password);
    const identifier = await serverProjectIdentifier(seed.workspaceSlug, seed.projectId, session);
    const name = `Shell transport ${Date.now()}`;
    const created = await createIssue(seed.workspaceSlug, seed.projectId, session, name);
    const fileName = `shell-roundtrip-${Date.now()}.png`;
    await driver.deskShellBeginTrafficWatch();
    try {
      await test.step("sign in and reach the scenario issue", async () => {
        await driver.openEntry();
        await driver.signInWithPassword(seed.email, seed.password);
        await driver.openWorkspaceHome(seed.workspaceSlug);
        await expect.poll(() => driver.sidebarLinkTexts(), POLL_60).toContain("Home");
        await driver.openIssueDetail(seed.workspaceSlug, `${identifier}-${created.sequence_id}`);
        await expect.poll(() => driver.issueDetailTitle(), POLL_60).toBe(name);
      });

      await test.step("an upload round-trips its bytes back down", async () => {
        await driver.uploadAttachment({ name: fileName, mime: "image/png", bytes: PIXEL_PNG });
        await expect
          .poll(
            async () => (await issueAttachments(seed.workspaceSlug, seed.projectId, created.id, session)).length,
            POLL_60
          )
          .toBe(1);
        await expect
          .poll(() => driver.widgetRowNames("Attachments"), POLL_60)
          .toEqual(expect.arrayContaining([expect.stringContaining(fileName)]));
        const opened = await driver.downloadWidgetRowFile("Attachments", fileName);
        expect(opened.fileName).toBe(fileName);
        expect(opened.bytes.equals(PIXEL_PNG)).toBe(true);
      });

      await test.step("the session spoke same-origin fetch with no native IPC", async () => {
        const urls = await driver.deskShellTrafficUrls();
        // Non-vacuity: the watch observed the session's real traffic.
        expect(urls.length).toBeGreaterThan(0);
        const apiUrls = urls.filter((url) => url.includes("/api/"));
        expect(apiUrls.length).toBeGreaterThan(0);
        for (const url of urls) {
          expect(url).toMatch(/^https?:\/\//);
          expect(url).not.toContain("tauri://");
          expect(url).not.toContain("ipc://");
        }
        // Relative addresses resolve against the page: every API call
        // leaves from the hosted app origin.
        const origin = pageOrigin(driver);
        for (const url of apiUrls) expect(url.startsWith(origin)).toBe(true);
      });
    } finally {
      // The watch ends before the failure probe below so the probe's own
      // unroutable request cannot pollute the traffic assertions above.
      await driver.deskShellEndTrafficWatch().catch(() => undefined);
      await deleteIssue(seed.workspaceSlug, seed.projectId, created.id, session);
    }

    await test.step("cancels, timeouts, and failures are browser semantics", async () => {
      // No desktop_api_cancel command exists without the bridge: an
      // aborted fetch rejects as AbortError, a blown timeout as
      // TimeoutError, and an unroutable host as a TypeError network
      // failure — never a forged logout.
      expect(await driver.deskShellAbortSemantics()).toEqual({
        aborted: "AbortError",
        timedOut: "TimeoutError",
        unroutable: "TypeError",
      });
    });

    await test.step("no native bridge appears behind signed-in use", async () => {
      expect(await driver.desktopRuntimeIsTauriPresent()).toBe(false);
      expect(await driver.deskShellNativeHttpPresent()).toBe(false);
    });

    await test.step("the server session stays valid throughout", async () => {
      expect(await sessionValid(await signInSession(seed.email, seed.password))).toBe(true);
    });
  }
);

test(
  specTitle(["DESK-021"], "CLI sign-in on web runs through the human-typed device approval"),
  { tag: specTags(["DESK-021"]) },
  async ({ driver, seed }) => {
    await test.step("sign in and reach the device page", async () => {
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.openWorkspaceHome(seed.workspaceSlug);
      await expect.poll(() => driver.sidebarLinkTexts(), POLL_60).toContain("Home");
      await driver.visit("/auth/device/");
      await expect.poll(() => driver.deskShellDeviceAuthHeading(), POLL_60).toBe(DEVICE_PAGE_HEADING);
      // The form needs a full code before it submits.
      expect(await driver.deskShellDeviceApproveEnabled()).toBe(false);
    });

    await test.step("an unknown code is refused with its reason", async () => {
      await driver.deskShellDeviceSubmitCode("ZZZZ9999");
      await expect.poll(() => driver.pageText(), POLL_60).toContain("not recognized");
      // The refusal keeps the form up for a corrected code.
      expect(await driver.deskShellDeviceAuthHeading()).toBe(DEVICE_PAGE_HEADING);
    });

    const grant = await serverDeskShellDeviceStart();
    await test.step("a grant starts with its polling facts", async () => {
      expect(grant.userCode).toMatch(/^[A-Z0-9]{4}-[A-Z0-9]{4}$/);
      expect(grant.deviceCode.length).toBeGreaterThan(0);
      expect(grant.verificationUri).toContain("/auth/device/");
      expect(grant.expiresIn).toBe(600);
      expect(grant.interval).toBe(5);
    });

    await test.step("the token poll waits while the grant is pending", async () => {
      expect(await serverDeskShellDeviceTokenError(grant.deviceCode)).toEqual({
        status: 400,
        error: "authorization_pending",
      });
    });

    await test.step("typing the code in the page approves the grant", async () => {
      await driver.deskShellDeviceSubmitCode(grant.userCode);
      await expect.poll(() => driver.pageText(), POLL_60).toContain("Approved.");
      expect(await driver.pageText()).toContain(seed.email);
    });

    await test.step("the approved device code exchanges for a CLI token", async () => {
      // Honor the RFC poll gap: the pending poll above touched the grant
      // moments ago, and a fast re-poll answers slow_down instead.
      await new Promise((resolve) => setTimeout(resolve, 4_000));
      const token = await serverDeskShellDeviceToken(grant.deviceCode);
      expect(token.accessToken.length).toBeGreaterThan(0);
      expect(token.userEmail).toBe(seed.email);
      expect(token.workspaceSlug).toBe(seed.workspaceSlug);
    });

    await test.step("a second grant approves through the API alone", async () => {
      // The desktop flow's exact path: start the grant, approve it with
      // the session over the API (no web page involved), then exchange.
      // The fresh grant has never been polled, so its first token call
      // is accepted without honoring the poll gap.
      const second = await serverDeskShellDeviceStart();
      const session = await signInSession(seed.email, seed.password);
      const approval = await serverDeskShellDeviceApprove(second.userCode, session);
      expect(approval.ok).toBe(true);
      expect(approval.userEmail).toBe(seed.email);
      expect(approval.workspaceSlug).toBe(seed.workspaceSlug);
      const token = await serverDeskShellDeviceToken(second.deviceCode);
      expect(token.accessToken.length).toBeGreaterThan(0);
      expect(token.userEmail).toBe(seed.email);
    });

    await test.step("approval stays gated on a session and a real code", async () => {
      // Anonymous callers cannot approve: the endpoint demands the
      // signed-in session.
      expect([401, 403]).toContain(await serverDeskShellDeviceApproveStatus(grant.userCode, null));
      const session = await signInSession(seed.email, seed.password);
      expect(await serverDeskShellDeviceApproveStatus("ZZZZ9999", session)).toBe(404);
    });

    await test.step("no native bridge backs the CLI commands", async () => {
      // detect/install/login are Tauri invokes; without the bridge the
      // in-app self-approving flow cannot run at all.
      expect(await driver.desktopRuntimeIsTauriPresent()).toBe(false);
      expect(await driver.deskShellNativeHttpPresent()).toBe(false);
    });

    await test.step("the server session stays valid throughout", async () => {
      expect(await sessionValid(await signInSession(seed.email, seed.password))).toBe(true);
    });
  }
);

test(
  specTitle(["DESK-022"], "closing the web app ends the page with no tray parking"),
  { tag: specTags(["DESK-022"]) },
  async ({ driver, seed }) => {
    await test.step("sign in with no native window bridge", async () => {
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.openWorkspaceHome(seed.workspaceSlug);
      await expect.poll(() => driver.sidebarLinkTexts(), POLL_60).toContain("Home");
      expect(await driver.desktopRuntimeIsTauriPresent()).toBe(false);
    });

    await test.step("the session is valid before the close", async () => {
      expect(await sessionValid(await signInSession(seed.email, seed.password))).toBe(true);
    });

    await test.step("reopening after the close lands back in the workspace", async () => {
      // Nothing parks: the close ends the page, and the fresh load
      // carries the same cookie session, landing in the workspace by
      // normal browser semantics. The driver's page stays closed, so
      // only API checks follow this step.
      expect(await driver.deskShellCloseReopenPath()).toContain(seed.workspaceSlug);
    });

    await test.step("the server session survives the close", async () => {
      expect(await sessionValid(await signInSession(seed.email, seed.password))).toBe(true);
    });
  }
);

test(
  specTitle(["DESK-023"], "no app zoom menu on web — browser zoom only"),
  { tag: specTags(["DESK-023"]) },
  async ({ driver, seed }) => {
    await test.step("entry shows no zoom menu", async () => {
      await driver.visit("/");
      await expect.poll(() => driver.authStep(), POLL_60).toBe("email");
      expect(await driver.deskShellZoomMenuVisible()).toBe(false);
      const text = await driver.pageText();
      for (const label of ZOOM_LABELS) expect(text).not.toContain(label);
    });

    await test.step("signed-in use stays zoom-menu free", async () => {
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.openWorkspaceHome(seed.workspaceSlug);
      await expect.poll(() => driver.sidebarLinkTexts(), POLL_60).toContain("Home");
      expect(await driver.deskShellZoomMenuVisible()).toBe(false);
      expect(await driver.pageText()).not.toContain(ZOOM_RESET_LABEL);
    });

    await test.step("navigating keeps it absent across mounts", async () => {
      await driver.visit(`/${seed.workspaceSlug}/projects/${seed.projectId}/issues`);
      await expect.poll(() => driver.currentPath(), POLL_60).toContain("/issues");
      expect(await driver.deskShellZoomMenuVisible()).toBe(false);
      expect(await driver.pageText()).not.toContain(ZOOM_RESET_LABEL);
    });

    await test.step("no native bridge backs the zoom commands", async () => {
      expect(await driver.desktopRuntimeIsTauriPresent()).toBe(false);
    });

    await test.step("the server session stays valid throughout", async () => {
      expect(await sessionValid(await signInSession(seed.email, seed.password))).toBe(true);
    });
  }
);

test(
  specTitle(["DESK-024"], "server-host navigation on web loads normally with no bounce back"),
  { tag: specTags(["DESK-024"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const identifier = await serverProjectIdentifier(seed.workspaceSlug, seed.projectId, session);
    const issues = await serverIssues(seed.workspaceSlug, seed.projectId, session);
    if (issues.length === 0) throw new Error("[parity] seeded project carried no issues.");
    const seq = `${identifier}-${issues[0].sequenceId}`;

    let entryOrigin = "";
    await test.step("entry and its redirect stay on the server host", async () => {
      await driver.visit("/");
      await expect.poll(() => driver.authStep(), POLL_60).toBe("email");
      entryOrigin = pageOrigin(driver);
      expect(entryOrigin).toMatch(/^https?:\/\//);
      await driver.visit("/login");
      await expect.poll(() => driver.currentPath(), POLL_60).toBe("/");
      expect(pageOrigin(driver)).toBe(entryOrigin);
      await expect.poll(() => driver.authStep(), POLL_60).toBe("email");
    });

    await test.step("sign in and load the workspace in place", async () => {
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.openWorkspaceHome(seed.workspaceSlug);
      await expect.poll(() => driver.sidebarLinkTexts(), POLL_60).toContain("Home");
      expect(pageOrigin(driver)).toBe(entryOrigin);
    });

    await test.step("the issues list loads in place", async () => {
      await driver.visit(`/${seed.workspaceSlug}/projects/${seed.projectId}/issues`);
      await expect.poll(() => driver.currentPath(), POLL_60).toContain("/issues");
      expect(pageOrigin(driver)).toBe(entryOrigin);
      await expect.poll(() => driver.visibleIssueNames(), POLL_60).not.toHaveLength(0);
    });

    await test.step("the work-item detail loads in place", async () => {
      await driver.openIssueDetail(seed.workspaceSlug, seq);
      await expect.poll(() => driver.currentPath(), POLL_60).toContain(`/browse/${seq}`);
      expect(pageOrigin(driver)).toBe(entryOrigin);
      expect((await driver.pageText()).length).toBeGreaterThan(0);
    });

    await test.step("the server session stays valid throughout", async () => {
      expect(await sessionValid(await signInSession(seed.email, seed.password))).toBe(true);
    });
  }
);

test(
  specTitle(["DESK-025"], "marketing entry renders on web and unknown paths hit the normal catch-all"),
  { tag: specTags(["DESK-025"]) },
  async ({ driver, seed }) => {
    await test.step("the signed-out entry renders its framed card, not not-found", async () => {
      // DESK-011 owns entry-route equivalence; this leg pins the same
      // render as bundle completeness: the marketing entry resolves to
      // content on web instead of falling through.
      await driver.visit("/");
      await expect.poll(() => driver.authStep(), POLL_60).toBe("email");
      expect(await driver.currentPath()).toBe("/");
      expect(await driver.deskSigninMarketingFrameVisible()).toBe(true);
      expect(await driver.seesGenericSignInHeader()).toBe(true);
      expect(await driver.pageText()).not.toContain(NOT_FOUND_HEADING);
    });

    await test.step("the login path resolves to the framed sign-in", async () => {
      await driver.visit("/login");
      await expect.poll(() => driver.currentPath(), POLL_60).toBe("/");
      await expect.poll(() => driver.authStep(), POLL_60).toBe("email");
      expect(await driver.deskSigninMarketingFrameVisible()).toBe(true);
    });

    await test.step("the visits planted no session", async () => {
      expect(await sessionCookieNames(driver)).not.toContain("session-id");
    });

    await test.step("a genuinely unknown path renders the app not-found page", async () => {
      // Signed-out unknown paths bounce to the sign-in (the auth guard's
      // territory), and a single segment matches :workspaceSlug (the
      // workspace scope's own not-found); signed in, a path no route
      // matches reaches the healthy catch-all — unknown paths 404 because
      // they do not exist, not because the bundle lacks them.
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.openWorkspaceHome(seed.workspaceSlug);
      await expect.poll(() => driver.sidebarLinkTexts(), POLL_60).toContain("Home");
      await driver.visit("/__parity_no_such_ws__/__parity_no_such_page__");
      await expect.poll(() => driver.pageText(), POLL_60).toContain(NOT_FOUND_HEADING);
      expect(await driver.pageText()).toContain(NOT_FOUND_HOME);
    });

    await test.step("the server session stays valid throughout", async () => {
      expect(await sessionValid(await signInSession(seed.email, seed.password))).toBe(true);
    });
  }
);
