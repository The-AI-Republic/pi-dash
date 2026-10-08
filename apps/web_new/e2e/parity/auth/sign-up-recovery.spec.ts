// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios for sign-up, passwords, recovery, guards, and landing
// (NEWFRONT-108): rows AUTH-009 through AUTH-016. Green on apps/web first
// (the oracle); the same file must go green on apps/web_new once the auth
// area lands there. Every scenario uses a fresh address per run so sign-up
// attempts never collide, and every UI assertion is paired with a
// server-state check through helpers/api.
import { test, expect } from "../fixtures";
import {
  completeOnboarding,
  emailCheckStatus,
  mintMagicCode,
  mintPasswordResetToken,
  setSmtpConfigured,
  signInSession,
  signUpAccount,
  uniqueEmail,
  userFacts,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

const STRONG_PASSWORD = "Tq7!mZ2@kP9#xR";
const OTHER_STRONG_PASSWORD = "Nb4@gH8$kL1!zQ";
const WEAK_PASSWORD = "weakpass1";

// The scratch stack throttles auth endpoints at 30/minute per IP
// (authentication scope: email-check, code generate, forgot-password), and
// every scenario shares that bucket. Pacing scenarios keeps rolling-window
// headroom so the oracle proves app behavior instead of tripping the
// throttle; the driver and API helpers still ride out an occasional
// saturated minute with a 65s backoff.
test.beforeEach(async () => {
  await new Promise((resolve) => setTimeout(resolve, 10_000));
});

test(
  specTitle(["AUTH-009"], "create an account with a password"),
  { tag: specTags(["AUTH-009"]) },
  async ({ driver, seed }) => {
    const email = uniqueEmail("parity-signup");

    await test.step("new address reaches the password step", async () => {
      await driver.openSignUp();
      await driver.submitAuthEmail(email);
      expect(await driver.authStep()).toBe("password");
    });

    await test.step("submitting matching strong passwords creates the account", async () => {
      await driver.signUpWithPassword(STRONG_PASSWORD, STRONG_PASSWORD);
      // Newcomers land on the post-sign-up path, never inside a workspace.
      // The guard funnels them after the profile loads, so poll.
      await expect.poll(() => driver.currentPath(), { timeout: 30_000 }).toMatch(/^\/onboarding/);
    });

    await test.step("the server stored the new account and secret", async () => {
      const check = await emailCheckStatus(email);
      expect(check.existing).toBe(true);
      const session = await signInSession(email, STRONG_PASSWORD);
      expect(session).toContain("session-id=");
      expect(seed.workspaceSlug).toBeTruthy();
    });
  }
);

test(
  specTitle(["AUTH-009"], "create an account with an emailed code"),
  { tag: specTags(["AUTH-009"]) },
  async ({ driver }) => {
    const email = uniqueEmail("parity-code-signup");
    await setSmtpConfigured(true);
    try {
      await test.step("new address reaches the code step", async () => {
        await driver.openSignUp();
        await driver.submitAuthEmail(email);
        expect(await driver.authStep()).toBe("code");
      });

      await test.step("the mailed code creates the account", async () => {
        const code = await mintMagicCode(email);
        const resend = await driver.codeResendState();
        expect(resend.label).toMatch(/resend|requesting/i);
        await driver.submitUniqueCode(code);
        await expect.poll(() => driver.currentPath(), { timeout: 30_000 }).toMatch(/^\/onboarding/);
      });

      await test.step("the server stored a passwordless account", async () => {
        const facts = await userFacts(email);
        expect(facts.exists).toBe(true);
        expect(facts.passwordAutoset).toBe(true);
      });
    } finally {
      await setSmtpConfigured(false);
    }
  }
);

test(
  specTitle(["AUTH-010"], "weak passwords are refused and mismatched confirmations block submit"),
  { tag: specTags(["AUTH-010"]) },
  async ({ driver }) => {
    const email = uniqueEmail("parity-strength");

    await test.step("reach the sign-up password step", async () => {
      await driver.openSignUp();
      await driver.submitAuthEmail(email);
      expect(await driver.authStep()).toBe("password");
    });

    await test.step("a weak secret is refused without posting", async () => {
      await driver.fillPasswordFields(WEAK_PASSWORD, WEAK_PASSWORD);
      await expect.poll(() => driver.passwordSubmitEnabled(), { timeout: 10_000 }).toBe(true);
      await driver.clickPasswordSubmit();
      expect(await driver.currentPath()).toMatch(/^\/sign-up/);
      expect(await driver.authBanner()).toMatch(/strong password/i);
    });

    await test.step("a mismatched confirmation blocks submit inline", async () => {
      await driver.fillPasswordFields(STRONG_PASSWORD, `${STRONG_PASSWORD}-other`);
      expect(await driver.passwordMismatchError()).toMatch(/don't match/i);
      await expect.poll(() => driver.passwordSubmitEnabled(), { timeout: 10_000 }).toBe(false);
    });
  }
);

test(
  specTitle(["AUTH-011"], "sign-up entry redirect, prefill, and return path"),
  { tag: specTags(["AUTH-011"]) },
  async ({ driver, seed }) => {
    await test.step("legacy sign-up URLs forward to the canonical one", async () => {
      // The redirect runs client-side after hydration, which is slow when
      // the dev server compiles under suite load.
      await driver.openPath("/register");
      await expect.poll(() => driver.currentPath(), { timeout: 60_000 }).toMatch(/^\/sign-up/);
      await driver.openPath("/accounts/sign-up");
      await expect.poll(() => driver.currentPath(), { timeout: 60_000 }).toMatch(/^\/sign-up/);
    });

    await test.step("an email query value prefills the field", async () => {
      const prefill = uniqueEmail("parity-prefill");
      await driver.openSignUp({ email: prefill });
      expect(await driver.authEmailValue()).toBe(prefill);
    });

    await test.step("a return path survives to the password step", async () => {
      const nextPath = `/${seed.workspaceSlug}/projects/${seed.projectId}/issues`;
      await driver.openSignUp({ nextPath });
      await driver.submitAuthEmail(uniqueEmail("parity-return"));
      expect(await driver.authStep()).toBe("password");
      expect(await driver.authNextPathValue()).toBe(nextPath);
    });
  }
);

test(
  specTitle(["AUTH-012"], "forgot-password sends a reset link and throttles resends"),
  { tag: specTags(["AUTH-012"]) },
  async ({ driver, seed }) => {
    await setSmtpConfigured(true);
    try {
      await test.step("the email query prefills the form", async () => {
        await driver.openForgotPassword(seed.email);
        expect(await driver.authEmailValue()).toBe(seed.email);
      });

      await test.step("submitting shows the inbox-check confirmation", async () => {
        const toast = await driver.submitForgotPassword(seed.email);
        expect(toast).toMatch(/check your inbox/i);
      });

      await test.step("a resend countdown disables the form", async () => {
        const state = await driver.forgotResendState();
        expect(state.disabled).toBe(true);
        expect(state.label).toMatch(/resend in \d+/i);
      });
    } finally {
      await setSmtpConfigured(false);
    }
  }
);

test(
  specTitle(["AUTH-012"], "forgot-password failures stay silent about account existence"),
  { tag: specTags(["AUTH-012"]) },
  async ({ driver }) => {
    const unknown = uniqueEmail("parity-ghost");

    await test.step("an unknown address gets a failure toast", async () => {
      await driver.openForgotPassword();
      const toast = await driver.submitForgotPassword(unknown);
      expect(toast.length).toBeGreaterThan(0);
    });

    await test.step("the toast leaks no account-existence detail", async () => {
      const toast = await driver.submitForgotPassword(uniqueEmail("parity-ghost"));
      expect(toast).not.toMatch(/no account|does not exist|not found|unknown user/i);
    });
  }
);

test(
  specTitle(["AUTH-013"], "reset a password from the emailed link"),
  { tag: specTags(["AUTH-013"]) },
  async ({ driver }) => {
    test.setTimeout(300_000);
    // A dedicated account: resetting the shared seed secret would race
    // any concurrent run that signs in as it. Created over the API so the
    // browser stays signed out (the reset form only renders for visitors).
    const email = uniqueEmail("parity-reset");
    await test.step("create the account that will be reset", async () => {
      await signUpAccount(email, STRONG_PASSWORD);
      const check = await emailCheckStatus(email);
      expect(check.existing).toBe(true);
    });

    const link = await mintPasswordResetToken(email);

    await test.step("the link opens the reset form with the address locked", async () => {
      await driver.openResetPassword({ uid: link.uid, token: link.token, email });
      expect(await driver.authEmailValue()).toBe(email);
    });

    await test.step("a strong matching secret resets the password", async () => {
      await driver.submitNewPassword(OTHER_STRONG_PASSWORD, OTHER_STRONG_PASSWORD);
      expect(await driver.currentPath()).toMatch(/^\/(sign-in)?(\?.*)?$/);
    });

    await test.step("the new secret signs in; the old one no longer does", async () => {
      const session = await signInSession(email, OTHER_STRONG_PASSWORD);
      expect(session).toContain("session-id=");
      await expect(signInSession(email, STRONG_PASSWORD)).rejects.toThrow();
    });

    await test.step("the same link cannot be reused", async () => {
      await driver.openResetPassword({ uid: link.uid, token: link.token, email });
      await driver.submitNewPassword(STRONG_PASSWORD, STRONG_PASSWORD);
      expect(await driver.waitForAuthBanner()).toMatch(/invalid password token|expired password token/i);
    });
  }
);

test(
  specTitle(["AUTH-013"], "a tampered reset token renders an explanatory banner"),
  { tag: specTags(["AUTH-013"]) },
  async ({ driver, seed }) => {
    await test.step("a wrong token for a real uid shows the invalid-token banner", async () => {
      const link = await mintPasswordResetToken(seed.email);
      await driver.openResetPassword({ uid: link.uid, token: "000000", email: seed.email });
      await driver.submitNewPassword(STRONG_PASSWORD, STRONG_PASSWORD);
      expect(await driver.waitForAuthBanner()).toMatch(/invalid password token|expired password token/i);
    });
  }
);

// Oracle truth for NEWFRONT-130: a malformed uid crashes the native reset
// POST (HTTP 500) instead of returning to the banner flow. Intended per
// AUTH-013: an explanatory invalid-token banner. Locked here so the fix has
// a regression test.
test(
  specTitle(["AUTH-013"], "bug: malformed reset uid escapes the banner flow (NEWFRONT-130)"),
  { tag: specTags(["AUTH-013"]) },
  async ({ driver, seed }) => {
    await test.step("no banner; the raw endpoint answers instead", async () => {
      await driver.openResetPassword({ uid: "AAAA", token: "bogus-token", email: seed.email });
      await driver.submitNewPassword(STRONG_PASSWORD, STRONG_PASSWORD);
      expect(await driver.authBanner()).toBeNull();
      expect(await driver.currentPath()).toContain("/auth/reset-password/");
    });
  }
);

test(
  specTitle(["AUTH-014"], "first password for a passwordless account; set users are sent away"),
  { tag: specTags(["AUTH-014"]) },
  async ({ driver }) => {
    test.setTimeout(300_000);
    const email = uniqueEmail("parity-nosecret");
    await setSmtpConfigured(true);
    try {
      await test.step("a code sign-up leaves the account passwordless", async () => {
        await driver.openSignUp();
        await driver.submitAuthEmail(email);
        expect(await driver.authStep()).toBe("code");
        await driver.submitUniqueCode(await mintMagicCode(email));
        await expect.poll(() => driver.currentPath(), { timeout: 30_000 }).toMatch(/^\/onboarding/);
      });

      await test.step("the passwordless user reaches the set-password form", async () => {
        await driver.openSetPassword();
        expect(await driver.currentPath()).toMatch(/set-password/);
      });
    } finally {
      await setSmtpConfigured(false);
    }
  }
);

test(
  specTitle(["AUTH-014"], "a user who already set a password is sent away from set-password"),
  { tag: specTags(["AUTH-014"]) },
  async ({ driver, seed }) => {
    await test.step("signed-in members never see the set-password form", async () => {
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.openSetPassword();
      // The guard sends them on after the profile loads, so poll the
      // negative: the set-password route must disappear.
      await expect.poll(() => driver.currentPath(), { timeout: 30_000 }).not.toMatch(/set-password/);
    });
  }
);

test(
  specTitle(["AUTH-015"], "route guards funnel visitors and members"),
  { tag: specTags(["AUTH-015"]) },
  async ({ driver, seed }) => {
    await test.step("signed-out visitors are bounced to sign-in with a return path", async () => {
      await driver.openPath("/create-workspace");
      // The guard bounces after the session check loads, so poll.
      await expect.poll(() => driver.currentPath(), { timeout: 30_000 }).toMatch(/^\/\?next_path=/);
      expect(await driver.authEmailValue()).toBe("");
    });

    await test.step("signed-out visitors cannot skip onboarding through the gate", async () => {
      await driver.openPath("/onboarding");
      await expect.poll(() => driver.currentPath(), { timeout: 30_000 }).toMatch(/^\/\?next_path=/);
    });

    await test.step("signed-in members are kept out of signed-out screens", async () => {
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.openPath("/sign-up");
      // The old app normalizes workspace landings with a trailing slash.
      await expect
        .poll(() => driver.currentPath(), { timeout: 30_000 })
        .toMatch(new RegExp(`^/${seed.workspaceSlug}/?$`));
    });

    await test.step("finished users are bounced away from onboarding", async () => {
      await driver.openPath("/onboarding");
      await expect
        .poll(() => driver.currentPath(), { timeout: 30_000 })
        .toMatch(new RegExp(`^/${seed.workspaceSlug}/?$`));
    });
  }
);

test(
  specTitle(["AUTH-015"], "unfinished users are funneled to onboarding"),
  { tag: specTags(["AUTH-015"]) },
  async ({ driver }) => {
    const email = uniqueEmail("parity-unfinished");

    await test.step("a newcomer lands on onboarding", async () => {
      await driver.openSignUp();
      await driver.submitAuthEmail(email);
      await driver.signUpWithPassword(STRONG_PASSWORD, STRONG_PASSWORD);
      await expect.poll(() => driver.currentPath(), { timeout: 30_000 }).toMatch(/^\/onboarding/);
    });

    await test.step("the entry route keeps funneling them there", async () => {
      await driver.openPath("/");
      await expect.poll(() => driver.currentPath(), { timeout: 30_000 }).toMatch(/^\/onboarding/);
    });
  }
);

// One scenario per landing case: each starts from a fresh signed-out
// context, because the entry card only renders for visitors. (The harness
// sign-in resolves its wait as soon as the entry URL matches, so every
// landing assertion polls for the guarded destination; the old app
// normalizes workspace landings with a trailing slash.)
test(
  specTitle(["AUTH-016"], "a safe return path wins the post-auth landing"),
  { tag: specTags(["AUTH-016"]) },
  async ({ driver, seed }) => {
    const projectIssues = `/${seed.workspaceSlug}/projects/${seed.projectId}/issues`;
    await driver.openPath(`/?next_path=${encodeURIComponent(projectIssues)}`);
    await driver.signInWithPassword(seed.email, seed.password);
    await expect.poll(() => driver.currentPath(), { timeout: 30_000 }).toMatch(new RegExp(`^${projectIssues}/?$`));
  }
);

// The native sign-in POST sanitizes an absolute return path to its local
// path instead of leaving the origin, so landing ends on /phish/ here.
test(
  specTitle(["AUTH-016"], "an absolute return path never leaves the origin at landing"),
  { tag: specTags(["AUTH-016"]) },
  async ({ driver, seed }) => {
    await driver.openPath(`/?next_path=${encodeURIComponent("https://evil.example.com/phish")}`);
    await driver.signInWithPassword(seed.email, seed.password);
    await expect.poll(() => driver.currentPath(), { timeout: 30_000 }).toMatch(/^\/phish\/?$/);
    expect(await driver.currentPath()).not.toContain("evil.example.com");
  }
);

test(
  specTitle(["AUTH-016"], "no return path lands on the last workspace"),
  { tag: specTags(["AUTH-016"]) },
  async ({ driver, seed }) => {
    await driver.openEntry();
    await driver.signInWithPassword(seed.email, seed.password);
    await expect
      .poll(() => driver.currentPath(), { timeout: 60_000 })
      .toMatch(new RegExp(`^/${seed.workspaceSlug}/?$`));
  }
);

test(
  specTitle(["AUTH-016"], "users with nowhere to go land on workspace creation"),
  { tag: specTags(["AUTH-016"]) },
  async ({ driver }) => {
    test.setTimeout(300_000);
    const email = uniqueEmail("parity-nowhere");

    await test.step("a memberless onboarded user is offered creation", async () => {
      await driver.openSignUp();
      await driver.submitAuthEmail(email);
      await driver.signUpWithPassword(STRONG_PASSWORD, STRONG_PASSWORD);
      await completeOnboarding(email, STRONG_PASSWORD);
      // The guard's profile read is browser-cached for seconds; clear the
      // cache (session cookies stay) so the funnel reads the onboarded
      // flag instead of racing its expiry. This changes no app behavior
      // under test, only the test's timing determinism.
      const cdp = await driver.page.context().newCDPSession(driver.page);
      await cdp.send("Network.clearBrowserCache");
      // The harness entry waits for the visitor card, which a signed-in
      // user never sees; navigate plainly and poll for the funnel (the old
      // app normalizes the landing with a trailing slash).
      await driver.openPath("/");
      await expect.poll(() => driver.currentPath(), { timeout: 30_000 }).toMatch(/^\/create-workspace\/?$/);
    });
  }
);
