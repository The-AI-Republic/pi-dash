// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-446): instance-admin cloud entry gate.
// Rows ADM-074 (cloud OIDC entry, no password/setup forms), ADM-075
// (authorized-admin spinner), ADM-076 (OIDC bounce with deep-link
// return_to), ADM-077 (terminal not-authorized screen), ADM-078
// (retryable error screen) and ADM-079 (inconclusive probe resolves to
// retry). Green on the cloud-edition old admin console (the
// overlay-swapped admin oracle), never on OSS behavior.
//
// Observed behavior notes (paraphrased; the inventory rows carry them
// at update time): the cloud root never renders a password field or a
// first-run setup form — entry is an authorization decision over OIDC
// only, and the OSS password endpoints answer 403 in the cloud
// edition. An allowlisted admin sees only a spinner while the shared
// home layout redirects to general settings. Every other visitor,
// however, lands on the retryable error screen and retrying loops on
// it: anonymous visits issue zero OIDC handoffs and signed-in
// non-admins never see the denied screen, because the gate classifies
// the admin check through a status read that never matches (bug:
// NEWFRONT-496) — so the auth probe never fires and its bounce/deny
// branches are dead. A genuinely transient admin check does surface
// the retry screen, and retrying re-runs the check: recovery lands an
// admin on general settings with no deny and no bounce.
//
// The web_new leg asserts the intended classification instead (the
// gate builds it, per NEWFRONT-496): a session-less 401 bounces
// through the provider handoff carrying the console path back as a
// relative return_to while replacing the history entry, a 403 probes
// the session once and either denies terminally or bounces, a failed
// check or an inconclusive probe retries, and a retry resets both
// verdicts. The oracle leg keeps the observed old behavior above; the
// `bug:` titles name the old gate's loop, which stays true of the
// oracle.
import { test, expect } from "../fixtures";
import {
  cloudBrowserCookies,
  cloudDevLogin,
  serverAdminCloudGrantAdmin,
  serverAdminMe,
  serverCloudAdminPasswordShadowed,
  serverCloudAuthMe,
  uniqueEmail,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import type { ParityDriver } from "../drivers/parity-driver";

/** Local sleep for mid-delay spinner sampling and retry settling. */
function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

/** Fresh dev-login identity promoted to instance admin (per-scenario fixture). */
async function adminFixture(prefix: string): Promise<{ cookie: string; email: string }> {
  const email = uniqueEmail(prefix);
  const session = await cloudDevLogin(email);
  expect(await serverAdminCloudGrantAdmin(email)).toBe(true);
  return { cookie: session.cookie, email };
}

test(
  specTitle(["ADM-074"], "cloud entry never shows a password or setup form and password endpoints stay shadowed"),
  { tag: specTags(["ADM-074"]) },
  async ({ driver }: { driver: ParityDriver }) => {
    // The settled screens differ by target: the old console lands everyone
    // it cannot classify on the retry screen, while web_new hands a
    // session-less visitor to the provider (ADM-076) and a signed-in
    // non-admin to the terminal denied screen (ADM-077). Neither renders a
    // credential form, which is what this row pins.
    await driver.cloudArmSignInCapture();
    await driver.adminCloudOpenRootWithCookies([]);
    if (driver.target === "web_new") {
      await driver.cloudAwaitSignInRequest(0);
    } else {
      await driver.adminCloudWaitForHomeBranch("error");
    }
    let facts = await driver.adminCloudHomeFacts();
    expect(facts.passwordFields).toBe(0);
    expect(facts.setupFields).toBe(0);

    const user = await cloudDevLogin(uniqueEmail("parity-446-gate"));
    await driver.adminCloudOpenRootWithCookies(cloudBrowserCookies(user.cookie));
    await driver.adminCloudWaitForHomeBranch(driver.target === "web_new" ? "denied" : "error");
    facts = await driver.adminCloudHomeFacts();
    expect(facts.passwordFields).toBe(0);
    expect(facts.setupFields).toBe(0);

    // Server half: OIDC is the only way in — the OSS password
    // endpoints stay shadowed off in the cloud edition.
    expect(await serverCloudAdminPasswordShadowed()).toEqual({ signIn: 403, signUp: 403 });
  }
);

test(
  specTitle(["ADM-075"], "authorized admin sees the spinner then lands on general settings"),
  { tag: specTags(["ADM-075"]) },
  async ({ driver }: { driver: ParityDriver }) => {
    const { cookie, email } = await adminFixture("parity-446-gate-admin");
    expect((await serverAdminMe(cookie)).email).toBe(email);
    // Hold the admin check open so the spinner phase reads deterministically.
    await driver.adminCloudDelayAdminCheck(2000);
    await driver.adminCloudOpenRootWithCookies(cloudBrowserCookies(cookie));
    await sleep(700);
    const mid = await driver.adminCloudHomeFacts();
    expect(mid.branch).toBe("spinner");
    expect(mid.heading).toBe(null);
    expect(mid.passwordFields).toBe(0);
    expect(mid.setupFields).toBe(0);
    await expect.poll(async () => driver.adminCurrentPath(), { timeout: 30_000 }).toBe("/god-mode/general/");
    // The full dashboard serves past the cloud gate.
    expect((await driver.adminSidebarFacts()).entries.length).toBeGreaterThan(0);
  }
);

test(
  specTitle(["ADM-076"], "bug: NEWFRONT-496 anonymous visitor gets the retry screen, never the OIDC bounce"),
  { tag: specTags(["ADM-076"]) },
  async ({ driver }: { driver: ParityDriver }) => {
    // Intended: bounce through OIDC with a relative deep-link-preserving
    // return_to, replacing history. Observed: the retry screen loops.
    await driver.cloudArmSignInCapture();
    await driver.adminCloudStartGateFetchSpy();
    await driver.adminCloudOpenRootWithCookies([]);
    if (driver.target === "web_new") {
      // Intended (NEWFRONT-496): the anonymous 401 is already proof of
      // no session, so the gate replaces the history entry with the
      // provider handoff — carrying the console path as a relative
      // return_to — and reads nothing further.
      const handoff = await driver.cloudAwaitSignInRequest(0);
      expect(new URL(handoff).pathname).toBe("/api/auth/sign-in/");
      expect(new URL(handoff).searchParams.get("return_to")).toBe("/god-mode/");
      const counts = await driver.adminCloudGateFetchCounts();
      expect(counts.adminCheck).toBe(1);
      expect(counts.probe).toBe(0);
      // The handoff replaced the gate's entry: going back leaves the
      // console behind rather than re-entering it (a push would land
      // the visitor back on /god-mode/ and start the loop again).
      const backAfterHandoff = await driver.adminCloudGoBack();
      expect(new URL(backAfterHandoff).pathname).not.toBe("/god-mode/");
    } else {
      await driver.adminCloudWaitForHomeBranch("error");
      const facts = await driver.adminCloudHomeFacts();
      expect(facts.heading).toBe("Something went wrong");
      expect(facts.retryVisible).toBe(true);
      const counts = await driver.adminCloudGateFetchCounts();
      expect(counts.adminCheck).toBeGreaterThanOrEqual(1);
      expect(counts.probe).toBe(0);
      expect(await driver.cloudSignInRequests()).toEqual([]);
      // Retry re-runs the admin check and stays put: no bounce on the loop.
      await driver.adminCloudRetry();
      await sleep(1500);
      expect((await driver.adminCloudHomeFacts()).branch).toBe("error");
      expect((await driver.adminCloudGateFetchCounts()).adminCheck).toBeGreaterThan(counts.adminCheck);
      expect(await driver.cloudSignInRequests()).toEqual([]);
    }
    // Server half: no session (the case the bounce was designed for)
    // and the admin check rejects the anonymous read.
    expect((await serverCloudAuthMe("")).status).toBe(401);
    expect((await serverAdminMe("")).status).toBe(401);
  }
);

test(
  specTitle(["ADM-077"], "bug: NEWFRONT-496 signed-in non-admin gets the retry screen, never the denied screen"),
  { tag: specTags(["ADM-077"]) },
  async ({ driver }: { driver: ParityDriver }) => {
    // Intended: a terminal not-authorized screen with a root link, no
    // redirect, no bounce. Observed: the retry screen loops.
    const email = uniqueEmail("parity-446-gate-user");
    const session = await cloudDevLogin(email);
    expect((await serverCloudAuthMe(session.cookie)).status).toBe(200);
    expect((await serverAdminMe(session.cookie)).status).toBe(403);
    await driver.cloudArmSignInCapture();
    await driver.adminCloudStartGateFetchSpy();
    await driver.adminCloudOpenRootWithCookies(cloudBrowserCookies(session.cookie));
    if (driver.target === "web_new") {
      // Intended (NEWFRONT-496): the 403 is classified by exactly one
      // session probe — authenticated, so the screen is terminal: no
      // redirect, no bounce, no retry button, one root link.
      await driver.adminCloudWaitForHomeBranch("denied");
      const facts = await driver.adminCloudHomeFacts();
      expect(facts.heading).toBe("Access denied");
      expect(facts.deniedHref).toBe("/");
      expect(facts.retryVisible).toBe(false);
      expect((await driver.adminCloudGateFetchCounts()).probe).toBe(1);
      expect(await driver.page.getByRole("button").count()).toBe(0);
      expect(await driver.cloudSignInRequests()).toEqual([]);
      // Terminal means terminal: the gate stays on its own address
      // instead of navigating the visitor anywhere.
      expect(await driver.adminCurrentPath()).toBe("/god-mode/");
    } else {
      await driver.adminCloudWaitForHomeBranch("error");
      const facts = await driver.adminCloudHomeFacts();
      expect(facts.heading).toBe("Something went wrong");
      expect(facts.retryVisible).toBe(true);
      expect(facts.deniedHref).toBe(null);
      expect((await driver.adminCloudGateFetchCounts()).probe).toBe(0);
      expect(await driver.cloudSignInRequests()).toEqual([]);
      await driver.adminCloudRetry();
      await sleep(1500);
      expect((await driver.adminCloudHomeFacts()).branch).toBe("error");
      expect(await driver.cloudSignInRequests()).toEqual([]);
    }
  }
);

test(
  specTitle(["ADM-078"], "transient admin-check failure surfaces the retry screen and retry recovers"),
  { tag: specTags(["ADM-078"]) },
  async ({ driver }: { driver: ParityDriver }) => {
    const { cookie } = await adminFixture("parity-446-gate-flaky");
    await driver.cloudArmSignInCapture();
    await driver.adminCloudStartGateFetchSpy();
    if (driver.target === "web_new") {
      // Hold the check failing rather than handing out one bad answer:
      // the entry mounts under React strict mode, and the guard plus the
      // resolver both read the same query, so a single-shot fault is too
      // easy to consume before the screen can settle. The rule under
      // test is that a transient failure reads as retry, and that the
      // screen stays put until the visitor retries — the outage lasts
      // until the retry below.
      await driver.adminCloudFailAdminCheck(Number.POSITIVE_INFINITY, 500);
      await driver.adminCloudOpenRootWithCookies(cloudBrowserCookies(cookie));
      await driver.adminCloudWaitForHomeBranch("error");
      expect((await driver.adminCloudHomeFacts()).heading).toBe("Something went wrong");
      expect((await driver.adminCloudGateFetchCounts()).probe).toBe(0);
      expect(await driver.cloudSignInRequests()).toEqual([]);
      // Retry re-runs the admin check: with the outage over, success
      // lands the admin — never a deny, never a bounce.
      await driver.adminCloudReleaseAdminCheckFault();
      await driver.adminCloudRetry();
      await expect.poll(async () => driver.adminCurrentPath(), { timeout: 30_000 }).toBe("/god-mode/general/");
      expect(await driver.cloudSignInRequests()).toEqual([]);
    } else {
      await driver.adminCloudFailAdminCheck(1, 500);
      await driver.adminCloudOpenRootWithCookies(cloudBrowserCookies(cookie));
      await driver.adminCloudWaitForHomeBranch("error");
      expect((await driver.adminCloudHomeFacts()).heading).toBe("Something went wrong");
      expect((await driver.adminCloudGateFetchCounts()).adminCheck).toBe(1);
      // Retry re-runs the admin check: success lands the admin on
      // general settings — never a deny, never a bounce.
      await driver.adminCloudRetry();
      await expect.poll(async () => driver.adminCurrentPath(), { timeout: 30_000 }).toBe("/god-mode/general/");
      expect((await driver.adminCloudGateFetchCounts()).adminCheck).toBe(2);
      expect(await driver.cloudSignInRequests()).toEqual([]);
    }
    await driver.adminCloudReleaseAdminCheckFault();
    expect(await driver.cloudSignInRequests()).toEqual([]);
  }
);

test(
  specTitle(
    ["ADM-079"],
    "bug: NEWFRONT-496 a probe blip reaches the retry screen unprobed (intended: probed, retried)"
  ),
  { tag: specTags(["ADM-079"]) },
  async ({ driver }: { driver: ParityDriver }) => {
    // A signed-in non-admin reaches the probe (the check answers 403),
    // then the probe blips. Guessing "anonymous" here would hand a
    // returning visitor to OIDC and loop right back, so the gate must
    // land on the retry screen — and stay there until retried. The old
    // console reaches that screen too, but without ever probing.
    const session = await cloudDevLogin(uniqueEmail("parity-446-gate-probe-blip"));
    expect((await serverAdminMe(session.cookie)).status).toBe(403);
    await driver.cloudArmSignInCapture();
    await driver.adminCloudStartGateFetchSpy();
    // Hold the probe failing rather than handing out one bad answer:
    // the entry mounts under React strict mode, so a single-shot fault
    // can be consumed before the screen settles. The old console never
    // reaches the probe, so the held fault simply goes unread there.
    await driver.adminCloudFailAuthProbe(Number.POSITIVE_INFINITY, 500);
    await driver.adminCloudOpenRootWithCookies(cloudBrowserCookies(session.cookie));
    await driver.adminCloudWaitForHomeBranch("error");
    const facts = await driver.adminCloudHomeFacts();
    expect(facts.heading).toBe("Something went wrong");
    expect(facts.retryVisible).toBe(true);
    expect(facts.deniedHref).toBe(null);
    const counts = await driver.adminCloudGateFetchCounts();
    if (driver.target === "web_new") {
      // Intended (NEWFRONT-496): the genuine 403 is classified, and the
      // inconclusive probe resolves to retry — never to a bounce.
      expect(counts.probe).toBeGreaterThanOrEqual(1);
      expect(await driver.cloudSignInRequests()).toEqual([]);
      // Retry re-probes (the check still answers 403); with the blip
      // over the recovered session reads authenticated and the gate
      // settles on the terminal denied screen — the branch a guessed
      // anonymous would have skipped.
      await driver.adminCloudReleaseAuthProbeFault();
      await driver.adminCloudRetry();
      await driver.adminCloudWaitForHomeBranch("denied");
      expect((await driver.adminCloudHomeFacts()).heading).toBe("Access denied");
      expect((await driver.adminCloudGateFetchCounts()).probe).toBeGreaterThan(counts.probe);
    }
    await driver.adminCloudReleaseAuthProbeFault();
    // Neither target ever bounces on an inconclusive probe.
    expect(await driver.cloudSignInRequests()).toEqual([]);
  }
);

test(
  specTitle(["ADM-079"], "bug: NEWFRONT-496 the auth probe never fires, so failures skip classification"),
  { tag: specTags(["ADM-079"]) },
  async ({ driver }: { driver: ParityDriver }) => {
    // Intended: the probe classifies genuine 403s (no session bounces,
    // session denies) and an inconclusive probe resolves to retry,
    // never to a guessed anonymous. Observed: the probe never runs —
    // failures reach the retry screen unclassified.
    await driver.adminCloudStartGateFetchSpy();
    const user = await cloudDevLogin(uniqueEmail("parity-446-gate-probe"));
    const admin = await adminFixture("parity-446-gate-probe-admin");
    if (driver.target === "web_new") {
      // Intended (NEWFRONT-496): each verdict reaches its own branch —
      // an anonymous 401 bounces without probing, a 403 probes once and
      // denies an authenticated visitor, and an admin never probes.
      await driver.cloudArmSignInCapture();
      await driver.adminCloudOpenRootWithCookies([]);
      await driver.cloudAwaitSignInRequest(0);
      let counts = await driver.adminCloudGateFetchCounts();
      expect(counts.adminCheck).toBe(1);
      expect(counts.probe).toBe(0);
      await driver.adminCloudOpenRootWithCookies(cloudBrowserCookies(user.cookie));
      await driver.adminCloudWaitForHomeBranch("denied");
      counts = await driver.adminCloudGateFetchCounts();
      expect(counts.adminCheck).toBe(2);
      expect(counts.probe).toBe(1);
      await driver.adminCloudOpenRootWithCookies(cloudBrowserCookies(admin.cookie));
      await expect.poll(async () => driver.adminCurrentPath(), { timeout: 30_000 }).toBe("/god-mode/general/");
      counts = await driver.adminCloudGateFetchCounts();
      expect(counts.adminCheck).toBe(3);
      expect(counts.probe).toBe(1);
      return;
    }
    await driver.adminCloudOpenRootWithCookies([]);
    await driver.adminCloudWaitForHomeBranch("error");
    await driver.adminCloudOpenRootWithCookies(cloudBrowserCookies(user.cookie));
    await driver.adminCloudWaitForHomeBranch("error");
    await driver.adminCloudOpenRootWithCookies(cloudBrowserCookies(admin.cookie));
    await expect.poll(async () => driver.adminCurrentPath(), { timeout: 30_000 }).toBe("/god-mode/general/");
    const counts = await driver.adminCloudGateFetchCounts();
    expect(counts.probe).toBe(0);
    expect(counts.adminCheck).toBeGreaterThanOrEqual(3);
  }
);
