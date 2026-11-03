// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios: auth session, account, device, desktop seams, denials,
// keyboard (NEWFRONT-109). Rows AUTH-017–AUTH-025. Green on apps/web first
// (the oracle); the same file must go green on apps/web_new once the auth
// area lands.
//
// Recorded exceptions (see the hand-off comment; the issue pre-authorizes
// explicit exceptions for desktop-only steps that cannot run here):
// AUTH-022 and AUTH-023 have scenarios at the foot of this file
// (NEWFRONT-486), but they run against the desktop builds only, in the
// area gate: the system-browser login plus pidash://auth/callback return
// needs the desktop build, which cannot run in this checkout; the server
// has no desktop-exchange endpoint; and both the error landing
// (?error=/?error_description= alert above the card) and the unavailable
// card render only inside the desktop build. The web-observable provider
// pieces (provider buttons when the instance enables OAuth, error_code
// failure banners) belong to AUTH-003/AUTH-006, owned by a sibling, and
// the seeded stack configures no OAuth provider — so no seam for either
// row is testable on apps/web, and the gate runs these two scenarios
// old-first against the desktop build, then on apps/web_new.
// AUTH-017's desktop clause (the local webview store is wiped on sign-out)
// is likewise desktop-only and has no scenario here for the same reason.
import { test, expect } from "../fixtures";
import type { ParityDriver } from "../drivers/parity-driver";
import {
  approveDeviceCode,
  browserCookies,
  createAccountSession,
  createWorkspace,
  createWorkspaceViaApi,
  inviteWorkspaceMember,
  markOnboarded,
  serverIssueNames,
  sessionValid,
  setLastWorkspace,
  setOnboarded,
  signInSession,
  signInIssuesSessionWithoutCsrf,
  signUpFreshUser,
  startDeviceFlow,
  uniqueSlug,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

const POLL_60 = { timeout: 60_000 };

// The oracle runs from a dev server that compiles routes on demand; full
// sign-in flows take a minute or more per scenario even once warm.
test.setTimeout(480_000);

const THROWAWAY_PASSWORD = "Parity-Oracle-987!xQ";

function throwawayEmail(prefix: string): string {
  return `${prefix}-${Date.now()}@example.com`;
}

test(
  specTitle(["AUTH-017"], "sign out from the account menu ends the session"),
  { tag: specTags(["AUTH-017"]) },
  async ({ driver, seed }) => {
    await test.step("sign in through the UI", async () => {
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.openProjectIssues(seed.workspaceSlug, seed.projectId);
    });

    await test.step("sign out from the sidebar account menu", async () => {
      await driver.signOutViaAccountMenu();
    });

    await test.step("the signed-out entry shows again", async () => {
      expect(await driver.isSignedOut()).toBe(true);
    });

    await test.step("the server agrees the session is dead", async () => {
      // The old session cookie no longer opens an authenticated route: the
      // app bounces back to sign-in instead of rendering issues. The bounce
      // follows an async 401, so poll until the card shows.
      await driver.openProjectIssues(seed.workspaceSlug, seed.projectId);
      await expect.poll(() => driver.isSignedOut(), { timeout: 60_000 }).toBe(true);
    });
  }
);

test(
  specTitle(["AUTH-017"], "sign out from the command palette ends the session"),
  { tag: specTags(["AUTH-017"]) },
  async ({ driver, seed }) => {
    await test.step("sign in through the UI", async () => {
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.openProjectIssues(seed.workspaceSlug, seed.projectId);
    });

    await test.step("sign out from the command palette", async () => {
      await driver.signOutViaCommandPalette();
    });

    await test.step("the signed-out entry shows again", async () => {
      expect(await driver.isSignedOut()).toBe(true);
    });

    await test.step("the server agrees the session is dead", async () => {
      await driver.openProjectIssues(seed.workspaceSlug, seed.projectId);
      await expect.poll(() => driver.isSignedOut(), { timeout: 60_000 }).toBe(true);
    });
  }
);

test(
  specTitle(["AUTH-018"], "account switching routes through sign-out into a fresh login"),
  {
    tag: specTags(["AUTH-018"]),
  },
  async ({ driver }) => {
    const email = throwawayEmail("parity-switch");

    await test.step("a second account exists (seed owner untouched)", async () => {
      await createAccountSession(email, THROWAWAY_PASSWORD);
    });

    await test.step("sign in as the second account; onboarding names it", async () => {
      await driver.openEntry();
      await driver.signInWithPassword(email, THROWAWAY_PASSWORD);
      await driver.openSwitchAccount();
    });

    await test.step("the dialog names the active account", async () => {
      expect(await driver.switchAccountEmail()).toBe(email);
    });

    await test.step("confirming routes through sign-out into a fresh login", async () => {
      await driver.confirmSwitchAccount();
      expect(await driver.isSignedOut()).toBe(true);
    });
  }
);

test(
  specTitle(["AUTH-019"], "deactivating the account signs out with confirmation"),
  {
    tag: specTags(["AUTH-019"]),
  },
  async ({ driver, seed }) => {
    const email = throwawayEmail("parity-deactivate");

    const workspaceSlug = await test.step("a throwaway account exists (never the seed owner)", async () => {
      const session = await createAccountSession(email, THROWAWAY_PASSWORD);
      await setOnboarded(session);
      // An onboarded user with no workspace lands on workspace creation,
      // not the app shell — give the throwaway a workspace so Settings (and
      // deactivation) is reachable.
      return createWorkspace(session, "Throwaway Workspace", `throwaway-ws-${Date.now()}`);
    });

    await test.step("sign in as the throwaway and open deactivation", async () => {
      await driver.openEntry();
      await driver.signInWithPassword(email, THROWAWAY_PASSWORD);
      await driver.visit(`/${workspaceSlug}`);
      await driver.openDeactivateAccount();
    });

    await test.step("confirming signs out back at the signed-out entry", async () => {
      await driver.confirmDeactivation();
      expect(await driver.isSignedOut()).toBe(true);
    });

    await test.step("the server agrees the account is gone", async () => {
      await expect(signInSession(email, THROWAWAY_PASSWORD)).rejects.toThrow();
      // The seed owner still signs in: deactivation touched only the throwaway.
      const ownerSession = await signInSession(seed.email, seed.password);
      expect(await sessionValid(ownerSession)).toBe(true);
    });
  }
);

test(
  specTitle(["AUTH-020"], "an expired session returns to sign-in preserving the return path"),
  {
    tag: specTags(["AUTH-020"]),
  },
  async ({ driver, seed }) => {
    await test.step("sign in through the UI", async () => {
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.openProjectIssues(seed.workspaceSlug, seed.projectId);
    });

    await test.step("the local session expires", async () => {
      await driver.dropSession();
      await driver.openProjectIssues(seed.workspaceSlug, seed.projectId);
    });

    await test.step("the app lands on sign-in and keeps the return path", async () => {
      await expect.poll(() => driver.isSignedOut(), { timeout: 60_000 }).toBe(true);
      expect(await driver.currentPath()).toContain("next_path=");
    });
  }
);

test(
  specTitle(["AUTH-020"], "login submissions require a fresh forgery token"),
  {
    tag: specTags(["AUTH-020"]),
  },
  async ({ driver, seed }) => {
    await test.step("a token-less credential POST issues no session", async () => {
      expect(await signInIssuesSessionWithoutCsrf(seed.email, seed.password)).toBe(false);
    });

    await test.step("the UI flow (which supplies the token) still authenticates", async () => {
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.openProjectIssues(seed.workspaceSlug, seed.projectId);
      // The list populates asynchronously after navigation; poll the
      // user-visible read until every seeded title shows up.
      await expect
        .poll(() => driver.visibleIssueNames(), { timeout: 120_000 })
        .toEqual(expect.arrayContaining([...seed.issueNames]));
    });
  }
);

test(
  specTitle(["AUTH-021"], "the device code auto-formats as typed"),
  {
    tag: specTags(["AUTH-021"]),
  },
  async ({ driver, seed }) => {
    await test.step("sign in and open the device-approval page", async () => {
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.visit("/auth/device");
    });

    await test.step("typing a raw code formats it in dashed groups", async () => {
      await driver.typeDeviceCode("abcd1234");
      expect(await driver.deviceCodeFieldValue()).toBe("ABCD-1234");
    });
  }
);

test(
  specTitle(["AUTH-021"], "a deep-linked device code submits and failures allow retry"),
  {
    tag: specTags(["AUTH-021"]),
  },
  async ({ driver, seed }) => {
    await test.step("sign in through the UI", async () => {
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
    });

    await test.step("follow a CLI deep link with a bogus code", async () => {
      await driver.visit("/auth/device?code=ZZZZ-YYYY");
    });

    await test.step("the auto-attempt fails with an explanation, staying editable", async () => {
      // Bogus codes surface the server's "not recognized" detail; the
      // generic "Could not approve" fallback only shows when the server
      // returns no error detail.
      await expect.poll(() => driver.showsText("Code not recognized"), POLL_60).toBe(true);
      await driver.typeDeviceCode("abcd1234");
      expect(await driver.deviceCodeFieldValue()).toBe("ABCD-1234");
      await driver.submitDeviceApproval();
      await expect.poll(() => driver.showsText("Code not recognized"), POLL_60).toBe(true);
    });

    await test.step("the server approved nothing", async () => {
      const session = await signInSession(seed.email, seed.password);
      await expect(approveDeviceCode(session, "ZZZZYYYY")).rejects.toThrow();
    });
  }
);

test(
  specTitle(["AUTH-021"], "approving a live device code names the account and workspace"),
  {
    tag: specTags(["AUTH-021"]),
  },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const code = await test.step("start a CLI device flow", async () => startDeviceFlow(session));

    await test.step("approve the live code through the UI", async () => {
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.visit("/auth/device");
      await driver.typeDeviceCode(code.replace("-", ""));
      await driver.submitDeviceApproval();
    });

    await test.step("the confirmation names the account and workspace", async () => {
      await expect.poll(() => driver.showsText(seed.email), POLL_60).toBe(true);
      // The server resolves the approval workspace from the approver's
      // memberships, so read the expected slug back instead of assuming
      // the seed workspace: other specs (AUTH-026) add workspaces to the
      // seed owner, and same-user re-approval is idempotent by design
      // (the approve view only flips `consumed` when the CLI polls for
      // its token).
      const recorded = await approveDeviceCode(session, code);
      expect(recorded.email).toBe(seed.email);
      expect(recorded.workspace).not.toBeNull();
      await expect.poll(() => driver.showsText(recorded.workspace ?? ""), POLL_60).toBe(true);
    });

    await test.step("the server recorded the approval", async () => {
      // A different user must be rejected, proving the code is approved
      // as the seed owner.
      const other = await createAccountSession(throwawayEmail("parity-device-other"), THROWAWAY_PASSWORD);
      await expect(approveDeviceCode(other, code)).rejects.toThrow();
    });
  }
);

test(
  specTitle(["AUTH-024"], "unknown targets report not-found"),
  { tag: specTags(["AUTH-024"]) },
  async ({ driver, seed }) => {
    await test.step("sign in through the UI", async () => {
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
    });

    await test.step("open a project that does not exist", async () => {
      await driver.visit(`/${seed.workspaceSlug}/projects/00000000-0000-0000-0000-000000000000/issues`);
    });

    await test.step("the app reports not-found instead of a blank page", async () => {
      await expect.poll(() => driver.showsText("not found"), POLL_60).toBe(true);
    });
  }
);

test(
  specTitle(
    ["AUTH-024"],
    "bug: NEWFRONT-157 invited workspace outsiders see 'not found' instead of the not-a-member screen"
  ),
  {
    tag: specTags(["AUTH-024"]),
  },
  async ({ driver, seed }) => {
    const ownerSession = await signInSession(seed.email, seed.password);
    const foreignSlug = await test.step("a workspace the seed user was invited to but never joined", async () => {
      const outsider = await createAccountSession(throwawayEmail("parity-outsider"), THROWAWAY_PASSWORD);
      const slug = await createWorkspace(outsider, "Outsider Workspace", `outsider-ws-${Date.now()}`);
      // The seed owner is invited but never joins: even so, the app
      // reports "Workspace not found" (NEWFRONT-157).
      await inviteWorkspaceMember(slug, outsider, seed.email, 15);
      return slug;
    });

    await test.step("sign in as the seed owner and open the foreign workspace", async () => {
      expect(await sessionValid(ownerSession)).toBe(true);
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.visit(`/${foreignSlug}`);
    });

    await test.step("bug: the app reports not-found despite the pending invitation", async () => {
      // Current behavior (NEWFRONT-157): even with a pending invitation,
      // outsiders land on the generic not-found screen. Intended: the
      // not-a-member screen pointing at invitations or creation.
      await expect.poll(() => driver.showsText("Workspace not found"), POLL_60).toBe(true);
      await expect.poll(() => driver.showsText("Go Home"), POLL_60).toBe(true);
    });
  }
);

test(
  specTitle(["AUTH-025"], "keyboard-only sign-in keeps visible focus and submits with Enter"),
  { tag: specTags(["AUTH-025"]) },
  async ({ driver, seed }) => {
    await test.step("open the entry: the email step is focused", async () => {
      await driver.openEntry();
      await expect.poll(() => driver.focusedControlName(), POLL_60).toBe("name@company.com");
    });

    await test.step("type the address and submit with Enter only", async () => {
      await driver.typeText(seed.email);
      await driver.pressKey("Enter");
      await expect.poll(() => driver.focusedControlName(), POLL_60).toBe("Enter password");
    });

    await test.step("type the secret and submit with Enter only", async () => {
      await driver.typeText(seed.password);
      await driver.pressKey("Enter");
      // The native credential POST ends in a full page load (302 to "/")
      // followed by the app's own redirect to the last workspace. The
      // entry placeholder vanishes mid-flight, so a signed-out poll
      // passes too early and a goto races the redirect and gets
      // interrupted — wait for the workspace landing instead.
      await expect.poll(() => driver.currentPath(), POLL_60).toContain(seed.workspaceSlug);
      await driver.openProjectIssues(seed.workspaceSlug, seed.projectId);
    });

    await test.step("the screen and the server agree", async () => {
      await expect
        .poll(() => driver.visibleIssueNames(), { timeout: 120_000 })
        .toEqual(expect.arrayContaining([...seed.issueNames]));
      const session = await signInSession(seed.email, seed.password);
      const server = await serverIssueNames(seed.workspaceSlug, seed.projectId, session);
      expect(new Set(server)).toEqual(new Set(seed.issueNames));
    });
  }
);

/** Session cookie names a signed-out desktop visit must never plant. */
async function desktopSessionCookieNames(driver: ParityDriver): Promise<string[]> {
  const cookies = await driver.page.context().cookies();
  return cookies.map((cookie) => cookie.name);
}

test(
  specTitle(["AUTH-022"], "desktop sign-in never starts on its own and failures land back with an error"),
  { tag: specTags(["AUTH-022"]) },
  async ({ driver }) => {
    await test.step("a plain visit plants no session and navigates nowhere", async () => {
      await driver.visit("/");
      expect(await driver.currentPath()).toBe("/");
      expect(await driver.authDesktopUnavailableCardVisible()).toBe(true);
      expect(await desktopSessionCookieNames(driver)).not.toContain("session-id");
    });

    await test.step("a provider failure lands back with its description", async () => {
      await driver.visit("/sign-in?error=access_denied&error_description=The%20provider%20refused%20the%20flow.");
      await expect
        .poll(() => driver.authDesktopFailureAlertText(), POLL_60)
        .toContain("The provider refused the flow.");
    });

    await test.step("an aborted flow without a description names the error", async () => {
      await driver.visit("/login?error=access_denied");
      await expect.poll(() => driver.authDesktopFailureAlertText(), POLL_60).toContain("access_denied");
    });
  }
);

test(
  specTitle(["AUTH-023"], "the desktop entry explains the missing hand-off and offers the browser instead"),
  { tag: specTags(["AUTH-023"]) },
  async ({ driver }) => {
    for (const entry of ["/", "/login", "/sign-in"]) {
      await test.step(`a signed-out visit to ${entry} shows the unavailable card instead of a form`, async () => {
        await driver.visit(entry);
        expect(await driver.authDesktopUnavailableCardVisible()).toBe(true);
        expect(await driver.authStep()).toBe("unknown");
        expect(await driver.authDesktopBrowserActionLabel()).not.toBeNull();
      });
    }

    await test.step("the browser action leaves the desktop entry", async () => {
      await driver.visit("/");
      const before = driver.page.url();
      await driver.authDesktopClickBrowserAction();
      await expect.poll(() => driver.page.url(), POLL_60).not.toBe(before);
    });

    await test.step("signed-in visitors are forwarded to their workspace", async () => {
      const user = await signUpFreshUser("parity-desk");
      const slug = uniqueSlug("deskws");
      const workspace = await createWorkspaceViaApi(user, { name: "Desktop", slug });
      await setLastWorkspace(user, workspace.id);
      await markOnboarded(user);
      await driver.openAuthenticated("/", browserCookies(user));
      await expect.poll(() => driver.currentPath(), POLL_60).toContain(`/${slug}`);
    });

    await test.step("the forward honors a pending destination", async () => {
      const user = await signUpFreshUser("parity-desk");
      const homeSlug = uniqueSlug("deskhome");
      const awaySlug = uniqueSlug("deskaway");
      const home = await createWorkspaceViaApi(user, { name: "Desktop home", slug: homeSlug });
      await createWorkspaceViaApi(user, { name: "Desktop away", slug: awaySlug });
      await setLastWorkspace(user, home.id);
      await markOnboarded(user);
      // The pending destination differs from the last workspace on
      // purpose: landing on the last workspace must fail this step.
      await driver.openAuthenticated(`/?next_path=/${awaySlug}`, browserCookies(user));
      await expect.poll(() => driver.currentPath(), POLL_60).toContain(`/${awaySlug}`);
    });
  }
);
