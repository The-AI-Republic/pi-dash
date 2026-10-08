// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios for the sign-in core (NEWFRONT-107): one per row
// AUTH-001 through AUTH-008, green on apps/web first. The same file must go
// green on apps/web_new once the auth area lands there.
// Behavior notes (learned from the old app, reference only):
// - The card is email-first: an email check decides the next step, and an
//   unknown address flips the card into sign-up mode.
// - Password and code submits are native form POSTs, so every submit ends in
//   a full page load; failures redirect back with an error_code that forces
//   the fixing step and raises a dismissible banner.
// - The seeded stack is mail-less (no SMTP host): code login is hidden and
//   the scenarios that need it flip EMAIL_HOST in-spec and restore it after.
//   Code generates are capped per address (a few per 10 minutes server-side),
//   so each run performs at most two and unrelated scenarios never generate.
import { test, expect } from "../fixtures";
import {
  adminSignInSession,
  createInvitation,
  deleteInvitation,
  emailCheck,
  instanceConfig,
  magicGenerate,
  nativeMagicSignIn,
  patchInstanceConfig,
  signInSession,
  singleInvitation,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import type { ParityDriver } from "../drivers/index";

/**
 * Poll preset for reads that hit the server. The seeded stack throttles
 * anonymous calls per minute per IP across every parallel run, so
 * server-backed polls wait two seconds between attempts instead of
 * hammering. UI-only polls keep Playwright's default pace.
 */
const SERVER_POLL = { timeout: 60_000, intervals: [2_000] };

/**
 * Re-navigate until the no-methods card converges to the expected state. A
 * fresh load can transiently show (or hide) the card while a throttled
 * config fetch resolves, so one navigation is not proof either way.
 */
async function convergeNoMethods(driver: ParityDriver, expected: boolean): Promise<void> {
  await expect
    .poll(async () => {
      if ((await driver.seesNoAuthMethods()) !== expected) await driver.openSignInWithParams({});
      return driver.seesNoAuthMethods();
    }, SERVER_POLL)
    .toBe(expected);
}

/**
 * Seeded defaults for every flag the oracle scenarios flip. A crashed run
 * can leak a flip (its restore never runs) and the next run would then
 * restore the leaked value forever, so every flip starts by healing these
 * back to the seeded baseline.
 */
const FLAG_BASELINE: Record<string, string> = {
  EMAIL_HOST: "",
  ENABLE_EMAIL_PASSWORD: "1",
  ENABLE_MAGIC_LINK_LOGIN: "1",
  IS_GOOGLE_ENABLED: "0",
};

/** Restore the seeded flag baseline; returns the admin session. */
async function healBaseline(email: string, password: string): Promise<string> {
  const admin = await adminSignInSession(email, password);
  await patchInstanceConfig(FLAG_BASELINE, admin);
  return admin;
}

/**
 * Flip instance flags, run the body, then restore the previous values.
 * Signs in inside so a rejected admin session (shared-stack churn) is
 * retried once with a fresh sign-in instead of failing the scenario.
 */
async function withInstanceConfig(
  email: string,
  password: string,
  patch: Record<string, string>,
  body: (admin: string) => Promise<void>
): Promise<string> {
  const signIn = () => adminSignInSession(email, password);
  let admin = await signIn();
  await patchInstanceConfig(FLAG_BASELINE, admin);
  let before: Record<string, string>;
  try {
    before = await patchInstanceConfig(patch, admin);
  } catch (error) {
    if (!String(error).includes("401")) throw error;
    admin = await signIn();
    before = await patchInstanceConfig(patch, admin);
  }
  try {
    await body(admin);
  } finally {
    await patchInstanceConfig(before, admin);
  }
  return admin;
}

test(
  specTitle(["AUTH-001"], "wrong password shows a banner and keeps the password step"),
  { tag: specTags(["AUTH-001"]) },
  async ({ driver, seed }) => {
    // Heal the mail flag first so no leaked flip elsewhere can route this
    // address to the code step, then converge on the seeded routing.
    await healBaseline(seed.email, seed.password);
    await expect.poll(async () => (await emailCheck(seed.email)).status, SERVER_POLL).toBe("CREDENTIAL");
    await test.step("reach the password step", async () => {
      await driver.openEntry();
      await driver.submitEmail(seed.email);
      expect(await driver.authStep()).toBe("password");
    });

    await test.step("wrong password returns to the form with a banner", async () => {
      await driver.submitPassword("Wrong-Password-1");
      // The native POST ends in a full page load; poll until React renders.
      await expect.poll(() => driver.authStep()).toBe("password");
      await expect.poll(() => driver.bannerText()).toMatch(/authentication failed/i);
    });

    await test.step("the server created no session", async () => {
      await expect(signInSession(seed.email, "Wrong-Password-1")).rejects.toThrow();
    });

    await test.step("dismissing recovers the step and the right password still signs in", async () => {
      await driver.dismissBanner();
      expect(await driver.bannerText()).toBeNull();
      expect(await driver.authStep()).toBe("password");
      await driver.submitPassword(seed.password);
      await driver.page.waitForURL(/parity-ws/, { timeout: 30_000 });
    });
  }
);

test(
  specTitle(["AUTH-002"], "code step with resend cooldown and wrong-code recovery"),
  { tag: specTags(["AUTH-002"]) },
  async ({ driver, seed }) => {
    const admin = await withInstanceConfig(
      seed.email,
      seed.password,
      { EMAIL_HOST: "parity-mail.example" },
      async (admin) => {
        await expect
          .poll(async () => (await instanceConfig(undefined, admin)).is_smtp_configured, SERVER_POLL)
          .toBe(true);

        await test.step("code login is offered once mail is configured", async () => {
          await driver.openEntry();
          await driver.submitEmail(seed.email);
          expect(await driver.seesUniqueCodeButton()).toBe(true);
          await driver.requestUniqueCode();
          expect(await driver.authStep()).toBe("code");
        });

        await test.step("resend is throttled by a short cooldown", async () => {
          expect(await driver.resendCodeLabel()).toMatch(/resend/i);
          await driver.clickResendCode();
          await expect.poll(() => driver.resendCodeLabel()).toMatch(/resend in \d+ second/i);
        });

        await test.step("a wrong code returns a banner on the code step", async () => {
          await driver.submitCode("000000");
          await expect.poll(() => driver.authStep()).toBe("code");
          await expect.poll(() => driver.bannerText()).toMatch(/invalid magic code/i);
        });

        await test.step("the server created no session for the wrong code", async () => {
          const result = await nativeMagicSignIn(seed.email, "000000");
          expect(result.status).toBe(302);
          expect(result.location).toContain("error_code=5090");
          expect(result.sessionCookie).toBe(false);
        });
      }
    );
    await expect.poll(async () => (await instanceConfig(undefined, admin)).is_smtp_configured, SERVER_POLL).toBe(false);
  }
);

test(
  specTitle(["AUTH-003"], "disabled providers hidden, enabled provider starts its flow"),
  { tag: specTags(["AUTH-003"]) },
  async ({ driver, seed }) => {
    await test.step("no provider is offered on the seeded instance", async () => {
      await driver.openEntry();
      expect(await driver.providerSignInButtons()).toEqual([]);
      const config = await instanceConfig();
      expect(config.is_google_enabled).toBe(false);
      expect(config.is_github_enabled).toBe(false);
      expect(config.is_gitlab_enabled).toBe(false);
      expect(config.is_gitea_enabled).toBe(false);
    });

    await withInstanceConfig(seed.email, seed.password, { IS_GOOGLE_ENABLED: "1" }, async (admin) => {
      await expect.poll(async () => (await instanceConfig(undefined, admin)).is_google_enabled, SERVER_POLL).toBe(true);

      await test.step("the enabled provider appears while the rest stay hidden", async () => {
        // Re-navigate until the button converges: a throttled config fetch
        // can paint the card without the fresh flag.
        await expect
          .poll(async () => {
            const buttons = await driver.providerSignInButtons();
            if (JSON.stringify(buttons) !== JSON.stringify(["Google"])) await driver.openEntry();
            return driver.providerSignInButtons();
          }, SERVER_POLL)
          .toEqual(["Google"]);
      });

      await test.step("starting it navigates to the provider auth URL", async () => {
        const navigation = driver.page.waitForRequest(/\/auth\/google\//, { timeout: 30_000 });
        await driver.clickProviderButton("Google");
        const request = await navigation;
        expect(request.url()).toContain("/auth/google/");
      });
    });
  }
);

test(
  specTitle(["AUTH-004"], "email-first routing selects the path and clearing resets the card"),
  { tag: specTags(["AUTH-004"]) },
  async ({ driver, seed }) => {
    await healBaseline(seed.email, seed.password);
    await expect.poll(async () => (await emailCheck(seed.email)).status, SERVER_POLL).toBe("CREDENTIAL");
    await driver.openEntry();

    await test.step("a known address stays in sign-in mode", async () => {
      await driver.submitEmail(seed.email);
      expect(await driver.authStep()).toBe("password");
      expect(await driver.seesConfirmPassword()).toBe(false);
      expect(await driver.seesGenericSignInHeader()).toBe(true);
      const check = await emailCheck(seed.email);
      expect(check).toEqual({ existing: true, status: "CREDENTIAL" });
    });

    await test.step("clearing the address resets the card", async () => {
      await driver.clearEmail();
      expect(await driver.authStep()).toBe("email");
      expect(await driver.seesGenericSignInHeader()).toBe(true);
    });

    await test.step("an unknown address routes to the sign-up path", async () => {
      const fresh = `routing-${Date.now()}@example.com`;
      await driver.submitEmail(fresh);
      expect(await driver.authStep()).toBe("password");
      expect(await driver.seesConfirmPassword()).toBe(true);
      expect(await driver.seesGenericSignUpHeader()).toBe(true);
      const check = await emailCheck(fresh);
      expect(check).toEqual({ existing: false, status: "CREDENTIAL" });
    });
  }
);

test(
  specTitle(["AUTH-005"], "invitation link names the workspace on match, generic header otherwise"),
  { tag: specTags(["AUTH-005"]) },
  async ({ driver, seed }) => {
    const owner = await signInSession(seed.email, seed.password);
    const inviteEmail = `invitee-${Date.now()}@example.com`;
    const invitation = await createInvitation(seed.workspaceSlug, inviteEmail, owner);
    try {
      await test.step("the server fetch behind the header matches", async () => {
        const fetched = await singleInvitation(seed.workspaceSlug, invitation.id, undefined, owner);
        expect(fetched.email).toBe(inviteEmail);
        expect(fetched.workspaceName).toBe(seed.workspaceName);
      });

      await test.step("matching address shows the workspace header", async () => {
        await driver.openSignInWithParams({
          invitation_id: invitation.id,
          slug: seed.workspaceSlug,
          email: inviteEmail,
        });
        // The header fetch resolves after the form paints.
        await expect.poll(() => driver.seesWorkspaceInviteHeader(seed.workspaceName)).toBe(true);
      });

      await test.step("another address keeps the generic header", async () => {
        await driver.openSignInWithParams({
          invitation_id: invitation.id,
          slug: seed.workspaceSlug,
          email: "someone-else@example.com",
        });
        await expect.poll(() => driver.seesWorkspaceInviteHeader(seed.workspaceName)).toBe(false);
        await expect.poll(() => driver.seesGenericSignInHeader()).toBe(true);
      });
    } finally {
      await deleteInvitation(seed.workspaceSlug, invitation.id, owner);
    }

    await test.step("the invitation is gone afterwards", async () => {
      await expect(singleInvitation(seed.workspaceSlug, invitation.id, undefined, owner)).rejects.toThrow();
    });
  }
);

test(
  specTitle(["AUTH-006"], "error links land on the fixing step with a dismissible banner"),
  { tag: specTags(["AUTH-006"]) },
  async ({ driver }) => {
    const cases: { code: string; step: "email" | "password" | "code"; banner: RegExp }[] = [
      { code: "5065", step: "password", banner: /authentication failed/i },
      { code: "5090", step: "code", banner: /invalid magic code/i },
      { code: "5100", step: "code", banner: /expired magic code/i },
      { code: "5060", step: "email", banner: /no account found/i },
      { code: "5900", step: "email", banner: /rate limit exceeded/i },
      { code: "5015", step: "email", banner: /sign up disabled/i },
    ];
    for (const { code, step, banner } of cases) {
      await test.step(`error ${code} recovers on the ${step} step`, async () => {
        await driver.openSignInWithParams({ error_code: code });
        await expect.poll(() => driver.authStep()).toBe(step);
        await expect.poll(() => driver.bannerText()).toMatch(banner);
        await driver.dismissBanner();
        await expect.poll(() => driver.bannerText()).toBeNull();
        await expect.poll(() => driver.authStep()).toBe(step);
      });
    }

    await test.step("an unknown error code raises no banner", async () => {
      await driver.openSignInWithParams({ error_code: "9999" });
      await expect.poll(() => driver.authStep()).toBe("email");
      expect(await driver.bannerText()).toBeNull();
    });
  }
);

test(
  specTitle(["AUTH-007"], "no-methods card replaces the form and points at the administrator"),
  { tag: specTags(["AUTH-007"]) },
  async ({ driver, seed }) => {
    const admin = await withInstanceConfig(
      seed.email,
      seed.password,
      { ENABLE_EMAIL_PASSWORD: "0", ENABLE_MAGIC_LINK_LOGIN: "0" },
      async (admin) => {
        await expect
          .poll(async () => (await instanceConfig(undefined, admin)).is_email_password_enabled, SERVER_POLL)
          .toBe(false);

        await test.step("the card explains and offers no form", async () => {
          await driver.openSignInWithParams({});
          await convergeNoMethods(driver, true);
          await expect.poll(() => driver.authStep()).toBe("unavailable");
        });

        await test.step("the server agrees both email methods are off", async () => {
          const config = await instanceConfig(undefined, admin);
          expect(config.is_email_password_enabled).toBe(false);
          expect(config.is_magic_login_enabled).toBe(false);
        });
      }
    );

    await test.step("restoring brings the form back", async () => {
      await expect
        .poll(async () => (await instanceConfig(undefined, admin)).is_email_password_enabled, SERVER_POLL)
        .toBe(true);
      await driver.openSignInWithParams({});
      await convergeNoMethods(driver, false);
      await expect.poll(() => driver.authStep()).toBe("email");
    });
  }
);

test(
  specTitle(["AUTH-008"], "mail-less mode explains reset, hides code login, relabels submit"),
  { tag: specTags(["AUTH-008"]) },
  async ({ driver, seed }) => {
    await healBaseline(seed.email, seed.password);
    await test.step("the server reports mail unconfigured", async () => {
      // Converge on the seeded default first: a concurrent flip elsewhere
      // would otherwise read through here.
      await expect.poll(async () => (await instanceConfig()).is_smtp_configured, SERVER_POLL).toBe(false);
      await expect(magicGenerate(seed.email)).rejects.toThrow(/5025/);
    });

    await test.step("the password step reflects the mail-less mode", async () => {
      await driver.openEntry();
      await driver.submitEmail(seed.email);
      expect(await driver.authStep()).toBe("password");
      expect(await driver.forgotPasswordEntry()).toBe("popover");
      const explanation = await driver.forgotPasswordPopoverText();
      expect(explanation).toMatch(/smtp/i);
      expect(await driver.seesUniqueCodeButton()).toBe(false);
      expect(await driver.passwordPrimaryButtonLabel()).toBe("Go to workspace");
    });
  }
);
