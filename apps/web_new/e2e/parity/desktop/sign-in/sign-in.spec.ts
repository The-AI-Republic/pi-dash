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
import type { ParityDriver, ParitySeedFacts } from "../../drivers/parity-driver";
import {
  createAccountSession,
  serverDeskSigninDesktopExchangeStatus,
  serverDeskSigninSessionHasDesktopMarker,
  serverSetTourCompleted,
  sessionBrowserCookies,
  sessionValid,
  signInSession,
  uniqueEmail,
  userProfile,
} from "../../helpers/api";
import { parityOracleUrl } from "../../helpers/parity-env";
import { specTags, specTitle } from "../../helpers/tags";
import { desktopEmit, desktopInvocations, installDesktopMock } from "../tauri-mock";

/** Heading of the desktop-only failure banner (overlay home page). */
const DESKTOP_FAILURE_HEADING = "Sign-in didn't complete";

/** Heading of the desktop-only community fallback card. */
const FALLBACK_CARD_HEADING = "Desktop sign-in isn't available yet";

/** Action of the desktop-only community fallback card. */
const FALLBACK_CARD_ACTION = "Open Pi Dash in your browser";

/** web_new desktop failure-banner heading (rewritten, never the overlay's). */
const NEW_FAILURE_HEADING = "Could not finish signing in";

/** web_new desktop unavailable-card heading on the OSS build (no hand-off). */
const NEW_UNAVAILABLE_HEADING = "The desktop app can't sign you in yet";

/** web_new desktop unavailable-card browser action. */
const NEW_UNAVAILABLE_ACTION = "Continue in the browser";

/** Install the mock bridge, then plant a signed-in session for desktop legs. */
async function enterDesktopSignedIn(driver: ParityDriver, email: string, password: string): Promise<string> {
  await installDesktopMock(driver.page);
  const session = await signInSession(email, password);
  await driver.page.context().addCookies(sessionBrowserCookies(session));
  return session;
}

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
    if (driver.target === "web_new") {
      await desk011SignedOutPositive(driver);
      return;
    }
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
    if (driver.target === "web_new") {
      await desk011SignedInPositive(driver, seed);
      return;
    }
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
    if (driver.target === "web_new") {
      await desk012Positive(driver);
      return;
    }
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
    if (driver.target === "web_new") {
      await desk013Positive(driver, seed);
      return;
    }
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
    if (driver.target === "web_new") {
      await desk014Positive(driver, seed);
      return;
    }
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
    if (driver.target === "web_new") {
      await desk015Positive(driver, seed);
      return;
    }
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
    if (driver.target === "web_new") {
      await desk027Positive(driver);
      return;
    }
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

// ---------------------------------------------------------------------------
// Desktop-positive legs (NEWFRONT-595). The oracle above pins the web side
// (framed card, redirects, absence); these legs run only on web_new in a
// desktop-mode run and prove the bare entry, the failure landing, the
// fallback card, deep-link handling, the opener guard and the onboarding
// seam. Signed-in legs plant the session cookie over the API: the OSS
// server offers no desktop hand-off, so no UI sign-in exists to drive.
// ---------------------------------------------------------------------------

async function desk011SignedOutPositive(driver: ParityDriver): Promise<void> {
  await installDesktopMock(driver.page);
  for (const route of ENTRY_ROUTES) {
    await test.step(`route ${route} renders the bare card in place`, async () => {
      await driver.visit(route);
      await expect.poll(() => driver.pageText(), POLL_60).toContain(NEW_UNAVAILABLE_HEADING);
      // The desktop forks render in place; nothing collapses onto "/".
      expect(await driver.currentPath()).toBe(route);
      expect(await driver.deskSigninMarketingFrameVisible()).toBe(false);
      expect(await driver.seesGenericSignInHeader()).toBe(false);
      expect(await driver.bannerText()).toBeNull();
      const text = await driver.pageText();
      expect(text).toContain(NEW_UNAVAILABLE_ACTION);
      expect(text).not.toContain(FALLBACK_CARD_HEADING);
    });
  }

  await test.step("the visits planted no session", async () => {
    expect(await sessionCookieNames(driver)).not.toContain("session-id");
  });
}

async function desk011SignedInPositive(driver: ParityDriver, seed: ParitySeedFacts): Promise<void> {
  const destination = `/${seed.workspaceSlug}/projects/${seed.projectId}/issues`;
  await enterDesktopSignedIn(driver, seed.email, seed.password);

  await test.step("a pending destination is honored", async () => {
    await driver.visit(`/?next_path=${encodeURIComponent(destination)}`);
    await expect.poll(() => driver.currentPath(), POLL_60).toContain(destination);
  });

  await test.step("a bare entry visit lands in the workspace", async () => {
    await driver.visit("/");
    await expect.poll(() => driver.currentPath(), POLL_60).toContain(seed.workspaceSlug);
  });

  await test.step("the login route keeps its query and honors the destination", async () => {
    // Inversion of the oracle's drop leg: the desktop fork keeps the
    // query, so the pending destination wins instead of the default
    // workspace landing.
    await driver.visit(`/login?next_path=${encodeURIComponent(destination)}`);
    await expect
      .poll(async () => {
        const path = await driver.currentPath();
        return !path.includes("next_path=") && path.includes("/issues");
      }, POLL_60)
      .toBe(true);
  });

  await test.step("the server session stays valid throughout", async () => {
    expect(await sessionValid(await signInSession(seed.email, seed.password))).toBe(true);
  });
}

async function desk012Positive(driver: ParityDriver): Promise<void> {
  await installDesktopMock(driver.page);
  const failure = "error=access_denied&error_description=The+provider+refused+the+flow";

  await test.step("the sign-in failure landing keeps its query and banners", async () => {
    await driver.visit(`/sign-in?${failure}`);
    await expect.poll(() => driver.bannerText(), POLL_60).toContain(NEW_FAILURE_HEADING);
    expect(await driver.bannerText()).toContain("The provider refused the flow");
    expect(await driver.currentPath()).toContain("error=access_denied");
    await expect.poll(() => driver.pageText(), POLL_60).toContain(NEW_UNAVAILABLE_HEADING);
  });

  await test.step("the entry maps the same failure the same way", async () => {
    await driver.visit(`/?${failure}`);
    await expect.poll(() => driver.bannerText(), POLL_60).toContain(NEW_FAILURE_HEADING);
    expect(await driver.bannerText()).toContain("The provider refused the flow");
  });

  await test.step("the failure landings planted no session", async () => {
    expect(await sessionCookieNames(driver)).not.toContain("session-id");
  });
}

async function desk013Positive(driver: ParityDriver, seed: ParitySeedFacts): Promise<void> {
  await installDesktopMock(driver.page);
  for (const route of ENTRY_ROUTES) {
    await test.step(`route ${route} shows the fallback card, never a dead form`, async () => {
      await driver.visit(route);
      await expect.poll(() => driver.pageText(), POLL_60).toContain(NEW_UNAVAILABLE_HEADING);
      expect(await driver.deskSigninMarketingFrameVisible()).toBe(false);
      expect(await driver.seesGenericSignInHeader()).toBe(false);
      expect(await driver.bannerText()).toBeNull();
    });
  }

  await test.step("the fallback action opens the hosted app through the native opener", async () => {
    // The gate serves the desktop dev bundle with PIDASH_WEB_BASE_URL set
    // to the stack's oracle origin, so the card resolves there.
    const hosted = parityOracleUrl(process.env);
    await driver.visit("/");
    await expect.poll(() => driver.pageText(), POLL_60).toContain(NEW_UNAVAILABLE_ACTION);
    await driver.page.getByRole("button", { name: NEW_UNAVAILABLE_ACTION, exact: true }).click();
    await expect
      .poll(async () => (await desktopInvocations(driver.page, "plugin:opener|open_url")).length, POLL_60)
      .toBe(1);
    const opened = await desktopInvocations(driver.page, "plugin:opener|open_url");
    expect((opened[0]?.args as { url?: unknown } | undefined)?.url).toBe(hosted);
    // No in-page popup: the opener owns the navigation.
    expect(await driver.currentPath()).toBe("/");
  });

  await test.step("a planted session still lands in the workspace", async () => {
    const session = await signInSession(seed.email, seed.password);
    await driver.page.context().addCookies(sessionBrowserCookies(session));
    await driver.visit("/");
    await expect.poll(() => driver.currentPath(), POLL_60).toContain(seed.workspaceSlug);
    expect(await sessionValid(await signInSession(seed.email, seed.password))).toBe(true);
  });
}

async function desk014Positive(driver: ParityDriver, seed: ParitySeedFacts): Promise<void> {
  await installDesktopMock(driver.page);

  await test.step("unrecognized links are ignored", async () => {
    await driver.visit("/");
    await expect.poll(() => driver.pageText(), POLL_60).toContain(NEW_UNAVAILABLE_HEADING);
    await desktopEmit(driver.page, "deep-link://new-url", "not-a-link");
    await desktopEmit(driver.page, "deep-link://new-url", ["pidash://callback", "pidash://callback?code=&state="]);
    await driver.page.waitForTimeout(1_000);
    expect(await driver.currentPath()).toBe("/");
    expect(await driver.bannerText()).toBeNull();
    expect(await sessionCookieNames(driver)).not.toContain("session-id");
  });

  await test.step("provider refusals land on sign-in with their reason", async () => {
    // The exchange navigation itself is native; the web side of a
    // refusal is the DESK-012 failure landing.
    await driver.visit("/sign-in?error=access_denied&error_description=The+provider+refused+the+flow");
    await expect.poll(() => driver.bannerText(), POLL_60).toContain(NEW_FAILURE_HEADING);
  });

  await test.step("the community server still exposes no exchange route", async () => {
    expect((await serverDeskSigninDesktopExchangeStatus({})).status).toBe(404);
  });

  await test.step("the planted session carries no desktop marker", async () => {
    const session = await signInSession(seed.email, seed.password);
    expect(await serverDeskSigninSessionHasDesktopMarker(session)).toBe(false);
  });
}

async function desk015Positive(driver: ParityDriver, seed: ParitySeedFacts): Promise<void> {
  const session = await enterDesktopSignedIn(driver, seed.email, seed.password);
  // The first-run tour overlay covers the dashboard for fresh users, so clear it.
  await serverSetTourCompleted(session, true);

  await test.step("sign in and reach the app home", async () => {
    await driver.openWorkspaceHome(seed.workspaceSlug);
    await expect.poll(() => driver.sidebarLinkTexts(), POLL_60).toContain("Home");
  });

  await test.step("the native bridge is present", async () => {
    expect(await driver.desktopRuntimeIsTauriPresent()).toBe(true);
  });

  await test.step("the updates dialog exposes its externals as opener buttons", async () => {
    await driver.openHelpMenu();
    expect(await driver.activateHelpEntry("What's new?")).toBeNull();
    await expect.poll(() => driver.isDialogWithTextVisible("What's new?"), { timeout: 30_000 }).toBe(true);
    const links = await driver.deskSigninUpdateDialogLinks();
    expect(links.map((link) => link.label)).toEqual(["Docs", "Full changelog", "Support"]);
    for (const link of links) {
      expect(link.tag).toBe("button");
    }
  });

  await test.step("a web address opens through the native opener with no popup", async () => {
    expect(await driver.deskSigninOpenUpdateDialogLink("Docs")).toBeNull();
    await expect
      .poll(async () => (await desktopInvocations(driver.page, "plugin:opener|open_url")).length, POLL_60)
      .toBe(1);
    const opened = await desktopInvocations(driver.page, "plugin:opener|open_url");
    const url = String((opened[0]?.args as { url?: unknown } | undefined)?.url ?? "");
    expect(url).toMatch(/^https:/);
  });

  await test.step("a non-web address is refused with no opener call", async () => {
    expect(await driver.deskSigninOpenUpdateDialogLink("Support")).toBeNull();
    const opened = await desktopInvocations(driver.page, "plugin:opener|open_url");
    expect(opened).toHaveLength(1);
    expect(JSON.stringify(opened)).not.toContain("mailto:");
  });

  await test.step("the server session is untouched by the external navigation", async () => {
    expect(await sessionValid(await signInSession(seed.email, seed.password))).toBe(true);
  });
}

async function desk027Positive(driver: ParityDriver): Promise<void> {
  const email = uniqueEmail("desk-cli-new");
  const session = await test.step("a fresh account exists", async () => createAccountSession(email, FRESH_PASSWORD));
  const baseline = (await userProfile(session)).onboarding_step;
  await installDesktopMock(driver.page);
  await driver.page.context().addCookies(sessionBrowserCookies(session));

  await test.step("sign in and open onboarding", async () => {
    await driver.openOnboarding();
    await expect.poll(() => driver.pageText(), POLL_60).toContain("Create your profile");
  });

  await test.step("onboarding opens on profile setup with no CLI step", async () => {
    expect(await driver.pageText()).not.toContain("Install the Pi Dash CLI");
    const progress = parseProgress(await driver.deskSigninOnboardingProgress());
    expect(progress.current).toBe(1);
    expect(progress.total).toBeGreaterThanOrEqual(2);
    expect(await driver.isOnboardingBackVisible()).toBe(false);
  });

  await test.step("the visit stored nothing", async () => {
    expect((await userProfile(session)).onboarding_step).toEqual(baseline);
  });
}
