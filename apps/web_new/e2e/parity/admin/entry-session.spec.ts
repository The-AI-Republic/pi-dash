// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-441): instance-admin entry gate, first-run
// setup, and admin sign-in/session. Rows ADM-001 (the root gate resolves
// to loader, failure, setup or sign-in from one instance-info fetch),
// ADM-002 (brand loader while resolving, never beside a form or shell),
// ADM-003 (fetch-failure screen with manual retry and no auto-retry),
// ADM-004 (first-run admin creation form), ADM-005 (setup validation),
// ADM-006 (setup URL prefill plus server-error surfacing), ADM-007
// (admin sign-in form), ADM-008 (sign-in error banners), ADM-009 (route
// guards with replace navigation), ADM-010 (sign-out with client-state
// reset), ADM-011 (404 catch-all) and ADM-012 (first-workspace popup).
//
// Observed behavior notes (paraphrased; the inventory rows carry them at
// update time): the gate issues a single info read and branches on the
// setup flag once it lands; a spinner covers only the unresolved window;
// a failed read renders an explanatory screen whose retry is a full page
// reload, with no silent refetch; setup and sign-in are native full-page
// posts carrying a hidden token, so every round-trip is a real document
// navigation ending on the dashboard or back at the entry with the
// outcome encoded in the address; setup echoes its inputs back through
// the address after a rejected post, while sign-in echoes the address
// typed; the guards bounce by session state without adding history; the
// sign-out both ends the server session and resets local client state;
// unknown paths render a not-found screen with a way back; and the first
// info load that reports no workspaces raises a corner card inviting the
// admin to create one, dismissed on demand.
import { test, expect } from "../fixtures";
import {
  requireMentionMember,
  serverAdminClearForSetup,
  serverAdminInstanceInfo,
  serverAdminListAdmins,
  serverAdminMe,
  serverAdminRestoreAfterSetup,
  serverAdminSession,
  serverAdminSetGrant,
  serverAdminSetSetupDone,
  serverAdminSignOut,
  signInSession,
  uniqueEmail,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import type { ParityDriver, ParitySeedFacts } from "../drivers/parity-driver";

/** Strong under both the client rule and the server check (verified live). */
const STRONG_PASSWORD = "Parity-Setup-1!";

/** Too short for the client rule, so the gate stays shut. */
const WEAK_PASSWORD = "short";

/** Mismatching confirmation, shorter than the password it trails. */
const SHORT_CONFIRM = "Parity-Se";

/** Local sleep for proving an absence (no auto-retry) over a window. */
function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

/** Run one callback with the setup flag off, restoring the previous value after. */
async function withSetupUndone<T>(run: () => Promise<T>): Promise<T> {
  const wasDone = await serverAdminSetSetupDone(false);
  try {
    return await run();
  } finally {
    await serverAdminSetSetupDone(wasDone);
  }
}

test(
  specTitle(["ADM-001"], "set-up instance resolves to sign-in from a single info fetch"),
  { tag: specTags(["ADM-001"]) },
  async ({ driver }) => {
    const info = await serverAdminInstanceInfo();
    expect(info.isSetupDone).toBe(true);

    await driver.adminStartInstanceFetchSpy();
    await driver.adminOpenRoot();
    await driver.adminWaitForRootBranch("sign-in");
    expect(await driver.adminInstanceFetchCount()).toBe(1);

    const facts = await driver.adminSignInFacts();
    expect(facts.submitDisabled).toBe(true);
    expect(await driver.adminCurrentPath()).toBe("/god-mode/");
  }
);

test(
  specTitle(["ADM-001"], "un-set-up instance resolves to the setup form from a single info fetch"),
  { tag: specTags(["ADM-001"]) },
  async ({ driver }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    await withSetupUndone(async () => {
      expect((await serverAdminInstanceInfo()).isSetupDone).toBe(false);

      await driver.adminStartInstanceFetchSpy();
      await driver.adminOpenRoot();
      await driver.adminWaitForRootBranch("setup");
      expect(await driver.adminInstanceFetchCount()).toBe(1);

      const facts = await driver.adminSetupFacts();
      expect(facts.submitDisabled).toBe(true);
      expect(await driver.adminCurrentPath()).toBe("/god-mode/");
    });
    expect((await serverAdminInstanceInfo()).isSetupDone).toBe(true);
  }
);

test(
  specTitle(["ADM-002"], "brand loader covers only the unresolved window"),
  { tag: specTags(["ADM-002"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    await driver.adminStartInstanceFetchSpy();
    await driver.adminDelayInstanceFetch(3000);
    try {
      await driver.adminOpenRoot();
      // The loader appears with no form beside it while the read is held.
      await expect
        .poll(
          async () => {
            const facts = await driver.adminLoaderFacts();
            return facts.visible && !facts.formVisible ? "loader-only" : "waiting";
          },
          { timeout: 15_000 }
        )
        .toBe("loader-only");
    } finally {
      await driver.adminClearInstanceFetchFault();
    }
    // Once the read lands the form replaces the loader outright.
    await driver.adminWaitForRootBranch("sign-in");
    const settled = await driver.adminLoaderFacts();
    expect(settled.visible).toBe(false);
    expect(settled.formVisible).toBe(true);

    // And the shell likewise never shares the screen with the loader.
    await driver.adminSignInFill(seed.email, seed.password);
    await driver.adminSignInSubmit();
    await driver.adminWaitForRootBranch("dashboard");
    const shelled = await driver.adminLoaderFacts();
    expect(shelled.visible).toBe(false);
  }
);

test(
  specTitle(["ADM-001", "ADM-003"], "failed info fetch renders the retry screen once, then recovers on retry"),
  { tag: specTags(["ADM-001", "ADM-003"]) },
  async ({ driver }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    await driver.adminStartInstanceFetchSpy();
    await driver.adminFailInstanceFetch();
    await driver.adminOpenRoot();
    await driver.adminWaitForRootBranch("failure");

    const facts = await driver.adminFailureFacts();
    expect(facts.illustrationVisible).toBe(true);
    expect(facts.retryVisible).toBe(true);
    expect(facts.retryLabel).toBe("Retry");
    expect(await driver.adminInstanceFetchCount()).toBe(1);

    // No silent refetch: the count holds while the screen sits.
    await sleep(3000);
    expect(await driver.adminInstanceFetchCount()).toBe(1);
    expect(await driver.adminRootBranch()).toBe("failure");

    // Manual retry reloads the page, re-issues the single read, and lands
    // on the branch the server state selects.
    await driver.adminClearInstanceFetchFault();
    await driver.adminFailureRetry();
    await driver.adminWaitForRootBranch("sign-in");
    expect(await driver.adminInstanceFetchCount()).toBe(2);
    expect(await driver.adminCurrentPath()).toBe("/god-mode/");
  }
);

test(
  specTitle(["ADM-004"], "first-run submit creates the admin, marks setup done, and lands on the dashboard"),
  { tag: specTags(["ADM-004"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    const before = await serverAdminInstanceInfo();
    const createdEmail = uniqueEmail("parity-adm-setup");
    const cleared = await serverAdminClearForSetup();
    try {
      expect((await serverAdminInstanceInfo()).isSetupDone).toBe(false);

      await driver.adminOpenRoot();
      await driver.adminWaitForRootBranch("setup");
      const fresh = await driver.adminSetupFacts();
      expect(fresh.telemetryChecked).toBe(true);

      await driver.adminSetupFill({
        firstName: "Parity",
        lastName: "Admin",
        email: createdEmail,
        company: "Parity Setup Co",
        password: STRONG_PASSWORD,
        confirmPassword: STRONG_PASSWORD,
      });
      // Opt out, so the round-trip proves the flag rides along untampered.
      await driver.adminSetupSetTelemetry(false);
      expect((await driver.adminSetupFacts()).submitDisabled).toBe(false);
      await driver.adminSetupSubmit();
      await driver.adminWaitForRootBranch("dashboard");
      expect(await driver.adminCurrentPath()).toBe("/god-mode/general/");

      // The server created the admin, marked setup, and stored the company
      // name plus the opted-out telemetry flag.
      const session = await serverAdminSession(createdEmail, STRONG_PASSWORD);
      expect(await serverAdminListAdmins(session)).toEqual([createdEmail]);
      const after = await serverAdminInstanceInfo();
      expect(after.isSetupDone).toBe(true);
      expect(after.instanceName).toBe("Parity Setup Co");
      expect(after.telemetryEnabled).toBe(false);
    } finally {
      await serverAdminRestoreAfterSetup(seed.email, createdEmail, {
        adminCount: cleared.adminCount,
        wasSetupDone: cleared.wasSetupDone,
        instanceName: before.instanceName,
        telemetryEnabled: before.telemetryEnabled,
      });
    }
    const restored = await serverAdminInstanceInfo();
    expect(restored.isSetupDone).toBe(true);
    expect(restored.instanceName).toBe(before.instanceName);
    const seedSession = await serverAdminSession(seed.email, seed.password);
    expect(await serverAdminListAdmins(seedSession)).toEqual([seed.email]);
  }
);

test(
  specTitle(["ADM-005"], "setup validation gates submit and constrains every field"),
  { tag: specTags(["ADM-005"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    await withSetupUndone(async () => {
      await driver.adminOpenRoot();
      await driver.adminWaitForRootBranch("setup");

      await test.step("empty form stays shut with the meter showing and no hint", async () => {
        const facts = await driver.adminSetupFacts();
        expect(facts.submitDisabled).toBe(true);
        expect(facts.strengthVisible).toBe(true);
        expect(facts.strengthMessage).toBeNull();
        expect(facts.mismatchVisible).toBe(false);
      });

      await test.step("weak password keeps the gate shut with a live message", async () => {
        await driver.adminSetupFill({
          firstName: "Parity",
          email: uniqueEmail("parity-adm-weak"),
          password: WEAK_PASSWORD,
          confirmPassword: WEAK_PASSWORD,
        });
        await driver.adminSetupFocus("password");
        const facts = await driver.adminSetupFacts();
        expect(facts.submitDisabled).toBe(true);
        expect(facts.strengthVisible).toBe(true);
        expect(facts.strengthMessage).not.toBeNull();
      });

      await test.step("mismatch shows only once the confirm field is left or catches up", async () => {
        await driver.adminSetupFill({ password: STRONG_PASSWORD, confirmPassword: SHORT_CONFIRM });
        await driver.adminSetupFocus("confirm");
        expect((await driver.adminSetupFacts()).mismatchVisible).toBe(false);
        await driver.adminSetupBlur();
        expect((await driver.adminSetupFacts()).mismatchVisible).toBe(true);
        expect((await driver.adminSetupFacts()).submitDisabled).toBe(true);
        await driver.adminSetupFill({ confirmPassword: STRONG_PASSWORD });
        const facts = await driver.adminSetupFacts();
        expect(facts.mismatchVisible).toBe(false);
        expect(facts.submitDisabled).toBe(false);
      });

      await test.step("strong password hides the meter on blur and shows it on focus", async () => {
        await driver.adminSetupBlur();
        expect((await driver.adminSetupFacts()).strengthVisible).toBe(false);
        await driver.adminSetupFocus("password");
        expect((await driver.adminSetupFacts()).strengthVisible).toBe(true);
      });

      await test.step("toggles flip each field independently and stay out of tab order", async () => {
        expect(await driver.adminSetupToggleTabIndex("password")).toBe("-1");
        expect(await driver.adminSetupToggleTabIndex("confirm")).toBe("-1");
        await driver.adminSetupTogglePassword("password");
        let facts = await driver.adminSetupFacts();
        expect(facts.shown).toEqual({ password: true, confirm: false });
        await driver.adminSetupTogglePassword("confirm");
        facts = await driver.adminSetupFacts();
        expect(facts.shown).toEqual({ password: true, confirm: true });
        await driver.adminSetupTogglePassword("password");
        facts = await driver.adminSetupFacts();
        expect(facts.shown).toEqual({ password: false, confirm: true });
      });

      await test.step("names reject invalid characters and cap length", async () => {
        // Rejected input leaves the previous value untouched ("Parity" above).
        await driver.adminSetupFill({ firstName: "Ann1" });
        expect((await driver.adminSetupFacts()).values.firstName).toBe("Parity");
        await driver.adminSetupFill({ firstName: "Jean-Paul" });
        expect((await driver.adminSetupFacts()).values.firstName).toBe("Jean-Paul");
        await driver.adminSetupFill({ firstName: "a".repeat(60) });
        expect((await driver.adminSetupFacts()).values.firstName).toHaveLength(50);
        await driver.adminSetupFill({ company: "Acme{Corp}" });
        expect((await driver.adminSetupFacts()).values.company).toBe("");
        await driver.adminSetupFill({ company: "Acme_123 Corp-x" });
        expect((await driver.adminSetupFacts()).values.company).toBe("Acme_123 Corp-x");
        await driver.adminSetupFill({ company: "b".repeat(90) });
        expect((await driver.adminSetupFacts()).values.company).toHaveLength(80);
      });

      // Nothing here wrote: setup stays undone and the admin roster is intact.
      expect((await serverAdminInstanceInfo()).isSetupDone).toBe(false);
      const session = await serverAdminSession(seed.email, seed.password);
      expect(await serverAdminListAdmins(session)).toEqual([seed.email]);
    });
  }
);

test(
  specTitle(["ADM-006"], "setup fields prefill from the address with telemetry defaulting on"),
  { tag: specTags(["ADM-006"]) },
  async ({ driver }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    await withSetupUndone(async () => {
      const email = uniqueEmail("parity-adm-prefill");
      await driver.adminOpenPath(
        `/?first_name=Parity&last_name=Prefill&company=Prefill%20Co&email=${encodeURIComponent(email)}`
      );
      await driver.adminWaitForRootBranch("setup");
      let facts = await driver.adminSetupFacts();
      expect(facts.values).toEqual({
        firstName: "Parity",
        lastName: "Prefill",
        email,
        company: "Prefill Co",
      });
      expect(facts.telemetryChecked).toBe(true);

      await driver.adminOpenPath("/?is_telemetry_enabled=False");
      await driver.adminWaitForRootBranch("setup");
      facts = await driver.adminSetupFacts();
      expect(facts.values).toEqual({ firstName: "", lastName: "", email: "", company: "" });
      expect(facts.telemetryChecked).toBe(false);
    });
  }
);

test(
  specTitle(["ADM-006"], "setup error codes surface inline or as a banner, unknown codes stay silent"),
  { tag: specTags(["ADM-006"]) },
  async ({ driver }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    await withSetupUndone(async () => {
      await driver.adminOpenPath("/?error_code=INVALID_EMAIL&error_message=parity-probe-email");
      await driver.adminWaitForRootBranch("setup");
      let facts = await driver.adminSetupFacts();
      expect(facts.inlineErrors).toEqual({ email: "parity-probe-email", password: null });
      expect(facts.bannerText).toBeNull();

      await driver.adminOpenPath("/?error_code=INVALID_PASSWORD&error_message=parity-probe-password");
      await driver.adminWaitForRootBranch("setup");
      facts = await driver.adminSetupFacts();
      expect(facts.inlineErrors).toEqual({ email: null, password: "parity-probe-password" });
      expect(facts.bannerText).toBeNull();

      await driver.adminOpenPath("/?error_code=ADMIN_ALREADY_EXIST&error_message=parity-probe-banner");
      await driver.adminWaitForRootBranch("setup");
      facts = await driver.adminSetupFacts();
      expect(facts.inlineErrors).toEqual({ email: null, password: null });
      expect(facts.bannerText).toBe("parity-probe-banner");

      await driver.adminOpenPath("/?error_code=NO_SUCH_CODE&error_message=parity-probe-unknown");
      await driver.adminWaitForRootBranch("setup");
      facts = await driver.adminSetupFacts();
      expect(facts.inlineErrors).toEqual({ email: null, password: null });
      expect(facts.bannerText).toBeNull();
    });
  }
);

test(
  specTitle(["ADM-006"], "bug: NEWFRONT-457 rejected setup posts resurface silently and drop the company"),
  { tag: specTags(["ADM-006"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    const before = await serverAdminInstanceInfo();
    const cleared = await serverAdminClearForSetup();
    try {
      await driver.adminOpenRoot();
      await driver.adminWaitForRootBranch("setup");
      // The seed address is taken, so the server rejects the post — but
      // the numeric round-tripped code matches none of the form's named
      // cases, and the company echo rides under a key the form never
      // reads, so the form comes back bare and the company is lost.
      await driver.adminSetupFill({
        firstName: "Parity",
        lastName: "Admin",
        email: seed.email,
        company: "Parity Bug Co",
        password: STRONG_PASSWORD,
        confirmPassword: STRONG_PASSWORD,
      });
      await driver.adminSetupSubmit();
      await driver.adminWaitForRootBranch("setup");
      const facts = await driver.adminSetupFacts();
      expect(facts.values.firstName).toBe("Parity");
      expect(facts.values.lastName).toBe("Admin");
      expect(facts.values.email).toBe(seed.email);
      if (driver.target === "web_new") {
        // Intended (NEWFRONT-457): the rejection surfaces its reason and
        // the company echo survives the round trip.
        expect(facts.bannerText).not.toBeNull();
        expect(facts.values.company).toBe("Parity Bug Co");
      } else {
        expect(facts.bannerText).toBeNull();
        expect(facts.inlineErrors).toEqual({ email: null, password: null });
        expect(facts.values.company).toBe("");
      }

      // The rejection wrote nothing: setup stays undone with no admins.
      expect((await serverAdminInstanceInfo()).isSetupDone).toBe(false);
      expect((await serverAdminMe("")).status).toBe(401);
    } finally {
      await serverAdminRestoreAfterSetup(seed.email, "parity-adm-rejected-none@example.com", {
        adminCount: cleared.adminCount,
        wasSetupDone: cleared.wasSetupDone,
        instanceName: before.instanceName,
        telemetryEnabled: before.telemetryEnabled,
      });
    }
  }
);

test(
  specTitle(["ADM-007"], "sign-in gates, prefills, focuses, toggles, and lands on the dashboard"),
  { tag: specTags(["ADM-007"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    await driver.adminOpenRoot();
    await driver.adminWaitForRootBranch("sign-in");

    await test.step("fresh form is shut with the email field focused", async () => {
      const facts = await driver.adminSignInFacts();
      expect(facts.email).toBe("");
      expect(facts.passwordFilled).toBe(false);
      expect(facts.submitDisabled).toBe(true);
      expect(facts.submitBusy).toBe(false);
      expect(await driver.adminSignInEmailFocused()).toBe(true);
    });

    await test.step("email prefills from the address", async () => {
      await driver.adminOpenPath(`/?email=${encodeURIComponent(seed.email)}`);
      await driver.adminWaitForRootBranch("sign-in");
      expect((await driver.adminSignInFacts()).email).toBe(seed.email);
    });

    await test.step("toggle flips the password field", async () => {
      await driver.adminSignInFill(seed.email, seed.password);
      expect((await driver.adminSignInFacts()).passwordShown).toBe(false);
      await driver.adminSignInTogglePassword();
      expect((await driver.adminSignInFacts()).passwordShown).toBe(true);
      await driver.adminSignInTogglePassword();
      expect((await driver.adminSignInFacts()).passwordShown).toBe(false);
      expect((await driver.adminSignInFacts()).submitDisabled).toBe(false);
    });

    await test.step("submit lands on the dashboard", async () => {
      // The submit swaps its label for a spinner while the native post
      // is in flight, but that render is unobservable: once the post is
      // sent the frame is navigating, and the execution context stalls
      // every DOM read until the response commits — holding the post
      // with a route only stretches the stall. The scenario pins the
      // round-trip outcome instead.
      await driver.adminSignInSubmit();
      await driver.adminWaitForRootBranch("dashboard");
      expect(await driver.adminCurrentPath()).toBe("/god-mode/general/");
    });

    // The round-trip established a server session for the seed admin.
    const session = await serverAdminSession(seed.email, seed.password);
    expect(await serverAdminMe(session)).toEqual({ status: 200, email: seed.email });
  }
);

test(
  specTitle(["ADM-008"], "wrong password shows the mapped dismissible banner"),
  { tag: specTags(["ADM-008"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    await driver.adminOpenRoot();
    await driver.adminWaitForRootBranch("sign-in");
    await driver.adminSignInFill(seed.email, "Parity-Wrong-1!");
    await driver.adminSignInSubmit();
    await driver.adminWaitForRootBranch("sign-in");
    expect(await driver.adminCurrentSearch()).toContain("error_code=");

    const facts = await driver.adminSignInFacts();
    expect(facts.genericBanner).toBeNull();
    expect(facts.mappedBanner).not.toBeNull();
    expect(facts.mappedBanner?.text ?? "").toContain("Authentication failed");
    expect(facts.mappedBanner?.linkHref ?? null).toBeNull();

    await driver.adminSignInDismissMappedBanner();
    expect((await driver.adminSignInFacts()).mappedBanner).toBeNull();

    // The failure established nothing: the API rejects the same password
    // and anonymous reads stay rejected.
    await expect(serverAdminSession(seed.email, "Parity-Wrong-1!")).rejects.toThrow();
    expect((await serverAdminMe("")).status).toBe(401);
  }
);

test(
  specTitle(["ADM-008"], "unknown account banner carries a link; crafted codes drive generic or silent paths"),
  { tag: specTags(["ADM-008"]) },
  async ({ driver }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    await driver.adminOpenRoot();
    await driver.adminWaitForRootBranch("sign-in");
    await driver.adminSignInFill(uniqueEmail("parity-adm-unknown"), "Parity-Wrong-1!");
    await driver.adminSignInSubmit();
    await driver.adminWaitForRootBranch("sign-in");

    const facts = await driver.adminSignInFacts();
    expect(facts.genericBanner).toBeNull();
    expect(facts.mappedBanner).not.toBeNull();
    expect(facts.mappedBanner?.linkHref ?? null).not.toBeNull();

    await test.step("crafted known code renders the generic banner", async () => {
      await driver.adminOpenPath("/?error_code=INVALID_EMAIL&error_message=parity-probe-generic");
      await driver.adminWaitForRootBranch("sign-in");
      const crafted = await driver.adminSignInFacts();
      expect(crafted.genericBanner).toBe("parity-probe-generic");
      expect(crafted.mappedBanner).toBeNull();
    });

    await test.step("crafted unknown code renders no banner", async () => {
      await driver.adminOpenPath("/?error_code=NO_SUCH_CODE&error_message=parity-probe-unknown");
      await driver.adminWaitForRootBranch("sign-in");
      const crafted = await driver.adminSignInFacts();
      expect(crafted.genericBanner).toBeNull();
      expect(crafted.mappedBanner).toBeNull();
    });
  }
);

test(
  specTitle(["ADM-008"], "bug: NEWFRONT-458 mapped banner link lands on not-found"),
  { tag: specTags(["ADM-008"]) },
  async ({ driver }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    await driver.adminOpenRoot();
    await driver.adminWaitForRootBranch("sign-in");
    await driver.adminSignInFill(uniqueEmail("parity-adm-unknown"), "Parity-Wrong-1!");
    await driver.adminSignInSubmit();
    await driver.adminWaitForRootBranch("sign-in");

    // The banner link targets a route that does not exist: following it
    // renders the not-found screen instead of returning to sign-in.
    // The hrefs are mount-relative, so strip the console mount before
    // handing them to the path helper.
    const href = (await driver.adminSignInFacts()).mappedBanner?.linkHref ?? null;
    if (driver.target === "web_new") {
      // Intended (NEWFRONT-458): the link returns to the sign-in entry.
      expect(href).toBe("/god-mode/");
      await driver.adminOpenPath((href ?? "/god-mode/").replace(/^\/god-mode/, ""));
      await driver.adminWaitForRootBranch("sign-in");
      return;
    }
    expect(href).toBe("/god-mode/admin/");
    await driver.adminOpenPath((href ?? "/god-mode/admin/").replace(/^\/god-mode/, ""));
    const facts = await driver.adminNotFoundFacts();
    expect(facts.illustrationVisible).toBe(true);
    expect(facts.messageVisible).toBe(true);
  }
);

test(
  specTitle(["ADM-009"], "guards bounce by session state with replace navigation"),
  { tag: specTags(["ADM-009"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    await test.step("signed-out dashboard visit bounces to the root adding no history", async () => {
      await driver.adminOpenRoot();
      await driver.adminWaitForRootBranch("sign-in");
      const before = await driver.adminHistoryLength();
      await driver.adminOpenPath("/general/");
      await driver.adminWaitForRootBranch("sign-in");
      expect(await driver.adminCurrentPath()).toBe("/god-mode/");
      expect(await driver.adminHistoryLength()).toBe(before + 1);
      // Anonymous reads are rejected. Admin paths authenticate on a
      // separate admin cookie, so even a signed-in member's app session
      // reads as anonymous here — and both drive the guard to its
      // logged-out branch.
      expect(await serverAdminMe("")).toEqual({ status: 401, email: null });
      const member = requireMentionMember(seed);
      const memberSession = await signInSession(member.email, member.password);
      expect((await serverAdminMe(memberSession)).status).toBe(401);
    });

    await test.step("revoked grant reads forbidden while the session stands", async () => {
      // The 403 the guard maps to "authentication not done" arises only
      // for an admin session whose grant was revoked mid-session.
      const session = await serverAdminSession(seed.email, seed.password);
      expect((await serverAdminMe(session)).status).toBe(200);
      await serverAdminSetGrant(seed.email, false);
      try {
        expect((await serverAdminMe(session)).status).toBe(403);
      } finally {
        await serverAdminSetGrant(seed.email, true);
      }
      expect((await serverAdminMe(session)).status).toBe(200);
    });

    await test.step("loader covers the unresolved session check", async () => {
      await driver.adminDelaySessionFetch(2500);
      try {
        await driver.adminOpenPath("/general/");
        await expect
          .poll(async () => ((await driver.adminLoaderFacts()).visible ? "loader" : "waiting"), { timeout: 15_000 })
          .toBe("loader");
      } finally {
        await driver.adminClearSessionFetchDelay();
      }
      await driver.adminWaitForRootBranch("sign-in");
    });

    await test.step("signed-in root visit replaces to general adding no history", async () => {
      await driver.adminSignInFill(seed.email, seed.password);
      await driver.adminSignInSubmit();
      await driver.adminWaitForRootBranch("dashboard");
      const before = await driver.adminHistoryLength();
      await driver.adminOpenRoot();
      await driver.adminWaitForRootBranch("dashboard");
      expect(await driver.adminCurrentPath()).toBe("/god-mode/general/");
      expect(await driver.adminHistoryLength()).toBe(before + 1);
      const session = await serverAdminSession(seed.email, seed.password);
      expect((await serverAdminMe(session)).status).toBe(200);
    });
  }
);

test(
  specTitle(["ADM-010"], "sign-out ends the session and resets client state"),
  { tag: specTags(["ADM-010"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    await driver.adminEnsureSignedIn(seed.email, seed.password);
    await driver.adminSetTheme("dark");
    expect((await driver.adminThemeFacts()).stored).toBe("dark");

    await driver.adminOpenAccountMenu();
    const menu = await driver.adminAccountMenuFacts();
    expect(menu.email).toBe(seed.email);
    expect(menu.signOutVisible).toBe(true);

    await driver.adminSidebarSignOut();
    await driver.adminWaitForRootBranch("sign-in");
    expect(await driver.adminDashboardVisible()).toBe(false);
    expect((await driver.adminThemeFacts()).stored).toBe("system");

    // The signed-out client bounces off the dashboard again.
    await driver.adminOpenPath("/general/");
    await driver.adminWaitForRootBranch("sign-in");

    // The endpoint clears the server session too: the same session the
    // flow used reads as signed out after a direct sign-out post.
    const session = await serverAdminSession(seed.email, seed.password);
    expect((await serverAdminMe(session)).status).toBe(200);
    await serverAdminSignOut(session);
    expect((await serverAdminMe(session)).status).toBe(401);
  }
);

test(
  specTitle(["ADM-011"], "unknown paths render not-found with a back action to general"),
  { tag: specTags(["ADM-011"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    await test.step("signed-in unknown path offers the way back", async () => {
      await driver.adminEnsureSignedIn(seed.email, seed.password);
      await driver.adminOpenPath("/no-such-place/");
      const facts = await driver.adminNotFoundFacts();
      expect(facts.illustrationVisible).toBe(true);
      expect(facts.messageVisible).toBe(true);
      expect(facts.actionLabel).toBe("Go to general settings");
      await driver.adminNotFoundFollowAction();
      expect(await driver.adminCurrentPath()).toBe("/god-mode/general/");
    });

    await test.step("signed-out unknown path renders the same screen", async () => {
      await driver.adminSidebarSignOut();
      await driver.adminWaitForRootBranch("sign-in");
      await driver.adminOpenPath("/no-such-place/");
      const facts = await driver.adminNotFoundFacts();
      expect(facts.illustrationVisible).toBe(true);
      expect(facts.messageVisible).toBe(true);
      expect(facts.actionLabel).toBe("Go to general settings");
    });
  }
);

test(
  specTitle(["ADM-012"], "first-workspace popup invites, links, and dismisses"),
  { tag: specTags(["ADM-012"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    // The seed holds workspaces, so no card shows on a truthful load.
    expect((await serverAdminInstanceInfo()).workspacesExist).toBe(true);
    const session = await serverAdminSession(seed.email, seed.password);
    expect((await serverAdminMe(session)).status).toBe(200);
    await driver.adminStartInstanceFetchSpy();
    await driver.adminEnsureSignedIn(seed.email, seed.password);
    expect((await driver.adminFirstWorkspacePopupFacts()).visible).toBe(false);

    // The first load reporting no workspaces raises the card.
    await driver.adminStubNoWorkspaces();
    await driver.adminOpenRoot();
    await driver.adminWaitForRootBranch("dashboard");
    // The card reads the dashboard's info load, so it lands once that
    // read settles rather than with the shell.
    await expect
      .poll(async () => (await driver.adminFirstWorkspacePopupFacts()).visible, { timeout: 15_000 })
      .toBe(true);
    const popup = await driver.adminFirstWorkspacePopupFacts();
    expect(popup.createHref).toBe("/god-mode/workspace/create/");

    // The create action leads to the create page without clearing the card.
    await driver.adminFirstWorkspacePopupFollowCreate();
    expect(await driver.adminCurrentPath()).toBe("/god-mode/workspace/create/");
    await driver.adminOpenPath("/general/");
    await driver.adminWaitForRootBranch("dashboard");
    await expect
      .poll(async () => (await driver.adminFirstWorkspacePopupFacts()).visible, { timeout: 15_000 })
      .toBe(true);

    // Dismissing clears it, and a truthful reload keeps it gone.
    await driver.adminFirstWorkspacePopupDismiss();
    expect((await driver.adminFirstWorkspacePopupFacts()).visible).toBe(false);
    await driver.adminClearInstanceFetchFault();
    await driver.adminOpenRoot();
    await driver.adminWaitForRootBranch("dashboard");
    expect((await driver.adminFirstWorkspacePopupFacts()).visible).toBe(false);
  }
);
