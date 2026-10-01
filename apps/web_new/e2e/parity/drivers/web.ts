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
import type {
  LayoutsLayoutKey,
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

  // --- NEWFRONT-117 (layouts A): shared layout switching, list rows, quick
  // --- actions. Selectors observed on the running old app (seeded stack):
  // --- the header switcher is a five-button segmented control in fixed
  // --- order (list, board, calendar, spreadsheet, timeline) with an active
  // --- background marker; list sections hang group headers over anchors
  // --- with id="issue-<uuid>"; the row quick-actions trigger is a hover
  // --- control with an accessible toggle name; the peek panel is the
  // --- absolute right-side panel plus a peekIssueId URL param.

  private static readonly LAYOUTS_ORDER: LayoutsLayoutKey[] = [
    "list",
    "kanban",
    "calendar",
    "spreadsheet",
    "gantt_chart",
  ];

  // First contact with a freshly loaded issues page waits longer than the
  // shared budget: route compile plus the filter/issue fetch chains take
  // 90-180s on the loaded shared host (8GB box, ~25 containers, several
  // dev servers), and header chrome plus rows appear together only after
  // both settle. Sized for failure latency, not pass time: passes resolve
  // as soon as the chrome renders.
  private static readonly LAYOUTS_FIRST_WAIT_MS = 300_000;

  private layoutsSwitcherButtons(): Locator {
    return this.page.locator("div.flex.items-center.gap-1.rounded-md.bg-layer-3.p-1 > button");
  }

  private layoutsIssueRow(issueName: string): Locator {
    return this.page.locator('a[id^="issue-"]', { hasText: issueName }).first();
  }

  private layoutsGroupHeaders(): Locator {
    return this.page.locator('div[class*="group/list-header"]');
  }

  private layoutsPeekPanel(): Locator {
    return this.page.locator("div.absolute.top-0.right-0.bottom-0").first();
  }

  private static layoutsHeaderTitle(headerText: string): string {
    // Headers render "Title <count>"; the count is a trailing bare number.
    return headerText
      .trim()
      .replace(/\s+/g, " ")
      .replace(/\s+\d+$/, "");
  }

  async layoutsOfferedLayouts(): Promise<LayoutsLayoutKey[]> {
    const buttons = this.layoutsSwitcherButtons();
    await buttons.first().waitFor({ timeout: WebDriver.LAYOUTS_FIRST_WAIT_MS });
    const count = await buttons.count();
    if (count !== WebDriver.LAYOUTS_ORDER.length) {
      throw new Error(`[parity] layout switcher offers ${count} layouts, expected ${WebDriver.LAYOUTS_ORDER.length}.`);
    }
    return [...WebDriver.LAYOUTS_ORDER];
  }

  async layoutsActiveLayout(): Promise<LayoutsLayoutKey> {
    const buttons = this.layoutsSwitcherButtons();
    await buttons.first().waitFor({ timeout: WebDriver.LAYOUTS_FIRST_WAIT_MS });
    const count = await buttons.count();
    for (let i = 0; i < count; i++) {
      const cls = (await buttons.nth(i).getAttribute("class")) ?? "";
      if (cls.includes("bg-layer-transparent-active")) {
        const key = WebDriver.LAYOUTS_ORDER[i];
        if (key === undefined) throw new Error(`[parity] switcher has no layout key at index ${i}.`);
        return key;
      }
    }
    throw new Error("[parity] no switcher button carries the active marker.");
  }

  private async layoutsWaitForLayout(layout: LayoutsLayoutKey): Promise<void> {
    // Layout switches refetch and re-render heavy views; the timeline
    // compiles a heavy chart bundle on first load and needs ~150s on the
    // loaded shared host, so every switch gets a long leash.
    const deadline = Date.now() + 300_000;
    for (;;) {
      const visible =
        layout === "list"
          ? await this.layoutsListVisible()
          : layout === "kanban"
            ? await this.layoutsKanbanVisible()
            : layout === "calendar"
              ? await this.layoutsCalendarVisible()
              : layout === "spreadsheet"
                ? await this.layoutsSpreadsheetVisible()
                : await this.layoutsGanttVisible();
      if (visible) return;
      if (Date.now() >= deadline) throw new Error(`[parity] ${layout} layout never rendered after switching.`);
      await this.page.waitForTimeout(500);
    }
  }

  async layoutsSwitchTo(layout: LayoutsLayoutKey): Promise<void> {
    const index = WebDriver.LAYOUTS_ORDER.indexOf(layout);
    const buttons = this.layoutsSwitcherButtons();
    await buttons.nth(index).waitFor({ timeout: WebDriver.LAYOUTS_FIRST_WAIT_MS });
    await buttons.nth(index).scrollIntoViewIfNeeded();
    await buttons.nth(index).click();
    // Clicking the active layout is a specified no-op; the marker is
    // already visible then, so this wait resolves immediately.
    await this.layoutsWaitForLayout(layout);
  }

  async layoutsReloadIssues(): Promise<void> {
    await this.page.reload();
    await this.page.waitForLoadState("domcontentloaded");
    await this.layoutsSwitcherButtons().first().waitFor({ timeout: WebDriver.LAYOUTS_FIRST_WAIT_MS });
  }

  async layoutsListVisible(): Promise<boolean> {
    // Immediate read, no waiting: switch assertions poll through
    // layoutsSwitchTo, and absence must read fast.
    return (await this.layoutsGroupHeaders().count()) > 0;
  }

  async layoutsCalendarVisible(): Promise<boolean> {
    return (await this.page.getByRole("button", { name: "Options" }).count()) > 0;
  }

  async layoutsSpreadsheetVisible(): Promise<boolean> {
    // The sheet's first-column header reads "Work items" (lowercase i);
    // the breadcrumb elsewhere reads "Work Items", so the exact match is
    // unambiguous.
    return (await this.page.getByText("Work items", { exact: true }).count()) > 0;
  }

  async layoutsKanbanVisible(): Promise<boolean> {
    // The board carries no exclusive text on the seed: it renders issue
    // cards but none of the other layouts' markers. A fully collapsed
    // board shows no cards, so the active switcher also counts (the
    // switch scenario separately proves cards render when expanded).
    const otherMarkers =
      (await this.page.getByText("All work items", { exact: true }).count()) +
      (await this.page.getByRole("button", { name: "Options" }).count()) +
      (await this.page.getByText("Work items", { exact: true }).count()) +
      (await this.page.getByText("Quarter", { exact: true }).count());
    if (otherMarkers > 0) return false;
    if ((await this.page.locator('a[id^="issue-"]').count()) > 0) return true;
    return (await this.layoutsActiveLayout()) === "kanban";
  }

  async layoutsGanttVisible(): Promise<boolean> {
    // The zoom control ("Week / Month / Quarter / Today") is a row of
    // role-less divs, so the marker is the Quarter label as plain text.
    return (await this.page.getByText("Quarter", { exact: true }).count()) > 0;
  }

  async layoutsListGroups(): Promise<string[]> {
    const headers = this.layoutsGroupHeaders();
    const count = await headers.count();
    const titles: string[] = [];
    for (let i = 0; i < count; i++) {
      titles.push(WebDriver.layoutsHeaderTitle((await headers.nth(i).innerText()) ?? ""));
    }
    return titles;
  }

  private async layoutsGroupSectionFast(title: string): Promise<Locator | null> {
    const sections = this.page.locator('div[data-drop-target-for-element="true"]');
    const count = await sections.count();
    for (let i = 0; i < count; i++) {
      const header = sections.nth(i).locator('div[class*="group/list-header"]').first();
      if ((await header.count()) === 0) continue;
      if (WebDriver.layoutsHeaderTitle((await header.innerText()) ?? "") === title) return sections.nth(i);
    }
    return null;
  }

  private async layoutsGroupSection(title: string): Promise<Locator> {
    // The list body (sections) renders after the header chrome the page
    // waits settle on, so a fresh open/reload needs a bounded wait here
    // instead of an immediate throw.
    const deadline = Date.now() + 120_000;
    for (;;) {
      const found = await this.layoutsGroupSectionFast(title);
      if (found) return found;
      if (Date.now() >= deadline) throw new Error(`[parity] list group "${title}" not found.`);
      await this.page.waitForTimeout(500);
    }
  }

  async layoutsListGroupExpanded(title: string): Promise<boolean> {
    // A collapsed section hides its rows and its quick-add; the sticky
    // quick-add is present exactly when expanded (member+ view).
    const section = await this.layoutsGroupSection(title);
    return (await section.locator("div.sticky.bottom-0").count()) > 0;
  }

  async layoutsListToggleGroup(title: string): Promise<void> {
    const section = await this.layoutsGroupSection(title);
    const before = await this.layoutsListGroupExpanded(title);
    await section.locator('div[class*="group/list-header"]').first().click();
    const deadline = Date.now() + 15_000;
    for (;;) {
      if ((await this.layoutsListGroupExpanded(title)) !== before) return;
      if (Date.now() >= deadline) throw new Error(`[parity] list group "${title}" never toggled.`);
      await this.page.waitForTimeout(300);
    }
  }

  async layoutsListGroupIssueNames(title: string): Promise<string[]> {
    const section = await this.layoutsGroupSection(title);
    const rows = section.locator('a[id^="issue-"]');
    const count = await rows.count();
    const names: string[] = [];
    for (let i = 0; i < count; i++) {
      const text = await rows.nth(i).locator("p").first().innerText();
      names.push(text.trim());
    }
    return names;
  }

  async layoutsListGroupHasLoadMore(title: string): Promise<boolean> {
    // Absence-tolerant: a missing group reads as no row (callers polling
    // for true still converge; callers asserting false pair it with a
    // positive read so a slow load cannot pass vacuously).
    const section = await this.layoutsGroupSectionFast(title);
    if (!section) return false;
    return (await section.getByText("Load more").count()) > 0;
  }

  async layoutsListGroupLoadMore(title: string): Promise<void> {
    const section = await this.layoutsGroupSection(title);
    const before = await this.layoutsListGroupIssueNames(title);
    await section.getByText("Load more").first().click();
    const deadline = Date.now() + 30_000;
    for (;;) {
      const after = await this.layoutsListGroupIssueNames(title);
      if (after.length > before.length || !(await this.layoutsListGroupHasLoadMore(title))) return;
      if (Date.now() >= deadline) throw new Error(`[parity] group "${title}" never loaded more.`);
      await this.page.waitForTimeout(500);
    }
  }

  async layoutsListScrollEnd(): Promise<void> {
    const rows = this.page.locator('a[id^="issue-"]');
    const count = await rows.count();
    if (count === 0) throw new Error("[parity] no list rows to scroll to.");
    await rows.nth(count - 1).scrollIntoViewIfNeeded();
  }

  async layoutsListQuickAdd(title: string, groupTitle?: string): Promise<void> {
    const scope = groupTitle === undefined ? this.page : await this.layoutsGroupSection(groupTitle);
    const trigger = scope.locator("div.sticky.bottom-0", { hasText: "New work item" }).first();
    await trigger.scrollIntoViewIfNeeded();
    await trigger.click();
    const field = this.page.getByPlaceholder("Work item title");
    await field.waitFor({ timeout: WebDriver.LAYOUTS_FIRST_WAIT_MS });
    await field.fill(title);
    await field.press("Enter");
    // The row appearing proves the save landed; the title is unique per
    // scenario run, so this cannot match a stale row.
    await this.page.locator('a[id^="issue-"]', { hasText: title }).first().waitFor({ timeout: 120_000 });
  }

  async layoutsRowCanEditState(issueName: string): Promise<boolean> {
    // Guests and other read-only viewers render the chip without the
    // dropdown. The dropdown carries its own search field, but the page
    // already holds one, so editability reads as the field count growing
    // after the click rather than as mere presence.
    const search = this.page.getByPlaceholder("Search", { exact: true });
    const before = await search.count();
    const chip = this.layoutsRowStateButton(issueName);
    await chip.scrollIntoViewIfNeeded();
    await chip.click();
    const deadline = Date.now() + 5_000;
    let opened = false;
    for (;;) {
      if ((await search.count()) > before) {
        opened = true;
        break;
      }
      if (Date.now() >= deadline) break;
      await this.page.waitForTimeout(300);
    }
    await this.page.keyboard.press("Escape");
    return opened;
  }

  async layoutsRowHref(issueName: string): Promise<string | null> {
    const row = this.layoutsIssueRow(issueName);
    await row.waitFor({ timeout: 120_000 });
    return row.getAttribute("href");
  }

  async layoutsRowOpenPeek(issueName: string): Promise<void> {
    const row = this.layoutsIssueRow(issueName);
    await row.locator("p").first().click();
    await this.page.waitForURL((url) => url.href.includes("peekIssueId"), { timeout: 120_000 });
    await this.layoutsPeekPanel().waitFor({ timeout: 120_000 });
  }

  async layoutsPeekVisible(): Promise<boolean> {
    if (!this.page.url().includes("peekIssueId")) return false;
    const panel = this.layoutsPeekPanel();
    return (await panel.count()) > 0 && (await panel.isVisible());
  }

  async layoutsPeekTitle(): Promise<string | null> {
    // Seed issues carry no description, so the panel reads as the
    // identifier line (PAR-1), the title line, then the description
    // placeholder: the title is the line right after the identifier.
    const panel = this.layoutsPeekPanel();
    if ((await panel.count()) === 0) return null;
    const lines = ((await panel.innerText()) ?? "")
      .split("\n")
      .map((line) => line.trim())
      .filter((line) => line.length > 0);
    const at = lines.findIndex((line) => /^[A-Z]+-\d+$/.test(line));
    if (at < 0 || at + 1 >= lines.length) return null;
    return lines[at + 1] ?? null;
  }

  async layoutsPeekClose(): Promise<void> {
    await this.page.keyboard.press("Escape");
    const deadline = Date.now() + 15_000;
    for (;;) {
      if (!this.page.url().includes("peekIssueId")) return;
      if (Date.now() >= deadline) throw new Error("[parity] peek panel never closed.");
      await this.page.waitForTimeout(300);
    }
  }

  async layoutsRowHasSubIssueToggle(issueName: string): Promise<boolean> {
    // The leading cell is an empty grid slot without children and carries
    // the expander button once sub-issues exist.
    const row = this.layoutsIssueRow(issueName);
    await row.waitFor({ timeout: 120_000 });
    const slot = row.locator("div.grid.size-4").first();
    if ((await slot.count()) === 0) return false;
    return (await slot.locator("button").count()) > 0;
  }

  async layoutsRowExpandSubIssues(issueName: string): Promise<void> {
    const row = this.layoutsIssueRow(issueName);
    const toggle = row.locator("div.grid.size-4 button").first();
    await toggle.waitFor({ timeout: WebDriver.LAYOUTS_FIRST_WAIT_MS });
    await toggle.click();
    const deadline = Date.now() + 30_000;
    for (;;) {
      if ((await this.layoutsRowSubIssueNames(issueName)).length > 0) return;
      if (Date.now() >= deadline) throw new Error(`[parity] sub-issues of "${issueName}" never rendered.`);
      await this.page.waitForTimeout(500);
    }
  }

  async layoutsRowSubIssueNames(issueName: string): Promise<string[]> {
    // Expanded children render as nested rows inside the parent's block,
    // after the parent's own link.
    const row = this.layoutsIssueRow(issueName);
    await row.waitFor({ timeout: 120_000 });
    const block = row.locator("xpath=..");
    const nested = block.locator('a[id^="issue-"]');
    const count = await nested.count();
    const names: string[] = [];
    for (let i = 1; i < count; i++) {
      names.push(((await nested.nth(i).locator("p").first().innerText()) ?? "").trim());
    }
    return names;
  }

  private layoutsRowStateButton(issueName: string): Locator {
    // The state chip is the row's span-carrying button (the identifier is
    // a bare button, the icon controls carry no span).
    return this.layoutsIssueRow(issueName).locator("button:has(span)").first();
  }

  async layoutsRowState(issueName: string): Promise<string> {
    const chip = this.layoutsRowStateButton(issueName);
    await chip.waitFor({ timeout: WebDriver.LAYOUTS_FIRST_WAIT_MS });
    return ((await chip.innerText()) ?? "").trim();
  }

  async layoutsRowSetState(issueName: string, stateName: string): Promise<void> {
    const chip = this.layoutsRowStateButton(issueName);
    await chip.scrollIntoViewIfNeeded();
    await chip.click();
    // The option portal renders at the end of the document, after the
    // row chips with the same text, so the last match is the option.
    const option = this.page.getByRole("button", { name: stateName, exact: true }).last();
    await option.waitFor({ timeout: WebDriver.LAYOUTS_FIRST_WAIT_MS });
    await option.click();
    const deadline = Date.now() + 30_000;
    for (;;) {
      if ((await this.layoutsRowState(issueName)) === stateName) return;
      if (Date.now() >= deadline) throw new Error(`[parity] row "${issueName}" never showed state "${stateName}".`);
      await this.page.waitForTimeout(500);
    }
  }

  private async layoutsRowPriorityControl(issueName: string): Promise<Locator> {
    // Priority is the first visible icon-only control in the strip: the
    // identifier is disabled, the state chip carries a span, the menu
    // triggers carry the toggle name, and the mobile trigger is hidden on
    // desktop, so what remains first is the priority control.
    const row = this.layoutsIssueRow(issueName);
    await row.waitFor({ timeout: 120_000 });
    const candidates = row.locator("button:not([disabled])").filter({ hasNot: row.locator("span") });
    const count = await candidates.count();
    for (let i = 0; i < count; i++) {
      const candidate = candidates.nth(i);
      if ((await candidate.getAttribute("aria-label")) === "Toggle quick actions menu") continue;
      if (!(await candidate.isVisible())) continue;
      return candidate;
    }
    throw new Error(`[parity] no priority control found on row "${issueName}".`);
  }

  async layoutsRowPriority(issueName: string): Promise<string> {
    const control = await this.layoutsRowPriorityControl(issueName);
    await control.waitFor({ timeout: WebDriver.LAYOUTS_FIRST_WAIT_MS });
    const text = ((await control.innerText()) ?? "").trim();
    return text === "" ? "None" : text;
  }

  async layoutsRowSetPriority(issueName: string, priorityName: string): Promise<void> {
    const control = await this.layoutsRowPriorityControl(issueName);
    await control.scrollIntoViewIfNeeded();
    await control.click();
    const option = this.page.getByRole("button", { name: priorityName, exact: true }).last();
    await option.waitFor({ timeout: WebDriver.LAYOUTS_FIRST_WAIT_MS });
    await option.click();
    const deadline = Date.now() + 30_000;
    for (;;) {
      const current = await this.layoutsRowPriority(issueName);
      if (current === priorityName || (priorityName === "None" && current === "None")) return;
      if (Date.now() >= deadline) {
        throw new Error(`[parity] row "${issueName}" never showed priority "${priorityName}".`);
      }
      await this.page.waitForTimeout(500);
    }
  }

  private async layoutsOpenRowMenu(issueName: string): Promise<void> {
    const row = this.layoutsIssueRow(issueName);
    const trigger = row.getByRole("button", { name: "Toggle quick actions menu" }).first();
    await trigger.waitFor({ timeout: WebDriver.LAYOUTS_FIRST_WAIT_MS });
    // The trigger is hover-revealed and sits under the property strip for
    // automation clicks, so hover it into its clickable state first; a
    // keyboard activation covers the case where the strip still overlaps.
    await trigger.hover();
    await trigger.click({ timeout: 10_000 }).catch(async () => {
      await trigger.focus();
      await this.page.keyboard.press("Enter");
    });
    await this.page.getByRole("menuitem").first().waitFor({ timeout: 15_000 });
  }

  private async layoutsReadOpenMenuItems(): Promise<string[]> {
    const items = this.page.getByRole("menuitem");
    const count = await items.count();
    const texts: string[] = [];
    for (let i = 0; i < count; i++) {
      texts.push(((await items.nth(i).innerText()) ?? "").trim().replace(/\s+/g, " "));
    }
    return texts;
  }

  async layoutsRowMenuItems(issueName: string): Promise<string[]> {
    await this.layoutsOpenRowMenu(issueName);
    try {
      return await this.layoutsReadOpenMenuItems();
    } finally {
      await this.page.keyboard.press("Escape");
    }
  }

  async layoutsRowMenuChoose(issueName: string, item: string): Promise<void> {
    await this.layoutsOpenRowMenu(issueName);
    await this.page.getByRole("menuitem", { name: item, exact: true }).first().click();
    const deadline = Date.now() + 15_000;
    for (;;) {
      if ((await this.page.getByRole("menuitem").count()) === 0) return;
      if (Date.now() >= deadline) throw new Error(`[parity] row menu never closed after choosing "${item}".`);
      await this.page.waitForTimeout(300);
    }
  }

  async layoutsRowContextMenuItems(issueName: string): Promise<string[]> {
    const row = this.layoutsIssueRow(issueName);
    await row.locator("p").first().click({ button: "right" });
    await this.page.getByRole("menuitem").first().waitFor({ timeout: 15_000 });
    try {
      return await this.layoutsReadOpenMenuItems();
    } finally {
      await this.page.keyboard.press("Escape");
    }
  }

  // --- NEWFRONT-117 round 2 (spreadsheet, calendar, row actions, empty
  // --- states, mobile/loaders): landing as throwing stubs first so the
  // --- extended interface compiles; each wave replaces its stubs with
  // --- observed-app implementations.
  private layoutsTodo(target: string): never {
    throw new Error(`[parity] layouts driver ${target} not implemented yet.`);
  }

  async layoutsSheetHeaders(): Promise<string[]> {
    return this.layoutsTodo("layoutsSheetHeaders");
  }

  async layoutsSheetRowNames(): Promise<string[]> {
    return this.layoutsTodo("layoutsSheetRowNames");
  }

  async layoutsSheetFirstColumnSticky(): Promise<boolean> {
    return this.layoutsTodo("layoutsSheetFirstColumnSticky");
  }

  async layoutsSheetFirstColumnShadowed(): Promise<boolean> {
    return this.layoutsTodo("layoutsSheetFirstColumnShadowed");
  }

  async layoutsSheetScrollRight(): Promise<void> {
    return this.layoutsTodo("layoutsSheetScrollRight");
  }

  async layoutsSheetHeaderSticky(): Promise<boolean> {
    return this.layoutsTodo("layoutsSheetHeaderSticky");
  }

  async layoutsSheetCellText(_issueName: string, _column: string): Promise<string> {
    return this.layoutsTodo("layoutsSheetCellText");
  }

  async layoutsSheetCellEditable(_issueName: string, _column: string): Promise<boolean> {
    return this.layoutsTodo("layoutsSheetCellEditable");
  }

  async layoutsSheetCellSetState(_issueName: string, _stateName: string): Promise<void> {
    return this.layoutsTodo("layoutsSheetCellSetState");
  }

  async layoutsSheetCellSetPriority(_issueName: string, _priorityName: string): Promise<void> {
    return this.layoutsTodo("layoutsSheetCellSetPriority");
  }

  async layoutsSheetFocusCell(_issueName: string, _column: string): Promise<void> {
    return this.layoutsTodo("layoutsSheetFocusCell");
  }

  async layoutsSheetPressArrow(_arrow: "up" | "down" | "left" | "right"): Promise<void> {
    return this.layoutsTodo("layoutsSheetPressArrow");
  }

  async layoutsSheetFocusedCell(): Promise<{ issueName: string; column: string } | null> {
    return this.layoutsTodo("layoutsSheetFocusedCell");
  }

  async layoutsSheetSortMenu(_column: string): Promise<string[]> {
    return this.layoutsTodo("layoutsSheetSortMenu");
  }

  async layoutsSheetSort(_column: string, _direction: "ascending" | "descending"): Promise<void> {
    return this.layoutsTodo("layoutsSheetSort");
  }

  async layoutsSheetClearSort(_column: string): Promise<void> {
    return this.layoutsTodo("layoutsSheetClearSort");
  }

  async layoutsSheetSortMarker(_column: string): Promise<"ascending" | "descending" | "none"> {
    return this.layoutsTodo("layoutsSheetSortMarker");
  }

  async layoutsSheetQuickAdd(_title: string): Promise<void> {
    return this.layoutsTodo("layoutsSheetQuickAdd");
  }

  async layoutsSheetScrollEnd(): Promise<void> {
    return this.layoutsTodo("layoutsSheetScrollEnd");
  }

  async layoutsSheetHasSubIssueToggle(_issueName: string): Promise<boolean> {
    return this.layoutsTodo("layoutsSheetHasSubIssueToggle");
  }

  async layoutsSheetExpandSubIssues(_issueName: string): Promise<void> {
    return this.layoutsTodo("layoutsSheetExpandSubIssues");
  }

  async layoutsSheetSubIssueNames(_issueName: string): Promise<string[]> {
    return this.layoutsTodo("layoutsSheetSubIssueNames");
  }

  async layoutsSheetOpenSubIssueCount(_issueName: string): Promise<void> {
    return this.layoutsTodo("layoutsSheetOpenSubIssueCount");
  }

  async layoutsCalMode(): Promise<"month" | "week"> {
    return this.layoutsTodo("layoutsCalMode");
  }

  async layoutsCalTitle(): Promise<string> {
    return this.layoutsTodo("layoutsCalTitle");
  }

  async layoutsCalPrev(): Promise<void> {
    return this.layoutsTodo("layoutsCalPrev");
  }

  async layoutsCalNext(): Promise<void> {
    return this.layoutsTodo("layoutsCalNext");
  }

  async layoutsCalToday(): Promise<void> {
    return this.layoutsTodo("layoutsCalToday");
  }

  async layoutsCalMonthPickerMonths(): Promise<string[]> {
    return this.layoutsTodo("layoutsCalMonthPickerMonths");
  }

  async layoutsCalMonthPickerYear(): Promise<number> {
    return this.layoutsTodo("layoutsCalMonthPickerYear");
  }

  async layoutsCalMonthPickerYearStep(_direction: "prev" | "next"): Promise<void> {
    return this.layoutsTodo("layoutsCalMonthPickerYearStep");
  }

  async layoutsCalMonthPickerChoose(_month: string): Promise<void> {
    return this.layoutsTodo("layoutsCalMonthPickerChoose");
  }

  async layoutsCalMonthPickerEnabled(): Promise<boolean> {
    return this.layoutsTodo("layoutsCalMonthPickerEnabled");
  }

  async layoutsCalSetMode(_mode: "month" | "week"): Promise<void> {
    return this.layoutsTodo("layoutsCalSetMode");
  }

  async layoutsCalWeekendsVisible(): Promise<boolean> {
    return this.layoutsTodo("layoutsCalWeekendsVisible");
  }

  async layoutsCalSetWeekends(_show: boolean): Promise<void> {
    return this.layoutsTodo("layoutsCalSetWeekends");
  }

  async layoutsCalColumnCount(): Promise<number> {
    return this.layoutsTodo("layoutsCalColumnCount");
  }

  async layoutsCalDayIssueNames(_dayNumber: number): Promise<string[]> {
    return this.layoutsTodo("layoutsCalDayIssueNames");
  }

  async layoutsCalDayIsToday(_dayNumber: number): Promise<boolean> {
    return this.layoutsTodo("layoutsCalDayIsToday");
  }

  async layoutsCalDayHasLoadMore(_dayNumber: number): Promise<boolean> {
    return this.layoutsTodo("layoutsCalDayHasLoadMore");
  }

  async layoutsCalDayLoadMore(_dayNumber: number): Promise<void> {
    return this.layoutsTodo("layoutsCalDayLoadMore");
  }

  async layoutsCalDragBlock(_issueName: string, _toDayNumber: number): Promise<void> {
    return this.layoutsTodo("layoutsCalDragBlock");
  }

  async layoutsCalBlockText(_issueName: string): Promise<string> {
    return this.layoutsTodo("layoutsCalBlockText");
  }

  async layoutsCalBlockHoverPreview(_issueName: string): Promise<boolean> {
    return this.layoutsTodo("layoutsCalBlockHoverPreview");
  }

  async layoutsCalBlockOpenPeek(_issueName: string): Promise<void> {
    return this.layoutsTodo("layoutsCalBlockOpenPeek");
  }

  async layoutsCalBlockQuickActions(_issueName: string): Promise<string[]> {
    return this.layoutsTodo("layoutsCalBlockQuickActions");
  }

  async layoutsCalDayQuickAdd(_dayNumber: number, _title: string): Promise<void> {
    return this.layoutsTodo("layoutsCalDayQuickAdd");
  }

  async layoutsCalDayAddMenu(_dayNumber: number): Promise<string[]> {
    return this.layoutsTodo("layoutsCalDayAddMenu");
  }

  async layoutsCalTapDay(_dayNumber: number): Promise<void> {
    return this.layoutsTodo("layoutsCalTapDay");
  }

  async layoutsCalDayDetailNames(): Promise<string[]> {
    return this.layoutsTodo("layoutsCalDayDetailNames");
  }

  async layoutsRowMenuItemDisabled(_issueName: string, _item: string): Promise<boolean> {
    return this.layoutsTodo("layoutsRowMenuItemDisabled");
  }

  async layoutsRowMenuItemNote(_issueName: string, _item: string): Promise<string | null> {
    return this.layoutsTodo("layoutsRowMenuItemNote");
  }

  async layoutsWorkItemModalVisible(): Promise<boolean> {
    return this.layoutsTodo("layoutsWorkItemModalVisible");
  }

  async layoutsWorkItemModalTitle(): Promise<string | null> {
    return this.layoutsTodo("layoutsWorkItemModalTitle");
  }

  async layoutsWorkItemModalClose(): Promise<void> {
    return this.layoutsTodo("layoutsWorkItemModalClose");
  }

  async layoutsDeleteModalVisible(): Promise<boolean> {
    return this.layoutsTodo("layoutsDeleteModalVisible");
  }

  async layoutsDeleteModalConfirm(): Promise<void> {
    return this.layoutsTodo("layoutsDeleteModalConfirm");
  }

  async layoutsArchiveModalVisible(): Promise<boolean> {
    return this.layoutsTodo("layoutsArchiveModalVisible");
  }

  async layoutsArchiveModalConfirm(): Promise<void> {
    return this.layoutsTodo("layoutsArchiveModalConfirm");
  }

  async layoutsMoveModalVisible(): Promise<boolean> {
    return this.layoutsTodo("layoutsMoveModalVisible");
  }

  async layoutsMoveModalChoose(_projectName: string): Promise<void> {
    return this.layoutsTodo("layoutsMoveModalChoose");
  }

  async layoutsAddExistingModalVisible(): Promise<boolean> {
    return this.layoutsTodo("layoutsAddExistingModalVisible");
  }

  async layoutsAddExistingModalChoose(_issueName: string): Promise<void> {
    return this.layoutsTodo("layoutsAddExistingModalChoose");
  }

  async layoutsDetailMenuItems(): Promise<string[]> {
    return this.layoutsTodo("layoutsDetailMenuItems");
  }

  async layoutsDetailMenuChoose(_item: string): Promise<void> {
    return this.layoutsTodo("layoutsDetailMenuChoose");
  }

  async layoutsPeekCopyLinkVisible(): Promise<boolean> {
    return this.layoutsTodo("layoutsPeekCopyLinkVisible");
  }

  async layoutsListPageMenuItems(): Promise<string[]> {
    return this.layoutsTodo("layoutsListPageMenuItems");
  }

  async layoutsGroupHeaderAddMenu(_groupTitle: string): Promise<string[] | null> {
    return this.layoutsTodo("layoutsGroupHeaderAddMenu");
  }

  async layoutsEmptyTitle(): Promise<string | null> {
    return this.layoutsTodo("layoutsEmptyTitle");
  }

  async layoutsEmptyActions(): Promise<Array<{ label: string; disabled: boolean }>> {
    return this.layoutsTodo("layoutsEmptyActions");
  }

  async layoutsEmptyChoose(_label: string): Promise<void> {
    return this.layoutsTodo("layoutsEmptyChoose");
  }

  async layoutsMobileOfferedLayouts(): Promise<LayoutsLayoutKey[]> {
    return this.layoutsTodo("layoutsMobileOfferedLayouts");
  }

  async layoutsMobileDisplayVisible(): Promise<boolean> {
    return this.layoutsTodo("layoutsMobileDisplayVisible");
  }

  async layoutsMobileAnalyticsVisible(): Promise<boolean> {
    return this.layoutsTodo("layoutsMobileAnalyticsVisible");
  }

  async layoutsSkeletonVisible(): Promise<boolean> {
    return this.layoutsTodo("layoutsSkeletonVisible");
  }

  async layoutsMutationSpinnerVisible(): Promise<boolean> {
    return this.layoutsTodo("layoutsMutationSpinnerVisible");
  }

  async layoutsRowHighlighted(_issueName: string): Promise<boolean> {
    return this.layoutsTodo("layoutsRowHighlighted");
  }

  async layoutsTempRowVisible(): Promise<boolean> {
    return this.layoutsTodo("layoutsTempRowVisible");
  }

  async layoutsStallIssuesGet(_delayMs: number): Promise<void> {
    return this.layoutsTodo("layoutsStallIssuesGet");
  }

  async layoutsStallIssueMutation(_delayMs: number): Promise<void> {
    return this.layoutsTodo("layoutsStallIssueMutation");
  }

  async layoutsReleaseStalls(): Promise<void> {
    return this.layoutsTodo("layoutsReleaseStalls");
  }

  async layoutsSheetCellSetDueDate(_issueName: string, _isoDate: string): Promise<void> {
    return this.layoutsTodo("layoutsSheetCellSetDueDate");
  }

  async layoutsCurrentUrl(): Promise<string> {
    return this.layoutsTodo("layoutsCurrentUrl");
  }

  async layoutsSheetCellSetAssignee(_issueName: string, _memberName: string): Promise<void> {
    return this.layoutsTodo("layoutsSheetCellSetAssignee");
  }

  async layoutsCalDayAddExisting(_dayNumber: number): Promise<void> {
    return this.layoutsTodo("layoutsCalDayAddExisting");
  }

  async layoutsAddExistingModalIssueNames(): Promise<string[]> {
    return this.layoutsTodo("layoutsAddExistingModalIssueNames");
  }

  async layoutsMobileSwitchTo(_layout: LayoutsLayoutKey): Promise<void> {
    return this.layoutsTodo("layoutsMobileSwitchTo");
  }

  async layoutsMobileDisplayCycleModuleDisabled(): Promise<{ cycleDisabled: boolean; moduleDisabled: boolean }> {
    return this.layoutsTodo("layoutsMobileDisplayCycleModuleDisabled");
  }

  async layoutsRowMenuOpenNewTabUrl(_issueName: string): Promise<string> {
    return this.layoutsTodo("layoutsRowMenuOpenNewTabUrl");
  }

  async layoutsWorkItemModalHasText(_text: string): Promise<boolean> {
    return this.layoutsTodo("layoutsWorkItemModalHasText");
  }

  async layoutsListPageMenuChoose(_item: string): Promise<void> {
    return this.layoutsTodo("layoutsListPageMenuChoose");
  }

  async layoutsGroupHeaderAddChoose(_groupTitle: string, _item: string | null): Promise<void> {
    return this.layoutsTodo("layoutsGroupHeaderAddChoose");
  }
}
