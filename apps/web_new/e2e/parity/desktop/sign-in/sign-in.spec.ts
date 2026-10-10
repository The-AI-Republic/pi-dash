// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-208): desktop bare sign-in, deep links,
// onboarding seam — web-observable sides. Rows: DESK-011–DESK-015,
// DESK-027.
//
// The oracle runs on apps/web, where the desktop halves are absent by
// design: signed-out entry routes show the marketing-framed sign-in (the
// web login/sign-in routes redirect to "/" dropping their query, while the
// desktop overrides keep the failure query and render the bare card); the
// failure-query mapping and the community fallback card live only in the
// desktop bundle; the community server exposes no desktop-exchange route
// (the cloud edition owns it per CLOUD-018); external destinations open
// through browser tabs with no native bridge; and onboarding still opens
// on the CLI-install step (its removal is the in-flight desktop-only
// main#494, so the oracle pins the current-main web behavior).
//
// Overlap: AUTH-030 owns the CLI step's guidance and continues, AUTH-043
// owns workspace-step back navigation, SHELL-060 owns the help menu and
// its popups, AUTH-020 owns the signed-out next_path bounce, and AUTH-006
// owns error_code banners. These scenarios assert only the DESK seam
// angles: multi-route entry equivalence, the signed-in forward honoring a
// pending destination, the failure-query drop, fallback-card absence with
// a full normal sign-in, exchange-route absence plus unmarked sessions,
// plain-anchor externals with no bridge, and the CLI-first progress
// order with profile-to-CLI back navigation.
//
// Fixtures: the seeded workspace for signed-in entry angles; fresh
// accounts minted over the native sign-up endpoint for onboarding. The
// desktop deep-link scheme itself is native-side (main.rs) and has no
// web-observable half beyond the exchange route's absence, recorded in
// DESK-014.
import { test, expect } from "../../fixtures";
import type { ParityDriver } from "../../drivers/parity-driver";
import {
  createAccountSession,
  serverDeskSigninDesktopExchangeStatus,
  serverDeskSigninSessionHasDesktopMarker,
  sessionValid,
  signInSession,
  uniqueEmail,
  userProfile,
} from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";

/** Heading of the desktop-only failure banner (overlay home page). */
const DESKTOP_FAILURE_HEADING = "Sign-in didn't complete";

/** Heading of the desktop-only community fallback card. */
const FALLBACK_CARD_HEADING = "Desktop sign-in isn't available yet";

/** Action of the desktop-only community fallback card. */
const FALLBACK_CARD_ACTION = "Open Pi Dash in your browser";

const FRESH_PASSWORD = "Parity-Desk-1!";

const POLL_60 = { timeout: 60_000 };

/** Entry routes that render the bare card on desktop. */
const ENTRY_ROUTES = ["/", "/login", "/sign-in"] as const;

/** Session cookie names a signed-out visit must never plant. */
async function sessionCookieNames(driver: ParityDriver): Promise<string[]> {
  const cookies = await driver.page.context().cookies();
  return cookies.map((cookie) => cookie.name);
}

function parseProgress(reading: string | null): { current: number; total: number } {
  const match = /^(\d+)\/(\d+)$/.exec(reading ?? "");
  if (!match) throw new Error(`[parity] unexpected onboarding progress reading: ${String(reading)}.`);
  return { current: Number(match[1]), total: Number(match[2]) };
}

test(
  specTitle(["DESK-011"], "signed-out entry routes show the marketing-framed sign-in"),
  { tag: specTags(["DESK-011"]) },
  async ({ driver }) => {
    for (const route of ENTRY_ROUTES) {
      await test.step(`route ${route} lands on the framed card`, async () => {
        await driver.visit(route);
        await expect.poll(() => driver.authStep(), POLL_60).toBe("email");
        // The web redirects collapse every entry route onto "/".
        expect(await driver.currentPath()).toBe("/");
        expect(await driver.deskSigninMarketingFrameVisible()).toBe(true);
        expect(await driver.seesGenericSignInHeader()).toBe(true);
        expect(await driver.bannerText()).toBeNull();
        const text = await driver.pageText();
        expect(text).not.toContain(FALLBACK_CARD_HEADING);
        expect(text).not.toContain(FALLBACK_CARD_ACTION);
      });
    }

    await test.step("the visits planted no session", async () => {
      expect(await sessionCookieNames(driver)).not.toContain("session-id");
    });
  }
);

test(
  specTitle(["DESK-011"], "signed-in users are forwarded into the workspace honoring a pending destination"),
  { tag: specTags(["DESK-011"]) },
  async ({ driver, seed }) => {
    const destination = `/${seed.workspaceSlug}/projects/${seed.projectId}/issues`;

    await test.step("sign in through the UI", async () => {
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
    });

    await test.step("a pending destination is honored", async () => {
      await driver.visit(`/?next_path=${encodeURIComponent(destination)}`);
      await expect.poll(() => driver.currentPath(), POLL_60).toContain(destination);
    });

    await test.step("a bare entry visit lands in the workspace", async () => {
      await driver.visit("/");
      await expect.poll(() => driver.currentPath(), POLL_60).toContain(seed.workspaceSlug);
    });

    await test.step("the login redirect drops its query on web", async () => {
      // The web /login route redirects to "/" without carrying the query
      // (the desktop override keeps it), so a pending destination passed
      // there is lost and the default workspace landing wins instead. The
      // workspace slug also appears inside the encoded query, so the poll
      // must wait for the query to be gone, not merely the slug present.
      await driver.visit(`/login?next_path=${encodeURIComponent(destination)}`);
      await expect
        .poll(async () => {
          const path = await driver.currentPath();
          return !path.includes("next_path=") && path.includes(seed.workspaceSlug);
        }, POLL_60)
        .toBe(true);
      expect(await driver.currentPath()).not.toContain("/issues");
    });

    await test.step("the server session stays valid throughout", async () => {
      expect(await sessionValid(await signInSession(seed.email, seed.password))).toBe(true);
    });
  }
);

test(
  specTitle(["DESK-012"], "failure queries land on the plain sign-in with no banner"),
  { tag: specTags(["DESK-012"]) },
  async ({ driver }) => {
    const failure = "error=access_denied&error_description=The+provider+refused+the+flow";

    await test.step("the sign-in failure landing collapses onto the plain card", async () => {
      // Desktop renders the failure banner here (its sign-in override
      // keeps the query); web redirects to "/" and drops it.
      await driver.visit(`/sign-in?${failure}`);
      await expect.poll(() => driver.authStep(), POLL_60).toBe("email");
      expect(await driver.currentPath()).toBe("/");
      expect(await driver.bannerText()).toBeNull();
      expect(await driver.pageText()).not.toContain(DESKTOP_FAILURE_HEADING);
    });

    await test.step("the failure query maps to nothing on the web entry either", async () => {
      // The ?error=/?error_description= mapping lives only in the desktop
      // bundle; the web card reads error_code instead (AUTH-006).
      await driver.visit(`/?${failure}`);
      await expect.poll(() => driver.authStep(), POLL_60).toBe("email");
      expect(await driver.bannerText()).toBeNull();
      expect(await driver.pageText()).not.toContain(DESKTOP_FAILURE_HEADING);
    });

    await test.step("the failure landings planted no session", async () => {
      expect(await sessionCookieNames(driver)).not.toContain("session-id");
    });
  }
);

test(
  specTitle(["DESK-013"], "community users get the normal sign-in; the fallback card never renders"),
  { tag: specTags(["DESK-013"]) },
  async ({ driver, seed }) => {
    for (const route of ENTRY_ROUTES) {
      await test.step(`route ${route} shows the working card, not the fallback`, async () => {
        await driver.visit(route);
        await expect.poll(() => driver.authStep(), POLL_60).toBe("email");
        expect(await driver.seesGenericSignInHeader()).toBe(true);
        const text = await driver.pageText();
        expect(text).not.toContain(FALLBACK_CARD_HEADING);
        expect(text).not.toContain(FALLBACK_CARD_ACTION);
      });
    }

    await test.step("the normal sign-in completes into the workspace", async () => {
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await expect.poll(() => driver.currentPath(), POLL_60).toContain(seed.workspaceSlug);
    });

    await test.step("the server agrees the session is live", async () => {
      expect(await sessionValid(await signInSession(seed.email, seed.password))).toBe(true);
    });
  }
);

test(
  specTitle(["DESK-014"], "the community server exposes no desktop code exchange; web sessions stay unmarked"),
  { tag: specTags(["DESK-014"]) },
  async ({ driver, seed }) => {
    const session = await test.step("a web session exists", async () => signInSession(seed.email, seed.password));

    await test.step("sign-in through the UI completes with no callback leg", async () => {
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await expect.poll(() => driver.currentPath(), POLL_60).toContain(seed.workspaceSlug);
      expect(await driver.currentPath()).not.toContain("desktop-exchange");
    });

    await test.step("the browser-facing exchange address answers not-found", async () => {
      // The exact address the native shell navigates its webview to after
      // a deep-link callback; the community stack serves nothing there.
      expect((await serverDeskSigninDesktopExchangeStatus({})).status).toBe(404);
      expect((await serverDeskSigninDesktopExchangeStatus({ query: "" })).status).toBe(404);
    });

    await test.step("an authenticated exchange attempt answers not-found too", async () => {
      // A web session triggers nothing: no route exists with or without
      // credentials, over GET or POST, at the edge or direct.
      expect((await serverDeskSigninDesktopExchangeStatus({ method: "POST", sessionCookie: session })).status).toBe(
        404
      );
      expect((await serverDeskSigninDesktopExchangeStatus({ viaEdge: false })).status).toBe(404);
    });

    await test.step("the web session carries no desktop marker", async () => {
      expect(await serverDeskSigninSessionHasDesktopMarker(session)).toBe(false);
    });
  }
);

test(
  specTitle(["DESK-015"], "external destinations open in browser tabs with no native bridge"),
  { tag: specTags(["DESK-015"]) },
  async ({ driver, seed }) => {
    await test.step("sign in and reach the app home", async () => {
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.openWorkspaceHome(seed.workspaceSlug);
      await expect.poll(() => driver.sidebarLinkTexts(), POLL_60).toContain("Home");
    });

    await test.step("no native bridge is present", async () => {
      expect(await driver.desktopRuntimeIsTauriPresent()).toBe(false);
    });

    await test.step("the updates dialog exposes its externals as plain anchors", async () => {
      await driver.openHelpMenu();
      expect(await driver.activateHelpEntry("What's new?")).toBeNull();
      await expect.poll(() => driver.isDialogWithTextVisible("What's new?"), { timeout: 30_000 }).toBe(true);
      const links = await driver.deskSigninUpdateDialogLinks();
      expect(links.length).toBeGreaterThanOrEqual(4);
      for (const link of links) {
        expect(link.tag).toBe("a");
        expect(link.target).toBe("_blank");
      }
      const web = links.filter((link) => link.href.startsWith("http"));
      expect(web.length).toBeGreaterThan(0);
      for (const link of web) {
        expect(link.href).toMatch(/^https:/);
      }
      expect(links.map((link) => link.label)).toEqual(
        expect.arrayContaining(["Docs", "Full changelog", "Support", "Forum"])
      );
    });

    await test.step("following one opens a browser tab at the hosted address", async () => {
      const docsUrl = await driver.deskSigninOpenUpdateDialogLink("Docs");
      expect(docsUrl ?? "").toContain("github.com/The-AI-Republic/pi-dash");
      expect(await driver.desktopRuntimeIsTauriPresent()).toBe(false);
    });

    await test.step("the server session is untouched by the external navigation", async () => {
      expect(await sessionValid(await signInSession(seed.email, seed.password))).toBe(true);
    });
  }
);

test(
  specTitle(["DESK-027"], "web onboarding opens on the CLI step with it first in the progress order"),
  { tag: specTags(["DESK-027"]) },
  async ({ driver }) => {
    const email = uniqueEmail("desk-cli");
    const session = await test.step("a fresh account exists", async () => createAccountSession(email, FRESH_PASSWORD));
    const baseline = (await userProfile(session)).onboarding_step;

    await test.step("sign in and open onboarding", async () => {
      await driver.openEntry();
      await driver.signInWithPassword(email, FRESH_PASSWORD);
      await driver.openOnboarding();
      await expect.poll(() => driver.pageText(), POLL_60).toContain("Install the Pi Dash CLI");
    });

    let total = 0;
    await test.step("onboarding opens on the CLI step, first in the order", async () => {
      expect(await driver.pageText()).not.toContain("Create your profile");
      const progress = parseProgress(await driver.deskSigninOnboardingProgress());
      expect(progress.current).toBe(1);
      expect(progress.total).toBeGreaterThanOrEqual(2);
      total = progress.total;
      expect(await driver.isOnboardingBackVisible()).toBe(false);
    });

    await test.step("advancing reaches profile setup as the second step", async () => {
      await driver.advanceCliInstall();
      await expect.poll(() => driver.pageText(), POLL_60).toContain("Create your profile");
      const progress = parseProgress(await driver.deskSigninOnboardingProgress());
      expect(progress).toEqual({ current: 2, total });
    });

    await test.step("profile setup offers the way back to the CLI step", async () => {
      // Desktop's target hides the back control here (no earlier step);
      // on web it returns to the CLI step it came from.
      expect(await driver.isOnboardingBackVisible()).toBe(true);
      await driver.goBackOnboardingStep();
      await expect.poll(() => driver.pageText(), POLL_60).toContain("Install the Pi Dash CLI");
      expect(parseProgress(await driver.deskSigninOnboardingProgress())).toEqual({ current: 1, total });
    });

    await test.step("the round trip stored nothing", async () => {
      expect((await userProfile(session)).onboarding_step).toEqual(baseline);
    });
  }
);
