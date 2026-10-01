// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Oracle driver (NEWFRONT-19): implements the parity driver interface
// against apps/web. Selectors follow the sign-in card behavior observed on
// the running old app: the entry route renders the email step, a submit
// moves to the password step, and the password submit posts the native
// form, landing in the workspace. Later oracle issues extend this driver
// (never fork it) as new areas need new actions.
import { expect, type Locator, type Page } from "@playwright/test";
import type {
  ParityBrowserCookie,
  ParityDriver,
  ParityTarget,
  RulesCommentMenuOption,
  WorkspaceOnboardingView,
} from "./parity-driver";

export class WebDriver implements ParityDriver {
  readonly target: ParityTarget = "web";
  readonly page: Page;

  constructor(page: Page) {
    this.page = page;
  }

  /** Every wait below is explicitly bounded: the suite config leaves action and navigation timeouts at Playwright's unbounded defaults, so a bare waitFor would hang to the test timeout instead of failing honestly. */
  private static readonly WAIT_MS = 30_000;

  async openEntry(): Promise<void> {
    // The entry render occasionally never arrives under concurrent
    // load; run the paint again instead of burning the whole test
    // budget on one load. Each attempt settles on the card (any auth
    // step or the no-methods notice), never on one field.
    for (let attempt = 0; attempt < 2; attempt += 1) {
      await this.page.goto("/");
      try {
        await this.waitForCard();
        return;
      } catch {
        // One more paint before failing honestly.
      }
    }
    await this.page.goto("/");
    await this.waitForCard();
  }

  async currentPath(): Promise<string> {
    const url = new URL(this.page.url());
    return `${url.pathname}${url.search}`;
  }

  private submitOf(form: Locator): Locator {
    return form.locator('button[type="submit"]');
  }

  private emailField(): Locator {
    return this.page.getByPlaceholder("name@company.com").first();
  }

  private passwordField(): Locator {
    return this.page.getByPlaceholder("Enter password").first();
  }

  private confirmField(): Locator {
    return this.page.getByPlaceholder("Confirm password").first();
  }

  private codeField(): Locator {
    return this.page.getByPlaceholder("123456").first();
  }

  async openSignUp(params?: { email?: string; nextPath?: string }): Promise<void> {
    const query = new URLSearchParams();
    if (params?.email !== undefined) query.set("email", params.email);
    if (params?.nextPath !== undefined) query.set("next_path", params.nextPath);
    const suffix = query.size > 0 ? `?${query.toString()}` : "";
    await this.page.goto(`/sign-up${suffix}`);
    await this.waitForContent("sign-up card", () => this.emailField().waitFor({ timeout: 30_000 }));
  }

  private async waitForContent(label: string, wait: () => Promise<unknown>): Promise<void> {
    // A throttled loader minute can serve the route-error shell instead of
    // the page; one reload recovers the oracle. The anonymous throttle is
    // wide on the scratch stack, so a short settle suffices here (the
    // authentication-scope backoff lives in submitAuthEmail).
    try {
      await wait();
    } catch {
      // eslint-disable-next-line no-console -- oracle runs surface retries in the log.
      console.log(`[parity] ${label} content missing; settling and reloading once.`);
      await this.page.waitForTimeout(20_000);
      await this.page.reload();
      await wait();
    }
  }

  private async nextAuthStepShown(timeoutMs: number): Promise<boolean> {
    const password = this.passwordField()
      .waitFor({ state: "visible", timeout: timeoutMs })
      .then(
        () => true,
        () => false
      );
    const code = this.codeField()
      .waitFor({ state: "visible", timeout: timeoutMs })
      .then(
        () => true,
        () => false
      );
    const [passwordShown, codeShown] = await Promise.all([password, code]);
    return passwordShown || codeShown;
  }

  async submitAuthEmail(email: string): Promise<void> {
    const field = this.emailField();
    const form = this.page.locator("form", { has: field });
    // A saturated throttle minute answers the check with a rate-limit
    // banner instead of advancing; wait out the window and resubmit.
    for (let attempt = 1; ; attempt += 1) {
      await field.fill(email);
      await this.submitOf(form).click();
      if (await this.nextAuthStepShown(30_000)) return;
      const banner = await this.authBanner();
      if (banner !== null && /rate limit/i.test(banner) && attempt < 3) {
        // eslint-disable-next-line no-console -- oracle runs surface retries in the log.
        console.log(`[parity] email-check throttled (attempt ${attempt}/3); waiting out the window.`);
        await this.page.waitForTimeout(65_000);
        continue;
      }
      if (await this.nextAuthStepShown(60_000)) return;
      throw new Error("[parity] email submit advanced to neither password nor code step.");
    }
  }

  // NOTE (rebase over NEWFRONT-107): the shared authStep keeps the wider
  // implementation below (it also detects "unavailable"/"unknown" for the
  // sign-in core). It keys on the same card placeholders this area uses, so
  // the sign-up/recovery scenarios observe identical steps.
  async authEmailValue(): Promise<string> {
    return this.emailField().inputValue();
  }

  async authNextPathValue(): Promise<string | null> {
    const hidden = this.page.locator('input[type="hidden"][name="next_path"]');
    if ((await hidden.count()) === 0) return null;
    return hidden.first().getAttribute("value");
  }

  private async clickNativeSubmit(form: Locator): Promise<void> {
    // The old app posts native forms, so every submit ends in a full page
    // load; wait for the URL to move (success and failure both redirect).
    // Bounded: a submit that never navigates (disabled button, missing CSRF
    // token) is a real failure and must surface instead of eating the test
    // budget.
    const before = this.page.url();
    await Promise.all([
      this.page.waitForURL((url) => url.href !== before, { timeout: 60_000 }),
      this.submitOf(form).click(),
    ]);
    await this.page.waitForLoadState("domcontentloaded");
  }

  async signUpWithPassword(password: string, confirmPassword: string): Promise<void> {
    await this.passwordField().fill(password);
    await this.confirmField().fill(confirmPassword);
    await this.clickNativeSubmit(this.page.locator("form", { has: this.passwordField() }));
  }

  async submitUniqueCode(code: string): Promise<void> {
    const field = this.codeField();
    await field.fill(code);
    await this.clickNativeSubmit(this.page.locator("form", { has: field }));
  }

  async codeResendState(): Promise<{ disabled: boolean; label: string }> {
    const button = this.page.getByRole("button", { name: /resend|requesting new code/i });
    await button.waitFor({ timeout: 30_000 });
    const label = ((await button.textContent()) ?? "").trim().replace(/\s+/g, " ");
    return { disabled: await button.isDisabled(), label };
  }

  async requestNewCode(): Promise<void> {
    await this.page.getByRole("button", { name: /resend|requesting new code/i }).click();
  }

  async passwordSubmitEnabled(): Promise<boolean> {
    const form = this.page.locator("form", { has: this.passwordField() });
    return this.submitOf(form).isEnabled();
  }

  async fillPasswordFields(password: string, confirmPassword: string): Promise<void> {
    await this.passwordField().fill(password);
    await this.confirmField().fill(confirmPassword);
  }

  async clickPasswordSubmit(): Promise<void> {
    const form = this.page.locator("form", { has: this.passwordField() });
    await this.submitOf(form).click();
    await this.page.waitForTimeout(2000);
  }

  async passwordMismatchError(): Promise<string | null> {
    const note = this.page.getByText("Passwords don't match");
    if ((await note.count()) === 0) return null;
    return ((await note.first().textContent()) ?? "").trim();
  }

  async authBanner(): Promise<string | null> {
    const banner = this.page.getByRole("alert");
    if ((await banner.count()) > 0) return ((await banner.first().textContent()) ?? "").trim();
    // The sign-up card refuses weak secrets with its own dismissible
    // notice instead of the shared banner component.
    const weakNotice = this.page.getByText("Try setting-up a strong password to proceed");
    if ((await weakNotice.count()) > 0) return ((await weakNotice.first().textContent()) ?? "").trim();
    return null;
  }

  async waitForAuthBanner(): Promise<string> {
    // The banner renders after hydration plus the error-code effect, so
    // poll instead of reading once. Bounded: a missing banner is a real
    // failure and must surface instead of eating the test budget.
    const banner = this.page.getByRole("alert");
    await banner.first().waitFor({ timeout: 60_000 });
    return ((await banner.first().textContent()) ?? "").trim();
  }

  async openForgotPassword(email?: string): Promise<void> {
    const suffix = email !== undefined ? `?email=${encodeURIComponent(email)}` : "";
    await this.page.goto(`/accounts/forgot-password${suffix}`);
    await this.waitForContent("forgot-password form", () =>
      this.page.getByRole("button", { name: /send reset link|resend in/i }).waitFor({ timeout: 30_000 })
    );
  }

  async submitForgotPassword(email: string): Promise<string> {
    const field = this.emailField();
    await field.fill(email);
    const form = this.page.locator("form", { has: field });
    await this.submitOf(form).click();
    // Either the inbox-check toast or the failure toast appears; the
    // button label flips to the countdown on success.
    const toast = this.page.getByText(/check your inbox|something went wrong|smtp not configured|error!/i).first();
    await toast.waitFor({ timeout: 30_000 });
    return ((await toast.textContent()) ?? "").trim();
  }

  async forgotResendState(): Promise<{ disabled: boolean; label: string }> {
    const button = this.page.getByRole("button", { name: /send reset link|resend in/i });
    await button.waitFor({ timeout: 30_000 });
    const label = ((await button.textContent()) ?? "").trim().replace(/\s+/g, " ");
    return { disabled: (await button.isDisabled()) || (await button.getAttribute("data-loading")) !== null, label };
  }

  async openResetPassword(params: { uid: string; token: string; email: string }): Promise<void> {
    const query = new URLSearchParams({ uidb64: params.uid, token: params.token, email: params.email });
    await this.page.goto(`/accounts/reset-password?${query.toString()}`);
    await this.waitForContent("reset-password form", () => this.passwordField().waitFor({ timeout: 30_000 }));
  }

  async submitNewPassword(password: string, confirmPassword: string): Promise<void> {
    await this.passwordField().fill(password);
    await this.confirmField().fill(confirmPassword);
    await this.clickNativeSubmit(this.page.locator("form", { has: this.passwordField() }));
  }

  async openSetPassword(): Promise<void> {
    await this.page.goto("/accounts/set-password");
    await this.page.waitForLoadState("domcontentloaded");
  }

  async openPath(path: string): Promise<void> {
    await this.page.goto(path);
    await this.page.waitForLoadState("domcontentloaded");
  }

  async signInWithPassword(email: string, password: string): Promise<void> {
    // The dev server occasionally swallows a submit, so every pass is
    // retried whole until the workspace landing is confirmed.
    for (let attempt = 0; attempt < 3; attempt += 1) {
      try {
        await this.signInAttempt(email, password);
        return;
      } catch {
        // Still on entry or a swallowed submit: run the pass again.
      }
    }
    await this.signInAttempt(email, password);
  }

  private async signInAttempt(email: string, password: string): Promise<void> {
    const page = this.page;
    if (this.signedInPath(page.url())) return;
    await page.goto("/");
    // The shared email submit waits out a throttled email-check minute
    // (rate-limit banner instead of advancing) and resubmits; a bare
    // fill-and-click here would burn the whole test budget retrying
    // into 429s under concurrent parity runs.
    await this.submitAuthEmail(email);
    const passwordField = page.getByPlaceholder("Enter password");
    await passwordField.waitFor({ timeout: WebDriver.WAIT_MS });
    await passwordField.fill(password);
    const passwordForm = page.locator("form", { has: passwordField });
    // The old app posts the native form, so this ends in a full page load.
    await Promise.all([
      page.waitForURL(/\/[^/]+\//, { timeout: WebDriver.WAIT_MS }),
      this.submitOf(passwordForm).click(),
    ]);
  }

  private signedInPath(url: string): boolean {
    const pathname = new URL(url).pathname;
    return pathname !== "/" && !pathname.startsWith("/auth") && !pathname.startsWith("/sign");
  }

  async openProjectIssues(workspaceSlug: string, projectId: string): Promise<void> {
    await this.page.goto(`/${workspaceSlug}/projects/${projectId}/issues`);
    await this.page.waitForLoadState("domcontentloaded", { timeout: WebDriver.WAIT_MS });
  }

  async openSignInWithParams(params: Record<string, string>): Promise<void> {
    const query = new URLSearchParams(params).toString();
    await this.page.goto(query === "" ? "/" : `/?${query}`);
    await this.waitForCard();
  }

  /**
   * Wait until the sign-in card settles. Either a form step renders, or
   * (methods disabled) the no-methods card renders instead of any form.
   * When the shared stack throttles the instance-config fetch, the old app
   * shows a startup-failure screen instead of the card; that screen carries
   * the failed URL, so reloading it is a safe GET that converges once the
   * throttle minute passes (bounded: at most three reloads).
   */
  private async waitForCard(): Promise<void> {
    // Settle with all branches bounded: a blank or stalled page resolves
    // every branch as absent instead of hanging the race. Rounds are short
    // because the usual cause is a network blip (failed sub-resources leave
    // a blank page that a reload fixes), not a slow app.
    const page = this.page;
    const roundMs = 15_000;
    const formInput = page
      .locator(
        'input[placeholder="name@company.com"], input[placeholder="Enter password"], input[placeholder="123456"]'
      )
      .first();
    const noMethods = page.getByText("No authentication methods available");
    const startupFailure = page.getByText("didn't start up correctly");
    const seen = async (target: { waitFor: (options: { timeout: number }) => Promise<void> }): Promise<boolean> =>
      target.waitFor({ timeout: roundMs }).then(
        () => true,
        () => false
      );
    for (let attempt = 0; ; attempt++) {
      const [form, noMethod, maintenance] = await Promise.all([seen(formInput), seen(noMethods), seen(startupFailure)]);
      if (form || noMethod || attempt >= 5) return;
      if (maintenance) {
        await page.waitForTimeout(10_000);
        await page.reload({ timeout: WebDriver.WAIT_MS }).catch(() => {});
      } else {
        // Nothing rendered at all (stalled load): reload once per round too.
        await page.reload({ timeout: WebDriver.WAIT_MS }).catch(() => {});
      }
    }
  }

  async submitEmail(email: string): Promise<void> {
    const page = this.page;
    const emailField = page.getByPlaceholder("name@company.com").first();
    await emailField.fill(email);
    const emailForm = page.locator("form", { has: emailField });
    await this.submitOf(emailForm).click();
    // The email check round-trips to the server, then the card shows the
    // password or code step. Either placeholder marks the transition.
    await page
      .locator('input[placeholder="Enter password"], input[placeholder="123456"]')
      .first()
      .waitFor({ timeout: WebDriver.WAIT_MS });
  }

  /**
   * The old app posts the native form, so the submit ends in a full page
   * load whether the credentials were right or wrong. The load wait never
   * fails the test: when it times out the scenario's polls still decide
   * against whatever actually rendered.
   */
  private async nativeSubmit(form: Locator): Promise<void> {
    const page = this.page;
    await Promise.all([
      page.waitForLoadState("domcontentloaded", { timeout: WebDriver.WAIT_MS }).catch(() => {}),
      this.submitOf(form).click(),
    ]);
  }

  async submitPassword(password: string): Promise<void> {
    const page = this.page;
    const passwordField = page.getByPlaceholder("Enter password");
    await passwordField.fill(password);
    await this.nativeSubmit(page.locator("form", { has: passwordField }));
  }

  async authStep(): Promise<"email" | "password" | "code" | "unavailable" | "unknown"> {
    const page = this.page;
    if (
      await page
        .getByText("No authentication methods available")
        .count()
        .then((n) => n > 0)
    ) {
      return "unavailable";
    }
    if ((await page.getByPlaceholder("Enter password").count()) > 0) return "password";
    if ((await page.getByPlaceholder("123456").count()) > 0) return "code";
    if ((await page.getByPlaceholder("name@company.com").count()) > 0) return "email";
    return "unknown";
  }

  async bannerText(): Promise<string | null> {
    const alert = this.page.getByRole("alert");
    if ((await alert.count()) === 0) return null;
    const text = (await alert.first().innerText()).trim();
    return text === "" ? null : text;
  }

  async dismissBanner(): Promise<void> {
    await this.page.getByRole("alert").getByRole("button").first().click();
  }

  async seesWorkspaceInviteHeader(workspaceName: string): Promise<boolean> {
    // The invitation header reads as one line ("Join <workspace>"); any
    // visible line pairing an invitation verb with the workspace name
    // proves the match, without coupling to the header markup.
    const candidates = this.page.getByText(/join/i);
    const count = await candidates.count();
    for (let i = 0; i < count; i++) {
      if ((await candidates.nth(i).innerText()).trim().includes(workspaceName)) return true;
    }
    return false;
  }

  async seesGenericSignInHeader(): Promise<boolean> {
    return (await this.page.getByText("Welcome back to Pi Dash.").count()) > 0;
  }

  async seesGenericSignUpHeader(): Promise<boolean> {
    return (await this.page.getByText("Create your Pi Dash account.").count()) > 0;
  }

  async seesConfirmPassword(): Promise<boolean> {
    return (await this.page.getByPlaceholder("Confirm password").count()) > 0;
  }

  async passwordPrimaryButtonLabel(): Promise<string | null> {
    const field = this.page.getByPlaceholder("Enter password");
    if ((await field.count()) === 0) return null;
    const form = this.page.locator("form", { has: field });
    const label = await form.locator('button[type="submit"]').first().innerText();
    return label.trim() === "" ? null : label.trim();
  }

  async forgotPasswordEntry(): Promise<"link" | "popover" | "absent"> {
    const page = this.page;
    const entry = page.getByText("Forgot your password?");
    if ((await entry.count()) === 0) return "absent";
    // The reset link navigates to the forgot-password page; the mail-less
    // explanation is a popover toggle with no destination.
    const asLink = entry.locator("xpath=ancestor-or-self::a");
    if ((await asLink.count()) > 0) return "link";
    return "popover";
  }

  async seesUniqueCodeButton(): Promise<boolean> {
    return (await this.page.getByRole("button", { name: "Sign in with unique code" }).count()) > 0;
  }

  async requestUniqueCode(): Promise<void> {
    await this.page.getByRole("button", { name: "Sign in with unique code" }).click();
    await this.page.getByPlaceholder("123456").waitFor({ timeout: WebDriver.WAIT_MS });
  }

  async resendCodeLabel(): Promise<string | null> {
    const page = this.page;
    const resend = page.getByRole("button", { name: /resend/i });
    if ((await resend.count()) === 0) return null;
    const label = (await resend.first().innerText()).trim();
    return label === "" ? null : label;
  }

  async clickResendCode(): Promise<void> {
    await this.page
      .getByRole("button", { name: /resend/i })
      .first()
      .click();
  }

  async submitCode(code: string): Promise<void> {
    const page = this.page;
    const codeField = page.getByPlaceholder("123456");
    await codeField.fill(code);
    await this.nativeSubmit(page.locator("form", { has: codeField }));
  }

  async providerSignInButtons(): Promise<string[]> {
    const names: string[] = [];
    for (const provider of ["Google", "GitHub", "GitLab", "Gitea"]) {
      const button = this.page.getByRole("button", { name: new RegExp(`Sign in with ${provider}`, "i") });
      if ((await button.count()) > 0) names.push(provider);
    }
    return names;
  }

  async clickProviderButton(name: string): Promise<void> {
    await this.page.getByRole("button", { name: new RegExp(`Sign in with ${name}`, "i") }).click();
  }

  async clearEmail(): Promise<void> {
    const field =
      (await this.page.getByPlaceholder("Enter password").count()) > 0
        ? this.page.getByPlaceholder("Enter password")
        : this.page.getByPlaceholder("123456");
    const form = this.page.locator("form", { has: field });
    await form.getByRole("button", { name: "Clear email" }).click();
    await this.page.getByPlaceholder("name@company.com").first().waitFor({ timeout: WebDriver.WAIT_MS });
  }

  async seesNoAuthMethods(): Promise<boolean> {
    // The card explains login is unavailable, points at the administrator,
    // and shows no form at all.
    const page = this.page;
    const heading = page.getByText("No authentication methods available");
    if ((await heading.count()) === 0) return false;
    const card = heading.locator("..");
    const cardText = ((await card.innerText()) ?? "").toLowerCase();
    return cardText.includes("administrator") && (await page.getByPlaceholder("name@company.com").count()) === 0;
  }

  async forgotPasswordPopoverText(): Promise<string | null> {
    const page = this.page;
    const entry = page.getByText("Forgot your password?");
    if ((await entry.count()) === 0) return null;
    await entry.first().click();
    const panel = page.getByText(/smtp/i);
    if ((await panel.count()) === 0) return null;
    const text = (await panel.first().innerText()).trim();
    return text === "" ? null : text;
  }

  /**
   * User-visible read of the issues list. Observed on the running old app:
   * each issue row renders its title as paragraph text inside a main
   * landmark, and every seeded title is unique on the page — so no
   * app-side hook is needed. Returns every non-empty paragraph text found
   * (this includes surrounding chrome such as nav labels); callers match
   * the names they care about out of it. Scenarios poll this until the
   * list populates instead of waiting on a fixed selector.
   */
  async visibleIssueNames(): Promise<string[]> {
    const texts = await this.page.getByRole("main").locator("p").allTextContents();
    return texts.map((t) => t.trim()).filter((t) => t.length > 0);
  }

  // --- Mention flows (NEWFRONT-115, CMT-019/020/021). Appended after the
  // --- NEWFRONT-19 methods, which are untouched per the shared contract.
  //
  // Observed on the running old app (issue detail, activity section):
  // the comment composer is the last rich-text editable on the page;
  // typing "@" opens a suggestion popup whose items are buttons with ids
  // like "mention-item-0-0" grouped under section headers ("Users");
  // Enter picks the highlighted suggestion; the "Comment" button posts
  // (a success toast confirms); saved comments render each mention as a
  // link to "/{workspaceSlug}/profile/{memberId}" showing "@Display Name".

  /**
   * Sign in with the shared flow and verify the session actually landed.
   * The shared sign-in posts the native form, and the backend rejects
   * valid credentials intermittently while sibling parity runs hammer the
   * shared scratch stack, so this retries the whole flow for up to 90
   * seconds: leaving the entry route proves the login POST went through,
   * and the sidebar printing the account email proves whose session it
   * is. Calls the shared methods; never edits them. Bounded at 60 seconds so
   * a scenario still fits the suite's per-test budget alongside its poll
   * waits; a window needing longer is broken for every scenario anyway.
   */
  /**
   * One sign-in attempt bounded wall-clock: the shared methods carry no
   * explicit timeouts of their own, so a stalled oracle would otherwise
   * hang an attempt until the suite's test timeout with no retry.
   */
  private async mentionsAttempt<T>(work: () => Promise<T>, ms: number, label: string): Promise<T> {
    let timer: ReturnType<typeof setTimeout> | undefined;
    try {
      return await Promise.race([
        work(),
        new Promise<never>((_, reject) => {
          timer = setTimeout(() => reject(new Error(`[parity] ${label} timed out after ${ms}ms`)), ms);
        }),
      ]);
    } finally {
      if (timer !== undefined) clearTimeout(timer);
    }
  }

  async mentionsEnsureSignedIn(email: string, password: string): Promise<void> {
    const deadline = Date.now() + 60_000;
    let lastError = "no attempt finished";
    let attempts = 0;
    while (Date.now() < deadline && attempts < 3) {
      attempts += 1;
      try {
        await this.mentionsAttempt(
          async () => {
            await this.openEntry();
            await this.signInWithPassword(email, password);
            await this.page.waitForURL((url) => url.pathname !== "/", { timeout: 15_000 });
            await this.page.getByText(email).first().waitFor({ timeout: 15_000 });
          },
          45_000,
          `sign-in attempt ${attempts}`
        );
        return;
      } catch (error) {
        lastError = String(error).split("\n")[0] ?? String(error);
      }
    }
    throw new Error(`[parity] sign-in as ${email} did not land (attempts: ${attempts}): ${lastError}`);
  }

  async mentionsOpenIssueDetail(workspaceSlug: string, projectId: string, issueId: string): Promise<void> {
    // Explicit navigation timeout: without one a stalled oracle hangs the
    // step until the suite's test timeout instead of failing diagnosably.
    await this.page.goto(`/${workspaceSlug}/projects/${projectId}/issues/${issueId}`, { timeout: 60_000 });
    await this.page.waitForLoadState("domcontentloaded");
    // The composer submit existing proves the detail plus its activity
    // section loaded authenticated; the app redirects this URL to the
    // canonical browse path, so no URL assertion here.
    try {
      await this.page.getByRole("button", { name: "Comment", exact: true }).first().waitFor({ timeout: 60_000 });
    } catch (error) {
      // Name the intake race distinctly: a sibling run can triage the
      // issue between selection and open, landing on the intake view
      // which has no composer. Anything else is slowness or auth.
      if (this.page.url().includes("/intake/")) {
        throw new Error(`[parity] issue ${issueId} landed on the intake view; it was triaged after selection.`);
      }
      throw error;
    }
  }

  private mentionsComposer(): Locator {
    return this.page.locator('[contenteditable="true"]').last();
  }

  private mentionsSuggestionButtons(): Locator {
    return this.page.locator('button[id^="mention-item-"]');
  }

  async mentionsSuggestionsFor(query: string): Promise<string[]> {
    const composer = this.mentionsComposer();
    // A suggestion popup left open by a previous query covers the composer
    // and would intercept the click; dismiss it first.
    await this.page.keyboard.press("Escape");
    await composer.scrollIntoViewIfNeeded();
    await composer.click();
    // Start from an empty composer so repeated calls re-query instead of
    // appending to the previous trigger text.
    await this.page.keyboard.press("ControlOrMeta+a");
    await this.page.keyboard.press("Backspace");
    await composer.pressSequentially(`@${query}`, { delay: 90 });
    const items = this.mentionsSuggestionButtons();
    await items.first().waitFor({ timeout: 30_000 });
    // The name paragraph excludes the avatar initial, which allTextContents
    // of the whole button would prepend.
    const names = await items.locator("p").allTextContents();
    return names.map((t) => t.trim()).filter((t) => t.length > 0);
  }

  async mentionsSuggestionsHaveAvatars(): Promise<boolean> {
    const items = this.mentionsSuggestionButtons();
    const count = await items.count();
    if (count === 0) return false;
    for (const item of await items.all()) {
      const avatar = item.locator("span").first();
      if ((await avatar.count()) === 0) return false;
      if (!((await avatar.evaluate((el) => (el.textContent ?? "").trim())).length > 0)) return false;
    }
    return true;
  }

  async mentionsSuggestionSections(): Promise<string[]> {
    const headers = this.page.locator(".react-renderer h6");
    await headers.first().waitFor({ timeout: 30_000 });
    const texts = await headers.allTextContents();
    return texts.map((t) => t.trim()).filter((t) => t.length > 0);
  }

  async mentionsPostComment(displayName: string, bodyText: string): Promise<void> {
    const composer = this.mentionsComposer();
    await this.page.keyboard.press("Escape");
    await composer.scrollIntoViewIfNeeded();
    await composer.click();
    await composer.pressSequentially(`@${displayName}`, { delay: 90 });
    await this.mentionsSuggestionButtons().first().waitFor({ timeout: 30_000 });
    await this.page.keyboard.press("Enter");
    await composer.pressSequentially(` ${bodyText}`, { delay: 40 });
    await this.page.getByRole("button", { name: "Comment", exact: true }).first().click();
    // The posted body text appearing in the feed proves the save landed;
    // bodyText is unique per scenario run, so this cannot match a stale row.
    await this.page.getByText(bodyText).first().waitFor({ timeout: 60_000 });
  }

  async mentionsVisibleReferences(): Promise<{ text: string; href: string | null }[]> {
    const links = this.page.locator('a[href*="/profile/"]');
    const count = await links.count();
    const refs: { text: string; href: string | null }[] = [];
    for (let i = 0; i < count; i++) {
      const link = links.nth(i);
      const text = (await link.allTextContents()).join("").trim();
      // Mention chips render "@Display Name"; other profile links (author
      // rows, assignees) do not carry the "@" marker.
      if (!text.startsWith("@")) continue;
      refs.push({ text, href: await link.getAttribute("href") });
    }
    return refs;
  }

  /**
   * Remove the mention from the comment showing `oldBodyText` through the
   * card overflow menu. Observed on the running old app: each comment card
   * header carries an ellipsis trigger opening a small popover with an
   * edit option; the inline edit form saves on Enter.
   */
  async mentionsEditRemovingMention(oldBodyText: string, plainText: string): Promise<void> {
    const page = this.page;
    const body = page.locator(`text=${oldBodyText}`).first();
    await body.waitFor({ timeout: 60_000 });
    await body.scrollIntoViewIfNeeded();
    // The header controls render on card hover.
    await body.hover();
    const trigger = await body.evaluateHandle((el) => {
      const origin = el.getBoundingClientRect();
      // Climb to the card root: the nearest ancestor that also renders the
      // card header ("commented … ago").
      let card: HTMLElement | null = el instanceof HTMLElement ? el : el.parentElement;
      while (card !== null && card !== document.body) {
        if ((card.innerText ?? "").includes("commented")) break;
        card = card.parentElement;
      }
      const scope = card ?? document.body;
      const originBox = origin;
      const scoreOf = (candidate: Element): number | null => {
        const box = candidate.getBoundingClientRect();
        if (box.width === 0 || box.height === 0) return null;
        const dy = originBox.top - box.top;
        if (dy < -10 || dy > 260) return null;
        return Math.abs(box.left + box.width / 2 - (originBox.left + originBox.width / 2)) + dy;
      };
      // The overflow trigger is an icon-only control: prefer a literal
      // ellipsis, else the rightmost header button. Proximity alone picks
      // the reaction button (same header, no text either), which opens the
      // emoji picker instead of the card menu.
      let best: Element | null = null;
      let bestScore = 1e12;
      for (const candidate of scope.querySelectorAll("button, div")) {
        const text = (candidate.textContent ?? "").trim();
        if (text !== "…" && text !== "...") continue;
        const score = scoreOf(candidate);
        if (score !== null && score < bestScore) {
          bestScore = score;
          best = candidate;
        }
      }
      if (best === null) {
        let bestX = -1e12;
        for (const candidate of scope.querySelectorAll("button")) {
          const text = (candidate.textContent ?? "").trim();
          if (text !== "") continue;
          const score = scoreOf(candidate);
          if (score === null) continue;
          const x = candidate.getBoundingClientRect().left;
          if (x > bestX) {
            bestX = x;
            best = candidate;
          }
        }
      }
      return best;
    });
    const triggerElement = trigger.asElement();
    if (triggerElement === null) throw new Error("[parity] comment overflow trigger not found.");
    await triggerElement.click({ force: true });
    // The popover option edits inline. Prefer a real button (the static
    // "Last edited …" header text must never match); fall back to anchored
    // visible text for non-button menu rows.
    const editButton = page.getByRole("button", { name: /^edit/i });
    const editOpened = await editButton
      .first()
      .waitFor({ timeout: 15_000 })
      .then(() => true)
      .catch(() => false);
    if (editOpened) {
      await editButton.first().click();
    } else {
      const editText = page.getByText(/^edit/i);
      await editText.first().waitFor({ timeout: 15_000 });
      await editText.first().click();
    }
    // The card swaps to an inline rich-text form pre-filled with the old
    // body; find the editable holding it (the page composer below the
    // feed is empty, so the text uniquely identifies the inline form).
    const editorHandle = await page.waitForFunction(
      (text) => {
        const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
        let node: Text | null = null;
        while ((node = walker.nextNode() as Text | null) !== null) {
          if ((node.textContent ?? "").includes(text)) break;
        }
        if (node === null || !(node.parentElement instanceof HTMLElement)) return null;
        const editable = node.parentElement.closest('[contenteditable="true"]');
        return editable instanceof HTMLElement ? editable : null;
      },
      oldBodyText,
      { timeout: 30_000 }
    );
    const editorElement = editorHandle.asElement();
    if (editorElement === null) throw new Error("[parity] inline comment editor not found.");
    await editorElement.click();
    await page.keyboard.press("ControlOrMeta+a");
    await page.keyboard.press("Backspace");
    await editorElement.type(plainText, { delay: 40 });
    await page.keyboard.press("Enter");
    // The edited plain text appearing while the old body disappears proves
    // the save landed.
    await page.getByText(plainText).first().waitFor({ timeout: 60_000 });
  }
  // -------------------------------------------------------------------------
  // Workspace onboarding + creation (NEWFRONT-111, rows AUTH-034..043).
  // Selectors follow the running old app: onboarding step headings render as
  // <h1> (CommonOnboardingHeader), form fields carry <label htmlFor> so
  // getByLabel resolves them, and actions are plain buttons named by their
  // visible text. The old app exposes no data-testid, so the two icon-only
  // controls this area needs (the header back chevron) are reached
  // structurally, never by a hook added to apps/web.
  // -------------------------------------------------------------------------

  private async isShown(locator: Locator): Promise<boolean> {
    if ((await locator.count()) === 0) return false;
    return locator.first().isVisible();
  }

  async openAuthenticated(path: string, cookies: ParityBrowserCookie[]): Promise<void> {
    await this.page.context().addCookies(cookies);
    await this.page.goto(path);
    await this.page.waitForLoadState("domcontentloaded");
  }

  // NOTE (rebase over NEWFRONT-107): a single currentPath keeps the
  // pathname-plus-search shape above (the guard scenarios assert on
  // ?next_path=); the other suites only use toContain, which is unaffected.
  async hasVisibleText(text: string): Promise<boolean> {
    return this.isShown(this.page.getByText(text, { exact: false }));
  }

  async awaitWorkspaceStep(): Promise<void> {
    for (let attempt = 0; attempt < 60; attempt += 1) {
      if ((await this.visibleWorkspaceView()) !== "none") return;
      await this.page.waitForTimeout(500);
    }
    throw new Error("[parity] workspace create-or-join step never appeared.");
  }

  async visibleWorkspaceView(): Promise<WorkspaceOnboardingView> {
    const page = this.page;
    if (await this.isShown(page.getByText("Waiting for approval", { exact: false }))) return "pending";
    if (await this.isShown(page.getByRole("heading", { name: "Join invites or create a workspace", exact: true })))
      return "invites";
    if (await this.isShown(page.getByRole("heading", { name: "Join an existing workspace", exact: true })))
      return "join_by_email";
    if (await this.isShown(page.getByRole("heading", { name: "Create your workspace", exact: true }))) return "create";
    return "none";
  }

  async fillWorkspaceName(name: string): Promise<void> {
    await this.page.getByLabel("Name your workspace").fill(name);
  }

  async fillWorkspaceSlug(slug: string): Promise<void> {
    await this.page.getByLabel("Set your workspace's URL").fill(slug);
  }

  async workspaceSlugValue(): Promise<string> {
    return this.page.getByLabel("Set your workspace's URL").inputValue();
  }

  async selectTeamSizePill(label: string): Promise<void> {
    await this.page.getByRole("button", { name: label, exact: true }).click();
  }

  async selectTeamSizeDropdown(label: string): Promise<void> {
    // The standalone form uses a CustomSelect: click the trigger (shows the
    // placeholder until a choice is made), then the option.
    await this.page.getByText("Select a range", { exact: false }).first().click();
    await this.page.getByText(label, { exact: true }).last().click();
  }

  async submitCreateWorkspace(): Promise<void> {
    await this.page.getByRole("button", { name: "Create workspace", exact: true }).click();
  }

  async isCreateWorkspaceSubmitDisabled(): Promise<boolean> {
    return this.page.getByRole("button", { name: "Create workspace", exact: true }).isDisabled();
  }

  async workspaceSlugErrorText(): Promise<string | null> {
    const candidates = [
      "Workspace URL is already taken!",
      "URLs can contain only ('-') and alphanumeric characters.",
      "Limit your URL to 48 characters.",
    ];
    for (const text of candidates) {
      const loc = this.page.getByText(text, { exact: false });
      if (await this.isShown(loc)) return (await loc.first().textContent())?.trim() ?? text;
    }
    return null;
  }

  // --- NEWFRONT-113 (rules): comment permissions, visibility, deep links. ---
  // Selectors observed on the running old app. Comment cards render with
  // id="comment-<uuid>"; the overflow trigger carries a data-testid hook;
  // menu items are headless-ui buttons found by their visible titles.

  private rulesCard(commentId: string): Locator {
    return this.page.locator(`#comment-${commentId}`);
  }

  private async rulesWaitForFeed(): Promise<void> {
    // The composer group marks a loaded activity section; the feed itself
    // may be empty (no cards), so waiting on cards would hang. The dev
    // server compiles the detail route on first visit, hence the wait.
    await this.page.getByRole("group", { name: "Add comment" }).first().waitFor({ timeout: 120_000 });
  }

  async rulesOpenIssueDetail(workspaceSlug: string, projectId: string, issueId: string): Promise<void> {
    await this.page.goto(`/${workspaceSlug}/projects/${projectId}/issues/${issueId}`);
    await this.page.waitForLoadState("domcontentloaded");
    await this.rulesWaitForFeed();
  }

  async rulesOpenIntakeIssue(workspaceSlug: string, projectId: string, inboxIssueId: string): Promise<void> {
    await this.page.goto(`/${workspaceSlug}/projects/${projectId}/intake?inboxIssueId=${inboxIssueId}`);
    await this.page.waitForLoadState("domcontentloaded");
    // The intake screen resolves the triage row, redirects onto the linked
    // issue form, then loads its feed; that chain is slow on the dev server.
    await this.page.getByRole("group", { name: "Add comment" }).first().waitFor({ timeout: 300_000 });
  }

  async rulesCommentBodyText(commentId: string): Promise<string | null> {
    const card = this.rulesCard(commentId);
    await card.waitFor();
    // The read-only editor container is unmounted while the card is
    // folded shut, so its absence is the collapsed signal.
    const body = card.locator(`#editor-container-${commentId}`);
    if ((await body.count()) === 0) return null;
    return (await body.first().innerText()).trim();
  }

  private async rulesClickCoords(target: Locator): Promise<void> {
    // The overflow trigger (and its menu items) nest a button inside a
    // button, so Playwright's hit-test never settles on the right node.
    // Clicking the coordinates runs the real mouse pipeline instead.
    const box = await target.boundingBox();
    if (!box) throw new Error("[parity] rules menu target has no layout box.");
    await this.page.mouse.click(box.x + box.width / 2, box.y + box.height / 2);
  }

  private async rulesMenuOpen(card: Locator): Promise<boolean> {
    // Items mount only while the CustomMenu is open; the headless-ui
    // expanded flag never flips (its toggle is preventDefaulted), so it
    // cannot be the signal.
    return (await card.getByRole("menu").getByRole("menuitem").count()) > 0;
  }

  private async rulesOpenMenu(commentId: string): Promise<void> {
    const card = this.rulesCard(commentId);
    const trigger = card.locator('button[aria-haspopup="menu"] button').first();
    await trigger.waitFor();
    // Cards render far down the feed; the trigger must be scrolled under
    // the viewport before a coordinate click can land on it. The feed
    // keeps streaming entries while it loads, so a click can land on a
    // shifted-away point: re-aim from a fresh box until the menu reports
    // open (each attempt is a genuine pointer click).
    for (let attempt = 0; attempt < 5; attempt++) {
      await trigger.scrollIntoViewIfNeeded();
      await this.rulesClickCoords(trigger);
      await this.page.waitForTimeout(800);
      if (await this.rulesMenuOpen(card)) return;
    }
    throw new Error(`[parity] comment overflow menu did not open for ${commentId}.`);
  }

  async rulesCommentMenuOptions(commentId: string): Promise<string[]> {
    await this.rulesOpenMenu(commentId);
    // Scope to the card: every CustomMenu mounts its items statically, so
    // an unscoped query matches permanently hidden menus elsewhere. Wait
    // on the items, not the container: popper leaves the fixed container
    // at a zero box while the items themselves render with size.
    const menu = this.rulesCard(commentId).getByRole("menu");
    await menu.getByRole("menuitem").first().waitFor();
    const keys: string[] = [];
    const titles = await menu.getByRole("menuitem").allTextContents();
    // Map visible English titles back to stable option keys. Titles are
    // matched case-insensitively so a copy tweak cannot silently pass.
    for (const raw of titles) {
      const title = raw.trim().toLowerCase();
      if (title === "edit") keys.push("edit");
      else if (title === "copy link") keys.push("copy_link");
      else if (title.startsWith("switch to public") || title.startsWith("switch to private"))
        keys.push("access_switch");
      else if (title.startsWith("fold comment")) keys.push("fold");
      else if (title.startsWith("unfold comment")) keys.push("unfold");
      else if (title === "delete") keys.push("delete");
    }
    await this.page.keyboard.press("Escape");
    return keys;
  }

  async rulesChooseCommentMenuOption(commentId: string, option: RulesCommentMenuOption): Promise<void> {
    // Copying writes to the clipboard, which headless Chromium denies
    // without an explicit grant; arrange it before the pick.
    if (option === "copy_link") await this.page.context().grantPermissions(["clipboard-read", "clipboard-write"]);
    await this.rulesOpenMenu(commentId);
    const menu = this.rulesCard(commentId).getByRole("menu");
    await menu.getByRole("menuitem").first().waitFor();
    const title =
      option === "edit"
        ? "Edit"
        : option === "copy_link"
          ? "Copy link"
          : option === "access_switch"
            ? /Switch to (public|private) comment/
            : option === "fold"
              ? "Fold comment"
              : option === "unfold"
                ? "Unfold comment"
                : "Delete";
    // Same nested-button hit-testing reason as the trigger above.
    const item = menu.getByRole("menuitem", { name: title });
    const inner = item.locator("button");
    if ((await inner.count()) > 0) await this.rulesClickCoords(inner.first());
    else await this.rulesClickCoords(item);
  }

  async rulesReadClipboard(): Promise<string> {
    await this.page.context().grantPermissions(["clipboard-read", "clipboard-write"]);
    return this.page.evaluate(() => navigator.clipboard.readText());
  }

  async rulesOpenDeepLink(url: string): Promise<void> {
    await this.page.goto(url);
    await this.page.waitForLoadState("domcontentloaded");
    await this.rulesWaitForFeed();
  }

  async rulesCommentHighlighted(commentId: string): Promise<boolean> {
    const card = this.rulesCard(commentId);
    await card.waitFor();
    // The anchor highlight swaps the body border to the accent color.
    return (await card.locator(".border-accent-strong").count()) > 0;
  }

  async rulesCommentAccessBadge(commentId: string): Promise<"internal" | "public" | "hidden"> {
    const card = this.rulesCard(commentId);
    await card.waitFor();
    // The corner marker renders only while the project is externally
    // shared. The marker icons carry no text, so read the direction off
    // the overflow switch label instead: it names the state a pick would
    // move AWAY from.
    const marker = card.locator(".absolute.top-2\\.5.right-2\\.5");
    if ((await marker.count()) === 0) return "hidden";
    await this.rulesOpenMenu(commentId);
    const menu = card.getByRole("menu");
    await menu.getByRole("menuitem").first().waitFor();
    const titles = await menu.getByRole("menuitem").allTextContents();
    await this.page.keyboard.press("Escape");
    const switching = titles.map((t) => t.trim().toLowerCase()).find((t) => t.startsWith("switch to"));
    if (switching?.includes("private")) return "public";
    if (switching?.includes("public")) return "internal";
    throw new Error(`[parity] no visibility switch offered on ${commentId}.`);
  }

  async rulesLastToast(): Promise<{ title: string; message: string } | null> {
    // Toasts stack bottom-right and auto-dismiss; only a currently
    // visible one with text is reported, newest first.
    const roots = this.page.locator("div.absolute.right-3.bottom-3");
    const total = await roots.count();
    for (let i = total - 1; i >= 0; i--) {
      const text = (await roots.nth(i).innerText()).trim();
      if (text === "") continue;
      const lines = text
        .split("\n")
        .map((l) => l.trim())
        .filter((l) => l.length > 0);
      return { title: lines[0] ?? "", message: lines.slice(1).join(" ") };
    }
    return null;
  }

  async gotoJoinByEmailFromCreate(): Promise<void> {
    await this.page.getByRole("button", { name: "Join an existing workspace by admin email", exact: true }).click();
  }

  async gotoInvitesFromCreate(): Promise<void> {
    await this.page.getByRole("button", { name: "Join existing workspace", exact: true }).click();
  }

  async fillWorkspaceAdminEmail(email: string): Promise<void> {
    await this.page.getByLabel("Workspace admin email").fill(email);
  }

  async submitJoinRequest(): Promise<void> {
    await this.page.getByRole("button", { name: "Send join request", exact: true }).click();
  }

  async pendingApprovalNamesEmail(email: string): Promise<boolean> {
    await this.page.getByText("Waiting for approval", { exact: false }).first().waitFor();
    return this.hasVisibleText(email);
  }

  async createInsteadFromPending(): Promise<void> {
    await this.page.getByRole("button", { name: "Create your own workspace instead", exact: true }).click();
  }

  async selectInviteByWorkspace(workspaceName: string): Promise<void> {
    // Each invite row shows the workspace name and a checkbox. Find the row
    // carrying the name and toggle its checkbox; fall back to the sole
    // checkbox when the row grouping is not addressable.
    const row = this.page
      .locator("div")
      .filter({ hasText: workspaceName })
      .filter({ has: this.page.getByRole("checkbox") });
    if ((await row.count()) > 0) {
      await row.last().getByRole("checkbox").first().click();
      return;
    }
    await this.page.getByRole("checkbox").first().click();
  }

  async continueWithSelectedInvites(): Promise<void> {
    await this.page.getByRole("button", { name: "Continue", exact: true }).click();
  }

  async awaitInviteMembersStep(): Promise<void> {
    await this.page.getByRole("heading", { name: "Invite your teammates", exact: true }).waitFor({ timeout: 30_000 });
  }

  async isInviteMembersStepVisible(): Promise<boolean> {
    return this.isShown(this.page.getByRole("heading", { name: "Invite your teammates", exact: true }));
  }

  async inviteRowCount(): Promise<number> {
    return this.page.locator('input[name^="emails."][name$=".email"]').count();
  }

  async fillInviteRow(index: number, email: string): Promise<void> {
    await this.page.locator(`input[name="emails.${index}.email"]`).fill(email);
  }

  async clickAddAnotherInvite(): Promise<void> {
    await this.page.getByRole("button", { name: "Add another", exact: true }).click();
  }

  async isSendInvitesDisabled(): Promise<boolean> {
    return this.page.getByRole("button", { name: "Continue", exact: true }).isDisabled();
  }

  async sendInvites(): Promise<void> {
    await this.page.getByRole("button", { name: "Continue", exact: true }).click();
  }

  async deferInvites(): Promise<void> {
    // The label uses a curly apostrophe; match either form.
    await this.page.getByRole("button", { name: /I.?ll do it later/ }).click();
  }

  /** The icon-only header back control: the empty-text button in the sticky header. */
  private headerBackButton(): Locator {
    return this.page.locator('div.sticky.top-0.z-10 button[type="button"]').filter({ hasText: /^\s*$/ });
  }

  async isOnboardingBackVisible(): Promise<boolean> {
    return this.isShown(this.headerBackButton());
  }

  async clickOnboardingBack(): Promise<void> {
    await this.headerBackButton().first().click();
  }

  async isTourWelcomeVisible(): Promise<boolean> {
    return this.hasVisibleText("Take a Product Tour");
  }

  async declineTour(): Promise<void> {
    await this.page.getByRole("button", { name: "No thanks, I will explore it myself", exact: true }).click();
  }

  async isStandaloneCreationDisabledVisible(): Promise<boolean> {
    return this.hasVisibleText("Only your instance admin can create workspaces");
  }

  async isRequestInstanceAdminLinkVisible(): Promise<boolean> {
    return this.isShown(this.page.getByRole("link", { name: "Request instance admin", exact: true }));
  }

  async isInOnboardingCreationDisabledNoticeVisible(): Promise<boolean> {
    return this.hasVisibleText("your instance admin has restricted creation");
  }

  async rulesReload(): Promise<void> {
    await this.page.reload();
    await this.page.waitForLoadState("domcontentloaded");
    await this.rulesWaitForFeed();
  }

  async rulesEnsureSignedIn(email: string, password: string, workspaceSlug: string): Promise<void> {
    for (let attempt = 0; attempt < 3; attempt++) {
      await this.openEntry();
      await this.signInWithPassword(email, password);
      const landed = await this.page
        .waitForURL(new RegExp(workspaceSlug.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")), { timeout: 60_000 })
        .then(() => true)
        .catch(() => false);
      if (landed) return;
    }
    throw new Error(`[parity] sign-in never landed on ${workspaceSlug}.`);
  }

  async rulesOpenIssueDetailRaw(workspaceSlug: string, projectId: string, issueId: string): Promise<void> {
    // Same navigation as the full open, minus the feed wait: a refused
    // viewer never renders the activity section, so waiting would hang.
    await this.page.goto(`/${workspaceSlug}/projects/${projectId}/issues/${issueId}`);
    await this.page.waitForLoadState("domcontentloaded");
  }

  async rulesIssueMissingVisible(): Promise<boolean> {
    // A refused viewer lands on the does-not-exist empty state instead of
    // the detail screen; the text is the user-visible signal.
    const missing = this.page.getByText("Work item does not exist").first();
    await missing.waitFor({ timeout: 60_000 });
    return missing.isVisible();
  }

  async rulesCommentComposerVisible(): Promise<boolean> {
    // Immediate read, no waiting: several specs assert the composer's
    // absence, where a wait would only burn the timeout.
    return (await this.page.getByRole("group", { name: "Add comment" }).count()) > 0;
  }

  async rulesCommentCardVisible(commentId: string): Promise<boolean> {
    // Immediate read, no waiting: callers poll for presence and assert
    // absence directly.
    return (await this.rulesCard(commentId).count()) > 0;
  }

  async rulesCommentCardText(commentId: string): Promise<string> {
    const card = this.rulesCard(commentId);
    await card.waitFor();
    return (await card.innerText()).trim();
  }

  async rulesIntakeTriageVisible(): Promise<boolean> {
    // Accept/Decline prove the intake variant; each is awaited so a slow
    // triage chain cannot read as absent, then confirmed visible so a
    // prerendered-but-hidden control cannot pass.
    const accept = this.page.getByRole("button", { name: "Accept" }).first();
    const decline = this.page.getByRole("button", { name: "Decline" }).first();
    await accept.waitFor({ timeout: 60_000 });
    await decline.waitFor({ timeout: 60_000 });
    return (await accept.isVisible()) && (await decline.isVisible());
  }
  // --- NEWFRONT-123 (home): selectors observed on the running old app. ---
  // The dashboard centers on a narrow column: a greeting heading carrying
  // the salutation plus the user name, a date-and-clock sub-line, an
  // assistant card, then the widget stack titled per widget.

  private homeMain(): Locator {
    return this.page.getByRole("main");
  }

  private openDialog(): Locator {
    // The open dialog wraps its panel in a zero-height mount (the panel
    // itself is fixed-positioned), so visibility waits must target the
    // visible content inside, never the container.
    return this.page.locator('[role="dialog"][data-headlessui-state="open"]').first();
  }

  private async waitDialogSettled(): Promise<void> {
    await this.openDialog().locator("div:visible").first().waitFor({ timeout: 30_000 });
  }

  private async needFirst(target: Locator, what: string): Promise<Locator> {
    // Clicking a possibly-empty locator burns the whole test budget, so
    // every action asserts presence first and fails fast with a name.
    const first = target.first();
    if ((await first.count()) === 0) throw new Error(`[parity] home ${what} is missing.`);
    return first;
  }

  async homeOpen(workspaceSlug: string): Promise<void> {
    // Settles when the greeting, the tour overlay, or a loading skeleton
    // shows up; the dev server compiles the route on first visit, so the
    // navigation itself is retried once on a slow first paint.
    for (let attempt = 0; attempt < 2; attempt += 1) {
      await this.page.goto(`/${workspaceSlug}/`);
      await this.page.waitForLoadState("domcontentloaded");
      const settled = await this.page
        .locator("h2, h3, div.fixed")
        .first()
        .waitFor({ timeout: 90_000 })
        .then(() => true)
        .catch(() => false);
      if (settled) return;
    }
    throw new Error(`[parity] home dashboard never settled at ${this.page.url()}.`);
  }

  async homeGreetingHeading(): Promise<string | null> {
    const heading = this.homeMain().locator("h2").first();
    if ((await heading.count()) === 0) return null;
    const text = (await heading.innerText()).trim();
    return text === "" ? null : text;
  }

  async homeDateLine(): Promise<string | null> {
    // The sub-line sits next to the greeting heading inside the same
    // centered block; it carries the weekday, date and live clock.
    const heading = this.homeMain().locator("h2").first();
    if ((await heading.count()) === 0) return null;
    const block = heading.locator("xpath=ancestor::div[./h2][1]");
    const sub = block.locator("h5").first();
    if ((await sub.count()) === 0) return null;
    const text = (await sub.innerText()).trim();
    return text === "" ? null : text;
  }

  private homeTourButtonScope(): Locator {
    // The tour renders above the dashboard in a full-screen fixed overlay.
    // The welcome card offers starting or declining; each step offers
    // Back/Next; the last step offers finishing into project creation.
    // Scoping on those controls keeps middle steps visible too.
    return this.page.locator("div.fixed").filter({
      has: this.page.getByRole("button", {
        name: /take a product tour|no thanks|^next$|^back$|create your first project/i,
      }),
    });
  }

  async homeTourVisible(): Promise<boolean> {
    return (await this.homeTourButtonScope().first().count()) > 0;
  }

  async homeTourAdvance(): Promise<void> {
    const overlay = this.homeTourButtonScope().first();
    const start = overlay.getByRole("button", { name: /take a product tour/i }).first();
    if ((await start.count()) > 0) {
      await start.click();
      return;
    }
    const next = overlay.getByRole("button", { name: /^next$/i }).first();
    if ((await next.count()) > 0) {
      await next.click();
      return;
    }
    await overlay
      .getByRole("button", { name: /create your first project/i })
      .first()
      .click();
  }

  async homeTourDismiss(): Promise<void> {
    const overlay = this.homeTourButtonScope().first();
    const decline = overlay.getByRole("button", { name: /no thanks/i }).first();
    if ((await decline.count()) > 0) {
      await decline.click();
      return;
    }
    // Mid-tour the overlay offers an icon-only close control instead.
    await overlay.getByRole("button").filter({ hasNotText: /\S/ }).first().click();
  }

  private homeAssistantCard(): Locator {
    return this.homeMain().locator("div").filter({ hasText: "Pi Dash AI" }).first();
  }

  async homeAssistantState(): Promise<"hidden" | "setup" | "ready"> {
    if ((await this.homeAssistantCard().count()) === 0) return "hidden";
    const text = (await this.homeAssistantCard().innerText()).toLowerCase();
    if (text.includes("api key")) return "setup";
    return "ready";
  }

  async homeAssistantSuggestions(): Promise<string[]> {
    if ((await this.homeAssistantCard().count()) === 0) return [];
    const texts = await this.homeAssistantCard().getByRole("button").allTextContents();
    return texts.map((t) => t.trim()).filter((t) => t.length > 0 && t.toLowerCase() !== "ask");
  }

  private homeQuickstartHeader(): Locator {
    return this.homeMain().getByText("Your quickstart guide").first();
  }

  private homeQuickstartPanel(): Locator {
    // The header row's grandparent is the panel root: header text, then
    // the flex row, then the panel container holding the card grid.
    return this.homeQuickstartHeader().locator("xpath=ancestor::div[2]");
  }

  async homeQuickstartVisible(): Promise<boolean> {
    return (await this.homeQuickstartHeader().count()) > 0;
  }

  async homeQuickstartTitles(): Promise<string[]> {
    if (!(await this.homeQuickstartVisible())) return [];
    const texts = await this.homeQuickstartPanel().locator("h3").allTextContents();
    return [...new Set(texts.map((t) => t.trim()).filter((t) => t.length > 0))];
  }

  async homeQuickstartCreateEnabled(): Promise<boolean> {
    if (!(await this.homeQuickstartVisible())) return false;
    const actions = await this.homeQuickstartActionTexts();
    return actions.some((text) => /get started/i.test(text));
  }

  async homeQuickstartDismiss(): Promise<void> {
    const panel = this.homeQuickstartPanel();
    await (
      await this.needFirst(panel.getByRole("button", { name: /not right now/i }), "quickstart dismiss control")
    ).click();
  }

  private homeQuickstartCard(title: string): Locator {
    const panel = this.homeQuickstartPanel();
    return panel
      .locator("div")
      .filter({ has: this.page.getByRole("heading", { name: title }) })
      .last();
  }

  async homeQuickstartCardDone(title: string): Promise<boolean> {
    // A finished card swaps its call-to-action for a green marker pill
    // (the card keeps its icon, so icon presence alone proves nothing).
    // Absence of any action is NOT done: forbidden actions also render
    // nothing clickable, which the role-gating scenarios pin separately.
    const card = this.homeQuickstartCard(title);
    if ((await card.count()) === 0) return false;
    return (await card.locator('[class*="17a34a"]').count()) > 0;
  }

  async homeQuickstartActionTexts(): Promise<string[]> {
    if (!(await this.homeQuickstartVisible())) return [];
    const panel = this.homeQuickstartPanel();
    const links = await panel.getByRole("link").allTextContents();
    const buttons = await panel.getByRole("button").allTextContents();
    return [...links, ...buttons].map((t) => t.trim()).filter((t) => t.length > 0 && !/not right now/i.test(t));
  }

  async homeWidgetTitles(): Promise<string[]> {
    // Section titles in render order, so reorder and refresh assertions
    // observe the sequence the user actually sees.
    const nodes = this.homeMain().locator(
      "xpath=.//div[normalize-space()='Quicklinks' or normalize-space()='Recents']"
    );
    const texts = await nodes.allTextContents();
    return texts.map((t) => t.trim()).filter((t) => t.length > 0);
  }

  async homeOpenManageWidgets(): Promise<void> {
    // The header control carries its label as its accessible name (an
    // icon button with no text children), so the lookup is by name; the
    // header-last-button fallback stays for older layouts.
    const headerButton = this.page.getByRole("button", { name: /widget/i }).first();
    if ((await headerButton.count()) > 0) {
      await headerButton.click();
    } else {
      const fallback = this.page.locator("header").getByRole("button").last();
      if ((await fallback.count()) === 0) throw new Error("[parity] home manage-widgets control is missing.");
      await fallback.click();
    }
    await this.waitDialogSettled();
  }

  async homeCloseManageWidgets(): Promise<void> {
    await this.page.keyboard.press("Escape");
  }

  async homeManageWidgetNames(): Promise<string[]> {
    const dialog = this.openDialog();
    const texts = await dialog.locator("p, span, div").allTextContents();
    return [...new Set(texts.map((t) => t.trim()).filter((t) => t.length > 0 && t.length < 60))];
  }

  private homeManageName(name: string): Locator {
    return this.openDialog().getByText(name, { exact: true }).first();
  }

  private homeManageRow(name: string): Locator {
    // The name cell's parent holds the drag handle for that widget.
    return this.homeManageName(name).locator("xpath=ancestor::div[1]");
  }

  private homeManageSwitch(name: string): Locator {
    // The toggle sits beside the name cell inside their shared wrapper.
    return this.homeManageName(name).locator("xpath=ancestor::div[2]").getByRole("switch").first();
  }

  async homeManageWidgetEnabled(name: string): Promise<boolean> {
    const toggle = await this.needFirst(this.homeManageSwitch(name), `widget toggle for ${name}`);
    return (await toggle.getAttribute("aria-checked")) === "true";
  }

  async homeToggleManageWidget(name: string): Promise<void> {
    await (await this.needFirst(this.homeManageSwitch(name), `widget toggle for ${name}`)).click();
  }

  async homeDragWidget(sourceName: string, targetName: string): Promise<void> {
    // The drag listener lives on the row's handle button, so the handle
    // starts the gesture; dropping near the target row's bottom edge
    // asks for a below-drop, which visibly moves a first-row widget.
    const source = this.homeManageRow(sourceName).getByRole("button").first();
    const targetRow = this.homeManageRow(targetName);
    const srcBox = await source.boundingBox();
    if (!srcBox) throw new Error("[parity] widget drag source has no layout box.");
    const dstBox = await targetRow.boundingBox();
    if (!dstBox) throw new Error("[parity] widget drag target has no layout box.");
    await this.page.mouse.move(srcBox.x + srcBox.width / 2, srcBox.y + srcBox.height / 2);
    await this.page.mouse.down();
    await this.page.mouse.move(dstBox.x + dstBox.width / 2, dstBox.y + dstBox.height - 6, { steps: 12 });
    await this.page.mouse.up();
  }

  async homeAllOffVisible(): Promise<boolean> {
    return (
      (await this.homeMain()
        .getByText(/without widgets/i)
        .count()) > 0
    );
  }

  private homeLinksTitle(): Locator {
    return this.homeMain().getByText("Quicklinks", { exact: true }).first();
  }

  private homeLinksSection(): Locator {
    // Title, then the header row, then the section holding the rows.
    return this.homeLinksTitle().locator("xpath=ancestor::div[2]");
  }

  async homeQuickLinkNames(): Promise<string[]> {
    if ((await this.homeLinksTitle().count()) === 0) return [];
    // Rows render as plain cards (title line plus relative-age line),
    // not anchors, so each row is recognized by that two-line shape.
    return this.homeLinksSection().evaluate((root) => {
      const age = /ago|just now|less than/i;
      const titles = new Set<string>();
      root.querySelectorAll("div").forEach((el) => {
        const lines = (el.innerText ?? "")
          .split("\n")
          .map((line) => line.trim())
          .filter((line) => line !== "");
        const [title, second] = lines;
        if (lines.length === 2 && title !== undefined && second !== undefined && !age.test(title) && age.test(second)) {
          titles.add(title);
        }
      });
      return [...titles];
    });
  }

  private homeLinkRow(title: string): Locator {
    // The row card sits two levels above the title line: the title leaf
    // lives in a text wrapper inside the clickable card, and the card
    // also holds the hover-revealed menu trigger a sibling lookup needs.
    const exact = title.replace(/"/g, "");
    return this.homeLinksSection().locator(`xpath=.//div[normalize-space()="${exact}"]/../..`).first();
  }

  async homeExpandQuickLinks(): Promise<void> {
    await (
      await this.needFirst(this.homeLinksSection().getByRole("button", { name: /show all/i }), "link list expander")
    ).click();
  }

  async homeQuickLinksCollapsed(): Promise<boolean> {
    if ((await this.homeLinksTitle().count()) === 0) return false;
    return (
      (await this.homeLinksSection()
        .getByRole("button", { name: /show all|show less/i })
        .count()) > 0
    );
  }

  private async homeLinkFormFill(title: string, url: string): Promise<void> {
    // The dialog asks for the address first, then the display name.
    const dialog = this.openDialog();
    await (await this.needFirst(dialog.locator("#url"), "link address field")).fill(url);
    await (await this.needFirst(dialog.locator("#title"), "link title field")).fill(title);
  }

  async homeAddQuickLink(title: string, url: string): Promise<void> {
    await (await this.needFirst(this.homeLinksSection().getByRole("button"), "link add control")).click();
    await this.waitDialogSettled();
    await this.homeLinkFormFill(title, url);
    const dialog = this.openDialog();
    await dialog
      .getByRole("button", { name: /save|create|add|submit/i })
      .first()
      .click();
  }

  private async homeLinkMenuPick(name: RegExp): Promise<void> {
    // The row menu offers its options as menu items; button-shaped
    // menus stay as a fallback. The open signal is the option itself,
    // never the menu container (zero-size when open, like the recents
    // filter).
    const item = this.page.getByRole("menuitem", { name }).first();
    try {
      await item.waitFor({ state: "visible", timeout: 5_000 });
      await item.click();
      return;
    } catch {
      // Fall through to button-shaped menus.
    }
    await (await this.needFirst(this.page.getByRole("button", { name }), "link menu option")).click();
  }

  async homeEditQuickLink(currentTitle: string, nextTitle: string, nextUrl: string): Promise<void> {
    await this.homeLinkRowMenu(currentTitle);
    await this.homeLinkMenuPick(/edit/i);
    await this.waitDialogSettled();
    await this.homeLinkFormFill(nextTitle, nextUrl);
    const dialog = this.openDialog();
    await dialog
      .getByRole("button", { name: /save|update/i })
      .first()
      .click();
  }

  async homeDeleteQuickLink(title: string): Promise<void> {
    await this.homeLinkRowMenu(title);
    await this.homeLinkMenuPick(/delete|remove/i);
    const confirm = this.openDialog();
    if ((await confirm.count()) > 0) {
      await confirm
        .getByRole("button", { name: /delete|remove|confirm/i })
        .first()
        .click();
    }
  }

  async homeLinkDialogError(): Promise<string | null> {
    const dialog = this.openDialog();
    if ((await dialog.count()) === 0) return null;
    const text = (await dialog.innerText()).trim();
    if (/invalid|required|enter|address|url/i.test(text)) return text;
    return null;
  }

  private async homeLinkRowMenu(title: string): Promise<void> {
    // Rows expose open, copy, edit and delete through a context menu
    // whose trigger reveals on hover (with an animation beat).
    const row = await this.needFirst(this.homeLinkRow(title), `reference-link row for ${title}`);
    await row.hover();
    const trigger = row.getByRole("button").first();
    try {
      await trigger.waitFor({ state: "visible", timeout: 5_000 });
      await trigger.click();
      return;
    } catch {
      // No hover trigger: fall back to a context click.
    }
    await row.click({ button: "right" });
  }

  async homeCopyQuickLink(title: string): Promise<void> {
    // The page writes through the async clipboard API, which needs the
    // permission before the click, not just before the read.
    await this.page.context().grantPermissions(["clipboard-read", "clipboard-write"]);
    await this.homeLinkRowMenu(title);
    await this.homeLinkMenuPick(/copy/i);
  }

  async homeReadClipboard(): Promise<string> {
    await this.page.context().grantPermissions(["clipboard-read", "clipboard-write"]);
    return this.page.evaluate(() => navigator.clipboard.readText());
  }

  async homeOpenQuickLinkPopup(title: string): Promise<string | null> {
    // Clicking the row card opens the saved address in a new tab.
    const row = await this.needFirst(this.homeLinkRow(title), `reference-link row for ${title}`);
    const [popup] = await Promise.all([
      this.page.waitForEvent("popup", { timeout: 15_000 }).catch(() => null),
      row.click(),
    ]);
    if (popup === null) return null;
    await popup.waitForLoadState("domcontentloaded").catch(() => undefined);
    return popup.url();
  }

  async homeLinkDialogOpen(): Promise<boolean> {
    return (await this.page.locator('[role="dialog"] input:visible').count()) > 0;
  }

  async homeCancelLinkDialog(): Promise<void> {
    await this.openDialog()
      .getByRole("button", { name: /cancel/i })
      .first()
      .click();
  }

  private homeRecentsTitle(): Locator {
    return this.homeMain().getByText("Recents", { exact: true }).first();
  }

  private homeRecentsSection(): Locator {
    // Title, then the header row, then the section holding the rows.
    return this.homeRecentsTitle().locator("xpath=ancestor::div[2]");
  }

  async homeSetRecentsFilter(name: "all" | "issue" | "page" | "project"): Promise<void> {
    const section = this.homeRecentsSection();
    const labels: Record<string, RegExp> = {
      all: /^all$/i,
      issue: /work item/i,
      page: /^pages?$/i,
      project: /^projects?$/i,
    };
    // The trigger nests the active-filter label as its own button, so a
    // positional click lands on the label. Focusing the menu button and
    // opening it from the keyboard reaches the real control instead.
    // Proven by probe: the open menu container itself resolves hidden (a
    // zero-size fixed box), so the open signal is the option, never the
    // container — same class as the headlessui dialog root cause above.
    const trigger = section.locator('button[aria-haspopup="menu"]').first();
    if ((await trigger.count()) === 0) throw new Error("[parity] recents filter is missing.");
    const options = this.page.getByRole("menuitem", { name: labels[name] });
    // Open like the user: a plain click on the trigger (it lands on the
    // nested label and bubbles to the toggle). Keyboard Enter stays as
    // the fallback when the click does not open the menu. The option
    // node is pinned before clicking so a re-render cannot swap the
    // target between the wait and the click.
    try {
      await trigger.click({ timeout: 10_000 });
      await options.first().waitFor({ state: "visible", timeout: 10_000 });
    } catch {
      await trigger.focus();
      await this.page.keyboard.press("Enter");
      await options.first().waitFor({ state: "visible", timeout: 10_000 });
    }
    const handle = await options.first().elementHandle();
    if (handle === null) throw new Error("[parity] recents filter option never stabilized.");
    // The feed loads async behind the open menu, so layout shifts keep
    // the option off Playwright's stability bar even though a user can
    // click it; dispatch on the pinned visible node instead (the inner
    // button carries the selection handler). The specs prove the
    // selection took by polling the refetched rows after.
    const button = await handle.$("button");
    await (button ?? handle).evaluate((element) => (element as HTMLElement).click());
  }

  async homeRecentRowTexts(): Promise<string[]> {
    if ((await this.homeRecentsTitle().count()) === 0) return [];
    const rows = await this.homeRecentsSection().getByRole("link").allTextContents();
    return [...new Set(rows.map((t) => t.trim()).filter((t) => t.length > 0))];
  }

  async homeOpenRecentRow(text: string): Promise<void> {
    const section = this.homeRecentsSection();
    // The feed clips its overflow behind a fading gradient that swallows
    // pointer events (a slimmer one lingers even expanded), and sibling
    // suites share the owner's visit history, so the target row can sit
    // anywhere in a long feed: expand, then center the row away from
    // both edges before clicking.
    const expander = section.getByRole("button", { name: /show all/i }).first();
    if ((await expander.count()) > 0) {
      await expander.click();
    }
    const row = await this.needFirst(
      section.getByRole("link", { name: new RegExp(text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"), "i") }),
      `recent row for ${text}`
    );
    await row.evaluate((element) => element.scrollIntoView({ block: "center" }));
    await row.click();
  }

  async homeIssuePreviewVisible(): Promise<boolean> {
    // The work-item preview mounts as a side panel inside the fullscreen
    // portal without leaving home; absence is an honest negative signal.
    const portal = this.page.locator("#full-screen-portal").first();
    if ((await portal.count()) === 0) return false;
    return (await portal.locator("div:visible").first().count()) > 0;
  }

  async homeIssuePreviewText(): Promise<string> {
    // The preview mixes rendered text with editable fields (the title
    // is an input), so both are combined for assertions.
    const portal = this.page.locator("#full-screen-portal").first();
    if ((await portal.count()) === 0) return "";
    const text = await portal.innerText().catch(() => "");
    const boxes = portal.getByRole("textbox");
    const values: string[] = [];
    for (let index = 0; index < (await boxes.count()); index += 1) {
      values.push(
        await boxes
          .nth(index)
          .inputValue()
          .catch(() => "")
      );
    }
    return [text, ...values].join("\n");
  }

  async homeBreadcrumb(): Promise<string | null> {
    // The dashboard header pairs a home breadcrumb with the manage-widgets
    // control (an icon button whose label is its accessible name, with no
    // text children). The crumb is the exact Home label inside the same
    // content landmark as the control (the sidebar carries its own Home
    // link in a separate landmark).
    const control = this.page.getByRole("button", { name: /manage widgets/i }).first();
    if ((await control.count()) === 0) return null;
    const scope = control.locator("xpath=ancestor::main[1]");
    const crumb = scope.getByText("Home", { exact: true }).first();
    if ((await crumb.count()) === 0) return null;
    const text = (await crumb.innerText()).trim();
    return text === "" ? null : text;
  }

  async homeLastToast(): Promise<{ title: string; message: string } | null> {
    // Toasts stack bottom-right and dismiss after a few seconds, so
    // callers poll this right after the triggering action.
    const candidates = this.page.locator('[class*="right-3"][class*="bottom-3"], [role="status"], [role="alert"]');
    const count = await candidates.count();
    if (count === 0) return null;
    const text = ((await candidates.last().innerText()).trim() || "")
      .split("\n")
      .map((t) => t.trim())
      .filter(Boolean);
    if (text.length === 0) return null;
    return { title: text[0] ?? "", message: text.slice(1).join(" ") };
  }

  async homeReload(): Promise<void> {
    await this.page.reload();
    await this.page.waitForLoadState("domcontentloaded");
  }

  async homeOpenIssueDetail(workspaceSlug: string, projectId: string, issueId: string): Promise<void> {
    await this.page.goto(`/${workspaceSlug}/projects/${projectId}/issues/${issueId}`);
    await this.page.waitForLoadState("domcontentloaded");
  }

  async homeWaitForWidgets(): Promise<string[]> {
    // A failed widget fetch leaves the loader mounted with no retry, so a
    // scratch-stack hiccup bricks that paint until the next load.
    const read = () => this.homeWidgetTitles();
    const first = await read();
    if (first.length > 0) return first;
    await this.page.waitForTimeout(10_000);
    const second = await read();
    if (second.length > 0) return second;
    await this.homeReload();
    const settled = await this.page
      .locator("h2, h3, div.fixed")
      .first()
      .waitFor({ timeout: 90_000 })
      .then(() => true)
      .catch(() => false);
    if (!settled) return [];
    await this.page.waitForTimeout(5_000);
    return read();
  }

  async homeSkeletonVisible(): Promise<boolean> {
    // Loaders render pulsing placeholder tiles; any animate-pulse node or
    // skeleton-styled block while the dashboard settles counts.
    const pulsing = this.page.locator(".animate-pulse, [data-testid*='skeleton' i], [aria-busy='true']").first();
    if ((await pulsing.count()) > 0) return true;
    return false;
  }

  // --- Comment composer and CRUD (NEWFRONT-112). Selectors observed on
  // the running old app against the seeded stack; no app code is reused.

  /** The composer box labelled "Add comment" in the activity section. */
  private composerBox(): Locator {
    return this.page.getByRole("group", { name: "Add comment" });
  }

  /** The editable rich-text surface inside the composer box. */
  private composerEditor(): Locator {
    return this.composerBox().locator('[contenteditable="true"]');
  }

  /** The composer submit button (the "Comment" action, not "Comment & Run"). */
  private composerSubmitButton(): Locator {
    return this.composerBox().getByRole("button", { name: "Comment", exact: true });
  }

  /** The card element for the comment showing the given text. */
  private commentCard(text: string): Locator {
    return this.page.locator('div[id^="comment-"]', { hasText: text });
  }

  /** The editable surface of the open inline edit form. */
  private commentEditEditor(): Locator {
    return this.page.locator('div[id^="comment-"] [contenteditable="true"]');
  }

  async composerOpenIssue(workspaceSlug: string, issueRef: string): Promise<void> {
    // domcontentloaded (not full load): same cold-dev-server reasoning as
    // openEntry; the Activity wait below is the readiness gate.
    await this.page.goto(`/${workspaceSlug}/browse/${issueRef}/`, { waitUntil: "domcontentloaded", timeout: 300_000 });
    // The first navigation after sign-in compiles the work-item bundle in
    // the dev server; observed 80s+ on a loaded host with 120s flaky, so
    // the budget is 300s (the scenario timeout is 600s).
    await this.page.getByText("Activity").first().waitFor({ timeout: 300_000 });
  }

  async composerType(text: string): Promise<void> {
    const editor = this.composerEditor();
    await editor.click();
    await editor.pressSequentially(text);
  }

  async composerPasteHtml(html: string): Promise<void> {
    const page = this.page;
    await page.context().grantPermissions(["clipboard-read", "clipboard-write"]);
    await page.evaluate(
      ([fragment]: [string]) => {
        const blobHtml = new Blob([fragment], { type: "text/html" });
        const blobText = new Blob([fragment.replace(/<[^>]*>/g, "")], { type: "text/plain" });
        return navigator.clipboard.write([new ClipboardItem({ "text/html": blobHtml, "text/plain": blobText })]);
      },
      [html]
    );
    const editor = this.composerEditor();
    await editor.click();
    await editor.press(`${process.platform === "darwin" ? "Meta" : "Control"}+v`);
  }

  async composerDraftText(): Promise<string> {
    const raw = await this.composerEditor().innerText();
    return raw.trim();
  }

  async composerSubmitDisabled(): Promise<boolean> {
    return this.composerSubmitButton().isDisabled();
  }

  async composerSubmit(): Promise<void> {
    // Fail fast when the composer never arms (e.g. a stuck upload leaves
    // submit disabled): a blind click would hang until the test timeout.
    const button = this.composerSubmitButton();
    await button.waitFor({ state: "visible", timeout: 30_000 });
    await expect(button).toBeEnabled({ timeout: 30_000 });
    await button.click({ timeout: 30_000 });
  }

  async composerPressEnter(): Promise<void> {
    const editor = this.composerEditor();
    await editor.focus();
    await this.page.keyboard.press("Enter");
  }

  async composerPressShiftEnter(): Promise<void> {
    const editor = this.composerEditor();
    await editor.focus();
    await this.page.keyboard.press("Shift+Enter");
  }

  async composerAttachFile(path: string): Promise<void> {
    const page = this.page;
    const attachButton = this.composerBox().locator('button:has(svg[class*="image"])');
    const [chooser] = await Promise.all([
      page.waitForEvent("filechooser", { timeout: 30_000 }),
      attachButton.click({ timeout: 30_000 }),
    ]);
    await chooser.setFiles(path);
  }

  async composerVisibleCommentTexts(): Promise<string[]> {
    const cards = this.page.locator('div[id^="comment-"]');
    const count = await cards.count();
    const bodies: string[] = [];
    for (let index = 0; index < count; index += 1) {
      const body = cards.nth(index).locator('[contenteditable="false"]').first();
      if ((await body.count()) === 0) continue;
      bodies.push(((await body.innerText()) ?? "").trim());
    }
    return bodies;
  }

  async composerOpenCommentMenu(text: string): Promise<void> {
    const card = this.commentCard(text);
    await card.scrollIntoViewIfNeeded();
    await card.locator("[data-main-menu] > button").click({ timeout: 30_000 });
  }

  async composerMenuClick(item: string): Promise<void> {
    await this.page.getByRole("menuitem", { name: item }).click({ timeout: 30_000 });
  }

  async composerEditType(text: string): Promise<void> {
    const editor = this.commentEditEditor();
    await editor.click();
    await editor.press(`${process.platform === "darwin" ? "Meta" : "Control"}+a`);
    await editor.pressSequentially(text);
  }

  /** The card holding the open inline edit form (exactly one is ever open). */
  private commentEditCard(): Locator {
    // Found from the open editor upward: a `{ has: <page-scoped locator> }`
    // filter cannot see the card, since the inner locator must match a
    // *descendant* of each candidate, not the page.
    return this.commentEditEditor().locator('xpath=ancestor::div[starts-with(@id, "comment-")][1]');
  }

  async composerEditSave(): Promise<void> {
    // The edit form renders save (check) then discard (cross) side by side
    // whenever the draft is non-empty; the save control leads.
    await this.commentEditCard().locator('form div[class*="self-end"] button').first().click({ timeout: 30_000 });
  }

  async composerEditDiscard(): Promise<void> {
    await this.commentEditCard().locator('form div[class*="self-end"] button').last().click({ timeout: 30_000 });
  }

  async composerEditSaveDisabled(): Promise<boolean> {
    return this.commentEditCard().locator('form div[class*="self-end"] button').first().isDisabled();
  }

  async composerEditPressEnter(): Promise<void> {
    const editor = this.commentEditEditor();
    await editor.focus();
    await this.page.keyboard.press("Enter");
  }

  async composerCommentMeta(text: string): Promise<{
    author: string;
    time: string;
    edited: boolean;
    tooltip: string | null;
  }> {
    const card = this.commentCard(text);
    // Center the card first: the sticky composer overlaps viewport edges,
    // so a minimal scroll can park the time span under it where hover
    // events never land.
    await card.evaluate((element) => element.scrollIntoView({ block: "center" }));
    const author = (
      (await card
        .locator("div.text-caption-sm-medium")
        .first()
        .innerText()
        .catch(() => "")) ?? ""
    ).trim();
    const timeSpan = card.locator('span[tabindex="0"]').first();
    const time = ((await timeSpan.innerText().catch(() => "")) ?? "").trim();
    const tip = this.page.locator(".bp4-tooltip2");
    // The exact-time tooltip is hover-triggered (200ms open delay); focus
    // alone never opens it.
    let tooltip: string | null = null;
    try {
      await timeSpan.hover({ timeout: 10_000 });
      await tip.first().waitFor({ timeout: 10_000 });
      tooltip =
        (
          (await tip
            .first()
            .innerText()
            .catch(() => "")) ?? ""
        ).trim() || null;
    } catch {
      tooltip = null;
    }
    // Park the mouse away so the tooltip closes; a lingering portal would
    // be misread as the next card's tooltip.
    await this.page.mouse.move(8, 8);
    await tip
      .first()
      .waitFor({ state: "detached", timeout: 5_000 })
      .catch(() => undefined);
    return { author, time, edited: time.includes("(edited)"), tooltip };
  }

  async composerCommentImageCount(text: string): Promise<number> {
    // Attached images render as embedded image nodes (an <img> each) in
    // the read-only card body; the card shows no file-name list, and the
    // header avatar lives outside the body, so body <img> count is exact.
    const card = this.commentCard(text);
    await card.scrollIntoViewIfNeeded();
    return card.locator('[contenteditable="false"] img').count();
  }

  async composerVisibleNotices(): Promise<{ message: string; kind: "success" | "error" | "unknown" }[]> {
    const dialogs = this.page.locator('div[aria-label="Notifications"] div[role="dialog"]');
    const count = await dialogs.count();
    const notices: { message: string; kind: "success" | "error" | "unknown" }[] = [];
    for (let index = 0; index < count; index += 1) {
      const dialog = dialogs.nth(index);
      const message = ((await dialog.innerText().catch(() => "")) ?? "").trim();
      if (message.length === 0) continue;
      let kind: "success" | "error" | "unknown" = "unknown";
      if ((await dialog.locator('[class*="bg-success"]').count()) > 0) kind = "success";
      else if ((await dialog.locator('[class*="bg-danger"], [class*="bg-error"]').count()) > 0) kind = "error";
      notices.push({ message, kind });
    }
    return notices;
  }
}
