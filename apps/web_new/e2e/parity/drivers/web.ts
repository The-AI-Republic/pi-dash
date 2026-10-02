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
  BoardLayoutKey,
  GanttSidebarRow,
  GanttZoom,
  KanbanCard,
  KanbanColumn,
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

  // --- NEWFRONT-118 (layouts B): kanban board + gantt timeline.
  // --- Appended; existing methods above are untouched per the shared
  // --- driver contract. Selectors below address the old app's rendered
  // --- DOM as observed during oracle recon; they carry no old-app code.

  private static readonly BOARD_ORDER: BoardLayoutKey[] = ["list", "kanban", "calendar", "spreadsheet", "gantt"];
  // Generous cold ceilings: the oracle dev server compiles routes on first
  // load, which takes minutes on a loaded host. CI binds tighter through
  // the per-test timeout, so these only extend the local ceiling.
  private static readonly BOARD_FIRST_WAIT_MS = 240_000;
  private static readonly BOARD_SETTLE_WAIT_MS = 240_000;
  private static readonly BOARD_POLL_STEP_MS = 2_000;

  private boardSwitcherButtons(): Locator {
    // The header layout switcher is a row of icon-only buttons with no
    // accessible names; position is the only stable address.
    return this.page.locator("div.flex.items-center.gap-1.rounded-md.bg-layer-3.p-1 > button");
  }

  private boardMain(): Locator {
    return this.page.getByRole("main");
  }

  private async boardWaitForLayout(layout: BoardLayoutKey): Promise<void> {
    const buttons = this.boardSwitcherButtons();
    await buttons.first().waitFor({ timeout: WebDriver.BOARD_FIRST_WAIT_MS });
    const index = WebDriver.BOARD_ORDER.indexOf(layout);
    // Re-click while the layout is wrong: a click that lands mid-hydration
    // can be swallowed, leaving the previous layout active.
    const deadline = Date.now() + WebDriver.BOARD_SETTLE_WAIT_MS;
    for (;;) {
      if ((await this.boardActiveLayout()) === layout) {
        if (layout === "kanban" && (await this.kanbanBoardVisible())) return;
        if (layout === "gantt" && (await this.ganttTimelineVisible())) return;
        if (layout !== "kanban" && layout !== "gantt") return;
      }
      if (Date.now() > deadline) throw new Error(`[parity] timed out waiting for the ${layout} layout to render.`);
      await buttons
        .nth(index)
        .click({ timeout: WebDriver.BOARD_FIRST_WAIT_MS })
        .catch(() => undefined);
      await this.page.waitForTimeout(WebDriver.BOARD_POLL_STEP_MS);
    }
  }

  async kanbanOpenBoard(): Promise<void> {
    await this.boardWaitForLayout("kanban");
  }

  async kanbanBoardVisible(): Promise<boolean> {
    // Kanban column bodies carry ids of the shape {group}__{subgroup};
    // no other layout renders such ids.
    const columns = this.boardMain().locator('div[id*="__"]');
    if ((await columns.count()) === 0) return false;
    return (await this.page.locator("#gantt-container").count()) === 0;
  }

  async ganttOpenTimeline(): Promise<void> {
    await this.boardWaitForLayout("gantt");
  }

  async ganttTimelineVisible(): Promise<boolean> {
    return (await this.page.locator("#gantt-container").count()) > 0;
  }

  async boardActiveLayout(): Promise<BoardLayoutKey> {
    const buttons = this.boardSwitcherButtons();
    const count = await buttons.count();
    for (let index = 0; index < count; index += 1) {
      const cls =
        (await buttons
          .nth(index)
          .getAttribute("class")
          .catch(() => null)) ?? "";
      if (cls.includes("bg-layer-transparent-active")) return WebDriver.BOARD_ORDER[index] ?? "list";
    }
    return "list";
  }

  async boardReloadIssues(): Promise<void> {
    await this.page.reload();
    await this.boardSwitcherButtons().first().waitFor({ timeout: WebDriver.BOARD_FIRST_WAIT_MS });
  }

  private static splitHeaderCount(text: string): { name: string; count: number } {
    const clean = text.trim().replace(/\s+/g, " ");
    const match = /^(.*)\s+(\d+)$/.exec(clean);
    if (!match || match[1] === undefined || match[1].length === 0) {
      throw new Error(`[parity] header text carried no trailing count: ${JSON.stringify(clean)}.`);
    }
    return { name: match[1], count: Number(match[2]) };
  }

  private boardFlatColumnOuters(): Locator {
    // Outer column shells in flat (non-swimlane) mode, in display order.
    // Each shell holds a sticky header plus, when the column body is
    // rendered, the {group}__null inner drop target.
    return this.boardMain().locator("div.group.relative.flex.flex-shrink-0.flex-col");
  }

  private async boardIsSwimlane(): Promise<boolean> {
    return (await this.boardMain().locator('div[class*="top-[50px]"]').count()) > 0;
  }

  async kanbanColumns(): Promise<KanbanColumn[]> {
    if (await this.boardIsSwimlane()) return this.kanbanSwimlaneGroupColumns();
    const outers = this.boardFlatColumnOuters();
    const count = await outers.count();
    const columns: KanbanColumn[] = [];
    for (let index = 0; index < count; index += 1) {
      const outer = outers.nth(index);
      const header = outer.locator(":scope > div.sticky").first();
      if ((await header.count()) === 0) continue;
      const { name, count: issues } = WebDriver.splitHeaderCount(await header.innerText());
      const inner = outer.locator(':scope div[id$="__null"]').first();
      const rendered = (await inner.count()) > 0;
      const id = rendered ? (((await inner.getAttribute("id")) ?? "").replace(/__null$/, "") ?? "") : "";
      columns.push({ id, name, count: issues, rendered });
    }
    return columns;
  }

  private async kanbanSwimlaneGroupColumns(): Promise<KanbanColumn[]> {
    // Swimlane mode renders the group headers in a top row (one container
    // holding one cell per group); their value ids come positionally from
    // the first lane's column drop targets, which share the same group
    // order. Trailing columns mount lazily, so cells past the mounted
    // prefix carry no id yet.
    const headerRow = this.boardMain().locator('div.sticky.top-0[class*="z-[4]"]').first();
    await headerRow.waitFor({ timeout: WebDriver.WAIT_MS });
    let headers = headerRow.locator(":scope > div > div");
    if ((await headers.count()) === 0) headers = headerRow.locator(":scope > div");
    const headerCount = await headers.count();
    const firstLane = await this.kanbanLaneWrapper(0);
    const inners = firstLane.locator('div[id*="__"]');
    const innerCount = await inners.count();
    const columns: KanbanColumn[] = [];
    for (let index = 0; index < headerCount; index += 1) {
      const { name, count } = WebDriver.splitHeaderCount(await headers.nth(index).innerText());
      const mounted = index < innerCount;
      const id = mounted ? (((await inners.nth(index).getAttribute("id")) ?? "").split("__")[0] ?? "") : "";
      columns.push({ id, name, count, rendered: mounted });
    }
    return columns;
  }

  private kanbanLaneBars(): Locator {
    return this.boardMain().locator('div[class*="top-[50px]"]');
  }

  private async kanbanLaneWrapper(laneIndex: number): Promise<Locator> {
    return this.kanbanLaneBars().nth(laneIndex).locator("xpath=..");
  }

  private async kanbanLaneWrapperByName(laneName: string): Promise<Locator> {
    const bars = this.kanbanLaneBars();
    const count = await bars.count();
    for (let index = 0; index < count; index += 1) {
      const { name } = WebDriver.splitHeaderCount(await bars.nth(index).innerText());
      if (name === laneName) return bars.nth(index).locator("xpath=..");
    }
    throw new Error(`[parity] no swimlane named ${JSON.stringify(laneName)}.`);
  }

  async kanbanSwimlanes(): Promise<KanbanColumn[]> {
    const bars = this.kanbanLaneBars();
    const count = await bars.count();
    const lanes: KanbanColumn[] = [];
    for (let index = 0; index < count; index += 1) {
      const { name, count: issues } = WebDriver.splitHeaderCount(await bars.nth(index).innerText());
      const wrapper = bars.nth(index).locator("xpath=..");
      const inner = wrapper.locator('div[id*="__"]').first();
      const rendered = (await inner.count()) > 0;
      const id = rendered ? (((await inner.getAttribute("id")) ?? "").split("__")[1] ?? "") : "";
      lanes.push({ id, name, count: issues, rendered });
    }
    return lanes;
  }

  private boardCardLinks(): Locator {
    // Kanban cards link with ids of the shape issue_{id}_{group}_{sub};
    // the underscore prefix distinguishes them from list/gantt rows.
    return this.boardMain().locator('a[id^="issue_"]');
  }

  private static splitCardId(cardId: string): { issueId: string; groupId: string; subGroupId: string } {
    const parts = cardId.split("_");
    if (parts.length < 4 || parts[0] !== "issue") {
      throw new Error(`[parity] unexpected kanban card id ${JSON.stringify(cardId)}.`);
    }
    return { issueId: parts[1] ?? "", groupId: parts[2] ?? "", subGroupId: parts.slice(3).join("_") };
  }

  private boardCardName(card: Locator): Locator {
    return card.locator("div.text-body-sm-medium > span").first();
  }

  private async boardCardByName(issueName: string): Promise<Locator> {
    const cards = this.boardCardLinks();
    const count = await cards.count();
    for (let index = 0; index < count; index += 1) {
      const card = cards.nth(index);
      if (
        (await this.boardCardName(card)
          .innerText()
          .catch(() => null)) === issueName
      )
        return card;
    }
    throw new Error(`[parity] no kanban card titled ${JSON.stringify(issueName)}.`);
  }

  async kanbanCards(): Promise<KanbanCard[]> {
    const cards = this.boardCardLinks();
    const count = await cards.count();
    const out: KanbanCard[] = [];
    for (let index = 0; index < count; index += 1) {
      const card = cards.nth(index);
      const id = (await card.getAttribute("id")) ?? "";
      const { issueId, groupId, subGroupId } = WebDriver.splitCardId(id);
      out.push({ issueId, name: await this.boardCardName(card).innerText(), groupId, subGroupId });
    }
    return out;
  }

  private async boardFlatColumnOuterByName(columnName: string): Promise<Locator> {
    const outers = this.boardFlatColumnOuters();
    const count = await outers.count();
    for (let index = 0; index < count; index += 1) {
      const outer = outers.nth(index);
      const header = outer.locator(":scope > div.sticky").first();
      if ((await header.count()) === 0) continue;
      const { name } = WebDriver.splitHeaderCount(await header.innerText());
      if (name === columnName) return outer;
    }
    throw new Error(`[parity] no kanban column named ${JSON.stringify(columnName)}.`);
  }

  async kanbanColumnCards(columnName: string): Promise<string[]> {
    const outer = await this.boardFlatColumnOuterByName(columnName);
    const cards = outer.locator('a[id^="issue_"]');
    const count = await cards.count();
    const names: string[] = [];
    for (let index = 0; index < count; index += 1) {
      names.push(await this.boardCardName(cards.nth(index)).innerText());
    }
    return names;
  }

  private boardHeaderButtons(outer: Locator): Locator {
    // Flat header entries in order: the collapse toggle, then the create
    // (+) entry when offered.
    return outer.locator(":scope > div.sticky button");
  }

  async kanbanToggleColumn(columnName: string): Promise<void> {
    const outer = await this.boardFlatColumnOuterByName(columnName);
    const before = await this.kanbanColumnCollapsed(columnName);
    await this.boardHeaderButtons(outer).first().click({ timeout: WebDriver.WAIT_MS });
    const deadline = Date.now() + WebDriver.WAIT_MS;
    for (;;) {
      if ((await this.kanbanColumnCollapsed(columnName)) !== before) return;
      if (Date.now() > deadline) throw new Error(`[parity] column ${JSON.stringify(columnName)} never toggled.`);
      await this.page.waitForTimeout(500);
    }
  }

  async kanbanColumnCollapsed(columnName: string): Promise<boolean> {
    // A collapsed header folds to a narrow vertical strip; the width
    // class is the definitive marker (a lazy column keeps full width).
    const outer = await this.boardFlatColumnOuterByName(columnName);
    const header = outer.locator(":scope > div.sticky > div").first();
    const cls = (await header.getAttribute("class").catch(() => null)) ?? "";
    return cls.includes("w-[44px]");
  }

  async kanbanToggleSwimlane(laneName: string): Promise<void> {
    const bar = await this.kanbanLaneBarByName(laneName);
    const before = await this.kanbanSwimlaneCollapsed(laneName);
    // The toggle handler lives on the header card: clicking the empty
    // full-width bar area does nothing.
    await bar.locator("div.cursor-pointer").first().click({ timeout: WebDriver.WAIT_MS });
    const deadline = Date.now() + WebDriver.WAIT_MS;
    for (;;) {
      if ((await this.kanbanSwimlaneCollapsed(laneName)) !== before) return;
      if (Date.now() > deadline) throw new Error(`[parity] swimlane ${JSON.stringify(laneName)} never toggled.`);
      await this.page.waitForTimeout(500);
    }
  }

  private async kanbanLaneBarByName(laneName: string): Promise<Locator> {
    const bars = this.kanbanLaneBars();
    const count = await bars.count();
    for (let index = 0; index < count; index += 1) {
      const { name } = WebDriver.splitHeaderCount(await bars.nth(index).innerText());
      if (name === laneName) return bars.nth(index);
    }
    throw new Error(`[parity] no swimlane named ${JSON.stringify(laneName)}.`);
  }

  async kanbanSwimlaneCollapsed(laneName: string): Promise<boolean> {
    // A collapsed lane unmounts its board section (no column drop
    // targets); an expanded-but-empty lane keeps them.
    const wrapper = await this.kanbanLaneWrapperByName(laneName);
    return (await wrapper.locator('div[id*="__"]').count()) === 0;
  }

  async kanbanCardIdentifier(issueName: string): Promise<string | null> {
    const card = await this.boardCardByName(issueName);
    const badge = card.locator("button[disabled]").first();
    if ((await badge.count()) === 0) return null;
    return (await badge.innerText()).trim() || null;
  }

  async kanbanCardShowsProperties(issueName: string): Promise<boolean> {
    const card = await this.boardCardByName(issueName);
    return (await card.locator("div.whitespace-nowrap button").count()) > 0;
  }

  async kanbanCardHover(issueName: string): Promise<void> {
    const card = await this.boardCardByName(issueName);
    await card.hover({ timeout: WebDriver.WAIT_MS });
  }

  async kanbanCardQuickActionsVisible(issueName: string): Promise<boolean> {
    const card = await this.boardCardByName(issueName);
    const menu = card.locator('button[id^="headlessui-menu-button"]').first();
    if ((await menu.count()) === 0) return false;
    return await menu.isVisible();
  }

  async kanbanCardHref(issueName: string): Promise<string | null> {
    const card = await this.boardCardByName(issueName);
    return await card.getAttribute("href");
  }

  private issuePeekPanel(): Locator {
    return this.page.locator("div.absolute.top-0.right-0.bottom-0").last();
  }

  async kanbanOpenCardPeek(issueName: string): Promise<void> {
    const card = await this.boardCardByName(issueName);
    await card.click({ timeout: WebDriver.WAIT_MS });
    await this.issuePeekPanel().waitFor({ timeout: WebDriver.BOARD_FIRST_WAIT_MS });
    const deadline = Date.now() + WebDriver.BOARD_FIRST_WAIT_MS;
    for (;;) {
      if ((await this.issuePeekTitle()) === issueName) return;
      if (Date.now() > deadline) throw new Error(`[parity] peek never showed ${JSON.stringify(issueName)}.`);
      await this.page.waitForTimeout(WebDriver.BOARD_POLL_STEP_MS);
    }
  }

  async issuePeekVisible(): Promise<boolean> {
    const panel = this.issuePeekPanel();
    if ((await panel.count()) === 0) return false;
    return await panel.isVisible();
  }

  async issuePeekTitle(): Promise<string | null> {
    if (!(await this.issuePeekVisible())) return null;
    const text = await this.issuePeekPanel().innerText();
    const lines = text
      .split("\n")
      .map((line) => line.trim())
      .filter((line) => line.length > 0);
    const idIndex = lines.findIndex((line) => /^[A-Z]+-\d+$/.test(line));
    if (idIndex < 0 || idIndex + 1 >= lines.length) return null;
    return lines[idIndex + 1] ?? null;
  }

  async issuePeekClose(): Promise<void> {
    await this.page.keyboard.press("Escape");
    const deadline = Date.now() + WebDriver.WAIT_MS;
    for (;;) {
      if (!(await this.issuePeekVisible())) return;
      if (Date.now() > deadline) throw new Error("[parity] peek never closed.");
      await this.page.waitForTimeout(500);
    }
  }

  private boardColumnQuickAdd(outer: Locator): Locator {
    return outer.getByText("New work item", { exact: true });
  }

  async kanbanColumnHasQuickAdd(columnName: string): Promise<boolean> {
    const outer = await this.boardFlatColumnOuterByName(columnName);
    const entry = this.boardColumnQuickAdd(outer);
    if ((await entry.count()) === 0) return false;
    return await entry.first().isVisible();
  }

  async kanbanQuickAdd(columnName: string, title: string): Promise<void> {
    const outer = await this.boardFlatColumnOuterByName(columnName);
    await this.boardColumnQuickAdd(outer).first().click({ timeout: WebDriver.WAIT_MS });
    const field = outer.getByPlaceholder("Work item title");
    await field.waitFor({ timeout: WebDriver.WAIT_MS });
    await field.fill(title);
    await field.press("Enter");
    const deadline = Date.now() + WebDriver.BOARD_FIRST_WAIT_MS;
    for (;;) {
      if ((await this.kanbanColumnCards(columnName)).includes(title)) return;
      if (Date.now() > deadline) {
        throw new Error(`[parity] quick-added card ${JSON.stringify(title)} never showed.`);
      }
      await this.page.waitForTimeout(WebDriver.BOARD_POLL_STEP_MS);
    }
  }

  async kanbanHeaderCreateVisible(columnName: string): Promise<boolean> {
    const outer = await this.boardFlatColumnOuterByName(columnName);
    const header = outer.locator(":scope > div.sticky").first();
    // Project context renders a second header button; cycle/module
    // context renders a menu entry instead.
    if ((await this.boardHeaderButtons(outer).count()) > 1) return true;
    const menuEntry = header.locator("span.cursor-pointer").first();
    if ((await menuEntry.count()) === 0) return false;
    return await menuEntry.isVisible();
  }

  async kanbanHeaderCreate(columnName: string): Promise<void> {
    const outer = await this.boardFlatColumnOuterByName(columnName);
    const header = outer.locator(":scope > div.sticky").first();
    if ((await this.boardHeaderButtons(outer).count()) > 1) {
      await this.boardHeaderButtons(outer).nth(1).click({ timeout: WebDriver.WAIT_MS });
    } else {
      await header.locator("span.cursor-pointer").first().click({ timeout: WebDriver.WAIT_MS });
    }
    const deadline = Date.now() + WebDriver.BOARD_FIRST_WAIT_MS;
    for (;;) {
      if (await this.kanbanCreateModalVisible()) return;
      const items = header.locator('[role="menuitem"], [role="menu"] button, ul button');
      if ((await items.count()) > 0) return;
      if (Date.now() > deadline) throw new Error("[parity] header create opened neither a modal nor a menu.");
      await this.page.waitForTimeout(500);
    }
  }

  async kanbanCreateModalVisible(): Promise<boolean> {
    // The create modal is the only surface pairing the assignee picker
    // placeholder with a dialog; peek labels the same control differently.
    const field = this.page.getByPlaceholder("Assignees");
    if ((await field.count()) === 0) return false;
    return await field.first().isVisible();
  }

  private async boardHeaderMenuScope(columnName: string): Promise<{ header: Locator; items: Locator }> {
    const outer = await this.boardFlatColumnOuterByName(columnName);
    const header = outer.locator(":scope > div.sticky").first();
    return { header, items: this.page.locator('[role="menu"] [role="menuitem"], [role="menu"] button') };
  }

  async kanbanHeaderMenuItems(columnName: string): Promise<string[]> {
    const { items } = await this.boardHeaderMenuScope(columnName);
    const count = await items.count();
    const out: string[] = [];
    for (let index = 0; index < count; index += 1) {
      out.push(((await items.nth(index).innerText()) ?? "").trim().replace(/\s+/g, " "));
    }
    return out.filter((entry) => entry.length > 0);
  }

  async kanbanHeaderMenuChoose(columnName: string, item: string): Promise<void> {
    const { items } = await this.boardHeaderMenuScope(columnName);
    const count = await items.count();
    for (let index = 0; index < count; index += 1) {
      const label = ((await items.nth(index).innerText()) ?? "").trim().replace(/\s+/g, " ");
      if (label === item) {
        await items.nth(index).click({ timeout: WebDriver.WAIT_MS });
        return;
      }
    }
    throw new Error(`[parity] no header menu entry ${JSON.stringify(item)}.`);
  }

  private async boardMouseDrag(points: { x: number; y: number }[]): Promise<void> {
    // The board drag engine is pointer-based: press, stepped moves with
    // pauses so drop targets register, then release.
    if (points.length < 2) throw new Error("[parity] drag needs at least two points.");
    const [first, ...rest] = points as [{ x: number; y: number }, ...{ x: number; y: number }[]];
    await this.page.mouse.move(first.x, first.y);
    await this.page.mouse.down();
    for (const point of rest) {
      await this.page.mouse.move(point.x, point.y, { steps: 8 });
      await this.page.waitForTimeout(150);
    }
    await this.page.mouse.up();
  }

  private static boxCenter(box: { x: number; y: number; width: number; height: number }): {
    x: number;
    y: number;
  } {
    return { x: box.x + box.width / 2, y: box.y + box.height / 2 };
  }

  private async boardSettle(description: string, ready: () => Promise<boolean>): Promise<void> {
    const deadline = Date.now() + WebDriver.BOARD_FIRST_WAIT_MS;
    for (;;) {
      if (await ready().catch(() => false)) return;
      if (Date.now() > deadline) throw new Error(`[parity] ${description} never settled.`);
      await this.page.waitForTimeout(WebDriver.BOARD_POLL_STEP_MS);
    }
  }

  async kanbanDragCardBefore(sourceName: string, targetName: string): Promise<void> {
    const source = await this.boardCardByName(sourceName);
    const target = await this.boardCardByName(targetName);
    const sourceBox = await source.boundingBox();
    const targetBox = await target.boundingBox();
    if (!sourceBox || !targetBox) throw new Error("[parity] drag card has no box.");
    const from = WebDriver.boxCenter(sourceBox);
    const onto = { x: targetBox.x + targetBox.width / 2, y: targetBox.y + 8 };
    await this.boardMouseDrag([from, onto]);
    await this.boardSettle("card reorder", async () => {
      const cards = await this.kanbanCards();
      const names = cards.map((card) => card.name);
      const sourceIndex = names.indexOf(sourceName);
      const targetIndex = names.indexOf(targetName);
      return sourceIndex >= 0 && targetIndex >= 0 && sourceIndex === targetIndex - 1;
    });
  }

  async kanbanAttemptCardBefore(sourceName: string, targetName: string): Promise<void> {
    const source = await this.boardCardByName(sourceName);
    const target = await this.boardCardByName(targetName);
    const sourceBox = await source.boundingBox();
    const targetBox = await target.boundingBox();
    if (!sourceBox || !targetBox) throw new Error("[parity] drag card has no box.");
    const from = WebDriver.boxCenter(sourceBox);
    const onto = { x: targetBox.x + targetBox.width / 2, y: targetBox.y + 8 };
    await this.boardMouseDrag([from, onto]);
    await this.page.waitForTimeout(3_000);
  }

  async kanbanDragCardToColumnEnd(sourceName: string, columnName: string): Promise<void> {
    const source = await this.boardCardByName(sourceName);
    const outer = await this.boardFlatColumnOuterByName(columnName);
    const sourceBox = await source.boundingBox();
    await outer.scrollIntoViewIfNeeded({ timeout: WebDriver.WAIT_MS });
    const outerBox = await outer.boundingBox();
    if (!sourceBox || !outerBox) throw new Error("[parity] drag card or column has no box.");
    const from = WebDriver.boxCenter(sourceBox);
    const onto = { x: outerBox.x + outerBox.width / 2, y: outerBox.y + outerBox.height - 12 };
    await this.boardMouseDrag([from, onto]);
    await this.boardSettle("column drop", async () => (await this.kanbanColumnCards(columnName)).includes(sourceName));
  }

  private boardDeleteZone(): Locator {
    return this.page.getByText("Drop here to delete", { exact: false });
  }

  async kanbanDragCardToDelete(sourceName: string): Promise<void> {
    const source = await this.boardCardByName(sourceName);
    const sourceBox = await source.boundingBox();
    // The zone mounts with the board (transparent until a drag starts),
    // so its box can be read before pressing the card.
    const zoneBox = await this.boardDeleteZone().first().boundingBox();
    if (!sourceBox || !zoneBox) throw new Error("[parity] delete drag has no box.");
    const from = WebDriver.boxCenter(sourceBox);
    const onto = WebDriver.boxCenter(zoneBox);
    await this.boardMouseDrag([from, onto]);
    await this.boardSettle("delete modal", async () => this.kanbanDeleteModalVisible());
  }

  private boardDeleteModal(): Locator {
    return this.page.getByRole("dialog").filter({ hasText: "Delete Work item" });
  }

  async kanbanDeleteModalVisible(): Promise<boolean> {
    const modal = this.boardDeleteModal();
    if ((await modal.count()) === 0) {
      // Fall back to the title text: some modal roots carry no dialog role.
      const title = this.page.getByText("Delete Work item", { exact: true });
      if ((await title.count()) === 0) return false;
      return await title.first().isVisible();
    }
    return await modal.first().isVisible();
  }

  async kanbanConfirmDelete(): Promise<void> {
    const modal = this.boardDeleteModal();
    const scope = (await modal.count()) > 0 ? modal.first() : this.page;
    const confirm = scope.getByRole("button", { name: /^Delete$/ }).first();
    await confirm.click({ timeout: WebDriver.WAIT_MS });
    await this.boardSettle("delete confirm", async () => !(await this.kanbanDeleteModalVisible()));
  }

  async kanbanDragHoldOverColumn(sourceName: string, columnName: string): Promise<{ overlay: string | null }> {
    const source = await this.boardCardByName(sourceName);
    const outer = await this.boardFlatColumnOuterByName(columnName);
    const sourceBox = await source.boundingBox();
    await outer.scrollIntoViewIfNeeded({ timeout: WebDriver.WAIT_MS });
    const outerBox = await outer.boundingBox();
    if (!sourceBox || !outerBox) throw new Error("[parity] overlay drag has no box.");
    const from = WebDriver.boxCenter(sourceBox);
    const onto = WebDriver.boxCenter(outerBox);
    await this.page.mouse.move(from.x, from.y);
    await this.page.mouse.down();
    await this.page.mouse.move(onto.x, onto.y, { steps: 8 });
    await this.page.waitForTimeout(1_000);
    // The feedback overlay is the column body's first child; it hides
    // behind a `hidden` class when the column accepts the card.
    const overlay = outer.locator(':scope div[id$="__null"] > div').first();
    let text: string | null = null;
    if ((await overlay.count()) > 0) {
      const cls = (await overlay.getAttribute("class").catch(() => null)) ?? "";
      if (!cls.split(/\s+/).includes("hidden")) {
        text = ((await overlay.innerText().catch(() => null)) ?? "").trim().replace(/\s+/g, " ") || null;
      }
    }
    await this.page.mouse.up();
    await this.page.waitForTimeout(1_500);
    return { overlay: text };
  }

  async boardLastToast(): Promise<{ title: string; message: string } | null> {
    const toasts = this.page.locator('[aria-label="Notifications"] [role="dialog"]');
    const count = await toasts.count();
    if (count === 0) return null;
    const text = (
      (await toasts
        .nth(count - 1)
        .innerText()
        .catch(() => null)) ?? ""
    ).trim();
    if (!text) return null;
    const [title, ...rest] = text
      .split("\n")
      .map((line) => line.trim())
      .filter((line) => line.length > 0);
    if (!title) return null;
    return { title, message: rest.join(" ") };
  }

  async kanbanColumnScrollEnd(columnName: string): Promise<void> {
    const outer = await this.boardFlatColumnOuterByName(columnName);
    const inner = outer.locator(':scope div[id$="__null"]').first();
    await inner.evaluate((element) => {
      element.scrollTop = element.scrollHeight;
    });
    await this.page.waitForTimeout(2_000);
  }

  async kanbanColumnHasLoadMore(columnName: string): Promise<boolean> {
    const outer = await this.boardFlatColumnOuterByName(columnName);
    const link = outer.getByText("Load more", { exact: false });
    if ((await link.count()) === 0) return false;
    return await link.first().isVisible();
  }

  async kanbanColumnLoadMore(columnName: string): Promise<void> {
    const outer = await this.boardFlatColumnOuterByName(columnName);
    const before = await this.kanbanColumnCards(columnName);
    await outer.getByText("Load more", { exact: false }).first().click({ timeout: WebDriver.WAIT_MS });
    await this.boardSettle("load more", async () => (await this.kanbanColumnCards(columnName)).length > before.length);
  }

  async kanbanColumnLoading(columnName: string): Promise<boolean> {
    const outer = await this.boardFlatColumnOuterByName(columnName);
    return (await outer.locator('div[class*="animate-pulse"]').count()) > 0;
  }

  private async kanbanSwimlaneCell(columnName: string, laneName: string): Promise<Locator> {
    // Swimlane cells share the group order of the header row: resolve the
    // column positionally, then take that cell of the named lane.
    const columns = await this.kanbanSwimlaneGroupColumns();
    const index = columns.findIndex((column) => column.name === columnName);
    if (index < 0) throw new Error(`[parity] no swimlane group column named ${JSON.stringify(columnName)}.`);
    const wrapper = await this.kanbanLaneWrapperByName(laneName);
    const cells = wrapper.locator('div[id*="__"]');
    if ((await cells.count()) <= index) {
      throw new Error(`[parity] swimlane ${JSON.stringify(laneName)} has no cell for ${JSON.stringify(columnName)}.`);
    }
    return cells.nth(index);
  }

  async kanbanCellCards(columnName: string, laneName: string): Promise<string[]> {
    const cell = await this.kanbanSwimlaneCell(columnName, laneName);
    const cards = cell.locator('a[id^="issue_"]');
    const count = await cards.count();
    const names: string[] = [];
    for (let index = 0; index < count; index += 1) {
      names.push(await this.boardCardName(cards.nth(index)).innerText());
    }
    return names;
  }

  async kanbanCellHasLoadMore(columnName: string, laneName: string): Promise<boolean> {
    const cell = await this.kanbanSwimlaneCell(columnName, laneName);
    const link = cell.getByText("Load more", { exact: false });
    if ((await link.count()) === 0) return false;
    return await link.first().isVisible();
  }

  async kanbanCellLoadMore(columnName: string, laneName: string): Promise<void> {
    const cell = await this.kanbanSwimlaneCell(columnName, laneName);
    const before = await this.kanbanCellCards(columnName, laneName);
    await cell.getByText("Load more", { exact: false }).first().click({ timeout: WebDriver.WAIT_MS });
    await this.boardSettle(
      "cell load more",
      async () => (await this.kanbanCellCards(columnName, laneName)).length > before.length
    );
  }

  async kanbanBoardScroll(): Promise<{ x: number; y: number }> {
    return await this.page.evaluate(() => {
      const probe = document.querySelector('div[id*="__"]');
      let node: HTMLElement | null = probe instanceof HTMLElement ? probe : null;
      while (node) {
        if (node.scrollWidth > node.clientWidth + 4 || node.scrollHeight > node.clientHeight + 4) {
          return { x: node.scrollLeft, y: node.scrollTop };
        }
        node = node.parentElement;
      }
      return { x: 0, y: 0 };
    });
  }

  async kanbanDragHoldNearEdge(
    sourceName: string,
    edge: "left" | "right" | "top" | "bottom",
    holdMs: number
  ): Promise<void> {
    const source = await this.boardCardByName(sourceName);
    const sourceBox = await source.boundingBox();
    const viewport = this.page.viewportSize() ?? { width: 1280, height: 720 };
    if (!sourceBox) throw new Error("[parity] edge-hold card has no box.");
    const from = WebDriver.boxCenter(sourceBox);
    const margin = 12;
    const onto =
      edge === "left"
        ? { x: margin, y: viewport.height / 2 }
        : edge === "right"
          ? { x: viewport.width - margin, y: viewport.height / 2 }
          : edge === "top"
            ? { x: viewport.width / 2, y: margin }
            : { x: viewport.width / 2, y: viewport.height - margin };
    await this.page.mouse.move(from.x, from.y);
    await this.page.mouse.down();
    await this.page.mouse.move(onto.x, onto.y, { steps: 10 });
    await this.page.waitForTimeout(holdMs);
    await this.page.mouse.up();
    await this.page.waitForTimeout(1_000);
  }

  private ganttContainer(): Locator {
    return this.page.locator("#gantt-container");
  }

  private ganttChartRoot(): Locator {
    return this.ganttContainer().locator("xpath=..");
  }

  async ganttHeader(): Promise<{ count: number | null; views: string[]; hasToday: boolean; hasFullscreen: boolean }> {
    const root = this.ganttChartRoot();
    const text =
      (
        (await root
          .first()
          .innerText()
          .catch(() => null)) ?? ""
      ).split("\n")[0] ?? "";
    const countMatch = /(\d+)\s+Work items/.exec(text);
    // The zoom switcher renders one entry per zoom level; the active one
    // carries the pill marker.
    const views: string[] = [];
    for (const view of ["Week", "Month", "Quarter"] as const) {
      if ((await root.getByRole("button", { name: view, exact: true }).count()) > 0) views.push(view);
    }
    const hasToday = (await root.getByRole("button", { name: "Today", exact: true }).count()) > 0;
    const buttons = root.locator(":scope button");
    const buttonCount = await buttons.count();
    let hasFullscreen = false;
    for (let index = 0; index < buttonCount; index += 1) {
      const label = (
        (await buttons
          .nth(index)
          .innerText()
          .catch(() => null)) ?? ""
      ).trim();
      // The fullscreen toggle is the header's trailing icon-only button.
      if (label === "") hasFullscreen = true;
    }
    return { count: countMatch ? Number(countMatch[1]) : null, views, hasToday, hasFullscreen };
  }

  private ganttZoomCells(): Locator {
    // Sub-title cells of the rendered zoom: days (week), week ranges
    // (month), or months (quarter).
    return this.ganttContainer().locator("div.flex.h-5 > div");
  }

  async ganttActiveZoom(): Promise<GanttZoom | "unknown"> {
    const cells = this.ganttZoomCells();
    if ((await cells.count()) === 0) return "unknown";
    const first = (
      (await cells
        .first()
        .innerText()
        .catch(() => null)) ?? ""
    )
      .trim()
      .replace(/\s+/g, " ");
    if (/^\d+\s*-\s*\d+/.test(first)) return "Month";
    if (/^(Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec)/i.test(first)) return "Quarter";
    if (/^(Su|M|T|W|Th|F|Sa)\b/i.test(first)) return "Week";
    return "unknown";
  }

  async ganttSetZoom(view: GanttZoom): Promise<void> {
    const root = this.ganttChartRoot();
    const entry = root.getByRole("button", { name: view, exact: true }).first();
    if ((await entry.count()) === 0) {
      // Select-style switcher: open the current value, then pick the entry.
      const current = await this.ganttActiveZoom();
      if (current === "unknown") throw new Error("[parity] timeline zoom switcher not found.");
      await root.getByRole("button", { name: current, exact: true }).first().click({ timeout: WebDriver.WAIT_MS });
      await this.page.getByRole("option", { name: view, exact: true }).first().click({ timeout: WebDriver.WAIT_MS });
    } else {
      await entry.click({ timeout: WebDriver.WAIT_MS });
    }
    await this.boardSettle("zoom switch", async () => (await this.ganttActiveZoom()) === view);
  }

  async ganttDayWidth(): Promise<number> {
    const zoom = await this.ganttActiveZoom();
    if (zoom === "unknown") throw new Error("[parity] cannot measure day width for an unknown zoom.");
    const cells = this.ganttZoomCells();
    if (zoom === "Week" || zoom === "Month") {
      const box = await cells.first().boundingBox();
      if (!box) throw new Error("[parity] zoom cell has no box.");
      return zoom === "Week" ? box.width : box.width / 7;
    }
    // Quarter cells span whole months: measure the current month's cell
    // (marked with the today pill) and divide by its days.
    const count = await cells.count();
    for (let index = 0; index < count; index += 1) {
      const pill = cells.nth(index).locator('[class*="bg-accent-primary"]');
      if ((await pill.count()) > 0) {
        const box = await cells.nth(index).boundingBox();
        if (!box) throw new Error("[parity] quarter cell has no box.");
        const now = new Date();
        const days = new Date(now.getFullYear(), now.getMonth() + 1, 0).getDate();
        return box.width / days;
      }
    }
    throw new Error("[parity] current month cell not found; center today first.");
  }

  async ganttWeekendTinted(): Promise<boolean> {
    // Weekend day columns carry an inner tint block; weekdays do not.
    const tinted = await this.page.evaluate(() => {
      const container = document.querySelector("#gantt-container");
      if (!container) return { total: 0, weekend: 0 };
      const columns = [...container.querySelectorAll("div.flex.h-full.w-full > div")];
      const weekend = columns.filter((column) => column.querySelector("div.bg-surface-2")).length;
      return { total: columns.length, weekend };
    });
    return tinted.total > 0 && tinted.weekend > 0 && tinted.weekend < tinted.total;
  }

  async ganttWeekRowStarts(): Promise<string[]> {
    if ((await this.ganttActiveZoom()) !== "Week") return [];
    return await this.page.evaluate(() => {
      const container = document.querySelector("#gantt-container");
      if (!container) return [];
      const blocks = [...container.querySelectorAll("div.absolute.top-0.left-0 > div.relative")];
      return blocks.map((block) => {
        const first = block.querySelector("div.flex.h-5 > div div");
        return (first?.textContent ?? "").trim().split(/\s+/)[0] ?? "";
      });
    });
  }

  private ganttTodayRects(): Promise<{ left: number; right: number }[]> {
    return this.page.evaluate(() => {
      const marked = [...document.querySelectorAll("#gantt-container div.bg-accent-primary\\/20")] as HTMLElement[];
      return marked.map((element) => {
        const rect = element.getBoundingClientRect();
        return { left: rect.left, right: rect.right };
      });
    });
  }

  async ganttClickToday(): Promise<void> {
    await this.ganttChartRoot()
      .getByRole("button", { name: "Today", exact: true })
      .click({ timeout: WebDriver.WAIT_MS });
    const viewport = this.page.viewportSize() ?? { width: 1280, height: 720 };
    await this.boardSettle("today re-center", async () => {
      const rects = await this.ganttTodayRects();
      if (rects.length === 0) return false;
      const widest = rects.reduce((a, b) => (b.right - b.left > a.right - a.left ? b : a));
      const center = (widest.left + widest.right) / 2;
      return Math.abs(center - viewport.width / 2) < 150;
    });
  }

  async ganttTodayVisible(): Promise<boolean> {
    const viewport = this.page.viewportSize() ?? { width: 1280, height: 720 };
    const rects = await this.ganttTodayRects();
    return rects.some((rect) => rect.left < viewport.width && rect.right > 0);
  }

  async ganttTodayHighlighted(): Promise<boolean> {
    return (await this.ganttContainer().locator('div[class*="bg-accent-primary/20"]').count()) > 0;
  }

  async ganttToggleFullscreen(): Promise<void> {
    const root = this.ganttChartRoot();
    const before = await this.ganttFullscreenActive();
    const buttons = root.locator(":scope button");
    const count = await buttons.count();
    for (let index = count - 1; index >= 0; index -= 1) {
      const label = (
        (await buttons
          .nth(index)
          .innerText()
          .catch(() => null)) ?? ""
      ).trim();
      if (label === "") {
        await buttons.nth(index).click({ timeout: WebDriver.WAIT_MS });
        await this.boardSettle("fullscreen toggle", async () => (await this.ganttFullscreenActive()) !== before);
        return;
      }
    }
    throw new Error("[parity] fullscreen toggle not found.");
  }

  async ganttFullscreenActive(): Promise<boolean> {
    return (await this.page.locator("#full-screen-portal #gantt-container").count()) > 0;
  }

  async ganttTimelineWidth(): Promise<number> {
    return await this.page.evaluate(() => {
      const container = document.querySelector("#gantt-container");
      if (!(container instanceof HTMLElement)) return 0;
      const items = [...container.querySelectorAll("div")].find((element) => {
        const width = (element as HTMLElement).style?.width ?? "";
        return /^\d+px$/.test(width) && element.children.length > 0;
      }) as HTMLElement | undefined;
      return items ? Math.round(items.getBoundingClientRect().width) : 0;
    });
  }

  async ganttScrollLeft(): Promise<number> {
    return await this.ganttContainer().evaluate((element) => element.scrollLeft);
  }

  async ganttScrollTo(x: number): Promise<void> {
    await this.ganttContainer().evaluate((element, target) => {
      element.scrollLeft = target;
    }, x);
    await this.page.waitForTimeout(2_000);
  }

  private ganttSidebar(): Locator {
    return this.page.locator("#gantt-sidebar");
  }

  private ganttSidebarLinks(): Locator {
    return this.ganttSidebar().locator('a[id^="issue-"]');
  }

  private async ganttSidebarLinkByName(issueName: string): Promise<Locator> {
    const links = this.ganttSidebarLinks();
    const count = await links.count();
    for (let index = 0; index < count; index += 1) {
      if ((await this.ganttSidebarRowName(links.nth(index))) === issueName) return links.nth(index);
    }
    throw new Error(`[parity] no timeline row titled ${JSON.stringify(issueName)}.`);
  }

  private async ganttSidebarRowName(link: Locator): Promise<string> {
    const text = ((await link.innerText().catch(() => null)) ?? "").trim().replace(/\s+/g, " ");
    return text.replace(/^[A-Z]+-\d+\s+/, "");
  }

  private async ganttSidebarRowIdentifier(link: Locator): Promise<string | null> {
    const text = ((await link.innerText().catch(() => null)) ?? "").trim().replace(/\s+/g, " ");
    const match = /^([A-Z]+-\d+)\s+/.exec(text);
    return match ? (match[1] ?? null) : null;
  }

  private ganttSidebarRowDuration(link: Locator): Locator {
    return link.locator("xpath=../..").locator(":scope > div.flex-shrink-0");
  }

  async ganttSidebarRows(): Promise<GanttSidebarRow[]> {
    const links = this.ganttSidebarLinks();
    const count = await links.count();
    const rows: GanttSidebarRow[] = [];
    for (let index = 0; index < count; index += 1) {
      const link = links.nth(index);
      const duration = this.ganttSidebarRowDuration(link);
      rows.push({
        identifier: await this.ganttSidebarRowIdentifier(link),
        name: await this.ganttSidebarRowName(link),
        duration: (await duration.count()) > 0 ? (await duration.innerText()).trim() || null : null,
      });
    }
    return rows;
  }

  async ganttOpenRowPeek(issueName: string): Promise<void> {
    const link = await this.ganttSidebarLinkByName(issueName);
    await link.click({ timeout: WebDriver.WAIT_MS });
    await this.issuePeekPanel().waitFor({ timeout: WebDriver.BOARD_FIRST_WAIT_MS });
    await this.boardSettle("row peek", async () => (await this.issuePeekTitle()) === issueName);
  }

  async ganttSidebarOrder(): Promise<string[]> {
    const links = this.ganttSidebarLinks();
    const count = await links.count();
    const names: string[] = [];
    for (let index = 0; index < count; index += 1) {
      names.push(await this.ganttSidebarRowName(links.nth(index)));
    }
    return names;
  }

  async ganttDragRowBefore(sourceName: string, targetName: string): Promise<void> {
    const source = await this.ganttSidebarLinkByName(sourceName);
    const target = await this.ganttSidebarLinkByName(targetName);
    const sourceBox = await source.boundingBox();
    const targetBox = await target.boundingBox();
    if (!sourceBox || !targetBox) throw new Error("[parity] sidebar row has no box.");
    const from = WebDriver.boxCenter(sourceBox);
    const onto = { x: targetBox.x + targetBox.width / 2, y: targetBox.y + 6 };
    await this.boardMouseDrag([from, onto]);
    await this.boardSettle("row reorder", async () => {
      const order = await this.ganttSidebarOrder();
      return order.indexOf(sourceName) === order.indexOf(targetName) - 1;
    });
  }

  async ganttAttemptRowBefore(sourceName: string, targetName: string): Promise<void> {
    const source = await this.ganttSidebarLinkByName(sourceName);
    const target = await this.ganttSidebarLinkByName(targetName);
    const sourceBox = await source.boundingBox();
    const targetBox = await target.boundingBox();
    if (!sourceBox || !targetBox) throw new Error("[parity] sidebar row has no box.");
    const from = WebDriver.boxCenter(sourceBox);
    const onto = { x: targetBox.x + targetBox.width / 2, y: targetBox.y + 6 };
    await this.boardMouseDrag([from, onto]);
    await this.page.waitForTimeout(3_000);
  }

  private async ganttIssueIdByName(issueName: string): Promise<string> {
    const link = await this.ganttSidebarLinkByName(issueName);
    const id = (await link.getAttribute("id")) ?? "";
    const match = /^issue-(.+)$/.exec(id);
    if (!match || !match[1]) throw new Error(`[parity] unexpected timeline row id ${JSON.stringify(id)}.`);
    return match[1];
  }

  private ganttBar(issueId: string): Locator {
    return this.ganttContainer().locator(`div[id="gantt-block-${issueId}"]`);
  }

  async ganttBarExists(issueName: string): Promise<boolean> {
    // Dated bars take a positive-width positioned box; undated rows keep
    // a zero-width placeholder, whether mounted or virtualized away.
    const issueId = await this.ganttIssueIdByName(issueName);
    const bar = this.ganttBar(issueId);
    if ((await bar.count()) === 0) return false;
    const box = await bar.boundingBox();
    return !!box && box.width > 0;
  }

  private ganttBarHandle(issueId: string, side: "left" | "right"): Locator {
    const bar = this.ganttBar(issueId);
    const marker = side === "left" ? "-left-1.5" : "-right-1.5";
    return bar.locator(`div.cursor-col-resize[class*="${marker}"]`).first();
  }

  async ganttDragBar(issueName: string, dayDelta: number): Promise<void> {
    if (dayDelta === 0) throw new Error("[parity] bar drag needs a nonzero day delta.");
    const issueId = await this.ganttIssueIdByName(issueName);
    const bar = this.ganttBar(issueId);
    const pxPerDay = await this.ganttDayWidth();
    await bar.scrollIntoViewIfNeeded({ timeout: WebDriver.WAIT_MS });
    const box = await bar.boundingBox();
    if (!box) throw new Error("[parity] bar has no box.");
    const before = await bar.evaluate((element) => (element as HTMLElement).style.marginLeft);
    const from = WebDriver.boxCenter(box);
    const onto = { x: from.x + dayDelta * pxPerDay, y: from.y };
    await this.page.mouse.move(from.x, from.y);
    await this.page.mouse.down();
    await this.page.mouse.move(onto.x, onto.y, { steps: 12 });
    await this.page.mouse.up();
    await this.boardSettle(
      "bar move",
      async () => (await bar.evaluate((element) => (element as HTMLElement).style.marginLeft)) !== before
    );
  }

  async ganttResizeBar(issueName: string, side: "left" | "right", dayDelta: number): Promise<void> {
    if (dayDelta === 0) throw new Error("[parity] bar resize needs a nonzero day delta.");
    const issueId = await this.ganttIssueIdByName(issueName);
    const bar = this.ganttBar(issueId);
    const pxPerDay = await this.ganttDayWidth();
    await bar.scrollIntoViewIfNeeded({ timeout: WebDriver.WAIT_MS });
    const handle = this.ganttBarHandle(issueId, side);
    await handle.waitFor({ timeout: WebDriver.WAIT_MS });
    const box = await handle.boundingBox();
    if (!box) throw new Error("[parity] resize handle has no box.");
    const before = await bar.evaluate((element) => (element as HTMLElement).style.width);
    const from = WebDriver.boxCenter(box);
    const onto = { x: from.x + dayDelta * pxPerDay, y: from.y };
    await this.page.mouse.move(from.x, from.y);
    await this.page.mouse.down();
    await this.page.mouse.move(onto.x, onto.y, { steps: 12 });
    await this.page.mouse.up();
    await this.boardSettle(
      "bar resize",
      async () => (await bar.evaluate((element) => (element as HTMLElement).style.width)) !== before
    );
  }

  async ganttResizePreview(issueName: string, side: "left" | "right"): Promise<string | null> {
    const issueId = await this.ganttIssueIdByName(issueName);
    const handle = this.ganttBarHandle(issueId, side);
    if ((await handle.count()) === 0) return null;
    await handle.hover({ timeout: WebDriver.WAIT_MS });
    await this.page.waitForTimeout(600);
    const pill = this.ganttBar(issueId).locator("div.bg-accent-subtle").first();
    if ((await pill.count()) === 0) return null;
    if (!(await pill.isVisible())) return null;
    return (await pill.innerText()).trim() || null;
  }

  async ganttHandlesVisible(issueName: string): Promise<boolean> {
    const issueId = await this.ganttIssueIdByName(issueName);
    return (await this.ganttBar(issueId).locator("div.cursor-col-resize").count()) > 0;
  }

  private ganttRowOf(issueId: string): Locator {
    // A bar wrapper always mounts inside its timeline row, dated or not;
    // the row is the nearest full-width ancestor.
    return this.ganttBar(issueId).locator("xpath=ancestor::div[contains(@class,'min-w-full')][1]");
  }

  async ganttRowAddVisible(issueName: string): Promise<boolean> {
    const issueId = await this.ganttIssueIdByName(issueName);
    const row = this.ganttRowOf(issueId);
    await row.scrollIntoViewIfNeeded({ timeout: WebDriver.WAIT_MS });
    await row.hover({ timeout: WebDriver.WAIT_MS });
    await this.page.waitForTimeout(600);
    const add = row.locator("button.absolute").first();
    if ((await add.count()) === 0) return false;
    return await add.isVisible();
  }

  async ganttAddBlock(issueName: string, dayOffset: number): Promise<void> {
    const issueId = await this.ganttIssueIdByName(issueName);
    const pxPerDay = await this.ganttDayWidth();
    const row = this.ganttRowOf(issueId);
    await row.scrollIntoViewIfNeeded({ timeout: WebDriver.WAIT_MS });
    const sidebarBox = await this.ganttSidebar().boundingBox();
    const rowBox = await row.boundingBox();
    if (!sidebarBox || !rowBox) throw new Error("[parity] add-block row has no box.");
    const at = {
      x: sidebarBox.x + sidebarBox.width + dayOffset * pxPerDay,
      y: rowBox.y + rowBox.height / 2,
    };
    await this.page.mouse.move(at.x, at.y);
    await this.page.waitForTimeout(600);
    await this.page.mouse.click(at.x, at.y);
    await this.boardSettle("add block", async () => this.ganttBarExists(issueName));
  }

  async ganttQuickAdd(title: string): Promise<void> {
    const root = this.ganttChartRoot();
    await root.getByText("New work item", { exact: true }).first().click({ timeout: WebDriver.WAIT_MS });
    const field = root.getByPlaceholder("Work item title");
    await field.waitFor({ timeout: WebDriver.WAIT_MS });
    await field.fill(title);
    await field.press("Enter");
    await this.boardSettle("timeline quick-add", async () => this.ganttBarExists(title));
  }

  async ganttHasQuickAdd(): Promise<boolean> {
    const entry = this.ganttChartRoot().getByText("New work item", { exact: true }).first();
    if ((await entry.count()) === 0) return false;
    return await entry.isVisible();
  }

  async ganttBarInfo(issueName: string): Promise<{ tinted: boolean; masked: boolean; namePinned: boolean } | null> {
    if (!(await this.ganttBarExists(issueName))) return null;
    const issueId = await this.ganttIssueIdByName(issueName);
    return await this.ganttBar(issueId).evaluate((bar) => {
      const content = bar.querySelector("div[id^='issue-']") as HTMLElement | null;
      if (!content) return null;
      const tint = window.getComputedStyle(content).backgroundColor;
      const mask = window.getComputedStyle(content).maskImage ?? "";
      const name = [...content.querySelectorAll("div")].find((element) => {
        const style = window.getComputedStyle(element);
        return style.position === "sticky";
      }) as HTMLElement | undefined;
      const pinned = !!name && window.getComputedStyle(name).left !== "auto";
      return {
        tinted: !!tint && tint !== "rgba(0, 0, 0, 0)" && tint !== "transparent",
        masked: mask.includes("gradient"),
        namePinned: pinned,
      };
    });
  }

  async ganttHoverBar(issueName: string): Promise<void> {
    const issueId = await this.ganttIssueIdByName(issueName);
    const bar = this.ganttBar(issueId);
    await bar.scrollIntoViewIfNeeded({ timeout: WebDriver.WAIT_MS });
    await bar.hover({ timeout: WebDriver.WAIT_MS });
    await this.page.waitForTimeout(600);
  }

  async ganttPreviewVisible(): Promise<boolean> {
    const popover = this.page.locator('[data-slot="popover-content"]');
    if ((await popover.count()) === 0) return false;
    return await popover.first().isVisible();
  }

  async ganttOpenBarPeek(issueName: string): Promise<void> {
    const issueId = await this.ganttIssueIdByName(issueName);
    const bar = this.ganttBar(issueId);
    await bar.scrollIntoViewIfNeeded({ timeout: WebDriver.WAIT_MS });
    await bar.click({ timeout: WebDriver.WAIT_MS });
    await this.issuePeekPanel().waitFor({ timeout: WebDriver.BOARD_FIRST_WAIT_MS });
    await this.boardSettle("bar peek", async () => (await this.issuePeekTitle()) === issueName);
  }

  async ganttScrollArrowVisible(issueName: string): Promise<boolean> {
    const issueId = await this.ganttIssueIdByName(issueName);
    const arrow = this.ganttRowOf(issueId).locator("button.sticky").first();
    if ((await arrow.count()) === 0) return false;
    return await arrow.isVisible();
  }

  async ganttClickScrollArrow(issueName: string): Promise<void> {
    const issueId = await this.ganttIssueIdByName(issueName);
    await this.ganttRowOf(issueId).locator("button.sticky").first().click({ timeout: WebDriver.WAIT_MS });
    await this.boardSettle("scroll to block", async () => this.ganttBarInView(issueName));
  }

  async ganttBarInView(issueName: string): Promise<boolean> {
    const issueId = await this.ganttIssueIdByName(issueName);
    const box = await this.ganttBar(issueId).boundingBox();
    if (!box) return false;
    const viewport = this.page.viewportSize() ?? { width: 1280, height: 720 };
    return box.x < viewport.width && box.x + box.width > 0;
  }

  async ganttSidebarLoading(): Promise<boolean> {
    return (await this.ganttSidebar().locator('div[class*="animate-pulse"]').count()) > 0;
  }

  async ganttLoadMoreVisible(): Promise<boolean> {
    const pulses = this.ganttSidebar().locator('div[class*="animate-pulse"]');
    if ((await pulses.count()) === 0) return false;
    return await pulses.last().isVisible();
  }

  async ganttLoadingObservedOnReload(): Promise<boolean> {
    const pattern = "**/api/**/issues**";
    await this.page.route(pattern, async (route) => {
      await new Promise((resolve) => setTimeout(resolve, 2_500));
      await route.continue();
    });
    try {
      await this.page.reload();
      const deadline = Date.now() + 60_000;
      for (;;) {
        if (await this.ganttSidebarLoading()) return true;
        if (Date.now() > deadline) return false;
        await this.page.waitForTimeout(250);
      }
    } finally {
      await this.page.unroute(pattern);
    }
  }
}
