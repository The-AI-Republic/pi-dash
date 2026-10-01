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
  /** Tighter bound for menus and dialogs, which render synchronously once open. */
  private static readonly OPEN_MS = 30_000;

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
    // Wait for the navigation itself: a URL regex also matches the bare
    // origin ("//host/"), which would return before the sign-in POST lands.
    await Promise.all([
      page.waitForNavigation({ waitUntil: "domcontentloaded", timeout: WebDriver.WAIT_MS }),
      this.submitOf(passwordForm).click(),
    ]);
  }

  private signedInPath(url: string): boolean {
    const pathname = new URL(url).pathname;
    return pathname !== "/" && !pathname.startsWith("/auth") && !pathname.startsWith("/sign");
  }

  async openProjectIssues(workspaceSlug: string, projectId: string): Promise<void> {
    await this.page.goto(`/${workspaceSlug}/projects/${projectId}/issues`);
    await this.page.waitForLoadState("domcontentloaded");
    // The dev-server route module intermittently fails to fetch under
    // sibling contention, leaving main empty; reload until the list boots
    // (chrome text renders) the same way the detail/peek opens do.
    const deadline = Date.now() + WebDriver.DETAIL_MS;
    let loops = 0;
    for (;;) {
      // Bounded: a blank boot renders no <main> at all, and an unbounded
      // read would hang past the deadline instead of iterating to it.
      const text = await this.page
        .getByRole("main")
        .innerText({ timeout: 5000 })
        .catch(() => "");
      if (text.trim().length > 50) return;
      if ((await this.page.getByPlaceholder("name@company.com").count()) > 0) {
        throw new Error("[parity] session lost while opening the issues list.");
      }
      loops++;
      if (Date.now() > deadline) return;
      if (loops % 15 === 0) await this.page.reload().catch(() => {});
      else await this.page.waitForTimeout(2000);
    }
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

  // --- Issues bulk-ops / modal / drafts driver methods (NEWFRONT-120).
  // --- Appended; existing methods above are untouched per the shared
  // --- driver contract.

  // Retry wrapper around the shared sign-in flow (NEWFRONT-120, ISS-108-141).
  // The shared stack rate-limits credential posts under concurrent oracle
  // runs; the shared signInWithPassword stays untouched for sibling areas.
  async signInWithPasswordRetry(email: string, password: string): Promise<void> {
    // The shared stack rate-limits credential posts under concurrent oracle
    // runs, surfacing as an authentication error page; retry the flow a few
    // times with a backoff before giving up.
    let lastError: unknown;
    for (let attempt = 1; attempt <= 4; attempt += 1) {
      try {
        await this.trySignInOnce(email, password);
        return;
      } catch (error) {
        lastError = error;
        if (attempt < 4) await this.page.waitForTimeout(5000 * attempt);
      }
    }
    throw lastError;
  }

  private async trySignInOnce(email: string, password: string): Promise<void> {
    // Waits stay short on purpose: a stalled page must fail an attempt
    // fast so the retry loop — not one hung wait — spends the budget.
    // Slow-but-healthy loads still pass because the next attempt retries
    // against an already-compiling page.
    const page = this.page;
    await page.goto("/");
    const emailField = page.getByPlaceholder("name@company.com").first();
    await emailField.waitFor({ timeout: 30_000 });
    await emailField.fill(email);
    const emailForm = page.locator("form", { has: emailField });
    await this.submitOf(emailForm).click();
    const passwordField = page.getByPlaceholder("Enter password");
    await passwordField.waitFor({ timeout: 30_000 });
    await passwordField.fill(password);
    const passwordForm = page.locator("form", { has: passwordField });
    // The old app posts the native form, so this ends in a full page load
    // landing inside the workspace. Wait for the path to actually leave the
    // entry route: the entry URL alone already satisfies looser patterns.
    await Promise.all([
      page.waitForURL((url) => url.pathname !== "/", { timeout: 30_000 }),
      this.submitOf(passwordForm).click(),
    ]);
  }

  /**
   * Reload past the dev server's "Runtime Error" overlay. Under concurrent
   * oracle load the dev server intermittently fails a dynamic chunk import
   * ("Failed to fetch dynamically imported module"), leaving the page on an
   * error boundary with the app detached; a reload re-serves it. Returns
   * after the overlay clears or the attempts run out (callers then judge).
   */
  private async healRuntimeError(attempts: number = 2): Promise<void> {
    for (let i = 0; i < attempts; i++) {
      const crashed = await this.page
        .getByText("Runtime Error", { exact: true })
        .count()
        .catch(() => 0);
      if (crashed === 0) return;
      await this.page.reload().catch(() => undefined);
      await this.page.waitForLoadState("domcontentloaded").catch(() => undefined);
      await this.page.waitForTimeout(3000);
    }
  }

  // Settled list entry (NEWFRONT-120, ISS-108-141). Reloads past stalled
  // fetches and the dev-server Runtime-Error overlay, returns to the list
  // layout (it persists across navigation on the shared stack), and waits
  // for signs of life. Separate from the shared openProjectIssues because
  // forcing the list layout would disturb sibling layout scenarios.
  async openProjectIssuesSettled(workspaceSlug: string, projectId: string): Promise<void> {
    // The list fetch stalls under shared-stack load and the list does not
    // retry on its own, so reload a few times instead of handing callers a
    // dead page. Signs of life are the header action plus rendered
    // paragraph text (issue rows and surrounding chrome). Every wait is
    // bounded: an unbounded navigation hang would eat the whole test
    // budget instead of letting the next attempt retry.
    for (let attempt = 0; attempt < 3; attempt++) {
      await this.page
        .goto(`/${workspaceSlug}/projects/${projectId}/issues`, { timeout: 45_000 })
        .catch(() => undefined);
      await this.page.waitForLoadState("domcontentloaded").catch(() => undefined);
      await this.healRuntimeError();
      // The active layout persists across navigation on the shared stack; a
      // non-list layout hides the rows list oracles assert on, so return to
      // the list layout here before judging signs of life.
      await this.ensureListLayout();
      const alive = await this.page
        .waitForFunction(
          () => {
            const main = document.querySelector("main");
            const hasText = (main?.querySelectorAll("p")?.length ?? 0) > 0;
            const hasAdd = Array.from(document.querySelectorAll("button")).some((button) =>
              (button.textContent ?? "").includes("Add work item")
            );
            return hasText && hasAdd;
          },
          { timeout: 25_000 }
        )
        .then(() => true)
        .catch(() => false);
      if (alive) return;
    }
    // Leave the last page as-is for the caller's polling to judge.
  }

  // Issue multi-select and bulk operations (NEWFRONT-120). Observed on the
  // running old app: with bulk operations unavailable, list rows render no
  // selection checkbox and no sticky bar ever appears.
  async selectionCheckboxCount(): Promise<number> {
    return this.page.getByRole("checkbox").count();
  }

  async bulkBarVisible(): Promise<boolean> {
    return (await this.page.getByText("Upgrade to One").count()) > 0;
  }

  async pressKey(key: string, shift?: boolean): Promise<void> {
    if (shift) await this.page.keyboard.down("Shift");
    try {
      await this.page.keyboard.press(key);
    } finally {
      if (shift) await this.page.keyboard.up("Shift");
    }
  }

  async reloadSawDialog(): Promise<boolean> {
    let sawDialog = false;
    const onDialog = async (dialog: import("@playwright/test").Dialog): Promise<void> => {
      sawDialog = true;
      await dialog.dismiss();
    };
    this.page.on("dialog", onDialog);
    try {
      await this.page.reload();
      await this.page.waitForLoadState("domcontentloaded");
      await this.page.waitForTimeout(2000);
    } finally {
      this.page.off("dialog", onDialog);
    }
    return sawDialog;
  }

  // Create/edit work-item modal (NEWFRONT-120). Observed on the running old
  // app: the issues header offers an "Add work item" button opening a modal
  // headed "Create new work item" with an autofocused "Title" field and
  // Discard / Save actions plus a "Create more" toggle in the footer.
  async dismissWelcomeDialog(): Promise<void> {
    const noThanks = this.page.getByRole("button", { name: "No thanks, I will explore it myself" });
    if ((await noThanks.count()) > 0) await noThanks.first().click();
  }

  async openCreateModal(): Promise<void> {
    // Heal first: on a boundary-crashed page the button resolves but stays
    // detached, hanging the click until the test times out.
    await this.healRuntimeError();
    const add = this.page.getByRole("button", { name: "Add work item" }).first();
    await add.waitFor({ timeout: 60_000 });
    await add.click({ timeout: 30_000 }).catch(async () => {
      await this.healRuntimeError();
      await this.page.getByRole("button", { name: "Add work item" }).first().click({ timeout: 30_000 });
    });
    await this.page.getByPlaceholder("Title").first().waitFor({ timeout: 60_000 });
  }

  async createModalOpen(): Promise<boolean> {
    return (await this.page.getByPlaceholder("Title").count()) > 0;
  }

  async createModalHeading(): Promise<string> {
    const texts = await this.modalScope().locator("h3").allTextContents();
    const hit = texts.map((t) => t.trim()).find((t) => t.length > 0);
    return hit ?? "";
  }

  async fillCreateTitle(title: string): Promise<void> {
    // Type with the keyboard: the title field only reports changes from
    // real keystrokes, so programmatic fills silently skip dirty tracking
    // (verified against the discard guard). Assert the value stuck.
    const field = this.page.getByPlaceholder("Title").first();
    await field.click();
    await this.page.keyboard.press("ControlOrMeta+a");
    await this.page.keyboard.press("Backspace");
    await this.page.keyboard.type(title);
    const value = await field.inputValue().catch(() => null);
    if (value !== title) throw new Error(`[parity] title field holds ${JSON.stringify(value)}.`);
  }

  async createTitleValue(): Promise<string> {
    return this.page.getByPlaceholder("Title").first().inputValue();
  }

  async createTitleError(): Promise<string> {
    const text = await this.page.locator("#name ~ span").first().textContent();
    return (text ?? "").trim();
  }

  private modalSubmit(): Locator {
    return this.page.locator("form").locator('button[type="submit"]').first();
  }

  async submitCreateModal(): Promise<void> {
    await this.modalSubmit().click();
  }

  async clickModalDiscard(): Promise<void> {
    await this.page.getByRole("button", { name: "Discard", exact: true }).first().click();
  }

  async enableCreateMore(): Promise<void> {
    await this.page.locator("button", { hasText: "Create more" }).first().click();
  }

  async modalPrimaryButtonLabel(): Promise<string> {
    return ((await this.modalSubmit().textContent()) ?? "").trim();
  }

  async gitBranchValue(): Promise<string> {
    return this.page.locator("#git_work_branch").first().inputValue();
  }

  async createTitleFocused(): Promise<boolean> {
    return this.page.evaluate(() => {
      const active = document.activeElement;
      return active instanceof HTMLInputElement && active.getAttribute("placeholder") === "Title";
    });
  }

  private modalScope(): Locator {
    return this.page.getByRole("dialog");
  }

  async modalTextContains(text: string): Promise<boolean> {
    return (await this.modalScope().getByText(text, { exact: false }).count()) > 0;
  }

  async confirmSaveDraft(): Promise<void> {
    await this.page.getByRole("button", { name: "Save to Drafts", exact: true }).last().click();
  }

  async cancelDiscardDialog(): Promise<void> {
    await this.page.getByRole("button", { name: "Cancel", exact: true }).first().click();
  }

  async discardDialogDiscard(): Promise<void> {
    const discard = this.page.getByRole("button", { name: "Discard", exact: true }).last();
    try {
      await discard.click({ timeout: 15_000 });
    } catch {
      // A stale modal overlay can keep covering the confirm while the
      // button itself stays focused and enabled; keyboard-activate it the
      // way a user would instead of failing on hit-testing.
      await discard.focus();
      await this.page.keyboard.press("Enter");
    }
  }

  async openDraftForEdit(name: string): Promise<void> {
    await this.page.getByText(name, { exact: true }).first().dblclick();
    await this.page.getByPlaceholder("Title").first().waitFor({ timeout: 30_000 });
  }

  async publishDraft(): Promise<void> {
    await this.page.getByRole("button", { name: "Publish issue", exact: true }).first().click();
  }

  /**
   * Bring a row into the rendered list. Newly created rows sort last, so
   * on a busy shared stack they hide behind group pagination: click
   * through "Load more" rows and prod the intersection auto-loader until
   * the name renders, then scroll it into view.
   */
  /** Scroll the list's own scroll container (not the main landmark). */
  private async scrollListToBottom(): Promise<void> {
    await this.page
      .evaluate(() => {
        const main = document.querySelector("main");
        if (!main) return;
        let deepest: HTMLElement | null = null;
        main.querySelectorAll("div").forEach((el) => {
          if (el.scrollHeight > el.clientHeight + 50) deepest = el;
        });
        (deepest ?? main).scrollTo(0, 999999);
      })
      .catch(() => undefined);
  }

  private async revealRowInList(issueName: string): Promise<void> {
    for (let attempt = 0; attempt < 10; attempt++) {
      const name = this.page.getByText(issueName, { exact: true });
      if ((await name.count()) > 0) {
        await name.first().scrollIntoViewIfNeeded({ timeout: 10_000 });
        return;
      }
      const more = this.page.getByText("Load more", { exact: false });
      if ((await more.count()) > 0) {
        await more
          .first()
          .click()
          .catch(() => undefined);
        await this.page.waitForTimeout(2000);
        continue;
      }
      // New rows sort last behind group pagination; scrolling the list's
      // own container trips the intersection auto-loader.
      await this.scrollListToBottom();
      await this.page.waitForTimeout(2500);
    }
    throw new Error(`[parity] row never rendered ${JSON.stringify(issueName)}.`);
  }

  async openRowMenuEntry(issueName: string, entry: string): Promise<void> {
    // Every row carries a right-click context menu with the same entries
    // as the hover-revealed ellipsis trigger (an icon-only button with no
    // accessible name), so right-click the row name and pick the entry.
    // Entries render as plain buttons once the menu opens. Retried: live
    // list re-renders on the shared stack can detach the row mid-action.
    let lastError: unknown = null;
    for (let attempt = 0; attempt < 2; attempt++) {
      try {
        await this.revealRowInList(issueName);
        const name = this.page.getByText(issueName, { exact: true }).first();
        await name.click({ button: "right", timeout: 15_000 });
        const item = this.page.getByRole("button", { name: entry, exact: true }).first();
        await item.waitFor({ timeout: 15_000 });
        await item.dispatchEvent("click");
        return;
      } catch (error) {
        lastError = error;
        await this.page.keyboard.press("Escape").catch(() => undefined);
      }
    }
    throw lastError;
  }

  async modalButtonDisabled(name: string): Promise<boolean> {
    const button = this.modalScope().getByRole("button", { name });
    if ((await button.count()) === 0) return false;
    return button.first().isDisabled();
  }

  async openParentPicker(): Promise<void> {
    await this.modalScope().getByRole("button", { name: "Add parent" }).first().click();
  }

  async searchParentInModal(query: string): Promise<void> {
    await this.page.getByPlaceholder("Type to search").first().fill(query);
  }

  async selectParentResult(issueName: string): Promise<void> {
    // Results are combobox options (identifier + name + a hover-revealed
    // new-tab link); click the name text inside the option so the option —
    // not the link — receives the click and the picker closes selected.
    const option = this.page.getByRole("option", { name: issueName }).first();
    await option.waitFor({ timeout: 30_000 });
    await option.getByText(issueName, { exact: true }).click();
  }

  async parentResultNewTabLinks(): Promise<number> {
    return this.modalScope().locator('a[target="_blank"]').count();
  }

  async removeParentInModal(issueName: string): Promise<void> {
    // The selected parent renders as a surface-2 tag (identifier + name)
    // whose only button is the icon-only remove control. Substring text
    // matching keeps working when the rendered name carries surrounding
    // whitespace that exact matching rejects.
    const tag = this.modalScope().locator("div.bg-surface-2").filter({ hasText: issueName }).last();
    // The tag holds two buttons: the disabled identifier chip first and
    // the remove control last.
    await tag.getByRole("button").last().click();
  }

  async openLabelsPicker(): Promise<void> {
    await this.modalScope().getByRole("button", { name: "Labels" }).first().click();
  }

  async createLabelInModal(name: string): Promise<void> {
    // The label picker's search box is placeholder "Search"; typing a new
    // name and pressing Enter creates the label inline, which then shows
    // as a selected chip in the modal.
    const box = this.modalScope().getByPlaceholder("Search").first();
    await box.waitFor({ state: "visible", timeout: 8000 });
    await box.fill(name);
    await box.press("Enter");
    await this.modalScope().getByText(name, { exact: true }).first().waitFor({ state: "visible", timeout: 30_000 });
  }

  async selectedLabelVisible(name: string): Promise<boolean> {
    return (await this.modalScope().getByText(name, { exact: true }).count()) > 0;
  }

  async hoverIssueRow(issueName: string): Promise<void> {
    // Let a stuck progress overlay clear first; bounded either way, so a
    // hung hover fails fast instead of eating the test budget.
    await this.page
      .waitForFunction(() => !document.documentElement.classList.contains("bprogress-busy"), null, {
        timeout: 15_000,
      })
      .catch(() => undefined);
    await this.page.getByText(issueName, { exact: true }).first().hover({ timeout: 60_000 });
  }

  async openToastViewAction(): Promise<string> {
    // The view action is a new-tab anchor, so capture the popup it opens.
    const [popup] = await Promise.all([
      this.page.waitForEvent("popup", { timeout: 30_000 }),
      this.page.getByText("View work item", { exact: true }).first().click(),
    ]);
    await popup.waitForLoadState("domcontentloaded").catch(() => undefined);
    return popup.url();
  }

  async confirmArchive(): Promise<void> {
    // Scope to the dialog: the still-open row menu behind it carries its
    // own same-named entry.
    await this.modalScope().getByRole("button", { name: "Archive", exact: true }).first().click();
  }

  async confirmDeleteIssue(): Promise<void> {
    // Scope to the dialog: the still-open row menu behind it carries its
    // own same-named entry.
    await this.modalScope().getByRole("button", { name: "Delete", exact: true }).first().click();
  }

  async modalHasPlaceholder(placeholder: string): Promise<boolean> {
    return (await this.modalScope().getByPlaceholder(placeholder).count()) > 0;
  }

  async fillModalPlaceholder(placeholder: string, text: string): Promise<void> {
    const field = this.modalScope().getByPlaceholder(placeholder).first();
    await field.waitFor({ timeout: 30_000 });
    await field.click();
    await this.page.keyboard.press("ControlOrMeta+a");
    await this.page.keyboard.press("Backspace");
    await this.page.keyboard.type(text);
  }

  async expandListRows(): Promise<boolean> {
    // Scroll first: group pagination also loads through an intersection
    // sentinel with no visible affordance.
    await this.scrollListToBottom();
    await this.page.waitForTimeout(2000);
    const more = this.page.getByText("Load more", { exact: false });
    const count = await more.count();
    for (let i = 0; i < count; i++) {
      await more
        .nth(i)
        .click()
        .catch(() => undefined);
      await this.page.waitForTimeout(1500);
    }
    return count > 0;
  }

  /** Buttons of the layout-switcher group, when the header rendered one. */
  private async layoutSwitcherButtons(): Promise<Locator[]> {
    const groups = this.page.locator("div.rounded-md.bg-layer-3").filter({ has: this.page.getByRole("button") });
    const scoped: Locator[] = [];
    for (let i = 0; i < (await groups.count()); i++) scoped.push(...(await groups.nth(i).getByRole("button").all()));
    return scoped;
  }

  /** Hover-scan `buttons` for the one whose tooltip reads `label`; clicks it. */
  private async clickButtonByTooltip(buttons: Locator[], label: string): Promise<boolean> {
    // Park the mouse first: a tooltip left open by an earlier scan would
    // otherwise read as a false appearance on the first button hovered.
    await this.page.mouse.move(4, 4).catch(() => undefined);
    await this.page.waitForTimeout(400);
    for (const button of buttons) {
      const before = await this.page.getByText(label, { exact: true }).count();
      await button.hover({ timeout: 10_000 }).catch(() => undefined);
      // Tooltips lag well past their nominal delay under load; dwell long
      // enough that a slow appearance still reads as one.
      await this.page.waitForTimeout(1000);
      const after = await this.page.getByText(label, { exact: true }).count();
      if (after > before) {
        await button.click().catch(() => undefined);
        return true;
      }
    }
    return false;
  }

  async switchIssueLayout(label: string): Promise<void> {
    // The layout switcher is icon-only; each button reveals its layout
    // through a hover tooltip. Scan the switcher group a few times (a
    // loaded run misses tooltips), then fall back once to every button on
    // the page before giving up.
    for (let attempt = 0; attempt < 3; attempt++) {
      if (await this.clickButtonByTooltip(await this.layoutSwitcherButtons(), label)) return;
    }
    if (await this.clickButtonByTooltip(await this.page.getByRole("button").all(), label)) return;
    throw new Error(`[parity] no layout switcher tooltip read ${JSON.stringify(label)}.`);
  }

  async ensureListLayout(): Promise<void> {
    // Best-effort only: a dead page or a header without the switcher
    // leaves the layout alone for the caller's polling to judge. Waits
    // briefly for the header so the first navigation already enforces
    // the list instead of only healing on a later reload.
    try {
      for (let i = 0; i < 40; i++) {
        if ((await this.page.locator("div.rounded-md.bg-layer-3").count()) > 0) break;
        await this.page.waitForTimeout(500);
      }
      for (let attempt = 0; attempt < 2; attempt++) {
        if (await this.clickButtonByTooltip(await this.layoutSwitcherButtons(), "List Layout")) return;
      }
    } catch {
      // Leave the layout alone.
    }
  }

  async toggleAdvancedGit(): Promise<void> {
    await this.page.getByRole("button", { name: "Advanced — git" }).first().click();
  }

  async fillGitBranch(branch: string): Promise<void> {
    const field = this.page.locator("#git_work_branch").first();
    await field.click();
    await this.page.keyboard.press("ControlOrMeta+a");
    await this.page.keyboard.press("Backspace");
    await this.page.keyboard.type(branch);
  }

  async gitBranchError(): Promise<string> {
    const text = await this.page.locator("#git_work_branch ~ span").first().textContent();
    return (text ?? "").trim();
  }

  async fillDescription(text: string): Promise<void> {
    // The editor's "Click to add description" hint is CSS-generated
    // placeholder text (a data-placeholder on the empty paragraph), so it
    // is not a text node to click: click the editor box itself instead,
    // exactly where the user clicks the hint, then type.
    await this.modalScope().locator('[contenteditable="true"]').first().click();
    await this.page.keyboard.type(text);
  }

  async countText(text: string): Promise<number> {
    return this.page.getByText(text, { exact: true }).count();
  }

  // Workspace drafts (NEWFRONT-120). Observed on the running old app: the
  // drafts route renders one block per draft, and the empty state offers a
  // "Create draft work item" action opening the draft variant of the modal.
  async openDraftsPage(workspaceSlug: string): Promise<void> {
    await this.page.goto(`/${workspaceSlug}/drafts`, { timeout: 60_000 }).catch(() => undefined);
    await this.page.waitForLoadState("domcontentloaded").catch(() => undefined);
    await this.healRuntimeError();
  }

  async visibleDraftNames(): Promise<string[]> {
    try {
      const texts = await this.page.getByRole("main").locator("p").allTextContents();
      return texts.map((t) => t.trim()).filter((t) => t.length > 0);
    } catch {
      return [];
    }
  }

  async openCreateDraftModal(): Promise<void> {
    // The empty state offers "Create draft work item"; once drafts exist
    // the header offers "Draft a work item". Either opens the draft modal.
    const emptyAction = this.page.getByRole("button", { name: "Create draft work item" });
    if ((await emptyAction.count()) > 0) await emptyAction.first().click();
    else await this.page.getByRole("button", { name: "Draft a work item" }).first().click();
    await this.page.getByPlaceholder("Title").first().waitFor({ timeout: 30_000 });
  }

  async draftBlockCount(): Promise<number> {
    try {
      return await this.page.locator('div[id^="issue-"]').count();
    } catch {
      return 0;
    }
  }

  async draftBlockText(name: string): Promise<string> {
    const text = await this.page.locator('div[id^="issue-"]', { hasText: name }).first().textContent();
    return (text ?? "").replace(/\s+/g, " ").trim();
  }

  async pageTextContains(text: string): Promise<boolean> {
    return (await this.page.getByText(text, { exact: false }).count()) > 0;
  }

  async deleteDraftByName(name: string): Promise<void> {
    // The block's right-click menu renders its entries as plain buttons
    // carrying the untranslated keys ("delete", …); the confirm alert
    // carries the capitalized Delete. Right-click the block center: a
    // corner click can land on the neighboring block (each block owns its
    // menu, so a miss acts on the wrong draft).
    const block = this.page.locator('div[id^="issue-"]', { hasText: name }).first();
    await block.click({ button: "right" });
    // The open menu intercepts pointer events at the item, so a real click
    // never lands; dispatch it straight to the item instead.
    const entry = this.page.getByRole("button", { name: "delete", exact: true }).first();
    await entry.waitFor({ timeout: 30_000 });
    await entry.dispatchEvent("click");
    await this.page.getByRole("button", { name: "Delete", exact: true }).last().click();
  }

  async settleDraftsPage(workspaceSlug: string): Promise<"empty" | "list"> {
    // The drafts fetch can be rate-limited on the shared stack; re-entering
    // the route remounts the view and retries it.
    const started = Date.now();
    let revisits = 0;
    for (;;) {
      if ((await this.page.getByText("Half-written work items").count()) > 0) return "empty";
      let blocks = 0;
      try {
        blocks = await this.page.locator('div[id^="issue-"]').count();
      } catch {
        blocks = 0;
      }
      if (blocks > 0) return "list";
      if (Date.now() - started > 150_000) throw new Error("[parity] drafts page never settled");
      if (Date.now() - started > 30_000 * (revisits + 1) && revisits < 4) {
        revisits += 1;
        await this.page.goto(`/${workspaceSlug}/drafts`);
        await this.page.waitForLoadState("domcontentloaded");
      }
      await this.page.waitForTimeout(2000);
    }
  }

  // Draft quick actions, continued (NEWFRONT-120, ISS-139). The block's
  // right-click menu carries the same untranslated keys as the delete
  // entry: copying opens the duplicated payload in the draft modal, moving
  // opens the move-to-project modal for the draft.
  async copyDraftByName(name: string): Promise<void> {
    const block = this.page.locator('div[id^="issue-"]', { hasText: name }).first();
    await block.click({ button: "right" });
    const entry = this.page.getByRole("button", { name: "make_a_copy", exact: true }).first();
    await entry.waitFor({ timeout: 30_000 });
    await entry.dispatchEvent("click");
  }

  async moveDraftToProject(name: string): Promise<void> {
    const block = this.page.locator('div[id^="issue-"]', { hasText: name }).first();
    await block.click({ button: "right" });
    const entry = this.page.getByRole("button", { name: "move_to_project", exact: true }).first();
    await entry.waitFor({ timeout: 30_000 });
    await entry.dispatchEvent("click");
  }

  async confirmMoveToProject(): Promise<void> {
    await this.page.getByRole("button", { name: "Add to project", exact: true }).first().click();
  }

  private modalFormScope(): Locator {
    // Scope to the dialog holding the modal form: toasts share the dialog
    // role, so an unscoped .first() lands on a toast instead of the modal.
    return this.page.getByRole("dialog").filter({ has: this.page.getByPlaceholder("Title") });
  }

  private modalProjectChip(): Locator {
    // The project chip is the modal's first nested button (an outer button
    // wrapping the clickable inner one); later nested buttons are the
    // state/assignee/date rows. Structural on purpose: the chip shows
    // whatever project the modal targeted, which on the shared stack is
    // often not the seed project.
    return this.modalFormScope()
      .getByRole("button")
      .filter({ has: this.page.getByRole("button") })
      .first()
      .getByRole("button");
  }

  async modalProjectName(): Promise<string> {
    const chip = this.modalProjectChip();
    await chip.waitFor({ timeout: 30_000 });
    return ((await chip.textContent()) ?? "").replace(/\s+/g, " ").trim();
  }

  async selectModalProject(name: string): Promise<void> {
    await this.modalProjectChip().click();
    const option = this.page.getByRole("option", { name, exact: true }).first();
    await option.waitFor({ timeout: 30_000 });
    await option.click();
    // Confirm the chip flipped; callers save right after, so a missed
    // selection must fail here instead of landing elsewhere.
    const started = Date.now();
    for (;;) {
      const current = (
        (await this.modalProjectChip()
          .textContent()
          .catch(() => null)) ?? ""
      )
        .replace(/\s+/g, " ")
        .trim();
      if (current === name) return;
      if (Date.now() - started > 15_000) throw new Error(`[parity] project chip stuck at ${JSON.stringify(current)}.`);
      await this.page.waitForTimeout(500);
    }
  }

  // Work-item preview card (NEWFRONT-120, ISS-133). Observed on the running
  // old app: hovering a calendar block pops a card whose heading carries
  // the title; the card root wraps an identifier/state row, the title, and
  // a priority-plus-date row around it. One hover reads everything: each
  // hover cycle risks a stuck progress overlay under shared-stack load,
  // so the text, icon and color come from a single pop.
  async hoverCardRead(issueName: string): Promise<{ text: string; priorityIcon: string; dateColor: string }> {
    const empty = { text: "", priorityIcon: "", dateColor: "" };
    // A failed hover propagates: only a successful hover with no card
    // (undated rows) reports empty fields.
    await this.hoverIssueRow(issueName);
    try {
      const heading = this.page.locator("h6", { hasText: issueName }).first();
      await heading.waitFor({ timeout: 15_000 });
      const read = await heading.evaluate((el) => {
        // The heading sits in a title wrapper inside the card root.
        const root = el.parentElement?.parentElement;
        const text = (root?.textContent ?? "").replace(/\s+/g, " ").trim();
        const row = root?.lastElementChild;
        const svg = row?.querySelector("svg");
        const classes = (svg?.getAttribute("class") ?? "").split(/\s+/);
        const priorityIcon = classes.filter((name) => name.startsWith("lucide-") && name !== "lucide").join(" ");
        const span = row?.querySelector("span");
        const dateColor = !span || (span.textContent ?? "").trim() === "" ? "" : getComputedStyle(span).color;
        return { text, priorityIcon, dateColor };
      });
      if (read.text.length <= issueName.length + 5) return empty;
      return read;
    } catch {
      return empty;
    } finally {
      // Close the popover so an open card never covers the next hover
      // target; a missing card leaves nothing to close.
      await this.page.keyboard.press("Escape").catch(() => undefined);
    }
  }

  // Modal keyboard contract (NEWFRONT-120, ISS-125). The modal carries an
  // explicit tab order over its fields; read it back as triples so the
  // scenario pins the order without depending on styling.
  async modalTabOrder(): Promise<string[]> {
    return this.modalScope().evaluate((scope: HTMLElement) => {
      const labelOf = (el: Element): string => {
        const labelled = el.getAttribute("placeholder") ?? el.getAttribute("id") ?? el.textContent ?? "";
        return labelled.replace(/\s+/g, " ").trim().slice(0, 40);
      };
      return Array.from(scope.querySelectorAll("[tabindex]")).map(
        (el) => `${el.tagName.toLowerCase()}#${el.getAttribute("tabindex")}:${labelOf(el)}`
      );
    });
  }

  async focusCreateTitle(): Promise<void> {
    await this.page.getByPlaceholder("Title").first().click();
  }

  // Cycle/module page entry (NEWFRONT-120, ISS-124). These pages compile on
  // first load and fetch their context over the throttled shared stack, so
  // re-enter until the Add action shows instead of trusting one load.
  private async openIssueContextPage(route: string, label: string): Promise<void> {
    const started = Date.now();
    for (;;) {
      await this.page.goto(route, { timeout: 45_000 }).catch(() => undefined);
      await this.page.waitForLoadState("domcontentloaded").catch(() => undefined);
      await this.healRuntimeError();
      const ready = await this.page
        .getByRole("button", { name: "Add work item" })
        .first()
        .waitFor({ timeout: 30_000 })
        .then(() => true)
        .catch(() => false);
      if (ready) return;
      if (Date.now() - started > 150_000) throw new Error(`[parity] ${label} page never offered its Add action.`);
    }
  }

  async openCyclePage(workspaceSlug: string, projectId: string, cycleId: string): Promise<void> {
    await this.openIssueContextPage(`/${workspaceSlug}/projects/${projectId}/cycles/${cycleId}`, "cycle");
  }

  async openModulePage(workspaceSlug: string, projectId: string, moduleId: string): Promise<void> {
    await this.openIssueContextPage(`/${workspaceSlug}/projects/${projectId}/modules/${moduleId}`, "module");
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

  // ---- Issue detail (NEWFRONT-121). ----
  // Selectors follow the detail behavior observed on the running old app:
  // the title is a textarea (placeholder "Work item title"), the sidebar
  // is a "Properties" section of label/value rows, dropdowns render a
  // listbox of options, and the description is the contenteditable above
  // the "Last edited by" line (the composer sits below it).

  private static readonly DETAIL_MS = 120_000;

  async signedIn(): Promise<boolean> {
    return (await this.page.getByPlaceholder("name@company.com").count()) === 0;
  }

  /**
   * Open one work-item detail page; ends hydrated.
   *
   * Two call shapes share this helper: browse by `IDENT-seq` with two
   * arguments (NEWFRONT-121), or by project/issue ids with three
   * (NEWFRONT-122). The arity selects the flow; both bodies below are the
   * owning area's verbatim opener.
   */
  async openIssueDetail(workspaceSlug: string, issueSeqOrProjectId: string, issueId?: string): Promise<void> {
    if (issueId === undefined) {
      await this.openIssueDetailBySeq(workspaceSlug, issueSeqOrProjectId);
      return;
    }
    await this.openIssueDetailByIds(workspaceSlug, issueSeqOrProjectId, issueId);
  }

  /** NEWFRONT-121 browse-by-seq opener. */
  private async openIssueDetailBySeq(workspaceSlug: string, issueSeq: string): Promise<void> {
    await this.page.goto(`/${workspaceSlug}/browse/${issueSeq}`);
    // Hydration can be slow on the dev-server oracle, but a missing issue
    // or a lost session must fail fast with a diagnosis, not a timeout.
    const deadline = Date.now() + WebDriver.DETAIL_MS;
    let loops = 0;
    for (;;) {
      if ((await this.titleField().count()) > 0) return;
      if (await this.seesDetailMissing()) {
        throw new Error(`[parity] detail shows the missing state for ${issueSeq} (reseeded away?).`);
      }
      if ((await this.page.getByPlaceholder("name@company.com").count()) > 0) {
        throw new Error("[parity] session lost while opening the detail page.");
      }
      loops++;
      if (Date.now() > deadline) {
        await this.titleField().waitFor({ timeout: 5000 });
        return;
      }
      // A blank page means the dev-server route module failed to fetch
      // (stack network flap); reloading usually converges.
      if (loops % 15 === 0) await this.page.reload().catch(() => {});
      else await this.page.waitForTimeout(2000);
    }
  }

  /** NEWFRONT-122 project/issue-ids opener. */
  private async openIssueDetailByIds(workspaceSlug: string, projectId: string, issueId: string): Promise<void> {
    // Re-opening the same issue must refetch: a same-URL goto is a router
    // no-op, so reload when already sitting on the browse view.
    if (this.page.url().includes("/browse/")) await this.page.reload();
    else {
      await this.page.goto(`/${workspaceSlug}/projects/${projectId}/issues/${issueId}`);
      await this.page.waitForLoadState("domcontentloaded");
    }
    try {
      await this.page.waitForURL(/\/browse\//, { timeout: 30_000 });
    } catch {
      throw new Error(`[parity] detail for ${issueId} never reached the browse view (current: ${this.page.url()}).`);
    }
    // The oracle dev server hydrates slowly under shared-stack load. This
    // budget favors failing fast so a retry reloads cleanly; the
    // missing-issue empty state fails fast instead of waiting it out.
    const deadline = Date.now() + 75_000;
    for (;;) {
      if ((await this.page.getByText(/does not exist|went wrong/i).count()) > 0)
        throw new Error(`[parity] detail for ${issueId} rendered an error state (${this.page.url()}).`);
      try {
        await this.activityHeading().waitFor({ timeout: Math.max(1_000, deadline - Date.now()) });
        return;
      } catch {
        if (Date.now() >= deadline)
          throw new Error(`[parity] Activity never rendered for ${issueId} (current: ${this.page.url()}).`);
      }
    }
  }

  async openReadOnlyIssueDetail(workspaceSlug: string, issueSeq: string): Promise<void> {
    await this.page.goto(`/${workspaceSlug}/browse/${issueSeq}`);
    // No title field renders on read-only detail (static title instead),
    // so the identifier plus a hydrated sidebar row decide. Reload
    // through blank dev-server boots like the opener.
    const deadline = Date.now() + WebDriver.DETAIL_MS;
    let loops = 0;
    for (;;) {
      if ((await this.issueDetailIdentifier()) === issueSeq) {
        if ((await this.sidebarProperty("State")) !== null) return;
      }
      if ((await this.page.getByPlaceholder("name@company.com").count()) > 0) {
        throw new Error("[parity] session lost while opening the read-only detail page.");
      }
      loops++;
      if (Date.now() > deadline) throw new Error(`[parity] read-only detail did not hydrate for ${issueSeq}.`);
      if (loops % 15 === 0) await this.page.reload().catch(() => {});
      else await this.page.waitForTimeout(2000);
    }
  }

  private titleField(): Locator {
    return this.page.getByPlaceholder("Work item title").first();
  }

  async issueDetailTitle(): Promise<string | null> {
    if ((await this.titleField().count()) === 0) return null;
    return await this.titleField().inputValue();
  }

  async issueDetailIdentifier(): Promise<string | null> {
    const id = this.page.getByText(/^[A-Z0-9]{2,}-\d+$/).first();
    if ((await id.count()) === 0) return null;
    return (await id.innerText()).trim();
  }

  async editIssueTitle(name: string): Promise<void> {
    await this.titleField().fill(name, { timeout: WebDriver.OPEN_MS });
    // Blur out of the title so the debounced autosave fires.
    const id = this.page.getByText(/^[A-Z0-9]{2,}-\d+$/).first();
    if ((await id.count()) > 0) await id.click({ timeout: WebDriver.OPEN_MS });
    else await this.titleField().press("Tab", { timeout: WebDriver.OPEN_MS });
  }

  async saveIndicator(): Promise<string | null> {
    const saving = this.page.getByText(/^Saving…$/).first();
    if ((await saving.count()) > 0) return "Saving…";
    const saved = this.page.getByText(/^Saved$/).first();
    if ((await saved.count()) > 0) return "Saved";
    return null;
  }

  /** The description editor: contenteditable above the "Last edited by" line. */
  private async descriptionEditor(): Promise<Locator | null> {
    const editors = this.page.locator("[contenteditable='true']");
    const count = await editors.count();
    if (count === 0) return null;
    if (count === 1) return editors.first();
    const marker = this.page.getByText(/Last edited by/).first();
    if ((await marker.count()) === 0) return editors.first();
    const markerBox = await marker.boundingBox();
    if (!markerBox) return editors.first();
    for (let i = 0; i < count; i++) {
      const box = await editors.nth(i).boundingBox();
      if (box && box.y + box.height <= markerBox.y) return editors.nth(i);
    }
    return null;
  }

  async descriptionText(): Promise<string | null> {
    const editor = await this.descriptionEditor();
    if (!editor) return null;
    return ((await editor.innerText()) ?? "").trim();
  }

  async setDescription(text: string): Promise<void> {
    const placeholder = this.page.getByText("Click to add description").first();
    if ((await placeholder.count()) > 0) await placeholder.click({ timeout: WebDriver.OPEN_MS });
    const editor = await this.descriptionEditor();
    if (!editor) throw new Error("[parity] no description editor on the detail page.");
    await editor.fill(text, { timeout: WebDriver.OPEN_MS });
    const id = this.page.getByText(/^[A-Z0-9]{2,}-\d+$/).first();
    if ((await id.count()) > 0) await id.click({ timeout: WebDriver.OPEN_MS });
    else await this.page.keyboard.press("Escape");
  }

  /** The label/value row for a sidebar property, found by walking up. */
  private async propertyRow(label: string): Promise<Locator | null> {
    // Labels repeat across the page (the sidebar "Modules" label and a nav
    // link share the text), so try every exact match until one climbs to a
    // row; rows with an empty value carry only the label text plus their
    // dropdown trigger, which the length check below would reject.
    const matches = this.page.getByText(label, { exact: true });
    const count = await matches.count();
    for (let m = 0; m < count; m++) {
      let node = matches.nth(m).locator("xpath=parent::*");
      for (let i = 0; i < 4; i++) {
        const text = ((await node.innerText().catch(() => "")) ?? "").trim();
        if (text.length < 300) {
          if (text.length > label.length + 1) return node;
          if ((await node.getByRole("button").count()) > 0) return node;
        }
        node = node.locator("xpath=parent::*");
      }
    }
    return null;
  }

  async sidebarProperty(label: string): Promise<string | null> {
    const row = await this.propertyRow(label);
    if (!row) return null;
    const text = ((await row.innerText()) ?? "").trim();
    return text.replace(label, "").trim() || null;
  }

  async sidebarRowPresent(label: string): Promise<boolean> {
    return (await this.propertyRow(label)) !== null;
  }

  async sidebarRowHasControl(label: string): Promise<boolean> {
    const row = await this.propertyRow(label);
    if (!row) return false;
    return (await row.getByRole("button").count()) > 0;
  }

  private async pickFromProperty(label: string, name: string): Promise<void> {
    const row = await this.propertyRow(label);
    if (!row) throw new Error(`[parity] no sidebar property ${JSON.stringify(label)}.`);
    await row.getByRole("button").first().click({ timeout: WebDriver.OPEN_MS });
    await this.page.getByRole("option", { name }).click({ timeout: WebDriver.OPEN_MS });
  }

  async pickState(name: string): Promise<void> {
    await this.pickFromProperty("State", name);
  }

  async pickPriority(name: string): Promise<void> {
    await this.pickFromProperty("Priority", name);
  }

  async pickAssignee(displayName: string): Promise<void> {
    const row = await this.propertyRow("Assignees");
    if (!row) throw new Error("[parity] no sidebar Assignees row.");
    await row.getByRole("button").first().click({ timeout: WebDriver.OPEN_MS });
    await this.page.getByRole("option", { name: displayName }).click({ timeout: WebDriver.OPEN_MS });
  }

  private async openRunsOn(): Promise<Locator> {
    const row = await this.propertyRow("Runs on");
    if (!row) throw new Error("[parity] no sidebar Runs-on row.");
    await row.getByRole("button").first().click({ timeout: WebDriver.OPEN_MS });
    const options = this.page.getByRole("option");
    await options.first().waitFor({ timeout: WebDriver.OPEN_MS });
    return options;
  }

  async runsOnOptions(): Promise<string[]> {
    const options = await this.openRunsOn();
    const names = (await options.allTextContents()).map((t) => t.trim()).filter((t) => t.length > 0);
    await this.page.keyboard.press("Escape");
    return names;
  }

  async pickRunsOn(name: string): Promise<void> {
    // A click swallowed by a re-render leaves the dropdown open with no
    // selection, so retry until the options detach (a select closes them).
    for (let attempt = 0; ; attempt++) {
      const options = await this.openRunsOn();
      await options.filter({ hasText: name }).first().click({ timeout: WebDriver.OPEN_MS });
      try {
        await this.page.getByRole("option").first().waitFor({ state: "detached", timeout: 5000 });
        return;
      } catch {
        if (attempt >= 2) throw new Error(`[parity] runs-on pick ${JSON.stringify(name)} did not land.`);
        await this.page.keyboard.press("Escape").catch(() => {});
      }
    }
  }

  /** Open the date picker popover for the named sidebar row. */
  private async openDatePicker(label: string): Promise<Locator> {
    const row = await this.propertyRow(label);
    if (!row) throw new Error(`[parity] no sidebar property ${JSON.stringify(label)}.`);
    await row.getByRole("button").first().click({ timeout: WebDriver.OPEN_MS });
    const grid = this.page.getByRole("grid");
    await grid.first().waitFor({ timeout: WebDriver.OPEN_MS });
    return row;
  }

  /** Day cells show the bare day number; adjacent-month days never read 15–25. */
  private dayCell(day: string): Locator {
    return this.page
      .getByRole("gridcell")
      .filter({ hasText: new RegExp(`^${day}$`) })
      .first();
  }

  async pickDate(label: string, day: string): Promise<void> {
    await this.openDatePicker(label);
    await this.dayCell(day).click({ timeout: WebDriver.OPEN_MS });
  }

  async calendarDayDisabled(day: string): Promise<boolean> {
    const cell = this.dayCell(day);
    if ((await cell.count()) === 0) return false;
    const disabled = await cell.getAttribute("aria-disabled").catch(() => null);
    if (disabled !== null) return disabled === "true";
    return ((await cell.getAttribute("class").catch(() => "")) ?? "").includes("disabled");
  }

  async clearDate(label: string): Promise<void> {
    const row = await this.propertyRow(label);
    if (!row) throw new Error(`[parity] no sidebar property ${JSON.stringify(label)}.`);
    // The clear X hides inside the nested value buttons until the group
    // wrapper is hovered; clicking the revealed icon clears the date.
    const group = row.locator("div.group").first();
    await group.hover();
    const icon = group.locator("svg[class*='group-hover']").first();
    await icon.waitFor({ timeout: WebDriver.OPEN_MS });
    await icon.click({ timeout: WebDriver.OPEN_MS });
  }

  async pickCycle(name: string): Promise<void> {
    await this.pickFromProperty("Cycle", name);
  }

  async clearCycle(): Promise<void> {
    await this.pickFromProperty("Cycle", "No cycle");
  }

  async toggleModule(name: string): Promise<void> {
    await this.pickFromProperty("Modules", name);
    // The multi-select menu stays open after a toggle; dismiss it so later
    // reads see the settled row rather than the open menu.
    await this.page.keyboard.press("Escape");
  }

  async setParentByName(name: string): Promise<void> {
    const row = await this.propertyRow("Parent");
    if (!row) throw new Error("[parity] no sidebar Parent row.");
    await row.getByRole("button", { name: /Add parent work item/ }).click({ timeout: WebDriver.OPEN_MS });
    const dialog = this.appDialogs();
    await dialog.first().waitFor({ state: "attached", timeout: WebDriver.OPEN_MS });
    await dialog.locator("input").first().fill(name, { timeout: WebDriver.OPEN_MS });
    const option = dialog.getByRole("option", { name: new RegExp(name.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")) });
    await option.first().click({ timeout: WebDriver.OPEN_MS });
  }

  /** The banner pill: bordered container holding an identifier link that is not the child's. */
  private async bannerPill(childSeq: string): Promise<Locator | null> {
    // Identifier links also appear in the sidebar (right column) and in
    // widgets; the banner pill sits in the main column (left), so collect
    // small containers per link and keep the leftmost one.
    const links = this.page.getByRole("link", { name: /[A-Z0-9]{2,}-\d+/ });
    const count = await links.count();
    let best: { node: Locator; x: number } | null = null;
    for (let i = 0; i < count; i++) {
      const text = (
        (await links
          .nth(i)
          .innerText()
          .catch(() => "")) ?? ""
      ).trim();
      // The banner link wraps the identifier plus the parent name; skip the
      // child's own link by comparing the embedded identifier.
      const match = /([A-Z0-9]{2,}-\d+)/.exec(text);
      if (!match || match[1] === childSeq) continue;
      let node = links.nth(i).locator("xpath=parent::*");
      for (let level = 0; level < 5; level++) {
        const inner = ((await node.innerText().catch(() => "")) ?? "").trim();
        if (inner.length >= 200) break;
        const up = node.locator("xpath=parent::*");
        const upText = ((await up.innerText().catch(() => "")) ?? "").trim();
        if (upText.length >= 200) break;
        node = up;
      }
      const inner = ((await node.innerText().catch(() => "")) ?? "").trim();
      if (inner.length >= 200 || inner === "") continue;
      const box = await node.boundingBox().catch(() => null);
      const x = box ? box.x : Number.MAX_SAFE_INTEGER;
      if (!best || x < best.x) best = { node, x };
    }
    return best ? best.node : null;
  }

  async parentBanner(childSeq: string): Promise<string | null> {
    const pill = await this.bannerPill(childSeq);
    if (!pill) return null;
    return ((await pill.innerText().catch(() => "")) ?? "").trim() || null;
  }

  /** Open the banner ellipsis menu; ends with its items visible. */
  private async openBannerMenu(childSeq: string): Promise<void> {
    // The banner re-renders after navigation, so poll for the pill first.
    let pill: Locator | null = null;
    const deadline = Date.now() + WebDriver.DETAIL_MS;
    while (!pill && Date.now() < deadline) {
      pill = await this.bannerPill(childSeq);
      if (!pill) await this.page.waitForTimeout(2000);
    }
    if (!pill) throw new Error("[parity] no parent banner pill.");
    await pill
      .locator('button[aria-haspopup="menu"], button[aria-haspopup="true"]')
      .first()
      .click({ timeout: WebDriver.OPEN_MS });
    await this.page.getByRole("menuitem").first().waitFor({ timeout: WebDriver.OPEN_MS });
  }

  async bannerMenuNames(childSeq: string): Promise<string[]> {
    // The menu lists sibling work items above the remove item; read all.
    await this.openBannerMenu(childSeq);
    const items = this.page.getByRole("menuitem");
    return (await items.allTextContents()).map((t) => t.trim()).filter((t) => t.length > 0);
  }

  async removeParent(): Promise<void> {
    const childSeq = (await this.issueDetailIdentifier()) ?? "";
    await this.openBannerMenu(childSeq);
    await this.page
      .getByRole("menuitem", { name: /Remove parent work item/i })
      .first()
      .click({ timeout: WebDriver.OPEN_MS });
  }

  async openParentFromBanner(): Promise<void> {
    const childSeq = (await this.issueDetailIdentifier()) ?? "";
    const pill = await this.bannerPill(childSeq);
    if (!pill) throw new Error("[parity] no parent banner pill.");
    await pill.getByRole("link").first().click({ timeout: WebDriver.OPEN_MS });
    await this.titleField().waitFor({ timeout: WebDriver.DETAIL_MS });
  }

  async addLabel(name: string): Promise<void> {
    const row = await this.propertyRow("Labels");
    if (!row) throw new Error("[parity] no sidebar Labels row.");
    await row.click({ timeout: WebDriver.OPEN_MS });
    const combo = this.page.getByRole("combobox");
    await combo.first().waitFor({ timeout: WebDriver.OPEN_MS });
    await combo.first().fill(name, { timeout: WebDriver.OPEN_MS });
    // Selecting the filtered option assigns; Enter creates it when absent.
    const option = this.page.getByRole("option", { name });
    if ((await option.count()) > 0) await option.first().click({ timeout: WebDriver.OPEN_MS });
    else await combo.first().press("Enter", { timeout: WebDriver.OPEN_MS });
    await this.page.keyboard.press("Escape");
  }

  async removeLabel(name: string): Promise<void> {
    const row = await this.propertyRow("Labels");
    if (!row) throw new Error("[parity] no sidebar Labels row.");
    // The chip button removes the label on click; make sure the picker is
    // closed first so its option list cannot intercept the click.
    await this.page.keyboard.press("Escape");
    await this.page
      .getByRole("combobox")
      .waitFor({ state: "detached", timeout: WebDriver.OPEN_MS })
      .catch(() => {});
    const chip = row.getByRole("button", {
      name: new RegExp(name.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")),
    });
    if ((await chip.count()) === 0) throw new Error(`[parity] no label chip ${JSON.stringify(name)}.`);
    await chip.first().click({ timeout: WebDriver.OPEN_MS });
  }

  async copyIssueLink(): Promise<void> {
    // The header copy control is an unnamed icon button beside the
    // subscribe toggle whose svg is not a lucide icon (same shape as the
    // peek copy control).
    let bar = this.page.getByRole("button", { name: /^(Subscribe|Unsubscribe)$/ }).first();
    await bar.waitFor({ timeout: WebDriver.OPEN_MS });
    let found = false;
    for (let i = 0; i < 8; i++) {
      const triggers = bar.locator('button[aria-haspopup="menu"], button[aria-haspopup="true"]');
      if ((await triggers.count()) > 0) {
        found = true;
        break;
      }
      bar = bar.locator("xpath=parent::*");
    }
    if (!found) throw new Error("[parity] no detail header bar.");
    const candidates = bar.locator("button:not([aria-haspopup]):has(svg:not([class*='lucide']))");
    const count = await candidates.count();
    for (let i = 0; i < count; i++) {
      const candidate = candidates.nth(i);
      const nested = await candidate
        .evaluate((node) => node.closest('[aria-haspopup="listbox"]') !== null)
        .catch(() => true);
      if (!nested) {
        await candidate.click({ timeout: WebDriver.OPEN_MS });
        return;
      }
    }
    throw new Error("[parity] no detail copy-link control.");
  }

  /**
   * App modal dialogs, excluding toasts: both render role=dialog, but
   * toasts live under the Notifications live region.
   */
  private appDialogs(): Locator {
    return this.page.locator('xpath=//*[@role="dialog" and not(ancestor::*[@aria-label="Notifications"])]');
  }

  async lastToast(): Promise<string | null> {
    // Toasts render as dialogs inside the Notifications region (the
    // viewport's own alert nodes only cover high-priority toasts).
    const toasts = this.page.locator(
      'xpath=//*[@aria-label="Notifications"]//*[@role="dialog" or @role="alertdialog"]'
    );
    if ((await toasts.count()) > 0) {
      // Newest first: the stack prepends, so .last() is the stale toast when
      // two overlap (e.g. Comment & Run's success + dispatch-error pair).
      const text = (
        (await toasts
          .first()
          .innerText()
          .catch(() => "")) ?? ""
      ).trim();
      if (text !== "") return text;
    }
    const status = this.page.getByRole("status");
    if ((await status.count()) > 0) {
      const text = ((await status.first().innerText()) ?? "").trim();
      if (text !== "") return text;
    }
    const alert = this.page.getByRole("alert");
    if ((await alert.count()) === 0) return null;
    const text = ((await alert.first().innerText()) ?? "").trim();
    return text === "" ? null : text;
  }

  async readClipboard(): Promise<string> {
    await this.page.context().grantPermissions(["clipboard-read", "clipboard-write"]);
    return await this.page.evaluate(() => navigator.clipboard.readText());
  }

  async subscribeToggle(): Promise<string | null> {
    const toggle = this.page.getByRole("button", { name: /^(Subscribe|Unsubscribe)$/ }).first();
    if ((await toggle.count()) === 0) return null;
    return ((await toggle.innerText()) ?? "").trim() || null;
  }

  async clickSubscribeToggle(): Promise<void> {
    await this.page
      .getByRole("button", { name: /^(Subscribe|Unsubscribe)$/ })
      .first()
      .click({ timeout: WebDriver.OPEN_MS });
  }

  private async openQuickActions(): Promise<Locator> {
    // The overflow trigger is an icon-only menu button in the detail
    // header bar (the row holding the breadcrumb and the subscribe
    // toggle). Walk up from the toggle to that bar, then open the popup
    // button inside it. Archived detail renders no subscribe toggle, so
    // anchor on the header identifier there instead.
    let bar = this.page.getByRole("button", { name: /^(Subscribe|Unsubscribe)$/ }).first();
    if ((await bar.count()) === 0) bar = this.page.getByText(/^[A-Z0-9]{2,}-\d+$/).first();
    for (let i = 0; i < 8; i++) {
      bar = bar.locator("xpath=parent::*");
      const triggers = bar.locator('button[aria-haspopup="menu"], button[aria-haspopup="true"]');
      if ((await triggers.count()) > 0) {
        await triggers.first().click({ timeout: WebDriver.OPEN_MS });
        break;
      }
    }
    const items = this.page.getByRole("menuitem");
    await items.first().waitFor({ timeout: WebDriver.DETAIL_MS });
    return items;
  }

  async quickActionNames(): Promise<string[]> {
    const items = await this.openQuickActions();
    return (await items.allTextContents()).map((t) => t.trim()).filter((t) => t.length > 0);
  }

  async clickQuickAction(name: string): Promise<void> {
    // A click swallowed by a re-render leaves the menu open with no
    // effect, so retry until the menu closes (every item closes it).
    for (let attempt = 0; ; attempt++) {
      const items = await this.openQuickActions();
      await items.filter({ hasText: name }).first().click({ timeout: WebDriver.OPEN_MS });
      try {
        await this.page.getByRole("menuitem").first().waitFor({ state: "detached", timeout: 5000 });
        return;
      } catch {
        if (attempt >= 2) throw new Error(`[parity] quick action ${JSON.stringify(name)} did not land.`);
        await this.page.keyboard.press("Escape").catch(() => {});
      }
    }
  }

  async quickActionDisabled(name: string): Promise<boolean> {
    const items = await this.openQuickActions();
    const disabled = await items.filter({ hasText: name }).first().isDisabled();
    await this.page.keyboard.press("Escape").catch(() => {});
    return disabled;
  }

  async openDescriptionHistory(): Promise<void> {
    // The "Last edited by" line is a history menu button; under contention
    // the first click can be swallowed by a re-render, so retry until the
    // version items show. Scope to visible items: re-renders can leave
    // hidden duplicate menu snapshots that .first() would otherwise hit.
    const trigger = this.page.getByRole("button", { name: /Last edited by/ }).first();
    const visibleItems = this.page.locator('[role="menuitem"]:visible');
    for (let attempt = 0; ; attempt++) {
      await trigger.click({ timeout: WebDriver.OPEN_MS });
      try {
        await visibleItems.first().waitFor({ timeout: 5000 });
        return;
      } catch {
        if (attempt >= 2) throw new Error("[parity] description history menu did not open.");
      }
    }
  }

  async historyVersionNames(): Promise<string[]> {
    // Visible items only: hidden duplicate menu snapshots would otherwise
    // prepend stale names that no clickable item matches.
    const items = this.page.locator('[role="menuitem"]:visible');
    return (await items.allTextContents()).map((t) => t.trim()).filter((t) => t.length > 0);
  }

  async restoreHistoryVersion(name: string): Promise<void> {
    // The menu can render hidden duplicate snapshots, so pick the first
    // visible item matching the name and fall back to a forced click when
    // a hidden overlay intercepts. Never scope through [role="menu"]: the
    // container role is unreliable across re-renders. Match time-blind:
    // item labels carry relative timestamps ("less than a minute ago")
    // that drift between the names read and the restore click.
    const key = (text: string): string =>
      text
        .replace(
          /(less than a minute ago|\d+\s+(second|minute|hour|day|week|month|year)s?\s+ago|just now|yesterday)/gi,
          ""
        )
        .replace(/\s+/g, "")
        .toLowerCase();
    const want = key(name);
    for (let attempt = 0; ; attempt++) {
      const candidates = this.page.locator('[role="menuitem"]:visible');
      const count = await candidates.count();
      let targetIndex = -1;
      for (let i = 0; i < count; i++) {
        const text = ((await candidates.nth(i).textContent()) ?? "").trim();
        if (want.length > 0 && key(text).includes(want)) {
          targetIndex = i;
          break;
        }
      }
      if (targetIndex >= 0) {
        const target = candidates.nth(targetIndex);
        try {
          await target.click({ timeout: 10_000 });
        } catch {
          await target.click({ timeout: 10_000, force: true });
        }
        break;
      }
      if (attempt >= 3) throw new Error(`[parity] history version ${JSON.stringify(name)} not visible.`);
      await this.openDescriptionHistory();
    }
    // The headlessui dialog wrapper is an in-flow zero-height node (the
    // visible panel lives in fixed children), so never wait on the dialog
    // container itself: drive the Restore button and await its detach.
    const restore = this.appDialogs().last().getByRole("button", { name: "Restore" });
    await restore.waitFor({ timeout: WebDriver.OPEN_MS });
    try {
      await restore.click({ timeout: WebDriver.OPEN_MS });
    } catch {
      await restore.click({ timeout: WebDriver.OPEN_MS, force: true });
    }
    await restore.waitFor({ state: "detached", timeout: WebDriver.OPEN_MS });
  }

  async openLegacyIssueRoute(workspaceSlug: string, projectId: string, issueId: string): Promise<void> {
    await this.page.goto(`/${workspaceSlug}/projects/${projectId}/issues/${issueId}`);
    await this.page.waitForURL(/\/browse\//, { timeout: WebDriver.DETAIL_MS });
  }

  async seesDetailMissing(): Promise<boolean> {
    return (await this.page.getByText(/does not exist/i).count()) > 0;
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
    // Every interaction below is explicitly bounded: the suite config
    // leaves Playwright's action timeout at its unbounded default, so a
    // bare click would hang to the test timeout instead of failing
    // honestly (first seen as a 600s notices hang on a loaded host).
    //
    // Type until stable: burst typing can be followed ~1-2s later by a
    // React commit that resets the composer from a stale form value,
    // wiping the draft after it visibly landed (intermittent, ~50/50;
    // slow human keystrokes commit between chars and never hit it). A
    // retype after the wipe lands on settled state, so bounded retries
    // converge while a single attempt would flake.
    const editor = this.composerEditor();
    for (let attempt = 1; attempt <= 3; attempt += 1) {
      await editor.click({ timeout: 30_000 });
      if (attempt > 1) {
        await editor.press(`${process.platform === "darwin" ? "Meta" : "Control"}+a`, { timeout: 30_000 });
      }
      await editor.pressSequentially(text, { timeout: 30_000 });
      await this.page.waitForTimeout(2_500);
      const draft = await expect
        .poll(() => this.composerDraftText(), { timeout: 5_000 })
        .toContain(text)
        .then(
          () => true,
          () => false
        );
      if (draft) return;
    }
    throw new Error(`[parity] composer draft never held ${JSON.stringify(text)} after 3 attempts`);
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
    await editor.click({ timeout: 30_000 });
    await editor.press(`${process.platform === "darwin" ? "Meta" : "Control"}+v`, { timeout: 30_000 });
  }

  async composerDraftText(): Promise<string> {
    const raw = await this.composerEditor().innerText({ timeout: 30_000 });
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
    await editor.focus({ timeout: 30_000 });
    await this.page.keyboard.press("Enter");
  }

  async composerPressShiftEnter(): Promise<void> {
    const editor = this.composerEditor();
    await editor.focus({ timeout: 30_000 });
    await this.page.keyboard.press("Shift+Enter");
  }

  async composerAttachFile(path: string): Promise<void> {
    const page = this.page;
    const attachButton = this.composerBox().locator('button:has(svg[class*="image"])');
    const [chooser] = await Promise.all([
      page.waitForEvent("filechooser", { timeout: 30_000 }),
      attachButton.click({ timeout: 30_000 }),
    ]);
    await chooser.setFiles(path, { timeout: 30_000 });
  }

  async composerVisibleCommentTexts(): Promise<string[]> {
    const cards = this.page.locator('div[id^="comment-"]');
    const count = await cards.count();
    const bodies: string[] = [];
    for (let index = 0; index < count; index += 1) {
      const body = cards.nth(index).locator('[contenteditable="false"]').first();
      if ((await body.count()) === 0) continue;
      bodies.push(((await body.innerText({ timeout: 30_000 })) ?? "").trim());
    }
    return bodies;
  }

  async composerOpenCommentMenu(text: string): Promise<void> {
    const card = this.commentCard(text);
    await card.scrollIntoViewIfNeeded({ timeout: 30_000 });
    const button = card.locator("[data-main-menu] > button");
    // Anchored projects (the seeded stack is one) render a small access
    // icon over the menu button's center, so a center click is
    // intercepted; a person clicks the visible corner instead. The point
    // is computed from the live box, so it tracks the rendered size.
    const box = await button.boundingBox({ timeout: 30_000 });
    if (box === null) throw new Error("[parity] comment menu button has no bounding box");
    await button.click({ timeout: 30_000, position: { x: box.width - 4, y: 4 } });
  }

  async composerMenuClick(item: string): Promise<void> {
    await this.page.getByRole("menuitem", { name: item }).click({ timeout: 30_000 });
  }

  async composerEditType(text: string): Promise<void> {
    const editor = this.commentEditEditor();
    await editor.click({ timeout: 30_000 });
    await editor.press(`${process.platform === "darwin" ? "Meta" : "Control"}+a`, { timeout: 30_000 });
    await editor.pressSequentially(text, { timeout: 30_000 });
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
    await editor.focus({ timeout: 30_000 });
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
        .innerText({ timeout: 30_000 })
        .catch(() => "")) ?? ""
    ).trim();
    const timeSpan = card.locator('span[tabindex="0"]').first();
    const time = ((await timeSpan.innerText({ timeout: 30_000 }).catch(() => "")) ?? "").trim();
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
            .innerText({ timeout: 30_000 })
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
    await card.scrollIntoViewIfNeeded({ timeout: 30_000 });
    return card.locator('[contenteditable="false"] img').count();
  }

  async composerVisibleNotices(): Promise<{ message: string; kind: "success" | "error" | "unknown" }[]> {
    const dialogs = this.page.locator('div[aria-label="Notifications"] div[role="dialog"]');
    const count = await dialogs.count();
    const notices: { message: string; kind: "success" | "error" | "unknown" }[] = [];
    for (let index = 0; index < count; index += 1) {
      const dialog = dialogs.nth(index);
      const message = ((await dialog.innerText({ timeout: 30_000 }).catch(() => "")) ?? "").trim();
      if (message.length === 0) continue;
      let kind: "success" | "error" | "unknown" = "unknown";
      if ((await dialog.locator('[class*="bg-success"]').count()) > 0) kind = "success";
      else if ((await dialog.locator('[class*="bg-danger"], [class*="bg-error"]').count()) > 0) kind = "error";
      notices.push({ message, kind });
    }
    return notices;
  }
  // ---- Peek panel (NEWFRONT-121). ----
  // Selectors follow the peek behavior observed on the running old app: the
  // panel renders inside #full-screen-portal; the header is icon-only
  // (a move-right button closes, a move-diagonal link opens the full page,
  // a listbox button switches the layout, an icon button copies the link);
  // the body reuses the detail title field, action row, Properties section
  // and comment composer. Deep-links use ?peekIssueId (peekProjectId is
  // omitted when it equals the route project).

  private peekPortal(): Locator {
    return this.page.locator("#full-screen-portal");
  }

  async openPeek(workspaceSlug: string, projectId: string, issueId: string): Promise<void> {
    await this.page.goto(`/${workspaceSlug}/projects/${projectId}/issues?peekIssueId=${issueId}`);
    const deadline = Date.now() + WebDriver.DETAIL_MS;
    let loops = 0;
    for (;;) {
      if ((await this.peekTitleField().count()) > 0) return;
      if ((await this.peekErrorTitle()) !== null) return;
      if ((await this.page.getByPlaceholder("name@company.com").count()) > 0) {
        throw new Error("[parity] session lost while opening the peek.");
      }
      loops++;
      if (Date.now() > deadline) {
        await this.peekTitleField().waitFor({ timeout: 5000 });
        return;
      }
      if (loops % 15 === 0) await this.page.reload().catch(() => {});
      else await this.page.waitForTimeout(2000);
    }
  }

  async peekOpen(): Promise<boolean> {
    // The portal shell stays mounted with no children once the peek
    // closes; while loading, the skeleton already renders children but
    // carries no text yet, so children (not text) decide.
    const portal = this.peekPortal();
    if ((await portal.count()) === 0) return false;
    if ((await portal.locator("xpath=./*").count()) > 0) return true;
    return ((await portal.innerText().catch(() => "")) ?? "").trim().length > 0;
  }

  private peekTitleField(): Locator {
    return this.peekPortal().getByPlaceholder("Work item title").first();
  }

  async peekTitle(): Promise<string | null> {
    if ((await this.peekTitleField().count()) === 0) return null;
    return await this.peekTitleField().inputValue();
  }

  async peekIdentifier(): Promise<string | null> {
    const id = this.peekPortal()
      .getByText(/^[A-Z0-9]{2,}-\d+$/)
      .first();
    if ((await id.count()) === 0) return null;
    return ((await id.innerText()) ?? "").trim();
  }

  async closePeek(): Promise<void> {
    await this.peekPortal().locator("button:has(svg.lucide-move-right)").first().click({ timeout: WebDriver.OPEN_MS });
  }

  async peekCloseVisible(): Promise<boolean> {
    return (await this.peekPortal().locator("button:has(svg.lucide-move-right)").count()) > 0;
  }

  async setPeekMode(mode: string): Promise<void> {
    const portal = this.peekPortal();
    await portal.locator('button[aria-haspopup="listbox"]').first().click({ timeout: WebDriver.OPEN_MS });
    await this.page.getByRole("option", { name: mode }).first().click({ timeout: WebDriver.OPEN_MS });
  }

  async peekPanelBox(): Promise<{ x: number; y: number; width: number; height: number } | null> {
    const panel = this.peekPortal().locator("xpath=./*").first();
    if ((await panel.count()) === 0) return null;
    const box = await panel.boundingBox().catch(() => null);
    if (!box) return null;
    return { x: box.x, y: box.y, width: box.width, height: box.height };
  }

  /** Right header group: smallest ancestor of the subscribe toggle holding the menu trigger. */
  private async peekHeaderActions(): Promise<Locator> {
    const portal = this.peekPortal();
    const toggle = portal.getByRole("button", { name: /^(Subscribe|Unsubscribe)$/ }).first();
    await toggle.waitFor({ timeout: WebDriver.OPEN_MS });
    let node = toggle.locator("xpath=parent::*");
    for (let i = 0; i < 6; i++) {
      const menus = node.locator('button[aria-haspopup="menu"], button[aria-haspopup="true"]');
      if ((await menus.count()) > 0) return node;
      node = node.locator("xpath=parent::*");
    }
    return portal;
  }

  async copyPeekLink(): Promise<void> {
    // The copy control is the unnamed icon button beside the subscribe
    // toggle whose svg is not a lucide icon. The layout toggle nests an
    // inner plain button holding the mode icon, which matches the same
    // shape, so skip any candidate inside the listbox toggle.
    const group = await this.peekHeaderActions();
    const candidates = group.locator("button:not([aria-haspopup]):has(svg:not([class*='lucide']))");
    const count = await candidates.count();
    for (let i = 0; i < count; i++) {
      const candidate = candidates.nth(i);
      const nested = await candidate
        .evaluate((node) => node.closest('[aria-haspopup="listbox"]') !== null)
        .catch(() => true);
      if (!nested) {
        await candidate.click({ timeout: WebDriver.OPEN_MS });
        return;
      }
    }
    throw new Error("[parity] no peek copy-link control.");
  }

  async peekFullScreenHref(): Promise<string | null> {
    const link = this.peekPortal().locator("a:has(svg.lucide-move-diagonal)").first();
    if ((await link.count()) === 0) return null;
    return await link.getAttribute("href").catch(() => null);
  }

  async peekQuickActionNames(): Promise<string[]> {
    const group = await this.peekHeaderActions();
    const trigger = group.locator('button[aria-haspopup="menu"], button[aria-haspopup="true"]').first();
    await trigger.click({ timeout: WebDriver.OPEN_MS });
    const items = this.page.locator('[role="menuitem"]:visible');
    await items.first().waitFor({ timeout: WebDriver.OPEN_MS });
    return (await items.allTextContents()).map((t) => t.trim()).filter((t) => t.length > 0);
  }

  async peekErrorTitle(): Promise<string | null> {
    const portal = this.peekPortal();
    if ((await portal.count()) === 0) return null;
    const text = ((await portal.innerText().catch(() => "")) ?? "").trim();
    const match = /Work item does not exist/.exec(text);
    return match ? match[0] : null;
  }

  /** Click the named issue row in the list; ends once the URL or peek settles. */
  async clickListRow(name: string): Promise<void> {
    const before = this.page.url();
    await this.page.getByText(name, { exact: true }).first().click({ timeout: WebDriver.OPEN_MS });
    const deadline = Date.now() + WebDriver.OPEN_MS;
    for (;;) {
      if (this.page.url() !== before) return;
      if (await this.peekOpen()) return;
      if (Date.now() > deadline) return;
      await this.page.waitForTimeout(500);
    }
  }

  // ---- Detail widgets (NEWFRONT-121). ----
  // Selectors follow the widget behavior observed on the running old app: a
  // collapsible per widget (a full-width header button holding the title
  // span plus count/progress, with the rows in a following grid sibling);
  // sections with no content do not render; the action row above offers add
  // buttons plus Run AI; the comment composer sits below with Comment and
  // Comment & Run controls.

  /** Header button of the named widget section, or null when absent. */
  private async widgetHeader(widget: string): Promise<Locator | null> {
    const btn = this.page.locator("button.w-full").filter({ hasText: widget }).first();
    if ((await btn.count()) === 0) return null;
    return btn;
  }

  /** Row container of the named widget section (header button's parent). */
  private async widgetSection(widget: string): Promise<Locator | null> {
    const btn = await this.widgetHeader(widget);
    if (!btn) return null;
    return btn.locator("xpath=parent::*");
  }

  /** Collapsible content sibling of the header button, or null. */
  private async widgetContent(widget: string): Promise<Locator | null> {
    const section = await this.widgetSection(widget);
    if (!section) return null;
    const content = section.locator("div.grid.overflow-hidden").first();
    if ((await content.count()) === 0) return null;
    return content;
  }

  async widgetTitles(): Promise<string[]> {
    const btns = this.page.locator("button.w-full");
    const count = await btns.count();
    const out: string[] = [];
    for (let i = 0; i < count; i++) {
      const span = btns.nth(i).locator("span.text-14").first();
      if ((await span.count()) === 0) continue;
      const text = ((await span.innerText().catch(() => "")) ?? "").trim();
      if (text !== "" && !out.includes(text)) out.push(text);
    }
    return out;
  }

  async widgetProgress(widget: string): Promise<string | null> {
    const btn = await this.widgetHeader(widget);
    if (!btn) return null;
    const text = ((await btn.innerText().catch(() => "")) ?? "").trim();
    const match = /(\d+\s*\/\s*\d+\s*Done)/.exec(text);
    return match ? match[1].replace(/\s+/g, " ") : null;
  }

  async widgetGroupNames(widget: string): Promise<string[]> {
    const content = await this.widgetContent(widget);
    if (!content) return [];
    const btns = content.locator("button");
    const count = await btns.count();
    const out: string[] = [];
    for (let i = 0; i < count; i++) {
      const text = (
        (await btns
          .nth(i)
          .innerText()
          .catch(() => "")) ?? ""
      ).trim();
      if (/relates|duplicate|blocked|blocking/i.test(text) && !out.includes(text)) out.push(text);
    }
    return out;
  }

  async openWidgetSection(widget: string): Promise<void> {
    // The header button's aria-expanded tracks the disclosure's internal
    // toggle, while the rows render only when the store-driven panel is
    // mounted — so content visibility (not the aria flag) decides. Clicking
    // while the panel is already mounted would collapse it.
    if (await this.widgetExpanded(widget)) return;
    const btn = await this.waitWidgetHeader(widget);
    await btn.click({ timeout: WebDriver.OPEN_MS });
    const deadline = Date.now() + WebDriver.OPEN_MS;
    for (;;) {
      if (await this.widgetExpanded(widget)) return;
      if (Date.now() > deadline) return;
      await this.page.waitForTimeout(500);
    }
  }

  /** Header button, waiting for counts to hydrate and render the section. */
  private async waitWidgetHeader(widget: string): Promise<Locator> {
    const deadline = Date.now() + WebDriver.OPEN_MS;
    for (;;) {
      const btn = await this.widgetHeader(widget);
      if (btn) return btn;
      if (Date.now() > deadline) throw new Error(`[parity] no widget section ${JSON.stringify(widget)}.`);
      await this.page.waitForTimeout(500);
    }
  }

  async widgetExpanded(widget: string): Promise<boolean> {
    // See openWidgetSection: the panel unmounts while closed, so a mounted
    // panel with a visible box means open. (The header aria flag tracks a
    // different toggle and disagrees with the panel on first paint.)
    const content = await this.widgetContent(widget);
    if (!content) return false;
    const box = await content.boundingBox().catch(() => null);
    return !!box && box.height > 2;
  }

  async toggleWidgetSection(widget: string): Promise<void> {
    const btn = await this.waitWidgetHeader(widget);
    await btn.click({ timeout: WebDriver.OPEN_MS });
  }

  async widgetHeaderControlCount(widget: string): Promise<number> {
    const header = await this.waitWidgetHeader(widget);
    return await header.locator("button").count();
  }

  /** Direct row blocks inside the section content. */
  private async widgetRows(widget: string): Promise<Locator[]> {
    const content = await this.widgetContent(widget);
    if (!content) return [];
    // Rows live one wrapper below the min-h-0 container; descend while a
    // single wrapper holds all the text.
    let scope = content.locator("div.min-h-0").first();
    if ((await scope.count()) === 0) scope = content;
    for (let depth = 0; depth < 3; depth++) {
      const kids = scope.locator("xpath=./*");
      if ((await kids.count()) !== 1) break;
      scope = kids.first();
    }
    const kids = scope.locator("xpath=./*");
    const count = await kids.count();
    const out: Locator[] = [];
    for (let i = 0; i < count; i++) out.push(kids.nth(i));
    return out;
  }

  async widgetRowNames(widget: string): Promise<string[]> {
    const rows = await this.widgetRows(widget);
    const out: string[] = [];
    for (const row of rows) {
      const text = ((await row.innerText().catch(() => "")) ?? "").trim().replace(/\s+/g, " ");
      if (text !== "") out.push(text);
    }
    return out;
  }

  async widgetAddMenuNames(widget: string): Promise<string[]> {
    // The section "+" (an unnamed icon button nested in the header) and
    // the action-row add button open the same menu; the action row reads
    // the same for every widget, so use it directly.
    const map: Record<string, string> = {
      "Sub-work items": "Add sub-work item",
      Relations: "Add relation",
      Links: "Add link",
      Attachments: "Attach",
    };
    const trigger = map[widget];
    if (!trigger) throw new Error(`[parity] no add menu for widget ${JSON.stringify(widget)}.`);
    await this.page.getByRole("button", { name: trigger }).first().click({ timeout: WebDriver.OPEN_MS });
    const items = this.page.locator('[role="menuitem"]:visible');
    await items.first().waitFor({ timeout: WebDriver.OPEN_MS });
    return (await items.allTextContents()).map((t) => t.trim()).filter((t) => t.length > 0);
  }

  async openSubIssueCreateModal(): Promise<void> {
    // The action-row button opens a Create-new/Add-existing menu; picking
    // Create new raises the modal with the parent preset.
    await this.page.getByRole("button", { name: "Add sub-work item" }).first().click({ timeout: WebDriver.OPEN_MS });
    const items = this.page.locator('[role="menuitem"]:visible');
    await items.first().waitFor({ timeout: WebDriver.OPEN_MS });
    await items.filter({ hasText: "Create new" }).first().click({ timeout: WebDriver.OPEN_MS });
    const dialog = this.appDialogs();
    await dialog.first().waitFor({ state: "attached", timeout: WebDriver.OPEN_MS });
  }

  async createModalParentName(): Promise<string | null> {
    // The preset parent renders as an identifier line followed by the
    // parent's name (no "Parent" caption in the text).
    const dialog = this.appDialogs().first();
    const text = ((await dialog.innerText().catch(() => "")) ?? "").trim();
    const lines = text.split("\n").map((line) => line.trim());
    for (let i = 0; i + 1 < lines.length; i++) {
      if (/^[A-Z0-9]{2,}-\d+$/.test(lines[i] ?? "")) {
        const next = lines[i + 1] ?? "";
        if (next !== "" && !/^(Create|Discard|Save|Cancel|Select|Deselect|No work|Advanced)/.test(next)) return next;
      }
    }
    return null;
  }

  async createModalProjectLocked(): Promise<boolean> {
    const dialog = this.appDialogs().first();
    const projectField = dialog.locator("input, button").filter({ hasText: /Parity Project|PAR/ });
    if ((await projectField.count()) === 0) return false;
    const disabled = await projectField
      .first()
      .getAttribute("disabled")
      .catch(() => null);
    const aria = await projectField
      .first()
      .getAttribute("aria-disabled")
      .catch(() => null);
    return disabled !== null || aria === "true";
  }

  async createModalSubmit(name: string): Promise<void> {
    // The modal's submit is the exact "Save" button ("Create more" only
    // toggles keep-open and matches a loose /Create/ first).
    const dialog = this.appDialogs().first();
    const nameField = dialog.locator('input[type="text"], input:not([type])').first();
    await nameField.fill(name, { timeout: WebDriver.OPEN_MS });
    await dialog.getByRole("button", { name: "Save", exact: true }).first().click({ timeout: WebDriver.OPEN_MS });
    await dialog.waitFor({ state: "detached", timeout: WebDriver.OPEN_MS }).catch(() => {});
  }

  async addExistingSubIssue(search: string, name: string): Promise<void> {
    await this.page.getByRole("button", { name: "Add sub-work item" }).first().click({ timeout: WebDriver.OPEN_MS });
    const menu = this.page.locator('[role="menuitem"]:visible');
    await menu.first().waitFor({ timeout: WebDriver.OPEN_MS });
    await menu.filter({ hasText: "Add existing" }).first().click({ timeout: WebDriver.OPEN_MS });
    const dialog = this.appDialogs();
    await dialog.first().waitFor({ state: "attached", timeout: WebDriver.OPEN_MS });
    const searchField = dialog.first().locator("input").first();
    await searchField.fill(search, { timeout: WebDriver.OPEN_MS });
    await dialog.first().getByRole("option", { name }).first().click({ timeout: WebDriver.OPEN_MS });
    await dialog
      .first()
      .getByRole("button", { name: "Add selected work items" })
      .first()
      .click({ timeout: WebDriver.OPEN_MS });
    await dialog
      .first()
      .waitFor({ state: "detached", timeout: WebDriver.OPEN_MS })
      .catch(() => {});
  }

  private async widgetRow(widget: string, rowName: string): Promise<Locator> {
    const rows = await this.widgetRows(widget);
    for (const row of rows) {
      const text = ((await row.innerText().catch(() => "")) ?? "").trim();
      if (text.includes(rowName)) return row;
    }
    throw new Error(`[parity] no row ${JSON.stringify(rowName)} in widget ${JSON.stringify(widget)}.`);
  }

  /**
   * Nearest block holding the named row's title that also contains the
   * wanted control. Row text blocks split by count (one link renders as
   * two blocks, two links as two cards), so walking up from the title to
   * the control's scope beats matching whole blocks.
   */
  private async widgetRowScope(widget: string, rowName: string, control: string): Promise<Locator> {
    const content = await this.widgetContent(widget);
    if (!content) throw new Error(`[parity] no widget section ${JSON.stringify(widget)}.`);
    const titleEl = content.getByText(rowName).last();
    if ((await titleEl.count()) === 0) {
      throw new Error(`[parity] no row ${JSON.stringify(rowName)} in widget ${JSON.stringify(widget)}.`);
    }
    let scope = titleEl.locator("xpath=parent::*");
    for (let i = 0; i < 6; i++) {
      if ((await scope.locator(control).count()) > 0) return scope;
      scope = scope.locator("xpath=parent::*");
    }
    throw new Error(`[parity] no control ${JSON.stringify(control)} for row ${JSON.stringify(rowName)}.`);
  }

  async clickWidgetRow(widget: string, rowName: string): Promise<void> {
    try {
      const scope = await this.widgetRowScope(widget, rowName, "a[href]");
      await scope.locator("a[href]").first().click({ timeout: WebDriver.OPEN_MS });
    } catch {
      const row = await this.widgetRow(widget, rowName);
      await row.click({ timeout: WebDriver.OPEN_MS });
    }
  }

  async widgetRowActionNames(widget: string, rowName: string): Promise<string[]> {
    const scope = await this.widgetRowScope(
      widget,
      rowName,
      'button[aria-haspopup="menu"], button[aria-haspopup="true"]'
    );
    const trigger = scope.locator('button[aria-haspopup="menu"], button[aria-haspopup="true"]').first();
    await trigger.click({ timeout: WebDriver.OPEN_MS });
    const items = this.page.locator('[role="menuitem"]:visible');
    await items.first().waitFor({ timeout: WebDriver.OPEN_MS });
    return (await items.allTextContents()).map((t) => t.trim()).filter((t) => t.length > 0);
  }

  async clickWidgetRowAction(widget: string, rowName: string, action: string): Promise<void> {
    const scope = await this.widgetRowScope(
      widget,
      rowName,
      'button[aria-haspopup="menu"], button[aria-haspopup="true"]'
    );
    const trigger = scope.locator('button[aria-haspopup="menu"], button[aria-haspopup="true"]').first();
    await trigger.click({ timeout: WebDriver.OPEN_MS });
    const items = this.page.locator('[role="menuitem"]:visible');
    await items.first().waitFor({ timeout: WebDriver.OPEN_MS });
    await items.filter({ hasText: action }).first().click({ timeout: WebDriver.OPEN_MS });
  }

  /**
   * First app dialog with rendered content, or null. The dialog wrapper
   * itself has a zero box (positioning shell — Playwright calls it
   * hidden), so content presence, not wrapper visibility, decides.
   */
  private async openAppDialog(): Promise<Locator | null> {
    const dialogs = this.appDialogs();
    const count = await dialogs.count();
    for (let i = 0; i < count; i++) {
      const candidate = dialogs.nth(i);
      const text = ((await candidate.innerText().catch(() => "")) ?? "").trim();
      if (text !== "") return candidate;
    }
    return null;
  }

  async confirmModalTitle(): Promise<string | null> {
    const dialog = await this.openAppDialog();
    if (!dialog) return null;
    const heading = dialog.locator("h1, h2, h3, [role='heading']").first();
    if ((await heading.count()) > 0) return ((await heading.innerText().catch(() => "")) ?? "").trim() || null;
    const text = ((await dialog.innerText().catch(() => "")) ?? "").trim();
    return text.split("\n")[0]?.trim() || null;
  }

  async confirmModalText(): Promise<string | null> {
    const dialog = await this.openAppDialog();
    if (!dialog) return null;
    const text = ((await dialog.innerText().catch(() => "")) ?? "").trim();
    return text === "" ? null : text;
  }

  async confirmModal(label: string): Promise<void> {
    // The dialog mounts asynchronously after the menu click, so poll for
    // it instead of checking once (a single check flakes under load).
    const deadline = Date.now() + WebDriver.OPEN_MS;
    let dialog: Locator | null = null;
    for (;;) {
      dialog = await this.openAppDialog();
      if (dialog) break;
      if (Date.now() > deadline) throw new Error("[parity] no confirm modal is open.");
      await this.page.waitForTimeout(250);
    }
    await dialog.getByRole("button", { name: label }).first().click({ timeout: WebDriver.OPEN_MS });
    await dialog.waitFor({ state: "detached", timeout: WebDriver.OPEN_MS }).catch(() => {});
  }

  async addRelationViaModal(type: string, search: string, name: string): Promise<void> {
    await this.page.getByRole("button", { name: "Add relation" }).first().click({ timeout: WebDriver.OPEN_MS });
    const items = this.page.locator('[role="menuitem"]:visible');
    await items.first().waitFor({ timeout: WebDriver.OPEN_MS });
    await items.filter({ hasText: type }).first().click({ timeout: WebDriver.OPEN_MS });
    // The shared existing-issues modal multi-selects, then submits.
    const dialog = this.appDialogs();
    await dialog.first().waitFor({ state: "attached", timeout: WebDriver.OPEN_MS });
    const searchField = dialog.first().locator("input").first();
    await searchField.fill(search, { timeout: WebDriver.OPEN_MS });
    await dialog.first().getByRole("option", { name }).first().click({ timeout: WebDriver.OPEN_MS });
    await dialog
      .first()
      .getByRole("button", { name: "Add selected work items" })
      .first()
      .click({ timeout: WebDriver.OPEN_MS });
    await dialog
      .first()
      .waitFor({ state: "detached", timeout: WebDriver.OPEN_MS })
      .catch(() => {});
  }

  async addLinkModal(url: string, title?: string): Promise<void> {
    await this.page.getByRole("button", { name: "Add link" }).first().click({ timeout: WebDriver.OPEN_MS });
    const dialog = this.appDialogs();
    await dialog.first().waitFor({ state: "attached", timeout: WebDriver.OPEN_MS });
    const fields = dialog.first().locator("input");
    await fields.first().fill(url, { timeout: WebDriver.OPEN_MS });
    if (title !== undefined && (await fields.count()) > 1) {
      await fields.nth(1).fill(title, { timeout: WebDriver.OPEN_MS });
    }
    await dialog
      .first()
      .getByRole("button", { name: /Add|Save|Create/ })
      .first()
      .click({ timeout: WebDriver.OPEN_MS });
    await dialog
      .first()
      .waitFor({ state: "detached", timeout: WebDriver.OPEN_MS })
      .catch(() => {});
  }

  async editLinkTitle(rowName: string, title: string): Promise<void> {
    const scope = await this.widgetRowScope(
      "Links",
      rowName,
      'button[aria-haspopup="menu"], button[aria-haspopup="true"]'
    );
    const trigger = scope.locator('button[aria-haspopup="menu"], button[aria-haspopup="true"]').first();
    await trigger.click({ timeout: WebDriver.OPEN_MS });
    const items = this.page.locator('[role="menuitem"]:visible');
    await items.first().waitFor({ timeout: WebDriver.OPEN_MS });
    await items.filter({ hasText: /edit/i }).first().click({ timeout: WebDriver.OPEN_MS });
    const dialog = this.appDialogs();
    await dialog.first().waitFor({ state: "attached", timeout: WebDriver.OPEN_MS });
    const fields = dialog.first().locator("input");
    await fields.nth(1).fill(title, { timeout: WebDriver.OPEN_MS });
    await dialog
      .first()
      .getByRole("button", { name: /Save|Update/ })
      .first()
      .click({ timeout: WebDriver.OPEN_MS });
    await dialog
      .first()
      .waitFor({ state: "detached", timeout: WebDriver.OPEN_MS })
      .catch(() => {});
  }

  async clickLinkCopy(rowName: string): Promise<void> {
    // The copy control is a pointer-styled span holding the copy icon
    // (the row's anchor opens the URL instead, so the generic row click
    // would navigate away rather than copy).
    const scope = await this.widgetRowScope("Links", rowName, "span.cursor-pointer");
    await scope.locator("span.cursor-pointer").first().click({ timeout: WebDriver.OPEN_MS });
  }

  async linkRowTarget(rowName: string): Promise<{ href: string; target: string | null } | null> {
    const scope = await this.widgetRowScope("Links", rowName, "a[href]").catch(() => null);
    if (!scope) return null;
    const link = scope.locator("a[href]").first();
    if ((await link.count()) === 0) return null;
    const href = await link.getAttribute("href").catch(() => null);
    if (!href) return null;
    return { href, target: await link.getAttribute("target").catch(() => null) };
  }

  async uploadAttachment(file: { name: string; mime: string; bytes: Buffer }): Promise<void> {
    const chooserWait = this.page.waitForEvent("filechooser", { timeout: WebDriver.OPEN_MS }).catch(() => null);
    await this.page.getByRole("button", { name: "Attach" }).first().click({ timeout: WebDriver.OPEN_MS });
    const chooser = await chooserWait;
    if (chooser) {
      await chooser.setFiles([{ name: file.name, mimeType: file.mime, buffer: file.bytes }]);
      return;
    }
    const input = this.page.locator('input[type="file"]').first();
    await input.waitFor({ state: "attached", timeout: WebDriver.OPEN_MS });
    await input.setInputFiles([{ name: file.name, mimeType: file.mime, buffer: file.bytes }]);
  }

  async clickWidgetAction(name: string): Promise<void> {
    await this.page.getByRole("button", { name }).first().click({ timeout: WebDriver.OPEN_MS });
  }

  async typeComment(text: string): Promise<void> {
    const editor = this.page.locator("[contenteditable='true']").last();
    await editor.click({ timeout: WebDriver.OPEN_MS });
    await editor.fill(text, { timeout: WebDriver.OPEN_MS });
  }

  async postComment(text: string): Promise<void> {
    await this.typeComment(text);
    await this.page
      .getByRole("button", { name: /^Comment$/ })
      .first()
      .click({ timeout: WebDriver.OPEN_MS });
  }

  async clickCommentAndRun(): Promise<void> {
    await this.page.getByRole("button", { name: "Comment & Run" }).first().click({ timeout: WebDriver.OPEN_MS });
  }

  async commentAndRunDisabled(): Promise<boolean> {
    const btn = this.page.getByRole("button", { name: "Comment & Run" }).first();
    await btn.waitFor({ timeout: WebDriver.OPEN_MS });
    return btn.isDisabled().catch(() => true);
  }

  // Activity feed (NEWFRONT-114). All reads scope from the user-visible
  // "Activity" heading: its parent is the header row (title plus the
  // worklog/sort/filter icon buttons, in that DOM order) and its
  // grandparent is the section. Entries are the children of the feed
  // container — the inner child without the composer editor.

  private activityHeading(): Locator {
    return this.page.getByText("Activity", { exact: true }).first();
  }

  private async activitySection(): Promise<Locator> {
    const heading = this.activityHeading();
    // Generous: the heading renders only after the whole detail page loads,
    // which stalls under concurrent parity runs on the shared stack.
    await heading.waitFor({ timeout: 120_000 });
    return heading.locator("xpath=../..");
  }

  /**
   * Header buttons decoded from the running app: the sort control is a
   * plain icon button, while the filter control is a popover whose
   * headless trigger button wraps an inner icon button plus the narrowed
   * marker dot. An optional worklog button may precede both, so neither
   * control is addressed by raw position.
   */
  private async activitySortButton(): Promise<Locator> {
    const header = (await this.activitySection()).locator(":scope > div:nth-child(1)");
    const buttons = header.getByRole("button");
    const n = await buttons.count();
    let sort: Locator | null = null;
    for (let i = 0; i < n; i++) {
      const candidate = buttons.nth(i);
      if ((await candidate.locator(":scope button").count()) > 0) continue;
      if (await candidate.evaluate((el) => el.parentElement?.tagName.toLowerCase() === "button")) continue;
      sort = candidate;
    }
    if (sort === null) throw new Error("[parity] activity sort button not found");
    return sort;
  }

  private async activityFilterButton(): Promise<Locator> {
    const header = (await this.activitySection()).locator(":scope > div:nth-child(1)");
    const buttons = header.getByRole("button");
    const n = await buttons.count();
    for (let i = 0; i < n; i++) {
      const candidate = buttons.nth(i);
      if ((await candidate.locator(":scope button").count()) > 0) return candidate;
    }
    throw new Error("[parity] activity filter button not found");
  }

  private async activityFeedRoot(section: Locator): Promise<Locator | null> {
    const inner = section.locator(":scope > div:nth-child(2) > div > div");
    if ((await inner.count()) === 0) return null;
    const kids = inner.locator(":scope > div");
    const n = await kids.count();
    for (let i = 0; i < n; i++) {
      const kid = kids.nth(i);
      if ((await kid.locator('[contenteditable="true"]').count()) === 0) return kid;
    }
    return null;
  }

  /**
   * Defensive sign-in for the shared scratch stack: the credential POST can
   * be throttled (429) or land mid-reseed by a concurrent parity run, which
   * leaves the page on the entry route with no session. Repeat the
   * entry-plus-password flow until the workspace URL lands, then return;
   * throw after three attempts. The base openEntry/signInWithPassword
   * methods are untouched (parent contract: extend, never modify).
   */
  async activitySignIn(email: string, password: string, workspaceSlug: string): Promise<void> {
    let lastUrl = "";
    for (let attempt = 1; attempt <= 3; attempt++) {
      await this.openEntry();
      await this.signInWithPassword(email, password);
      const start = Date.now();
      while (Date.now() - start < 45_000) {
        lastUrl = this.page.url();
        if (lastUrl.includes(workspaceSlug)) return;
        await this.page.waitForTimeout(1000);
      }
    }
    throw new Error(`[parity] sign-in did not land in the workspace after 3 tries (last URL: ${lastUrl})`);
  }

  async activityOpenIssueDetail(workspaceSlug: string, projectId: string, issueId: string): Promise<void> {
    await this.page.goto(`/${workspaceSlug}/projects/${projectId}/issues/${issueId}`);
    await this.activityHeading().waitFor();
  }

  async activityEntryTexts(): Promise<string[]> {
    const section = await this.activitySection();
    const feed = await this.activityFeedRoot(section);
    if (feed === null) return [];
    const entries = feed.locator(":scope > *");
    const n = await entries.count();
    const texts: string[] = [];
    for (let i = 0; i < n; i++) {
      texts.push(((await entries.nth(i).textContent()) ?? "").trim().replace(/\s+/g, " "));
    }
    return texts.filter((t) => t.length > 0);
  }

  async activityToggleSort(): Promise<void> {
    await (await this.activitySortButton()).click();
  }

  /**
   * The filter menu renders as an overlay without a menu role, next to the
   * filter button. Among the visible exact-text matches for a label (the
   * page itself can show the same words, e.g. a state control), the menu
   * option is the one nearest the filter button.
   */
  private async activityMenuOption(label: string): Promise<Locator | null> {
    const button = await this.activityFilterButton();
    const box = await button.boundingBox();
    if (box === null) throw new Error("[parity] filter button has no bounding box");
    const centerX = box.x + box.width / 2;
    const centerY = box.y + box.height / 2;
    const candidates = this.page.getByText(label, { exact: true });
    const n = await candidates.count();
    let best: Locator | null = null;
    let bestDistance = Number.POSITIVE_INFINITY;
    for (let i = 0; i < n; i++) {
      const candidate = candidates.nth(i);
      if (!(await candidate.isVisible())) continue;
      const target = await candidate.boundingBox();
      if (target === null) continue;
      const distance = Math.hypot(target.x + target.width / 2 - centerX, target.y + target.height / 2 - centerY);
      if (distance < bestDistance) {
        best = candidate;
        bestDistance = distance;
      }
    }
    return best;
  }

  async activityOpenFilterMenu(): Promise<void> {
    if ((await this.activityMenuOption("Updates")) !== null) return;
    await (await this.activityFilterButton()).click();
    const start = Date.now();
    while ((await this.activityMenuOption("Updates")) === null) {
      if (Date.now() - start > 15_000) throw new Error("[parity] filter menu did not open");
      await this.page.waitForTimeout(250);
    }
  }

  async activityFilterOptionLabels(): Promise<string[]> {
    const found: string[] = [];
    for (const label of ["Updates", "Comments", "State", "Assignee"]) {
      if ((await this.activityMenuOption(label)) !== null) found.push(label);
    }
    return found;
  }

  async activityToggleFilterOption(label: string): Promise<void> {
    const option = await this.activityMenuOption(label);
    if (option === null) throw new Error(`[parity] filter option not found near the menu: ${label}`);
    await option.click();
  }

  async activityFilterNarrowed(): Promise<boolean> {
    const filter = await this.activityFilterButton();
    return (await filter.locator(":scope > span").count()) > 0;
  }

  async activityComposerPosition(): Promise<"above" | "below" | "hidden"> {
    const section = await this.activitySection();
    const composer = section.locator('[contenteditable="true"]');
    if ((await composer.count()) === 0) return "hidden";
    const feed = await this.activityFeedRoot(section);
    if (feed === null) throw new Error("[parity] composer is present but the feed is empty");
    const composerHandle = await composer.first().elementHandle();
    const feedHandle = await feed.elementHandle();
    if (composerHandle === null || feedHandle === null) throw new Error("[parity] composer/feed handles missing");
    // eslint-disable-next-line no-bitwise -- compareDocumentPosition is a bitmask by design.
    const order = await composer
      .first()
      .evaluate(
        (el, other) => el.compareDocumentPosition(other as Node) & Node.DOCUMENT_POSITION_FOLLOWING,
        feedHandle
      );
    return order !== 0 ? "above" : "below";
  }

  async activityComposerType(text: string): Promise<void> {
    const section = await this.activitySection();
    const editor = section.locator('[contenteditable="true"]').first();
    await editor.click();
    await editor.pressSequentially(text, { delay: 10 });
  }

  async activityComposerSubmit(): Promise<void> {
    const section = await this.activitySection();
    await section.getByRole("button", { name: "Comment", exact: true }).click();
  }

  async activityRenameTitle(title: string): Promise<void> {
    const box = this.page.getByPlaceholder("Work item title");
    await box.click();
    await box.fill(title);
    await box.press("Enter");
    // The title commits on blur, so move focus back to the feed section.
    await this.activityHeading().click();
  }

  async activityLoadingVisible(): Promise<boolean> {
    const section = await this.activitySection();
    return (await section.getByRole("status").count()) > 0;
  }

  async activityStoredSort(): Promise<string | null> {
    return this.page.evaluate(() => window.localStorage.getItem("activity_sort_order"));
  }

  async activityStoredFilters(): Promise<string | null> {
    return this.page.evaluate(() => window.localStorage.getItem("issue_activity_filters"));
  }

  async activityOpenFirstEntryLink(): Promise<string> {
    const section = await this.activitySection();
    const feed = await this.activityFeedRoot(section);
    if (feed === null) throw new Error("[parity] feed is empty, no link to open");
    const link = feed.locator("a").first();
    await link.waitFor();
    await Promise.all([this.page.waitForURL(/profile/), link.click()]);
    return this.page.url();
  }

  private patchFailureRoute:
    | {
        matches: (url: URL) => boolean;
        handler: (route: Parameters<Parameters<Page["route"]>[1]>[0]) => Promise<void>;
      }
    | undefined;

  // --- Issue activity & comments (NEWFRONT-122, ISS-194–206). Observed on
  // --- the running old app: the detail route redirects to the canonical
  // --- browse URL; comment cards carry `comment-<id>` anchors; the composer
  // --- is a labelled group with a rich-text editor; card menus and emoji
  // --- pickers are icon-only triggers opened here with force clicks because
  // --- a decorative access-specifier overlay covers their hit target.

  private activityHeaderRow(): Locator {
    return this.activityHeading().locator("xpath=ancestor::div[.//button][1]");
  }

  private composer(): Locator {
    return this.page.locator('[aria-label="Add comment"]');
  }

  private async dismissOverlays(): Promise<void> {
    await this.page.keyboard.press("Escape");
    await this.page
      .locator('[role="dialog"]:visible')
      .first()
      .waitFor({ state: "detached", timeout: 5_000 })
      .catch(() => {});
  }

  async openArchivedIssueDetail(workspaceSlug: string, projectId: string, issueId: string): Promise<void> {
    // Archived issues render on the archives URL itself (no browse redirect)
    // with an archive banner above the same detail root.
    await this.page.goto(`/${workspaceSlug}/projects/${projectId}/archives/issues/${issueId}`);
    await this.page.waitForLoadState("domcontentloaded");
    const deadline = Date.now() + 75_000;
    for (;;) {
      if ((await this.page.getByText(/does not exist|went wrong/i).count()) > 0)
        throw new Error(`[parity] archived detail for ${issueId} rendered an error state (${this.page.url()}).`);
      try {
        await this.activityHeading().waitFor({ timeout: Math.max(1_000, deadline - Date.now()) });
        return;
      } catch {
        if (Date.now() >= deadline)
          throw new Error(`[parity] Activity never rendered for archived ${issueId} (current: ${this.page.url()}).`);
      }
    }
  }

  async activityCommentTexts(): Promise<string[]> {
    return this.page.locator('[id^="comment-"]').allInnerTexts();
  }

  async activityHasCreationEntry(): Promise<boolean> {
    return (
      (await this.page
        .getByRole("main")
        .getByText(/created the work item/i)
        .count()) > 0
    );
  }

  async activityFilterOptions(): Promise<{ label: string; selected: boolean }[]> {
    await this.dismissOverlays();
    const headerRow = this.activityHeaderRow();
    await headerRow.locator("button[data-headlessui-state]").first().click({ force: true, timeout: 15_000 });
    const panel = this.page.locator('[id^="headlessui-popover-panel"]').first();
    await panel.waitFor({ timeout: 10_000 });
    const options = await panel.evaluate((root) => {
      const rows = Array.from(root.querySelectorAll("div[class*='cursor-pointer']"));
      return rows.map((row) => ({
        label: (row as HTMLElement).innerText.trim(),
        selected: row.querySelector("svg") !== null,
      }));
    });
    await this.dismissOverlays();
    return options;
  }

  async activityToggleFilter(label: string): Promise<void> {
    await this.dismissOverlays();
    const headerRow = this.activityHeaderRow();
    await headerRow.locator("button[data-headlessui-state]").first().click({ force: true, timeout: 15_000 });
    const panel = this.page.locator('[id^="headlessui-popover-panel"]').first();
    await panel.waitFor({ timeout: 10_000 });
    await panel
      .locator("div[class*='cursor-pointer']", { hasText: label })
      .first()
      .click({ force: true, timeout: 15_000 });
    await this.dismissOverlays();
  }

  async activityFilterDotVisible(): Promise<boolean> {
    const trigger = this.activityHeaderRow().locator("button[data-headlessui-state]").first();
    return (
      (await trigger
        .evaluate((el) => el.querySelector('span[class*="bg-accent-primary"]') !== null)
        .catch(() => false)) ?? false
    );
  }

  async activityComposerIsAboveFeed(): Promise<boolean> {
    const composerBox = await this.composer().boundingBox();
    const firstCardBox = await this.page.locator('[id^="comment-"]').first().boundingBox();
    if (composerBox === null || firstCardBox === null)
      throw new Error("[parity] composer or feed card has no layout box.");
    return composerBox.y < firstCardBox.y;
  }

  async activityComposerText(): Promise<string> {
    return (await this.composer().locator('[contenteditable="true"]').first().innerText()).trim();
  }

  async activityPostComment(bodyText: string): Promise<void> {
    const editor = this.composer().locator('[contenteditable="true"]').first();
    await editor.waitFor({ timeout: 30_000 });
    await editor.click();
    await editor.fill(bodyText);
    await editor.press("Enter");
    await this.commentCard(bodyText).first().waitFor({ timeout: 30_000 });
  }

  async activityOpenCommentMenu(cardText: string): Promise<void> {
    await this.dismissOverlays();
    const card = this.commentCard(cardText).first();
    await card.waitFor({ timeout: 30_000 });
    await card.hover({ timeout: 15_000 });
    await card.locator('button[aria-haspopup="menu"]').first().click({ force: true, timeout: 15_000 });
    await this.page.getByRole("menuitem").first().waitFor({ timeout: 10_000 });
  }

  async activityMenuItems(): Promise<string[]> {
    return this.page.getByRole("menuitem").allInnerTexts();
  }

  async activityClickMenuItem(name: string): Promise<void> {
    await this.page.getByRole("menuitem", { name }).first().click({ timeout: 15_000 });
  }

  async activityEditComment(oldText: string, newText: string): Promise<void> {
    await this.activityOpenCommentMenu(oldText);
    await this.activityClickMenuItem("Edit");
    // The open editor is unique on the page; scope to it instead of the card
    // text, which stops matching once the body is replaced below.
    const editor = this.page.locator('[id^="comment-"] [contenteditable="true"]').first();
    await editor.waitFor({ timeout: 10_000 });
    await editor.click();
    await editor.press("ControlOrMeta+a");
    await editor.pressSequentially(newText, { timeout: 15_000 });
    await this.page.waitForFunction(
      (text) => {
        const open = document.querySelector('[id^="comment-"] [contenteditable="true"]');
        return open instanceof HTMLElement && open.innerText.includes(text);
      },
      newText,
      { timeout: 15_000 }
    );
    // Save through the confirm affordance: pressing Enter here races the
    // editor re-render, while the check button is the stable save path.
    const form = editor.locator("xpath=ancestor::form[1]");
    await form.locator('button[class*="border-success"]').first().click({ timeout: 15_000 });
    await this.commentCard(newText).first().getByText("(edited)", { exact: false }).waitFor({ timeout: 30_000 });
  }

  async sawToast(text: string): Promise<boolean> {
    return (await this.page.locator("body").innerText()).includes(text);
  }

  async activityCancelEdit(cardText: string): Promise<void> {
    const card = this.commentCard(cardText).first();
    await card.locator('button[class*="border-danger"]').first().click({ timeout: 15_000 });
    await card.locator('[contenteditable="true"]').first().waitFor({ state: "detached", timeout: 15_000 });
  }

  async activityCommentHighlighted(cardText: string): Promise<boolean> {
    const cls = await this.commentCard(cardText)
      .first()
      .evaluate((el) => {
        const editor = el.querySelector("div[class*='border-accent-strong']");
        return editor instanceof HTMLElement ? editor.className : "";
      });
    return cls.includes("border-accent-strong");
  }

  async activityChipTooltipText(cardText: string, emoji: string, expectedName: string): Promise<string> {
    await this.commentCard(cardText)
      .first()
      .locator("button:not([aria-haspopup])", { hasText: emoji })
      .first()
      .hover({ timeout: 15_000 });
    await this.page.waitForFunction((name) => document.body.innerText.includes(name), expectedName, {
      timeout: 10_000,
    });
    return this.page.evaluate((name) => {
      const candidates = Array.from(document.querySelectorAll("body *")).filter((el) =>
        (el as HTMLElement).innerText?.includes(name)
      );
      candidates.sort((a, b) => a.innerHTML.length - b.innerHTML.length);
      return ((candidates[0] as HTMLElement | undefined)?.innerText ?? "").slice(0, 200);
    }, expectedName);
  }

  async activityCommentBodyVisible(cardText: string): Promise<boolean> {
    // A folded card drops the body text, so the card itself stops matching:
    // no match means hidden, not an error. Callers poll through hydration.
    const card = this.commentCard(cardText).first();
    try {
      await card.waitFor({ timeout: 5_000 });
    } catch {
      return false;
    }
    return (await card.innerText()).includes(cardText);
  }

  async activityExpandFoldedComment(_cardText: string): Promise<void> {
    // The folded card no longer contains its body text; the expand toggle is
    // unique per feed in these scenarios, so match it directly.
    await this.page
      .getByRole("button", { name: /Click to expand/ })
      .first()
      .click({ timeout: 15_000 });
  }

  async activityCopyCommentLink(cardText: string): Promise<string> {
    await this.page.context().grantPermissions(["clipboard-read", "clipboard-write"]);
    await this.activityOpenCommentMenu(cardText);
    await this.activityClickMenuItem("Copy link");
    await this.page.waitForFunction(() => navigator.clipboard.readText().then((t) => t.length > 0), null, {
      timeout: 10_000,
    });
    return this.page.evaluate(() => navigator.clipboard.readText());
  }

  private static emojiCode(emoji: string): string {
    return Array.from(emoji)
      .map((char) => char.codePointAt(0))
      .join("-");
  }

  private async pickFirstEmoji(trigger: Locator): Promise<{ emoji: string; code: string }> {
    await this.dismissOverlays();
    await trigger.click({ force: true, timeout: 15_000 });
    // Take the first rendered grid entry straight away: typing in the search
    // box collapses this popover, so no query is used. A fresh browser
    // profile opens the same default recents grid on every run.
    const entry = this.page.locator('button[data-slot="emoji-picker-list-emoji"]:visible').first();
    await entry.waitFor({ timeout: 10_000 });
    const emoji = ((await entry.textContent()) ?? "").trim();
    if (emoji === "") throw new Error("[parity] first grid entry carried no character.");
    await entry.click({ force: true, timeout: 15_000 });
    return { emoji, code: WebDriver.emojiCode(emoji) };
  }

  private static parseChips(entries: { text: string; cls: string; popup: string | null }[]): {
    emoji: string;
    count: number;
    reacted: boolean;
  }[] {
    const chips: { emoji: string; count: number; reacted: boolean }[] = [];
    for (const entry of entries) {
      // Picker triggers wrap the whole chip group, so their text duplicates
      // every chip; only the chips themselves carry a highlight state.
      if (entry.popup === "dialog") continue;
      const text = entry.text.trim();
      if (text === "" || /^Folded comment/.test(text)) continue;
      const match = /^(.*?)(\d+)$/.exec(text.replace(/\s+/g, ""));
      if (match)
        chips.push({
          emoji: match[1] ?? "",
          count: Number(match[2]),
          reacted: entry.cls.includes("border-accent-strong"),
        });
    }
    return chips;
  }

  async activityAddCommentReaction(cardText: string): Promise<{ emoji: string; code: string }> {
    return this.pickFirstEmoji(this.commentCard(cardText).first().locator('button[aria-haspopup="dialog"]').first());
  }

  async activityCommentReactionChips(cardText: string): Promise<{ emoji: string; count: number; reacted: boolean }[]> {
    const entries = await this.commentCard(cardText)
      .first()
      .evaluate((root) =>
        Array.from(root.querySelectorAll("button")).map((b) => ({
          text: (b as HTMLElement).innerText,
          cls: (b as HTMLElement).className,
          popup: (b as HTMLElement).getAttribute("aria-haspopup"),
        }))
      );
    return WebDriver.parseChips(entries);
  }

  async activityClickCommentReactionChip(cardText: string, emoji: string): Promise<void> {
    // The picker trigger wraps the chips, so it name-matches too: exclude it.
    await this.commentCard(cardText)
      .first()
      .locator("button:not([aria-haspopup])", { hasText: emoji })
      .first()
      .click({ timeout: 15_000 });
  }

  private async issueReactionTrigger(): Promise<Locator> {
    const index = await this.page.evaluate(() => {
      const heading = Array.from(document.querySelectorAll("*")).find(
        (el) => el.children.length === 0 && el.textContent?.trim() === "Activity"
      );
      if (!heading) return -1;
      const top = (heading as HTMLElement).getBoundingClientRect().top;
      const triggers = Array.from(document.querySelectorAll('button[aria-haspopup="dialog"]'));
      return triggers.findIndex((el) => (el as HTMLElement).getBoundingClientRect().top < top);
    });
    if (index < 0) throw new Error("[parity] no issue-level reaction trigger above the Activity feed.");
    return this.page.locator('button[aria-haspopup="dialog"]').nth(index);
  }

  async issueAddReaction(): Promise<{ emoji: string; code: string }> {
    return this.pickFirstEmoji(await this.issueReactionTrigger());
  }

  async issueReactionChips(): Promise<{ emoji: string; count: number; reacted: boolean }[]> {
    const trigger = await this.issueReactionTrigger();
    const entries = await trigger.evaluate((root) =>
      Array.from(root.querySelectorAll("button")).map((b) => ({
        text: (b as HTMLElement).innerText,
        cls: (b as HTMLElement).className,
        popup: (b as HTMLElement).getAttribute("aria-haspopup"),
      }))
    );
    return WebDriver.parseChips(entries);
  }

  async issueClickReactionChip(emoji: string): Promise<void> {
    const trigger = await this.issueReactionTrigger();
    await trigger.getByRole("button", { name: emoji }).first().click({ timeout: 15_000 });
  }

  private codeReviewsSection(): Locator {
    return this.page.getByText("Code reviews", { exact: true }).first();
  }

  async codeReviewsVisible(): Promise<boolean> {
    return (await this.codeReviewsSection().count()) > 0;
  }

  async codeReviewLinks(): Promise<{ badge: string; title: string; href: string | null; target: string | null }[]> {
    return this.page.evaluate(() => {
      const heading = Array.from(document.querySelectorAll("*")).find(
        (el) => el.children.length === 0 && el.textContent?.trim() === "Code reviews"
      );
      if (!heading) return [];
      let container: Element | null = heading.parentElement;
      while (container && container.querySelectorAll('a[target="_blank"]').length === 0)
        container = container.parentElement;
      if (!container) return [];
      return Array.from(container.querySelectorAll('a[target="_blank"]')).map((anchor) => {
        const row = anchor.closest("div");
        const badge = row?.firstElementChild?.textContent?.trim() ?? "";
        return {
          badge,
          title: ((anchor as HTMLElement).innerText ?? "").trim(),
          href: anchor.getAttribute("href"),
          target: anchor.getAttribute("target"),
        };
      });
    });
  }

  async codeReviewAttach(url: string): Promise<void> {
    const input = this.page.getByPlaceholder("Paste a pull request or merge request URL").first();
    await input.waitFor({ timeout: 15_000 });
    await input.fill(url);
    // Scope to the review form: other widgets (attachments) have their own
    // Attach buttons earlier in the DOM.
    await input.locator("xpath=ancestor::form[1]").getByRole("button", { name: "Attach" }).click({ timeout: 15_000 });
    try {
      await this.page.waitForFunction(
        () => {
          const el = document.querySelector("input[placeholder='Paste a pull request or merge request URL']");
          return el instanceof HTMLInputElement && el.value === "";
        },
        null,
        { timeout: 30_000 }
      );
    } catch {
      const state = await this.page.evaluate(() => {
        const el = document.querySelector("input[placeholder='Paste a pull request or merge request URL']");
        return {
          inputValue: el instanceof HTMLInputElement ? el.value : "<missing>",
          bodyHasError: document.body.innerText.includes("Code review not attached"),
        };
      });
      throw new Error(`[parity] attach of ${url} never cleared the form (${JSON.stringify(state)}).`);
    }
  }

  async codeReviewAttemptAttach(url: string): Promise<void> {
    const input = this.page.getByPlaceholder("Paste a pull request or merge request URL").first();
    await input.waitFor({ timeout: 15_000 });
    await input.fill(url);
    await input.locator("xpath=ancestor::form[1]").getByRole("button", { name: "Attach" }).click({ timeout: 15_000 });
  }

  async codeReviewInputValue(): Promise<string> {
    const input = this.page.getByPlaceholder("Paste a pull request or merge request URL").first();
    await input.waitFor({ timeout: 15_000 });
    return input.inputValue();
  }

  async codeReviewDetach(title: string): Promise<void> {
    const row = this.page
      .locator("div", { hasText: title })
      .filter({ has: this.page.locator('a[target="_blank"]') })
      .last();
    await row.hover({ timeout: 15_000 });
    await row.getByRole("button").first().click({ force: true, timeout: 15_000 });
    await this.page.waitForFunction((text) => !document.body.innerText.includes(text), title, { timeout: 30_000 });
  }

  async worklogCreateVisible(): Promise<boolean> {
    const texts = await this.activityHeaderRow().getByRole("button").allInnerTexts();
    return texts.some((text) => /log/i.test(text));
  }

  // --- Shared property dropdowns (NEWFRONT-122, ISS-207–220). Observed on
  // --- the running old app: each sidebar row is a flex container pairing a
  // --- label span with a value cell holding the combobox trigger button;
  // --- the open popup lives in a body-level portal with an optional search
  // --- input above the option listbox.

  private propertyRowSync(label: string): Locator {
    return this.page.locator("span", { hasText: new RegExp(`^${label}$`) }).locator("xpath=ancestor::div[2]");
  }

  private propertyTrigger(label: string): Locator {
    return this.propertyRowSync(label).locator("div").nth(1).getByRole("button").first();
  }

  private pickerListbox(): Locator {
    // Headless UI renders the option list as a zero-size positioned `ul`
    // wrapper, so callers must wait on attached state or on the options,
    // never on listbox visibility.
    return this.page.getByRole("listbox").last();
  }

  private pickerPopup(): Locator {
    return this.pickerListbox();
  }

  private pickerOptions(): Locator {
    return this.pickerListbox().getByRole("option");
  }

  async propertyValueText(label: string): Promise<string> {
    const trigger = this.propertyTrigger(label);
    await trigger.waitFor({ timeout: 30_000 });
    return (await trigger.innerText()).trim();
  }

  async propertyOpenPicker(label: string): Promise<void> {
    await this.propertyTrigger(label).click({ timeout: 30_000 });
    await this.pickerListbox().waitFor({ state: "attached", timeout: 15_000 });
    await this.pickerOptions().first().waitFor({ timeout: 15_000 });
  }

  async propertyOpenPickerByKeyboard(label: string): Promise<void> {
    const trigger = this.propertyTrigger(label);
    await trigger.focus({ timeout: 30_000 });
    await this.page.keyboard.press("Enter");
    await this.pickerListbox().waitFor({ state: "attached", timeout: 15_000 });
    await this.pickerOptions().first().waitFor({ timeout: 15_000 });
  }

  async propertyPickerDisabled(label: string): Promise<boolean> {
    const trigger = this.propertyTrigger(label);
    await trigger.waitFor({ timeout: 30_000 });
    return trigger.isDisabled();
  }

  async propertyTriggerPresent(label: string): Promise<boolean> {
    return (await this.propertyTrigger(label).count()) > 0;
  }

  async pickerOpen(): Promise<boolean> {
    return (await this.page.getByRole("listbox").count()) > 0;
  }

  async pickerOptionTexts(): Promise<string[]> {
    const texts = await this.pickerListbox().getByRole("option").allInnerTexts();
    return texts.map((t) => t.trim()).filter((t) => t.length > 0);
  }

  async pickerHasSearch(): Promise<boolean> {
    return (await this.pickerPopup().locator("input[type='text']").count()) > 0;
  }

  async pickerSearch(query: string): Promise<void> {
    const input = this.pickerPopup().locator("input[type='text']").first();
    await input.waitFor({ timeout: 15_000 });
    await input.pressSequentially(query, { timeout: 15_000 });
  }

  async pickerSearchValue(): Promise<string> {
    return this.pickerPopup().locator("input[type='text']").first().inputValue();
  }

  async pickerSearchFocused(): Promise<boolean> {
    return this.pickerPopup()
      .locator("input[type='text']")
      .first()
      .evaluate((el) => el === document.activeElement);
  }

  async pickerPick(text: string): Promise<void> {
    await this.pickerListbox().getByRole("option", { name: text }).first().click({ timeout: 15_000 });
  }

  async pickerOptionDisabled(text: string): Promise<boolean> {
    const option = this.pickerListbox().getByRole("option", { name: text }).first();
    await option.waitFor({ timeout: 15_000 });
    const aria = await option.getAttribute("aria-disabled");
    if (aria !== null) return aria === "true";
    const data = await option.getAttribute("data-disabled");
    if (data !== null) return data === "" || data === "true";
    return !(await option.isEnabled());
  }

  async pickerEmptyText(): Promise<string> {
    if ((await this.pickerListbox().getByRole("option").count()) > 0) return "";
    return (await this.pickerPopup().innerText()).trim();
  }

  async pickerPressEscape(): Promise<void> {
    await this.page.keyboard.press("Escape");
  }

  async pickerClickOutside(): Promise<void> {
    await this.page.getByText("Properties", { exact: true }).first().click({ timeout: 30_000 });
  }

  // --- Single-date dropdowns (NEWFRONT-122, ISS-214). Observed on the
  // --- running old app: the calendar popup is a month grid with month/year
  // --- caption dropdowns; current-month days are plain buttons, out-of-range
  // --- days render disabled, and picking a day closes the popup.

  private calendar(): Locator {
    return this.page.locator(".rdp-root").last();
  }

  private calendarDay(day: number): Locator {
    // Outside-month spill days carry the same numbers, so scope to the
    // current month's cells.
    return this.calendar()
      .locator("td:not(.rdp-outside) button", { hasText: new RegExp(`^${day}$`) })
      .first();
  }

  async datePickerOpen(label: string): Promise<void> {
    await this.propertyTrigger(label).click({ timeout: 30_000 });
    await this.calendar().waitFor({ state: "visible", timeout: 15_000 });
  }

  async datePickerVisible(): Promise<boolean> {
    return (await this.page.locator(".rdp-root").count()) > 0;
  }

  async datePickerVisibleMonth(): Promise<{ month: string; year: string }> {
    const selects = this.calendar().locator("select");
    await selects.first().waitFor({ timeout: 15_000 });
    const selected = (select: Locator): Promise<string> => select.locator("option:checked").first().innerText();
    return { month: (await selected(selects.nth(0))).trim(), year: (await selected(selects.nth(1))).trim() };
  }

  async datePickerPickDay(day: number): Promise<void> {
    await this.calendarDay(day).click({ timeout: 15_000 });
    await this.calendar().waitFor({ state: "detached", timeout: 15_000 });
  }

  async datePickerDayDisabled(day: number): Promise<boolean> {
    const button = this.calendarDay(day);
    await button.waitFor({ timeout: 15_000 });
    return button.isDisabled();
  }

  async datePickerPortalAttached(label: string): Promise<boolean> {
    return this.page.evaluate((rowLabel) => {
      const spans = Array.from(document.querySelectorAll("span"));
      const label = spans.find((el) => el.textContent?.trim() === rowLabel);
      const row = label?.parentElement?.parentElement ?? null;
      const calendar = document.querySelector(".rdp-root");
      return row !== null && calendar !== null && !row.contains(calendar);
    }, label);
  }

  async datePickerClear(label: string): Promise<void> {
    const trigger = this.propertyTrigger(label);
    await trigger.hover({ timeout: 30_000 });
    await trigger.locator("svg").first().click({ timeout: 15_000 });
  }

  async propertyRowPresent(label: string): Promise<boolean> {
    // The detail sidebar must hydrate first: an empty DOM reads the same as
    // a missing row, so wait for the Properties heading to settle first.
    await this.page.getByText("Properties", { exact: true }).first().waitFor({ timeout: 60_000 });
    return (await this.page.locator("span", { hasText: new RegExp(`^${label}$`) }).count()) > 0;
  }

  // --- Create-issue modal project picker (NEWFRONT-122, ISS-211). Observed
  // --- on the running old app: the modal form opens under a "Create new
  // --- work item" heading with the project picker as the first button of
  // --- the header row; its popup is a listbox with a "Search" input and
  // --- one option per joined project the user may create in.

  private issueModalForm(): Locator {
    return this.page.getByRole("heading", { name: "Create new work item" }).locator("xpath=ancestor::form[1]");
  }

  private issueModalProjectTrigger(): Locator {
    return this.issueModalForm().locator("h3 + div button").first();
  }

  private issueModalProjectSearchInput(): Locator {
    return this.pickerListbox().getByPlaceholder("Search");
  }

  async issueModalOpenCreate(workspaceSlug: string, projectId: string): Promise<void> {
    await this.openProjectIssues(workspaceSlug, projectId);
    await this.page.getByRole("button", { name: "Add work item" }).click({ timeout: 30_000 });
    await this.page.getByRole("heading", { name: "Create new work item" }).waitFor({ timeout: 30_000 });
    // Hydration gate: the title field autofocuses on mount, so a focused
    // title proves React attached the trigger handlers; clicks before that
    // land on dead DOM under dev-server load.
    await this.page.waitForFunction(
      () => {
        const el = document.querySelector('input[name="name"]');
        return el !== null && document.activeElement === el;
      },
      { timeout: 15_000 }
    );
  }

  async issueModalProjectValue(): Promise<string> {
    const trigger = this.issueModalProjectTrigger();
    await trigger.waitFor({ timeout: 30_000 });
    return (await trigger.innerText()).trim();
  }

  async issueModalProjectOpenPicker(): Promise<void> {
    // Idempotent: Escape-after-search leaves the popup open (it only
    // clears the query), so a step that follows one must not toggle it
    // shut with a blind click.
    if (await this.pickerOpen()) return;
    // The trigger nests two buttons (an inert positioning wrapper around
    // the named inner trigger), so read the current value first and click
    // the inner button by name; the outer wrapper swallows plain clicks.
    const value = await this.issueModalProjectValue();
    const trigger = this.issueModalForm().getByRole("button", { name: value }).last();
    await trigger.click({ timeout: 30_000 });
    const opened = await this.pickerListbox()
      .waitFor({ state: "attached", timeout: 5_000 })
      .then(
        () => true,
        () => false
      );
    if (!opened) {
      // Swallowed click or toggle race under dev-server load: the trigger
      // still has focus, so one more click opens it.
      await trigger.click({ timeout: 30_000 });
      await this.pickerListbox().waitFor({ state: "attached", timeout: 15_000 });
    }
    await this.pickerOptions().first().waitFor({ timeout: 15_000 });
  }

  async issueModalProjectOptionTexts(): Promise<string[]> {
    const texts = await this.pickerOptions().allInnerTexts();
    return texts.map((t) => t.trim()).filter((t) => t.length > 0);
  }

  async issueModalProjectSearch(query: string): Promise<void> {
    const input = this.issueModalProjectSearchInput();
    await input.waitFor({ timeout: 15_000 });
    await input.pressSequentially(query, { timeout: 15_000 });
  }

  async issueModalProjectEmptyText(): Promise<string> {
    if ((await this.pickerOptions().count()) > 0) return "";
    const empty = this.pickerListbox().locator("p").first();
    if ((await empty.count()) === 0) return "";
    return (await empty.innerText()).trim();
  }

  async issueModalProjectPick(text: string): Promise<void> {
    await this.pickerListbox().getByRole("option", { name: text }).first().click({ timeout: 15_000 });
  }

  async issueModalProjectPressEscape(): Promise<void> {
    await this.page.keyboard.press("Escape");
    // Escape in a non-empty search box only clears the query (the popup
    // stays open), so close again when it is still attached after a
    // bounded wait; never press blindly twice — a stray Escape would
    // dismiss the whole create modal.
    const stillOpen = await this.pickerListbox()
      .waitFor({ state: "detached", timeout: 3_000 })
      .then(
        () => false,
        () => true
      );
    if (stillOpen) await this.page.keyboard.press("Escape");
    await this.pickerListbox().waitFor({ state: "detached", timeout: 15_000 });
  }

  async issueModalFillTitle(title: string): Promise<void> {
    await this.issueModalForm().locator('input[name="name"]').fill(title, { timeout: 30_000 });
  }

  async issueModalSubmit(): Promise<void> {
    await this.issueModalForm().getByRole("button", { name: "Save" }).click({ timeout: 30_000 });
    await this.page
      .getByRole("heading", { name: "Create new work item" })
      .waitFor({ state: "hidden", timeout: 30_000 });
  }

  // --- Date-range dropdowns (NEWFRONT-122, ISS-215). Observed on the
  // --- running old app: the list row's merged-dates trigger is the only
  // --- button in a single-issue project's list whose text matches the
  // --- smart label shape; its clear control is the last icon in the
  // --- button. The range calendar reuses the single-date month grid in
  // --- range mode: days stay clickable until both ends are picked.

  private rangeMergedCell(issueName: string): Locator {
    // Two mains render (list + peek shell); the issue's row disambiguates.
    // The trigger nests two buttons (an outer wrapper around the inner
    // trigger); the inner one is the leaf, owns the label and bubbles
    // clicks to the wrapper's handler.
    const list = this.page.locator("main", { has: this.page.locator("p", { hasText: issueName }) });
    return list.locator("button:not(:has(button))", { hasText: /\w{3} \d{1,2} - / });
  }

  async rangeMergedCellText(issueName: string): Promise<string> {
    const cell = this.rangeMergedCell(issueName);
    await cell.waitFor({ timeout: 30_000 });
    return (await cell.innerText()).trim();
  }

  async rangeMergedCellOpen(issueName: string): Promise<void> {
    // Click the label span, not the button box: a box click near the end
    // lands on the clear icon and wipes the dates instead of opening.
    const cell = this.rangeMergedCell(issueName);
    await cell.locator("span").first().click({ timeout: 30_000 });
    await this.calendar().waitFor({ state: "visible", timeout: 15_000 });
  }

  async rangeMergedCellClear(issueName: string): Promise<void> {
    const cell = this.rangeMergedCell(issueName);
    await cell.locator("svg").last().click({ timeout: 30_000 });
  }

  async rangeCalendarVisible(): Promise<boolean> {
    return (await this.page.locator(".rdp-root").count()) > 0;
  }

  async rangeCalendarPickDay(day: number): Promise<void> {
    await this.calendarDay(day).click({ timeout: 15_000 });
  }

  async rangeCalendarDayDisabled(day: number): Promise<boolean> {
    const button = this.calendarDay(day);
    await button.waitFor({ timeout: 15_000 });
    return button.isDisabled();
  }

  async rangeCalendarSelectMonth(monthLabel: string): Promise<void> {
    await this.calendar().locator("select").nth(0).selectOption({ label: monthLabel }, { timeout: 15_000 });
  }

  async rangeCalendarSelectYear(yearLabel: string): Promise<void> {
    await this.calendar().locator("select").nth(1).selectOption({ label: yearLabel }, { timeout: 15_000 });
  }

  private cycleForm(): Locator {
    return this.page.getByRole("heading", { name: "Create cycle" }).locator("xpath=ancestor::form[1]");
  }

  private cycleRangeTrigger(): Locator {
    return this.cycleForm().locator("button:not(:has(button))", { hasText: /Start date/ });
  }

  async cycleCreateOpen(workspaceSlug: string, projectId: string): Promise<void> {
    await this.page.goto(`/${workspaceSlug}/projects/${projectId}/cycles`);
    await this.page.waitForLoadState("domcontentloaded");
    await this.page.getByRole("button", { name: "Add cycle" }).click({ timeout: 30_000 });
    await this.page.getByRole("heading", { name: "Create cycle" }).waitFor({ timeout: 30_000 });
  }

  async cycleFormRangePlaceholders(): Promise<{ from: string; to: string }> {
    const trigger = this.cycleRangeTrigger();
    await trigger.waitFor({ timeout: 30_000 });
    const spans = await trigger.locator("span").allInnerTexts();
    const texts = spans.map((t) => t.trim()).filter((t) => t.length > 0);
    return { from: texts[0] ?? "", to: texts[1] ?? "" };
  }

  async cycleFormRangeOpen(): Promise<void> {
    await this.cycleRangeTrigger().locator("span").first().click({ timeout: 30_000 });
    await this.calendar().waitFor({ state: "visible", timeout: 15_000 });
  }

  async cycleFormFillName(name: string): Promise<void> {
    await this.cycleForm().locator('input[name="name"]').fill(name, { timeout: 30_000 });
  }

  async cycleFormSubmit(): Promise<void> {
    await this.cycleForm().getByRole("button", { name: "Create cycle" }).click({ timeout: 30_000 });
    await this.page.getByRole("heading", { name: "Create cycle" }).waitFor({ state: "hidden", timeout: 30_000 });
  }

  // --- Intake-state dropdown (NEWFRONT-122, ISS-217). Observed on the
  // --- running old app: the intake header's "Add work item" button opens
  // --- the intake-create modal under a "Create intake work item" heading;
  // --- the state picker trigger shows the current intake state name and
  // --- its popup is a searchable single-select listbox over state names.

  private intakeForm(): Locator {
    return this.page.getByRole("heading", { name: "Create intake work item" }).locator("xpath=ancestor::form[1]");
  }

  async intakeCreateOpen(workspaceSlug: string, projectId: string): Promise<void> {
    await this.page.goto(`/${workspaceSlug}/projects/${projectId}/intake`);
    await this.page.waitForLoadState("domcontentloaded");
    await this.page.getByRole("button", { name: "Add work item" }).click({ timeout: 30_000 });
    await this.page.getByRole("heading", { name: "Create intake work item" }).waitFor({ timeout: 30_000 });
  }

  async intakeStateValue(): Promise<string> {
    // The state picker is the first dropdown trigger in the modal's
    // properties row; read it by position, then click it by name.
    const trigger = this.intakeForm().getByRole("button").first();
    await trigger.waitFor({ timeout: 30_000 });
    return (await trigger.innerText()).trim();
  }

  async intakeStateOpenPicker(): Promise<void> {
    // Idempotent like the project picker: Escape-after-search leaves the
    // popup open, so never toggle an open popup shut with a blind click.
    if (await this.pickerOpen()) return;
    const value = await this.intakeStateValue();
    const trigger = this.intakeForm().getByRole("button", { name: value }).first();
    await trigger.click({ timeout: 30_000 });
    const opened = await this.pickerListbox()
      .waitFor({ state: "attached", timeout: 5_000 })
      .then(
        () => true,
        () => false
      );
    if (!opened) {
      await trigger.click({ timeout: 30_000 });
      await this.pickerListbox().waitFor({ state: "attached", timeout: 15_000 });
    }
    await this.pickerOptions().first().waitFor({ timeout: 15_000 });
  }

  async intakeStateOptionTexts(): Promise<string[]> {
    const texts = await this.pickerOptions().allInnerTexts();
    return texts.map((t) => t.trim()).filter((t) => t.length > 0);
  }

  async intakeStateSearch(query: string): Promise<void> {
    const input = this.pickerListbox().getByPlaceholder("Search");
    await input.waitFor({ timeout: 15_000 });
    await input.pressSequentially(query, { timeout: 15_000 });
  }

  async intakeStateEmptyText(): Promise<string> {
    if ((await this.pickerOptions().count()) > 0) return "";
    const empty = this.pickerListbox().locator("p").first();
    if ((await empty.count()) === 0) return "";
    return (await empty.innerText()).trim();
  }

  async intakeStatePick(text: string): Promise<void> {
    await this.pickerListbox().getByRole("option", { name: text }).first().click({ timeout: 15_000 });
  }

  async intakeCreateFillTitle(title: string): Promise<void> {
    await this.intakeForm().locator('input[name="name"]').fill(title, { timeout: 30_000 });
  }

  async intakeCreateSubmit(): Promise<void> {
    await this.intakeForm().getByRole("button", { name: "Create work item" }).click({ timeout: 30_000 });
    await this.page
      .getByRole("heading", { name: "Create intake work item" })
      .waitFor({ state: "hidden", timeout: 30_000 });
  }

  async intakeTriageStateDisabled(): Promise<boolean> {
    const row = this.page
      .locator("span", { hasText: new RegExp("^State$") })
      .first()
      .locator("xpath=ancestor::div[2]");
    const trigger = row.getByRole("button").first();
    await trigger.waitFor({ timeout: 30_000 });
    return trigger.isDisabled();
  }

  // --- Layout dropdown (NEWFRONT-122, ISS-219). Observed on the running
  // --- old app: the views list is empty on a fresh project, offering a
  // --- "Create view" action that opens the view form; the layout picker
  // --- trigger shows the current layout label and its popup lists the
  // --- five layouts with a checkmark on the selected one, no search box.

  private viewsForm(): Locator {
    return this.page.getByRole("heading", { name: "Create View" }).locator("xpath=ancestor::form[1]");
  }

  async viewsOpenList(workspaceSlug: string, projectId: string): Promise<void> {
    await this.page.goto(`/${workspaceSlug}/projects/${projectId}/views`);
    await this.page.waitForLoadState("domcontentloaded");
  }

  async viewsOpenCreate(): Promise<void> {
    await this.page.getByRole("button", { name: "Create view" }).click({ timeout: 30_000 });
    await this.page.getByRole("heading", { name: "Create View" }).waitFor({ timeout: 30_000 });
  }

  async viewsLayoutValue(): Promise<string> {
    const value = await this.viewsForm()
      .getByRole("button", { name: /List|Board|Calendar|Table|Timeline/ })
      .first()
      .innerText();
    return value.trim();
  }

  async viewsLayoutOpenPicker(): Promise<void> {
    // Idempotent like the project picker: never toggle an open popup shut.
    if (await this.pickerOpen()) return;
    const trigger = this.viewsForm()
      .getByRole("button", { name: /List|Board|Calendar|Table|Timeline/ })
      .first();
    await trigger.click({ timeout: 30_000 });
    const opened = await this.pickerListbox()
      .waitFor({ state: "attached", timeout: 5_000 })
      .then(
        () => true,
        () => false
      );
    if (!opened) {
      await trigger.click({ timeout: 30_000 });
      await this.pickerListbox().waitFor({ state: "attached", timeout: 15_000 });
    }
    await this.pickerOptions().first().waitFor({ timeout: 15_000 });
  }

  async viewsLayoutOptionTexts(): Promise<string[]> {
    const texts = await this.pickerOptions().allInnerTexts();
    return texts.map((t) => t.trim()).filter((t) => t.length > 0);
  }

  async viewsLayoutHasSearch(): Promise<boolean> {
    return (await this.pickerPopup().locator("input[type='text']").count()) > 0;
  }

  async viewsLayoutSelectedMarked(text: string): Promise<boolean> {
    const marked = this.pickerListbox().getByRole("option", { name: text, selected: true });
    if ((await marked.count()) === 0) return false;
    await marked.first().waitFor({ timeout: 15_000 });
    return true;
  }

  async viewsLayoutPick(text: string): Promise<void> {
    await this.pickerListbox().getByRole("option", { name: text }).first().click({ timeout: 15_000 });
  }

  async viewsFillName(name: string): Promise<void> {
    await this.viewsForm().locator('input[name="name"]').fill(name, { timeout: 30_000 });
  }

  async viewsSubmit(): Promise<void> {
    await this.viewsForm().getByRole("button", { name: "Create View" }).click({ timeout: 30_000 });
    await this.page.getByRole("heading", { name: "Create View" }).waitFor({ state: "hidden", timeout: 30_000 });
  }

  // --- Edition-only stubs (NEWFRONT-122, ISS-231–237). Locators marked
  // --- UNVERIFIED were written from component structure without a live
  // --- oracle pass (the shared stack was saturated); the oracle run
  // --- resolves them — see the edition-stubs spec.

  private subIssueFilterTrigger(): Locator {
    // The sub-issues widget header holds an icon-only filter trigger whose
    // icon is the small list-filter glyph; match the direct button > div >
    // svg chain so wider ancestor buttons carrying the same icon deeper do
    // not match.
    return this.page.locator('button:has(> div > svg.lucide-list-filter[class*="h-3.5"])');
  }

  private subIssueFilterPanel(): Locator {
    // The FiltersDropdown popover panel wraps its sections in a fixed-width
    // container; only one such panel opens at a time.
    return this.page.locator('div[class*="w-[18.75rem]"]');
  }

  async subIssueFiltersOpen(workspaceSlug: string, projectId: string, parentIssueId: string): Promise<void> {
    await this.openIssueDetail(workspaceSlug, projectId, parentIssueId);
    await this.page.locator("button", { hasText: "Sub-work items" }).first().waitFor({ timeout: 30_000 });
    // Two mains can render the widget; click the first visible trigger.
    const triggers = this.subIssueFilterTrigger();
    const count = await triggers.count();
    for (let i = 0; i < count; i++) {
      const trigger = triggers.nth(i);
      if (await trigger.isVisible().catch(() => false)) {
        await trigger.click({ timeout: 30_000 });
        await this.subIssueFilterPanel().first().waitFor({ timeout: 15_000 });
        return;
      }
    }
    throw new Error("[parity] no visible sub-issue filter trigger.");
  }

  async subIssueFiltersPanelText(): Promise<string> {
    const panel = this.subIssueFilterPanel().first();
    await panel.waitFor({ timeout: 15_000 });
    return ((await panel.innerText().catch(() => "")) || "").replace(/\s+/g, " ").trim();
  }

  private detailIdentifier(): Locator {
    return this.page.getByRole("button", { name: /^[A-Z0-9]+-\d+$/ }).first();
  }

  async detailIdentifierText(): Promise<string> {
    const badge = this.detailIdentifier();
    await badge.waitFor({ timeout: 30_000 });
    return (await badge.innerText()).trim();
  }

  async detailIdentifierCopy(): Promise<void> {
    await this.detailIdentifier().click({ timeout: 30_000 });
  }

  async viewsOpenDetail(workspaceSlug: string, projectId: string, viewId: string): Promise<void> {
    await this.page.goto(`/${workspaceSlug}/projects/${projectId}/views/${viewId}`);
    await this.page.waitForLoadState("domcontentloaded");
  }

  async ganttShowsIssue(issueName: string): Promise<boolean> {
    // UNVERIFIED: gantt blocks print the issue name; settle on it so the
    // dependency absence below is read on a rendered chart, not a loader.
    const block = this.page.getByText(issueName, { exact: true }).first();
    await block.waitFor({ timeout: 60_000 });
    return block.isVisible();
  }

  // --- Label management (NEWFRONT-122, ISS-226–230). The settings rows
  // --- key off the h6 name text: each row's block is the closest
  // --- div.group above it, holding the drag handle (opacity-0 until
  // --- hover, so hover it first), the icon-only ellipsis menu button and
  // --- the trash button (data-ph-element marked). The inline form keys
  // --- off its #labelName input; its error line is the page's only
  // --- danger-colored paragraph while a form error shows.
  private labelHeading(name: string): Locator {
    return this.page.getByRole("heading", { name, exact: true }).first();
  }

  private labelRow(name: string): Locator {
    return this.labelHeading(name).locator("xpath=ancestor::div[contains(@class,'group')][1]");
  }

  private labelForm(): Locator {
    return this.page.locator("#labelName").locator("xpath=ancestor::div[contains(@class,'scroll-m-8')][1]");
  }

  async settingsLabelsOpen(workspaceSlug: string, projectId: string): Promise<void> {
    await this.page.goto(`/${workspaceSlug}/settings/projects/${projectId}/labels`);
    await this.page.waitForLoadState("domcontentloaded");
    // Settles on either the list heading (editors) or the denial view
    // (guests); callers assert which one they got.
    const settled = await Promise.race([
      this.page
        .getByRole("heading", { name: "Labels", exact: true })
        .first()
        .waitFor({ timeout: 60_000 })
        .then(
          () => true,
          () => false
        ),
      this.page
        .getByRole("heading", { name: /not authorized/i })
        .first()
        .waitFor({ timeout: 60_000 })
        .then(
          () => true,
          () => false
        ),
    ]);
    if (!settled) throw new Error("[parity] labels settings settled on neither the list nor the denial view.");
  }

  async settingsLabelsNames(): Promise<string[]> {
    // Group and item names alike render as h6.text-13; the page heading
    // is h3 and other settings pages use h6 for their own titles.
    const heads = this.page.locator("h6.text-13");
    const total = await heads.count();
    const names: string[] = [];
    for (let i = 0; i < total; i++) {
      const text = (
        (await heads
          .nth(i)
          .innerText()
          .catch(() => "")) ?? ""
      ).trim();
      if (text !== "") names.push(text);
    }
    return names;
  }

  async settingsLabelsAddVisible(): Promise<boolean> {
    return this.page.getByRole("button", { name: "Add label", exact: true }).isVisible();
  }

  async settingsLabelsOpenCreate(): Promise<void> {
    await this.page.getByRole("button", { name: "Add label", exact: true }).click({ timeout: 30_000 });
    await this.page.locator("#labelName").waitFor({ timeout: 30_000 });
  }

  async settingsLabelsFormVisible(): Promise<boolean> {
    return (await this.page.locator("#labelName").count()) > 0;
  }

  async settingsLabelsFillName(name: string): Promise<void> {
    await this.page.locator("#labelName").fill(name, { timeout: 30_000 });
  }

  async settingsLabelsFormError(): Promise<string> {
    const err = this.page.locator('p[class*="text-danger-primary"]').first();
    if ((await err.count()) === 0) return "";
    return ((await err.innerText().catch(() => "")) ?? "").trim();
  }

  async settingsLabelsSubmitCreate(): Promise<void> {
    await this.labelForm().getByRole("button", { name: "Add", exact: true }).click({ timeout: 30_000 });
    await this.page.locator("#labelName").waitFor({ state: "detached", timeout: 30_000 });
  }

  async settingsLabelsSubmitUpdate(): Promise<void> {
    await this.labelForm().getByRole("button", { name: "Update", exact: true }).click({ timeout: 30_000 });
    await this.page.locator("#labelName").waitFor({ state: "detached", timeout: 30_000 });
  }

  async settingsLabelsCancelForm(): Promise<void> {
    await this.labelForm().getByRole("button", { name: "Cancel", exact: true }).click({ timeout: 30_000 });
    await this.page.locator("#labelName").waitFor({ state: "detached", timeout: 30_000 });
  }

  async settingsLabelsDotColor(): Promise<string> {
    const dot = this.labelForm().locator("span.h-4.w-4").first();
    await dot.waitFor({ timeout: 30_000 });
    return ((await dot.evaluate((el) => getComputedStyle(el).backgroundColor).catch(() => "")) ?? "").trim();
  }

  async settingsLabelsOpenColorPicker(): Promise<void> {
    await this.labelForm().locator("span.h-4.w-4").first().click({ timeout: 30_000 });
    await this.page.locator(".twitter-picker").waitFor({ timeout: 30_000 });
  }

  async settingsLabelsPickColor(hex: string): Promise<void> {
    const picker = this.page.locator(".twitter-picker");
    await picker.waitFor({ timeout: 30_000 });
    const swatches = picker.locator("[title]");
    const total = await swatches.count();
    const want = hex.toLowerCase();
    for (let i = 0; i < total; i++) {
      const title = ((await swatches.nth(i).getAttribute("title")) ?? "").toLowerCase();
      if (title === want) {
        await swatches.nth(i).click({ timeout: 15_000 });
        return;
      }
    }
    throw new Error(`[parity] no color swatch titled ${hex}.`);
  }

  async settingsLabelsOpenRowMenu(name: string): Promise<void> {
    const row = this.labelRow(name);
    await row.scrollIntoViewIfNeeded();
    await row.hover({ timeout: 30_000 });
    // Lucide 0.469 aliases MoreHorizontal to the ellipsis icon, so the
    // svg carries the new class; match both across icon versions.
    await row.locator("button:has(.lucide-ellipsis, .lucide-more-horizontal)").first().click({ timeout: 30_000 });
    await this.page.getByRole("menuitem").first().waitFor({ timeout: 15_000 });
  }

  async settingsLabelsMenuItems(): Promise<string[]> {
    const items = this.page.getByRole("menuitem");
    const total = await items.count();
    const texts: string[] = [];
    for (let i = 0; i < total; i++) {
      const text = (
        (await items
          .nth(i)
          .innerText()
          .catch(() => "")) ?? ""
      ).trim();
      if (text !== "") texts.push(text);
    }
    return texts;
  }

  async settingsLabelsMenuPick(text: string): Promise<void> {
    await this.page
      .getByRole("menuitem", { name: new RegExp(text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")) })
      .first()
      .click({ timeout: 30_000 });
    await this.page.getByRole("menuitem").first().waitFor({ state: "detached", timeout: 15_000 });
  }

  async settingsLabelsIsGroup(name: string): Promise<boolean> {
    // Only group headers sit under the pointer-cursor Disclosure row;
    // plain items have no such ancestor within a few levels.
    return this.labelHeading(name).evaluate((el) => {
      let parent = el.parentElement;
      for (let i = 0; i < 6 && parent !== null; i++, parent = parent.parentElement) {
        if (parent.className.includes("cursor-pointer")) return true;
      }
      return false;
    });
  }

  async settingsLabelsDeleteViaTrash(name: string): Promise<void> {
    const row = this.labelRow(name);
    await row.scrollIntoViewIfNeeded();
    await row.hover({ timeout: 30_000 });
    await row.locator('button[data-ph-element="labels_delete_button"]').first().click({ timeout: 30_000 });
    await this.page.getByRole("heading", { name: "Delete Label" }).waitFor({ timeout: 30_000 });
  }

  async settingsLabelsDeleteModalText(): Promise<string> {
    const heading = this.page.getByRole("heading", { name: "Delete Label" });
    if ((await heading.count()) === 0) return "";
    const body = heading.first().locator("xpath=following-sibling::div[1]");
    return ((await body.innerText().catch(() => "")) ?? "").replace(/\s+/g, " ").trim();
  }

  async settingsLabelsDeleteConfirm(): Promise<void> {
    const heading = this.page.getByRole("heading", { name: "Delete Label" });
    await heading.waitFor({ timeout: 30_000 });
    const dialog = heading.locator("xpath=ancestor::div[@role='dialog'][1]");
    const confirm = dialog.getByRole("button", { name: "Delete", exact: true });
    if ((await confirm.count()) === 0) {
      await this.page.getByRole("button", { name: "Delete", exact: true }).first().click({ timeout: 30_000 });
    } else {
      await confirm.first().click({ timeout: 30_000 });
    }
    await heading.waitFor({ state: "detached", timeout: 30_000 });
  }

  async settingsLabelsDeleteCancel(): Promise<void> {
    await this.page.getByRole("button", { name: "Cancel", exact: true }).first().click({ timeout: 30_000 });
    await this.page.getByRole("heading", { name: "Delete Label" }).waitFor({ state: "detached", timeout: 15_000 });
  }

  private async dragLabelHandle(source: string, target: string, edge: "center" | "top"): Promise<void> {
    const row = this.labelRow(source);
    await row.scrollIntoViewIfNeeded();
    await row.hover({ timeout: 30_000 });
    // Lucide 0.469 aliases MoreVertical to ellipsis-vertical; match both.
    const handle = row.locator("button:has(.lucide-ellipsis-vertical, .lucide-more-vertical)").first();
    await handle.waitFor({ timeout: 30_000 });
    const from = await handle.boundingBox();
    const targetBox = await this.labelRow(target).boundingBox();
    if (from === null || targetBox === null) throw new Error("[parity] label drag endpoints have no boxes.");
    const start = { x: from.x + from.width / 2, y: from.y + from.height / 2 };
    // Calibrated live: the drop target's top sits ~14px above the row
    // block's top (indicator + margins + borders) and its top quarter
    // (~12px) is the reorder-above zone — so the block's own top edge
    // already falls in the make-child middle. Aim 8px above the block
    // (zone middle); geometry is stable mid-drag since showing the
    // indicator only recolors it.
    const end =
      edge === "center"
        ? { x: targetBox.x + targetBox.width / 2, y: targetBox.y + targetBox.height / 2 }
        : { x: targetBox.x + targetBox.width / 2, y: targetBox.y - 8 };
    await this.page.mouse.move(start.x, start.y);
    await this.page.mouse.down();
    for (let i = 1; i <= 12; i++) {
      await this.page.mouse.move(start.x + ((end.x - start.x) * i) / 12, start.y + ((end.y - start.y) * i) / 12);
    }
    await this.page.waitForTimeout(300);
    await this.page.mouse.up();
  }

  async settingsLabelsDragOnto(source: string, target: string): Promise<void> {
    await this.dragLabelHandle(source, target, "center");
  }

  async settingsLabelsDragAbove(source: string, target: string): Promise<void> {
    await this.dragLabelHandle(source, target, "top");
  }

  async settingsLabelsEmptyTitle(): Promise<string> {
    const empty = this.page.getByText("No labels yet", { exact: true });
    if ((await empty.count()) === 0) return "";
    return (
      (await empty
        .first()
        .innerText()
        .catch(() => "")) ?? ""
    ).trim();
  }

  async settingsLabelsEmptyAction(): Promise<void> {
    await this.page.getByRole("button", { name: "Create your first label", exact: true }).click({ timeout: 30_000 });
    await this.page.locator("#labelName").waitFor({ timeout: 30_000 });
  }

  async settingsLabelsSkeletonVisible(): Promise<boolean> {
    const bones = this.page.locator('[role="status"].animate-pulse > div');
    if ((await bones.count()) < 4) return false;
    return bones.first().isVisible();
  }

  async settingsLabelsDelayLoad(ms: number): Promise<void> {
    // Hold the label-list answers (project and workspace alike, since
    // either one lets the store render) so the skeleton stays up long
    // enough to observe; writes pass through untouched.
    const hold = async (route: Parameters<Parameters<Page["route"]>[1]>[0]) => {
      if (route.request().method() !== "GET") {
        await route.continue();
        return;
      }
      await new Promise((resolve) => setTimeout(resolve, ms));
      await route.continue();
    };
    await this.page.route("**/issue-labels/*", hold);
    await this.page.route("**/api/workspaces/*/labels/", hold);
  }

  async issueLabelsOpenPicker(): Promise<void> {
    await this.propertyRowSync("Labels")
      .getByRole("button", { name: /Add labels/ })
      .first()
      .click({ timeout: 30_000 });
    await this.pickerListbox().waitFor({ state: "attached", timeout: 15_000 });
    await this.pickerOptions().first().waitFor({ timeout: 15_000 });
  }

  async issueLabelsOptionTexts(): Promise<string[]> {
    const options = this.pickerOptions();
    const total = await options.count();
    const texts: string[] = [];
    for (let i = 0; i < total; i++) {
      const text = (
        (await options
          .nth(i)
          .innerText()
          .catch(() => "")) ?? ""
      )
        .replace(/\s+/g, " ")
        .trim();
      if (text !== "") texts.push(text);
    }
    return texts;
  }

  async issueLabelsRowText(): Promise<string> {
    const row = this.propertyRowSync("Labels");
    await row.waitFor({ timeout: 30_000 });
    return ((await row.innerText().catch(() => "")) ?? "").replace(/\s+/g, " ").trim();
  }

  async settingsLabelsNameValue(): Promise<string> {
    return (await this.page.locator("#labelName").inputValue({ timeout: 30_000 })) ?? "";
  }

  async settingsLabelsAttemptSubmit(): Promise<void> {
    // Error paths keep the form open, so this never waits for close.
    const form = this.labelForm();
    const update = form.getByRole("button", { name: "Update", exact: true });
    if ((await update.count()) > 0) {
      await update.first().click({ timeout: 30_000 });
      return;
    }
    await form.getByRole("button", { name: "Add", exact: true }).click({ timeout: 30_000 });
  }

  async settingsLabelsAttemptDeleteConfirm(): Promise<void> {
    // The modal stays open when the delete fails, so this never waits.
    await this.page.getByRole("button", { name: "Delete", exact: true }).first().click({ timeout: 30_000 });
  }

  // --- Cross-cutting (NEWFRONT-122, ISS-221–225). Observed on the running
  // --- old app: the list quick-add trigger is a "New work item" button;
  // --- the detail title is a #title-input textarea when editable and a
  // --- plain div when not; unauthorized settings show a not-authorized
  // --- heading; the cycle page offers "Transfer work items" with a
  // --- search-box modal listing target cycles as buttons.

  async reloadPage(): Promise<void> {
    await this.page.reload();
    await this.page.waitForLoadState("domcontentloaded");
  }

  async projectQuickAddVisible(): Promise<boolean> {
    // The list trigger is a role-less Row div carrying the label text,
    // while the sidebar create buttons (same label, main + peek
    // duplicates) carry a data-ph-element marker: a text match counts
    // only when neither it nor an ancestor carries that marker.
    const matches = this.page.getByRole("main").getByText(/new work item/i);
    const count = await matches.count();
    for (let i = 0; i < count; i++) {
      const marked = await matches
        .nth(i)
        .evaluate((node) => {
          const self = node as HTMLElement;
          if (self.getAttribute("data-ph-element") === "sidebar_create_work_item_button") return true;
          return self.closest('[data-ph-element="sidebar_create_work_item_button"]') !== null;
        })
        .catch(() => true);
      if (!marked) return true;
    }
    return false;
  }

  async issueTitleInputEnabled(): Promise<boolean> {
    return (await this.page.locator("#title-input").count()) > 0;
  }

  async settingsLabelsAccessDenied(): Promise<boolean> {
    return (await this.page.getByRole("heading", { name: /not authorized/i }).count()) > 0;
  }

  async globalViewIssueVisible(name: string): Promise<boolean> {
    // Layout-agnostic (the global view renders spreadsheet, not list):
    // an exact-text match anywhere on the page. Scenario issue names
    // never equal a project name exactly, so sidebar project entries
    // cannot false-positive this.
    return (await this.page.getByText(name, { exact: true }).count()) > 0;
  }

  async openCycleIssues(workspaceSlug: string, projectId: string, cycleId: string): Promise<void> {
    await this.page.goto(`/${workspaceSlug}/projects/${projectId}/cycles/${cycleId}`);
    await this.page.waitForLoadState("domcontentloaded", { timeout: WebDriver.WAIT_MS });
  }

  async cycleTransferButtonVisible(): Promise<boolean> {
    return (await this.page.getByRole("button", { name: "Transfer work items", exact: true }).count()) > 0;
  }

  async cycleTransferOpen(): Promise<void> {
    // The collapsible cycle-details sidebar overlaps the prompt row at
    // desktop viewport, so close it first via its header chevron, the
    // way a user would, before clicking Transfer normally.
    const sidebar = this.page.locator("div.w-\\[21\\.5rem\\]");
    if ((await sidebar.count()) > 0) {
      await sidebar.getByRole("button").first().click({ timeout: 15_000 });
      await sidebar.waitFor({ state: "detached", timeout: 15_000 }).catch(() => {});
    }
    await this.page.getByRole("button", { name: "Transfer work items", exact: true }).click({ timeout: 30_000 });
    await this.page.getByPlaceholder("Search for a cycle...").waitFor({ timeout: 30_000 });
  }

  async cycleTransferOptionNames(): Promise<string[]> {
    const dialog = this.page.getByRole("dialog");
    const scope = (await dialog.count()) > 0 ? dialog : this.page;
    const texts = await scope.getByRole("button").allTextContents();
    return texts.map((t) => t.replace(/\s+/g, " ").trim()).filter((t) => t.length > 0);
  }

  async cycleTransferPick(name: string): Promise<void> {
    const dialog = this.page.getByRole("dialog");
    const scope = (await dialog.count()) > 0 ? dialog : this.page;
    // Option rows append a lowercase status badge to the name
    // ("<name>current"), so match by escaped substring.
    const pattern = new RegExp(name.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"));
    await scope.getByRole("button", { name: pattern }).first().click({ timeout: 30_000 });
    // The modal closes synchronously on pick; the transfer itself lands
    // after. Callers prove the transfer through the toast + server state.
    await this.page.getByPlaceholder("Search for a cycle...").waitFor({ state: "detached", timeout: 30_000 });
  }

  async failNextIssuePatch(status: number, delayMs: number): Promise<void> {
    // Only the next PATCH fails, once: reads and other writes pass
    // through, and the handler removes itself before answering so a
    // retried save goes to the real server. The delay keeps the
    // optimistic value on screen long enough to poll for it. A URL
    // predicate (not a glob) routes the request: the glob form proved
    // flaky against the dev proxy's request URLs.
    const matches = (url: URL): boolean =>
      url.pathname.includes("/api/workspaces/") &&
      url.pathname.includes("/projects/") &&
      url.pathname.includes("/issues/");
    const handler = async (route: Parameters<Parameters<Page["route"]>[1]>[0]) => {
      if (route.request().method() !== "PATCH") {
        await route.continue();
        return;
      }
      // Answer first, unroute after: removing the handler from inside
      // itself finalizes the route and a later fulfill throws
      // "already handled".
      try {
        await new Promise((resolve) => setTimeout(resolve, delayMs));
        await route.fulfill({ status, contentType: "application/json", body: "{}" });
      } finally {
        await this.clearIssuePatchFailure();
      }
    };
    this.patchFailureRoute = { matches, handler };
    await this.page.route(matches, handler);
  }

  async clearIssuePatchFailure(): Promise<void> {
    const current = this.patchFailureRoute;
    this.patchFailureRoute = undefined;
    if (current) await this.page.unroute(current.matches, current.handler).catch(() => {});
  }

  private async waitForSignedOut(): Promise<void> {
    // Every sign-out path lands back on the signed-out entry (sign-in card).
    await this.page.getByPlaceholder("name@company.com").first().waitFor({ timeout: 60_000 });
  }

  private async openAccountMenu(): Promise<void> {
    // The sidebar user menu trigger is the avatar button at the foot of the
    // sidebar. It can sit outside the test viewport, so open it with a click
    // dispatched on the element itself rather than a viewport-gated click.
    const trigger = this.page.locator("aside").getByRole("button").last();
    await trigger.waitFor();
    await trigger.evaluate((el) => (el as HTMLElement).click());
  }

  private async clickAccountMenuItem(name: string): Promise<void> {
    // The floating menu can render where the test viewport cannot reach it
    // (the trigger itself lives outside the viewport), so activate the item
    // on the element itself rather than with a viewport-gated click. The
    // action handler lives on the inner button, not the menuitem wrapper.
    const action = this.page.getByRole("menuitem", { name }).getByRole("button");
    await action.waitFor();
    await action.evaluate((el) => (el as HTMLElement).click());
  }

  async signOutViaAccountMenu(): Promise<void> {
    await this.openAccountMenu();
    await this.clickAccountMenuItem("Sign out");
    await this.waitForSignedOut();
  }

  async signOutViaCommandPalette(): Promise<void> {
    const page = this.page;
    // The palette is a custom power-k panel (not a cmdk dialog): typing
    // happens in the always-rendered top-nav search box, which opens the
    // panel on focus. Click it directly instead of relying on the global
    // shortcut handler, and wait attached (not visible): under a loaded
    // dev server the visibility poll can stall while the node is present.
    const search = page.getByPlaceholder("Search commands...");
    await search.waitFor({ state: "attached", timeout: 120_000 });
    await search.scrollIntoViewIfNeeded();
    // Focus on the element itself and type with the keyboard: click/fill
    // gate on the same visibility poll that stalls under a loaded dev
    // server, while typing reaches the focused input regardless.
    await search.evaluate((el) => (el as HTMLInputElement).focus());
    await page.keyboard.type("Sign out", { delay: 20 });
    const signOut = page
      .locator("[cmdk-item]")
      .filter({ hasText: /sign out/i })
      .first();
    await signOut.waitFor({ state: "attached", timeout: 120_000 });
    await signOut.evaluate((el) => (el as HTMLElement).click());
    await this.waitForSignedOut();
  }

  async isSignedOut(): Promise<boolean> {
    return this.page.getByPlaceholder("name@company.com").first().isVisible();
  }

  async openSwitchAccount(): Promise<void> {
    const page = this.page;
    // The onboarding header names the signed-in account in a dropdown
    // trigger (avatar + display name, which is the email for a fresh
    // account). The menu items mount only once the trigger opens the menu,
    // so wait for the trigger — never the item — then open it. Match on
    // "@": the header is a plain div (no header landmark), and its back
    // button (present from step two on) carries no accessible name.
    const trigger = page.getByRole("button", { name: /@/ }).first();
    await trigger.waitFor();
    await trigger.click();
    await page.getByText("Wrong e-mail address?", { exact: true }).click();
    await page.getByRole("heading", { name: "Switch account" }).waitFor();
  }

  async switchAccountEmail(): Promise<string> {
    // The dialog names the active account in its explanatory copy.
    const dialog = this.page.getByRole("dialog");
    const body = (await dialog.innerText()).trim();
    const match = /[\w.+-]+@[\w-]+\.[\w.]+/.exec(body);
    if (!match) throw new Error("[parity] switch-account dialog names no email.");
    return match[0];
  }

  async confirmSwitchAccount(): Promise<void> {
    await this.page.getByRole("dialog").getByRole("button", { name: "Switch account" }).click();
    await this.waitForSignedOut();
  }

  async openDeactivateAccount(): Promise<void> {
    const page = this.page;
    await this.openAccountMenu();
    await this.clickAccountMenuItem("Settings");
    const deactivate = page.getByRole("button", { name: "Deactivate account" });
    await deactivate.scrollIntoViewIfNeeded();
    await deactivate.click();
    await page.getByRole("heading", { name: "Deactivate your account" }).waitFor();
  }

  async confirmDeactivation(): Promise<void> {
    await this.page.getByRole("dialog").getByRole("button", { name: "Confirm" }).click();
    await this.waitForSignedOut();
  }

  async dropSession(): Promise<void> {
    // Clear every cookie in the browser context: the next authenticated
    // request behaves like an expired session.
    await this.page.context().clearCookies();
  }

  async visit(path: string): Promise<void> {
    await this.page.goto(path);
  }

  async showsText(text: string): Promise<boolean> {
    return this.page.getByText(text, { exact: false }).first().isVisible();
  }

  private deviceCodeField(): Locator {
    // The approval form carries a single code input; it auto-formats XXXX-YYYY.
    return this.page.locator('form input[type="text"]').first();
  }

  async typeDeviceCode(code: string): Promise<void> {
    const field = this.deviceCodeField();
    await field.waitFor();
    await field.fill("");
    await field.pressSequentially(code, { delay: 20 });
  }

  async deviceCodeFieldValue(): Promise<string> {
    const field = this.deviceCodeField();
    await field.waitFor();
    return field.inputValue();
  }

  async submitDeviceApproval(): Promise<void> {
    const form = this.page.locator("form", { has: this.deviceCodeField() });
    await form.getByRole("button", { name: /approve/i }).click();
  }

  async pressKey(key: string): Promise<void> {
    await this.page.keyboard.press(key);
  }

  async typeText(text: string): Promise<void> {
    await this.page.keyboard.type(text);
  }

  async focusedControlName(): Promise<string | null> {
    return this.page.evaluate(() => {
      const active = document.activeElement as HTMLElement | null;
      if (!active) return null;
      const labelled = active.getAttribute("aria-label") ?? active.getAttribute("placeholder");
      if (labelled && labelled.trim()) return labelled.trim();
      const labelledBy = active.getAttribute("aria-labelledby");
      if (labelledBy) {
        const label = document.getElementById(labelledBy)?.textContent?.trim();
        if (label) return label;
      }
      if (active.tagName === "BUTTON") return active.textContent?.trim() || "button";
      if (active.tagName === "BODY") return null;
      return active.tagName.toLowerCase();
    });
  }
}
