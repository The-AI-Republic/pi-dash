// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-452): spaces landing auth, rows SPC-058
// (email-first step with inline format validation), SPC-059 (known vs
// new routing by instance config), SPC-060 (code step: resend cooldown,
// wrong/missing/exhausted codes, valid-code sign-in), SPC-061 (password
// step: visibility toggles, strength meter, switch-to-code, CSRF
// submit), SPC-062 (provider buttons), SPC-063 (failure banner and
// error-coded links), SPC-064 (mode headings and consent line), green on
// the old space app first. The same file must go green on apps/web_new
// once the spaces auth area lands there.
//
// Observed behavior notes (this checkout): the landing card is
// email-first; a server check routes known addresses to sign-in and new
// ones to sign-up, then mail configuration picks the code step and the
// password flag the password step. Code and password submits are native
// CSRF form POSTs whose redirects target the /spaces/ base, so under the
// parity serving (space at the proxy root) both success and failure land
// on the catch-all page carrying the error_code; opening a coded link at
// the landing root lands on the fixing step with a dismissible banner.
// The seeded stack is mail-less (no SMTP host): code scenarios flip
// EMAIL_HOST in-spec and restore it after. One-time codes live in the
// stack redis (a few generates per address per 10 minutes server-side),
// so each code scenario uses a fresh address and reads the live code
// server-side. Namespaced runs need PARITY_NS plus PARITY_API_CONTAINER
// and PARITY_REDIS_CONTAINER pointing at their own stack (the docker
// helpers default to the shared parity19 names).
import { test, expect } from "../fixtures";
import {
  adminSignInSession,
  instanceConfig,
  nativeSpaceMagicAuth,
  nativeSpacePasswordAuth,
  patchInstanceConfig,
  spaceDropMagicCode,
  spaceEmailCheck,
  spaceMagicCode,
  spaceMagicGenerate,
  uniqueEmail,
  userFacts,
} from "../helpers/api";
import { parityApiUrl } from "../helpers/parity-env";
import { specTags, specTitle } from "../helpers/tags";
import type { ParityDriver } from "../drivers/index";

/**
 * Poll preset for reads that hit the server. The stack throttles
 * anonymous calls per minute per IP, so server-backed polls wait two
 * seconds between attempts instead of hammering. UI-only polls keep
 * Playwright's default pace.
 */
const SERVER_POLL = { timeout: 60_000, intervals: [2_000] };

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
  IS_GITHUB_ENABLED: "0",
  IS_GITLAB_ENABLED: "0",
  IS_GITEA_ENABLED: "0",
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

/**
 * Submit an address until the card converges on the expected step. A
 * fresh load can route off a stale config read, so a wrong landing
 * re-opens the card and resubmits (a few rounds at most) instead of
 * failing the scenario.
 */
async function convergeStep(driver: ParityDriver, email: string, expected: "password" | "code"): Promise<void> {
  await driver.spaceAuthOpenLanding();
  for (let round = 0; round < 4; round++) {
    await driver.spaceAuthSubmitEmail(email);
    try {
      await expect.poll(() => driver.spaceAuthStep(), { timeout: 15_000 }).toBe(expected);
      return;
    } catch {
      await driver.spaceAuthOpenLanding();
    }
  }
  await expect.poll(() => driver.spaceAuthStep(), SERVER_POLL).toBe(expected);
}

/**
 * Post a spaces native form without the CSRF token; resolves with the
 * refusal page body. The CSRF middleware refuses token-less posts with a
 * dedicated verification-failed page (served with a 200 status, no
 * redirect and no session) instead of running the form.
 */
async function nativePostWithoutCsrf(path: string, fields: Record<string, string>): Promise<string> {
  const apiBase = parityApiUrl(process.env);
  const tokenRes = await fetch(`${apiBase}/auth/get-csrf-token/`);
  const rawCookies = tokenRes.headers.getSetCookie?.() ?? [];
  const cookie = rawCookies
    .map((line) => line.split(";", 1)[0]?.trim())
    .filter((pair) => pair !== undefined && pair.length > 0)
    .join("; ");
  const res = await fetch(`${apiBase}${path}`, {
    method: "POST",
    headers: { "content-type": "application/x-www-form-urlencoded", cookie },
    body: new URLSearchParams(fields),
    redirect: "manual",
  });
  expect(res.status).toBe(200);
  expect(res.headers.get("location")).toBeNull();
  return res.text();
}

test(
  specTitle(["SPC-058"], "malformed addresses are blocked inline while valid ones advance"),
  { tag: specTags(["SPC-058"]) },
  async ({ driver, seed }) => {
    await healBaseline(seed.email, seed.password);
    await expect.poll(async () => (await instanceConfig()).is_smtp_configured, SERVER_POLL).toBe(false);

    await test.step("an empty address cannot continue", async () => {
      await driver.spaceAuthOpenLanding();
      await expect.poll(() => driver.spaceAuthStep()).toBe("email");
      expect(await driver.spaceAuthEmailSubmitEnabled()).toBe(false);
      expect(await driver.spaceAuthEmailError()).toBeNull();
    });

    await test.step("a malformed address is flagged after blur", async () => {
      await driver.spaceAuthFillEmail("not-an-email");
      expect(await driver.spaceAuthEmailSubmitEnabled()).toBe(false);
      expect(await driver.spaceAuthEmailError()).toBeNull();
      await driver.spaceAuthBlurEmail();
      // web_new carries its own validation copy (independence rule).
      const malformedCopy = driver.target === "web_new" ? "That email address doesn't look right." : "Email is invalid";
      await expect.poll(() => driver.spaceAuthEmailError()).toBe(malformedCopy);
      expect(await driver.spaceAuthEmailSubmitEnabled()).toBe(false);
    });

    await test.step("the server rejects the malformed address too", async () => {
      await expect(spaceEmailCheck("not-an-email")).rejects.toThrow(/5005/);
    });

    const fresh = uniqueEmail("spc058");
    await test.step("a valid address clears the error and advances", async () => {
      await driver.spaceAuthFillEmail(fresh);
      expect(await driver.spaceAuthEmailError()).toBeNull();
      expect(await driver.spaceAuthEmailSubmitEnabled()).toBe(true);
      await driver.spaceAuthSubmitEmail(fresh);
      await expect.poll(() => driver.spaceAuthStep()).toBe("password");
      expect(await driver.spaceAuthSeesConfirmPassword()).toBe(true);
      expect(await driver.spaceAuthMode()).toBe("sign-up");
    });

    await test.step("the server checked the address without creating anything", async () => {
      expect(await spaceEmailCheck(fresh)).toEqual({ existing: false, status: "CREDENTIAL" });
      expect((await userFacts(fresh)).exists).toBe(false);
    });
  }
);

test(
  specTitle(["SPC-059"], "known addresses sign in and new addresses sign up through the password step"),
  { tag: specTags(["SPC-059"]) },
  async ({ driver, seed }) => {
    await healBaseline(seed.email, seed.password);
    await expect.poll(async () => (await spaceEmailCheck(seed.email)).status, SERVER_POLL).toBe("CREDENTIAL");

    await test.step("a known address stays in sign-in mode", async () => {
      await driver.spaceAuthOpenLanding();
      await driver.spaceAuthSubmitEmail(seed.email);
      await expect.poll(() => driver.spaceAuthStep()).toBe("password");
      expect(await driver.spaceAuthSeesConfirmPassword()).toBe(false);
      expect(await driver.spaceAuthMode()).toBe("sign-in");
      expect(await spaceEmailCheck(seed.email)).toEqual({ existing: true, status: "CREDENTIAL" });
    });

    await test.step("clearing the address resets the step but keeps the mode", async () => {
      await driver.spaceAuthClearEmail();
      await expect.poll(() => driver.spaceAuthStep()).toBe("email");
      expect(await driver.spaceAuthMode()).toBe("sign-in");
    });

    const fresh = uniqueEmail("spc059");
    await test.step("an unknown address routes to the sign-up path", async () => {
      await driver.spaceAuthSubmitEmail(fresh);
      await expect.poll(() => driver.spaceAuthStep()).toBe("password");
      expect(await driver.spaceAuthSeesConfirmPassword()).toBe(true);
      expect(await driver.spaceAuthMode()).toBe("sign-up");
      expect(await spaceEmailCheck(fresh)).toEqual({ existing: false, status: "CREDENTIAL" });
    });
  }
);

test(
  specTitle(["SPC-059"], "the code step follows the mail configuration"),
  { tag: specTags(["SPC-059"]) },
  async ({ driver, seed }) => {
    const fresh = uniqueEmail("spc059code");
    const autoset = uniqueEmail("spc059auto");
    await withInstanceConfig(seed.email, seed.password, { EMAIL_HOST: "parity-mail.example" }, async (admin) => {
      await expect
        .poll(async () => (await instanceConfig(undefined, admin)).is_smtp_configured, SERVER_POLL)
        .toBe(true);

      await test.step("a new address lands on the code step in sign-up mode", async () => {
        await convergeStep(driver, fresh, "code");
        expect(await driver.spaceAuthMode()).toBe("sign-up");
        expect(await spaceEmailCheck(fresh)).toEqual({ existing: false, status: "MAGIC_CODE" });
      });

      await test.step("a password account still lands on the password step", async () => {
        await convergeStep(driver, seed.email, "password");
        expect(await driver.spaceAuthMode()).toBe("sign-in");
        expect(await spaceEmailCheck(seed.email)).toEqual({ existing: true, status: "CREDENTIAL" });
      });

      await test.step("a code-created account lands on the code step in sign-in mode", async () => {
        await spaceMagicGenerate(autoset);
        const code = await spaceMagicCode(autoset);
        expect(code).not.toBeNull();
        const created = await nativeSpaceMagicAuth(autoset, code ?? "", "sign-up");
        expect(created.status).toBe(302);
        expect(created.sessionCookie).toBe(true);
        const facts = await userFacts(autoset);
        expect(facts.exists).toBe(true);
        expect(facts.passwordAutoset).toBe(true);
        await convergeStep(driver, autoset, "code");
        expect(await driver.spaceAuthMode()).toBe("sign-in");
        expect(await spaceEmailCheck(autoset)).toEqual({ existing: true, status: "MAGIC_CODE" });
      });
    });
  }
);

test(
  specTitle(["SPC-059"], "bug: NEWFRONT-462 no enabled method misleads instead of explaining itself"),
  { tag: specTags(["SPC-059"]) },
  async ({ driver, seed }) => {
    const fresh = uniqueEmail("spc059none");
    const admin = await withInstanceConfig(
      seed.email,
      seed.password,
      { ENABLE_EMAIL_PASSWORD: "0", ENABLE_MAGIC_LINK_LOGIN: "0" },
      async (admin) => {
        await expect
          .poll(async () => (await instanceConfig(undefined, admin)).is_email_password_enabled, SERVER_POLL)
          .toBe(false);
        await expect
          .poll(async () => (await instanceConfig(undefined, admin)).is_magic_login_enabled, SERVER_POLL)
          .toBe(false);

        await test.step("a known address stays put with a banner", async () => {
          await driver.spaceAuthOpenLanding();
          await driver.spaceAuthSubmitEmail(seed.email);
          await expect.poll(() => driver.spaceAuthStep()).toBe("email");
          // Intended on web_new (bug NEWFRONT-462 fixed): the banner
          // explains that no sign-in method is enabled instead of
          // blaming the address.
          const noMethodBanner = driver.target === "web_new" ? /isn't available on this instance/i : /invalid email/i;
          await expect.poll(() => driver.spaceAuthBanner()).toMatch(noMethodBanner);
          await driver.spaceAuthDismissBanner();
          await expect.poll(() => driver.spaceAuthBanner()).toBeNull();
          expect(await driver.spaceAuthStep()).toBe("email");
        });

        await test.step("a new address stays put with no banner", async () => {
          await driver.spaceAuthOpenLanding();
          await driver.spaceAuthSubmitEmail(fresh);
          await expect.poll(() => driver.spaceAuthStep()).toBe("email");
          // Intended on web_new (bug NEWFRONT-462 fixed): new addresses
          // get the same explanation as known ones instead of silence.
          if (driver.target === "web_new") {
            await expect.poll(() => driver.spaceAuthBanner()).toMatch(/isn't available on this instance/i);
          } else {
            expect(await driver.spaceAuthBanner()).toBeNull();
          }
        });
      }
    );

    await test.step("restoring the methods brings the routing back", async () => {
      await expect
        .poll(async () => (await instanceConfig(undefined, admin)).is_email_password_enabled, SERVER_POLL)
        .toBe(true);
      await convergeStep(driver, seed.email, "password");
    });
  }
);

test(
  specTitle(["SPC-060"], "resend cooldown paces code delivery"),
  { tag: specTags(["SPC-060"]) },
  async ({ driver, seed }) => {
    const fresh = uniqueEmail("spc060re");
    await withInstanceConfig(seed.email, seed.password, { EMAIL_HOST: "parity-mail.example" }, async (admin) => {
      await expect
        .poll(async () => (await instanceConfig(undefined, admin)).is_smtp_configured, SERVER_POLL)
        .toBe(true);
      await convergeStep(driver, fresh, "code");

      await test.step("resending starts a short cooldown", async () => {
        // web_new carries its own resend copy (independence rule) and
        // shows a sending state while the first generate is still out,
        // so the idle control is polled for instead of read once.
        if (driver.target === "web_new") {
          await expect
            .poll(() => driver.spaceAuthCodeResendState(), SERVER_POLL)
            .toEqual({ disabled: false, label: "Send another code" });
        } else {
          expect(await driver.spaceAuthCodeResendState()).toEqual({ disabled: false, label: "Resend" });
        }
        await driver.spaceAuthClickResendCode();
        const cooldownLabel = driver.target === "web_new" ? /send another code in \d+s/i : /resend in \d+s/i;
        await expect
          .poll(() => driver.spaceAuthCodeResendState(), SERVER_POLL)
          .toEqual({
            disabled: true,
            label: expect.stringMatching(cooldownLabel),
          });
      });

      await test.step("the cooldown lapses back to resend", async () => {
        const idleLabel = driver.target === "web_new" ? "Send another code" : "Resend";
        await expect
          .poll(() => driver.spaceAuthCodeResendState(), SERVER_POLL)
          .toEqual({
            disabled: false,
            label: idleLabel,
          });
      });

      await test.step("the server holds a live code for the address", async () => {
        await expect.poll(() => spaceMagicCode(fresh), SERVER_POLL).not.toBeNull();
      });
    });
  }
);

test(
  specTitle(["SPC-060"], "wrong and missing codes return guidance on the code step"),
  { tag: specTags(["SPC-060"]) },
  async ({ driver, seed }) => {
    const wrong = uniqueEmail("spc060bad");
    const missing = uniqueEmail("spc060gone");
    await withInstanceConfig(seed.email, seed.password, { EMAIL_HOST: "parity-mail.example" }, async (admin) => {
      await expect
        .poll(async () => (await instanceConfig(undefined, admin)).is_smtp_configured, SERVER_POLL)
        .toBe(true);

      await test.step("a wrong code redirects with the invalid-code error", async () => {
        await convergeStep(driver, wrong, "code");
        await driver.spaceAuthSubmitCode("000000");
        await expect.poll(() => driver.currentPath(), SERVER_POLL).toContain("error_code=5092");
      });

      await test.step("the server created no session for the wrong code", async () => {
        const result = await nativeSpaceMagicAuth(wrong, "000000", "sign-up");
        expect(result.status).toBe(302);
        expect(result.location).toContain("error_code=5092");
        expect(result.sessionCookie).toBe(false);
      });

      await test.step("the coded link lands back on the code step with guidance", async () => {
        await driver.spaceAuthOpenLanding({ errorCode: "5092" });
        await expect.poll(() => driver.spaceAuthStep()).toBe("code");
        // web_new carries its own banner copy (independence rule).
        const invalidCodeBanner = driver.target === "web_new" ? /code isn't valid/i : /invalid magic code/i;
        await expect.poll(() => driver.spaceAuthBanner()).toMatch(invalidCodeBanner);
      });

      await test.step("a missing code redirects with the expired-code error", async () => {
        await convergeStep(driver, missing, "code");
        await spaceDropMagicCode(missing);
        await driver.spaceAuthSubmitCode("123456");
        await expect.poll(() => driver.currentPath(), SERVER_POLL).toContain("error_code=5097");
      });

      await test.step("the server created no session for the missing code", async () => {
        const result = await nativeSpaceMagicAuth(missing, "123456", "sign-up");
        expect(result.status).toBe(302);
        expect(result.location).toContain("error_code=5097");
        expect(result.sessionCookie).toBe(false);
      });

      await test.step("the expired link lands back on the code step with guidance", async () => {
        await driver.spaceAuthOpenLanding({ errorCode: "5097" });
        await expect.poll(() => driver.spaceAuthStep()).toBe("code");
        const expiredCodeBanner = driver.target === "web_new" ? /code has expired/i : /expired magic code/i;
        await expect.poll(() => driver.spaceAuthBanner()).toMatch(expiredCodeBanner);
      });
    });
  }
);

test(
  specTitle(["SPC-060"], "resending past the attempt cap raises a banner on the code step"),
  { tag: specTags(["SPC-060"]) },
  async ({ driver, seed }) => {
    const fresh = uniqueEmail("spc060cap");
    await withInstanceConfig(seed.email, seed.password, { EMAIL_HOST: "parity-mail.example" }, async (admin) => {
      await expect
        .poll(async () => (await instanceConfig(undefined, admin)).is_smtp_configured, SERVER_POLL)
        .toBe(true);
      await convergeStep(driver, fresh, "code");
      expect(await driver.spaceAuthBanner()).toBeNull();

      await test.step("three more generates approach the cap", async () => {
        await spaceMagicGenerate(fresh);
        await spaceMagicGenerate(fresh);
        await spaceMagicGenerate(fresh);
      });

      await test.step("the resend past the cap explains itself on the code step", async () => {
        await driver.spaceAuthClickResendCode();
        const capBanner = driver.target === "web_new" ? /code has expired/i : /expired magic code/i;
        await expect.poll(() => driver.spaceAuthBanner(), SERVER_POLL).toMatch(capBanner);
        expect(await driver.spaceAuthStep()).toBe("code");
      });

      await test.step("the server reports the cap as exhausted attempts", async () => {
        await expect(spaceMagicGenerate(fresh)).rejects.toThrow(/5102/);
      });
    });
  }
);

test(
  specTitle(["SPC-060"], "a valid code signs the visitor in through the CSRF form"),
  { tag: specTags(["SPC-060"]) },
  async ({ driver, seed }) => {
    const fresh = uniqueEmail("spc060ok");
    await withInstanceConfig(seed.email, seed.password, { EMAIL_HOST: "parity-mail.example" }, async (admin) => {
      await expect
        .poll(async () => (await instanceConfig(undefined, admin)).is_smtp_configured, SERVER_POLL)
        .toBe(true);
      await convergeStep(driver, fresh, "code");

      const code = await spaceMagicCode(fresh);
      expect(code).not.toBeNull();

      await test.step("the code form refuses a token-less post", async () => {
        const refusal = await nativePostWithoutCsrf("/auth/spaces/magic-sign-up/", { email: fresh, code: code ?? "" });
        expect(refusal).toContain("CSRF Verification Failed");
        expect((await userFacts(fresh)).exists).toBe(false);
      });

      await test.step("submitting the live code succeeds without an error code", async () => {
        await driver.spaceAuthSubmitCode(code ?? "");
        // web_new reports the old app's mount-free addresses, so the
        // spaces base reads as the bare root: pin the real pathname.
        if (driver.target === "web_new") {
          await expect.poll(() => new URL(driver.page.url()).pathname, SERVER_POLL).toMatch(/^\/spaces\/?$/);
        } else {
          await expect.poll(() => driver.currentPath(), SERVER_POLL).toContain("/spaces");
        }
        expect(new URL(driver.page.url()).searchParams.has("error_code")).toBe(false);
      });

      await test.step("the browser now holds a session", async () => {
        await driver.spaceAuthOpenLanding();
        await expect.poll(() => driver.spaceAuthStep(), SERVER_POLL).toBe("signed-in");
      });

      await test.step("the server created a code account and consumed the code", async () => {
        const facts = await userFacts(fresh);
        expect(facts.exists).toBe(true);
        expect(facts.passwordAutoset).toBe(true);
        const replay = await nativeSpaceMagicAuth(fresh, code ?? "", "sign-in");
        expect(replay.location).toContain("error_code=5095");
        expect(replay.sessionCookie).toBe(false);
      });
    });
  }
);

test(
  specTitle(["SPC-061"], "visibility toggles reveal each password field independently"),
  { tag: specTags(["SPC-061"]) },
  async ({ driver, seed }) => {
    await healBaseline(seed.email, seed.password);
    await expect.poll(async () => (await spaceEmailCheck(seed.email)).status, SERVER_POLL).toBe("CREDENTIAL");
    const fresh = uniqueEmail("spc061eye");
    await driver.spaceAuthOpenLanding();
    await driver.spaceAuthSubmitEmail(fresh);
    await expect.poll(() => driver.spaceAuthStep()).toBe("password");

    await test.step("each toggle flips only its own field", async () => {
      expect(await driver.spaceAuthPasswordFieldTypes()).toEqual({ password: "password", confirm: "password" });
      await driver.spaceAuthTogglePasswordVisibility("password");
      expect(await driver.spaceAuthPasswordFieldTypes()).toEqual({ password: "text", confirm: "password" });
      await driver.spaceAuthTogglePasswordVisibility("password");
      expect(await driver.spaceAuthPasswordFieldTypes()).toEqual({ password: "password", confirm: "password" });
      await driver.spaceAuthTogglePasswordVisibility("confirm");
      expect(await driver.spaceAuthPasswordFieldTypes()).toEqual({ password: "password", confirm: "text" });
      await driver.spaceAuthTogglePasswordVisibility("confirm");
      expect(await driver.spaceAuthPasswordFieldTypes()).toEqual({ password: "password", confirm: "password" });
    });

    await test.step("viewing the form created no account", async () => {
      expect((await userFacts(fresh)).exists).toBe(false);
    });
  }
);

test(
  specTitle(["SPC-061"], "sign-up gates submit on strength and confirmation"),
  { tag: specTags(["SPC-061"]) },
  async ({ driver, seed }) => {
    await healBaseline(seed.email, seed.password);
    await expect.poll(async () => (await spaceEmailCheck(seed.email)).status, SERVER_POLL).toBe("CREDENTIAL");
    const fresh = uniqueEmail("spc061pw");
    await driver.spaceAuthOpenLanding();
    await driver.spaceAuthSubmitEmail(fresh);
    await expect.poll(() => driver.spaceAuthStep()).toBe("password");

    await test.step("weak passwords surface the strength meter and stay disabled", async () => {
      // web_new carries its own meter copy (independence rule).
      const shortCopy = driver.target === "web_new" ? "Use 8 or more characters." : "Password is too short";
      const weakCopy =
        driver.target === "web_new" ? "Mix upper-case, lower-case, numbers and symbols." : "Password is weak";
      await driver.spaceAuthFillPassword("abc");
      expect(await driver.spaceAuthStrengthMeterVisible()).toBe(true);
      expect(await driver.spaceAuthStrengthMessage()).toBe(shortCopy);
      expect(await driver.spaceAuthPasswordSubmitEnabled()).toBe(false);
      await driver.spaceAuthFillPassword("abcdefgh");
      expect(await driver.spaceAuthStrengthMeterVisible()).toBe(true);
      expect(await driver.spaceAuthStrengthMessage()).toBe(weakCopy);
      expect(await driver.spaceAuthPasswordSubmitEnabled()).toBe(false);
    });

    await test.step("a strong password hides the meter but still needs its confirmation", async () => {
      await driver.spaceAuthFillPassword("Str0ng!Pass1");
      expect(await driver.spaceAuthStrengthMeterVisible()).toBe(false);
      expect(await driver.spaceAuthStrengthMessage()).toBeNull();
      expect(await driver.spaceAuthPasswordSubmitEnabled()).toBe(false);
    });

    await test.step("a mismatched confirmation errors and only a match enables submit", async () => {
      await driver.spaceAuthFillPassword("Str0ng!Pass1", "Str0ng!Pass2");
      await expect.poll(() => driver.spaceAuthPasswordMismatchError()).not.toBeNull();
      expect(await driver.spaceAuthPasswordSubmitEnabled()).toBe(false);
      await driver.spaceAuthFillPassword("Str0ng!Pass1", "Str0ng!Pass1");
      expect(await driver.spaceAuthPasswordMismatchError()).toBeNull();
      expect(await driver.spaceAuthPasswordSubmitEnabled()).toBe(true);
    });

    await test.step("sign-in shows no meter for the same weak input", async () => {
      await driver.spaceAuthOpenLanding();
      await driver.spaceAuthSubmitEmail(seed.email);
      await expect.poll(() => driver.spaceAuthStep()).toBe("password");
      await driver.spaceAuthFillPassword("abc");
      expect(await driver.spaceAuthStrengthMeterVisible()).toBe(false);
      expect(await driver.spaceAuthPasswordSubmitEnabled()).toBe(true);
    });

    await test.step("the draft passwords created no account", async () => {
      expect((await userFacts(fresh)).exists).toBe(false);
    });
  }
);

test(
  specTitle(["SPC-061"], "sign-in offers the code alternative only when mail is configured"),
  { tag: specTags(["SPC-061"]) },
  async ({ driver, seed }) => {
    await healBaseline(seed.email, seed.password);
    await expect.poll(async () => (await instanceConfig()).is_smtp_configured, SERVER_POLL).toBe(false);

    await test.step("mail-less sign-in has no code switch and a workspace-labeled submit", async () => {
      await driver.spaceAuthOpenLanding();
      await driver.spaceAuthSubmitEmail(seed.email);
      await expect.poll(() => driver.spaceAuthStep()).toBe("password");
      expect(await driver.spaceAuthSeesSwitchToCode()).toBe(false);
      // web_new labels its password submit "Sign in" in both mail states.
      const mailLessLabel = driver.target === "web_new" ? "Sign in" : "Go to workspace";
      expect(await driver.spaceAuthPasswordPrimaryLabel()).toBe(mailLessLabel);
    });

    await withInstanceConfig(seed.email, seed.password, { EMAIL_HOST: "parity-mail.example" }, async (admin) => {
      await expect
        .poll(async () => (await instanceConfig(undefined, admin)).is_smtp_configured, SERVER_POLL)
        .toBe(true);

      await test.step("configured sign-in offers the switch and a continue-labeled submit", async () => {
        await convergeStep(driver, seed.email, "password");
        expect(await driver.spaceAuthSeesSwitchToCode()).toBe(true);
        const configuredLabel = driver.target === "web_new" ? "Sign in" : "Continue";
        expect(await driver.spaceAuthPasswordPrimaryLabel()).toBe(configuredLabel);
      });

      await test.step("the switch moves to the code step and delivers a code", async () => {
        await driver.spaceAuthSwitchToCode();
        expect(await driver.spaceAuthStep()).toBe("code");
        await expect.poll(() => spaceMagicCode(seed.email), SERVER_POLL).not.toBeNull();
      });
    });
  }
);

test(
  specTitle(["SPC-061"], "password sign-in rejects wrong credentials and accepts the right ones"),
  { tag: specTags(["SPC-061"]) },
  async ({ driver, seed }) => {
    await healBaseline(seed.email, seed.password);
    await expect.poll(async () => (await spaceEmailCheck(seed.email)).status, SERVER_POLL).toBe("CREDENTIAL");

    await test.step("a wrong password redirects with the sign-in failure error", async () => {
      await driver.spaceAuthOpenLanding();
      await driver.spaceAuthSubmitEmail(seed.email);
      await expect.poll(() => driver.spaceAuthStep()).toBe("password");
      await driver.spaceAuthSubmitPassword("Wrong-Password-1");
      await expect.poll(() => driver.currentPath(), SERVER_POLL).toContain("error_code=5065");
    });

    await test.step("the server created no session for the wrong password", async () => {
      const result = await nativeSpacePasswordAuth(seed.email, "Wrong-Password-1", "sign-in");
      expect(result.status).toBe(302);
      expect(result.location).toContain("error_code=5065");
      expect(result.sessionCookie).toBe(false);
    });

    await test.step("the right password succeeds without an error code", async () => {
      await driver.spaceAuthOpenLanding();
      await driver.spaceAuthSubmitEmail(seed.email);
      await expect.poll(() => driver.spaceAuthStep()).toBe("password");
      await driver.spaceAuthSubmitPassword(seed.password);
      // web_new reports the old app's mount-free addresses, so the
      // spaces base reads as the bare root: pin the real pathname.
      if (driver.target === "web_new") {
        await expect.poll(() => new URL(driver.page.url()).pathname, SERVER_POLL).toMatch(/^\/spaces\/?$/);
      } else {
        await expect.poll(() => driver.currentPath(), SERVER_POLL).toContain("/spaces");
      }
      expect(new URL(driver.page.url()).searchParams.has("error_code")).toBe(false);
    });

    await test.step("the browser now holds a session", async () => {
      await driver.spaceAuthOpenLanding();
      await expect.poll(() => driver.spaceAuthStep(), SERVER_POLL).toBe("signed-in");
    });

    await test.step("the server authenticates the same credentials natively", async () => {
      const result = await nativeSpacePasswordAuth(seed.email, seed.password, "sign-in");
      expect(result.status).toBe(302);
      expect(result.location).not.toContain("error_code=");
      expect(result.sessionCookie).toBe(true);
    });
  }
);

test(
  specTitle(["SPC-061"], "password sign-up creates the account through the CSRF form"),
  { tag: specTags(["SPC-061"]) },
  async ({ driver, seed }) => {
    await healBaseline(seed.email, seed.password);
    await expect.poll(async () => (await spaceEmailCheck(seed.email)).status, SERVER_POLL).toBe("CREDENTIAL");
    const fresh = uniqueEmail("spc061new");
    const password = "Str0ng!Pass1";

    await test.step("the password form refuses a token-less post", async () => {
      const refusal = await nativePostWithoutCsrf("/auth/spaces/sign-up/", { email: fresh, password });
      expect(refusal).toContain("CSRF Verification Failed");
      expect((await userFacts(fresh)).exists).toBe(false);
    });

    await test.step("submitting matching strong passwords succeeds without an error code", async () => {
      await driver.spaceAuthOpenLanding();
      await driver.spaceAuthSubmitEmail(fresh);
      await expect.poll(() => driver.spaceAuthStep()).toBe("password");
      await driver.spaceAuthSubmitPassword(password, password);
      // web_new reports the old app's mount-free addresses, so the
      // spaces base reads as the bare root: pin the real pathname.
      if (driver.target === "web_new") {
        await expect.poll(() => new URL(driver.page.url()).pathname, SERVER_POLL).toMatch(/^\/spaces\/?$/);
      } else {
        await expect.poll(() => driver.currentPath(), SERVER_POLL).toContain("/spaces");
      }
      expect(new URL(driver.page.url()).searchParams.has("error_code")).toBe(false);
    });

    await test.step("the browser now holds a session", async () => {
      await driver.spaceAuthOpenLanding();
      await expect.poll(() => driver.spaceAuthStep(), SERVER_POLL).toBe("signed-in");
    });

    await test.step("the server created a password account", async () => {
      const facts = await userFacts(fresh);
      expect(facts.exists).toBe(true);
      expect(facts.passwordAutoset).toBe(false);
      const again = await nativeSpacePasswordAuth(fresh, password, "sign-up");
      expect(again.location).toContain("error_code=5030");
      expect(again.sessionCookie).toBe(false);
    });

    await test.step("the new account routes as known afterwards", async () => {
      await driver.page.context().clearCookies();
      await driver.spaceAuthOpenLanding();
      await driver.spaceAuthSubmitEmail(fresh);
      await expect.poll(() => driver.spaceAuthStep()).toBe("password");
      expect(await driver.spaceAuthSeesConfirmPassword()).toBe(false);
      expect(await driver.spaceAuthMode()).toBe("sign-in");
    });
  }
);

test(
  specTitle(["SPC-062"], "provider buttons follow the instance provider flags"),
  { tag: specTags(["SPC-062"]) },
  async ({ driver, seed }) => {
    await healBaseline(seed.email, seed.password);

    await test.step("no provider is offered on the seeded instance", async () => {
      const config = await instanceConfig();
      expect(config.is_google_enabled).toBe(false);
      expect(config.is_github_enabled).toBe(false);
      expect(config.is_gitlab_enabled).toBe(false);
      expect(config.is_gitea_enabled).toBe(false);
      await driver.spaceAuthOpenLanding();
      await expect.poll(() => driver.spaceAuthStep()).toBe("email");
      expect(await driver.spaceAuthProviderButtons()).toEqual([]);
      expect(await driver.spaceAuthProvidersDividerVisible()).toBe(false);
    });

    const admin = await withInstanceConfig(
      seed.email,
      seed.password,
      { IS_GOOGLE_ENABLED: "1", IS_GITHUB_ENABLED: "1" },
      async (admin) => {
        await expect
          .poll(async () => (await instanceConfig(undefined, admin)).is_google_enabled, SERVER_POLL)
          .toBe(true);

        await test.step("the enabled providers appear while the rest stay hidden", async () => {
          // Re-navigate until the buttons converge: a stale config read
          // can paint the card without the fresh flags.
          await expect
            .poll(async () => {
              const buttons = await driver.spaceAuthProviderButtons();
              if (JSON.stringify(buttons) !== JSON.stringify(["Google", "GitHub"])) {
                await driver.spaceAuthOpenLanding();
              }
              return driver.spaceAuthProviderButtons();
            }, SERVER_POLL)
            .toEqual(["Google", "GitHub"]);
          expect(await driver.spaceAuthProvidersCompact()).toBe(false);
          expect(await driver.spaceAuthProvidersDividerVisible()).toBe(true);
        });

        await test.step("starting one navigates to the provider auth URL", async () => {
          const navigation = driver.page.waitForRequest(/\/auth\/google\//, { timeout: 30_000 });
          await driver.spaceAuthClickProvider("Google");
          const request = await navigation;
          expect(request.url()).toContain("/auth/google/");
        });

        await test.step("the password step keeps the buttons compact", async () => {
          await convergeStep(driver, seed.email, "password");
          await expect.poll(() => driver.spaceAuthProviderButtons(), SERVER_POLL).toEqual(["Google", "GitHub"]);
          expect(await driver.spaceAuthProvidersCompact()).toBe(true);
        });
      }
    );

    await test.step("restoring the flags removes the buttons and the slot", async () => {
      await expect
        .poll(async () => (await instanceConfig(undefined, admin)).is_google_enabled, SERVER_POLL)
        .toBe(false);
      await expect
        .poll(async () => {
          const buttons = await driver.spaceAuthProviderButtons();
          if (buttons.length !== 0) await driver.spaceAuthOpenLanding();
          return driver.spaceAuthProviderButtons();
        }, SERVER_POLL)
        .toEqual([]);
      expect(await driver.spaceAuthProvidersDividerVisible()).toBe(false);
    });
  }
);

test(
  specTitle(["SPC-063"], "error links land on the fixing step with a dismissible banner"),
  { tag: specTags(["SPC-063"]) },
  async ({ driver, seed }) => {
    await healBaseline(seed.email, seed.password);
    // web_new carries its own banner copy (independence rule); the
    // already-registered wording happens to match on both.
    const webNewBanner = (code: string, fallback: RegExp | null): RegExp | null => {
      if (driver.target !== "web_new") return fallback;
      if (code === "5065") return /sign-in failed/i;
      if (code === "5035") return /sign-up failed/i;
      if (code === "5090" || code === "5092") return /code isn't valid/i;
      if (code === "5095" || code === "5097" || code === "5100" || code === "5102") return /code has expired/i;
      if (code === "5060") return /no account uses this address/i;
      return fallback;
    };
    const cases: {
      code: string;
      step: "email" | "password" | "code";
      mode: "sign-in" | "sign-up" | null;
      banner: RegExp | null;
    }[] = [
      { code: "5065", step: "password", mode: "sign-in", banner: webNewBanner("5065", /authentication failed/i) },
      { code: "5035", step: "password", mode: "sign-up", banner: webNewBanner("5035", /authentication failed/i) },
      { code: "5090", step: "code", mode: "sign-in", banner: webNewBanner("5090", /invalid magic code/i) },
      { code: "5092", step: "code", mode: "sign-up", banner: webNewBanner("5092", /invalid magic code/i) },
      { code: "5095", step: "code", mode: "sign-in", banner: webNewBanner("5095", /expired magic code/i) },
      { code: "5097", step: "code", mode: "sign-up", banner: webNewBanner("5097", /expired magic code/i) },
      { code: "5100", step: "code", mode: "sign-in", banner: webNewBanner("5100", /expired magic code/i) },
      { code: "5102", step: "code", mode: "sign-up", banner: webNewBanner("5102", /expired magic code/i) },
      { code: "5060", step: "email", mode: "sign-up", banner: webNewBanner("5060", /no account found/i) },
      { code: "5030", step: "email", mode: "sign-up", banner: webNewBanner("5030", /already registered/i) },
      { code: "5900", step: "email", mode: "sign-up", banner: null },
      { code: "9999", step: "email", mode: "sign-up", banner: null },
    ];
    for (const { code, step, mode, banner } of cases) {
      await test.step(`error ${code} recovers on the ${step} step`, async () => {
        await driver.spaceAuthOpenLanding({ errorCode: code });
        await expect.poll(() => driver.spaceAuthStep()).toBe(step);
        expect(await driver.spaceAuthMode()).toBe(mode);
        if (banner === null) {
          expect(await driver.spaceAuthBanner()).toBeNull();
        } else {
          await expect.poll(() => driver.spaceAuthBanner()).toMatch(banner);
          await driver.spaceAuthDismissBanner();
          await expect.poll(() => driver.spaceAuthBanner()).toBeNull();
          expect(await driver.spaceAuthStep()).toBe(step);
        }
      });
    }

    await test.step("the server produces the coded links the card honors", async () => {
      const result = await nativeSpacePasswordAuth(seed.email, "Wrong-Password-1", "sign-in");
      expect(result.status).toBe(302);
      expect(result.location).toContain("error_code=5065");
      expect(result.sessionCookie).toBe(false);
    });
  }
);

test(
  specTitle(["SPC-064"], "headings and consent follow the mode"),
  { tag: specTags(["SPC-064"]) },
  async ({ driver, seed }) => {
    await healBaseline(seed.email, seed.password);
    await expect.poll(async () => (await spaceEmailCheck(seed.email)).status, SERVER_POLL).toBe("CREDENTIAL");

    // web_new carries its own header copy (independence rule).
    const signUpHeader =
      driver.target === "web_new"
        ? { heading: "Join the conversation", subline: "Create an account to vote and comment on public boards." }
        : {
            heading: "View, comment, and do more",
            subline: "Sign up or log in to work with Pi Dash work items and Pages.",
          };
    const signInHeader =
      driver.target === "web_new"
        ? { heading: "Welcome back", subline: "Sign in to vote and comment on public boards." }
        : {
            heading: "Sign in to upvote or comment",
            subline: "Contribute in nudging the features you want to get built.",
          };

    await test.step("the email step opens in sign-up mode with its consent line", async () => {
      await driver.spaceAuthOpenLanding();
      await expect.poll(() => driver.spaceAuthStep()).toBe("email");
      expect(await driver.spaceAuthMode()).toBe("sign-up");
      expect(await driver.spaceAuthHeader()).toEqual(signUpHeader);
      const consent = await driver.spaceAuthConsent();
      expect(consent.text).toMatch(/^by creating an account/i);
      expect(consent.termsHref).toBe("https://airepublic.com/terms");
      expect(consent.privacyHref).toBe("https://airepublic.com/privacy");
    });

    await test.step("a known address flips the card to the sign-in wording", async () => {
      await driver.spaceAuthSubmitEmail(seed.email);
      await expect.poll(() => driver.spaceAuthStep()).toBe("password");
      expect(await driver.spaceAuthMode()).toBe("sign-in");
      expect(await driver.spaceAuthHeader()).toEqual(signInHeader);
      const consent = await driver.spaceAuthConsent();
      expect(consent.text).toMatch(/^by signing in/i);
      expect(consent.termsHref).toBe("https://airepublic.com/terms");
      expect(consent.privacyHref).toBe("https://airepublic.com/privacy");
    });

    await test.step("the server routing behind the modes agrees", async () => {
      expect(await spaceEmailCheck(seed.email)).toEqual({ existing: true, status: "CREDENTIAL" });
    });
  }
);
