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
import type { Locator, Page } from "@playwright/test";
import type { ParityBrowserCookie, ParityDriver, ParityTarget, WorkspaceOnboardingView } from "./parity-driver";

export class WebDriver implements ParityDriver {
  readonly target: ParityTarget = "web";
  readonly page: Page;

  constructor(page: Page) {
    this.page = page;
  }

  /** Every wait below is explicitly bounded: the suite config leaves action and navigation timeouts at Playwright's unbounded defaults, so a bare waitFor would hang to the test timeout instead of failing honestly. */
  private static readonly WAIT_MS = 30_000;

  async openEntry(): Promise<void> {
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
    const page = this.page;
    const emailField = page.getByPlaceholder("name@company.com").first();
    await emailField.fill(email);
    const emailForm = page.locator("form", { has: emailField });
    await this.submitOf(emailForm).click();
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
}
