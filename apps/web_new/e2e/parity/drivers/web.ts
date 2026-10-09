// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle driver (NEWFRONT-19): implements the parity driver interface
// against apps/web. Selectors follow the sign-in card behavior observed on
// the running old app: the entry route renders the email step, a submit
// moves to the password step, and the password submit posts the native
// form, landing in the workspace. Later oracle issues extend this driver
// (never fork it) as new areas need new actions.
// Shell-chrome extension (NEWFRONT-126): every selector below was observed
// on the running old app, never copied from its sources. Stable landmarks
// come from element ids and accessible names; three icon-only controls
// prefer data-testid hooks when a build carries them and otherwise fall
// back to structural reads (icon glyph, header position) proven
// element-identical on the oracle. Tab reads scope to the nested
// workspace main so sidebar rows and issue rows never leak in.
import { expect, type ElementHandle, type Locator, type Page } from "@playwright/test";
import type {
  AddRunnerFormState,
  AddRunnerRemotePhase,
  AssistantApiCounts,
  AssistantBubble,
  AssistantLandingGreeting,
  AssistantSidebarThread,
  AssistantStreamFrame,
  AssistantToolActivity,
  AutomationCloseRow,
  AutomationMonthModal,
  AutomationRow,
  BoardLayoutKey,
  DevMachineInstallCard,
  DevMachineModal,
  DevMachineRow,
  GanttSidebarRow,
  GanttZoom,
  KanbanCard,
  KanbanColumn,
  LayoutsLayoutKey,
  DocumentShellFacts,
  NotFoundFacts,
  NotificationsAppliedChip,
  NotificationsCard,
  NotificationsEmailPref,
  NotificationsFilterOption,
  NotificationsListQuery,
  NotificationsMode,
  NotificationsOrigin,
  NotificationsTab,
  ParityBrowserCookie,
  ParityDriver,
  ParityTarget,
  PromptEditorState,
  PromptReceiptCard,
  PromptRevertDialog,
  PromptSectionCard,
  RulesCommentMenuOption,
  SchedulerBindingHeader,
  SchedulerBindingRunRow,
  SchedulerBindingValues,
  SchedulerCalendarBlock,
  RunnerChatStreamFrame,
  SchedulerCatalogRow,
  SchedulerDefinitionValues,
  SchedulerInstallOption,
  SchedulerProjectInstallOption,
  SchedulerProjectRow,
  SchedulerScheduleValues,
  ServedShellMarkers,
  WorkspaceOnboardingView,
} from "./parity-driver";

// Placeholder of the command-palette input at root (each sub-page swaps in
// its own); the open chord settles on it to ride out the 200ms close-reset.
const PALETTE_ROOT_PLACEHOLDER = "Type a command or search";

export class WebDriver implements ParityDriver {
  readonly target: ParityTarget = "web";
  readonly page: Page;

  constructor(page: Page) {
    this.page = page;
  }

  // --- Runner-chat oracle state (NEWFRONT-181, RUN-025–032). Per-page
  // --- counters and stub slots backing the API spy, the one-shot
  // --- failure/delay stubs, and the canned SSE streams.
  private runnerChatSpyCounts = {
    warm: 0,
    sessionCreate: 0,
    send: 0,
    cancel: 0,
    close: 0,
    sessionList: 0,
    messageList: 0,
  };

  private runnerChatSessionCreateFailRemaining = 0;
  private runnerChatSessionCreateDelayMs = 0;
  private runnerChatSessionCreateDelayRemaining = 0;
  private runnerChatSendFailRemaining = 0;
  private runnerChatRunnerDetailDelayMs = 0;
  private runnerChatRunnerDetailDelayRemaining = 0;
  private runnerChatRunnerDetailDelayPattern: string | null = null;
  private runnerChatStreamFrames = new Map<string, RunnerChatStreamFrame[]>();
  private runnerChatStreamUrls = new Map<string, string[]>();
  private runnerChatMessageListHoldMs = 0;
  private runnerChatMessageListHoldArmed = false;

  // --- Assistant-chat oracle state (NEWFRONT-187, AGT-038–049/053–057).
  // --- Same shape as the runner-chat slots above: one classifying spy,
  // --- one-shot failure/delay stubs, and canned SSE streams per thread.
  private assistantSpyCounts: AssistantApiCounts = {
    threadCreate: 0,
    send: 0,
    cancel: 0,
    threadList: 0,
    messageList: 0,
  };

  private assistantThreadCreateFailRemaining = 0;
  private assistantThreadCreateDelayMs = 0;
  private assistantThreadCreateDelayRemaining = 0;
  private assistantSendDelayMs = 0;
  private assistantSendDelayRemaining = 0;
  private assistantStreamFrames = new Map<string, AssistantStreamFrame[]>();
  private assistantStreamUrls = new Map<string, string[]>();

  // --- Assistant-plus oracle state (NEWFRONT-188, AGT-050–052/058–061/063).
  // --- Same slot discipline as the c4 state above: one-shot stubs plus
  // --- recorded request shapes per stubbed endpoint.
  private assistantTranscribeTextStub: string | null = null;
  private assistantTranscribeFailure: { status: number; body: Record<string, string> } | null = null;
  private assistantTranscribeSeen: { contentType: string; hasFilePart: boolean; byteLength: number }[] = [];
  private assistantDesktopWatchHandler: ((request: { method(): string; url(): string }) => void) | null = null;
  private assistantDesktopCalls: { method: string; url: string }[] = [];
  private assistantInstanceLlmStub: boolean | null = null;
  private assistantGptAnswerStub: { response: string; response_html: string } | null = null;
  private assistantGptFailure: { status: number; body: Record<string, string> } | null = null;
  private assistantGptSeen: { prompt: string; task: string }[] = [];
  private pageEditorRephraseSeen: string[] = [];

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
    // retried whole until the workspace landing is confirmed. The scratch
    // stack also throttles anonymous calls per host IP, and every agent on
    // this host shares that bucket, so back off quietly between passes.
    for (let attempt = 0; attempt < 3; attempt++) {
      try {
        await this.signInAttempt(email, password);
        return;
      } catch {
        if (attempt === 2) break;
        await this.page.waitForTimeout(25_000);
      }
    }
    try {
      await this.signInAttempt(email, password);
    } catch {
      throw new Error("[parity] sign-in never landed in the workspace.");
    }
  }

  private async signInAttempt(email: string, password: string): Promise<void> {
    const page = this.page;
    if (this.signedInPath(page.url())) return;
    await page.goto("/");
    // Settle for whichever of the three entry states arrives. A retry
    // after a partially completed attempt lands here already signed in
    // (the session cookie survived even though the chrome wait below
    // timed out), so the sign-in card never renders. And when the shared
    // anonymous bucket is empty the app parks on its startup-error page
    // instead of rendering anything. Fail fast on the error page so the
    // retry loop, not a minute-long placeholder wait, spends the budget.
    const emailField = page.getByPlaceholder("name@company.com").first();
    const sidebar = page.locator("#main-sidebar");
    const bootError = page.getByText("didn't start up correctly");
    const deadline = Date.now() + 60_000;
    let settled: "signed-in" | "sign-in-card" | null = null;
    while (Date.now() < deadline) {
      if ((await sidebar.count()) > 0) {
        settled = "signed-in";
        break;
      }
      if ((await bootError.count()) > 0) throw new Error("[parity] oracle boot throttled; retrying.");
      if ((await emailField.count()) > 0) {
        settled = "sign-in-card";
        break;
      }
      await page.waitForTimeout(1_000);
    }
    if (settled === "signed-in") return;
    if (settled === null) throw new Error("[parity] sign-in card never rendered.");
    // The shared email submit waits out a throttled email-check minute
    // (rate-limit banner instead of advancing) and resubmits; a bare
    // fill-and-click here would burn the whole test budget retrying
    // into 429s under concurrent parity runs.
    await this.submitAuthEmail(email);
    const passwordField = page.getByPlaceholder("Enter password");
    await passwordField.waitFor({ timeout: 30_000 });
    await passwordField.fill(password);
    const passwordForm = page.locator("form", { has: passwordField });
    // The old app posts the native form, so this ends in a full page load.
    // Wait for the workspace chrome rather than any URL change: the
    // password-step URL already satisfies a path matcher, so a URL wait
    // would resolve before the login POST answers and the next navigation
    // would cancel it, losing the session. Generous timeout: under shared
    // stack contention the cold boot after login can take a while.
    await Promise.all([
      page.locator("#main-sidebar").waitFor({ state: "attached", timeout: 90_000 }),
      this.submitOf(passwordForm).click(),
    ]);
  }

  private signedInPath(url: string): boolean {
    // A fresh page reports about:blank (pathname "blank"), which the test
    // below would misread as a signed-in workspace path and skip the whole
    // sign-in; only real http(s) URLs can be signed-in paths.
    if (!url.startsWith("http://") && !url.startsWith("https://")) return false;
    const pathname = new URL(url).pathname;
    return pathname !== "/" && !pathname.startsWith("/auth") && !pathname.startsWith("/sign");
  }

  async openProjectIssues(workspaceSlug: string, projectId: string): Promise<void> {
    await this.page.goto(`/${workspaceSlug}/projects/${projectId}/issues`);
    await this.page.waitForLoadState("domcontentloaded");
    // A dead session bounces back to the entry route; fail fast and clearly
    // instead of polling an empty signed-out page to the test timeout.
    if (new URL(this.page.url()).pathname === "/")
      throw new Error("[parity] issues page bounced to sign-in; the session did not survive.");
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

  // Bounded single-node reads for flickering surfaces (the palette remounts
  // mid-read under load). Playwright's getAttribute/textContent/inputValue
  // auto-wait with no action timeout, so a detach between isShown and the read
  // hangs until the TEST timeout. These cap the read at 5s and report a
  // detach as null instead of hanging.
  private async readAttr(locator: Locator, name: string): Promise<string | null> {
    if (!(await this.isShown(locator))) return null;
    try {
      return await locator.first().getAttribute(name, { timeout: 5_000 });
    } catch {
      return null;
    }
  }

  private async readText(locator: Locator): Promise<string | null> {
    if (!(await this.isShown(locator))) return null;
    try {
      const text = await locator.first().textContent({ timeout: 5_000 });
      return text?.replace(/\s+/g, " ").trim() || null;
    } catch {
      return null;
    }
  }

  private async readValue(locator: Locator): Promise<string | null> {
    if (!(await this.isShown(locator))) return null;
    try {
      return await locator.first().inputValue({ timeout: 5_000 });
    } catch {
      return null;
    }
  }

  private async awaitAppBoot(path: string): Promise<void> {
    // Boot retry (NEWFRONT-124): on noisy hosts Chromium intermittently aborts
    // the dev server's route-module fetches (net::ERR_NETWORK_CHANGED) and the
    // React app never boots — the body stays empty past domcontentloaded. When
    // no rendered text appears, reload (up to four times); a healthy load pays
    // only one innerText read.
    for (let attempt = 0; attempt < 5; attempt += 1) {
      try {
        await this.page.waitForFunction(() => document.body.innerText.trim().length > 0, null, { timeout: 15_000 });
        return;
      } catch {
        if (attempt === 4) throw new Error(`[parity] app never booted at ${path} after 5 attempts.`);
        await this.page.reload();
        await this.page.waitForLoadState("domcontentloaded");
      }
    }
  }

  async openAuthenticated(path: string, cookies: ParityBrowserCookie[]): Promise<void> {
    await this.page.context().addCookies(cookies);
    // Phone viewports only: start with the navigation drawer collapsed.
    // The app auto-collapses it below 768px through an effect that the
    // dev oracle double-invokes (StrictMode), toggling it back open, so
    // without this the drawer covers the compact header's controls. The
    // seeded flag is exactly what a returning phone user carries, and
    // desktop contexts never take this branch.
    if ((this.page.viewportSize()?.width ?? 1280) < 768) {
      await this.page.addInitScript(() => window.localStorage.setItem("app_sidebar_collapsed", "true"));
    }
    // domcontentloaded, not load: the dev oracle serves hundreds of
    // unbundled modules, so the load event lands minutes after the app
    // is interactive; every scenario waits explicitly for its own chrome.
    await this.page.goto(path, { waitUntil: "domcontentloaded" });
    await this.page.waitForLoadState("domcontentloaded");
    await this.awaitAppBoot(path);
  }

  // -------------------------------------------------------------------------
  // Command palette / Power-K, search, help, browse, repo-star
  // (NEWFRONT-127, rows SHELL-080, 082, 083, 084, 085, 087, 089, 094, 103, 106).
  // The old app builds the palette on `cmdk` inside a Headless-UI dialog and
  // carries NO data-testid, so selectors target cmdk's own DOM attributes
  // ([cmdk-root]/[cmdk-input]/[cmdk-item]/[cmdk-group-heading]), placeholder
  // text ("Type a command or search"), aria-selected, and visible labels —
  // all user-visible. Derived from a source read of core/components/power-k;
  // the oracle driver is extended here, never forked.
  // -------------------------------------------------------------------------

  /**
   * The centered MODAL palette, scoped to its Headless-UI dialog so its cmdk
   * nodes never collide with the top-bar search box (SHELL-081), which embeds
   * its own cmdk surface inline (not in a dialog).
   */
  private paletteModal(): Locator {
    return this.page
      .getByRole("dialog")
      .filter({ has: this.page.locator("[cmdk-root]") })
      .first();
  }

  /** The cmdk command input of the open modal palette (root or a sub-page). */
  private paletteInput(): Locator {
    return this.paletteModal().locator("[cmdk-input]").first();
  }

  private paletteRoot(): Locator {
    return this.paletteModal().locator("[cmdk-root]").first();
  }

  private paletteItems(): Locator {
    return this.paletteModal().locator("[cmdk-item]");
  }

  async currentUrlPath(): Promise<string> {
    return new URL(this.page.url()).pathname;
  }

  async goToPath(path: string): Promise<void> {
    // Settle on parsed DOM, not full load: late sub-resources (fonts, art)
    // can stall the load event for minutes on a dev server while the app
    // itself is already interactive. Callers wait for their own content.
    await this.page.goto(path, { waitUntil: "domcontentloaded", timeout: 120_000 });
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

  // Shell chrome (NEWFRONT-126). The old app boots its workspace shell from
  // full page loads, and a load occasionally lands on the signed-out entry
  // with a next_path hint before the session read resolves; loads also
  // self-heal back, so navigation retries the load until the sidebar mounts.
  private async gotoShell(url: string): Promise<void> {
    const page = this.page;
    for (let attempt = 0; attempt < 3; attempt++) {
      await page.goto(url);
      await page.waitForLoadState("domcontentloaded");
      try {
        // Attached, not visible: a collapsed sidebar is in the DOM at zero
        // width, and navigation must still count it as mounted.
        await page.locator("#main-sidebar").waitFor({ state: "attached", timeout: 45_000 });
        return;
      } catch {
        if (attempt === 2) throw new Error(`[parity] sidebar never mounted at ${url}.`);
      }
    }
  }

  async openWorkspaceHome(workspaceSlug: string): Promise<void> {
    await this.gotoShell(`/${workspaceSlug}/`);
  }

  async openProjectsList(workspaceSlug: string): Promise<void> {
    await this.gotoShell(`/${workspaceSlug}/projects`);
  }

  async openProjectTab(workspaceSlug: string, projectId: string, tab: string): Promise<void> {
    await this.gotoShell(`/${workspaceSlug}/projects/${projectId}/${tab}`);
  }

  /** The mounted sidebar (the peek twin carries no id, so it never matches). */
  private sidebar(): Locator {
    return this.page.locator("#main-sidebar");
  }

  /** The workspace content main nested inside the page main. */
  private shellMain(): Locator {
    return this.page.locator("main main");
  }

  /** Tab strip region: the bar holding the project switcher and tab links. */
  private tabStrip(): Locator {
    return this.shellMain().first();
  }

  async sidebarPresent(): Promise<boolean> {
    return (await this.sidebar().count()) > 0;
  }

  async sidebarWidth(): Promise<number | null> {
    if ((await this.sidebar().count()) === 0) return null;
    const box = await this.sidebar().boundingBox();
    if (box === null) return null;
    // A collapsed sidebar keeps a one-pixel border in its box; report that
    // as closed so callers read a clean open/closed signal.
    const width = Math.round(box.width);
    return width <= 1 ? 0 : width;
  }

  async portalPresent(): Promise<boolean> {
    return (await this.page.locator("#full-screen-portal").count()) > 0;
  }

  /**
   * The rail renders only when the build enables it, and its settings entry
   * is a link to the workspace settings address that always mounts outside
   * the sidebar in either display mode — no label text or class names needed.
   */
  async railPresent(): Promise<boolean> {
    return await this.page.evaluate(() => {
      const sidebar = document.querySelector("#main-sidebar");
      return [...document.querySelectorAll<HTMLAnchorElement>('a[href$="/settings"]')].some(
        (link) => sidebar === null || !sidebar.contains(link)
      );
    });
  }

  /**
   * Computed left padding of the content holder: the top bar's row sibling
   * holds the optional rail ahead of the content, which always renders last,
   * so climbing from each inbox link to the ancestor whose next sibling
   * contains the page main lands on that holder without class names. The
   * rail branch drops its gutter to zero while the suppressed build keeps
   * the full padding; null when the chrome is absent.
   */
  async contentPaddingLeft(): Promise<number | null> {
    return await this.page.evaluate(() => {
      const mains = [...document.querySelectorAll("main")];
      const inner = mains.length > 1 ? mains[mains.length - 1] : null;
      if (inner === null) return null;
      const inboxes = [...document.querySelectorAll<HTMLAnchorElement>('a[href$="/notifications/"]')];
      for (const inbox of inboxes) {
        let cursor = inbox.parentElement;
        while (cursor !== null) {
          const row = cursor.nextElementSibling;
          if (row !== null && row.contains(inner)) {
            const content = row.lastElementChild;
            if (content === null) return null;
            return Number.parseFloat(getComputedStyle(content).paddingLeft) || 0;
          }
          cursor = cursor.parentElement;
        }
      }
      return null;
    });
  }

  /**
   * Tabs of the strip in order, deduplicated by destination: the strip also
   * renders a hidden measuring copy of every tab for overflow math, and the
   * page content links the same project with trailing slashes, so the first
   * hit per slash-free href is the visible tab. Scoped to the workspace main
   * so sidebar and issue rows stay out.
   */
  async projectTabs(): Promise<Array<{ name: string; href: string }>> {
    const links = this.shellMain().locator('a[href*="/projects/"]:visible');
    const count = await links.count();
    const seen = new Set<string>();
    const tabs: Array<{ name: string; href: string }> = [];
    for (let i = 0; i < count; i++) {
      const href = (await links.nth(i).getAttribute("href")) ?? "";
      const name = ((await links.nth(i).textContent()) ?? "").trim().replace(/\s+/g, " ");
      // Strip destinations never trail a slash; page content links the
      // same project with one, and those content links are never tabs.
      if (href === "" || href.endsWith("/") || name === "") continue;
      const key = href;
      if (seen.has(key)) continue;
      seen.add(key);
      tabs.push({ name, href });
    }
    return tabs;
  }

  /**
   * Name of the visually highlighted tab, read from the underline bar the
   * strip renders ahead of the active entry's content — never computed from
   * the address, so nested routes, detail pages and bare addresses report
   * what the app actually highlights (or null when it highlights nothing).
   */
  async activeTabName(): Promise<string | null> {
    // Document scope, never the shell main: shell-less pages (the not-found
    // page has no nested main) must report null fast instead of hanging an
    // empty locator's auto-wait until the test times out.
    return await this.page.evaluate(() => {
      const links = [...document.querySelectorAll<HTMLAnchorElement>('a[href*="/projects/"]')].filter((a) => {
        if (a.closest(".opacity-0")) return false;
        const raw = a.getAttribute("href") ?? "";
        if (raw.endsWith("/")) return false;
        const parts = new URL(raw, document.baseURI).pathname.split("/").filter((p) => p.length > 0);
        return parts.length === 4 && parts[1] === "projects";
      });
      for (const link of links) {
        let el: HTMLElement | null = link.parentElement;
        for (let depth = 0; depth < 8 && el !== null && el !== document.body; depth++) {
          if (el.tagName === "DIV") {
            const kids = [...el.children];
            const barFirst = kids.length > 0 && kids[0].tagName === "SPAN";
            const bodyFollows = kids.some((k) => k.tagName === "DIV" && k.contains(link));
            if (barFirst && bodyFollows) {
              const name = (link.textContent ?? "").trim().replace(/\s+/g, " ");
              return name === "" ? null : name;
            }
          }
          el = el.parentElement;
        }
      }
      return null;
    });
  }

  async editionBadgePresent(): Promise<boolean> {
    return (await this.page.getByRole("button", { name: "Community" }).count()) > 0;
  }

  async desktopUpdatePresent(): Promise<boolean> {
    return (
      (await this.sidebar()
        .getByRole("button", { name: /update/i })
        .count()) > 0
    );
  }

  async upgradePillCount(): Promise<number> {
    return await this.page.getByText("Pro", { exact: true }).count();
  }

  async topBarControls(): Promise<{
    workspaceMenu: boolean;
    sidebarToggle: boolean;
    search: boolean;
    inbox: boolean;
    help: boolean;
    starLink: boolean;
    accountFallback: boolean;
  }> {
    const page = this.page;
    const starLink = (await page.getByRole("link", { name: "Star us on GitHub" }).count()) > 0;
    const workspaceMenu = (await page.getByRole("button", { name: "Open workspace switcher" }).count()) > 0;
    const sidebarToggle = (await this.sidebarToggleButton().count()) > 0;
    const search = (await page.locator('input[placeholder*="Search" i], input[type="search"]').count()) > 0;
    const inbox = (await page.locator('a[href*="/notifications"]').count()) > 0;
    // The help trigger carries a question-mark icon found nowhere else.
    const help = (await page.locator("button:has(svg.lucide-circle-help)").count()) > 0;
    // The compact account control renders after the star link inside the
    // same action group; the sidebar hosts its own account card outside
    // that group, so the sibling-scoped lookup never confuses the two.
    let accountFallback = false;
    if (starLink) {
      const afterStar = page
        .getByRole("link", { name: "Star us on GitHub" })
        .locator('xpath=following-sibling::*//button[@aria-haspopup="menu"]');
      accountFallback = (await afterStar.count()) > 0;
    }
    return { workspaceMenu, sidebarToggle, search, inbox, help, starLink, accountFallback };
  }

  /**
   * The collapse toggle is an unlabeled icon button carrying the panel-left
   * glyph; prefer the testid when a build carries one, and fall back to the
   * glyph otherwise. Both resolve to the same control.
   */
  private sidebarToggleButton(): Locator {
    return this.page.locator('[data-testid="sidebar-toggle"], button:has(svg.lucide-panel-left)');
  }

  /**
   * The personalize trigger is the unlabeled preferences button heading the
   * sidebar beside the product wordmark: the first control in the sidebar.
   * Prefer the testid when a build carries one, and fall back to that
   * position otherwise.
   */
  private personalizeButton(): Locator {
    return this.sidebar().locator('[data-testid="personalize-nav"], button').first();
  }

  async toggleSidebar(): Promise<void> {
    await this.sidebarToggleButton().first().click();
  }

  async openPersonalizeDialog(): Promise<void> {
    await this.personalizeButton().click();
    await this.page.getByRole("heading", { name: "Customize navigation" }).waitFor({ timeout: 15_000 });
  }

  async personalizeDialogOpen(): Promise<boolean> {
    return (await this.page.getByRole("heading", { name: "Customize navigation" }).count()) > 0;
  }

  private personalItemRow(name: string): Locator {
    const dialog = this.page.locator('[role="dialog"]');
    // Anchor on the row's text label: a bare text match also hits the row
    // containers (their icons carry no text), and climbing from an outer
    // match lands on the first checkbox of the whole list instead of this
    // row's. Each label is unique, so the climb from it reaches this row.
    return dialog.locator(`xpath=.//label[normalize-space(.)="${name}"]/ancestor::div[.//input[@type="checkbox"]][1]`);
  }

  private dialogCheckbox(name: string): Locator {
    return this.personalItemRow(name).locator('xpath=.//input[@type="checkbox"]');
  }

  async movePersonalItem(dragged: string, target: string): Promise<void> {
    const source = this.personalItemRow(dragged);
    const dest = this.personalItemRow(target);
    const box = await dest.boundingBox();
    // The list resolves a drop to the target row's nearest edge, and a
    // drop on the row's vertical center can resolve to its top edge,
    // which computes back to the dragged row's own slot (a no-op move).
    // Land near the target's bottom edge to move past it instead.
    const targetPosition = box === null ? undefined : { x: box.width / 2, y: box.height - 4 };
    await source.dragTo(dest, { targetPosition });
  }

  async personalItemNames(): Promise<string[]> {
    const dialog = this.page.locator('[role="dialog"]');
    const labels = dialog.locator('xpath=.//label[normalize-space(.)="Your work" or normalize-space(.)="Drafts"]');
    const texts = await labels.allTextContents();
    return texts.map((t) => t.trim()).filter((t) => t.length > 0);
  }

  async personalItemChecked(name: string): Promise<boolean | null> {
    const box = this.dialogCheckbox(name);
    if ((await box.count()) === 0) return null;
    return await box.first().isChecked();
  }

  async setPersonalItemEnabled(name: string, enabled: boolean): Promise<void> {
    const box = this.dialogCheckbox(name).first();
    await box.scrollIntoViewIfNeeded();
    const checked = await box.isChecked();
    if (checked !== enabled) await box.click({ force: true });
  }

  async projectNavMode(): Promise<"ACCORDION" | "TABBED" | null> {
    const dialog = this.page.locator('[role="dialog"]');
    if ((await dialog.getByRole("radio", { name: "Accordion sidebar navigation" }).count()) === 0) return null;
    const tabbed = dialog.getByRole("radio", { name: "Tabbed Navigation" });
    return (await tabbed.isChecked()) ? "TABBED" : "ACCORDION";
  }

  async setProjectNavMode(mode: "ACCORDION" | "TABBED"): Promise<void> {
    const dialog = this.page.locator('[role="dialog"]');
    await dialog
      .getByRole("radio", { name: mode === "TABBED" ? "Tabbed Navigation" : "Accordion sidebar navigation" })
      .check({ force: true });
  }

  async projectCapInput(): Promise<string | null> {
    const dialog = this.page.locator('[role="dialog"]');
    const toggle = dialog.getByRole("checkbox", { name: "Show limited projects on sidebar" });
    if ((await toggle.count()) === 0) return null;
    if (!(await toggle.isChecked())) return null;
    const input = dialog.locator('input[type="number"]');
    if ((await input.count()) === 0) return null;
    return await input.first().inputValue();
  }

  async projectCapEnabled(): Promise<boolean | null> {
    const dialog = this.page.locator('[role="dialog"]');
    const toggle = dialog.getByRole("checkbox", { name: "Show limited projects on sidebar" });
    if ((await toggle.count()) === 0) return null;
    return await toggle.isChecked();
  }

  async setProjectCap(enabled: boolean, count?: number): Promise<void> {
    const dialog = this.page.locator('[role="dialog"]');
    const toggle = dialog.getByRole("checkbox", { name: "Show limited projects on sidebar" });
    const checked = await toggle.isChecked();
    if (checked !== enabled) await toggle.click({ force: true });
    if (enabled && count !== undefined) {
      const input = dialog.locator('input[type="number"]').first();
      await input.fill(String(count));
    }
  }

  async projectHeaderText(): Promise<string | null> {
    const switcher = this.shellMain().locator('button[aria-haspopup="listbox"]').first();
    if ((await switcher.count()) === 0) return null;
    return ((await switcher.textContent()) ?? "").trim().replace(/\s+/g, " ") || null;
  }

  /**
   * The header name sits in a width-capped truncating line: when the name
   * is long, its scrollable width exceeds its laid-out width while the
   * computed overflow hides the rest behind an ellipsis.
   */
  async projectHeaderTruncated(): Promise<boolean> {
    // Count-guard first: evaluate on an empty locator auto-waits instead of
    // rejecting, so the catch below never fires and the test would hang.
    const line = this.shellMain().locator('button[aria-haspopup="listbox"] p').first();
    if ((await line.count()) === 0) return false;
    return await line
      .evaluate((el) => {
        const style = getComputedStyle(el);
        return el.scrollWidth > el.clientWidth && style.textOverflow === "ellipsis";
      })
      .catch(() => false);
  }

  async openProjectSwitcher(): Promise<void> {
    await this.shellMain().locator('button[aria-haspopup="listbox"]').first().click();
  }

  async switcherOptionNames(): Promise<string[]> {
    const options = this.page.getByRole("option");
    const texts = await options.allTextContents();
    return texts.map((t) => t.trim().replace(/\s+/g, " ")).filter((t) => t.length > 0);
  }

  async chooseSwitcherOption(name: string): Promise<void> {
    await this.page.getByRole("option", { name }).click();
  }

  async openProjectActions(): Promise<void> {
    // The quick-actions trigger is a span wrapping the horizontal-ellipsis
    // glyph; prefer the testid when a build carries one, and fall back to
    // the glyph otherwise. The overflow trigger is a button, so the span
    // scope never confuses the two.
    await this.shellMain()
      .locator('[data-testid="project-actions-trigger"], span:has(> svg.lucide-ellipsis)')
      .first()
      .click();
  }

  async projectActionNames(): Promise<string[]> {
    const items = this.page.getByRole("menuitem");
    const texts = await items.allTextContents();
    return texts.map((t) => t.trim().replace(/\s+/g, " ")).filter((t) => t.length > 0);
  }

  async clickProjectAction(name: string): Promise<void> {
    await this.page.getByRole("menuitem", { name }).click();
  }

  async readClipboardText(): Promise<string> {
    return await this.page.evaluate(() => navigator.clipboard.readText());
  }

  async toastText(): Promise<string | null> {
    const region = this.page.locator('[aria-label="Notifications"]');
    if ((await region.count()) === 0) return null;
    const text = ((await region.first().textContent()) ?? "").trim().replace(/\s+/g, " ");
    return text === "" ? null : text;
  }

  async rightClickTab(name: string): Promise<void> {
    await this.shellMain().getByRole("link", { name, exact: true }).click({ button: "right" });
  }

  async contextMenuItems(): Promise<string[]> {
    // Radix renders menu items at the document level while the menu is
    // open; closed app menus unmount their items, so the open menu owns
    // every item on the page.
    const texts = await this.page.getByRole("menuitem").allTextContents();
    return texts.map((t) => t.trim().replace(/\s+/g, " ")).filter((t) => t.length > 0);
  }

  async clickContextMenuItem(name: string): Promise<void> {
    await this.page.getByRole("menuitem", { name }).first().click();
  }

  /**
   * The overflow trigger is the horizontal-ellipsis button inside the tab
   * list: the tabs container is the first div after the header switcher
   * whose subtree holds tab links, and the trigger is that container's
   * ellipsis button. Scoping to the container matters because page content
   * below the strip renders its own ellipsis menu buttons (issue rows show
   * them at narrow widths), which a document-wide following search would
   * mistake for the trigger. Only single-segment project destinations count
   * as tabs: page content links the same project with trailing slashes
   * (rejected by the final-character check, since XPath 1.0 has no
   * ends-with) and deeper paths (rejected by the slash count). The svg test
   * uses local-name because a bare `svg` step only matches the null
   * namespace while rendered icons live in the SVG namespace. One locator
   * resolves the trigger directly, so there is no snapshot index to go stale
   * between a read and its click.
   */
  private overflowTrigger(): Locator {
    const tabLink =
      'a[contains(@href,"/projects/")][substring(@href,string-length(@href))!="/"][string-length(@href)-string-length(translate(@href,"/",""))=4][not(ancestor::div[contains(@class,"opacity-0")])]';
    const ellipsisButton =
      'button[.//*[local-name()="svg"][contains(@class,"lucide-ellipsis")]][not(@aria-haspopup="listbox")][not(ancestor::div[contains(@class,"opacity-0")])]';
    return this.tabStrip().locator(
      `xpath=.//button[@aria-haspopup="listbox"]/following::div[.//${tabLink}][not(ancestor-or-self::div[contains(@class,"opacity-0")])][1]//${ellipsisButton}`
    );
  }

  async openOverflowMenu(): Promise<void> {
    const trigger = this.overflowTrigger();
    if ((await trigger.count()) === 0) throw new Error("[parity] no tab overflow trigger on this page.");
    await trigger.click();
  }

  async overflowTriggerPresent(): Promise<boolean> {
    const trigger = this.overflowTrigger();
    if ((await trigger.count()) === 0) return false;
    return await trigger.first().isVisible();
  }

  async overflowRowNames(): Promise<string[]> {
    const menu = this.page.locator('[role="menu"]');
    const texts = await menu
      .getByRole("menuitem")
      .allTextContents()
      .catch(() => [] as string[]);
    return texts.map((t) => t.trim().replace(/\s+/g, " ")).filter((t) => t.length > 0);
  }

  async restoreOverflowTab(name: string): Promise<void> {
    const menu = this.page.locator('[role="menu"]');
    const row = menu.getByRole("menuitem", { name: new RegExp(name.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")) });
    // The restore control only shows on row hover.
    await row.hover();
    await row.getByTitle("Show").click();
  }

  async setViewportSize(width: number, height: number): Promise<void> {
    await this.page.setViewportSize({ width, height });
  }

  async openNotifications(workspaceSlug: string): Promise<void> {
    await this.page.goto(`/${workspaceSlug}/notifications/`);
    await this.page.waitForLoadState("domcontentloaded");
  }

  // --- Sidebar + workspace navigation (NEWFRONT-125, SHELL-046..062). Appended;
  // --- existing methods above are untouched per the shared driver contract.
  private static readonly actionTimeout = 15_000;
  async activateHelpEntry(name: string): Promise<string | null> {
    const menus = this.page.getByRole("menu");
    for (let index = 0; index < (await menus.count()); index += 1) {
      const content = (
        (await menus
          .nth(index)
          .textContent({ timeout: WebDriver.itemTimeout })
          .catch(() => null)) ?? ""
      ).replace(/\s+/g, " ");
      if (!content.includes("Documentation")) continue;
      // Deepest match wins: some entries nest an actionable button inside
      // a menu item wrapper, and clicking the wrapper misses the handler.
      const item = menus.nth(index).locator("button, a, [role='menuitem']").filter({ hasText: name }).last();
      const [popup] = await Promise.all([
        this.page.waitForEvent("popup", { timeout: 10_000 }).catch(() => null),
        item.click({ timeout: WebDriver.actionTimeout }),
      ]);
      await this.page.waitForTimeout(1000);
      if (popup === null) return null;
      // A fresh popup reports about:blank until its first commit; wait for
      // the committed URL (bounded) instead of reading the blank.
      await popup.waitForFunction(() => window.location.href !== "about:blank", { timeout: 10_000 }).catch(() => {});
      return popup.url();
    }
    throw new Error(`[parity] help menu holding ${name} is not open.`);
  }
  async activateUserMenuItem(name: string): Promise<void> {
    // The item wrapper carries the activation (it reacts to the press
    // itself, which a bare synthetic click never produces), so it takes a
    // real pointer click on the outermost match. The menu holding the
    // entry is located by content: another open menu must never receive
    // the activation.
    const menus = this.page.getByRole("menu");
    const count = await menus.count();
    for (let index = 0; index < count; index += 1) {
      const item = menus.nth(index).locator("button, a, [role='menuitem']").filter({ hasText: name }).first();
      if ((await item.count()) === 0) continue;
      await item.click({ timeout: WebDriver.actionTimeout });
      return;
    }
    throw new Error(`[parity] user menu entry ${name} is not open.`);
  }
  private static clean(texts: string[]): string[] {
    return texts.map((t) => t.trim().replace(/\s+/g, " ")).filter((t) => t.length > 0);
  }
  async clickMainContent(): Promise<void> {
    await this.page
      .getByRole("main")
      .first()
      .click({ position: { x: 20, y: 20 }, timeout: WebDriver.actionTimeout });
  }
  private async controlledPanel(aside: Locator, toggle: Locator): Promise<Locator> {
    const controls = await toggle.getAttribute("aria-controls", { timeout: WebDriver.actionTimeout }).catch(() => null);
    if (controls === null) return aside;
    return aside.locator(`div[id="${controls}"]`);
  }
  async dismissTopmost(): Promise<void> {
    await this.page.keyboard.press("Escape");
  }
  async dragSidebarProjectBefore(sourceName: string, targetName: string): Promise<void> {
    // Pragmatic drag-and-drop starts from a handle that only reveals while
    // its own row is hovered. Handles are row-scoped: hovering one row
    // never reveals another row's handle, so the handle lookup stays
    // inside the source row's container.
    // One retry on a fresh lookup: a remount between resolve and hover
    // leaves a detached handle that burns the whole hover timeout.
    let container: Locator | null = null;
    for (let attempt = 1; attempt <= 2; attempt += 1) {
      try {
        const aside = await this.readyAside();
        const sourceRow = aside.getByRole("button").filter({ hasText: sourceName }).first();
        await sourceRow.waitFor({ state: "visible", timeout: 30_000 });
        container = sourceRow.locator("xpath=ancestor::*[contains(@class,'group/project-item')][1]");
        await container.hover({ timeout: WebDriver.actionTimeout });
        break;
      } catch (error) {
        if (attempt === 2) throw error;
        container = null;
      }
    }
    if (container === null) throw new Error(`[parity] drag source ${sourceName} never settled.`);
    const handle = container.locator("button.cursor-grab:visible").first();
    await handle.waitFor({ state: "visible", timeout: WebDriver.actionTimeout });
    const from = (await handle.boundingBox({ timeout: WebDriver.actionTimeout }).catch(() => null)) ?? {
      x: 0,
      y: 0,
      width: 0,
      height: 0,
    };
    const aside = await this.readyAside();
    const targetRow = aside.getByRole("button").filter({ hasText: targetName }).first();
    await targetRow.waitFor({ state: "visible", timeout: 30_000 });
    const to = (await targetRow.boundingBox({ timeout: WebDriver.actionTimeout }).catch(() => null)) ?? {
      x: 0,
      y: 0,
      width: 0,
      height: 0,
    };
    await this.page.mouse.move(from.x + from.width / 2, from.y + from.height / 2);
    await this.page.mouse.down();
    await this.page.mouse.move(to.x + to.width / 2, to.y + 2, { steps: 25 });
    await this.page.mouse.up();
  }
  async favoriteEntryNames(): Promise<string[]> {
    // Folders render as buttons and entries as links inside the favorites
    // group; header chrome (the toggle, the folder-create button) is
    // excluded so only entries come back.
    if (!(await this.isFavoritesOpen())) return [];
    const aside = await this.screenAside();
    const heading = aside.getByText("Favorites", { exact: true }).first();
    const group = heading.locator("xpath=ancestor::div[2]");
    const names = WebDriver.clean(await group.locator("a, button").allTextContents());
    const chrome = new Set(["Favorites", "Open favorites menu", "Close favorites menu", "Create favorites folder"]);
    return names.filter((name) => !chrome.has(name));
  }
  private async favoritesToggle(): Promise<Locator> {
    const aside = await this.screenAside();
    return aside.getByRole("button", { name: /favorites menu/i }).first();
  }
  private async folderNameField(): Promise<Locator> {
    const aside = await this.screenAside();
    return aside.getByPlaceholder("New folder").first();
  }
  private async hasPageShell(timeout: number): Promise<boolean> {
    try {
      await this.page.waitForFunction(() => document.querySelectorAll("aside, main").length > 0, { timeout });
      return true;
    } catch {
      return false;
    }
  }
  async helpMenuTexts(): Promise<string[]> {
    const menus = this.page.getByRole("menu");
    for (let index = 0; index < (await menus.count()); index += 1) {
      const content = (
        (await menus
          .nth(index)
          .textContent({ timeout: WebDriver.itemTimeout })
          .catch(() => null)) ?? ""
      ).replace(/\s+/g, " ");
      if (content.includes("Documentation")) {
        // The version footer is not a button, so include the whole menu
        // text alongside the actionable items.
        const full = content.length > 0 ? [content] : [];
        const items = WebDriver.clean(await menus.nth(index).locator("button, a, [role='menuitem']").allTextContents());
        return [...full, ...items];
      }
    }
    return [];
  }
  async isCreateProjectVisible(): Promise<boolean> {
    const aside = await this.screenAside();
    // The creation button reveals on group hover, so hover the group header
    // before reading it, exactly like a user would.
    await aside
      .getByRole("button", { name: /projects menu/i })
      .first()
      .hover()
      .catch(() => {});
    const button = aside.getByRole("button", { name: "Create new project" });
    return (await button.count()) > 0 && (await button.first().isVisible());
  }
  async isDialogWithTextVisible(text: string): Promise<boolean> {
    const dialog = this.page.getByRole("dialog").filter({ hasText: text });
    if ((await dialog.count()) === 0) return false;
    return this.isTextVisibleInDialog(dialog.first(), text);
  }
  async isFavoritesFolderDialogOpen(): Promise<boolean> {
    // The folder form renders inline in the sidebar, not as a dialog: its
    // name field is the presence marker.
    const field = await this.folderNameField();
    if ((await field.count()) === 0) return false;
    return field
      .first()
      .isVisible()
      .catch(() => false);
  }
  async isFavoritesOpen(): Promise<boolean> {
    if (!(await this.sidebarSectionNames()).includes("Favorites")) return false;
    const toggle = await this.favoritesToggle();
    if ((await toggle.count()) === 0) return false;
    const label = await toggle
      .first()
      .getAttribute("aria-label", { timeout: WebDriver.actionTimeout })
      .catch(() => null);
    return (label ?? "").startsWith("Close");
  }
  async isMoreSectionOpen(): Promise<boolean> {
    // Two open paths exist on the running old app: the toggle flips the
    // disclosure (aria-expanded follows), while landing on a member route
    // flips the stored flag behind its back — the panel renders with no
    // aria change. Either one counts as open.
    const aside = await this.screenAside();
    const toggle = await this.moreToggle();
    if ((await toggle.count()) === 0) return false;
    const expanded = await toggle
      .first()
      .getAttribute("aria-expanded", { timeout: WebDriver.actionTimeout })
      .catch(() => null);
    if ((expanded ?? "") === "true") return true;
    const panel = await this.controlledPanel(aside, toggle.first());
    if (panel === aside) return false;
    return (await panel.locator("a").count()) > 0;
  }
  async isOverflowCreateVisible(): Promise<boolean> {
    // Buttons outside the sidebar belong to slide-overs and dialogs; the
    // top bar carries no creation button.
    const candidates = this.page.getByRole("button", { name: /create/i });
    const count = await candidates.count();
    for (let index = 0; index < count; index += 1) {
      const outside = await candidates.nth(index).evaluate((el) => el.closest("aside") === null);
      if (outside && (await candidates.nth(index).isVisible())) return true;
    }
    return false;
  }
  async isOverflowEmptyStateVisible(): Promise<boolean> {
    const empty = this.page.getByText("No matching results.", { exact: false });
    return (await empty.count()) > 0 && (await empty.first().isVisible());
  }
  private async isPageSettled(timeout: number): Promise<boolean> {
    try {
      await this.page.waitForFunction(
        () =>
          document.querySelectorAll("aside a").length > 0 ||
          document.querySelectorAll("main p, main h1, main a, main button").length > 0,
        { timeout }
      );
      return true;
    } catch {
      return false;
    }
  }
  async isProjectRowInViewport(projectName: string): Promise<boolean> {
    const aside = await this.screenAside();
    const link = aside.locator("a").filter({ hasText: projectName }).first();
    if ((await link.count()) === 0) return false;
    const box = await link.boundingBox({ timeout: WebDriver.actionTimeout }).catch(() => null);
    if (box === null) return false;
    const viewport = this.page.viewportSize() ?? { width: 1280, height: 720 };
    return box.y >= 0 && box.y + box.height <= viewport.height && box.x >= 0 && box.x + box.width <= viewport.width;
  }
  async isProjectRowOpen(projectName: string): Promise<boolean> {
    // Two expansion paths exist on the running old app: the toggle sets
    // aria-expanded, while landing on a project route renders the
    // sub-navigation without flipping the toggle, so either one counts.
    // The sub-navigation renders as siblings AFTER the row container (which
    // holds only the bare row link), so the fallback resolves the row's
    // project id from its own link and looks for that project's subnav
    // hrefs across the sidebar — never inside the row container alone.
    const toggle = await this.projectRowToggle(projectName);
    if ((await toggle.count()) === 0) return false;
    const expanded =
      ((await toggle
        .first()
        .getAttribute("aria-expanded", { timeout: WebDriver.actionTimeout })
        .catch(() => null)) ?? "") === "true";
    if (expanded) return true;
    const rowHref = await this.projectRowHref(projectName);
    const idMatch = rowHref === null ? null : /\/projects\/([^/]+)\//.exec(`${rowHref}/`);
    if (idMatch === null) return false;
    const prefix = `/projects/${idMatch[1]}/`;
    const aside = await this.screenAside();
    const links = aside.locator("a");
    const count = await links.count();
    for (let index = 0; index < count; index += 1) {
      const href = await links
        .nth(index)
        .getAttribute("href", { timeout: WebDriver.itemTimeout })
        .catch(() => null);
      if (
        href !== null &&
        href.startsWith(prefix) &&
        /\/projects\/[^/]+\/(issues|pages|intake|schedulers|runners|cycles|modules|views)\//.test(href)
      ) {
        return true;
      }
    }
    return false;
  }
  async isProjectsGroupOpen(): Promise<boolean> {
    const toggle = await this.projectsGroupToggle();
    if ((await toggle.count()) === 0) return false;
    const label = await toggle
      .first()
      .getAttribute("aria-label", { timeout: WebDriver.actionTimeout })
      .catch(() => null);
    return (label ?? "").startsWith("Close");
  }
  async isProjectsOverflowOpen(): Promise<boolean> {
    const toggle = await this.overflowToggle();
    if ((await toggle.count()) === 0) return false;
    const label = await toggle
      .first()
      .getAttribute("aria-label", { timeout: WebDriver.actionTimeout })
      .catch(() => null);
    return (label ?? "").startsWith("Close");
  }
  async isProjectsOverflowVisible(): Promise<boolean> {
    const toggle = await this.overflowToggle();
    return (await toggle.count()) > 0 && (await toggle.first().isVisible());
  }
  async isQuickCreateDialogOpen(): Promise<boolean> {
    const dialog = this.page.getByRole("dialog").filter({ hasText: "Create new work item" });
    if ((await dialog.count()) === 0) return false;
    return this.isTextVisibleInDialog(dialog.first(), "Create new work item");
  }
  async isQuickCreateEnabled(): Promise<boolean> {
    const aside = await this.screenAside();
    const button = aside.getByRole("button", { name: "New work item" }).first();
    if ((await button.count()) === 0) return false;
    return button.isEnabled({ timeout: WebDriver.actionTimeout }).catch(() => false);
  }
  async isSidebarOnScreen(): Promise<boolean> {
    await this.page
      .getByRole("main")
      .first()
      .waitFor({ state: "attached", timeout: 60_000 })
      .catch(() => {});
    const boxes = await this.page
      .locator("aside")
      .evaluateAll((elements) =>
        elements.map((element) => {
          const box = element.getBoundingClientRect();
          return [box.x, box.width, box.height];
        })
      )
      .catch((): number[][] => []);
    return boxes.some(([x, width, height]) => x >= 0 && width > 50 && height > 50);
  }
  private async isTextVisibleInDialog(dialog: Locator, text: string): Promise<boolean> {
    // The dialog root is a zero-height positioning wrapper even while the
    // modal shows, so visibility is read off the content, not the wrapper
    // (proven against the running old app). Any visible match counts: a
    // hidden duplicate (inactive tab, a11y copy) must not shadow it.
    const matches = dialog.getByText(text);
    const count = await matches.count();
    for (let index = 0; index < count; index += 1) {
      if (
        await matches
          .nth(index)
          .isVisible()
          .catch(() => false)
      )
        return true;
    }
    return false;
  }
  async isToastVisible(text: string): Promise<boolean> {
    const toast = this.page.getByText(text, { exact: false });
    return (await toast.count()) > 0 && (await toast.first().isVisible());
  }
  private static readonly itemTimeout = 5_000;
  async moreSectionLinks(): Promise<{ text: string; href: string | null }[]> {
    const aside = await this.screenAside();
    const panel = await this.controlledPanel(aside, await this.moreToggle());
    const links = panel.locator("a");
    const texts = WebDriver.clean(await links.allTextContents());
    const hrefs: (string | null)[] = [];
    for (let index = 0; index < (await links.count()); index += 1) {
      hrefs.push(
        await links
          .nth(index)
          .getAttribute("href", { timeout: WebDriver.itemTimeout })
          .catch(() => null)
      );
    }
    return texts.map((text, index) => ({ text, href: hrefs[index] ?? null }));
  }
  private async moreToggle(): Promise<Locator> {
    const aside = await this.screenAside();
    return aside.getByRole("button", { name: "More" }).first();
  }
  async openFavoriteEntry(name: string): Promise<void> {
    const aside = await this.readyAside();
    const entry = aside.locator("a").filter({ hasText: name }).first();
    await entry.waitFor({ state: "visible", timeout: 30_000 });
    await entry.click({ timeout: WebDriver.actionTimeout });
  }
  async openFavoritesFolder(name: string): Promise<void> {
    const aside = await this.readyAside();
    await aside.getByRole("button", { name }).first().click({ timeout: WebDriver.actionTimeout });
  }
  async openFavoritesFolderDialog(): Promise<void> {
    // The create button reveals on group hover, so hover the header first
    // like a user, then open from the keyboard exactly like one. Retry
    // once: the hover-reveal can miss on a slow first paint.
    for (let attempt = 1; attempt <= 2; attempt += 1) {
      const aside = await this.readyAside();
      const header = aside.getByRole("button", { name: /favorites menu/i }).first();
      await header.hover({ timeout: WebDriver.actionTimeout }).catch(() => {});
      const button = aside.getByRole("button", { name: "Create favorites folder" }).first();
      await button.focus({ timeout: WebDriver.actionTimeout }).catch(() => {});
      await this.page.keyboard.press("Enter");
      await this.page.waitForTimeout(1500);
      if (await this.isFavoritesFolderDialogOpen()) return;
    }
  }
  async openHelpMenu(): Promise<void> {
    // Observed on the running old app: the help entry is an icon-only
    // top-bar button with no accessible name, so try each icon-only
    // top-bar button until the help menu opens. Rightmost first: the help
    // entry sits at the far end, while the leftmost candidate is the
    // sidebar collapse toggle, which must not be disturbed.
    const buttons = this.page.locator("button");
    const count = await buttons.count();
    const candidates: { index: number; x: number }[] = [];
    for (let index = 0; index < count; index += 1) {
      const box = await buttons
        .nth(index)
        .boundingBox({ timeout: WebDriver.itemTimeout })
        .catch(() => null);
      if (box === null || box.width === 0 || box.y >= 41 || box.x < 0) continue;
      const aria = await buttons
        .nth(index)
        .getAttribute("aria-label", { timeout: WebDriver.itemTimeout })
        .catch(() => null);
      if (aria !== null && aria.length > 0) continue;
      const text = (
        (await buttons
          .nth(index)
          .textContent({ timeout: WebDriver.itemTimeout })
          .catch(() => null)) ?? ""
      ).trim();
      if (text.length > 0) continue;
      candidates.push({ index, x: box.x });
    }
    candidates.sort((left, right) => right.x - left.x);
    for (const { index } of candidates) {
      await buttons
        .nth(index)
        .click({ timeout: WebDriver.actionTimeout })
        .catch(() => {});
      await this.page.waitForTimeout(800);
      const menus = this.page.getByRole("menu");
      for (let mi = 0; mi < (await menus.count()); mi += 1) {
        const content = (
          (await menus
            .nth(mi)
            .textContent({ timeout: WebDriver.itemTimeout })
            .catch(() => null)) ?? ""
        ).replace(/\s+/g, " ");
        if (content.includes("Documentation")) return;
      }
      await this.dismissTopmost();
    }
    throw new Error("[parity] help menu button not found in the top bar.");
  }
  async openProjectQuickMenu(projectName: string): Promise<void> {
    // The toggle is hover-revealed per row: hovering the row exposes its
    // own menu toggle, and a plain click opens the menu (a keyboard Enter
    // demonstrably does nothing). Verify and drive once more when a remount
    // swallows the click. Accordion rows expose the name on a disclosure
    // button; tabbed rows render it in a div inside the row link — hover
    // whichever shape is present.
    const aside = await this.readyAside();
    const rowButton = aside.getByRole("button").filter({ hasText: projectName }).first();
    const rowLink = aside.locator("a").filter({ hasText: projectName }).first();
    let row = rowLink;
    const deadline = Date.now() + 30_000;
    for (;;) {
      if ((await rowButton.count()) > 0) {
        row = rowButton;
        break;
      }
      if ((await rowLink.count()) > 0) {
        row = rowLink;
        break;
      }
      if (Date.now() > deadline) break;
      await this.page.waitForTimeout(500);
    }
    await row.waitFor({ state: "visible", timeout: 30_000 });
    const container = row.locator("xpath=ancestor::*[contains(@class,'group/project-item')][1]");
    for (let attempt = 1; attempt <= 2; attempt += 1) {
      await row.hover({ timeout: WebDriver.actionTimeout }).catch(() => {});
      const toggle = container.getByRole("button", { name: "Toggle quick actions menu" }).first();
      await toggle.click({ timeout: WebDriver.actionTimeout }).catch(() => {});
      await this.page.waitForTimeout(2000);
      if ((await this.projectQuickMenuTexts()).length > 0) return;
    }
  }
  async openSidebarLink(text: string): Promise<void> {
    const aside = await this.readyAside();
    await aside.locator("a").filter({ hasText: text }).first().click({ timeout: WebDriver.actionTimeout });
  }
  async openUserMenu(): Promise<void> {
    const aside = await this.readyAside();
    const email = (await aside.textContent({ timeout: WebDriver.actionTimeout }).catch(() => null))?.match(
      /[\w.+-]+@[\w-]+\.[\w.]+/
    );
    if (email === null || email === undefined) throw new Error("[parity] user card email not found in sidebar.");
    await aside.getByRole("button").filter({ hasText: email[0] }).first().click({ timeout: WebDriver.actionTimeout });
  }
  async openWorkspacePath(path: string): Promise<void> {
    // The oracle is a dev server: under sibling load a navigation can stay
    // half-loaded for a minute (the document arrives but the app hydrates
    // slowly), or never hydrate at all after a rebuild. Slow pages keep
    // their progress and get more time; only a page with no shell at all
    // earns a reload, exactly like a user refreshing a blank page.
    for (let attempt = 1; attempt <= 2; attempt += 1) {
      try {
        await this.page.goto(path, { timeout: 60_000 });
        await this.page.waitForLoadState("domcontentloaded", { timeout: 60_000 });
      } catch (error) {
        if (attempt === 2) throw error;
        continue;
      }
      if (await this.isPageSettled(45_000)) return;
      if (await this.hasPageShell(10_000)) {
        await this.isPageSettled(45_000);
        return;
      }
      await this.page.reload({ timeout: 60_000 }).catch(() => {});
      await this.isPageSettled(20_000);
      return;
    }
  }
  async openWorkspaceSwitcher(): Promise<void> {
    // Observed on the running old app: the switcher button lives in the
    // top bar and carries the workspace identity mark.
    await this.page
      .getByRole("button", { name: "Open workspace switcher" })
      .first()
      .click({ timeout: WebDriver.actionTimeout });
  }
  private async overflowPanelData(): Promise<{ names: string[]; text: string }> {
    return this.page.evaluate(() => {
      const clean = (value: string): string => value.trim().replace(/\s+/g, " ");
      const inputs = Array.from(document.querySelectorAll("input[placeholder='Search']"));
      let best: Element | null = null;
      let bestArea = Number.POSITIVE_INFINITY;
      for (const input of inputs) {
        let el: Element | null = input.parentElement;
        while (el !== null) {
          const box = el.getBoundingClientRect();
          if (box.x >= 200 && box.width > 100 && box.height > 100) {
            const area = box.width * box.height;
            if (area < bestArea) {
              bestArea = area;
              best = el;
            }
            break;
          }
          el = el.parentElement;
        }
      }
      if (best === null) return { names: [], text: "" };
      const names = Array.from(best.querySelectorAll("a"))
        .map((anchor) => clean(anchor.textContent ?? ""))
        .filter((name) => name.length > 0);
      return { names, text: clean(best.textContent ?? "") };
    });
  }
  async overflowProjectNames(): Promise<string[]> {
    return (await this.overflowPanelData()).names;
  }
  private async overflowToggle(): Promise<Locator> {
    const aside = await this.screenAside();
    return aside.locator("#extended-project-sidebar-toggle");
  }
  async projectQuickMenuTexts(): Promise<string[]> {
    const texts: string[] = [];
    for (const role of ["menu", "menuitem", "dialog"] as const) {
      const els = this.page.getByRole(role);
      const count = Math.min(await els.count(), 4);
      for (let index = 0; index < count; index += 1) {
        texts.push(
          (
            (await els
              .nth(index)
              .textContent({ timeout: WebDriver.itemTimeout })
              .catch(() => null)) ?? ""
          )
            .trim()
            .replace(/\s+/g, " ")
        );
      }
    }
    const portals = this.page.locator("div[data-headlessui-portal]");
    if ((await portals.count()) > 0) {
      texts.push(...WebDriver.clean(await portals.first().locator("button").allTextContents()));
    }
    return texts.filter((t) => t.length > 0);
  }
  async projectRowHref(projectName: string): Promise<string | null> {
    const aside = await this.screenAside();
    const link = aside.locator("a").filter({ hasText: projectName }).first();
    if ((await link.count()) === 0) return null;
    return link.getAttribute("href", { timeout: WebDriver.actionTimeout }).catch(() => null);
  }
  private async projectRowToggle(projectName: string): Promise<Locator> {
    const aside = await this.screenAside();
    return aside.getByRole("button").filter({ hasText: projectName }).first();
  }
  async projectSubnavLinks(): Promise<{ text: string; href: string | null }[]> {
    // Sub-navigation entries point at a single project's feature routes,
    // which aggregate rows never do; dedupe the group/project nesting.
    const aside = await this.screenAside();
    const links = aside.locator("a");
    const count = await links.count();
    const rows: { text: string; href: string | null }[] = [];
    const seen = new Set<string>();
    for (let index = 0; index < count; index += 1) {
      const href = await links
        .nth(index)
        .getAttribute("href", { timeout: WebDriver.itemTimeout })
        .catch(() => null);
      // The trailing slash separates sub-navigation entries from the bare
      // project-row link, which ends at the feature without one.
      if (
        href === null ||
        !/\/projects\/[^/]+\/(issues|pages|intake|schedulers|runners|cycles|modules|views)\//.test(href)
      ) {
        continue;
      }
      if (seen.has(href)) continue;
      seen.add(href);
      const text = (
        (await links
          .nth(index)
          .textContent({ timeout: WebDriver.itemTimeout })
          .catch(() => null)) ?? ""
      )
        .trim()
        .replace(/\s+/g, " ");
      if (text.length > 0) rows.push({ text, href });
    }
    return rows;
  }
  private async projectsGroupToggle(): Promise<Locator> {
    const aside = await this.screenAside();
    return aside.getByRole("button", { name: /projects menu/i }).first();
  }
  private async readyAside(): Promise<Locator> {
    try {
      await this.page.waitForFunction(
        () =>
          Array.from(document.querySelectorAll("aside")).some((el) => {
            const box = el.getBoundingClientRect();
            return box.x >= 0 && box.width > 50 && box.height > 50;
          }),
        { timeout: 12_000 }
      );
    } catch {
      // Fall through to whatever the page has; the click timeout decides.
    }
    return this.screenAside();
  }
  async resetSession(): Promise<void> {
    await this.page.context().clearCookies();
    await this.page.goto("/", { waitUntil: "domcontentloaded", timeout: 60_000 });
  }
  private async screenAside(): Promise<Locator> {
    // A single non-waiting read: boundingBox performs actionability waits
    // that serialize every sidebar read under remount churn, so measure
    // with getBoundingClientRect instead, which never waits.
    const asides = this.page.locator("aside");
    const boxes = await asides
      .evaluateAll((elements) =>
        elements.map((element) => {
          const box = element.getBoundingClientRect();
          return [box.x, box.width, box.height];
        })
      )
      .catch((): number[][] => []);
    for (let index = 0; index < boxes.length; index += 1) {
      const [x, width, height] = boxes[index];
      if (x >= 0 && width > 50 && height > 50) return asides.nth(index);
    }
    return asides.first();
  }
  async searchOverflowProjects(query: string): Promise<void> {
    // Exact match: the top bar carries a longer "Search commands…" sibling.
    await this.page
      .getByPlaceholder("Search", { exact: true })
      .first()
      .fill(query, { timeout: WebDriver.actionTimeout });
  }
  async setFavoritesOpen(open: boolean): Promise<void> {
    if ((await this.isFavoritesOpen()) !== open) {
      const aside = await this.readyAside();
      await aside
        .getByRole("button", { name: /favorites menu/i })
        .first()
        .click({ timeout: WebDriver.actionTimeout });
    }
  }
  async setMoreSectionOpen(open: boolean): Promise<void> {
    // The panel mounts asynchronously after the click: returning early lets
    // the next panel read race the render (no toggle association yet) and
    // fall back to the whole sidebar. Verify the end state and re-drive.
    for (let attempt = 0; attempt < 3; attempt += 1) {
      if ((await this.isMoreSectionOpen()) !== open) {
        const aside = await this.readyAside();
        await aside.getByRole("button", { name: "More" }).first().click({ timeout: WebDriver.actionTimeout });
        continue;
      }
      if (!open) return;
      const toggle = await this.moreToggle();
      const expanded = await toggle
        .first()
        .getAttribute("aria-expanded", { timeout: WebDriver.actionTimeout })
        .catch(() => null);
      // Route-driven opens render the panel with no toggle change: panel
      // reads already resolve, nothing to wait for.
      if ((expanded ?? "") !== "true") return;
      const deadline = Date.now() + 10_000;
      for (;;) {
        const controls = await toggle
          .first()
          .getAttribute("aria-controls", { timeout: WebDriver.itemTimeout })
          .catch(() => null);
        if (controls !== null) return;
        if (Date.now() > deadline) break;
        await this.page.waitForTimeout(200);
      }
    }
  }
  async setProjectRowOpen(projectName: string, open: boolean): Promise<void> {
    // A dev-server rebuild can remount the sidebar between the state read
    // and the click, swallowing it; verify and drive once more instead of
    // leaving the row shut for the caller's whole poll budget.
    for (let attempt = 1; attempt <= 2; attempt += 1) {
      if ((await this.isProjectRowOpen(projectName)) === open) return;
      await this.toggleProjectRow(projectName);
      await this.page.waitForTimeout(2000);
    }
  }
  async setProjectsGroupOpen(open: boolean): Promise<void> {
    if ((await this.isProjectsGroupOpen()) !== open) {
      const aside = await this.readyAside();
      await aside
        .getByRole("button", { name: /projects menu/i })
        .first()
        .click({ timeout: WebDriver.actionTimeout });
    }
  }
  async setProjectsOverflowOpen(open: boolean): Promise<void> {
    if ((await this.isProjectsOverflowOpen()) !== open) {
      const aside = await this.readyAside();
      await aside.locator("#extended-project-sidebar-toggle").first().click({ timeout: WebDriver.actionTimeout });
    }
  }
  async sidebarLinkTexts(): Promise<string[]> {
    const aside = await this.screenAside();
    return WebDriver.clean(await aside.locator("a").allTextContents());
  }
  async sidebarRowTone(linkText: string): Promise<{ background: string; color: string }> {
    const aside = await this.screenAside();
    const link = aside.locator("a").filter({ hasText: linkText }).first();
    if ((await link.count()) === 0) return { background: "", color: "" };
    // Simple rows carry the active tone inside the link, but project rows
    // paint it on the outer container: return the innermost non-transparent
    // background from the link outward, stopping two levels up so the read
    // never escapes to the sidebar panel itself.
    return link
      .evaluate(
        (el: Element) => {
          const inner = el.querySelector("div");
          const style = getComputedStyle(inner ?? el);
          const chain: (Element | null)[] = [
            inner ?? el,
            el,
            el.parentElement,
            el.parentElement?.parentElement ?? null,
          ];
          let background = "rgba(0, 0, 0, 0)";
          for (const node of chain) {
            if (node === null || node.tagName === "ASIDE") break;
            const painted = getComputedStyle(node).backgroundColor;
            if (painted !== "" && painted !== "rgba(0, 0, 0, 0)" && painted !== "transparent") {
              background = painted;
              break;
            }
          }
          return { background, color: style.color };
        },
        undefined,
        { timeout: WebDriver.actionTimeout }
      )
      .catch(() => ({ background: "", color: "" }));
  }
  async sidebarSectionNames(): Promise<string[]> {
    const aside = await this.screenAside();
    const text = ((await aside.textContent({ timeout: WebDriver.actionTimeout }).catch(() => null)) ?? "").replace(
      /\s+/g,
      " "
    );
    return ["Projects", "More", "Favorites"].filter((name) => text.includes(name));
  }
  async submitFavoritesFolderName(name: string): Promise<void> {
    // The form is inline (no submit button): filling plus Enter submits it.
    const field = await this.folderNameField();
    await field.fill(name, { timeout: WebDriver.actionTimeout });
    await this.page.keyboard.press("Enter");
  }
  async switchWorkspace(name: string): Promise<void> {
    // Outermost match wins here: the menu item wrapper carries the
    // selection handler (proven against the running old app).
    const menu = this.page.getByRole("menu").first();
    await menu.locator("button, a, [role='menuitem']").filter({ hasText: name }).first().click({
      timeout: WebDriver.actionTimeout,
    });
  }
  async toggleProjectRow(projectName: string): Promise<void> {
    // One retry on a fresh lookup: a remount between resolve and click
    // leaves a detached handle that burns the whole click timeout. The
    // button is also waited for first: after a row-limit change the rows
    // re-render without toggles until the fresh limit arrives, and the
    // name link alone must never count as ready.
    for (let attempt = 1; attempt <= 2; attempt += 1) {
      try {
        const aside = await this.readyAside();
        const button = aside.getByRole("button").filter({ hasText: projectName }).first();
        await button.waitFor({ state: "visible", timeout: 30_000 });
        await button.click({ timeout: WebDriver.actionTimeout });
        return;
      } catch (error) {
        if (attempt === 2) throw error;
      }
    }
  }
  async userMenuTexts(): Promise<string[]> {
    // The identity block is not a button, so read the whole menu text plus
    // the actionable items.
    const menu = this.page.getByRole("menu").first();
    const full = ((await menu.textContent({ timeout: WebDriver.actionTimeout }).catch(() => null)) ?? "")
      .trim()
      .replace(/\s+/g, " ");
    const items = WebDriver.clean(await menu.locator("button, a, [role='menuitem']").allTextContents());
    return [...(full.length > 0 ? [full] : []), ...items];
  }
  async workspaceLogoState(): Promise<{ hasImage: boolean; label: string | null; initial: string | null }> {
    // Observed on the running old app: the top-bar switcher button carries
    // the identity mark — an image when a logo is uploaded, otherwise the
    // workspace initial.
    const switcher = this.page.getByRole("button", { name: "Open workspace switcher" }).first();
    if ((await switcher.count()) === 0) return { hasImage: false, label: null, initial: null };
    // Real image elements only: decorative svgs also carry an img role.
    const images = switcher.locator("img");
    if ((await images.count()) > 0 && (await images.first().isVisible())) {
      return {
        hasImage: true,
        label: await images
          .first()
          .getAttribute("alt", { timeout: WebDriver.actionTimeout })
          .catch(() => null),
        initial: null,
      };
    }
    const text = ((await switcher.textContent({ timeout: WebDriver.actionTimeout }).catch(() => null)) ?? "")
      .trim()
      .replace(/\s+/g, " ");
    return {
      hasImage: false,
      label: await switcher.getAttribute("aria-label", { timeout: WebDriver.actionTimeout }).catch(() => null),
      initial: text.slice(0, 1),
    };
  }
  async workspaceSwitcherTexts(): Promise<string[]> {
    const texts: string[] = [];
    for (const role of ["menu", "dialog"] as const) {
      const els = this.page.getByRole(role);
      for (let index = 0; index < (await els.count()); index += 1) {
        texts.push(
          (
            (await els
              .nth(index)
              .textContent({ timeout: WebDriver.itemTimeout })
              .catch(() => null)) ?? ""
          )
            .trim()
            .replace(/\s+/g, " ")
        );
      }
    }
    return texts.filter((t) => t.length > 0);
  }
  async openQuickCreate(): Promise<void> {
    // A dev-server rebuild can swallow a click mid-remount, so click and
    // re-click once when the dialog stays closed, like a user would.
    const aside = await this.readyAside();
    const button = aside.getByRole("button", { name: "New work item" }).first();
    await button.click({ timeout: WebDriver.actionTimeout });
    if (await this.isQuickCreateDialogOpen()) return;
    await this.page.waitForTimeout(2000);
    if (await this.isQuickCreateDialogOpen()) return;
    await button.click({ timeout: WebDriver.actionTimeout }).catch(() => {});
  }

  // --- NEWFRONT-125 review fixes. Appended; existing methods above are
  // --- untouched per the shared driver contract.

  async activateProjectQuickMenuItem(name: string): Promise<void> {
    // The quick menu renders through a headless portal; deepest match wins
    // because entries nest actionable buttons inside menuitem wrappers.
    const portals = this.page.locator("div[data-headlessui-portal]");
    if ((await portals.count()) > 0) {
      const item = portals.first().locator("button, a, [role='menuitem']").filter({ hasText: name }).last();
      if ((await item.count()) > 0) {
        await item.click({ timeout: WebDriver.actionTimeout });
        return;
      }
    }
    for (const role of ["menu", "menuitem", "dialog"] as const) {
      const els = this.page.getByRole(role);
      for (let index = 0; index < Math.min(await els.count(), 4); index += 1) {
        const item = els.nth(index).locator("button, a, [role='menuitem']").filter({ hasText: name }).last();
        if ((await item.count()) > 0) {
          await item.click({ timeout: WebDriver.actionTimeout });
          return;
        }
      }
    }
    throw new Error(`[parity] project quick menu entry ${name} is not open.`);
  }

  async dragFavoriteBefore(sourceName: string, targetName: string): Promise<void> {
    // Favorite rows share the project rows' hover-revealed grab handle
    // inside a row container, so the drag mirrors the project drag: hover
    // the source row, drag from its revealed handle above the target row.
    // One retry on a fresh lookup: a remount between resolve and hover
    // leaves a detached handle that burns the whole hover timeout.
    let from = { x: 0, y: 0, width: 0, height: 0 };
    let settled = false;
    for (let attempt = 1; attempt <= 2 && !settled; attempt += 1) {
      try {
        const aside = await this.readyAside();
        const group = aside.getByText("Favorites", { exact: true }).first().locator("xpath=ancestor::div[2]");
        const sourceRow = group.locator("a, button").filter({ hasText: sourceName }).first();
        await sourceRow.waitFor({ state: "visible", timeout: 30_000 });
        const container = sourceRow.locator("xpath=ancestor::*[contains(@class,'group/project-item')][1]");
        await container.hover({ timeout: WebDriver.actionTimeout });
        const handle = container.locator(".cursor-grab:visible").first();
        await handle.waitFor({ state: "visible", timeout: WebDriver.actionTimeout });
        from = (await handle.boundingBox({ timeout: WebDriver.actionTimeout }).catch(() => null)) ?? from;
        settled = true;
      } catch (error) {
        if (attempt === 2) throw error;
      }
    }
    const aside = await this.readyAside();
    const group = aside.getByText("Favorites", { exact: true }).first().locator("xpath=ancestor::div[2]");
    const targetRow = group.locator("a, button").filter({ hasText: targetName }).first();
    await targetRow.waitFor({ state: "visible", timeout: 30_000 });
    const to = (await targetRow.boundingBox({ timeout: WebDriver.actionTimeout }).catch(() => null)) ?? {
      x: 0,
      y: 0,
      width: 0,
      height: 0,
    };
    await this.page.mouse.move(from.x + from.width / 2, from.y + from.height / 2);
    await this.page.mouse.down();
    await this.page.mouse.move(to.x + to.width / 2, to.y + 2, { steps: 25 });
    await this.page.mouse.up();
  }

  async openFavoriteQuickMenu(name: string): Promise<void> {
    // The toggle is hover-revealed per row and scoped to the row container,
    // exactly like the project quick menu. Verify and drive once more when
    // a remount swallows the click.
    const aside = await this.readyAside();
    const group = aside.getByText("Favorites", { exact: true }).first().locator("xpath=ancestor::div[2]");
    const row = group.locator("a, button").filter({ hasText: name }).first();
    await row.waitFor({ state: "visible", timeout: 30_000 });
    const container = row.locator("xpath=ancestor::*[contains(@class,'group/project-item')][1]");
    for (let attempt = 1; attempt <= 2; attempt += 1) {
      await row.hover({ timeout: WebDriver.actionTimeout }).catch(() => {});
      const toggle = container.getByRole("button", { name: "Toggle quick actions menu" }).first();
      await toggle.click({ timeout: WebDriver.actionTimeout }).catch(() => {});
      await this.page.waitForTimeout(2000);
      if ((await this.favoriteQuickMenuTexts()).length > 0) return;
    }
  }

  async favoriteQuickMenuTexts(): Promise<string[]> {
    // Same menu surface as the project quick menu: role menus plus the
    // headless portal the entries render through.
    const texts: string[] = [];
    for (const role of ["menu", "menuitem", "dialog"] as const) {
      const els = this.page.getByRole(role);
      const count = Math.min(await els.count(), 4);
      for (let index = 0; index < count; index += 1) {
        texts.push(
          (
            (await els
              .nth(index)
              .textContent({ timeout: WebDriver.itemTimeout })
              .catch(() => null)) ?? ""
          )
            .trim()
            .replace(/\s+/g, " ")
        );
      }
    }
    const portals = this.page.locator("div[data-headlessui-portal]");
    if ((await portals.count()) > 0) {
      texts.push(...WebDriver.clean(await portals.first().locator("button").allTextContents()));
    }
    return texts.filter((t) => t.length > 0);
  }

  async activateFavoriteQuickMenuItem(name: string): Promise<void> {
    const portals = this.page.locator("div[data-headlessui-portal]");
    if ((await portals.count()) > 0) {
      const item = portals.first().locator("button, a, [role='menuitem']").filter({ hasText: name }).last();
      if ((await item.count()) > 0) {
        await item.click({ timeout: WebDriver.actionTimeout });
        return;
      }
    }
    for (const role of ["menu", "menuitem", "dialog"] as const) {
      const els = this.page.getByRole(role);
      for (let index = 0; index < Math.min(await els.count(), 4); index += 1) {
        const item = els.nth(index).locator("button, a, [role='menuitem']").filter({ hasText: name }).last();
        if ((await item.count()) > 0) {
          await item.click({ timeout: WebDriver.actionTimeout });
          return;
        }
      }
    }
    throw new Error(`[parity] favorites quick menu entry ${name} is not open.`);
  }

  async setSidebarCollapsed(collapsed: boolean): Promise<void> {
    // The collapsed flag persists in browser-local storage and the shell
    // reads it on mount, so writing it plus a reload applies the state
    // without depending on the icon-only toggle's probe order.
    await this.page.evaluate((value) => {
      localStorage.setItem("app_sidebar_collapsed", value ? "true" : "false");
    }, collapsed);
    await this.page.reload({ timeout: 60_000 }).catch(() => {});
    await this.isPageSettled(45_000);
  }

  async isSidebarCollapsed(): Promise<boolean> {
    const bar = this.page.locator("#main-sidebar").first();
    if ((await bar.count()) === 0) return false;
    const box = await bar.boundingBox({ timeout: WebDriver.itemTimeout }).catch(() => null);
    if (box === null) return true;
    // Collapsed keeps a 1px border: the live width reads 1, not 0.
    return box.width <= 1;
  }

  async dismissDialogByOverlayClick(): Promise<void> {
    // Some dialogs (product updates) ignore Escape: the observed close path
    // is clicking the overlay outside the centered panel. A raw viewport
    // click lands on the overlay without tripping actionability waits on
    // the covered page beneath.
    await this.page.mouse.click(5, 200);
  }

  async openCompactUserMenu(): Promise<void> {
    // The compact trigger is the avatar button in the top bar, mounted
    // outside the sidebar only while it is collapsed or unmounted. Probe
    // the top strip rightmost-first (the trigger sits at the far end) and
    // verify by the opened menu: only the user menu carries the Community
    // entry with the identity (the switcher shares Sign out and the
    // email, so those alone would false-positive). Skip just the known
    // switcher label; after each miss confirm the sidebar is still
    // collapsed in case a probe click disturbed the toggle.
    const buttons = this.page.locator("button");
    const count = await buttons.count();
    const candidates: { index: number; x: number }[] = [];
    for (let index = 0; index < count; index += 1) {
      const candidate = buttons.nth(index);
      const outside = await candidate.evaluate((el) => el.closest("aside") === null).catch(() => false);
      if (!outside) continue;
      const box = await candidate.boundingBox({ timeout: WebDriver.itemTimeout }).catch(() => null);
      if (box === null || box.width === 0 || box.y >= 60 || box.x < 0) continue;
      const aria = await candidate.getAttribute("aria-label", { timeout: WebDriver.itemTimeout }).catch(() => null);
      if (aria === "Open workspace switcher") continue;
      candidates.push({ index, x: box.x });
    }
    candidates.sort((left, right) => right.x - left.x);
    for (const { index } of candidates.slice(0, 15)) {
      await buttons
        .nth(index)
        .click({ timeout: WebDriver.actionTimeout })
        .catch(() => {});
      await this.page.waitForTimeout(800);
      // Skip the menu read when the click opened nothing: the text read
      // would burn its full timeout on an empty locator.
      if ((await this.page.getByRole("menu").count()) === 0) continue;
      const texts = (await this.userMenuTexts()).join(" ");
      if (texts.includes("Community") && texts.includes("Sign out") && texts.includes("@")) return;
      await this.dismissTopmost();
      if (!(await this.isSidebarCollapsed())) await this.setSidebarCollapsed(true);
    }
    throw new Error("[parity] compact user menu trigger not found in the top bar.");
  }

  async profileSettingsActiveTab(): Promise<string | null> {
    // The settings dialog marks its active tab by tone only (no aria
    // marker), so read every known tab label's computed background and
    // return the odd one out.
    const known = new Set([
      "Profile",
      "Security",
      "Activity",
      "Preferences",
      "AI Assistant",
      "Auto Project Management",
      "Notifications",
      "Integrations",
      "Personal Access Tokens",
    ]);
    const dialog = this.page.getByRole("dialog").filter({ hasText: "Your profile" }).first();
    if ((await dialog.count()) === 0) return null;
    const buttons = dialog.getByRole("button");
    const count = await buttons.count();
    const tones: { label: string; background: string }[] = [];
    for (let index = 0; index < count; index += 1) {
      const label = (
        (await buttons
          .nth(index)
          .textContent({ timeout: WebDriver.itemTimeout })
          .catch(() => null)) ?? ""
      )
        .trim()
        .replace(/\s+/g, " ");
      if (!known.has(label)) continue;
      const background = await buttons
        .nth(index)
        .evaluate((el) => getComputedStyle(el).backgroundColor)
        .catch(() => "");
      tones.push({ label, background });
    }
    if (tones.length === 0) return null;
    const tally = new Map<string, number>();
    for (const tone of tones) tally.set(tone.background, (tally.get(tone.background) ?? 0) + 1);
    let common = "";
    let commonCount = -1;
    for (const [background, n] of tally) {
      if (n > commonCount) {
        common = background;
        commonCount = n;
      }
    }
    const odd = tones.filter((tone) => tone.background !== common);
    return odd.length === 1 ? odd[0].label : null;
  }

  async isSidebarPeekVisible(): Promise<boolean> {
    // The peek panel keeps an inline width while hidden (translated away
    // at opacity 0), so Playwright visibility always reads true: the shown
    // state is opacity 1, read off the computed style instead.
    const peek = this.page.getByRole("complementary", { name: "Sidebar peek view" });
    if ((await peek.count()) === 0) return false;
    const opacity = await peek
      .first()
      .evaluate((el) => getComputedStyle(el).opacity)
      .catch(() => "0");
    return Number.parseFloat(opacity) > 0;
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

  // Display (arrangement) controls (NEWFRONT-119). Observed on the running
  // old app: the header holds icon-only layout buttons, the builder toggle,
  // then named Display and Analytics buttons. Display opens a popover panel
  // (a headless-ui popover panel) with one block per section; each block
  // starts with its heading ("Display Properties", "Group by", "Order by";
  // the switches block has no heading) followed by pill, radio, or
  // checkbox buttons. Checked radios/checkboxes render a check icon.
  private displayButton() {
    return this.page.getByRole("button", { name: "Display" });
  }

  private displayPanel() {
    // Scoped by its heading: the page holds several popover panels (row
    // menus, layout switch) and an unscoped locator would wait on all of
    // them at once.
    return this.page.locator('div[id^="headlessui-popover-panel"]', { hasText: "Display Properties" });
  }

  private displaySection(heading: string) {
    return this.displayPanel().locator("div.py-2", { hasText: heading });
  }

  async openDisplayOptions(): Promise<void> {
    // Bounded: a throttled page load must fail fast and loudly, never burn
    // the whole test timeout on one click.
    await this.displayButton().waitFor({ state: "visible", timeout: 60_000 });
    await this.displayButton().click({ timeout: 30_000 });
    await this.displayPanel().getByText("Display Properties", { exact: true }).waitFor({ timeout: 30_000 });
  }

  async closeDisplayOptions(): Promise<void> {
    await this.page.keyboard.press("Escape");
    // Hidden, not detached: the popover lingers in the DOM while its
    // close transition runs. If Escape missed (focus was outside), toggle
    // the Display button itself instead of hanging.
    try {
      await this.displayPanel().waitFor({ state: "hidden", timeout: 10_000 });
    } catch {
      await this.displayButton().click();
      await this.displayPanel().waitFor({ state: "hidden" });
    }
  }

  async displayPanelText(): Promise<string> {
    return (await this.displayPanel().innerText()).trim();
  }

  /**
   * Activate a Display panel option. A real mouse click races the PATCH
   * re-render: the pill shifts under the cursor and the click lands on the
   * backdrop, dismissing the popover without toggling. A single dispatched
   * click cannot misfire that way; the visible-wait first keeps a genuinely
   * missing option failing honestly.
   */
  private async clickPanelOption(target: Locator): Promise<void> {
    // The popover can dismiss under load between the open and the click (a
    // re-render drops it). Re-open once when the panel itself is gone; when
    // it is still open, wait out the slow render instead. A genuinely
    // missing option fails the second wait honestly either way.
    try {
      await target.waitFor({ state: "visible", timeout: 15_000 });
    } catch {
      // Re-open only when no panel instance is visible (a closing transition
      // can linger beside the live one, so probe every match); otherwise
      // wait out the slow render instead of toggling a live panel shut.
      const panels = this.displayPanel();
      const count = await panels.count();
      let open = false;
      for (let index = 0; index < count && !open; index += 1) {
        open = await panels
          .nth(index)
          .isVisible()
          .catch(() => false);
      }
      if (!open) await this.openDisplayOptions();
      await target.waitFor({ state: "visible", timeout: 15_000 });
    }
    await target.dispatchEvent("click");
  }

  async setDisplayGroupBy(option: string): Promise<void> {
    await this.clickPanelOption(this.displaySection("Group by").getByRole("button", { name: option }));
  }

  async setDisplayOrderBy(option: string): Promise<void> {
    await this.clickPanelOption(this.displaySection("Order by").getByRole("button", { name: option }));
  }

  async setDisplayExtraOption(option: string, enabled: boolean): Promise<void> {
    const target = this.displayPanel().getByRole("button", { name: option });
    if ((await this.isDisplayOptionChecked(option)) !== enabled) await this.clickPanelOption(target);
  }

  async isDisplayOptionChecked(option: string): Promise<boolean> {
    const target = this.displayPanel().getByRole("button", { name: option });
    return (await target.locator("svg").count()) > 0;
  }

  async toggleDisplayProperty(option: string): Promise<void> {
    await this.clickPanelOption(this.displaySection("Display Properties").getByRole("button", { name: option }));
  }

  async isDisplayPropertyActive(option: string): Promise<boolean> {
    // Pills carry no check icon; the active one paints the accent
    // background while the inactive one stays unfilled.
    const pill = this.displaySection("Display Properties").getByRole("button", { name: option });
    await pill.waitFor({ state: "visible", timeout: 15_000 });
    return ((await pill.getAttribute("class")) ?? "").includes("bg-accent-primary");
  }

  // Condition-row builder (NEWFRONT-119). Observed on the running old app:
  // the toggle is the header button just before Display (icon-only, so it
  // is located structurally, not by name). The row itself renders below the
  // header when visible; its add control opens a searchable picker whose
  // options carry the property names.
  private headerControlButtons() {
    const bar = this.displayButton().locator("xpath=ancestor::div[contains(@class,'justify-end')][1]");
    return bar.getByRole("button");
  }

  private async richToggle() {
    // The toggle is the icon-only button immediately before Display (after
    // the layout switch cluster); locating it relative to Display survives
    // layout-count changes that a fixed offset would not.
    await this.displayButton().waitFor({ state: "visible", timeout: 60_000 });
    const buttons = await this.headerControlButtons().all();
    let displayIndex = -1;
    for (let index = 0; index < buttons.length; index += 1) {
      const button = buttons[index];
      if (button === undefined) continue;
      const text = ((await button.innerText().catch(() => "")) as string).trim();
      if (text === "Display") {
        displayIndex = index;
        break;
      }
    }
    const toggle = displayIndex > 0 ? buttons[displayIndex - 1] : undefined;
    if (toggle === undefined) throw new Error("[parity] rich-filter toggle not found.");
    return toggle;
  }

  private richRow() {
    // The bare class pair also matches cards and menus elsewhere on the
    // page, so require a condition remove control: the row renders one per
    // condition and only while it is visible.
    return this.page.locator('div.rounded-lg.bg-layer-1:has(button[aria-label="Remove filter"])').first();
  }

  async toggleRichFilterRow(): Promise<void> {
    await (await this.richToggle()).click({ timeout: 30_000 });
  }

  async isRichFilterRowVisible(): Promise<boolean> {
    return (await this.richRow().count()) > 0;
  }

  async richFilterRowText(): Promise<string> {
    return (await this.richRow().innerText()).trim();
  }

  async addRichCondition(property: string): Promise<void> {
    const before = await this.richConditionCount();
    await this.openAddPicker();
    await this.page.getByRole("option", { name: property, exact: true }).click({ timeout: 60_000 });
    // Picking a property adds the condition (its value slot usually pops
    // open next, so options lingering is expected, not a failure).
    const start = Date.now();
    while ((await this.richConditionCount()) <= before) {
      if (Date.now() - start > 15_000) throw new Error(`[parity] picking ${property} added no condition.`);
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

  // --- Invitation inbox + onboarding start (NEWFRONT-110, AUTH-026/033).
  // --- Appended; existing methods above are untouched per the shared
  // --- driver contract.
  async openInvitations(): Promise<void> {
    await this.page.goto("/invitations");
    await this.page.waitForLoadState("domcontentloaded");
  }

  /**
   * Workspace names of the invitation cards. Each card renders the
   * workspace name as its title line next to shorter chrome (avatar
   * initial, role label); the empty state instead renders a "no pending
   * invites" heading and no cards. The name is the longest text line,
   * which needs no knowledge of the role labels.
   */
  async invitationWorkspaceNames(): Promise<string[]> {
    const page = this.page;
    await page
      .getByRole("heading", { name: /join a workspace|no pending invites/i })
      .first()
      .waitFor();
    if (await this.invitationsEmptyStateVisible()) return [];
    const cards = page.locator("div.cursor-pointer");
    const count = await cards.count();
    const names: string[] = [];
    for (let i = 0; i < count; i++) {
      const lines = ((await cards.nth(i).innerText()) ?? "")
        .split("\n")
        .map((s) => s.trim())
        .filter(Boolean);
      const longest = lines.sort((a, b) => b.length - a.length)[0];
      if (longest !== undefined) names.push(longest);
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
    // trigger (avatar + display name — the display name, not the email, so
    // "@" matching fails on fresh accounts). The menu items mount only once
    // the trigger opens the menu, so wait for the trigger — never the item
    // — then open it. The trigger is the page's menu button (the header is
    // a plain div with no landmark, and its back button carries no
    // accessible name, so text matching is unreliable here).
    const trigger = page.locator('button[aria-haspopup="menu"]').first();
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
    // The approval page carries a single submit button ("Approve",
    // "Approving…" while the request is in flight). Match it directly: an
    // earlier `form`-with-`has` composition never resolved, because the
    // inner selector is evaluated inside each candidate form (i.e. it
    // looked for a form nested in the form). The exact-name match also
    // waits out the in-flight "Approving…" state.
    await this.page.getByRole("button", { name: /^approve$/i }).click();
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

  async sidebarBrandVisible(): Promise<boolean> {
    const brand = this.sidebar().getByText("Pi Dash", { exact: true }).first();
    return (await brand.count()) > 0 && (await brand.isVisible());
  }

  async sidebarQuickActionNames(): Promise<string[]> {
    const texts = await this.sidebar().getByRole("button").allTextContents();
    return texts.map((t) => t.trim().replace(/\s+/g, " ")).filter((t) => t.length > 0);
  }

  async sidebarAccountButtonCount(): Promise<number> {
    return await this.sidebar().getByRole("button").filter({ hasText: "@" }).count();
  }

  /** The resize grip on the sidebar's right edge. */
  private sidebarGrip(): Locator {
    return this.page.getByRole("separator", { name: "Resize sidebar" }).first();
  }

  async dragSidebarGripBy(dx: number): Promise<void> {
    const grip = this.sidebarGrip();
    const box = await grip.boundingBox();
    if (box === null) throw new Error("[parity] sidebar resize grip has no layout box.");
    const x = box.x + box.width / 2;
    const y = box.y + box.height / 2;
    await this.page.mouse.move(x, y);
    await this.page.mouse.down();
    await this.page.mouse.move(x + dx, y, { steps: 10 });
    await this.page.mouse.up();
  }

  async doubleClickSidebarGrip(): Promise<void> {
    const grip = this.sidebarGrip();
    const box = await grip.boundingBox();
    if (box === null) throw new Error("[parity] sidebar resize grip has no layout box.");
    // Offset down the edge: the grip's own center can sit under the
    // header row, while the edge below it takes the collapse gesture.
    await this.page.mouse.dblclick(box.x + box.width / 2, box.y + 100);
  }

  async hoverCollapsedEdge(): Promise<void> {
    await this.page.mouse.move(4, 400);
  }

  async clickOutsideSidebar(): Promise<void> {
    // Raw mouse event: the floating shell animates under the cursor, which
    // defeats actionability checks, while the outside detector only needs
    // the press itself.
    await this.page.mouse.click(450, 400);
  }

  async sidebarEntryVisible(name: string): Promise<boolean> {
    return (await this.sidebar().innerText()).includes(name);
  }

  /** The listed-projects count input inside the open dialog. */
  private projectCapField(): Locator {
    return this.page.locator('[role="dialog"]').locator('input[type="number"]').first();
  }

  async projectCapTypeText(text: string): Promise<void> {
    const input = this.projectCapField();
    await input.click();
    await input.press("End");
    for (const char of text) await input.press(char);
  }

  async projectCapFill(value: string): Promise<void> {
    await this.projectCapField().fill(value);
  }

  async projectCapMinErrorVisible(): Promise<boolean> {
    const error = this.page.locator('[role="dialog"]').getByText("Minimum value is 1");
    return (await error.count()) > 0 && (await error.first().isVisible());
  }

  async railSettingsEntryPresent(): Promise<boolean> {
    return (await this.page.getByRole("link", { name: "Settings", exact: true }).count()) > 0;
  }

  async railContextMenuText(): Promise<string> {
    await this.page.mouse.click(8, 400, { button: "right" });
    await this.page.waitForTimeout(1000);
    return (await this.page.locator("#context-menu-portal").textContent()) ?? "";
  }

  async inboxDotPresent(): Promise<boolean> {
    // The dot is a span nested inside the inbox link's icon wrapper and
    // mounts only with unread notifications, so the icon subtree carries no
    // span while the inbox is empty.
    return (await this.page.locator('a[href$="/notifications/"] div span').count()) > 0;
  }

  async hoverProjectHeader(): Promise<void> {
    await this.shellMain().locator('button[aria-haspopup="listbox"]').first().hover();
  }

  async projectNameVisibleCount(name: string): Promise<number> {
    return await this.page.getByText(name, { exact: false }).count();
  }

  async projectActionDialogHeading(): Promise<string | null> {
    // The dialog root is a zero-size wrapper around fixed panels, so the
    // heading inside the panel is the visible proof it opened.
    const heading = this.page.locator('[role="dialog"]').getByRole("heading").first();
    if ((await heading.count()) === 0) return null;
    return ((await heading.textContent()) ?? "").trim().replace(/\s+/g, " ") || null;
  }

  async activeCyclesHeaderVisible(): Promise<boolean> {
    const header = this.page.getByText("Active cycles", { exact: false }).first();
    return (await header.count()) > 0 && (await header.isVisible());
  }

  async errorNoticeVisible(): Promise<boolean> {
    const notice = this.page.getByText("Something went wrong");
    return (await notice.count()) > 0 && (await notice.first().isVisible());
  }

  // --- NEWFRONT-125 re-review fix (049 placeholders). Appended; existing
  // --- methods above are untouched per the shared driver contract.
  async sidebarProjectPlaceholderCount(): Promise<number> {
    // While the project collection resolves, the projects group shows a
    // loading-status region with one block per placeholder row; the region
    // unmounts once rows render. Scoped to the on-screen aside: the
    // off-screen mirror and out-of-sidebar live regions carry their own
    // status nodes that must not count.
    const aside = await this.screenAside();
    return aside
      .getByRole("status")
      .evaluateAll((regions) => regions.reduce((total, region) => total + region.children.length, 0))
      .catch(() => 0);
  }

  async pickRichValues(values: string[]): Promise<void> {
    for (const value of values) {
      await this.page.getByRole("option", { name: value, exact: true }).click({ timeout: 60_000 });
    }
    await this.closeValueSlot();
  }

  async pickRichValuesContaining(values: string[]): Promise<void> {
    for (const value of values) {
      await this.page.getByRole("option", { name: value }).click({ timeout: 60_000 });
    }
    await this.closeValueSlot();
  }

  /**
   * The row's + control: its only button with neither an accessible name
   * nor text (condition buttons carry property/operator/value text, remove
   * buttons are labelled, and row actions carry text).
   */
  private async findAddControl(): Promise<Locator> {
    const buttons = await this.richRow().getByRole("button").all();
    for (const button of buttons) {
      const label = await button.getAttribute("aria-label");
      const text = ((await button.innerText().catch(() => "")) as string).trim();
      if (label === null && text === "") return button;
    }
    throw new Error("[parity] rich-filter add control not found.");
  }

  private async openAddPicker(): Promise<void> {
    // Callers leave no popup open (peeks and picks dismiss after
    // themselves), so a click always opens; the + control toggles, and a
    // double-open would shut it again instead of failing here.
    await (await this.findAddControl()).click({ timeout: 30_000 });
    await this.page.getByRole("option").first().waitFor({ timeout: 15_000 });
  }

  private async closeAddPicker(): Promise<void> {
    if ((await this.page.getByRole("option").count()) === 0) return;
    await (await this.findAddControl()).click({ timeout: 30_000 });
    await this.page.waitForFunction(() => document.querySelectorAll('[role="option"]').length === 0, null, {
      timeout: 15_000,
    });
  }

  async listRichPickerOptions(): Promise<string[]> {
    await this.openAddPicker();
    const names = await this.page.getByRole("option").allTextContents();
    await this.closeAddPicker();
    return names.map((t) => t.trim()).filter((t) => t.length > 0);
  }

  async richValueOptions(): Promise<string[]> {
    const names = await this.page.getByRole("option").allTextContents();
    return names.map((t) => t.trim()).filter((t) => t.length > 0);
  }

  /**
   * Dismiss the value slot after picking. Multi-select slots stay open for
   * further picks; toggling the slot's own button shuts it (Escape does
   * nothing to these popups).
   */
  private async closeValueSlot(): Promise<void> {
    if ((await this.page.getByRole("option").count()) === 0) return;
    const row = this.richRow();
    const buttons = await row.getByRole("button").all();
    const named: Locator[] = [];
    for (const button of buttons) {
      if ((await button.getAttribute("aria-label")) !== null) continue;
      const text = ((await button.innerText().catch(() => "")) as string).trim();
      // Row actions sort with the conditions and must never be mistaken for
      // the value slot: view pages render Save as / Update view once dirty
      // propagates, and toggling Save as opens the Create View dialog.
      if (text === "" || text === "Clear all" || text === "Save view" || text === "Save as" || text === "Update view")
        continue;
      named.push(button);
    }
    // Property, operator, then the value slot of the last condition.
    const slot = named[named.length - 1];
    if (slot === undefined) throw new Error("[parity] value slot control not found.");
    await slot.click({ timeout: 30_000 });
    await this.page.waitForFunction(() => document.querySelectorAll('[role="option"]').length === 0, null, {
      timeout: 15_000,
    });
  }

  /**
   * The operator control is the second named button of a single-condition
   * row (property first, then operator, then the value slot, then the
   * labelled remove button).
   */
  private async singleConditionOperator() {
    const row = this.richRow();
    const buttons = await row.getByRole("button").all();
    const named: import("@playwright/test").Locator[] = [];
    for (const button of buttons) {
      const label = await button.getAttribute("aria-label");
      if (label !== null) continue;
      const text = ((await button.innerText().catch(() => "")) as string).trim();
      if (text === "") continue;
      named.push(button);
    }
    // Property, operator, then possibly the value slot.
    const operator = named[1];
    if (operator === undefined) throw new Error("[parity] condition operator control not found.");
    // The control locks when its property offers a single operator; fail
    // fast instead of clicking a disabled button to the test timeout.
    if (await operator.isDisabled()) throw new Error("[parity] condition operator is locked (single operator).");
    return operator;
  }

  async richOperatorOptions(): Promise<string[]> {
    const operator = await this.singleConditionOperator();
    await operator.click({ timeout: 30_000 });
    await this.page.getByRole("option").first().waitFor({ timeout: 15_000 });
    const names = await this.page.getByRole("option").allTextContents();
    // Escape does nothing to these popups; the operator button toggles.
    await (await this.singleConditionOperator()).click({ timeout: 30_000 });
    await this.page.waitForFunction(() => document.querySelectorAll('[role="option"]').length === 0, null, {
      timeout: 15_000,
    });
    return names.map((t) => t.trim()).filter((t) => t.length > 0);
  }

  async pickRichOperator(option: string): Promise<void> {
    await (await this.singleConditionOperator()).click({ timeout: 30_000 });
    await this.page.getByRole("option", { name: option, exact: true }).click({ timeout: 60_000 });
    await this.page.waitForFunction(() => document.querySelectorAll('[role="option"]').length === 0, null, {
      timeout: 15_000,
    });
  }

  async isSingleRichOperatorLocked(): Promise<boolean> {
    const row = this.richRow();
    const buttons = await row.getByRole("button").all();
    const named: import("@playwright/test").Locator[] = [];
    for (const button of buttons) {
      const label = await button.getAttribute("aria-label");
      if (label !== null) continue;
      const text = ((await button.innerText().catch(() => "")) as string).trim();
      if (text === "") continue;
      named.push(button);
    }
    const operator = named[1];
    if (operator === undefined) throw new Error("[parity] condition operator control not found.");
    return operator.isDisabled();
  }

  async isRichCalendarOpen(): Promise<boolean> {
    return (await this.page.getByRole("gridcell").count()) > 0;
  }

  async pickRichDay(day: string): Promise<void> {
    // The calendar toggles off its value slot; reopen when a previous pick
    // (or a range second slot) left it shut.
    if ((await this.page.getByRole("gridcell").count()) === 0) {
      const slots = this.richRow().getByRole("button", { name: "--", exact: true });
      await slots.last().click({ timeout: 30_000 });
      await this.page.getByRole("gridcell").first().waitFor({ timeout: 15_000 });
    }
    // Mid-month days (13-19) sit in the current month only, so a loose
    // name match still resolves to exactly one cell.
    await this.page.getByRole("gridcell", { name: day }).first().click({ timeout: 30_000 });
  }

  async richConditionCount(): Promise<number> {
    return this.richRow().getByRole("button", { name: "Remove filter" }).count();
  }

  async removeRichCondition(index: number): Promise<void> {
    await this.richRow().getByRole("button", { name: "Remove filter" }).nth(index).click({ timeout: 30_000 });
  }

  async clearRichFilters(): Promise<void> {
    await this.richRow().getByRole("button", { name: "Clear all" }).click({ timeout: 30_000 });
  }

  async openProjectView(workspaceSlug: string, projectId: string, viewId: string): Promise<void> {
    await this.page.goto(`/${workspaceSlug}/projects/${projectId}/views/${viewId}`, {
      waitUntil: "domcontentloaded",
    });
    if (new URL(this.page.url()).pathname === "/")
      throw new Error("[parity] view page bounced to sign-in; the session did not survive.");
  }

  /**
   * Activate a rich-row action. Like clickPanelOption: a real mouse click
   * races the PATCH re-render storm after a value pick (the button never
   * settles, so the click waits to the test timeout); a dispatched click
   * after a visible-wait cannot miss that way.
   */
  private async clickRowAction(name: string): Promise<void> {
    const target = this.richRow().getByRole("button", { name });
    // Generous: the Update affordance appears only once the dirty state
    // propagates after the PATCH round-trip, slow on a hot shared stack.
    await target.waitFor({ state: "visible", timeout: 60_000 });
    await target.dispatchEvent("click");
  }

  async saveRichViewAs(name: string): Promise<void> {
    await this.clickRowAction("Save view");
    const dialog = this.page.getByRole("dialog");
    await dialog.getByPlaceholder("Title").fill(name);
    await dialog.getByRole("button", { name: "Create View" }).click({ timeout: 30_000 });
    // Creating navigates to the new view page; wait for it so callers read
    // the committed view instead of racing the in-flight POST.
    await this.page.waitForURL(/\/views\//, { timeout: 60_000 });
  }

  async updateRichView(): Promise<void> {
    await this.clickRowAction("Update view");
  }

  // Header analytics entry (NEWFRONT-119). Observed on the running old app:
  // a named Analytics button opens a project-scoped dialog that carries the
  // project name and per-group counts; Escape dismisses it.
  async openAnalytics(): Promise<void> {
    await this.page.getByRole("button", { name: "Analytics" }).click();
    await this.page.getByRole("dialog").waitFor();
  }

  async closeAnalytics(): Promise<void> {
    await this.page.keyboard.press("Escape");
    await this.page.getByRole("dialog").waitFor({ state: "detached" });
  }

  async analyticsDialogText(): Promise<string> {
    return (await this.page.getByRole("dialog").innerText()).trim();
  }

  // --- palette open / close / reset (SHELL-080, SHELL-082) ---

  /** Close the modal palette if open (stuck sub-pages included) and ground focus. */
  private async resetPaletteToClosed(): Promise<void> {
    // Activating a picker entry does not reliably close the palette, and an
    // open-but-stale sub-page has no pending reset (the timer only arms on
    // close), so waiting on it would hang forever. Two Escapes cover a
    // leftover query (first clears, second closes).
    for (let i = 0; i < 2 && (await this.isCommandPaletteOpen()); i++) {
      await this.pressInCommandPalette("Escape");
    }
    await this.page
      .waitForFunction(() => document.querySelectorAll('[role="dialog"] [cmdk-root]').length === 0, {
        timeout: 10_000,
      })
      .catch(() => undefined);
    // After the palette unmounts, focus can strand on the detached input,
    // where keypresses dispatch into the void instead of reaching the
    // document shortcut listener. Blur grounds attached focus; Tab then
    // grounds even detached limbo (focus navigation is chrome-level, so it
    // works when document delivery is broken). Focus-only, no activation.
    // Two document-level Escapes sandwich the grounding: one first, because
    // a stray open menu (e.g. a work-item row dropdown) swallows the chord;
    // one after, because Tab itself can land on a row trigger and pop its
    // menu open again (suite15 proved five such menus stacked across
    // attempts, each swallowing the next chord). Escapes close menus only.
    await this.page.keyboard.press("Escape").catch(() => undefined);
    await this.page
      .evaluate(() => {
        if (document.activeElement instanceof HTMLElement) document.activeElement.blur();
      })
      .catch(() => undefined);
    await this.page.keyboard.press("Tab").catch(() => undefined);
    await this.page.keyboard.press("Escape").catch(() => undefined);
  }

  /** Whether the modal palette sits at root with a stable, non-empty command list. */
  private async paletteSettledAtRoot(): Promise<boolean> {
    // Commands stream in (static entries first, data-backed ones as their
    // stores resolve), so a bare non-empty read races stragglers: require
    // the count to hold still across half a second.
    if ((await this.commandPalettePlaceholder()) !== PALETTE_ROOT_PLACEHOLDER) return false;
    const first = await this.paletteItems().count();
    if (first === 0) return false;
    await this.page.waitForTimeout(500);
    if ((await this.commandPalettePlaceholder()) !== PALETTE_ROOT_PLACEHOLDER) return false;
    const second = await this.paletteItems().count();
    return second === first && second > 0;
  }

  async pressPaletteOpenChord(): Promise<void> {
    // The old app's handler catches Ctrl/Cmd+K on document before its typing
    // guard, so this opens the palette from anywhere, including inside inputs.
    // The first attempt is a bare press, preserving caller focus semantics
    // (080 proves the chord fires while an input holds focus); only retries
    // normalize first, for stuck or focus-stranded states.
    // Under shared-host load a lone chord can land before the shortcut
    // listener attaches (hydration or a chunk compile still in flight), so
    // retry until the palette reads open, populated AND back at root: the
    // input renders before the command list, callers assert commands
    // immediately, and a reopen within 200ms of a close briefly shows the
    // previous sub-page (the reset timer is uncleared, so it always
    // self-heals to root — this wait simply rides it out).
    // The read mirrors paletteModal scope (dialog-hosted cmdk only) so an
    // expanded top-bar search never masquerades as the open modal. Twelve
    // attempts (~120s worst case, ~1s happy path) ride out even a fully
    // stalled post-navigation remount.
    for (let attempt = 0; attempt < 12; attempt++) {
      if (attempt > 0) await this.resetPaletteToClosed();
      if (!(await this.isCommandPaletteOpen())) {
        await this.page.keyboard.press("ControlOrMeta+k");
      }
      const deadline = Date.now() + 10_000;
      let settled = false;
      while (Date.now() < deadline) {
        if (await this.paletteSettledAtRoot()) {
          settled = true;
          break;
        }
        await this.page.waitForTimeout(250);
      }
      if (settled) return;
    }
    // Never silently return unsettled: every caller proceeds to interact
    // with the palette, so an unsettled return only converts into a downstream
    // hang (suite15: a 300s click wait). Fail fast with the real cause.
    throw new Error("[parity] palette did not settle at root after 12 open attempts");
  }

  async isCommandPaletteOpen(): Promise<boolean> {
    return this.isShown(this.paletteInput());
  }

  async commandPalettePlaceholder(): Promise<string | null> {
    return this.readAttr(this.paletteInput(), "placeholder");
  }

  async focusAndTypeTopBarSearch(text: string): Promise<void> {
    // The ce top navigation always mounts an expandable search input
    // (placeholder "Search commands..."). Focus it and type to prove the
    // open chord still fires while a text field holds focus.
    const search = this.page.getByPlaceholder("Search commands...").first();
    await search.click();
    await search.pressSequentially(text);
  }

  async closeCommandPaletteViaBackdrop(): Promise<void> {
    // Headless-UI renders the backdrop as a fixed full-screen layer behind the
    // panel; a top-left click lands on it, not on the centered max-w-2xl panel.
    await this.page.mouse.click(5, 5);
  }

  // --- palette query + keyboard flow (SHELL-083, SHELL-085) ---

  async typeInCommandPalette(text: string): Promise<void> {
    await this.paletteInput().pressSequentially(text);
  }

  async commandPaletteQueryValue(): Promise<string> {
    return (await this.readValue(this.paletteInput())) ?? "";
  }

  async pressInCommandPalette(key: string): Promise<void> {
    await this.paletteInput().press(key);
  }

  async paletteGroupHeadings(): Promise<string[]> {
    const texts = await this.paletteModal().locator("[cmdk-group-heading]").allTextContents();
    return texts.map((t) => t.trim()).filter((t) => t.length > 0);
  }

  async paletteCommandTitles(): Promise<string[]> {
    // Each cmdk item renders an icon, its title, and optional shortcut badges;
    // the trimmed text content is dominated by the title. Callers match by
    // substring (paletteHasCommand) rather than exact equality.
    const texts = await this.paletteItems().allTextContents();
    return texts.map((t) => t.replace(/\s+/g, " ").trim()).filter((t) => t.length > 0);
  }

  async paletteHasCommand(title: string): Promise<boolean> {
    return this.isShown(this.paletteItems().filter({ hasText: title }));
  }

  async activatePaletteCommand(title: string): Promise<void> {
    // Bounded click with one re-settle retry: the palette can detach between
    // the caller's settle and this click, and an unbounded click then hangs
    // until the test timeout (suite15). Re-opening is a safe no-op when the
    // palette is already settled (no re-press, settle check only).
    try {
      await this.paletteItems().filter({ hasText: title }).first().click({ timeout: 15_000 });
    } catch {
      await this.pressPaletteOpenChord();
      await this.paletteItems().filter({ hasText: title }).first().click({ timeout: 15_000 });
    }
  }

  async paletteSelectedItemText(): Promise<string | null> {
    return this.readText(this.paletteModal().locator('[cmdk-item][aria-selected="true"]').first());
  }

  // --- server search (SHELL-084) ---

  private searchResultsHeading(): Locator {
    return this.paletteModal()
      .getByText(/Search results for/i)
      .first();
  }

  async paletteSearchResultsHeading(): Promise<string | null> {
    return this.readText(this.searchResultsHeading());
  }

  async isPaletteSearchHeadingPulsing(): Promise<boolean> {
    const cls = (await this.readAttr(this.searchResultsHeading(), "class")) ?? "";
    return cls.includes("animate-pulse");
  }

  private workspaceLevelToggle(): Locator {
    // Footer scope control labelled "Workspace level"; the ToggleSwitch is the
    // adjacent switch/button.
    return this.paletteRoot()
      .locator("*")
      .filter({ hasText: /Workspace level/i })
      .getByRole("switch")
      .first();
  }

  async paletteHasWorkspaceLevelToggle(): Promise<boolean> {
    return this.isShown(
      this.paletteModal()
        .getByText(/Workspace level/i)
        .first()
    );
  }

  async isWorkspaceLevelToggleEnabled(): Promise<boolean> {
    const toggle = this.workspaceLevelToggle();
    if ((await toggle.count()) === 0) return false;
    return toggle.isEnabled();
  }

  async toggleWorkspaceLevel(): Promise<void> {
    await this.workspaceLevelToggle().click();
  }

  private searchRequests: URL[] = [];
  private searchTrackingOn = false;

  private ensureSearchTracking(): void {
    if (this.searchTrackingOn) return;
    this.searchTrackingOn = true;
    this.page.on("request", (req) => {
      const url = req.url();
      if (/\/api\/workspaces\/[^/]+\/search\/?(\?|$)/.test(url)) {
        this.searchRequests.push(new URL(url));
      }
    });
  }

  async countSearchRequests(action: () => Promise<void>): Promise<number> {
    this.ensureSearchTracking();
    const before = this.searchRequests.length;
    await action();
    return this.searchRequests.length - before;
  }

  async lastSearchRequestParams(): Promise<Record<string, string> | null> {
    const last = this.searchRequests.at(-1);
    if (!last) return null;
    return Object.fromEntries(last.searchParams.entries());
  }

  // --- shortcuts reference dialog (SHELL-094) ---

  async isShortcutsDialogOpen(): Promise<boolean> {
    return this.isShown(this.page.getByText("Keyboard shortcuts", { exact: false }));
  }

  async pressShortcutsDialogChord(): Promise<void> {
    await this.page.keyboard.press("ControlOrMeta+/");
  }

  async typeShortcutsFilter(text: string): Promise<void> {
    await this.page.getByPlaceholder("Search for shortcuts").fill(text);
  }

  async shortcutsDialogCommandTitles(): Promise<string[]> {
    const dialog = this.page
      .getByRole("dialog")
      .filter({ has: this.page.getByPlaceholder("Search for shortcuts") })
      .first();
    const root = (await dialog.count()) > 0 ? dialog : this.page.getByRole("dialog").first();
    const texts = await root.locator("h5 ~ * , li, [class*='flex']").allTextContents();
    return texts.map((t) => t.replace(/\s+/g, " ").trim()).filter((t) => t.length > 0);
  }

  // --- repo-star action (SHELL-103) ---

  async repoStarLinkAttributes(): Promise<{ href: string; target: string; rel: string } | null> {
    const link = this.page.getByRole("link", { name: /Star us on GitHub/i }).first();
    if ((await link.count()) === 0) return null;
    return {
      href: (await link.getAttribute("href")) ?? "",
      target: (await link.getAttribute("target")) ?? "",
      rel: (await link.getAttribute("rel")) ?? "",
    };
  }

  async repoStarIconSrc(): Promise<string | null> {
    const img = this.page
      .getByRole("link", { name: /Star us on GitHub/i })
      .first()
      .locator("img")
      .first();
    if ((await img.count()) === 0) return null;
    return img.getAttribute("src");
  }

  // --- preferences: theme, language, timezone, first day of week
  //     (SHELL-088, 095, 096, 097). The palette preference commands open cmdk
  //     sub-pages whose options are ordinary [cmdk-item] nodes, so the existing
  //     palette readers/activators drive them; these two reads observe the
  //     applied result — the theme as a class on the document root, the
  //     interface language as the root lang attribute. ---

  async documentTheme(): Promise<string> {
    return (await this.page.locator("html").getAttribute("data-theme")) ?? "";
  }

  async documentLang(): Promise<string> {
    return (await this.page.locator("html").getAttribute("lang")) ?? "";
  }

  // --- palette creation entries (SHELL-086) ---

  // --- palette pickers: empty / no-results / no-recents (SHELL-093) ---

  async paletteHasText(text: string): Promise<boolean> {
    return this.isShown(this.paletteModal().getByText(text, { exact: false }));
  }

  // --- browse route (SHELL-106, negative row) ---

  async openBrowseWorkItem(workspaceSlug: string, identifier: string): Promise<void> {
    await this.goToPath(`/${workspaceSlug}/browse/${identifier}`);
  }

  async browseShowsWorkItemDetail(): Promise<boolean> {
    // The missing-key branch renders an explicit empty state; its absence
    // (paired with the caller's route-URL and work-item-title assertions)
    // proves the project-scoped detail rendered instead.
    return !(await this.isShown(this.page.getByText("Work item does not exist")));
  }

  async browseShowsWorkspaceWideList(): Promise<boolean> {
    // A cross-project browser would render many project/work-item cards or a
    // list grid; the negative row asserts none exists.
    const grid = this.page.locator('[class*="grid-cols-"]').filter({
      has: this.page.locator('a[href*="/projects/"]'),
    });
    return this.isShown(grid);
  }

  // --- top-bar search box (SHELL-081) ---
  //     The top navigation always mounts the plain-text search input; focusing
  //     it opens an inline cmdk panel (no dialog). The panel only ever
  //     coexists with a closed modal palette in these scenarios, so page-level
  //     cmdk readers observe it while the modal readers stay dialog-scoped.

  private topBarSearchInput(): Locator {
    return this.page.getByPlaceholder("Search commands...").first();
  }

  async topBarSearchPlaceholder(): Promise<string | null> {
    return this.readAttr(this.topBarSearchInput(), "placeholder");
  }

  async focusTopBarSearch(): Promise<void> {
    await this.topBarSearchInput().click();
  }

  async isTopBarResultsOpen(): Promise<boolean> {
    if (await this.isCommandPaletteOpen()) return false;
    return this.isShown(this.page.locator("[cmdk-list]"));
  }

  async typeInTopBarSearch(text: string): Promise<void> {
    await this.topBarSearchInput().pressSequentially(text);
  }

  async topBarSearchValue(): Promise<string> {
    return (await this.readValue(this.topBarSearchInput())) ?? "";
  }

  async topBarResultsCommandTitles(): Promise<string[]> {
    const texts = await this.page.locator("[cmdk-item]").allTextContents();
    return texts.map((t) => t.replace(/\s+/g, " ").trim()).filter((t) => t.length > 0);
  }

  async pressInTopBarSearch(key: string): Promise<void> {
    await this.topBarSearchInput().press(key);
  }

  async closeTopBarViaOutsideClick(): Promise<void> {
    // The inline panel closes on outside mousedown; a top-left click lands
    // far from the centered top-bar field.
    await this.page.mouse.click(5, 5);
  }

  // --- shared empty-state kit tiers (SHELL-104) ---

  private emptyStateBox(title: string): Locator {
    // The box is the nearest ancestor div of the exact title text that also
    // carries the tier's media or actions (an img, a button, or both); page
    // chrome outside the box never leaks into the read.
    return this.page.getByText(title, { exact: true }).first().locator("xpath=ancestor::div[.//img or .//button][1]");
  }

  async titledEmptyState(title: string): Promise<{
    description: string | null;
    imageSrc: string | null;
    buttons: string[];
  } | null> {
    const titleEl = this.page.getByText(title, { exact: true }).first();
    if (!(await this.isShown(titleEl))) return null;
    const box = this.emptyStateBox(title);
    if ((await box.count()) === 0) return null;
    const img = box.locator("img").first();
    const buttonTexts = await box.getByRole("button").allTextContents();
    // The description is a paragraph (Simple, Detailed) or, for Section, the
    // leaf span that is neither the title nor button chrome.
    let description: string | null = null;
    const para = box.locator("p").first();
    if (await this.isShown(para)) {
      description = ((await para.textContent()) ?? "").replace(/\s+/g, " ").trim() || null;
    } else {
      const spans = box.locator("span");
      for (let i = 0, n = await spans.count(); i < n; i += 1) {
        const candidate = spans.nth(i);
        if ((await candidate.locator("xpath=ancestor::button[1]").count()) > 0) continue;
        const text = ((await candidate.textContent()) ?? "").replace(/\s+/g, " ").trim();
        if (text.length > 0 && text !== title) {
          description = text;
          break;
        }
      }
    }
    return {
      description,
      imageSrc: (await this.isShown(img)) ? await img.getAttribute("src") : null,
      buttons: buttonTexts.map((t) => t.replace(/\s+/g, " ").trim()).filter((t) => t.length > 0),
    };
  }

  async clickEmptyStateAction(title: string, label: string): Promise<void> {
    const button = this.emptyStateBox(title).getByRole("button", { name: label }).first();
    await button.waitFor({ timeout: 60_000 });
    await button.click();
  }

  async typeInIssueSearchModal(text: string): Promise<void> {
    // The modal's search input is the only "Type to search" box on the page
    // (the top-bar search reads "Search commands...").
    const input = this.page.getByPlaceholder("Type to search").first();
    await input.waitFor({ timeout: 60_000 });
    await input.pressSequentially(text);
  }

  // --- cover-image primitive (SHELL-105) ---

  private projectCards(): Locator {
    // Project cards link to the project's issues list and always render the
    // cover slot (shimmer while empty, an image once art resolves), which
    // tells them apart from plain navigation links to the same area.
    return this.page
      .locator('a[href*="/projects/"][href*="/issues"]')
      .filter({ has: this.page.locator("img, .animate-pulse") });
  }

  async projectCardCoverSrcs(): Promise<(string | null)[]> {
    const cards = this.projectCards();
    const count = await cards.count();
    const srcs: (string | null)[] = [];
    for (let i = 0; i < count; i += 1) {
      const img = cards.nth(i).locator("img").first();
      srcs.push((await this.isShown(img)) ? await img.getAttribute("src") : null);
    }
    return srcs;
  }

  async projectCardCoverShimmerVisible(): Promise<boolean> {
    return this.isShown(this.projectCards().locator(".animate-pulse").first());
  }

  // -------------------------------------------------------------------------
  // Projects list + lifecycle (NEWFRONT-124, rows SHELL-024..045).
  // Selectors follow the old app's projects-list DOM as read from source
  // (card is a <Link href=".../projects/{id}/issues"> with an <h3> name and a
  // <p> short code; empty states render an <h3> heading; confirm dialogs are
  // ModalCore with a heading + named buttons + placeholder-only inputs).
  // User-visible targeting (getByRole/getByText/getByPlaceholder) throughout;
  // no data-testid is added to apps/web. The oracle driver is extended here,
  // never forked.
  // -------------------------------------------------------------------------

  /** A project card, located by the name heading inside a project link. */
  private cardByName(name: string): Locator {
    return this.page
      .locator('a[href*="/projects/"][href*="/issues"]')
      .filter({ has: this.page.getByRole("heading", { name, exact: true }) });
  }

  async openArchivedProjects(workspaceSlug: string): Promise<void> {
    const path = `/${workspaceSlug}/projects/archives`;
    await this.page.goto(path);
    await this.page.waitForLoadState("domcontentloaded");
    await this.awaitAppBoot(path);
  }

  async visibleProjectCardNames(): Promise<string[]> {
    const headings = this.page.locator('a[href*="/projects/"][href*="/issues"]').getByRole("heading");
    const texts = await headings.allTextContents();
    return texts.map((t) => t.trim()).filter((t) => t.length > 0);
  }

  async awaitProjectCard(name: string): Promise<void> {
    await this.cardByName(name).first().waitFor({ timeout: 60_000 });
  }

  async gridColumnCount(): Promise<number> {
    // The card grid sets grid-template-columns; count the resolved tracks.
    const grid = this.page.locator('[class*="grid-cols-"]').filter({ has: this.page.locator('a[href*="/projects/"]') });
    const cols = await grid.first().evaluate((el) => {
      const tpl = getComputedStyle(el).gridTemplateColumns;
      return tpl.split(" ").filter((t) => t.trim().length > 0).length;
    });
    return cols;
  }

  async setViewportWidth(width: number): Promise<void> {
    await this.page.setViewportSize({ width, height: 1000 });
  }

  async isProjectsSkeletonVisible(): Promise<boolean> {
    const shimmer = this.page.locator(".animate-pulse");
    const names = await this.visibleProjectCardNames();
    return names.length === 0 && (await this.isShown(shimmer));
  }

  async emptyStateHeading(): Promise<string | null> {
    for (const text of ["No active projects", "No matching results.", "No projects archived"]) {
      const loc = this.page.getByRole("heading", { name: text, exact: false });
      if (await this.isShown(loc)) return (await loc.first().textContent())?.trim() ?? text;
    }
    return null;
  }

  private emptyStateCreateButton(): Locator {
    return this.page.getByRole("button", { name: "Start your first project", exact: true });
  }

  async isEmptyStateCreateVisible(): Promise<boolean> {
    return this.isShown(this.emptyStateCreateButton());
  }

  async isEmptyStateCreateEnabled(): Promise<boolean> {
    return this.emptyStateCreateButton().isEnabled();
  }

  async clickEmptyStateCreate(): Promise<void> {
    await this.emptyStateCreateButton().click();
  }

  async emptyStateArtworkSignature(): Promise<string | null> {
    // The empty-state artwork sits next to its heading inside the same
    // block; the page carries several landmarks, so climb from the heading
    // to the nearest ancestor holding illustrations and signature the
    // biggest one (icons are an order of magnitude smaller than the art).
    const heading = this.page.getByRole("heading", {
      name: /No active projects|No matching results\.|No projects archived/,
    });
    if (!(await this.isShown(heading))) return null;
    return heading.first().evaluate((node) => {
      const area = (el: SVGSVGElement): number => {
        const rect = el.getBoundingClientRect();
        return rect.width * rect.height;
      };
      let scope: HTMLElement | null = node as HTMLElement;
      for (let depth = 0; depth < 6 && scope; depth += 1) {
        const svgs = [...scope.querySelectorAll("svg")];
        if (svgs.length > 0) {
          const biggest = svgs.reduce((a, b) => (area(a) >= area(b) ? a : b));
          const html = biggest.outerHTML;
          return `${html.length}:${html.slice(0, 160)}`;
        }
        scope = scope.parentElement;
      }
      return null;
    });
  }

  private headerCreateButton(): Locator {
    // Header create button: label "Add Project" (>=sm) or "Project" (below sm).
    return this.page.getByRole("button", { name: /^(Add Project|Project)$/ });
  }

  async isHeaderCreateButtonVisible(): Promise<boolean> {
    return this.isShown(this.headerCreateButton());
  }

  async headerCreateButtonLabel(): Promise<string | null> {
    // innerText, not textContent: both the full and the short label mount
    // and CSS hides one, so only rendered text tells them apart.
    const btn = this.headerCreateButton();
    if (!(await this.isShown(btn))) return null;
    return (
      (
        await btn
          .first()
          .innerText()
          .catch(() => null)
      )?.trim() ?? null
    );
  }

  async clickHeaderCreateButton(): Promise<void> {
    await this.headerCreateButton().first().click();
  }

  async breadcrumbLabels(): Promise<string[]> {
    const texts = await this.breadcrumbItems().allTextContents();
    return texts.map((t) => t.trim()).filter((t) => t.length > 0);
  }

  async isMobileListHeaderVisible(): Promise<boolean> {
    // Mobile header order-by/filter bar lives in an md:hidden container.
    const orderBy = this.page.locator('[class*="md:hidden"]').getByRole("button", { name: /Filters/ });
    return this.isShown(orderBy);
  }

  async isDesktopFilterRowVisible(): Promise<boolean> {
    const row = this.page.locator('[class*="hidden"][class*="md:flex"]').getByRole("button", { name: "Filters" });
    return this.isShown(row);
  }

  private breadcrumbItems(): Locator {
    // The shared Breadcrumbs primitive renders role-less divs: a flex-grow
    // row whose items are h-6 rows (link or plain-text label + chevron).
    const root = this.page
      .locator("div.flex.flex-grow.items-center")
      .filter({ has: this.page.locator("div.flex.h-6.items-center") })
      .first();
    return root.locator("div.flex.h-6.items-center");
  }

  async breadcrumbTerminalIsLink(): Promise<boolean> {
    const items = this.breadcrumbItems();
    const count = await items.count();
    if (count === 0) return false;
    const last = items.nth(count - 1);
    return (await last.getByRole("link").count()) > 0;
  }

  private sortTrigger(): Locator {
    // Order-by trigger shows the current option label among Manual/Name/...
    return this.page.getByRole("button", { name: /(Manual|Name|Created date|Number of members)/ });
  }

  private async shownTrigger(trigger: Locator): Promise<Locator> {
    // Role queries match hidden nodes too, and the desktop filter row and
    // the mobile bar both mount sort/filter triggers; drive the visible
    // one so the same methods work at desktop and phone widths.
    const count = await trigger.count();
    for (let index = 0; index < count; index += 1) {
      if (
        await trigger
          .nth(index)
          .isVisible()
          .catch(() => false)
      )
        return trigger.nth(index);
    }
    return trigger.first();
  }

  async openSortMenu(): Promise<void> {
    await (await this.shownTrigger(this.sortTrigger())).click();
  }

  async selectSortOption(label: string): Promise<void> {
    await this.page.getByRole("menuitem", { name: label, exact: true }).first().click();
  }

  async currentSortLabel(): Promise<string> {
    const trigger = await this.shownTrigger(this.sortTrigger());
    return ((await trigger.textContent()) ?? "").trim();
  }

  async isSortDirectionDisabled(): Promise<boolean> {
    const asc = this.page.getByRole("menuitem", { name: "Ascending", exact: true }).first();
    return asc.isDisabled().catch(() => true);
  }

  async closeMenu(): Promise<void> {
    await this.page.keyboard.press("Escape");
  }

  private filterTrigger(): Locator {
    return this.page.getByRole("button", { name: "Filters" });
  }

  async openFilterMenu(): Promise<void> {
    await (await this.shownTrigger(this.filterTrigger())).click();
  }

  private filterPanel(): Locator {
    // The panel is the only fixed-position popover carrying a Search box
    // (top-bar, sidebar and list searches live in non-fixed scopes).
    return this.page.locator("div.fixed", { has: this.page.getByPlaceholder("Search") });
  }

  async typeFilterSearch(text: string): Promise<void> {
    await this.filterPanel().getByPlaceholder("Search").fill(text);
  }

  async filterMenuHasOption(text: string): Promise<boolean> {
    return this.isShown(this.filterPanel().getByText(text, { exact: true }));
  }

  async selectFilterOption(label: string): Promise<void> {
    await this.filterPanel().getByText(label, { exact: true }).first().click();
  }

  async isFilterBadgeVisible(): Promise<boolean> {
    // Active-filter dot renders as a small accent span on the trigger.
    const trigger = await this.shownTrigger(this.filterTrigger());
    const dot = trigger.locator('span[class*="bg-accent-primary"]');
    return this.isShown(dot);
  }

  private appliedFilterStrip(): Locator {
    // The applied-filters strip sits above the grid; scope chips to it.
    return this.page.locator('[class*="flex"][class*="flex-wrap"]').filter({ hasText: "Clear all" }).first();
  }

  async appliedFilterChipTexts(): Promise<string[]> {
    const strip = this.appliedFilterStrip();
    if (!(await this.isShown(strip))) return [];
    const texts = await strip.locator("span, div").allTextContents();
    return texts.map((t) => t.trim()).filter((t) => t.length > 0);
  }

  async removeAppliedFilterChip(text: string): Promise<void> {
    const chip = this.page.locator("*").filter({ hasText: text }).last();
    await chip.getByRole("button").last().click();
  }

  async clickClearAllFilters(): Promise<void> {
    await this.page.getByText("Clear all", { exact: true }).first().click();
  }

  async filterMatchCountText(): Promise<string | null> {
    const count = this.page.getByText(/^\d+\/\d+$/).first();
    if (!(await this.isShown(count))) return null;
    return (await count.textContent())?.trim() ?? null;
  }

  private listToolbar(): Locator {
    // The list toolbar is the only @container scope holding a Search box,
    // which tells it apart from the top-bar and sidebar search inputs.
    return this.page.locator('[class*="@container"]');
  }

  private listSearchInput(): Locator {
    return this.listToolbar().getByPlaceholder("Search");
  }

  async openListSearch(): Promise<void> {
    if (await this.isShown(this.listSearchInput())) return;
    // Collapsed until the toolbar's icon-only search button expands it; it
    // is the first button in the toolbar (sort, Filters, create follow).
    await this.listToolbar().getByRole("button").first().click();
    // Settle: the expand lags the click, and a second opener must see the
    // open field rather than clicking the magnifier a second time.
    await this.listSearchInput().waitFor({ state: "visible", timeout: 10_000 });
  }

  async typeListSearch(text: string): Promise<void> {
    // Human-scale keystrokes, not fill: fill's single synthetic input event
    // leaves the outside-click detector holding a stale closure, so the
    // next outside click collapses the field despite the text. Spaced-out
    // trusted key events flush every keystroke and the field stays open.
    await this.openListSearch();
    await this.listSearchInput().pressSequentially(text, { delay: 60 });
  }

  async listSearchValue(): Promise<string> {
    return this.listSearchInput().inputValue();
  }

  async isListSearchExpanded(): Promise<boolean> {
    return this.isShown(this.listSearchInput());
  }

  async pressEscapeInListSearch(): Promise<void> {
    await this.listSearchInput().press("Escape");
  }

  async clickListSearchClear(): Promise<void> {
    await this.listSearchInput().locator("xpath=following-sibling::button").first().click();
  }

  async clickOutsideListSearch(): Promise<void> {
    // The leading breadcrumb is plain text with no handlers: a click there
    // is purely "outside" the search field without navigating anywhere.
    await this.breadcrumbItems().first().click();
  }

  async cardShortCode(name: string): Promise<string | null> {
    const card = this.cardByName(name).first();
    if (!(await this.isShown(card))) return null;
    const text = await card.locator("p").first().textContent();
    return text?.trim() ?? null;
  }

  async cardHasPrivateMark(name: string): Promise<boolean> {
    // The lock mark is a class-less svg rendered only for private projects,
    // as the sibling of the identifier line inside its own span.
    const card = this.cardByName(name).first();
    const idLine = card.locator("p").first().locator("xpath=parent::*");
    return this.isShown(idLine.locator("svg"));
  }

  async cardSubText(name: string): Promise<string | null> {
    const card = this.cardByName(name).first();
    const sub = card.locator("p.line-clamp-2, p[class*='line-clamp-2']").first();
    if (!(await this.isShown(sub))) return null;
    return (await sub.textContent())?.trim() ?? null;
  }

  private favoriteStar(name: string): Locator {
    const card = this.cardByName(name).first();
    return card.locator("button").filter({ has: this.page.locator('svg[class*="star" i]') });
  }

  async cardHasFavoriteStar(name: string): Promise<boolean> {
    return this.isShown(this.favoriteStar(name));
  }

  async clickFavoriteStar(name: string): Promise<void> {
    await this.favoriteStar(name).first().click();
  }

  async cardHasCoverImage(name: string): Promise<boolean> {
    // The cover carries the project name as its accessible label; a
    // cover-less card renders a loading block instead of an image.
    const card = this.cardByName(name).first();
    return this.isShown(card.getByRole("img", { name, exact: true }));
  }

  async cardHasLogo(name: string): Promise<boolean> {
    // The logo box holds an emoji glyph or an icon once a logo is set and
    // stays an empty skeleton otherwise.
    const card = this.cardByName(name).first();
    const box = card.locator("div.grid.h-9.w-9").first();
    if (!(await this.isShown(box))) return false;
    const text = ((await box.textContent()) ?? "").trim();
    if (text.length > 0) return true;
    return (await box.locator("svg").count()) > 0;
  }

  async cardAvatarStack(name: string): Promise<string[]> {
    // Avatar circles are the only fully round nodes on a card, nested three
    // deep per avatar; the leafmost circles carry one initial each, and the
    // stack overflows into a "+N" bubble past its display cap.
    const card = this.cardByName(name).first();
    return card.locator("div.rounded-full").evaluateAll((nodes) =>
      nodes
        .filter((node) => node.querySelector("div.rounded-full") === null)
        .map((node) => (node.textContent ?? "").trim())
        .filter((text) => text.length > 0)
    );
  }

  async clickProjectCard(name: string): Promise<void> {
    await this.cardByName(name).first().click();
  }

  async openCardContextMenu(name: string): Promise<void> {
    await this.cardByName(name).first().click({ button: "right" });
  }

  async contextMenuItemLabels(): Promise<string[]> {
    // The card context menu renders its entries as buttons inside its own
    // container (unlike headless-ui menus, which use the menuitem role).
    const menu = this.page.locator('[data-context-menu="true"]');
    const texts = await menu.getByRole("button").allTextContents();
    return texts.map((t) => t.trim()).filter((t) => t.length > 0);
  }

  async clickCardContextMenuItem(label: string): Promise<void> {
    // The card menu opens at the viewport origin (beneath the fixed header)
    // instead of at the cursor, so its entries fail pointer hit-testing and
    // its ArrowDown/Enter path stays inert; dispatch the click to the verified
    // entry directly. The menu opening, the entry list, the item action and
    // the resulting dialog are all the real app behavior.
    const menu = this.page.locator('[data-context-menu="true"]');
    const entry = menu.getByRole("button", { name: label, exact: true });
    if ((await entry.count()) === 0) {
      const labels = await this.contextMenuItemLabels();
      throw new Error(
        `[parity] card menu has no ${JSON.stringify(label)} entry (open menu shows ${JSON.stringify(labels)}).`
      );
    }
    await entry.first().evaluate((node) => (node as HTMLElement).click());
  }

  async cardFooterLabels(name: string): Promise<string[]> {
    const card = this.cardByName(name).first();
    const texts = await card.getByRole("button").allTextContents();
    const links = await card.getByRole("link").allTextContents();
    return [...texts, ...links].map((t) => t.trim()).filter((t) => t.length > 0);
  }

  async clickCardJoin(name: string): Promise<void> {
    await this.cardByName(name).first().getByRole("button", { name: "Join", exact: true }).click();
  }

  async isJoinDialogVisible(): Promise<boolean> {
    return this.isShown(this.page.getByRole("heading", { name: "Join Project?", exact: true }));
  }

  async joinDialogHeading(): Promise<string | null> {
    const h = this.page.getByRole("heading", { name: "Join Project?", exact: true });
    if (!(await this.isShown(h))) return null;
    return (await h.first().textContent())?.trim() ?? null;
  }

  async confirmJoin(): Promise<void> {
    await this.page.getByRole("button", { name: "Join Project", exact: true }).click();
  }

  /**
   * Leave is offered from the project quick-actions menus (detail header
   * and sidebar project item); both render it only for members without an
   * admin/member project role, i.e. guests. The sidebar's trigger is the
   * reachable one: hover the project link to reveal it, then open the menu.
   * Scoped to the main sidebar so the peek twin never matches.
   */
  async openLeaveProjectDialog(projectName: string): Promise<void> {
    const sidebar = this.page.getByRole("complementary", { name: "Main sidebar" });
    await sidebar.getByText(projectName, { exact: true }).first().hover();
    await sidebar.getByLabel("Toggle quick actions menu").click();
    await this.page.getByRole("menuitem", { name: "Leave project", exact: true }).click();
  }

  async fillLeaveProjectName(text: string): Promise<void> {
    await this.page.getByPlaceholder("Enter project name").fill(text);
  }

  async fillLeaveConfirmPhrase(text: string): Promise<void> {
    await this.page.getByPlaceholder("Enter 'leave project'").fill(text);
  }

  async submitLeave(): Promise<void> {
    await this.page.getByRole("button", { name: "Leave Project", exact: true }).click();
  }

  async leaveErrorText(): Promise<string | null> {
    for (const text of [
      "Please enter the project name as shown in the description.",
      "Please confirm leaving the project by typing the 'Leave Project'.",
    ]) {
      const loc = this.page.getByText(text, { exact: false });
      if (await this.isShown(loc)) return (await loc.first().textContent())?.trim() ?? text;
    }
    return null;
  }

  async isLeaveDialogVisible(): Promise<boolean> {
    return this.isShown(this.page.getByRole("heading", { name: "Leave Project", exact: true }));
  }

  /**
   * Archiving is offered from the project settings control section (the
   * card and sidebar menus carry no Archive entry), so open the settings
   * page for the project and start the Archive control.
   */
  async openArchiveProjectDialog(workspaceSlug: string, projectId: string): Promise<void> {
    const path = `/${workspaceSlug}/settings/projects/${projectId}`;
    await this.page.goto(path);
    await this.page.waitForLoadState("domcontentloaded");
    await this.awaitAppBoot(path);
    await this.page.getByRole("button", { name: "Archive", exact: true }).click();
  }

  async archiveDialogBodyText(): Promise<string | null> {
    const body = this.page.getByText(/will be archived|Restoring a project/i).first();
    if (!(await this.isShown(body))) return null;
    return (await body.textContent())?.trim() ?? null;
  }

  private archivedCard(name: string): Locator {
    return this.cardByName(name).first();
  }

  async clickCardRestore(name: string): Promise<void> {
    // The archived footer renders restore as nested clickable text nodes,
    // not a button; the first (outer) match carries the click handler.
    await this.archivedCard(name).getByText("Restore", { exact: true }).first().click();
  }

  async confirmRestore(): Promise<void> {
    // Scope to the dialog: the archived card behind it carries its own restore control.
    await this.modalScope().getByRole("button", { name: "Restore", exact: true }).first().click();
  }

  async archivedCardHasAdminActions(name: string): Promise<boolean> {
    const card = this.archivedCard(name);
    return this.isShown(card.getByText("Restore", { exact: true }));
  }

  async cardShowsArchivedMarker(name: string): Promise<boolean> {
    return this.isShown(this.archivedCard(name).getByText("Archived", { exact: false }));
  }

  async isRestoreDialogVisible(): Promise<boolean> {
    return this.isShown(this.modalScope().getByRole("heading", { name: /^Restore / }));
  }

  async openDeleteProjectDialog(name: string): Promise<void> {
    await this.openCardContextMenu(name);
    await this.clickCardContextMenuItem("Delete");
  }

  async fillDeleteProjectName(text: string): Promise<void> {
    await this.page.getByPlaceholder("Project name").fill(text);
  }

  async fillDeleteConfirmPhrase(text: string): Promise<void> {
    await this.page.getByPlaceholder("Enter 'delete my project'").fill(text);
  }

  async isDeleteSubmitDisabled(): Promise<boolean> {
    return this.page.getByRole("button", { name: "Delete project", exact: true }).isDisabled();
  }

  async submitDelete(): Promise<void> {
    await this.page.getByRole("button", { name: "Delete project", exact: true }).click();
  }

  async isCreateProjectDialogVisible(): Promise<boolean> {
    return this.isShown(this.page.getByPlaceholder("Project name"));
  }

  async fillCreateProjectName(text: string): Promise<void> {
    await this.page.getByPlaceholder("Project name").fill(text);
  }

  async createProjectShortCodeValue(): Promise<string> {
    return this.page.getByPlaceholder("Project ID").inputValue();
  }

  async fillCreateProjectShortCode(text: string): Promise<void> {
    await this.page.getByPlaceholder("Project ID").fill(text);
  }

  async submitCreateProject(): Promise<void> {
    // A sticky app-level toast (e.g. the cover-upload degradation warning,
    // which never auto-dismisses) can sit over the submit button and
    // intercept pointer clicks forever; the button itself is verified
    // present, so dispatch the click when the pointer cannot land.
    const btn = this.page.getByRole("button", { name: "Create project", exact: true });
    try {
      await btn.click({ timeout: 10_000 });
    } catch {
      await btn.first().evaluate((node) => (node as HTMLElement).click());
    }
  }

  async createProjectErrorText(): Promise<string | null> {
    for (const text of [
      "The project name is already taken.",
      "The project identifier is already taken.",
      "Cover image upload skipped — using a default cover.",
    ]) {
      const loc = this.page.getByText(text, { exact: false });
      if (await this.isShown(loc)) return (await loc.first().textContent())?.trim() ?? text;
    }
    return null;
  }

  async createFormCoverVisible(): Promise<boolean> {
    return this.isShown(this.modalScope().getByRole("img", { name: "Project cover image" }));
  }

  async createFormIconVisible(): Promise<boolean> {
    // The icon picker label holds the prefilled emoji glyph or icon and
    // stays empty until a logo value exists.
    const box = this.modalScope().locator("span.grid.h-11.w-11").first();
    if (!(await this.isShown(box))) return false;
    const text = ((await box.textContent()) ?? "").trim();
    if (text.length > 0) return true;
    return (await box.locator("svg").count()) > 0;
  }

  async toggleInvitation(workspaceName: string): Promise<void> {
    await this.page.locator("div.cursor-pointer", { hasText: workspaceName }).first().click();
  }

  async acceptSelectedInvitations(): Promise<void> {
    const page = this.page;
    const accept = page.getByRole("button", { name: /accept.*join/i });
    await accept.click();
  }

  async invitationsEmptyStateVisible(): Promise<boolean> {
    return (await this.page.getByRole("heading", { name: /no pending invites/i }).count()) > 0;
  }

  async openInvitationLink(workspaceSlug: string, invitationId: string, token: string): Promise<void> {
    const params = new URLSearchParams({ invitation_id: invitationId, slug: workspaceSlug, token });
    await this.page.goto(`/workspace-invitations?${params.toString()}`);
    await this.page.waitForLoadState("domcontentloaded");
  }

  async pageText(): Promise<string> {
    await this.page.locator("body").waitFor();
    return ((await this.page.locator("body").innerText()) ?? "").trim();
  }

  async acceptSingleInvitation(): Promise<void> {
    await this.page.getByRole("button", { name: "Accept" }).click();
  }

  async declineSingleInvitation(): Promise<void> {
    await this.page.getByRole("button", { name: "Ignore" }).click();
  }

  async openOnboarding(): Promise<void> {
    await this.page.goto("/onboarding");
    await this.page.waitForLoadState("domcontentloaded");
  }

  private async clickStepButton(name: string | RegExp): Promise<void> {
    await this.page.getByRole("button", { name }).first().click();
  }

  async advanceCliInstall(): Promise<void> {
    await this.clickStepButton(/done, continue/i);
  }

  async skipCliInstall(): Promise<void> {
    await this.clickStepButton(/skip for now/i);
  }

  async submitProfileStep(displayName: string): Promise<void> {
    const page = this.page;
    const nameField = page.getByPlaceholder("Enter your full name");
    await nameField.waitFor();
    await nameField.fill(displayName);
    await this.clickStepButton(/continue/i);
  }

  async submitRoleStep(roleLabel: string): Promise<void> {
    const page = this.page;
    await page.getByRole("button", { name: roleLabel }).click();
    await this.clickStepButton(/^continue$/i);
  }

  async skipRoleStep(): Promise<void> {
    await this.clickStepButton(/^skip$/i);
  }

  async submitUseCaseStep(useCaseLabels: string[]): Promise<void> {
    const page = this.page;
    for (const label of useCaseLabels) {
      await page.getByRole("button", { name: label }).click();
    }
    await this.clickStepButton(/^continue$/i);
  }

  async skipUseCaseStep(): Promise<void> {
    await this.clickStepButton(/^skip$/i);
  }

  async goBackOnboardingStep(): Promise<void> {
    // The header back control is the only chevron button on the step.
    await this.page
      .locator("button")
      .filter({ has: this.page.locator("svg") })
      .first()
      .click();
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
  // Body reads (rows, group headers, tiles) share the same budget: the
  // chrome the first wait settles on can precede the body paint by ~150s
  // on the loaded host (blank page, then chrome, then rows), so a 120s
  // body wait expires just as content arrives.
  private static readonly LAYOUTS_BODY_WAIT_MS = 300_000;

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
    // Phone viewports hide the desktop switcher entirely; derive the
    // active layout from the rendered layout container instead. The
    // probe is short so desktop runs keep the marker path.
    const switcherPresent = await buttons
      .first()
      .waitFor({ timeout: 10_000 })
      .then(() => true)
      .catch(() => false);
    if (!switcherPresent) return this.layoutsActiveLayoutFromContainers();
    await buttons.first().waitFor({ timeout: WebDriver.LAYOUTS_FIRST_WAIT_MS });
    // The buttons render before the stored selection applies (filters
    // still fetching), so the marker scan polls instead of reading once.
    const deadline = Date.now() + WebDriver.LAYOUTS_BODY_WAIT_MS;
    for (;;) {
      const count = await buttons.count();
      for (let i = 0; i < count; i++) {
        const cls = (await buttons.nth(i).getAttribute("class")) ?? "";
        if (cls.includes("bg-layer-transparent-active")) {
          const key = WebDriver.LAYOUTS_ORDER[i];
          if (key === undefined) throw new Error(`[parity] switcher has no layout key at index ${i}.`);
          return key;
        }
      }
      if (Date.now() >= deadline) throw new Error("[parity] no switcher button carries the active marker.");
      await this.page.waitForTimeout(500);
    }
  }

  private async layoutsActiveLayoutFromContainers(): Promise<LayoutsLayoutKey> {
    // Exactly one layout container renders at a time; poll until one
    // reports visible. Mid-transition doubles resolve to the first hit,
    // and callers polling for a target (SwitchTo) self-heal.
    const deadline = Date.now() + WebDriver.LAYOUTS_BODY_WAIT_MS;
    for (;;) {
      if (await this.layoutsListVisible()) return "list";
      if (await this.layoutsKanbanVisible()) return "kanban";
      if (await this.layoutsCalendarVisible()) return "calendar";
      if (await this.layoutsSpreadsheetVisible()) return "spreadsheet";
      if (await this.layoutsGanttVisible()) return "gantt_chart";
      if (Date.now() >= deadline) throw new Error("[parity] no layout container became visible.");
      await this.page.waitForTimeout(500);
    }
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
    await buttons.nth(index).scrollIntoViewIfNeeded({ timeout: WebDriver.LAYOUTS_BODY_WAIT_MS });
    await buttons.nth(index).click();
    // Clicking the active layout is a specified no-op; the marker is
    // already visible then, so this wait resolves immediately.
    await this.layoutsWaitForLayout(layout);
  }

  async layoutsReloadIssues(): Promise<void> {
    // domcontentloaded (see openAuthenticated): the load event is minutes
    // out on the dev oracle; the switcher wait below is the real gate.
    await this.page.reload({ waitUntil: "domcontentloaded" });
    // Phone viewports hide the desktop switcher entirely, so gate on
    // any layout container there; desktop keeps the switcher path.
    if ((this.page.viewportSize()?.width ?? 1280) < 768) {
      await this.layoutsActiveLayoutFromContainers();
      return;
    }
    await this.layoutsSwitcherButtons().first().waitFor({ timeout: WebDriver.LAYOUTS_FIRST_WAIT_MS });
  }

  async layoutsListVisible(): Promise<boolean> {
    // Immediate read, no waiting: switch assertions poll through
    // layoutsSwitchTo, and absence must read fast.
    return (await this.layoutsGroupHeaders().count()) > 0;
  }

  async layoutsCalendarVisible(): Promise<boolean> {
    // The Options trigger is icon-only (nameless) on phones, so the
    // mobile read falls back to Today-plus-tiles; desktop keeps the
    // Options read and satisfies the fallback identically.
    if ((await this.page.getByRole("button", { name: "Options" }).count()) > 0) return true;
    return (
      (await this.page.getByRole("button", { name: "Today" }).count()) > 0 && (await this.layoutsCalTiles().count()) > 0
    );
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
    // board shows no cards, so the desktop switcher's active marker
    // confirms it (the switch scenario separately proves cards render
    // when expanded). The marker is scanned directly — never through
    // layoutsActiveLayout, which recurses here through its container
    // fallback on switcher-less pages and spins to the test timeout.
    const otherMarkers =
      (await this.page.getByText("All work items", { exact: true }).count()) +
      (await this.page.getByRole("button", { name: "Options" }).count()) +
      (await this.page.getByText("Work items", { exact: true }).count()) +
      (await this.page.getByText("Quarter", { exact: true }).count());
    if (otherMarkers > 0) return false;
    if ((await this.page.locator('a[id^="issue-"]').count()) > 0) return true;
    const buttons = this.layoutsSwitcherButtons();
    if (
      !(await buttons
        .first()
        .isVisible()
        .catch(() => false))
    )
      return false;
    const count = await buttons.count();
    for (let i = 0; i < count; i++) {
      if (((await buttons.nth(i).getAttribute("class")) ?? "").includes("bg-layer-transparent-active")) {
        return WebDriver.LAYOUTS_ORDER[i] === "kanban";
      }
    }
    return false;
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
      // Bounded like the row reads: a re-render mid-scan must not hang
      // the caller to the test timeout.
      const text = await header.innerText({ timeout: 10_000 }).catch(() => "");
      if (text !== "" && WebDriver.layoutsHeaderTitle(text) === title) return sections.nth(i);
    }
    return null;
  }

  private async layoutsGroupSection(title: string): Promise<Locator> {
    // The list body (sections) renders after the header chrome the page
    // waits settle on, so a fresh open/reload needs a bounded wait here
    // instead of an immediate throw. The returned locator is anchored to
    // the header text, not a section index: the list re-renders (and
    // reorders sections) as groups fetch, so a positional nth() goes stale
    // between discovery and use. State names in parity specs are distinct
    // non-substrings, so the substring filter is unambiguous in practice.
    const deadline = Date.now() + WebDriver.LAYOUTS_BODY_WAIT_MS;
    for (;;) {
      const found = await this.layoutsGroupSectionFast(title);
      if (found) {
        return this.page.locator('div[data-drop-target-for-element="true"]').filter({
          has: this.page.locator('div[class*="group/list-header"]', { hasText: title }),
        });
      }
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
      // A row mid-virtualization can detach between count() and the
      // read; an unbounded read would hang to the test timeout, so a
      // stuck row is skipped and the caller's poll re-reads instead.
      const text = await rows
        .nth(i)
        .locator("p")
        .first()
        .innerText({ timeout: 10_000 })
        .catch(() => "");
      if (text.trim() !== "") names.push(text.trim());
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
    // Fail fast when the row is absent: an unbounded click would hang to
    // the test timeout (720s) with no actionable error.
    const more = section.getByText("Load more").first();
    await more.waitFor({ timeout: WebDriver.LAYOUTS_FIRST_WAIT_MS });
    await more.click({ timeout: WebDriver.LAYOUTS_FIRST_WAIT_MS });
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
    await rows.nth(count - 1).scrollIntoViewIfNeeded({ timeout: WebDriver.LAYOUTS_BODY_WAIT_MS });
  }

  async layoutsListQuickAdd(title: string, groupTitle?: string): Promise<void> {
    const scope = groupTitle === undefined ? this.page : await this.layoutsGroupSection(groupTitle);
    const trigger = scope.locator("div.sticky.bottom-0", { hasText: "New work item" }).first();
    await trigger.scrollIntoViewIfNeeded({ timeout: WebDriver.LAYOUTS_BODY_WAIT_MS });
    await trigger.click();
    const field = this.page.getByPlaceholder("Work item title");
    await field.waitFor({ timeout: WebDriver.LAYOUTS_FIRST_WAIT_MS });
    await field.fill(title);
    await field.press("Enter");
    // The row appearing proves the save landed; the title is unique per
    // scenario run, so this cannot match a stale row.
    await this.page
      .locator('a[id^="issue-"]', { hasText: title })
      .first()
      .waitFor({ timeout: WebDriver.LAYOUTS_BODY_WAIT_MS });
  }

  async layoutsRowCanEditState(issueName: string): Promise<boolean> {
    // Read-only viewers (guests) render the chip disabled; members get an
    // enabled chip that opens the state dropdown. The flag is the whole
    // signal — opening the dropdown to probe it leaves shared menu state
    // behind (a toggle-close race hung RowSetState for 300s), so no click.
    const chip = this.layoutsRowStateButton(issueName);
    await chip.waitFor({ timeout: WebDriver.LAYOUTS_BODY_WAIT_MS });
    return !(await chip.isDisabled());
  }

  async layoutsRowHref(issueName: string): Promise<string | null> {
    const row = this.layoutsIssueRow(issueName);
    await row.waitFor({ timeout: WebDriver.LAYOUTS_BODY_WAIT_MS });
    return row.getAttribute("href");
  }

  async layoutsRowOpenPeek(issueName: string): Promise<void> {
    const row = this.layoutsIssueRow(issueName);
    await row.locator("p").first().click();
    await this.page.waitForURL((url) => url.href.includes("peekIssueId"), { timeout: WebDriver.LAYOUTS_BODY_WAIT_MS });
    await this.layoutsPeekPanel().waitFor({ timeout: WebDriver.LAYOUTS_BODY_WAIT_MS });
  }

  async layoutsPeekVisible(): Promise<boolean> {
    if (!this.page.url().includes("peekIssueId")) return false;
    const panel = this.layoutsPeekPanel();
    return (await panel.count()) > 0 && (await panel.isVisible());
  }

  async layoutsPeekTitle(): Promise<string | null> {
    // The peek title is an editable textarea (a char counter like 18/255
    // sits beside it, which a text read would mistake for the title), so
    // the read takes the field value, falling back to the legacy line
    // scan when the field is absent. The panel shell renders before its
    // issue fetch lands, so the read waits for content first.
    const panel = this.layoutsPeekPanel();
    const deadline = Date.now() + WebDriver.LAYOUTS_BODY_WAIT_MS;
    for (;;) {
      if ((await panel.count()) > 0) {
        const field = panel.locator("textarea").first();
        if ((await field.count()) > 0) return ((await field.inputValue()) ?? "").trim() || null;
        if (/[A-Z]+-\d+/.test((await panel.innerText().catch(() => "")) ?? "")) break;
      }
      if (Date.now() >= deadline) return null;
      await this.page.waitForTimeout(500);
    }
    const field = panel.locator("textarea").first();
    if ((await field.count()) > 0) return ((await field.inputValue()) ?? "").trim() || null;
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
    await row.waitFor({ timeout: WebDriver.LAYOUTS_BODY_WAIT_MS });
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
    // Expanded children render as sibling blocks inside the parent's own
    // block container (an ancestor div carrying an issue_ id; the row
    // link itself carries issue-<id>), after the parent's own link.
    const row = this.layoutsIssueRow(issueName);
    await row.waitFor({ timeout: WebDriver.LAYOUTS_BODY_WAIT_MS });
    const block = row.locator('xpath=ancestor::div[starts-with(@id, "issue_")][1]');
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
    await chip.scrollIntoViewIfNeeded({ timeout: WebDriver.LAYOUTS_BODY_WAIT_MS });
    await chip.click();
    // The state menu is a listbox: its options carry the option role
    // (the row chip keeps the button role, so no disambiguation needed).
    const option = this.page.getByRole("option", { name: stateName, exact: true }).first();
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
    // desktop, so what remains first is the priority control. The span
    // check is explicit per candidate (a locator-level exclusion proved
    // unreliable across engine versions and matched the state chip).
    const row = this.layoutsIssueRow(issueName);
    await row.waitFor({ timeout: WebDriver.LAYOUTS_BODY_WAIT_MS });
    const candidates = row.locator("button:not([disabled])");
    const count = await candidates.count();
    for (let i = 0; i < count; i++) {
      const candidate = candidates.nth(i);
      if ((await candidate.getAttribute("aria-label")) === "Toggle quick actions menu") continue;
      if (!(await candidate.isVisible())) continue;
      if ((await candidate.locator("span").count()) > 0) continue;
      return candidate;
    }
    throw new Error(`[parity] no priority control found on row "${issueName}".`);
  }

  async layoutsRowPriority(issueName: string): Promise<string> {
    // The control is icon-only (no text, title, or label); the value is
    // encoded in a border-priority-<value> marker class on its inner
    // element. Absent marker (or a text render) reads as its fallback.
    const control = await this.layoutsRowPriorityControl(issueName);
    await control.waitFor({ timeout: WebDriver.LAYOUTS_FIRST_WAIT_MS });
    const html = (await control.innerHTML().catch(() => "")) ?? "";
    const marker = html.match(/border-priority-([a-z]+)/)?.[1];
    if (marker) return marker.charAt(0).toUpperCase() + marker.slice(1);
    const text = ((await control.innerText()) ?? "").trim();
    return text === "" ? "None" : text;
  }

  async layoutsRowSetPriority(issueName: string, priorityName: string): Promise<void> {
    const control = await this.layoutsRowPriorityControl(issueName);
    await control.scrollIntoViewIfNeeded({ timeout: WebDriver.LAYOUTS_BODY_WAIT_MS });
    await control.click();
    // Same listbox family as the state menu: options carry option role.
    const option = this.page.getByRole("option", { name: priorityName, exact: true }).first();
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

  private async layoutsDismissDetailRail(): Promise<void> {
    // Cycle/module pages float a detail rail over the list's right edge,
    // covering row triggers and group-header controls; its leading
    // text-less button dismisses it (the same control a user reaches
    // for). Absent everywhere else, where this is a no-op. Matched by
    // scrollbar + edge placement, not width: the cycle rail is wider
    // than the module rail.
    const rail = this.page.locator("div.vertical-scrollbar.absolute.right-0");
    if ((await rail.count()) === 0) return;
    const close = rail.first().locator("button").first();
    await close.waitFor({ timeout: 10_000 }).catch(() => {});
    if ((await close.count()) === 0) return;
    await close.click({ timeout: 10_000 }).catch(() => {});
    const deadline = Date.now() + 15_000;
    for (;;) {
      if ((await rail.count()) === 0) return;
      if (Date.now() >= deadline) return;
      await this.page.waitForTimeout(300);
    }
  }

  private async layoutsOpenRowMenu(issueName: string): Promise<void> {
    // List rows are anchors; the all-issues sheet renders rows as table
    // cells instead, whose trigger is likewise the last button. Either
    // shape can take minutes to arrive on a cold body, so the wait
    // polls for both rather than timing one out into the other.
    let row = this.layoutsIssueRow(issueName);
    const deadline = Date.now() + WebDriver.LAYOUTS_FIRST_WAIT_MS;
    for (;;) {
      if ((await row.count()) > 0) break;
      const cell = this.page.locator("td", { hasText: issueName }).first();
      if ((await cell.count()) > 0) {
        row = cell;
        break;
      }
      if (Date.now() >= deadline) throw new Error(`[parity] issue row "${issueName}" not found.`);
      await this.page.waitForTimeout(500);
    }
    // The rail loads with the body, so dismiss it only once the row has
    // resolved — any earlier the dismiss is a no-op on an empty page.
    await this.layoutsDismissDetailRail();
    // The trigger is the row's last button: an icon-only ellipsis with no
    // accessible name (a breakpoint duplicate renders first, hidden). It
    // sits under the property strip for automation clicks, so hover it
    // into its clickable state first — but neither hover nor click may
    // hang: when the detail rail (or a slow re-render) still covers the
    // trigger, a forced dispatch opens the menu the click cannot reach.
    const trigger = row.locator("button").last();
    await trigger.waitFor({ timeout: WebDriver.LAYOUTS_FIRST_WAIT_MS });
    await trigger.hover({ timeout: 10_000 }).catch(() => {});
    await trigger.click({ timeout: 10_000 }).catch(async () => {
      await trigger.click({ force: true }).catch(async () => {
        await trigger.focus();
        await this.page.keyboard.press("Enter");
      });
    });
    await this.page.getByRole("menuitem").first().waitFor({ timeout: 15_000 });
  }

  private async layoutsReadOpenMenuItems(): Promise<string[]> {
    // Row menus render the title in an h5 with an optional description
    // paragraph (Archive's gating note); plain-text menus (calendar day
    // add) have no h5, so those fall back to the full item text.
    const items = this.page.getByRole("menuitem");
    const count = await items.count();
    const texts: string[] = [];
    for (let i = 0; i < count; i++) {
      const heading = items.nth(i).locator("h5").first();
      const raw = (await heading.count()) > 0 ? await heading.innerText() : await items.nth(i).innerText();
      texts.push((raw ?? "").trim().replace(/\s+/g, " "));
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
    // Copying writes to the clipboard, which headless Chromium denies
    // without an explicit grant; arrange it before the pick.
    if (item === "Copy link") await this.page.context().grantPermissions(["clipboard-read", "clipboard-write"]);
    await this.layoutsOpenRowMenu(issueName);
    // Click by h5 title like layoutsOpenRowMenuItem: the accessible name
    // covers the whole item (title plus the Archive gating note), so an
    // exact name match cannot address a noted item.
    const entries = this.page.getByRole("menuitem");
    const count = await entries.count();
    let clicked = false;
    for (let i = 0; i < count; i++) {
      const heading = entries.nth(i).locator("h5").first();
      const title = (await heading.count()) > 0 ? await heading.innerText() : await entries.nth(i).innerText();
      if ((title ?? "").trim().replace(/\s+/g, " ") === item) {
        await entries.nth(i).click();
        clicked = true;
        break;
      }
    }
    if (!clicked) throw new Error(`[parity] row menu has no item "${item}".`);
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
    // The context menu is custom markup (buttons with h5 titles inside a
    // data-context-menu box), not menuitem roles; every row renders two
    // boxes (one per breakpoint slot) and both open at the same spot on a
    // right-click, so the read takes the first opaque one.
    const box = this.page.locator("div.opacity-100", { has: this.page.locator("div[data-context-menu]") }).first();
    await box.waitFor({ timeout: 15_000 });
    try {
      const headings = box.locator("div[data-context-menu] button h5");
      const count = await headings.count();
      const texts: string[] = [];
      for (let i = 0; i < count; i++) {
        texts.push(((await headings.nth(i).innerText()) ?? "").trim().replace(/\s+/g, " "));
      }
      return texts;
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

  async layoutsSheetQuickAdd(title: string): Promise<void> {
    // Two "Add work item" buttons render; the trailing (bottom-of-table)
    // one opens the title form, the leading one does nothing observable.
    const trigger = this.page.getByRole("button", { name: "Add work item", exact: true }).last();
    await trigger.scrollIntoViewIfNeeded({ timeout: WebDriver.LAYOUTS_BODY_WAIT_MS });
    await trigger.click({ timeout: WebDriver.LAYOUTS_FIRST_WAIT_MS });
    const field = this.page.getByPlaceholder("Work item title");
    await field.waitFor({ timeout: WebDriver.LAYOUTS_FIRST_WAIT_MS });
    await field.fill(title);
    await field.press("Enter");
    const deadline = Date.now() + WebDriver.LAYOUTS_BODY_WAIT_MS;
    for (;;) {
      if ((await this.layoutsSheetRowNames()).includes(title)) return;
      if (Date.now() >= deadline) throw new Error("[parity] sheet quick-add never rendered its row.");
      await this.page.waitForTimeout(500);
    }
  }

  private layoutsSheetToggleButtons(issueName: string): Locator {
    // The cell's buttons are indistinguishable by attributes: the
    // identifier is disabled, and the sub-issue chevron (leading, only
    // with children) and the hover trigger (trailing, always) are
    // enabled icon-only twins. Settled cells (menus closed) therefore
    // carry exactly one enabled button without children and two with,
    // the chevron first — which is what this locator narrows to.
    return this.layoutsSheetFirstCell(issueName).locator("button:not([disabled])").first();
  }

  async layoutsSheetHasSubIssueToggle(issueName: string): Promise<boolean> {
    const first = this.layoutsSheetFirstCell(issueName);
    await first.waitFor({ timeout: WebDriver.LAYOUTS_BODY_WAIT_MS });
    return (await first.locator("button:not([disabled])").count()) >= 2;
  }

  async layoutsSheetExpandSubIssues(issueName: string): Promise<void> {
    const table = this.layoutsSheetTable();
    await table.waitFor({ timeout: WebDriver.LAYOUTS_FIRST_WAIT_MS });
    const cells = table.locator('td[id^="issue-"]');
    const before = await cells.count();
    const first = this.layoutsSheetFirstCell(issueName);
    await first.waitFor({ timeout: WebDriver.LAYOUTS_BODY_WAIT_MS });
    await this.layoutsSheetToggleButtons(issueName).first().click({ timeout: WebDriver.LAYOUTS_FIRST_WAIT_MS });
    // Expansion fetches the children, so new rows arrive asynchronously.
    const deadline = Date.now() + WebDriver.LAYOUTS_BODY_WAIT_MS;
    for (;;) {
      if ((await cells.count()) > before) return;
      if (Date.now() >= deadline) throw new Error(`[parity] sub-issues of "${issueName}" never rendered in the sheet.`);
      await this.page.waitForTimeout(500);
    }
  }

  async layoutsSheetSubIssueNames(issueName: string): Promise<string[]> {
    // Children render as the rows directly after the parent; each level
    // indents with a wider spacer (inline width), so names are collected
    // while the spacer stays wider than the parent's own.
    const table = this.layoutsSheetTable();
    await table.waitFor({ timeout: WebDriver.LAYOUTS_FIRST_WAIT_MS });
    const cells = table.locator('tbody td[id^="issue-"]');
    const count = await cells.count();
    const names: string[] = [];
    const depths: number[] = [];
    for (let i = 0; i < count; i++) {
      const cell = cells.nth(i);
      names.push(await this.layoutsSheetFirstCellName(cell));
      const spacer = cell.locator('div[style*="width"]').first();
      const width =
        (await spacer.count()) > 0 ? await spacer.evaluate((node) => (node as HTMLElement).style.width) : "";
      depths.push(Number.parseFloat(width) || 0);
    }
    const parent = names.indexOf(issueName);
    if (parent < 0) throw new Error(`[parity] no sheet row "${issueName}".`);
    const parentDepth = depths[parent] ?? 0;
    const out: string[] = [];
    for (let i = parent + 1; i < count; i++) {
      if ((depths[i] ?? 0) <= parentDepth) break;
      const name = names[i] ?? "";
      if (name !== "") out.push(name);
    }
    return out;
  }

  async layoutsSheetOpenSubIssueCount(issueName: string): Promise<void> {
    // The sub-issues column cell carries a "N sub-work item(s)" label that
    // navigates to the issue's detail; the click landing is proven by the
    // URL changing underneath it.
    const first = this.layoutsSheetFirstCell(issueName);
    await first.waitFor({ timeout: WebDriver.LAYOUTS_BODY_WAIT_MS });
    const row = first.locator("xpath=ancestor::tr[1]");
    const label = row.getByText(/sub-work items?/).first();
    await label.waitFor({ timeout: WebDriver.LAYOUTS_BODY_WAIT_MS });
    const before = this.page.url();
    await label.click({ timeout: WebDriver.LAYOUTS_FIRST_WAIT_MS });
    const deadline = Date.now() + WebDriver.LAYOUTS_BODY_WAIT_MS;
    for (;;) {
      if (this.page.url() !== before) return;
      if (Date.now() >= deadline) throw new Error("[parity] sub-issue count never navigated.");
      await this.page.waitForTimeout(500);
    }
  }

  // --- Calendar layout (ISS-021..026). Day tiles are a static month
  // --- grid: positional reads are stable here (unlike list sections,
  // --- which reorder as groups fetch), and every read scopes to the
  // --- tile so the duplicated mobile blocks never leak in.
  private layoutsCalTiles(): Locator {
    return this.page.locator("div.group.relative.flex.h-full.w-full.flex-col");
  }

  private async layoutsCalTile(dayNumber: number): Promise<Locator> {
    // The desktop header shows the bare day number (today's sits in a
    // badge span); day-1 tiles carry a month prefix ("Oct 1"), and
    // adjacent-month filler tiles share numbers but render tertiary —
    // current-month tiles are font-medium. Parity specs only use days
    // strictly inside the month, so the exact+medium match is unique.
    const label = String(dayNumber);
    const tiles = this.layoutsCalTiles();
    const deadline = Date.now() + WebDriver.LAYOUTS_BODY_WAIT_MS;
    for (;;) {
      const count = await tiles.count();
      for (let i = 0; i < count; i++) {
        const header = tiles.nth(i).locator("div.hidden.flex-shrink-0.justify-end").first();
        if ((await header.count()) === 0) continue;
        // textContent, not innerText: the header hides below the md
        // breakpoint, where rendered text reads empty but the content
        // stays put. Desktop headers read identically either way.
        const text = ((await header.textContent()) ?? "").trim().replace(/\s+/g, " ");
        if (text !== label) continue;
        if (!((await header.getAttribute("class")) ?? "").includes("font-medium")) continue;
        return tiles.nth(i);
      }
      if (Date.now() >= deadline) throw new Error(`[parity] calendar tile for day ${label} not found.`);
      await this.page.waitForTimeout(500);
    }
  }

  private layoutsCalTitleButton(): Locator {
    return this.page.locator("button.text-18.font-semibold").first();
  }

  private layoutsCalStepButtons(): Locator {
    // Prev/next are the two buttons beside the title popover in the
    // header bar: the nearest gap-1.5 ancestor of the title button is
    // that bar's left group, whose button children are prev then next.
    return this.layoutsCalTitleButton().locator("xpath=ancestor::div[contains(@class, 'gap-1.5')][1]/button");
  }

  private layoutsCalWeekHeaderCells(): Locator {
    return this.page.locator("div.sticky.top-0").locator("div.flex.h-11");
  }

  async layoutsCalMode(): Promise<"month" | "week"> {
    // A month grid holds 28-35 tiles, a week row 5-7; the rows render
    // together once the calendar payload lands, so the count separates
    // the modes cleanly after the first tile appears.
    await this.layoutsCalTiles().first().waitFor({ timeout: WebDriver.LAYOUTS_BODY_WAIT_MS });
    return (await this.layoutsCalTiles().count()) > 10 ? "month" : "week";
  }

  async layoutsCalTitle(): Promise<string> {
    const title = this.layoutsCalTitleButton();
    await title.waitFor({ timeout: WebDriver.LAYOUTS_BODY_WAIT_MS });
    return ((await title.innerText()) ?? "").trim().replace(/\s+/g, " ");
  }

  async layoutsCalPrev(): Promise<void> {
    const before = await this.layoutsCalTitle();
    await this.layoutsCalStepButtons().nth(0).click();
    const deadline = Date.now() + WebDriver.LAYOUTS_BODY_WAIT_MS;
    for (;;) {
      if ((await this.layoutsCalTitle()) !== before) return;
      if (Date.now() >= deadline) throw new Error("[parity] calendar prev never changed the title.");
      await this.page.waitForTimeout(500);
    }
  }

  async layoutsCalNext(): Promise<void> {
    const before = await this.layoutsCalTitle();
    await this.layoutsCalStepButtons().nth(1).click();
    const deadline = Date.now() + WebDriver.LAYOUTS_BODY_WAIT_MS;
    for (;;) {
      if ((await this.layoutsCalTitle()) !== before) return;
      if (Date.now() >= deadline) throw new Error("[parity] calendar next never changed the title.");
      await this.page.waitForTimeout(500);
    }
  }

  async layoutsCalToday(): Promise<void> {
    await this.page.getByRole("button", { name: "Today", exact: true }).click();
  }

  private async layoutsCalOpenMonthPicker(): Promise<Locator> {
    await this.layoutsCalTitleButton().click();
    const panel = this.page.locator("div.w-56");
    await panel.waitFor({ timeout: 15_000 });
    return panel;
  }

  async layoutsCalMonthPickerMonths(): Promise<string[]> {
    const panel = await this.layoutsCalOpenMonthPicker();
    try {
      const buttons = panel.locator("div.grid.grid-cols-4 > button");
      const count = await buttons.count();
      const months: string[] = [];
      for (let i = 0; i < count; i++) months.push(((await buttons.nth(i).innerText()) ?? "").trim());
      return months;
    } finally {
      await this.page.keyboard.press("Escape");
    }
  }

  async layoutsCalMonthPickerYear(): Promise<number> {
    const panel = await this.layoutsCalOpenMonthPicker();
    try {
      const year = panel.locator("span.text-11").first();
      await year.waitFor({ timeout: 15_000 });
      return Number.parseInt(((await year.innerText()) ?? "").trim(), 10);
    } finally {
      await this.page.keyboard.press("Escape");
    }
  }

  async layoutsCalMonthPickerYearStep(direction: "prev" | "next"): Promise<void> {
    const panel = await this.layoutsCalOpenMonthPicker();
    try {
      const year = panel.locator("span.text-11").first();
      const before = ((await year.innerText()) ?? "").trim();
      const header = year.locator("xpath=..");
      const buttons = header.locator("button");
      if (direction === "prev") await buttons.first().click();
      else await buttons.last().click();
      const deadline = Date.now() + 15_000;
      for (;;) {
        if (((await year.innerText()) ?? "").trim() !== before) return;
        if (Date.now() >= deadline) throw new Error("[parity] month picker year never stepped.");
        await this.page.waitForTimeout(300);
      }
    } finally {
      await this.page.keyboard.press("Escape");
    }
  }

  async layoutsCalMonthPickerChoose(month: string): Promise<void> {
    const panel = await this.layoutsCalOpenMonthPicker();
    try {
      const before = await this.layoutsCalTitle();
      await panel.locator("div.grid.grid-cols-4 > button", { hasText: month }).first().click();
      const deadline = Date.now() + WebDriver.LAYOUTS_BODY_WAIT_MS;
      for (;;) {
        if ((await this.layoutsCalTitle()) !== before) return;
        if (Date.now() >= deadline) throw new Error(`[parity] month picker never applied "${month}".`);
        await this.page.waitForTimeout(500);
      }
    } finally {
      await this.page.keyboard.press("Escape");
    }
  }

  async layoutsCalMonthPickerEnabled(): Promise<boolean> {
    return await this.layoutsCalTitleButton().isEnabled();
  }

  private async layoutsCalOpenOptions(): Promise<Locator> {
    // Desktop names the trigger "Options"; on phones the same popover
    // trigger is an icon-only button right after Today. The panel is
    // content-selected (the sidebar's row-menu twins share its shape).
    // Both branches wait for the header first: cold calendar paints are
    // minutes late on the loaded host.
    const options = this.page.getByRole("button", { name: "Options" });
    const today = this.page.getByRole("button", { name: "Today" });
    const header = await this.layoutsWaitForCount(
      async () => (await options.count()) + (await today.count()),
      1,
      WebDriver.LAYOUTS_BODY_WAIT_MS
    );
    if (header === 0) throw new Error("[parity] calendar header never rendered.");
    if ((await options.count()) > 0) {
      await options.first().click({ timeout: 30_000 });
    } else {
      const trigger = today.first().locator("xpath=following-sibling::*[1]//button");
      await trigger.first().click({ timeout: 30_000 });
    }
    const panels = this.page.locator("div.min-w-\\[12rem\\]");
    const deadline = Date.now() + 15_000;
    for (;;) {
      const count = await panels.count();
      for (let i = 0; i < count; i++) {
        const text = (
          (await panels
            .nth(i)
            .innerText()
            .catch(() => "")) ?? ""
        ).replace(/\s+/g, " ");
        if (text.includes("Month layout")) return panels.nth(i);
      }
      if (Date.now() >= deadline) throw new Error("[parity] calendar options panel never opened.");
      await this.page.waitForTimeout(300);
    }
  }

  async layoutsCalSetMode(mode: "month" | "week"): Promise<void> {
    const panel = await this.layoutsCalOpenOptions();
    try {
      await panel
        .locator("button", { hasText: mode === "month" ? "Month layout" : "Week layout" })
        .first()
        .click();
    } finally {
      await this.page.keyboard.press("Escape");
    }
    const deadline = Date.now() + WebDriver.LAYOUTS_BODY_WAIT_MS;
    for (;;) {
      if ((await this.layoutsCalMode()) === mode) return;
      if (Date.now() >= deadline) throw new Error(`[parity] calendar never switched to ${mode} mode.`);
      await this.page.waitForTimeout(500);
    }
  }

  async layoutsCalWeekendsVisible(): Promise<boolean> {
    await this.layoutsCalWeekHeaderCells().first().waitFor({ timeout: WebDriver.LAYOUTS_BODY_WAIT_MS });
    const cells = this.layoutsCalWeekHeaderCells();
    const count = await cells.count();
    for (let i = 0; i < count; i++) {
      if (((await cells.nth(i).innerText()) ?? "").trim() === "Sat") return true;
    }
    return false;
  }

  async layoutsCalSetWeekends(show: boolean): Promise<void> {
    if ((await this.layoutsCalWeekendsVisible()) === show) return;
    const panel = await this.layoutsCalOpenOptions();
    try {
      await panel.locator("button", { hasText: "Show weekends" }).first().click();
    } finally {
      await this.page.keyboard.press("Escape");
    }
    const deadline = Date.now() + WebDriver.LAYOUTS_BODY_WAIT_MS;
    for (;;) {
      if ((await this.layoutsCalWeekendsVisible()) === show) return;
      if (Date.now() >= deadline) throw new Error("[parity] calendar weekends toggle never applied.");
      await this.page.waitForTimeout(500);
    }
  }

  async layoutsCalColumnCount(): Promise<number> {
    await this.layoutsCalWeekHeaderCells().first().waitFor({ timeout: WebDriver.LAYOUTS_BODY_WAIT_MS });
    return await this.layoutsCalWeekHeaderCells().count();
  }

  private layoutsCalBlockName(block: Locator): Locator {
    // The identifier chip carries no truncate class, so the truncate
    // div inside a block is exactly the issue name.
    return block.locator("div.truncate.text-13").first();
  }

  async layoutsCalDayIssueNames(dayNumber: number): Promise<string[]> {
    const tile = await this.layoutsCalTile(dayNumber);
    const blocks = tile.locator('a[id^="issue-"]');
    const count = await blocks.count();
    const names: string[] = [];
    for (let i = 0; i < count; i++) {
      names.push(((await this.layoutsCalBlockName(blocks.nth(i)).innerText()) ?? "").trim());
    }
    return names;
  }

  async layoutsCalDayIsToday(dayNumber: number): Promise<boolean> {
    const tile = await this.layoutsCalTile(dayNumber);
    const header = tile.locator("div.hidden.flex-shrink-0.justify-end").first();
    return (await header.locator("span.rounded-full").count()) > 0;
  }

  async layoutsCalDayHasLoadMore(dayNumber: number): Promise<boolean> {
    const tile = await this.layoutsCalTile(dayNumber);
    return (await tile.getByRole("button", { name: "Load more", exact: true }).count()) > 0;
  }

  async layoutsCalDayLoadMore(dayNumber: number): Promise<void> {
    const tile = await this.layoutsCalTile(dayNumber);
    await tile.getByRole("button", { name: "Load more", exact: true }).click();
  }

  async layoutsCalDragBlock(issueName: string, toDayNumber: number): Promise<void> {
    // Pragmatic drag-and-drop listens to pointer events, so a stepped
    // mouse path (not dragTo) drives the drop target the app registers
    // on the destination tile. The press lands on the block's first
    // button, not its center: the block is a link, and pressing its
    // text starts a native link-drag that hijacks the pointer flow
    // (dragstart/dragend, no drop), while a button press stays a pure
    // pointer gesture the drop target answers.
    const block = this.layoutsIssueRow(issueName);
    await block.waitFor({ timeout: WebDriver.LAYOUTS_BODY_WAIT_MS });
    await block.scrollIntoViewIfNeeded({ timeout: WebDriver.LAYOUTS_BODY_WAIT_MS });
    const tile = await this.layoutsCalTile(toDayNumber);
    await tile.scrollIntoViewIfNeeded({ timeout: WebDriver.LAYOUTS_BODY_WAIT_MS });
    const press = block.locator("button").first();
    const from = (await press.boundingBox()) ?? (await block.boundingBox());
    const to = await tile.boundingBox();
    if (!from || !to) throw new Error("[parity] calendar drag endpoints have no bounding box.");
    const mouse = this.page.mouse;
    // An open hover preview blinds the drop lookup: while any preview
    // card is mounted the app shell carries pointer-events none, so the
    // tile under the pointer is unhittable and the drop silently no-drops
    // (proven by bisection: the flip tracks the card, not the drag, and
    // clears when the card closes). Parking closes any open card; the
    // poll below makes that a precondition, and the press-travel runs
    // with no stops so the pointer leaves the block inside the card's
    // 100ms hover delay and the card never reopens mid-drag.
    await mouse.move(8, 8);
    const popGoneBy = Date.now() + 10_000;
    for (;;) {
      const pops = await this.page.locator("div.w-72").count();
      if (pops === 0) break;
      if (Date.now() >= popGoneBy) throw new Error("[parity] calendar hover preview never closed before drag.");
      await this.page.waitForTimeout(250);
    }
    const fromX = from.x + from.width / 2;
    const fromY = from.y + from.height / 2;
    await mouse.move(fromX, fromY);
    await mouse.down();
    // The exit is a single step: a stepped exit hovers the block's
    // midpoint, dwelling past the 100ms hover delay and reopening the
    // card mid-drag. Three travel steps still drive the drop target.
    await mouse.move(fromX, fromY - 30, { steps: 1 });
    await mouse.move(to.x + to.width / 2, to.y + to.height / 2, { steps: 3 });
    await mouse.up();
  }

  async layoutsCalTileDrag(fromDayNumber: number, toDayNumber: number): Promise<void> {
    // Mobile day tiles render no block anchors (dots only), so there is
    // no draggable source: pressing the tile face and traveling to
    // another tile must move nothing. The gesture mirrors the desktop
    // drag's stepped travel so a future draggable would answer it.
    const from = await this.layoutsCalTile(fromDayNumber);
    await from.scrollIntoViewIfNeeded({ timeout: WebDriver.LAYOUTS_BODY_WAIT_MS });
    const to = await this.layoutsCalTile(toDayNumber);
    await to.scrollIntoViewIfNeeded({ timeout: WebDriver.LAYOUTS_BODY_WAIT_MS });
    const fromBox = await from.boundingBox();
    const toBox = await to.boundingBox();
    if (!fromBox || !toBox) throw new Error("[parity] calendar tile drag endpoints have no bounding box.");
    const mouse = this.page.mouse;
    const fromX = fromBox.x + fromBox.width / 2;
    const fromY = fromBox.y + fromBox.height / 2;
    await mouse.move(fromX, fromY);
    await mouse.down();
    await mouse.move(fromX, fromY - 30, { steps: 1 });
    await mouse.move(toBox.x + toBox.width / 2, toBox.y + toBox.height / 2, { steps: 3 });
    await mouse.up();
  }

  async layoutsCalBlockText(issueName: string): Promise<string> {
    const block = this.layoutsIssueRow(issueName);
    await block.waitFor({ timeout: WebDriver.LAYOUTS_BODY_WAIT_MS });
    return ((await block.innerText()) ?? "").trim().replace(/\s+/g, " ");
  }

  async layoutsCalBlockHoverPreview(issueName: string): Promise<boolean> {
    const block = this.layoutsIssueRow(issueName);
    await block.waitFor({ timeout: WebDriver.LAYOUTS_BODY_WAIT_MS });
    await block.hover();
    const card = this.page.locator("div.w-72.space-y-2");
    try {
      await card.waitFor({ timeout: 15_000 });
      return true;
    } catch {
      return false;
    }
  }

  async layoutsCalBlockOpenPeek(issueName: string): Promise<void> {
    // A block is a peek-link anchor with no paragraph (the list row's
    // opener clicks a <p>), so the block itself is the click target;
    // the peek wait below mirrors the row opener's.
    const block = this.layoutsIssueRow(issueName);
    await block.waitFor({ timeout: WebDriver.LAYOUTS_BODY_WAIT_MS });
    await block.click({ timeout: WebDriver.LAYOUTS_FIRST_WAIT_MS });
    await this.page.waitForURL((url) => url.href.includes("peekIssueId"), {
      timeout: WebDriver.LAYOUTS_BODY_WAIT_MS,
    });
    await this.layoutsPeekPanel().waitFor({ timeout: WebDriver.LAYOUTS_BODY_WAIT_MS });
  }

  async layoutsCalBlockQuickActions(issueName: string): Promise<string[]> {
    const block = this.layoutsIssueRow(issueName);
    await block.waitFor({ timeout: WebDriver.LAYOUTS_BODY_WAIT_MS });
    await block.hover();
    await block.locator("div.cursor-pointer").first().click();
    await this.page.getByRole("menuitem").first().waitFor({ timeout: 15_000 });
    try {
      return await this.layoutsReadOpenMenuItems();
    } finally {
      await this.page.keyboard.press("Escape");
    }
  }

  private async layoutsCalOpenDayAddMenu(dayNumber: number): Promise<void> {
    const tile = await this.layoutsCalTile(dayNumber);
    await tile.hover();
    // The tile-level add control is hover-revealed (opacity-0 until the
    // tile hovers), so the tile hover above precedes the click.
    await tile.locator("div.flex.w-full.items-center", { hasText: "Add work item" }).first().click();
    await this.page.getByRole("menuitem").first().waitFor({ timeout: 15_000 });
  }

  async layoutsCalDayQuickAdd(dayNumber: number, title: string): Promise<void> {
    await this.layoutsCalOpenDayAddMenu(dayNumber);
    await this.page.getByRole("menuitem", { name: "Add work item", exact: true }).first().click();
    const field = this.page.getByPlaceholder("Work item Title");
    await field.waitFor({ timeout: 15_000 });
    await field.fill(title);
    await field.press("Enter");
    await this.page.keyboard.press("Escape");
  }

  async layoutsCalDayAddMenu(dayNumber: number): Promise<string[]> {
    await this.layoutsCalOpenDayAddMenu(dayNumber);
    try {
      return await this.layoutsReadOpenMenuItems();
    } finally {
      await this.page.keyboard.press("Escape");
    }
  }

  async layoutsCalTapDay(dayNumber: number): Promise<void> {
    // The mobile tile face (hidden on desktop) selects the day whose
    // blocks the mobile detail list below the grid renders.
    const tile = await this.layoutsCalTile(dayNumber);
    await tile.locator("div.mx-auto.cursor-pointer").first().click();
  }

  async layoutsCalDayDetailNames(): Promise<string[]> {
    // Day-detail rows are generic pointer rows, not anchors: a disabled
    // identifier chip in a wrapper div, with the name in the wrapper's
    // next sibling (a truncate div). The list renders multiplied, so the
    // read dedupes; other disabled buttons (context-menu Archive twins)
    // fail the chip pattern. Calendar layout renders no other
    // identifier chips, so the page scope is exact.
    const chips = this.page.locator("button[disabled]");
    const count = await chips.count();
    const names: string[] = [];
    for (let i = 0; i < count; i++) {
      const chipText = (
        (await chips
          .nth(i)
          .textContent()
          .catch(() => "")) ?? ""
      ).trim();
      if (!/^[A-Z]+-\d+$/.test(chipText)) continue;
      const nameEl = chips.nth(i).locator("xpath=../following-sibling::div[contains(@class, 'truncate')][1]");
      if ((await nameEl.count()) === 0) continue;
      names.push(
        (
          ((await nameEl
            .first()
            .textContent()
            .catch(() => "")) ?? "") as string
        ).trim()
      );
    }
    return [...new Set(names)];
  }

  private async layoutsOpenRowMenuItem(issueName: string, item: string): Promise<Locator> {
    // Items are located by their h5 title: the accessible name covers
    // the whole item (title plus the Archive gating note), so an exact
    // name match cannot address a noted item.
    await this.layoutsOpenRowMenu(issueName);
    const entries = this.page.getByRole("menuitem");
    const count = await entries.count();
    for (let i = 0; i < count; i++) {
      const heading = entries.nth(i).locator("h5").first();
      if ((await heading.count()) > 0 && ((await heading.innerText()) ?? "").trim() === item) {
        return entries.nth(i);
      }
    }
    throw new Error(`[parity] row menu has no item "${item}".`);
  }

  async layoutsRowMenuItemDisabled(issueName: string, item: string): Promise<boolean> {
    const entry = await this.layoutsOpenRowMenuItem(issueName, item);
    try {
      if (((await entry.getAttribute("aria-disabled")) ?? "") === "true") return true;
      return ((await entry.getAttribute("class")) ?? "").includes("text-placeholder");
    } finally {
      await this.page.keyboard.press("Escape");
    }
  }

  async layoutsRowMenuItemNote(issueName: string, item: string): Promise<string | null> {
    const entry = await this.layoutsOpenRowMenuItem(issueName, item);
    try {
      const note = entry.locator("p").first();
      if ((await note.count()) === 0) return null;
      return ((await note.innerText()) ?? "").trim().replace(/\s+/g, " ");
    } finally {
      await this.page.keyboard.press("Escape");
    }
  }

  private layoutsWorkItemModalTitleHeading(): Locator {
    return this.page.getByRole("dialog").locator("h3.text-h4-medium").first();
  }

  async layoutsWorkItemModalVisible(): Promise<boolean> {
    return (await this.layoutsWorkItemModalTitleHeading().count()) > 0;
  }

  async layoutsWorkItemModalTitle(): Promise<string | null> {
    const heading = this.layoutsWorkItemModalTitleHeading();
    if ((await heading.count()) === 0) return null;
    return ((await heading.innerText()) ?? "").trim();
  }

  async layoutsWorkItemModalClose(): Promise<void> {
    // The modal has no dedicated close control; Escape dismisses it via
    // the dialog shell. Parity specs never dirty the form first, so no
    // draft prompt intervenes.
    await this.page.keyboard.press("Escape");
    const deadline = Date.now() + WebDriver.LAYOUTS_BODY_WAIT_MS;
    for (;;) {
      if (!(await this.layoutsWorkItemModalVisible())) return;
      if (Date.now() >= deadline) throw new Error("[parity] work-item modal never closed.");
      await this.page.waitForTimeout(500);
    }
  }

  async layoutsWorkItemModalSetTitle(title: string): Promise<void> {
    // The title field carries its placeholder; the modal shows exactly
    // one text field, so the first match is unambiguous.
    const field = this.page.getByRole("dialog").getByPlaceholder("Title").first();
    await field.waitFor({ timeout: WebDriver.LAYOUTS_BODY_WAIT_MS });
    await field.fill(title);
  }

  async layoutsWorkItemModalSubmit(): Promise<void> {
    // The primary button reads Save on create and Update on edit.
    const dialog = this.page.getByRole("dialog");
    const save = dialog.getByRole("button", { name: "Save", exact: true });
    const update = dialog.getByRole("button", { name: "Update", exact: true });
    if ((await save.count()) > 0) await save.first().click();
    else if ((await update.count()) > 0) await update.first().click();
    else throw new Error("[parity] work-item modal has no Save/Update button.");
    const deadline = Date.now() + WebDriver.LAYOUTS_BODY_WAIT_MS;
    for (;;) {
      if (!(await this.layoutsWorkItemModalVisible())) return;
      if (Date.now() >= deadline) throw new Error("[parity] work-item modal never closed after submit.");
      await this.page.waitForTimeout(500);
    }
  }

  async layoutsDeleteModalVisible(): Promise<boolean> {
    return (await this.page.getByRole("heading", { name: "Delete Work item", exact: true }).count()) > 0;
  }

  async layoutsDeleteModalConfirm(): Promise<void> {
    // Scoped to the dialog: every row's closed context box carries its own
    // Delete button, so a page-wide match is ambiguous.
    await this.page.getByRole("dialog").getByRole("button", { name: "Delete", exact: true }).click();
    const deadline = Date.now() + WebDriver.LAYOUTS_BODY_WAIT_MS;
    for (;;) {
      if (!(await this.layoutsDeleteModalVisible())) return;
      if (Date.now() >= deadline) throw new Error("[parity] delete modal never closed after confirm.");
      await this.page.waitForTimeout(500);
    }
  }

  async layoutsArchiveModalVisible(): Promise<boolean> {
    // The heading carries the identifier suffix ("Archive Work item PAR
    // 12"), so the marker is a heading containing the fixed prefix.
    return (await this.page.locator("h3", { hasText: "Archive Work item" }).count()) > 0;
  }

  async layoutsArchiveModalConfirm(): Promise<void> {
    // Scoped to the dialog: closed context boxes carry their own Archive
    // buttons, so a page-wide match is ambiguous.
    await this.page.getByRole("dialog").getByRole("button", { name: "Archive", exact: true }).click();
    const deadline = Date.now() + WebDriver.LAYOUTS_BODY_WAIT_MS;
    for (;;) {
      if (!(await this.layoutsArchiveModalVisible())) return;
      if (Date.now() >= deadline) throw new Error("[parity] archive modal never closed after confirm.");
      await this.page.waitForTimeout(500);
    }
  }

  async layoutsMoveModalVisible(): Promise<boolean> {
    // Scoped to the dialog: row menus carry a same-titled h5, and the
    // post-move detail page renders headings that would read as open.
    return (
      (await this.page.getByRole("dialog").getByRole("heading", { name: "Move to project", exact: true }).count()) > 0
    );
  }

  async layoutsMoveModalChoose(projectName: string): Promise<void> {
    // Project buttons move immediately on click and the view navigates
    // to the moved issue, so the wait is for the modal to disappear.
    const dialog = this.page.getByRole("dialog");
    await dialog.locator("button", { hasText: projectName }).first().click();
    const deadline = Date.now() + WebDriver.LAYOUTS_BODY_WAIT_MS;
    for (;;) {
      if (!(await this.layoutsMoveModalVisible())) return;
      if (Date.now() >= deadline) throw new Error(`[parity] move modal never applied "${projectName}".`);
      await this.page.waitForTimeout(500);
    }
  }

  async layoutsAddExistingModalVisible(): Promise<boolean> {
    // The submit button ("Add selected work items") exists only while
    // the modal is open; the search input placeholder is shared with
    // other pickers, so the submit button is the marker.
    return (await this.page.getByRole("button", { name: "Add selected work items", exact: true }).count()) > 0;
  }

  async layoutsAddExistingModalChoose(issueName: string): Promise<void> {
    // Each option is a label (htmlFor issue-<id>) whose truncate span
    // carries the name; clicking toggles selection, then submit dates
    // every selected issue onto the tile and closes the modal.
    const option = this.page.locator('label[for^="issue-"]', { hasText: issueName }).first();
    await option.waitFor({ timeout: WebDriver.LAYOUTS_BODY_WAIT_MS });
    await option.click();
    // The label wraps the checkbox (its `for` dangles), so the click
    // selects — but React commits asynchronously, and submitting first
    // dates nothing and just closes the modal. Wait for the check.
    const box = option.locator('input[type="checkbox"]').first();
    const checkedBy = Date.now() + 30_000;
    for (;;) {
      if (await box.isChecked().catch(() => false)) break;
      if (Date.now() >= checkedBy) throw new Error(`[parity] add-existing option never checked "${issueName}".`);
      await this.page.waitForTimeout(250);
    }
    await this.page.getByRole("button", { name: "Add selected work items", exact: true }).click();
    const deadline = Date.now() + WebDriver.LAYOUTS_BODY_WAIT_MS;
    for (;;) {
      if (!(await this.layoutsAddExistingModalVisible())) return;
      if (Date.now() >= deadline) throw new Error("[parity] add-existing modal never closed after submit.");
      await this.page.waitForTimeout(500);
    }
  }

  private async layoutsOpenDetailMenu(): Promise<void> {
    // The detail trigger wraps its ellipsis IconButton in the menu
    // button, so it is the button containing a button; row triggers
    // wrap a div instead and never match. Both the browse header and
    // the peek header pack other nested buttons around it (help,
    // breadcrumb, collapse, emoji), so the search additionally
    // requires the CustomMenu wrapper (which stamps data-main-menu).
    // Comment cards carry their own nested-button ellipses, and on a
    // short page one can sit inside the header band right of the
    // detail trigger — so banded candidates are tried right-to-left
    // and the one whose menu carries the detail marker ("Make a
    // copy", never present on comment menus) wins.
    const panel = this.page.locator("div.absolute.top-0.right-0.bottom-0").first();
    const scope = (await panel.count()) > 0 ? panel : this.page;
    // The header renders after navigation, so poll for banded
    // triggers rather than scanning the empty page once.
    const all = scope.locator("xpath=.//button[.//button and ancestor::div[@data-main-menu='true']]");
    const deadline = Date.now() + WebDriver.LAYOUTS_FIRST_WAIT_MS;
    let order: number[] = [];
    for (;;) {
      const count = await all.count();
      const boxed: { index: number; x: number }[] = [];
      for (let i = 0; i < count; i++) {
        const box = await all
          .nth(i)
          .boundingBox()
          .catch(() => null);
        if (box && box.width > 0 && box.height > 0 && box.y > 30 && box.y < 120) {
          boxed.push({ index: i, x: box.x });
        }
      }
      if (boxed.length > 0) {
        order = boxed.sort((a, b) => b.x - a.x).map((b) => b.index);
        break;
      }
      if (Date.now() >= deadline) throw new Error("[parity] detail header menu trigger not found.");
      await this.page.waitForTimeout(500);
    }
    // Center each candidate before clicking: at the viewport edge a
    // scrolled-under comment card can cover the click point. When a
    // stable overlay still wins the hit test, a forced dispatch opens
    // the menu the click cannot reach.
    for (const index of order) {
      const found = all.nth(index);
      await found.evaluate((el) => el.scrollIntoView({ block: "center", inline: "center" })).catch(() => {});
      await found.click({ timeout: 10_000 }).catch(async () => found.click({ force: true }));
      const opened = await this.page
        .getByRole("menuitem")
        .first()
        .waitFor({ timeout: 15_000 })
        .then(() => true)
        .catch(() => false);
      if (!opened) {
        await this.page.keyboard.press("Escape");
        continue;
      }
      const items = await this.layoutsReadOpenMenuItems();
      if (items.includes("Make a copy")) return;
      await this.page.keyboard.press("Escape");
    }
    throw new Error("[parity] detail header menu trigger not found.");
  }

  async layoutsDetailMenuItems(): Promise<string[]> {
    await this.layoutsOpenDetailMenu();
    try {
      return await this.layoutsReadOpenMenuItems();
    } finally {
      await this.page.keyboard.press("Escape");
    }
  }

  async layoutsDetailMenuChoose(_item: string): Promise<void> {
    return this.layoutsTodo("layoutsDetailMenuChoose");
  }

  async layoutsPeekCopyLinkVisible(): Promise<boolean> {
    // Scoped to the peek panel (absent on the browse page): inside the
    // header action group the copy control is the text-less plain
    // button — the menu trigger nests a button, the subscribe control
    // carries text, and both live outside the menu container.
    const panel = this.page.locator("div.absolute.top-0.right-0.bottom-0").first();
    if ((await panel.count()) === 0) return false;
    const trigger = panel.locator("button:has(button)").first();
    if ((await trigger.count()) === 0) return false;
    const group = trigger.locator("xpath=ancestor::div[contains(@class, 'gap-2')][1]");
    const copy = group.locator(
      "xpath=.//button[not(.//button) and not(ancestor::div[@data-main-menu='true']) and normalize-space(string(.))='']"
    );
    return (await copy.count()) > 0;
  }

  private layoutsListPageMenuTrigger(): Locator {
    // The whole-list ellipsis pins its own size in its classes (the
    // project page carries no such button; the entry point is a view
    // page, where exactly one renders).
    return this.page.locator("button.size-\\[26px\\]").first();
  }

  async layoutsListPageMenuItems(): Promise<string[]> {
    const trigger = this.layoutsListPageMenuTrigger();
    await trigger.waitFor({ timeout: WebDriver.LAYOUTS_FIRST_WAIT_MS });
    await trigger.click();
    await this.page.getByRole("menuitem").first().waitFor({ timeout: 15_000 });
    try {
      return await this.layoutsReadOpenMenuItems();
    } finally {
      await this.page.keyboard.press("Escape");
    }
  }

  private async layoutsGroupHeaderAddControl(groupTitle: string): Promise<{ menu: boolean; control: Locator }> {
    // Cycle/module headers wrap the plus in a menu button (span); plain
    // project headers render the plus as a bare div that opens the
    // create modal directly.
    const section = await this.layoutsGroupSection(groupTitle);
    // The rail loads with the body, so dismiss it only once the section
    // has resolved — any earlier the dismiss is a no-op on an empty page.
    await this.layoutsDismissDetailRail();
    const header = section.locator('div[class*="group/list-header"]').first();
    const menuPlus = header.locator("span.h-5.w-5").first();
    if ((await menuPlus.count()) > 0) return { menu: true, control: menuPlus };
    return { menu: false, control: header.locator("div.h-5.w-5").first() };
  }

  async layoutsGroupHeaderAddMenu(groupTitle: string): Promise<string[] | null> {
    const { menu, control } = await this.layoutsGroupHeaderAddControl(groupTitle);
    if (!menu) return null;
    // Bounded like the row trigger: when the detail rail still covers
    // the plus, a forced dispatch opens the menu the click cannot reach.
    await control.click({ timeout: 10_000 }).catch(async () => control.click({ force: true }));
    await this.page.getByRole("menuitem").first().waitFor({ timeout: 15_000 });
    try {
      return await this.layoutsReadOpenMenuItems();
    } finally {
      await this.page.keyboard.press("Escape");
    }
  }

  private layoutsEmptyScope(): Locator {
    // Every empty state renders through the detailed empty-state card,
    // whose copy column pins its width; scoping to it keeps list chrome
    // (group headers, switcher) out of title/action reads. The read is
    // visible-only: some pages (all-issues, profile) mount a hidden
    // "No matching results." twin ahead of the real card in DOM order.
    return this.page.locator("div.max-w-\\[25rem\\]:visible").first();
  }

  async layoutsEmptyTitle(): Promise<string | null> {
    const scope = this.layoutsEmptyScope();
    if ((await scope.count()) === 0) return null;
    const heading = scope.locator("h3").first();
    if ((await heading.count()) === 0) return null;
    return ((await heading.innerText()) ?? "").trim();
  }

  async layoutsEmptyActions(): Promise<Array<{ label: string; disabled: boolean }>> {
    const scope = this.layoutsEmptyScope();
    if ((await scope.count()) === 0) return [];
    const buttons = scope.locator("button");
    const count = await buttons.count();
    const actions: Array<{ label: string; disabled: boolean }> = [];
    for (let i = 0; i < count; i++) {
      actions.push({
        label: ((await buttons.nth(i).innerText()) ?? "").trim().replace(/\s+/g, " "),
        disabled: await buttons.nth(i).isDisabled(),
      });
    }
    return actions;
  }

  async layoutsEmptyChoose(label: string): Promise<void> {
    await this.layoutsEmptyScope().locator("button", { hasText: label }).first().click();
  }

  private layoutsFilterPills(): Locator {
    return this.page.getByRole("button", { name: "Remove filter" });
  }

  /** Deadline loop for counts, which locators cannot waitFor directly. */
  private async layoutsWaitForCount(read: () => Promise<number>, atLeast: number, timeoutMs: number): Promise<number> {
    const deadline = Date.now() + timeoutMs;
    let count = 0;
    for (;;) {
      count = await read();
      if (count >= atLeast || Date.now() > deadline) return count;
      await this.page.waitForTimeout(1000);
    }
  }

  private async layoutsFilterPickMenuItem(label: string): Promise<void> {
    // Property and value menus render as menuitem or option roles
    // depending on the surface; wait for the ENTRY itself, never the
    // menu container — the open listbox container is Playwright-hidden
    // (zero-height, options overflowing visible, same as the due-date
    // picker) while its options are fully visible, so a container wait
    // never resolves even with the menu open on screen. The hidden
    // headlessui twin carries no options, so a page-scoped entry read
    // cannot match it.
    const item = this.page.getByRole("menuitem", { name: label }).first();
    const option = this.page.getByRole("option", { name: label }).first();
    const found = await this.layoutsWaitForCount(
      async () => (await item.count()) + (await option.count()),
      1,
      WebDriver.LAYOUTS_BODY_WAIT_MS
    );
    if (found === 0) throw new Error(`[parity] no filter menu entry ${label}.`);
    if ((await item.count()) > 0) await item.click({ timeout: 60_000 });
    else await option.click({ timeout: 60_000 });
  }

  async layoutsFilterAddConditionViaRow(propertyLabel: string, valueLabel: string): Promise<void> {
    // The row must be visible (seeded pill present): its add control sits
    // immediately right of the last pill, in the same band.
    const before = await this.layoutsWaitForCount(
      async () => this.layoutsFilterPills().count(),
      1,
      WebDriver.LAYOUTS_BODY_WAIT_MS
    );
    if (before === 0) throw new Error("[parity] no filter pill: seed one before adding a condition.");
    const pillBox = await this.layoutsFilterPills().first().boundingBox();
    if (!pillBox) throw new Error("[parity] filter pill has no box.");
    const buttons = this.page.locator("main button");
    const total = await buttons.count();
    let plus = -1;
    let bestX = Infinity;
    for (let i = 0; i < total; i++) {
      const box = await buttons
        .nth(i)
        .boundingBox()
        .catch(() => null);
      if (!box) continue;
      if (Math.abs(box.y - pillBox.y) > 25) continue;
      if (box.x <= pillBox.x + pillBox.width - 4) continue;
      const name = (
        (await buttons.nth(i).getAttribute("aria-label")) ??
        (await buttons
          .nth(i)
          .innerText()
          .catch(() => "")) ??
        ""
      ).trim();
      if (name) continue;
      if (box.x < bestX) {
        bestX = box.x;
        plus = i;
      }
    }
    if (plus < 0) throw new Error("[parity] filter row add control not found.");
    await buttons.nth(plus).click({ timeout: 30_000 });
    await this.layoutsFilterPickMenuItem(propertyLabel);
    await this.page.waitForTimeout(2000);
    await this.layoutsFilterPickMenuItem(valueLabel);
    // Settle proof: the new pill renders (past a server round-trip, so
    // this gets the same contention-grade budget as the menu render).
    const after = await this.layoutsWaitForCount(async () => this.layoutsFilterPills().count(), before + 1, 120_000);
    if (after <= before) throw new Error("[parity] filter pill count did not grow.");
  }

  async layoutsSeedArchivedLocalFilter(workspaceSlug: string, projectId: string, expression: unknown): Promise<void> {
    // The archived page loads its filter instance from window-local
    // storage only, and reads the camel-case richFilters member — the
    // shape below mirrors what the read path expects so the row renders
    // with a live pill, exactly as if persisted filters had loaded.
    await this.page.evaluate(
      ({ slug, pid, rich }) => {
        window.localStorage.setItem(
          "issue_local_filters",
          JSON.stringify([{ key: "ARCHIVED", workspaceSlug: slug, viewId: pid, filters: { richFilters: rich } }])
        );
      },
      { slug: workspaceSlug, pid: projectId, rich: expression }
    );
    // layoutsReloadIssues waits for the layout switcher, which the
    // archived page has none of — reload bare and poll the pill instead.
    await this.page.reload({ waitUntil: "domcontentloaded" });
    const pills = await this.layoutsWaitForCount(
      async () => this.layoutsFilterPills().count(),
      1,
      WebDriver.LAYOUTS_BODY_WAIT_MS
    );
    if (pills === 0) throw new Error("[parity] archived pill never rendered after local seed.");
  }

  async layoutsProfileActivityVisible(): Promise<boolean> {
    // Immediate read; specs poll it. The activity tab renders chrome
    // (Recent activity) with an empty list and no empty-state card.
    return (await this.page.getByRole("heading", { name: "Recent activity" }).count()) > 0;
  }

  /** The compact header bar, resolved through its Analytics button. */
  private layoutsMobileBar(): Locator {
    return this.page.getByRole("button", { name: "Analytics", exact: true }).locator("xpath=..");
  }

  private layoutsMobileLayoutTrigger(): Locator {
    // The bar holds the layout menu, the Display dropdown, then
    // Analytics: its first button opens the layout menu.
    return this.layoutsMobileBar().locator("button").first();
  }

  private static readonly LAYOUTS_MOBILE_LABELS: Record<LayoutsLayoutKey, string> = {
    list: "List",
    kanban: "Board",
    calendar: "Calendar",
    spreadsheet: "Table",
    gantt_chart: "Timeline",
  };

  async layoutsMobileOfferedLayouts(): Promise<LayoutsLayoutKey[]> {
    await this.layoutsMobileLayoutTrigger().click();
    await this.page.getByRole("menuitem").first().waitFor({ timeout: 15_000 });
    const items = this.page.getByRole("menuitem");
    const count = await items.count();
    const labels: string[] = [];
    for (let i = 0; i < count; i++) {
      labels.push(((await items.nth(i).innerText()) ?? "").trim().replace(/\s+/g, " "));
    }
    await this.page.keyboard.press("Escape");
    const entries = Object.entries(WebDriver.LAYOUTS_MOBILE_LABELS) as Array<[LayoutsLayoutKey, string]>;
    return labels.map((label) => {
      const found = entries.find(([, text]) => text === label);
      if (!found) throw new Error(`[parity] unknown mobile layout label "${label}".`);
      return found[0];
    });
  }

  async layoutsMobileDisplayVisible(): Promise<boolean> {
    const trigger = this.page.getByRole("button", { name: "Display", exact: true });
    return (await trigger.count()) > 0 && (await trigger.first().isVisible());
  }

  async layoutsMobileAnalyticsVisible(): Promise<boolean> {
    const trigger = this.page.getByRole("button", { name: "Analytics", exact: true });
    return (await trigger.count()) > 0 && (await trigger.first().isVisible());
  }

  async layoutsSkeletonVisible(): Promise<boolean> {
    // Skeleton rows render as dozens of pulsing placeholder spans (the
    // full loader paints three sections; group pagination paints rows).
    // A lone pulsing overlay is the optimistic temp row instead, so the
    // read thresholds well above one.
    return (await this.page.locator('[class*="animate-pulse"]').count()) >= 5;
  }

  private async layoutsVisibleBox(
    element: Locator,
    timeoutMs: number
  ): Promise<{
    x: number;
    y: number;
    width: number;
    height: number;
  } | null> {
    // boundingBox() has no timeout knob and hangs while the loaded
    // renderer is wedged; race it so polled reads resolve instead of
    // eating the caller's whole budget in one hung call.
    return await Promise.race([
      element.boundingBox().catch(() => null),
      new Promise<null>((resolve) => setTimeout(() => resolve(null), timeoutMs)),
    ]);
  }

  async layoutsMutationSpinnerVisible(): Promise<boolean> {
    // While the list loader reads "mutation" (a filter/display refetch
    // in flight) the app floats a small fixed square docked top-right
    // holding a status spinner. Role plus geometry only — no copied
    // classes: toasts dock bottom-right and context twins carry no
    // status role, so the quadrant-plus-size gate is exact.
    const boxes = this.page.locator("div.fixed", { has: this.page.locator('[role="status"]') });
    const viewport = this.page.viewportSize() ?? { width: 1280, height: 800 };
    const count = await boxes.count();
    for (let i = 0; i < count; i++) {
      const box = await this.layoutsVisibleBox(boxes.nth(i), 10_000);
      if (!box) continue;
      if (box.width < 30 || box.width > 64 || box.height < 30 || box.height > 64) continue;
      if (box.x < viewport.width / 2 || box.y > viewport.height / 2) continue;
      return true;
    }
    return false;
  }

  async layoutsRowHighlighted(issueName: string): Promise<boolean> {
    // Drops add the highlight class to the block anchor (id issue-<id>)
    // ~200ms after release; poll briefly since callers read right after
    // the drop lands.
    const block = this.layoutsIssueRow(issueName);
    const deadline = Date.now() + 15_000;
    for (;;) {
      if ((await block.count()) > 0 && ((await block.first().getAttribute("class")) ?? "").includes("highlight"))
        return true;
      if (Date.now() >= deadline) return false;
      await this.page.waitForTimeout(300);
    }
  }

  async layoutsTempRowVisible(): Promise<boolean> {
    // An unconfirmed quick-add paints its row link with a pulsing
    // overlay until the server confirms; skeleton placeholders are bare
    // divs, never row links, so scoping to links disambiguates.
    const rows = this.page.locator('a[id^="issue-"]', { has: this.page.locator('[class*="animate-pulse"]') });
    const count = await rows.count();
    for (let i = 0; i < count; i++) {
      if (
        await rows
          .nth(i)
          .isVisible()
          .catch(() => false)
      )
        return true;
    }
    return false;
  }

  async layoutsSheetCellSetDueDate(issueName: string, isoDate: string): Promise<void> {
    // The due-date cell opens a month calendar in a "Due date" listbox:
    // month/year comboboxes (native selects) plus a day grid. Day
    // buttons are named "Weekday, Month D<suffix>, Year" ("Today, ..."
    // when the day is today); the grid also renders neighbouring-month
    // days, so the click matches the full date stamp, not the number.
    const cell = await this.layoutsSheetCell(issueName, "Due date");
    const trigger = cell.locator("button:not([disabled])").first();
    await trigger.scrollIntoViewIfNeeded({ timeout: WebDriver.LAYOUTS_BODY_WAIT_MS });
    await trigger.click({ timeout: WebDriver.LAYOUTS_FIRST_WAIT_MS });
    const [year, month, day] = isoDate.split("-").map((part) => Number.parseInt(part, 10));
    const monthNames = [
      "January",
      "February",
      "March",
      "April",
      "May",
      "June",
      "July",
      "August",
      "September",
      "October",
      "November",
      "December",
    ];
    const monthName = monthNames[(month ?? 1) - 1] ?? "";
    // The picker root is a zero-height listbox container (Playwright
    // sees it as hidden), so the driver addresses its visible children
    // directly: native month/year selects plus the day grid.
    const monthCombo = this.page.getByRole("combobox", { name: "Choose the Month" }).first();
    await monthCombo.waitFor({ timeout: WebDriver.LAYOUTS_FIRST_WAIT_MS });
    await monthCombo.selectOption({ label: monthName });
    const yearCombo = this.page.getByRole("combobox", { name: "Choose the Year" }).first();
    await yearCombo.selectOption({ label: String(year) });
    const weekday = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"][
      new Date(Date.UTC(year ?? 0, (month ?? 1) - 1, day ?? 0)).getUTCDay()
    ];
    const suffix =
      day === 1 || day === 21 || day === 31
        ? "st"
        : day === 2 || day === 22
          ? "nd"
          : day === 3 || day === 23
            ? "rd"
            : "th";
    const dayButton = this.page
      .getByRole("button", { name: `${weekday}, ${monthName} ${day}${suffix}, ${year}`, exact: false })
      .first();
    await dayButton.waitFor({ timeout: WebDriver.LAYOUTS_FIRST_WAIT_MS });
    await dayButton.click({ timeout: WebDriver.LAYOUTS_FIRST_WAIT_MS });
    const deadline = Date.now() + WebDriver.LAYOUTS_BODY_WAIT_MS;
    for (;;) {
      if ((await this.layoutsSheetCellText(issueName, "Due date")).includes(String(day))) return;
      if (Date.now() >= deadline) throw new Error(`[parity] sheet cell never showed due day "${day}".`);
      await this.page.waitForTimeout(500);
    }
  }

  async layoutsSheetCellSetAssignee(issueName: string, memberName: string): Promise<void> {
    const cell = await this.layoutsSheetCell(issueName, "Assignees");
    const trigger = cell.locator("button:not([disabled])").first();
    await this.layoutsSheetPickOption(trigger, memberName, issueName, false);
    // The assignee picker is multi-select: unlike the single-select
    // pickers it stays open after a pick, so Escape dismisses it. The
    // spec asserts the server-side assignees right after, which proves
    // the dismissal kept the selection.
    await this.page.keyboard.press("Escape");
    // The assignee cell renders avatars, not the name, so the menu
    // closing (the option leaving the document) proves the pick landed.
    const deadline = Date.now() + WebDriver.LAYOUTS_BODY_WAIT_MS;
    for (;;) {
      const option = this.page.getByRole("option", { name: memberName, exact: false });
      if ((await option.count()) === 0) return;
      if (Date.now() >= deadline) throw new Error(`[parity] assignee menu never closed on "${memberName}".`);
      await this.page.waitForTimeout(500);
    }
  }

  async layoutsCalDayAddExisting(dayNumber: number): Promise<void> {
    await this.layoutsCalOpenDayAddMenu(dayNumber);
    await this.page.getByRole("menuitem", { name: "Add existing work item", exact: true }).first().click();
    const deadline = Date.now() + WebDriver.LAYOUTS_BODY_WAIT_MS;
    for (;;) {
      if (await this.layoutsAddExistingModalVisible()) return;
      if (Date.now() >= deadline) throw new Error("[parity] add-existing modal never opened.");
      await this.page.waitForTimeout(500);
    }
  }

  async layoutsAddExistingModalIssueNames(): Promise<string[]> {
    // The search fetch lands after the modal opens, so an empty list
    // waits for options (or the empty state) instead of reading once.
    const options = this.page.locator('label[for^="issue-"]');
    const deadline = Date.now() + WebDriver.LAYOUTS_BODY_WAIT_MS;
    for (;;) {
      if ((await options.count()) > 0) break;
      if ((await this.page.getByText("No work items found").count()) > 0) return [];
      if (!(await this.layoutsAddExistingModalVisible())) return [];
      if (Date.now() >= deadline) throw new Error("[parity] add-existing modal never listed issues.");
      await this.page.waitForTimeout(500);
    }
    const names: string[] = [];
    const count = await options.count();
    for (let i = 0; i < count; i++) {
      names.push(((await options.nth(i).locator("span.truncate").first().innerText()) ?? "").trim());
    }
    return names;
  }

  async layoutsMobileSwitchTo(layout: LayoutsLayoutKey): Promise<void> {
    // The compact selector offers list/kanban/calendar only; anything
    // else is a spec bug, failed loudly instead of hanging on a missing
    // menu item.
    if (layout !== "list" && layout !== "kanban" && layout !== "calendar") {
      throw new Error(`[parity] the mobile selector offers no "${layout}" layout.`);
    }
    await this.layoutsMobileLayoutTrigger().click();
    const item = this.page.getByRole("menuitem", { name: WebDriver.LAYOUTS_MOBILE_LABELS[layout], exact: false });
    await item.first().click();
    const deadline = Date.now() + 60_000;
    for (;;) {
      if ((await this.layoutsActiveLayout()) === layout) return;
      if (Date.now() >= deadline) throw new Error(`[parity] mobile switch to "${layout}" never applied.`);
      await this.page.waitForTimeout(500);
    }
  }

  async layoutsMobileDisplayCycleModuleDisabled(): Promise<{ cycleDisabled: boolean; moduleDisabled: boolean }> {
    // Disabled here means absent: the Display popover filters the
    // cycle/modules property buttons out when their features are off.
    await this.page.getByRole("button", { name: "Display", exact: true }).click();
    await this.page.getByText("Display Properties", { exact: true }).waitFor({ timeout: 15_000 });
    const cycle = await this.page.getByRole("button", { name: "Cycle", exact: true }).count();
    const module = await this.page.getByRole("button", { name: "Module", exact: true }).count();
    await this.page.keyboard.press("Escape");
    return { cycleDisabled: cycle === 0, moduleDisabled: module === 0 };
  }

  async layoutsRowMenuOpenNewTabUrl(issueName: string): Promise<string> {
    // window.open lands in a popup page: read its URL without waiting
    // for the app to boot there, then close it again.
    await this.layoutsOpenRowMenu(issueName);
    const [popup] = await Promise.all([
      this.page.context().waitForEvent("page", { timeout: 15_000 }),
      this.page.getByRole("menuitem", { name: "Open in new tab", exact: true }).first().click(),
    ]);
    const url = popup.url();
    await popup.close();
    return url;
  }

  async layoutsWorkItemModalHasText(text: string): Promise<boolean> {
    // Scoped to the dialog: the list behind the modal shows the same
    // names. Prefilled names sit in inputs (display value) or select
    // chips (text) depending on the field.
    const dialog = this.page.getByRole("dialog");
    if ((await dialog.count()) === 0) return false;
    if ((await dialog.getByText(text, { exact: false }).count()) > 0) return true;
    return dialog.first().evaluate((node, want) => {
      const fields = node.querySelectorAll("input, textarea, select");
      return Array.from(fields).some((field) => ((field as HTMLInputElement).value ?? "").includes(want));
    }, text);
  }

  async layoutsListPageMenuChoose(item: string): Promise<void> {
    // Copying writes to the clipboard, which headless Chromium denies
    // without an explicit grant; arrange it before the pick.
    if (item === "Copy link") await this.page.context().grantPermissions(["clipboard-read", "clipboard-write"]);
    await this.layoutsListPageMenuTrigger().click();
    await this.page.getByRole("menuitem", { name: item, exact: true }).first().click();
    const deadline = Date.now() + 15_000;
    for (;;) {
      if ((await this.page.getByRole("menuitem").count()) === 0) return;
      if (Date.now() >= deadline) throw new Error(`[parity] list page menu never closed after "${item}".`);
      await this.page.waitForTimeout(300);
    }
  }

  async layoutsGroupHeaderAddChoose(groupTitle: string, item: string | null): Promise<void> {
    const { menu, control } = await this.layoutsGroupHeaderAddControl(groupTitle);
    // Bounded like the row trigger: when the detail rail still covers
    // the plus, a forced dispatch opens the menu the click cannot reach.
    const press = async (): Promise<void> => {
      await control.click({ timeout: 10_000 }).catch(async () => control.click({ force: true }));
    };
    if (!menu || item === null) {
      await press();
    } else {
      await press();
      await this.page.getByRole("menuitem", { name: item, exact: true }).first().click();
    }
    // Either branch lands in a modal (create/add-existing); wait for one
    // instead of trusting the click-to-paint gap on the loaded host.
    const deadline = Date.now() + WebDriver.LAYOUTS_BODY_WAIT_MS;
    for (;;) {
      if ((await this.layoutsWorkItemModalVisible()) || (await this.layoutsAddExistingModalVisible())) return;
      if (Date.now() >= deadline) throw new Error("[parity] group-header add never opened a modal.");
      await this.page.waitForTimeout(500);
    }
  }

  async layoutsSheetToggleSubIssues(issueName: string): Promise<void> {
    // Same chevron as expand, but with no row-count expectation: past the
    // nesting limit the toggle opens peek instead of expanding inline.
    const first = this.layoutsSheetFirstCell(issueName);
    await first.waitFor({ timeout: WebDriver.LAYOUTS_BODY_WAIT_MS });
    await this.layoutsSheetToggleButtons(issueName).first().click({ timeout: WebDriver.LAYOUTS_FIRST_WAIT_MS });
  }
  // --- NEWFRONT-117 round 2: spreadsheet (ISS-015..020). ---

  private layoutsSheetTable(): Locator {
    return this.page
      .locator("table")
      .filter({ has: this.page.locator('th span:text-is("Work items")') })
      .first();
  }

  private async layoutsSheetHeaderCells(): Promise<Locator> {
    const table = this.layoutsSheetTable();
    await table.waitFor({ timeout: WebDriver.LAYOUTS_FIRST_WAIT_MS });
    const cells = table.locator("thead th");
    const deadline = Date.now() + WebDriver.LAYOUTS_BODY_WAIT_MS;
    for (;;) {
      if ((await cells.count()) > 0) return cells;
      if (Date.now() >= deadline) throw new Error("[parity] sheet header never rendered.");
      await this.page.waitForTimeout(500);
    }
  }

  private layoutsSheetFirstCell(issueName: string): Locator {
    return this.layoutsSheetTable().locator('td[id^="issue-"]', { hasText: issueName }).first();
  }

  private async layoutsSheetFirstCellName(cell: Locator): Promise<string> {
    // The leading cell carries no paragraph: its text is the identifier
    // button plus the name div, so the name is the cell text minus any
    // button text. Bounded like the list reads so a re-render mid-scan
    // cannot hang the caller to the test timeout.
    const text = await cell.innerText({ timeout: 10_000 }).catch(() => "");
    if (text.trim() === "") return "";
    const buttons = await cell
      .locator("button")
      .allInnerTexts()
      .catch(() => [] as string[]);
    const drop = new Set(buttons.map((line) => line.trim()).filter((line) => line !== ""));
    return text
      .split("\n")
      .map((line) => line.trim())
      .filter((line) => line !== "" && !drop.has(line))
      .join(" ")
      .trim();
  }

  private async layoutsSheetHeaderIndex(column: string): Promise<number> {
    const headers = await this.layoutsSheetHeaders();
    const index = headers.indexOf(column);
    if (index < 0) throw new Error(`[parity] no sheet column "${column}" (have: ${headers.join(", ")}).`);
    return index;
  }

  private async layoutsSheetCell(issueName: string, column: string): Promise<Locator> {
    const index = await this.layoutsSheetHeaderIndex(column);
    const first = this.layoutsSheetFirstCell(issueName);
    await first.waitFor({ timeout: WebDriver.LAYOUTS_BODY_WAIT_MS });
    return first.locator("xpath=ancestor::tr[1]").locator(":scope > td").nth(index);
  }

  private async layoutsSheetScroller(): Promise<ElementHandle<HTMLElement>> {
    const table = this.layoutsSheetTable();
    await table.waitFor({ timeout: WebDriver.LAYOUTS_FIRST_WAIT_MS });
    const handle = await table.evaluateHandle((node) => {
      let current = node.parentElement;
      while (current && current.scrollWidth <= current.clientWidth + 1) current = current.parentElement;
      return current as HTMLElement | null;
    });
    const element = handle.asElement() as ElementHandle<HTMLElement> | null;
    if (!element) throw new Error("[parity] no horizontal sheet scroller found.");
    return element;
  }

  async layoutsSheetHeaders(): Promise<string[]> {
    const cells = await this.layoutsSheetHeaderCells();
    const count = await cells.count();
    const titles: string[] = [];
    for (let i = 0; i < count; i++) {
      titles.push((((await cells.nth(i).innerText()) ?? "") as string).trim().replace(/\s+/g, " "));
    }
    return titles;
  }

  async layoutsSheetRowNames(): Promise<string[]> {
    const table = this.layoutsSheetTable();
    await table.waitFor({ timeout: WebDriver.LAYOUTS_FIRST_WAIT_MS });
    const cells = table.locator('td[id^="issue-"]');
    const deadline = Date.now() + WebDriver.LAYOUTS_BODY_WAIT_MS;
    for (;;) {
      if ((await cells.count()) > 0) break;
      if (Date.now() >= deadline) throw new Error("[parity] sheet rows never rendered.");
      await this.page.waitForTimeout(500);
    }
    const count = await cells.count();
    const names: string[] = [];
    for (let i = 0; i < count; i++) {
      const name = await this.layoutsSheetFirstCellName(cells.nth(i));
      if (name !== "") names.push(name);
    }
    return names;
  }

  async layoutsSheetFirstColumnSticky(): Promise<boolean> {
    const cells = await this.layoutsSheetHeaderCells();
    const first = cells.first();
    return (await first.evaluate((node) => getComputedStyle(node).position)) === "sticky";
  }

  async layoutsSheetHeaderSticky(): Promise<boolean> {
    const table = this.layoutsSheetTable();
    await table.waitFor({ timeout: WebDriver.LAYOUTS_FIRST_WAIT_MS });
    const head = table.locator("thead").first();
    return (await head.evaluate((node) => getComputedStyle(node).position)) === "sticky";
  }

  async layoutsSheetScrollRight(): Promise<void> {
    const scroller = await this.layoutsSheetScroller();
    await scroller.evaluate((node) => node.scrollTo({ left: node.scrollWidth }));
  }

  async layoutsSheetFirstColumnShadowed(): Promise<boolean> {
    const table = this.layoutsSheetTable();
    await table.waitFor({ timeout: WebDriver.LAYOUTS_FIRST_WAIT_MS });
    const first = table.locator('tbody td[id^="issue-"]').first();
    await first.waitFor({ timeout: WebDriver.LAYOUTS_BODY_WAIT_MS });
    const shadow = await first.evaluate((node) => (node as HTMLElement).style.boxShadow);
    return shadow !== "" && shadow !== "none";
  }

  async layoutsSheetCellText(issueName: string, column: string): Promise<string> {
    const cell = await this.layoutsSheetCell(issueName, column);
    return (((await cell.innerText()) ?? "") as string).trim().replace(/\s+/g, " ");
  }

  async layoutsSheetCellEditable(issueName: string, column: string): Promise<boolean> {
    // The trigger is the cell's first button; a second visible button
    // (present for members and guests alike) is not the dropdown, so
    // only the first answers.
    const cell = await this.layoutsSheetCell(issueName, column);
    const trigger = cell.locator("button").first();
    if ((await trigger.count()) === 0) return false;
    return !(await trigger.isDisabled());
  }

  private async layoutsSheetPickOption(
    trigger: Locator,
    optionName: string,
    _issueName: string,
    exact = true
  ): Promise<void> {
    await trigger.scrollIntoViewIfNeeded({ timeout: WebDriver.LAYOUTS_BODY_WAIT_MS });
    await trigger.click({ timeout: WebDriver.LAYOUTS_FIRST_WAIT_MS });
    // The property menus are listboxes (same family as the row's state
    // menu): options carry the option role, not button. Member options
    // prefix the avatar initial ("P Parity Mention"), so assignees
    // match by substring while states and priorities stay exact.
    const option = this.page.getByRole("option", { name: optionName, exact }).first();
    await option.waitFor({ timeout: WebDriver.LAYOUTS_FIRST_WAIT_MS });
    await option.click();
  }

  async layoutsSheetCellSetState(issueName: string, stateName: string): Promise<void> {
    const cell = await this.layoutsSheetCell(issueName, "State");
    const trigger = cell.locator("button:not([disabled])").first();
    await this.layoutsSheetPickOption(trigger, stateName, issueName);
    const deadline = Date.now() + WebDriver.LAYOUTS_BODY_WAIT_MS;
    for (;;) {
      if ((await this.layoutsSheetCellText(issueName, "State")) === stateName) return;
      if (Date.now() >= deadline) throw new Error(`[parity] sheet cell never showed state "${stateName}".`);
      await this.page.waitForTimeout(500);
    }
  }

  async layoutsSheetCellSetPriority(issueName: string, priorityName: string): Promise<void> {
    const cell = await this.layoutsSheetCell(issueName, "Priority");
    const trigger = cell.locator("button:not([disabled])").first();
    await this.layoutsSheetPickOption(trigger, priorityName, issueName);
    const deadline = Date.now() + WebDriver.LAYOUTS_BODY_WAIT_MS;
    for (;;) {
      if ((await this.layoutsSheetCellText(issueName, "Priority")) === priorityName) return;
      if (Date.now() >= deadline) throw new Error(`[parity] sheet cell never showed priority "${priorityName}".`);
      await this.page.waitForTimeout(500);
    }
  }

  async layoutsSheetFocusCell(issueName: string, column: string): Promise<void> {
    const cell = await this.layoutsSheetCell(issueName, column);
    await cell.evaluate((node) => (node as HTMLElement).focus());
  }

  async layoutsSheetPressArrow(arrow: "up" | "down" | "left" | "right"): Promise<void> {
    const key = { up: "ArrowUp", down: "ArrowDown", left: "ArrowLeft", right: "ArrowRight" }[arrow];
    await this.page.keyboard.press(key);
    await this.page.waitForTimeout(500);
  }

  async layoutsSheetFocusedCell(): Promise<{ issueName: string; column: string } | null> {
    const headers = await this.layoutsSheetHeaders();
    const cellIndex = await this.page.evaluate(() => {
      const active = document.activeElement as HTMLElement | null;
      const cell = active?.closest?.("td") as HTMLTableCellElement | null;
      if (!cell) return -1;
      return cell.cellIndex;
    });
    if (cellIndex < 0) return null;
    // The focused row's leading cell names the issue; resolve it back
    // through a locator so the shared name reader applies.
    const row = this.page.locator("td:focus-within").first().locator("xpath=ancestor::tr[1]");
    if ((await row.count()) === 0) return null;
    const issueName = await this.layoutsSheetFirstCellName(row.locator("td").first());
    if (!issueName) return null;
    const column = headers[cellIndex] ?? "";
    if (!column) return null;
    return { issueName, column };
  }

  private async layoutsSheetOpenSortMenu(column: string): Promise<Locator> {
    const index = await this.layoutsSheetHeaderIndex(column);
    const cells = await this.layoutsSheetHeaderCells();
    const header = cells.nth(index);
    await header.scrollIntoViewIfNeeded({ timeout: WebDriver.LAYOUTS_BODY_WAIT_MS });
    await header.click();
    const menu = this.page.getByRole("menuitem");
    const deadline = Date.now() + WebDriver.LAYOUTS_BODY_WAIT_MS;
    for (;;) {
      if ((await menu.count()) > 0) return menu;
      if (Date.now() >= deadline) throw new Error(`[parity] sort menu for "${column}" never opened.`);
      await this.page.waitForTimeout(500);
    }
  }

  private async layoutsSheetCloseMenu(): Promise<void> {
    await this.page.keyboard.press("Escape");
    const deadline = Date.now() + 30_000;
    for (;;) {
      if ((await this.page.getByRole("menuitem").count()) === 0) return;
      if (Date.now() >= deadline) throw new Error("[parity] menu never closed.");
      await this.page.waitForTimeout(500);
    }
  }

  async layoutsSheetSortMenu(column: string): Promise<string[]> {
    const menu = await this.layoutsSheetOpenSortMenu(column);
    const count = await menu.count();
    const entries: string[] = [];
    for (let i = 0; i < count; i++) {
      entries.push((((await menu.nth(i).innerText()) ?? "") as string).trim().replace(/\s+/g, " "));
    }
    await this.layoutsSheetCloseMenu();
    return entries;
  }

  async layoutsSheetSort(column: string, direction: "ascending" | "descending"): Promise<void> {
    const menu = await this.layoutsSheetOpenSortMenu(column);
    await menu.nth(direction === "ascending" ? 0 : 1).click();
    const deadline = Date.now() + WebDriver.LAYOUTS_BODY_WAIT_MS;
    for (;;) {
      if ((await this.layoutsSheetSortMarker(column)) !== "none") return;
      if (Date.now() >= deadline) throw new Error(`[parity] sort marker never appeared on "${column}".`);
      await this.page.waitForTimeout(500);
    }
  }

  async layoutsSheetClearSort(column: string): Promise<void> {
    const menu = await this.layoutsSheetOpenSortMenu(column);
    const count = await menu.count();
    for (let i = 0; i < count; i++) {
      const text = (((await menu.nth(i).innerText()) ?? "") as string).trim();
      if (text.includes("Clear sorting")) {
        await menu.nth(i).click();
        const deadline = Date.now() + WebDriver.LAYOUTS_BODY_WAIT_MS;
        for (;;) {
          if ((await this.layoutsSheetSortMarker(column)) === "none") return;
          if (Date.now() >= deadline) throw new Error(`[parity] sort marker never cleared on "${column}".`);
          await this.page.waitForTimeout(500);
        }
      }
    }
    await this.layoutsSheetCloseMenu();
    throw new Error(`[parity] no Clear sorting entry on "${column}".`);
  }

  async layoutsSheetSortMarker(column: string): Promise<"ascending" | "descending" | "none"> {
    const index = await this.layoutsSheetHeaderIndex(column);
    const cells = await this.layoutsSheetHeaderCells();
    const marker = cells.nth(index).locator("div.rounded-full svg").first();
    if ((await marker.count()) === 0) return "none";
    const cls = (await marker.getAttribute("class")) ?? "";
    // The sorted header carries a direction glyph: the wide-to-narrow
    // arrow marks the ascending key, the narrow-to-wide the descending.
    if (cls.includes("arrow-down-wide-narrow")) return "ascending";
    if (cls.includes("arrow-up-narrow-wide")) return "descending";
    throw new Error(`[parity] unknown sort marker classes "${cls}".`);
  }

  async layoutsSheetScrollEnd(): Promise<void> {
    const table = this.layoutsSheetTable();
    await table.waitFor({ timeout: WebDriver.LAYOUTS_FIRST_WAIT_MS });
    const scroller = await table.evaluateHandle((node) => {
      let current = node.parentElement;
      while (current && current.scrollHeight <= current.clientHeight + 1) current = current.parentElement;
      return current as HTMLElement | null;
    });
    const element = scroller.asElement() as ElementHandle<HTMLElement> | null;
    if (!element) throw new Error("[parity] no vertical sheet scroller found.");
    await element.evaluate((node) => node.scrollTo({ top: node.scrollHeight }));
    await this.page.waitForTimeout(1000);
  }

  async layoutsCurrentUrl(): Promise<string> {
    return this.page.url();
  }

  async layoutsStallIssuesGet(delayMs: number): Promise<void> {
    await this.page.route(/\/issues\//, async (route) => {
      if (route.request().method() !== "GET") {
        await route.continue();
        return;
      }
      await new Promise((resolve) => setTimeout(resolve, delayMs));
      await route.continue();
    });
  }

  private layoutsStalledMutations = 0;

  async layoutsStallIssueMutation(delayMs: number): Promise<void> {
    this.layoutsStalledMutations = 0;
    await this.page.route(/\/issues\//, async (route) => {
      const method = route.request().method();
      if (method !== "POST" && method !== "PATCH" && method !== "PUT") {
        await route.continue();
        return;
      }
      this.layoutsStalledMutations += 1;
      await new Promise((resolve) => setTimeout(resolve, delayMs));
      await route.continue();
    });
  }

  async layoutsStalledMutationCount(): Promise<number> {
    return this.layoutsStalledMutations;
  }

  async layoutsReleaseStalls(): Promise<void> {
    await this.page.unrouteAll({ behavior: "wait" });
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
    // Re-click while the layout is wrong — but only once it is STABLY wrong.
    // Clicking mid-load is destructive: the app persists its current (still
    // default) grouping over the preferences the scenario just patched, and
    // a swallowed click leaves the previous layout active. So the first
    // seconds only observe, and afterwards a click needs the same wrong
    // layout twice in a row (a loading switcher flips between values).
    const deadline = Date.now() + WebDriver.BOARD_SETTLE_WAIT_MS;
    const graceUntil = Date.now() + 12_000;
    let lastActive: BoardLayoutKey | null = null;
    let stableWrong = 0;
    for (;;) {
      const active = await this.boardActiveLayout();
      if (active === layout) {
        if (layout === "kanban" && (await this.kanbanBoardVisible())) return;
        if (layout === "gantt" && (await this.ganttTimelineVisible())) return;
        if (layout !== "kanban" && layout !== "gantt") return;
      }
      if (Date.now() > deadline) throw new Error(`[parity] timed out waiting for the ${layout} layout to render.`);
      // No active marker anywhere means the switcher is still loading (the
      // reader defaults that to list): never click, only wait.
      let anyActive = false;
      const buttonCount = await buttons.count();
      for (let buttonIndex = 0; buttonIndex < buttonCount; buttonIndex += 1) {
        const cls =
          (await buttons
            .nth(buttonIndex)
            .getAttribute("class")
            .catch(() => null)) ?? "";
        if (cls.includes("bg-layer-transparent-active")) {
          anyActive = true;
          break;
        }
      }
      if (!anyActive) {
        stableWrong = 0;
        lastActive = null;
      } else {
        stableWrong = active === lastActive ? stableWrong + 1 : 0;
        lastActive = active;
        if (Date.now() > graceUntil && stableWrong >= 2) {
          await buttons
            .nth(index)
            .click({ timeout: WebDriver.BOARD_FIRST_WAIT_MS })
            .catch(() => undefined);
          stableWrong = 0;
        }
      }
      await this.page.waitForTimeout(WebDriver.BOARD_POLL_STEP_MS);
    }
  }

  async kanbanOpenBoard(): Promise<void> {
    await this.boardWaitForLayout("kanban");
  }

  async kanbanBoardVisible(): Promise<boolean> {
    // Kanban column bodies carry ids of the shape {group}__{subgroup};
    // no other layout renders such ids. A fully collapsed board unmounts
    // every body but keeps the column shells (flat mode) or the lane bars
    // (swimlane mode), so those count as rendered too.
    const columns = this.boardMain().locator('div[id*="__"]');
    if ((await columns.count()) === 0) {
      const shells = await this.boardFlatColumnOuters().count();
      const bars = await this.kanbanLaneBars().count();
      if (shells === 0 && bars === 0) return false;
    }
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
    // An open header menu appends its entries to the header text, so the
    // count is the last number followed by a space or the end — not
    // necessarily trailing. Greedy name keeps digits inside names (cycle
    // suffixes) attached to the name.
    const clean = text.trim().replace(/\s+/g, " ");
    const match = /^(.*)\s+(\d+)(?:\s|$)/.exec(clean);
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

  private async boardCardByName(issueName: string): Promise<Locator> {
    // One evaluate per poll for the whole board: serial innerText round
    // trips race the virtualized window (shells mount, churn, and unmount
    // mid-read), and innerText depends on render state where textContent
    // reads DOM truth. Placeholder shells carry no name span and read as
    // "". Lane columns also load lazily after their headers mount, so a
    // just-opened board may need a beat before the card has content.
    const cards = this.boardCardLinks();
    const deadline = Date.now() + 60_000;
    for (;;) {
      const index = await cards.evaluateAll((els, wanted) => {
        for (let i = 0; i < els.length; i += 1) {
          const name = els[i]?.querySelector("div.text-body-sm-medium > span")?.textContent?.trim() ?? "";
          if (name === wanted) return i;
        }
        return -1;
      }, issueName);
      if (index >= 0) return cards.nth(index);
      if (Date.now() > deadline) {
        throw new Error(`[parity] no kanban card titled ${JSON.stringify(issueName)}.`);
      }
      await this.page.waitForTimeout(1_000);
    }
  }

  async kanbanCards(): Promise<KanbanCard[]> {
    // One evaluate for the whole board: per-card round trips stall when a
    // hundred virtualized cards mount, churn, and unmount mid-read.
    return this.boardCardLinks().evaluateAll((cards) =>
      cards.map((card) => {
        const parts = (card.id ?? "").split("_");
        const name = card.querySelector("div.text-body-sm-medium > span")?.textContent?.trim() ?? "";
        return {
          issueId: parts[1] ?? "",
          name,
          groupId: parts[2] ?? "",
          subGroupId: parts.slice(3).join("_"),
        };
      })
    );
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
    // One evaluate for the whole column: per-card round trips stall when a
    // hundred virtualized cards mount, churn, and unmount mid-read.
    return outer
      .locator('a[id^="issue_"]')
      .evaluateAll((cards) =>
        cards.map((card) => card.querySelector("div.text-body-sm-medium > span")?.textContent?.trim() ?? "")
      );
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
    // Card links target a new tab; the app opens peek client-side instead,
    // so never wait for a navigation here (see ganttOpenRowPeek).
    await card.click({ timeout: WebDriver.WAIT_MS, noWaitAfter: true });
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
    // The title is an editable textarea (inputs never appear in innerText);
    // the line after the identifier is only its character counter.
    const title = this.issuePeekPanel().locator("textarea").first();
    if ((await title.count()) > 0) {
      const value = await title.inputValue().catch(() => null);
      if (value !== null && value !== "") return value;
    }
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
    const clickEntry = async (): Promise<void> => {
      if ((await this.boardHeaderButtons(outer).count()) > 1) {
        await this.boardHeaderButtons(outer)
          .nth(1)
          .click({ timeout: WebDriver.WAIT_MS })
          .catch(() => undefined);
      } else {
        await header
          .locator("span.cursor-pointer")
          .first()
          .click({ timeout: WebDriver.WAIT_MS })
          .catch(() => undefined);
      }
    };
    // Fingerprint of the rendered board; equal readings on consecutive polls
    // mean the board is calm. Reads degrade to partial on churn (never
    // throw): a churning board must quiet the loop, not fail it. The race
    // bounds the whole read: locator waits below inherit long timeouts, and
    // one hanging read must never starve the modal check above.
    const fingerprint = async (): Promise<string> => {
      const read = (async () => {
        const columns = await this.kanbanColumns().catch(() => []);
        const cards = await this.kanbanCards().catch(() => []);
        return JSON.stringify({
          columns: columns.map((entry) => [entry.name, entry.count]),
          cards: cards.length,
        });
      })();
      const timedOut = new Promise<string>((resolve) => {
        setTimeout(() => resolve(`timeout-${Date.now()}`), 15_000);
      });
      return Promise.race([read, timedOut]);
    };
    await clickEntry();
    // Re-click while nothing opened: a click that lands mid-hydration can be
    // swallowed, and a board re-render can close the menu under us. Re-clicks
    // only fire on a calm board: clicking into churn risks mis-clicks that
    // open stray popups and keep the board churning under us. The loop never
    // re-resolves the column: the menu locator is page-global, and a re-render
    // between the click and the modal/menu paint would make a re-lookup
    // throw spuriously.
    const deadline = Date.now() + WebDriver.BOARD_FIRST_WAIT_MS;
    const menuItems = this.boardHeaderMenuItems();
    let lastClick = Date.now();
    let lastFingerprint = "";
    for (;;) {
      if (await this.kanbanCreateModalVisible()) return;
      if ((await menuItems.count()) > 0) return;
      // Bounded explicitly: locator waits inherit long timeouts, and the
      // columns can vanish for minutes (a silent board unmount observed
      // after the click) — one hanging read must never starve this loop.
      const headerText = await header.innerText({ timeout: 10_000 }).catch(() => "");
      if (headerText.includes("Add an existing work item")) return;
      if (Date.now() > deadline) throw new Error("[parity] header create opened neither a modal nor a menu.");
      const current = await fingerprint();
      const calm = lastFingerprint !== "" && current === lastFingerprint;
      lastFingerprint = current;
      if (calm && Date.now() - lastClick > 4_000) {
        // No Escape here: an open-but-slowly-painting modal must never be
        // dismissed by its own waiter; a re-click suffices.
        await clickEntry();
        lastClick = Date.now();
        lastFingerprint = "";
      }
      await this.page.waitForTimeout(500);
    }
  }

  async kanbanCreateModalVisible(): Promise<boolean> {
    // The modal title is the stable signal; the assignee placeholder covers
    // variants that render the picker before the title paints. Any visible
    // title counts: the app can stage a hidden twin of the dialog whose
    // text matches first in document order.
    const title = this.page.getByText("Create new work item", { exact: true });
    const titleCount = await title.count();
    for (let index = 0; index < titleCount; index += 1) {
      if (await title.nth(index).isVisible()) return true;
    }
    const field = this.page.getByPlaceholder("Assignees");
    const fieldCount = await field.count();
    for (let index = 0; index < fieldCount; index += 1) {
      if (await field.nth(index).isVisible()) return true;
    }
    return false;
  }

  private boardHeaderMenuItems(): Locator {
    // Page-global: the header menu portals outside the column, so waiters
    // use this directly instead of re-resolving the column mid-paint.
    return this.page.locator('[role="menu"] [role="menuitem"], [role="menu"] button');
  }

  private async boardHeaderMenuScope(columnName: string): Promise<{ header: Locator; items: Locator }> {
    const outer = await this.boardFlatColumnOuterByName(columnName);
    const header = outer.locator(":scope > div.sticky").first();
    return { header, items: this.boardHeaderMenuItems() };
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
    // The dialog-role wrapper is a zero-height portal anchor at the viewport
    // edge (never "visible" itself); the title text carries visibility.
    const title = this.page.getByText("Delete Work item", { exact: true });
    if ((await title.count()) === 0) return false;
    return await title.first().isVisible();
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
    // One evaluate for the whole cell: same virtualization churn as the
    // column reader; placeholder shells read as "".
    return cell
      .locator('a[id^="issue_"]')
      .evaluateAll((cards) =>
        cards.map((card) => card.querySelector("div.text-body-sm-medium > span")?.textContent?.trim() ?? "")
      );
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
    // The board container is the innermost ancestor that both overflows
    // horizontally and actually scrolls (overflow-x auto/scroll): an inner
    // sizing wrapper overflows without scrolling and always reads 0.
    return await this.page.evaluate(() => {
      const probe = document.querySelector('main div[id*="__"]') ?? document.querySelector("main");
      let node: HTMLElement | null = probe instanceof HTMLElement ? probe : null;
      while (node) {
        const axis = window.getComputedStyle(node).overflowX;
        if (node.scrollWidth > node.clientWidth + 4 && (axis === "auto" || axis === "scroll")) {
          return { x: node.scrollLeft, y: node.scrollTop };
        }
        node = node.parentElement;
      }
      return { x: 0, y: 0 };
    });
  }

  async kanbanColumnScroll(columnName: string): Promise<{ x: number; y: number }> {
    const outer = await this.boardFlatColumnOuterByName(columnName);
    return await outer.evaluate((root) => {
      const body = root.querySelector('div[id*="__"]');
      let node: HTMLElement | null = body instanceof HTMLElement ? body : root;
      while (node && root.contains(node)) {
        const axis = window.getComputedStyle(node).overflowY;
        if (
          (node.scrollHeight > node.clientHeight + 4 || node.scrollWidth > node.clientWidth + 4) &&
          (axis === "auto" || axis === "scroll")
        ) {
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
    // Holds track the grabbed card's own axis so the cursor stays over its
    // column (vertical holds) or its row band (horizontal holds); a centered
    // hold can land over a short neighbor column with no room to scroll.
    const clamp = (value: number, max: number): number => Math.min(Math.max(value, margin), max - margin);
    const onto =
      edge === "left"
        ? { x: margin, y: clamp(from.y, viewport.height) }
        : edge === "right"
          ? { x: viewport.width - margin, y: clamp(from.y, viewport.height) }
          : edge === "top"
            ? { x: clamp(from.x, viewport.width), y: margin }
            : { x: clamp(from.x, viewport.width), y: viewport.height - margin };
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
    // The zoom switcher renders one clickable div per zoom level (exact
    // label text, cursor-pointer); the active one carries the pill marker.
    const views: string[] = [];
    for (const view of ["Week", "Month", "Quarter"] as const) {
      const entry = root.locator(`xpath=.//div[normalize-space(.)='${view}' and contains(@class,'cursor-pointer')]`);
      if ((await entry.count()) > 0) views.push(view);
    }
    const hasToday = (await root.getByRole("button", { name: "Today", exact: true }).count()) > 0;
    const hasFullscreen = (await this.ganttFullscreenButton().count()) > 0;
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
    // Week cells glue the weekday to the date ("Su31"), so no word
    // boundary follows the name. Month/Quarter match first, keeping the
    // bare prefixes unambiguous.
    if (/^(Su|Mo|Tu|We|Th|Fr|Sa|M|T|W|F)/i.test(first)) return "Week";
    return "unknown";
  }

  async ganttSetZoom(view: GanttZoom): Promise<void> {
    const entry = this.ganttChartRoot()
      .locator(`xpath=.//div[normalize-space(.)='${view}' and contains(@class,'cursor-pointer')]`)
      .first();
    await entry.click({ timeout: WebDriver.WAIT_MS });
    await this.boardSettle("zoom switch", async () => (await this.ganttActiveZoom()) === view);
    // The switch re-scrolls on a deferred tick after rendering; wait for
    // the offset to land so today-visibility reads don't race the scroll.
    let last = await this.ganttScrollLeft();
    const deadline = Date.now() + 15_000;
    for (;;) {
      await this.page.waitForTimeout(400);
      const now = await this.ganttScrollLeft();
      if (now === last) return;
      last = now;
      if (Date.now() > deadline) return;
    }
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
    // Today restores the standard scroll offset with the marker in view;
    // the marker lands off-center (about 275px right of it), so settle on
    // visibility, not centering.
    await this.boardSettle("today re-center", async () => this.ganttTodayVisible());
  }

  async ganttTodayVisible(): Promise<boolean> {
    const viewport = this.page.viewportSize() ?? { width: 1280, height: 720 };
    const rects = await this.ganttTodayRects();
    if (rects.some((rect) => rect.left < viewport.width && rect.right > 0)) return true;
    // NEWFRONT-163: on the week's last day the Month view renders no today
    // marker at all, although the switch still re-centers on today's week.
    // Fall back to the current-month pill, which the re-center keeps in
    // view every day. Other zooms keep the strict marker reading.
    if ((await this.ganttActiveZoom()) !== "Month") return false;
    const pill = this.ganttContainer().locator('span[class*="bg-accent-primary"]', { hasText: "Current" }).first();
    if ((await pill.count()) === 0) return false;
    const box = await pill.boundingBox();
    if (!box) return false;
    return box.x < viewport.width && box.x + box.width > 0;
  }

  async ganttTodayHighlighted(): Promise<boolean> {
    return (await this.ganttContainer().locator('div[class*="bg-accent-primary/20"]').count()) > 0;
  }

  private ganttFullscreenButton(): Locator {
    // The header (count + zoom + Today + fullscreen) is the timeline
    // container's preceding sibling in both inline and portal modes, and
    // the toggle is its only bordered icon button. Never match page-wide:
    // other overlays carry empty-text buttons past the chart in DOM order.
    return this.ganttContainer().locator("xpath=preceding-sibling::div//button[contains(@class,'border-subtle')]");
  }

  async ganttToggleFullscreen(): Promise<void> {
    const before = await this.ganttFullscreenActive();
    await this.ganttFullscreenButton().first().click({ timeout: WebDriver.WAIT_MS });
    await this.boardSettle("fullscreen toggle", async () => (await this.ganttFullscreenActive()) !== before);
  }

  async ganttFullscreenActive(): Promise<boolean> {
    return (await this.page.locator("#full-screen-portal #gantt-container").count()) > 0;
  }

  async ganttTimelineWidth(): Promise<number> {
    // The scrollable content width minus the sticky sidebar: day columns
    // grow when the infinite range extends, the sidebar never does.
    return await this.page.evaluate(() => {
      const container = document.querySelector("#gantt-container");
      const sidebar = document.querySelector("#gantt-sidebar");
      if (!(container instanceof HTMLElement)) return 0;
      const side = sidebar instanceof HTMLElement ? sidebar.getBoundingClientRect().width : 0;
      return Math.round(container.scrollWidth - side);
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
    // The duration cell is the row's trailing flex-shrink-0 div, a sibling
    // of the link's own wrapper. Anchor on the nearest row ancestor so a
    // dated row's duration can never leak into an undated row's read.
    return link.locator("xpath=ancestor::div[contains(@class,'px-page-x')][1]//div[contains(@class,'flex-shrink-0')]");
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
    // The row link is a target=_blank anchor: never wait for a navigation
    // after the click (the app opens peek client-side instead). The peek
    // waits below still fail if the panel never opens.
    await link.click({ timeout: WebDriver.WAIT_MS, noWaitAfter: true });
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
    // Dated bars carry a positive style width; undated rows mount a
    // full-row invisible overlay with no width style, so only the style
    // tells them apart.
    const issueId = await this.ganttIssueIdByName(issueName);
    const bar = this.ganttBar(issueId);
    if ((await bar.count()) === 0) return false;
    const width = await bar.evaluate((element) => (element as HTMLElement).style.width);
    const px = /^(-?\d+(?:\.\d+)?)px$/.exec(width ?? "");
    return !!px && Number(px[1]) > 0;
  }

  private ganttBarHandle(issueId: string, side: "left" | "right"): Locator {
    const bar = this.ganttBar(issueId);
    const marker = side === "left" ? "-left-1.5" : "-right-1.5";
    return bar.locator(`div.cursor-col-resize[class*="${marker}"]`).first();
  }

  private async ganttClearDragSpan(
    measure: () => Promise<{ fromX: number; ontoX: number; y: number }>
  ): Promise<{ from: { x: number; y: number }; onto: { x: number; y: number } }> {
    // Raw bar drags must grab on the bar (clear of the sticky sidebar)
    // and drop inside the viewport, or the release is lost and the commit
    // never fires. They must also stay out of the app's drag auto-scroll
    // bands (15% at each chart edge), which scroll mid-drag and fold the
    // extra travel into the commit. Scroll until the whole span sits in
    // the safe band with margin.
    const viewport = this.page.viewportSize() ?? { width: 1280, height: 720 };
    for (let attempt = 0; attempt < 4; attempt += 1) {
      const side = await this.ganttSidebar().boundingBox();
      const chart = await this.ganttContainer().boundingBox();
      const sideRight = side ? side.x + side.width : 360;
      const chartWidth = chart ? chart.width - (sideRight - chart.x) : viewport.width - sideRight;
      const band = chartWidth * 0.25;
      const minX = sideRight + band;
      const maxX = sideRight + chartWidth - band;
      const { fromX, ontoX, y } = await measure();
      const lo = Math.min(fromX, ontoX);
      const hi = Math.max(fromX, ontoX);
      if (lo >= minX && hi <= maxX) return { from: { x: fromX, y }, onto: { x: ontoX, y } };
      // Shift exactly onto the band edge: padded overcorrections ping-pong
      // across narrow bands instead of converging.
      const shift = lo < minX ? lo - minX - 10 : hi - maxX + 10;
      await this.ganttScrollTo((await this.ganttScrollLeft()) + shift);
    }
    throw new Error("[parity] could not clear room for the timeline drag.");
  }

  private async ganttBarDragEngaged(
    bar: Locator,
    prop: "marginLeft" | "width",
    before: string,
    from: { x: number; y: number },
    onto: { x: number; y: number }
  ): Promise<void> {
    await this.page.mouse.move(from.x, from.y);
    await this.page.mouse.down();
    // Probe three quarters across: day snapping can swallow a half-day
    // midpoint for single-day drags, but never a three-quarter one.
    await this.page.mouse.move(from.x + (onto.x - from.x) * 0.75, from.y, { steps: 6 });
    await this.page.waitForTimeout(500);
    // The app live-updates the bar mid-drag; no change means the grab
    // missed (sidebar cover, virtualized placeholder) and no commit will
    // follow, so fail loudly instead of settling on a phantom drag.
    const mid = await bar.evaluate((element, name) => (element as HTMLElement).style[name], prop);
    if (mid === before) {
      await this.page.mouse.up();
      throw new Error("[parity] bar drag did not engage; the grab missed the bar.");
    }
    await this.page.mouse.move(onto.x, onto.y, { steps: 6 });
    await this.page.mouse.up();
  }

  async ganttDragBar(issueName: string, dayDelta: number): Promise<void> {
    if (dayDelta === 0) throw new Error("[parity] bar drag needs a nonzero day delta.");
    const issueId = await this.ganttIssueIdByName(issueName);
    const bar = this.ganttBar(issueId);
    const pxPerDay = await this.ganttDayWidth();
    await bar.scrollIntoViewIfNeeded({ timeout: WebDriver.WAIT_MS });
    const { from, onto } = await this.ganttClearDragSpan(async () => {
      const box = await bar.boundingBox();
      if (!box) throw new Error("[parity] bar has no box.");
      const fromX = box.x + box.width / 2;
      return { fromX, ontoX: fromX + dayDelta * pxPerDay, y: box.y + box.height / 2 };
    });
    const before = await bar.evaluate((element) => (element as HTMLElement).style.marginLeft);
    await this.ganttBarDragEngaged(bar, "marginLeft", before, from, onto);
    // No post-drop UI settle: the bar's post-drop position is racy
    // (NEWFRONT-161) while the server persist is exact, so scenarios
    // assert the persisted dates.
  }

  async ganttAttemptBarMove(issueName: string, dayDelta: number): Promise<void> {
    if (dayDelta === 0) throw new Error("[parity] bar drag needs a nonzero day delta.");
    const issueId = await this.ganttIssueIdByName(issueName);
    const bar = this.ganttBar(issueId);
    const pxPerDay = await this.ganttDayWidth();
    await bar.scrollIntoViewIfNeeded({ timeout: WebDriver.WAIT_MS });
    const { from, onto } = await this.ganttClearDragSpan(async () => {
      const box = await bar.boundingBox();
      if (!box) throw new Error("[parity] bar has no box.");
      const fromX = box.x + box.width / 2;
      return { fromX, ontoX: fromX + dayDelta * pxPerDay, y: box.y + box.height / 2 };
    });
    await this.page.mouse.move(from.x, from.y);
    await this.page.mouse.down();
    await this.page.mouse.move(onto.x, onto.y, { steps: 12 });
    await this.page.mouse.up();
    await this.page.waitForTimeout(3_000);
  }

  async ganttResizeBar(issueName: string, side: "left" | "right", dayDelta: number): Promise<void> {
    if (dayDelta === 0) throw new Error("[parity] bar resize needs a nonzero day delta.");
    const issueId = await this.ganttIssueIdByName(issueName);
    const bar = this.ganttBar(issueId);
    const pxPerDay = await this.ganttDayWidth();
    await bar.scrollIntoViewIfNeeded({ timeout: WebDriver.WAIT_MS });
    const handle = this.ganttBarHandle(issueId, side);
    await handle.waitFor({ timeout: WebDriver.WAIT_MS });
    const { from, onto } = await this.ganttClearDragSpan(async () => {
      const box = await handle.boundingBox();
      if (!box) throw new Error("[parity] resize handle has no box.");
      // Grab the handle's outer strip: its inner edge sits exactly on the
      // bar content boundary, where the grab can land the move handler.
      const fromX = side === "left" ? box.x + 2 : box.x + box.width - 2;
      return { fromX, ontoX: fromX + dayDelta * pxPerDay, y: box.y + box.height / 2 };
    });
    const before = await bar.evaluate((element) => (element as HTMLElement).style.width);
    await this.ganttBarDragEngaged(bar, "width", before, from, onto);
  }

  async ganttResizePreview(issueName: string, side: "left" | "right"): Promise<string | null> {
    const issueId = await this.ganttIssueIdByName(issueName);
    const bar = this.ganttBar(issueId);
    const handle = this.ganttBarHandle(issueId, side);
    if ((await handle.count()) === 0) return null;
    await bar.scrollIntoViewIfNeeded({ timeout: WebDriver.WAIT_MS });
    // The bar's own sticky name label covers the handle center and the
    // sidebar can cover a freshly scrolled bar, so clear the handle past
    // the sidebar and hover its exposed outer strip with the raw mouse.
    for (let attempt = 0; attempt < 3; attempt += 1) {
      const box = await handle.boundingBox();
      const sideBox = await this.ganttSidebar().boundingBox();
      if (!box || !sideBox) return null;
      if (box.x >= sideBox.x + sideBox.width + 4) break;
      await this.ganttScrollTo((await this.ganttScrollLeft()) + (box.x - sideBox.x - sideBox.width - 120));
    }
    const box = await handle.boundingBox();
    if (!box) return null;
    const at =
      side === "left"
        ? { x: box.x + 1, y: box.y + box.height / 2 }
        : { x: box.x + box.width - 1, y: box.y + box.height / 2 };
    await this.page.mouse.move(at.x, at.y);
    await this.page.waitForTimeout(800);
    const pill = bar.locator("div.bg-accent-subtle").first();
    if ((await pill.count()) === 0) return null;
    if (!(await pill.isVisible())) return null;
    return (await pill.innerText()).trim() || null;
  }

  async ganttHandlesVisible(issueName: string): Promise<boolean> {
    const issueId = await this.ganttIssueIdByName(issueName);
    return (await this.ganttBar(issueId).locator("div.cursor-col-resize").count()) > 0;
  }

  private async ganttChartPointForRow(issueName: string, dayOffset: number): Promise<{ x: number; y: number }> {
    // Bars live in an overlay layer, rows in a sibling layer, so chart
    // rows cannot be resolved from bars. Sidebar rows align vertically
    // with chart rows, and day columns start past the sticky sidebar.
    const pxPerDay = await this.ganttDayWidth();
    const link = await this.ganttSidebarLinkByName(issueName);
    await link.scrollIntoViewIfNeeded({ timeout: WebDriver.WAIT_MS });
    const linkBox = await link.boundingBox();
    const sidebarBox = await this.ganttSidebar().boundingBox();
    if (!linkBox || !sidebarBox) throw new Error("[parity] timeline row has no box.");
    return {
      x: sidebarBox.x + sidebarBox.width + dayOffset * pxPerDay,
      y: linkBox.y + linkBox.height / 2,
    };
  }

  async ganttRowAddVisible(issueName: string): Promise<boolean> {
    const at = await this.ganttChartPointForRow(issueName, 2);
    await this.page.mouse.move(at.x, at.y);
    await this.page.waitForTimeout(800);
    // The "+" mounts under the cursor on row hover; dated rows and guests
    // render no add layer at all.
    const add = this.ganttContainer().locator("button.absolute").first();
    if ((await add.count()) === 0) return false;
    return await add.isVisible();
  }

  async ganttAddBlock(issueName: string, dayOffset: number): Promise<void> {
    const at = await this.ganttChartPointForRow(issueName, dayOffset);
    await this.page.mouse.move(at.x, at.y);
    const add = this.ganttContainer().locator("button.absolute").first();
    await add.waitFor({ timeout: WebDriver.WAIT_MS });
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

  // --- Root document shell + not-found (NEWFRONT-173, SHELL-107/108). ---
  // --- Every read below was observed on the running oracle: the head ---
  // --- carries product metadata plus PWA markers, two portal roots sit ---
  // --- above the provider tree, and no recorder snippet loads. ---

  async documentShellFacts(): Promise<DocumentShellFacts> {
    return await this.page.evaluate(() => {
      const meta = (key: string): string | null => {
        const byName = document.querySelector<HTMLMetaElement>(`meta[name="${key}"]`);
        if (byName !== null) return byName.content || null;
        return document.querySelector<HTMLMetaElement>(`meta[property="${key}"]`)?.content ?? null;
      };
      const links = (rel: string): string[] =>
        [...document.querySelectorAll<HTMLLinkElement>(`link[rel="${rel}"]`)]
          .map((link) => link.getAttribute("href") ?? "")
          .filter((href) => href.length > 0);
      const sizeOf = (key: "og:image:width" | "og:image:height"): number | null => {
        const raw = meta(key);
        if (raw === null) return null;
        const parsed = Number.parseInt(raw, 10);
        return Number.isNaN(parsed) ? null : parsed;
      };
      const width = sizeOf("og:image:width");
      const height = sizeOf("og:image:height");
      return {
        lang: document.documentElement.getAttribute("lang"),
        title: document.title,
        description: meta("description"),
        keywordsPresent: document.querySelector('meta[name="keywords"]') !== null,
        viewport: meta("viewport"),
        themeColor: meta("theme-color"),
        robots: meta("robots"),
        ogTitle: meta("og:title"),
        ogDescription: meta("og:description"),
        ogUrl: meta("og:url"),
        ogImage: meta("og:image"),
        ogImageSize: width === null || height === null ? null : { width, height },
        ogImageAlt: meta("og:image:alt"),
        twitterSite: meta("twitter:site"),
        twitterCard: meta("twitter:card"),
        installability: {
          applicationName: meta("application-name"),
          appleMobileCapable: meta("apple-mobile-web-app-capable"),
          mobileWebCapable: meta("mobile-web-app-capable"),
        },
        iconHrefs: [...links("icon"), ...links("shortcut icon")],
        appleTouchIconHrefs: links("apple-touch-icon"),
        manifestHrefs: links("manifest"),
        rootColorScheme: document.documentElement.style.getPropertyValue("color-scheme").trim() || null,
        mainMounted: document.querySelector("main") !== null,
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

  private async ganttScrollArrowIndex(issueName: string): Promise<number | null> {
    // One sticky arrow per off-screen bar, mounted in its chart row; rows
    // carry no id, so match the arrow overlapping the sidebar link's band.
    const link = await this.ganttSidebarLinkByName(issueName);
    const linkBox = await link.boundingBox();
    if (!linkBox) return null;
    const targetY = linkBox.y + linkBox.height / 2;
    return await this.page.evaluate((bandY) => {
      const arrows = [...document.querySelectorAll("#gantt-container button.sticky")];
      const index = arrows.findIndex((arrow) => {
        const rect = (arrow as HTMLElement).getBoundingClientRect();
        return rect.width > 0 && bandY >= rect.y - 12 && bandY <= rect.y + rect.height + 12;
      });
      return index >= 0 ? index : null;
    }, targetY);
  }

  async ganttScrollArrowVisible(issueName: string): Promise<boolean> {
    const index = await this.ganttScrollArrowIndex(issueName);
    if (index === null) return false;
    return await this.ganttContainer().locator("button.sticky").nth(index).isVisible();
  }

  async ganttClickScrollArrow(issueName: string): Promise<void> {
    const index = await this.ganttScrollArrowIndex(issueName);
    if (index === null) throw new Error(`[parity] no scroll arrow for ${JSON.stringify(issueName)}.`);
    await this.ganttContainer().locator("button.sticky").nth(index).click({ timeout: WebDriver.WAIT_MS });
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
    // The chart's own skeleton rows paint at most one frame (the layout
    // loader covers the fetch), so the observable loading state is the
    // layout-level pulsing placeholder before the chart mounts.
    const pattern = "**/api/**/issues**";
    await this.page.route(pattern, async (route) => {
      await new Promise((resolve) => setTimeout(resolve, 2_500));
      // A reload can cancel the held request first; that is fine.
      await route.continue().catch(() => undefined);
    });
    try {
      await this.page.reload();
      const deadline = Date.now() + 120_000;
      for (;;) {
        const loader = this.page.locator("div.animate-pulse").first();
        if ((await loader.count()) > 0 && (await loader.isVisible().catch(() => false))) return true;
        if (Date.now() > deadline) return false;
        await this.page.waitForTimeout(250);
      }
    } finally {
      await this.page.unroute(pattern);
    }
  }

  async ganttEmptyVisible(): Promise<boolean> {
    // Projects without issues render the first-run empty state instead of
    // the chart, with the layout switcher still marking gantt active.
    const heading = this.page.getByRole("heading", { name: "Start with your first work item." });
    if ((await heading.count()) === 0) return false;
    return await heading.first().isVisible();
  }

  async ganttLoadMoreObservedOnScroll(): Promise<boolean> {
    // Trip the infinite-scroll sentinel on a 100+ issue timeline: the
    // delayed page-two fetch holds the pulsing placeholder up for the
    // poll. Re-scroll while polling so late-loading page one still trips.
    const pattern = "**/api/**/issues**";
    await this.page.route(pattern, async (route) => {
      await new Promise((resolve) => setTimeout(resolve, 2_500));
      // A reload can cancel the held request first; that is fine.
      await route.continue().catch(() => undefined);
    });
    try {
      await this.page.reload();
      await this.ganttOpenTimeline();
      const deadline = Date.now() + 180_000;
      for (;;) {
        if (await this.ganttLoadMoreVisible().catch(() => false)) return true;
        if (Date.now() > deadline) return false;
        await this.ganttContainer()
          .evaluate((element) => {
            element.scrollTop = element.scrollHeight;
          })
          .catch(() => undefined);
        await this.page.waitForTimeout(1_000);
      }
    } finally {
      await this.page.unroute(pattern);
    }
  }

  async overlayPortalsPresent(): Promise<{ contextMenu: boolean; editor: boolean }> {
    return await this.page.evaluate(() => ({
      contextMenu: document.getElementById("context-menu-portal") !== null,
      editor: document.getElementById("editor-portal") !== null,
    }));
  }

  async sessionRecorderPresent(): Promise<boolean> {
    // Absence marker read from the old sources (an absence cannot be
    // observed on a passing run): a tagged loader script plus any script
    // fetched from the recorder host. Verified absent on the oracle.
    return await this.page.evaluate(() => {
      if (document.getElementById("clarity-tracking") !== null) return true;
      return [...document.scripts].some((script) => script.src.includes("clarity.ms"));
    });
  }

  async installAssetStatuses(): Promise<{ href: string; status: number }[]> {
    const facts = await this.documentShellFacts();
    const hrefs = [...new Set([...facts.manifestHrefs, ...facts.iconHrefs, ...facts.appleTouchIconHrefs])];
    const out: { href: string; status: number }[] = [];
    for (const href of hrefs) {
      const url = new URL(href, this.page.url()).toString();
      try {
        out.push({ href, status: (await this.page.request.get(url)).status() });
      } catch {
        out.push({ href, status: 0 });
      }
    }
    return out;
  }

  async notFoundFacts(): Promise<NotFoundFacts | null> {
    const title = await this.page.title();
    if (!title.includes("404")) return null;
    const surface = await this.page.evaluate(() => {
      const scope = document.querySelector("main") ?? document.body;
      const heading =
        scope.querySelector("h1, h2, h3")?.textContent?.trim() ||
        document.querySelector("h1, h2, h3")?.textContent?.trim() ||
        null;
      const body =
        [...scope.querySelectorAll("p")]
          .map((node) => node.textContent?.trim() ?? "")
          .filter((text) => text.length > 0)
          .sort((a, b) => b.length - a.length)[0] ?? null;
      const home = scope.querySelector<HTMLAnchorElement>('a[href="/"]');
      const img = scope.querySelector("img");
      return {
        heading,
        body,
        homeHref: home?.getAttribute("href") ?? null,
        homeLabel: home?.textContent?.trim() || null,
        illustrationSrc: img?.getAttribute("src") ?? null,
        illustrationAlt: img?.getAttribute("alt") ?? null,
        robots: document.querySelector<HTMLMetaElement>('meta[name="robots"]')?.content ?? null,
      };
    });
    let illustration: NotFoundFacts["illustration"] = null;
    if (surface.illustrationSrc !== null) {
      const url = new URL(surface.illustrationSrc, this.page.url()).toString();
      let status = 0;
      try {
        status = (await this.page.request.get(url)).status();
      } catch {
        status = 0;
      }
      illustration = { src: surface.illustrationSrc, alt: surface.illustrationAlt ?? "", status };
    }
    return {
      title,
      heading: surface.heading,
      body: surface.body,
      homeHref: surface.homeHref,
      homeLabel: surface.homeLabel,
      illustration,
      robots: surface.robots,
    };
  }

  async notFoundGoHome(): Promise<void> {
    const before = this.page.url();
    const scope = this.page.locator("main");
    const home = scope.locator('a[href="/"]').first();
    await home.click({ timeout: WebDriver.OPEN_MS });
    await expect.poll(() => this.page.url(), { timeout: WebDriver.WAIT_MS }).not.toBe(before);
  }

  async servedShellMarkers(path: string): Promise<ServedShellMarkers> {
    // Marker presence only: the served document is the crawler-facing
    // layer, so the scenario pins which markers it carries without
    // repeating its copy.
    const url = new URL(path, this.page.url()).toString();
    const response = await this.page.request.get(url, { timeout: WebDriver.WAIT_MS });
    const markup = await response.text();
    const nonEmpty = (pattern: RegExp): boolean => {
      const match = pattern.exec(markup);
      return match !== null && (match[1] ?? "").trim().length > 0;
    };
    const present = (needle: string): boolean => markup.includes(needle);
    return {
      status: response.status(),
      hasTitle: nonEmpty(/<title>([^<]*)<\/title>/),
      hasDescription: nonEmpty(/<meta[^>]*name="description"[^>]*content="([^"]*)"/),
      hasSocial:
        present('property="og:title"') &&
        present('property="og:description"') &&
        present('property="og:image"') &&
        present('name="twitter:card"'),
      hasIcons: present('rel="icon"'),
      hasManifests: present('rel="manifest"'),
      hasPortals: present('id="context-menu-portal"') && present('id="editor-portal"'),
      hasRecorder: present("clarity-tracking") || present("clarity.ms"),
    };
  }

  // --- Desktop-only chat + agent runtime (NEWFRONT-182, RUN-033–036,
  // --- RUN-044–045, RUN-047). Selectors follow the runners area
  // --- behavior observed on the running old app: the side nav hosts an
  // --- overview link plus one chat link per connected runner, the chat
  // --- page pairs a history panel with a composer, and the desktop-only
  // --- seams (built-in section, inline prompts, mode control, runtime
  // --- banner, native bridge) are absent throughout.

  /** URLs logged by the desktop-runtime request spy while it runs. */
  private desktopRuntimeSpyUrlsLogged: string[] = [];

  /** Route patterns the desktop-runtime spy watches (all fall through). */
  private static readonly DESKTOP_RUNTIME_SPY_PATTERNS = [
    "**/api/users/me/ai-assistant/**",
    "**/api/v1/runner/dev-machines/desktop-enroll/**",
    "**/api/runners/chat/**",
  ];

  private desktopRuntimeSideNav(): Locator {
    return this.page.locator("aside", { hasText: "AI Agents" });
  }

  private desktopRuntimeHistoryPanel(): Locator {
    return this.page.locator("aside", { has: this.page.getByText("Chats", { exact: true }) });
  }

  private desktopRuntimeComposerBox(): Locator {
    return this.page.locator('textarea[placeholder*="Message this runner"]');
  }

  private desktopRuntimeThreadColumn(): Locator {
    return this.page.locator("div.flex.min-w-0.flex-1.flex-col.overflow-hidden.px-4");
  }

  private desktopRuntimeListColumn(): Locator {
    return this.desktopRuntimeThreadColumn().locator("div.flex.flex-col.gap-3");
  }

  async desktopRuntimeOpenRunners(workspaceSlug: string): Promise<void> {
    await this.page.goto(`/${workspaceSlug}/runners`);
    await this.page.waitForLoadState("domcontentloaded");
    await this.waitForContent("runners side nav", () =>
      this.desktopRuntimeSideNav().first().waitFor({ timeout: 60_000 })
    );
  }

  async desktopRuntimeRailSectionHeaders(): Promise<string[]> {
    const headers = this.desktopRuntimeSideNav().locator("nav > div.uppercase");
    const total = await headers.count();
    const texts: string[] = [];
    for (let i = 0; i < total; i++) {
      texts.push((await headers.nth(i).innerText()).trim());
    }
    return texts;
  }

  async desktopRuntimeRailChatLinks(): Promise<{ name: string; href: string }[]> {
    const links = this.desktopRuntimeSideNav().locator('a[href*="/chat/"]');
    const total = await links.count();
    const contacts: { name: string; href: string }[] = [];
    for (let i = 0; i < total; i++) {
      const link = links.nth(i);
      contacts.push({
        name: (await link.innerText()).trim(),
        href: (await link.getAttribute("href")) ?? "",
      });
    }
    return contacts;
  }

  async desktopRuntimeOpenChat(workspaceSlug: string, runnerId: string, sessionId?: string): Promise<void> {
    const suffix = sessionId === undefined ? "" : `?sessionId=${encodeURIComponent(sessionId)}`;
    await this.page.goto(`/${workspaceSlug}/runners/chat/${runnerId}${suffix}`);
    await this.page.waitForLoadState("domcontentloaded");
    // Settles on the history heading plus the composer: both render for
    // known and unknown runners. Generous bounds: the first hit compiles
    // the route on the dev server, slow under parallel oracle runs.
    await this.waitForContent("desktop-runtime chat history", () =>
      this.desktopRuntimeHistoryPanel().getByText("Chats", { exact: true }).first().waitFor({ timeout: 60_000 })
    );
    await this.waitForContent("desktop-runtime chat composer", () =>
      this.desktopRuntimeComposerBox().first().waitFor({ timeout: 60_000 })
    );
  }

  async desktopRuntimeApprovalPromptVisible(): Promise<boolean> {
    return (await this.page.getByRole("alertdialog", { name: "Approval requested" }).count()) > 0;
  }

  async desktopRuntimeApprovalModeVisible(): Promise<boolean> {
    return (await this.page.getByRole("group", { name: "Approval mode" }).count()) > 0;
  }

  async desktopRuntimeRuntimeBannerVisible(): Promise<boolean> {
    // The runtime banner pins itself to the viewport bottom-center; toast
    // stacks anchor to a corner instead, so fixed positioning tells them
    // apart without repeating either one's copy.
    return (await this.page.locator('div[role="status"].fixed').count()) > 0;
  }

  async desktopRuntimeIsTauriPresent(): Promise<boolean> {
    return this.page.evaluate(() => "__TAURI__" in window);
  }

  async desktopRuntimeChatBubbles(): Promise<{ role: string; text: string }[]> {
    const column = this.desktopRuntimeListColumn();
    if ((await column.count()) === 0) return [];
    return column.evaluate((element) => {
      const rows: { role: string; text: string }[] = [];
      for (const child of Array.from(element.children)) {
        const node = child as HTMLElement;
        // Activity-strip items are direct rounded children, not bubbles.
        if (node.classList.contains("rounded")) continue;
        const text = (node.innerText ?? "").trim();
        // The bottom scroll anchor carries no text.
        if (text === "") continue;
        if (node.querySelector(".justify-end") !== null) rows.push({ role: "user", text });
        else if (node.querySelector(".justify-start") !== null) rows.push({ role: "assistant", text });
        else rows.push({ role: "status", text });
      }
      return rows;
    });
  }

  async desktopRuntimeStorageKeys(): Promise<{ local: string[]; session: string[] }> {
    return this.page.evaluate(() => ({
      local: Object.keys(window.localStorage),
      session: Object.keys(window.sessionStorage),
    }));
  }

  async desktopRuntimeStartRequestSpy(): Promise<void> {
    this.desktopRuntimeSpyUrlsLogged = [];
    for (const pattern of WebDriver.DESKTOP_RUNTIME_SPY_PATTERNS) {
      // One logging route per watched pattern: records the URL, then falls
      // through so the request still reaches the server.
      await this.page.route(pattern, async (route) => {
        this.desktopRuntimeSpyUrlsLogged.push(route.request().url());
        await route.fallback();
      });
    }
  }

  async desktopRuntimeSpyUrls(): Promise<string[]> {
    return [...this.desktopRuntimeSpyUrlsLogged];
  }

  async desktopRuntimeStopRequestSpy(): Promise<void> {
    for (const pattern of WebDriver.DESKTOP_RUNTIME_SPY_PATTERNS) {
      await this.page.unroute(pattern);
    }
    this.desktopRuntimeSpyUrlsLogged = [];
  }

  // --- Scheduler catalog + definitions (NEWFRONT-184, AGT-001..006, AGT-022).
  // --- Appended; existing methods above are untouched per the shared driver
  // --- contract. Every selector was observed on the running old app: the
  // --- catalog is a plain table, dialogs hang under the modal panel
  // --- wrapper, and the install picker's option panel portals to the body.

  /** The open modal's panel wrapper (the backdrop sibling carries no z-30). */
  private schedulerDialog(): Locator {
    return this.page.locator("div.fixed.inset-0.z-30").last();
  }

  /** The route gate's refusal heading, shared by the schedulers/prompts layouts. */
  private schedulerGateHeading(): Locator {
    return this.page.getByRole("heading", { name: "Oops! You are not authorized to view this page" });
  }

  /** The workspace wrapper's refusal heading, shown above the route layouts. */
  private schedulerNotFoundHeading(): Locator {
    return this.page.getByRole("heading", { name: "Workspace not found" });
  }

  /** One catalog table row, matched on its exact handle cell. */
  private schedulerCatalogRow(handle: string): Locator {
    const exact = new RegExp(`^${handle.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}$`);
    return this.page.locator("tbody tr").filter({ has: this.page.locator("code", { hasText: exact }) });
  }

  private async settleOnCatalogOrGate(): Promise<void> {
    // Three outcomes, no sidebar wait: the not-found surface mounts no
    // sidebar at all, so waiting for chrome would hang the refusal halves.
    await this.page
      .locator("tbody")
      .or(this.schedulerGateHeading())
      .or(this.schedulerNotFoundHeading())
      .first()
      .waitFor({ timeout: WebDriver.OPEN_MS });
  }

  async schedulerOpenCatalog(workspaceSlug: string): Promise<void> {
    await this.page.goto(`/${workspaceSlug}/schedulers`);
    await this.settleOnCatalogOrGate();
  }

  async schedulerCatalogRows(): Promise<SchedulerCatalogRow[]> {
    const rows = this.page.locator("tbody tr");
    const total = await rows.count();
    const out: SchedulerCatalogRow[] = [];
    for (let i = 0; i < total; i++) {
      const row = rows.nth(i);
      // The empty catalog renders one guidance row spanning every column;
      // data-row reads skip it (the empty read below owns it).
      if ((await row.locator("td[colspan]").count()) > 0) continue;
      const cells = row.locator("td");
      const cellText = async (index: number): Promise<string> =>
        (
          (await cells
            .nth(index)
            .innerText()
            .catch(() => "")) ?? ""
        ).trim();
      out.push({
        name: await cellText(0),
        handle: await cellText(1),
        origin: await cellText(2),
        installs: await cellText(3),
        status: await cellText(4),
        updated: await cellText(5),
      });
    }
    return out;
  }

  async schedulerCatalogEmptyVisible(): Promise<boolean> {
    return this.isShown(this.page.getByText("No schedulers in this workspace yet", { exact: false }));
  }

  async schedulerPageTitle(): Promise<string> {
    return this.page.title();
  }

  async schedulerCreateVisible(): Promise<boolean> {
    return this.isShown(this.page.getByRole("button", { name: "New scheduler", exact: true }));
  }

  async schedulerRowActions(handle: string): Promise<string[]> {
    // Scope to the trailing actions cell: the origin/status marks are
    // buttons too, so a row-wide read would report them as actions.
    const buttons = this.schedulerCatalogRow(handle).locator("td").last().getByRole("button");
    const total = await buttons.count();
    const names: string[] = [];
    for (let i = 0; i < total; i++) {
      const text = (
        (await buttons
          .nth(i)
          .innerText()
          .catch(() => "")) ?? ""
      ).trim();
      if (text !== "") names.push(text);
    }
    return names;
  }

  async schedulerOpenCreate(): Promise<void> {
    await this.page.getByRole("button", { name: "New scheduler", exact: true }).click();
    await this.schedulerDialog().getByLabel("Slug").waitFor({ timeout: WebDriver.OPEN_MS });
  }

  async schedulerFillDefinition(input: {
    name?: string;
    handle?: string;
    description?: string;
    prompt?: string;
    color?: string;
  }): Promise<void> {
    const dialog = this.schedulerDialog();
    if (input.name !== undefined) await dialog.getByLabel("Name").fill(input.name);
    if (input.handle !== undefined) await dialog.getByLabel("Slug").fill(input.handle);
    if (input.description !== undefined) await dialog.getByLabel("Description").fill(input.description);
    if (input.prompt !== undefined) await dialog.getByLabel("Prompt").fill(input.prompt);
    if (input.color !== undefined) {
      await dialog.getByRole("button", { name: `Color ${input.color.toLowerCase()}` }).click();
    }
  }

  async schedulerSetDefinitionEnabled(enabled: boolean): Promise<void> {
    const toggle = this.schedulerDialog().getByRole("switch");
    const current = (await toggle.getAttribute("aria-checked")) === "true";
    if (current !== enabled) await toggle.click();
  }

  async schedulerDefinitionValues(): Promise<SchedulerDefinitionValues> {
    const dialog = this.schedulerDialog();
    const pressed = dialog.locator('button[aria-pressed="true"]');
    const label = (
      (await pressed
        .first()
        .getAttribute("aria-label")
        .catch(() => null)) ?? ""
    ).trim();
    const toggle = dialog.getByRole("switch");
    return {
      name: await dialog.getByLabel("Name").inputValue(),
      handle: await dialog.getByLabel("Slug").inputValue(),
      description: await dialog.getByLabel("Description").inputValue(),
      prompt: await dialog.getByLabel("Prompt").inputValue(),
      color: label.replace(/^Color\s+/i, ""),
      enabled: (await toggle.getAttribute("aria-checked")) === "true",
    };
  }

  async schedulerDefinitionHandleLocked(): Promise<boolean> {
    return this.schedulerDialog().getByLabel("Slug").isDisabled();
  }

  async schedulerSubmitDefinition(): Promise<void> {
    // No close wait: a rejected submit keeps the dialog open with an error
    // toast, and the scenario asserts that half too.
    await this.schedulerDialog().locator('button[type="submit"]').click();
  }

  async schedulerDefinitionOpen(): Promise<boolean> {
    return (await this.schedulerDialog().getByLabel("Slug").count()) > 0;
  }

  async schedulerDefinitionErrors(): Promise<string[]> {
    // Immediate read: callers poll while a submit round-trips.
    const errors = this.schedulerDialog().locator(".text-danger-primary");
    const total = await errors.count();
    const texts: string[] = [];
    for (let i = 0; i < total; i++) {
      if (
        !(await errors
          .nth(i)
          .isVisible()
          .catch(() => false))
      )
        continue;
      const text = (
        (await errors
          .nth(i)
          .innerText()
          .catch(() => "")) ?? ""
      ).trim();
      if (text !== "") texts.push(text);
    }
    return texts;
  }

  async schedulerCloseDefinition(): Promise<void> {
    const dialog = this.schedulerDialog();
    await dialog.getByRole("button", { name: "Cancel", exact: true }).click();
    await dialog.getByLabel("Slug").waitFor({ state: "detached", timeout: WebDriver.OPEN_MS });
  }

  async schedulerOpenEdit(handle: string): Promise<void> {
    await this.schedulerCatalogRow(handle).getByRole("button", { name: "Edit", exact: true }).click();
    const slugField = this.schedulerDialog().getByLabel("Slug");
    await slugField.waitFor({ timeout: WebDriver.OPEN_MS });
    // The dialog pre-fills from the row; settle on the handle so later
    // value reads never catch the form mid-reset.
    await expect(slugField).toHaveValue(handle, { timeout: WebDriver.OPEN_MS });
  }

  async schedulerOpenDelete(handle: string): Promise<void> {
    await this.schedulerCatalogRow(handle).getByRole("button", { name: "Delete", exact: true }).click();
    await this.schedulerDialog()
      .getByRole("heading", { name: "Delete scheduler?", exact: true })
      .waitFor({ timeout: WebDriver.OPEN_MS });
  }

  async schedulerDeleteDialogText(): Promise<string> {
    return (
      (await this.schedulerDialog()
        .innerText()
        .catch(() => "")) ?? ""
    ).trim();
  }

  async schedulerConfirmDelete(): Promise<void> {
    const dialog = this.schedulerDialog();
    await dialog.getByRole("button", { name: "Delete", exact: true }).click();
    // Either the dialog closes (deleted) or a failure notice shows while it
    // stays open. Race the two: waiting out the full close timeout first
    // would let the notice auto-dismiss before the scenario reads it.
    const heading = dialog.getByRole("heading", { name: "Delete scheduler?", exact: true });
    const deadline = Date.now() + WebDriver.OPEN_MS;
    for (;;) {
      if ((await heading.count()) === 0) return;
      const toast = await this.rulesLastToast();
      if (toast !== null && toast.title === "Something went wrong") {
        await expect
          .poll(() => this.rulesLastToast(), { timeout: WebDriver.OPEN_MS })
          .toEqual({ title: "Something went wrong", message: expect.any(String) });
        return;
      }
      if (Date.now() > deadline) throw new Error("[parity] delete confirmation settled on neither outcome.");
      await this.page.waitForTimeout(500);
    }
  }

  async schedulerDeleteOpen(): Promise<boolean> {
    return (await this.schedulerDialog().getByRole("heading", { name: "Delete scheduler?", exact: true }).count()) > 0;
  }

  async schedulerCancelDelete(): Promise<void> {
    const dialog = this.schedulerDialog();
    await dialog.getByRole("button", { name: "Cancel", exact: true }).click();
    await dialog
      .getByRole("heading", { name: "Delete scheduler?", exact: true })
      .waitFor({ state: "detached", timeout: WebDriver.OPEN_MS });
  }

  async schedulerOpenProjectSchedulers(workspaceSlug: string, projectId: string): Promise<void> {
    await this.page.goto(`/${workspaceSlug}/projects/${projectId}/schedulers/list`);
    await this.page.locator("#main-sidebar").waitFor({ state: "attached", timeout: WebDriver.OPEN_MS });
    await this.page.getByRole("button", { name: "New Scheduler", exact: true }).waitFor({ timeout: WebDriver.OPEN_MS });
  }

  async schedulerOpenProjectCreate(): Promise<void> {
    await this.page.getByRole("button", { name: "New Scheduler", exact: true }).click();
    const dialog = this.schedulerDialog();
    // The modal opens on the install tab (no derivation fields mounted), so
    // settle on the title first, flip to the create tab, then wait for the
    // handle field. Tabs render for workspace admins; the derivation halves
    // always run as the owner.
    await dialog.getByText("New Scheduler", { exact: true }).waitFor({ timeout: WebDriver.OPEN_MS });
    await dialog.getByRole("tab", { name: "Create new", exact: true }).click({ timeout: WebDriver.OPEN_MS });
    await dialog.getByLabel("Slug").waitFor({ timeout: WebDriver.OPEN_MS });
  }

  async schedulerProjectCreateSubmit(): Promise<void> {
    await this.schedulerDialog().locator('button[type="submit"]').click();
  }

  async schedulerProjectCreateFillName(name: string): Promise<void> {
    await this.schedulerDialog().getByLabel("Name").fill(name);
  }

  async schedulerProjectCreateHandleValue(): Promise<string> {
    return this.schedulerDialog().getByLabel("Slug").inputValue();
  }

  async schedulerProjectCreateFillHandle(handle: string): Promise<void> {
    await this.schedulerDialog().getByLabel("Slug").fill(handle);
  }

  async schedulerProjectCreateErrors(): Promise<string[]> {
    const errors = this.schedulerDialog().locator(".text-danger-primary");
    const total = await errors.count();
    const texts: string[] = [];
    for (let i = 0; i < total; i++) {
      if (
        !(await errors
          .nth(i)
          .isVisible()
          .catch(() => false))
      )
        continue;
      const text = (
        (await errors
          .nth(i)
          .innerText()
          .catch(() => "")) ?? ""
      ).trim();
      if (text !== "") texts.push(text);
    }
    return texts;
  }

  async schedulerCloseProjectCreate(): Promise<void> {
    const dialog = this.schedulerDialog();
    await dialog.getByRole("button", { name: "Cancel", exact: true }).click();
    await dialog.getByLabel("Slug").waitFor({ state: "detached", timeout: WebDriver.OPEN_MS });
  }

  async schedulerOpenInstall(handle: string): Promise<void> {
    await this.schedulerCatalogRow(handle).getByRole("button", { name: "Install", exact: true }).click();
    const dialog = this.schedulerDialog();
    await dialog.locator('button[aria-haspopup="listbox"]').waitFor({ timeout: WebDriver.OPEN_MS });
  }

  /** The picker's option panel (portaled to the body, outside the dialog). */
  private schedulerInstallPanel(): Locator {
    return this.page.locator("div[data-prevent-outside-click]");
  }

  private async schedulerEnsurePickerOpen(): Promise<Locator> {
    const dialog = this.schedulerDialog();
    const search = this.page.getByPlaceholder("Search projects…");
    if (
      (await search.count()) === 0 ||
      !(await search
        .first()
        .isVisible()
        .catch(() => false))
    ) {
      await dialog.locator('button[aria-haspopup="listbox"]').click();
      await search.waitFor({ timeout: WebDriver.OPEN_MS });
    }
    return this.schedulerInstallPanel();
  }

  async schedulerInstallPickerOptions(): Promise<SchedulerInstallOption[]> {
    const panel = await this.schedulerEnsurePickerOpen();
    // Detection shows skeleton rows first; settle on real options (or the
    // no-match guidance) so the read never catches the loader.
    await panel
      .locator("li")
      .or(panel.getByText("No projects match your search."))
      .first()
      .waitFor({ timeout: WebDriver.OPEN_MS });
    const items = panel.locator("li");
    const total = await items.count();
    const out: SchedulerInstallOption[] = [];
    for (let i = 0; i < total; i++) {
      const item = items.nth(i);
      const checkbox = item.getByRole("checkbox");
      const locked = await checkbox.isDisabled().catch(() => false);
      const checked = locked ? true : await checkbox.isChecked().catch(() => false);
      const lines = ((await item.innerText().catch(() => "")) ?? "")
        .split("\n")
        .map((line) => line.trim())
        .filter((line) => line.length > 0);
      const textLines = locked && lines[lines.length - 1] === "Installed" ? lines.slice(0, -1) : lines;
      out.push({
        name: textLines[0] ?? "",
        identifier: textLines.length > 1 ? (textLines[1] ?? null) : null,
        checked,
        locked,
      });
    }
    return out;
  }

  async schedulerInstallSearch(query: string): Promise<void> {
    await this.schedulerEnsurePickerOpen();
    await this.page.getByPlaceholder("Search projects…").fill(query);
  }

  async schedulerInstallToggleSelectAll(): Promise<void> {
    const panel = await this.schedulerEnsurePickerOpen();
    const toggle = panel
      .getByRole("button", { name: "Select all", exact: true })
      .or(panel.getByRole("button", { name: "Clear selection", exact: true }));
    await toggle.first().click();
  }

  async schedulerInstallToggleProject(name: string): Promise<void> {
    const panel = await this.schedulerEnsurePickerOpen();
    const item = panel.locator("li").filter({ hasText: name });
    await item.getByRole("checkbox").click();
  }

  async schedulerInstallSelectedSummary(): Promise<string> {
    const toggle = this.schedulerDialog().locator('button[aria-haspopup="listbox"]');
    return ((await toggle.innerText().catch(() => "")) ?? "").trim();
  }

  async schedulerInstallSubmit(): Promise<void> {
    // No close wait: failed targets keep the dialog open for retry.
    await this.schedulerDialog().locator('button[type="submit"]').click();
  }

  async schedulerInstallOpen(): Promise<boolean> {
    return (await this.schedulerDialog().locator('button[aria-haspopup="listbox"]').count()) > 0;
  }

  async schedulerCloseInstall(): Promise<void> {
    const dialog = this.schedulerDialog();
    await dialog.getByRole("button", { name: "Cancel", exact: true }).click();
    await dialog.locator('button[aria-haspopup="listbox"]').waitFor({ state: "detached", timeout: WebDriver.OPEN_MS });
  }

  async schedulerVisibleToasts(): Promise<{ title: string; message: string }[]> {
    // Same toast roots as rulesLastToast, but every visible one oldest
    // first so partitioned outcomes (one success plus one failure) assert
    // together. Immediate read: toasts auto-dismiss, so callers read
    // right after the submit that raised them.
    const roots = this.page.locator("div.absolute.right-3.bottom-3");
    const total = await roots.count();
    const out: { title: string; message: string }[] = [];
    for (let i = 0; i < total; i++) {
      if (
        !(await roots
          .nth(i)
          .isVisible()
          .catch(() => false))
      )
        continue;
      const text = (
        (await roots
          .nth(i)
          .innerText()
          .catch(() => "")) ?? ""
      ).trim();
      if (text === "") continue;
      const lines = text
        .split("\n")
        .map((line) => line.trim())
        .filter((line) => line.length > 0);
      out.push({ title: lines[0] ?? "", message: lines.slice(1).join(" ") });
    }
    return out;
  }

  async schedulerOpenPrompts(workspaceSlug: string): Promise<void> {
    await this.page.goto(`/${workspaceSlug}/prompts`);
    await this.page
      .getByRole("heading", { name: "Prompts", exact: true })
      .or(this.schedulerGateHeading())
      .or(this.schedulerNotFoundHeading())
      .first()
      .waitFor({ timeout: WebDriver.OPEN_MS });
  }

  async schedulerNotAuthorizedVisible(): Promise<boolean> {
    return this.isShown(this.schedulerGateHeading());
  }

  async schedulerWorkspaceNotFoundVisible(): Promise<boolean> {
    return this.isShown(this.schedulerNotFoundHeading());
  }

  async schedulerShellCount(): Promise<number> {
    return this.page.locator("#main-sidebar").count();
  }

  // --- Dev machines, runner detail, agent activity (NEWFRONT-183, RUN-037–043) ---
  // --- Every selector below was observed on the running old app: the
  // --- machines table carries six cells per data row (states span one),
  // --- badges render as buttons, and the confirm modals are headless
  // --- dialogs with a heading, a warning body and cancel/confirm.

  /** Matches any dev-machines REST call the page makes (list, rotate, revoke, delete). */
  private static readonly DEV_MACHINES_PATTERN = "**/api/runners/dev-machines/**";

  private devMachinesDeleteSpyState: {
    urls: string[];
    handler: (route: Parameters<Parameters<Page["route"]>[1]>[0]) => Promise<void>;
  } | null = null;

  private devMachinesTable(): Locator {
    return this.page.locator("table", { has: this.page.getByRole("columnheader", { name: "Last heartbeat" }) }).first();
  }

  async devMachinesOpen(workspaceSlug: string): Promise<void> {
    await this.page.goto(`/${workspaceSlug}/ai-dev-machines`);
    await this.waitForContent("dev-machines table", () =>
      this.devMachinesTable().waitFor({ timeout: WebDriver.WAIT_MS })
    );
  }

  async devMachinesRows(): Promise<DevMachineRow[]> {
    const table = this.devMachinesTable();
    if ((await table.count()) === 0) return [];
    const rows = await table.evaluate((root) => {
      const out: {
        name: string;
        subline: string;
        status: string;
        runners: string;
        lastSeen: string;
        lastHeartbeat: string;
        actions: string[];
      }[] = [];
      for (const tr of root.querySelectorAll("tbody tr")) {
        const cells = tr.querySelectorAll(":scope > td");
        if (cells.length < 6) continue;
        const divs = cells[0]?.querySelectorAll(":scope > div") ?? [];
        const buttons = cells[5]?.querySelectorAll("button") ?? [];
        out.push({
          name: divs[0]?.textContent?.trim() ?? "",
          subline: divs[1]?.textContent?.trim() ?? "",
          status: cells[1]?.querySelector("button")?.textContent?.trim() ?? "",
          runners: cells[2]?.textContent?.trim().replace(/\s+/g, " ") ?? "",
          lastSeen: cells[3]?.textContent?.trim() ?? "",
          lastHeartbeat: cells[4]?.textContent?.trim() ?? "",
          actions: [...buttons].map((button) => (button.textContent ?? "").trim()).filter((label) => label !== ""),
        });
      }
      return out;
    });
    return rows;
  }

  async devMachinesRowByName(name: string): Promise<DevMachineRow | null> {
    const rows = await this.devMachinesRows();
    return rows.find((row) => row.name === name) ?? null;
  }

  private async devMachinesStateText(): Promise<string> {
    const table = this.devMachinesTable();
    if ((await table.count()) === 0) return "";
    return ((await table.locator("tbody").first().innerText()) ?? "").trim();
  }

  async devMachinesEmptyVisible(): Promise<boolean> {
    return (await this.devMachinesStateText()).includes("No dev machines");
  }

  async devMachinesLoadingVisible(): Promise<boolean> {
    return (await this.devMachinesStateText()).includes("Loading dev machines");
  }

  async devMachinesErrorVisible(): Promise<boolean> {
    return (await this.devMachinesStateText()).includes("Could not load dev machines");
  }

  async devMachinesStubListOnce(rows: unknown[]): Promise<void> {
    // Armed-flag shaping, not times:1: a continued request still consumes
    // a times slot, so an unrelated poll could disarm the stub.
    let armed = true;
    await this.page.route(WebDriver.DEV_MACHINES_PATTERN, async (route) => {
      if (armed && route.request().method() === "GET") {
        armed = false;
        await route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify(rows) });
        return;
      }
      await route.continue();
    });
  }

  async devMachinesFailListOnce(): Promise<void> {
    let armed = true;
    await this.page.route(WebDriver.DEV_MACHINES_PATTERN, async (route) => {
      if (armed && route.request().method() === "GET") {
        armed = false;
        await route.fulfill({
          status: 500,
          contentType: "application/json",
          body: JSON.stringify({ error: "parity list failure" }),
        });
        return;
      }
      await route.continue();
    });
  }

  async devMachinesDelayListOnce(ms: number): Promise<void> {
    let armed = true;
    await this.page.route(WebDriver.DEV_MACHINES_PATTERN, async (route) => {
      if (armed && route.request().method() === "GET") {
        armed = false;
        await this.page.waitForTimeout(ms);
      }
      await route.continue();
    });
  }

  async devMachinesListPollCount(windowMs: number): Promise<number> {
    let calls = 0;
    const pattern = WebDriver.DEV_MACHINES_PATTERN;
    const counter = async (route: Parameters<Parameters<Page["route"]>[1]>[0]) => {
      if (route.request().method() === "GET") calls += 1;
      await route.continue();
    };
    await this.page.route(pattern, counter);
    try {
      await this.page.waitForTimeout(windowMs);
      return calls;
    } finally {
      await this.page.unroute(pattern, counter).catch(() => undefined);
    }
  }

  private devMachinesDataRow(name: string): Locator {
    return this.devMachinesTable()
      .locator("tbody tr")
      .filter({ has: this.page.locator("td:nth-child(6)") })
      .filter({ hasText: name })
      .first();
  }

  private async devMachinesOpenAction(name: string, action: string): Promise<void> {
    const row = this.devMachinesDataRow(name);
    await row.getByRole("button", { name: action, exact: true }).click({ timeout: WebDriver.WAIT_MS });
    await expect.poll(() => this.devMachinesModalVisible(), { timeout: WebDriver.WAIT_MS }).toBe(true);
  }

  async devMachinesOpenRotate(name: string): Promise<void> {
    await this.devMachinesOpenAction(name, "Rotate");
  }

  async devMachinesOpenRevoke(name: string): Promise<void> {
    await this.devMachinesOpenAction(name, "Revoke");
  }

  async devMachinesOpenDelete(name: string): Promise<void> {
    await this.devMachinesOpenAction(name, "Delete");
  }

  private devMachinesDialog(): Locator {
    return this.appDialogs()
      .filter({ has: this.page.locator("h3") })
      .first();
  }

  async devMachinesModal(): Promise<DevMachineModal | null> {
    const dialog = this.devMachinesDialog();
    if ((await dialog.count()) === 0) return null;
    const title = ((await dialog.locator("h3").first().innerText()) ?? "").trim();
    const body = (
      (await dialog.locator("h3").first().locator("xpath=following-sibling::div[1]").innerText()) ?? ""
    ).trim();
    const buttons = dialog.getByRole("button");
    if ((await buttons.count()) === 0) return null;
    const confirmLabel = ((await buttons.last().innerText()) ?? "").trim();
    return { title, body, confirmLabel };
  }

  async devMachinesModalVisible(): Promise<boolean> {
    return (await this.devMachinesDialog().count()) > 0;
  }

  async devMachinesModalConfirm(): Promise<void> {
    const dialog = this.devMachinesDialog();
    await dialog.getByRole("button").last().click({ timeout: WebDriver.WAIT_MS });
  }

  async devMachinesModalCancel(): Promise<void> {
    const dialog = this.devMachinesDialog();
    const cancel = dialog.getByRole("button", { name: "Cancel", exact: true });
    if ((await cancel.count()) > 0) {
      await cancel.first().click({ timeout: WebDriver.WAIT_MS });
    }
  }

  async devMachinesModalPressEscape(): Promise<void> {
    await this.page.keyboard.press("Escape");
    await this.page.waitForTimeout(300);
  }

  private static devMachinesIsActionRequest(url: string, method: string): boolean {
    if (method === "DELETE") return true;
    return method === "POST" && (url.includes("/rotate/") || url.includes("/revoke/"));
  }

  async devMachinesDelayActionOnce(ms: number): Promise<void> {
    let armed = true;
    await this.page.route(WebDriver.DEV_MACHINES_PATTERN, async (route) => {
      const request = route.request();
      if (armed && WebDriver.devMachinesIsActionRequest(request.url(), request.method())) {
        armed = false;
        await this.page.waitForTimeout(ms);
      }
      await route.continue();
    });
  }

  async devMachinesFailActionOnce(): Promise<void> {
    let armed = true;
    await this.page.route(WebDriver.DEV_MACHINES_PATTERN, async (route) => {
      const request = route.request();
      if (armed && WebDriver.devMachinesIsActionRequest(request.url(), request.method())) {
        armed = false;
        await route.fulfill({
          status: 500,
          contentType: "application/json",
          body: JSON.stringify({ error: "parity action failure" }),
        });
        return;
      }
      await route.continue();
    });
  }

  async devMachinesLastToast(): Promise<string | null> {
    return this.lastToast();
  }

  async devMachinesDeleteSpyStart(): Promise<void> {
    const urls: string[] = [];
    const handler = async (route: Parameters<Parameters<Page["route"]>[1]>[0]) => {
      if (route.request().method() === "DELETE") urls.push(route.request().url());
      await route.continue();
    };
    this.devMachinesDeleteSpyState = { urls, handler };
    await this.page.route(WebDriver.DEV_MACHINES_PATTERN, handler);
  }

  async devMachinesDeleteSpyUrls(): Promise<string[]> {
    return [...(this.devMachinesDeleteSpyState?.urls ?? [])];
  }

  async devMachinesDeleteSpyStop(): Promise<void> {
    const state = this.devMachinesDeleteSpyState;
    this.devMachinesDeleteSpyState = null;
    if (state !== null) {
      await this.page.unroute(WebDriver.DEV_MACHINES_PATTERN, state.handler).catch(() => undefined);
    }
  }

  private devMachinesInstallSection(): Locator {
    return this.page
      .locator("section", { has: this.page.getByRole("heading", { name: "Install the pidash CLI" }) })
      .first();
  }

  async devMachinesInstallCards(): Promise<DevMachineInstallCard[]> {
    const section = this.devMachinesInstallSection();
    if ((await section.count()) === 0) return [];
    return section.evaluate((root) => {
      const cards: { label: string; command: string; downloadHref: string | null }[] = [];
      for (const pre of root.querySelectorAll("pre")) {
        const card = pre.parentElement;
        if (card === null) continue;
        cards.push({
          label: card.querySelector("span")?.textContent?.trim() ?? "",
          command: pre.textContent ?? "",
          downloadHref: card.querySelector("a[href]")?.getAttribute("href") ?? null,
        });
      }
      return cards;
    });
  }

  private async devMachinesInstallCardRoot(label: string): Promise<Locator | null> {
    const section = this.devMachinesInstallSection();
    const pres = section.locator("pre");
    const count = await pres.count();
    for (let index = 0; index < count; index += 1) {
      const root = pres.nth(index).locator("xpath=..");
      // textContent, not innerText: the label renders uppercased by style.
      const text = (
        (await root
          .locator("span")
          .first()
          .textContent()
          .catch(() => "")) ?? ""
      ).trim();
      if (text === label) return root;
    }
    return null;
  }

  async devMachinesInstallCopy(label: string): Promise<void> {
    // The page writes through the async clipboard API, which headless
    // Chromium denies without an explicit grant.
    await this.page.context().grantPermissions(["clipboard-read", "clipboard-write"]);
    const root = await this.devMachinesInstallCardRoot(label);
    if (root === null) throw new Error(`[parity] no install card labelled ${label}.`);
    await root.getByRole("button").first().click({ timeout: WebDriver.WAIT_MS });
  }

  async devMachinesInstallCopyState(label: string): Promise<string | null> {
    const root = await this.devMachinesInstallCardRoot(label);
    if (root === null) return null;
    const button = root.getByRole("button").first();
    if ((await button.count()) === 0) return null;
    return (((await button.innerText()) ?? "").trim() || null) as string | null;
  }

  async devMachinesInstallPrereq(): Promise<string | null> {
    const section = this.devMachinesInstallSection();
    const note = section.locator("p", { hasText: "doctor" }).first();
    if ((await note.count()) === 0) return null;
    return (((await note.innerText()) ?? "").trim() || null) as string | null;
  }

  async devMachinesInstallBreakClipboard(): Promise<void> {
    await this.page.evaluate(() => {
      window.Clipboard.prototype.writeText = () => Promise.reject(new Error("parity: clipboard blocked"));
    });
  }

  async devMachinesReadClipboard(): Promise<string> {
    return this.readClipboard();
  }

  async runnerDetailOpen(workspaceSlug: string, runnerId: string, projectId?: string): Promise<void> {
    const base =
      projectId !== undefined ? `/${workspaceSlug}/projects/${projectId}/runners` : `/${workspaceSlug}/runners`;
    await this.page.goto(`${base}/detail/${runnerId}`);
    const header = this.page.locator("h1").first();
    const error = this.page.getByText("Failed to load runner").first();
    await this.waitForContent("runner detail", () =>
      expect(header.or(error)).toBeVisible({ timeout: WebDriver.WAIT_MS })
    );
  }

  async runnerDetailState(): Promise<"loaded" | "loading" | "error"> {
    const header = this.page.locator("h1").first();
    if ((await header.count()) > 0 && (await header.isVisible().catch(() => false))) return "loaded";
    if ((await this.page.getByText("Failed to load runner").count()) > 0) return "error";
    return "loading";
  }

  async runnerDetailHeader(): Promise<{ name: string; status: string } | null> {
    const header = this.page.locator("h1").first();
    if ((await header.count()) === 0) return null;
    const badge = header.locator("xpath=ancestor::section[1]").getByRole("button").first();
    if ((await badge.count()) === 0) return null;
    return {
      name: ((await header.innerText()) ?? "").trim(),
      status: ((await badge.innerText()) ?? "").trim(),
    };
  }

  async runnerDetailMeta(): Promise<{ label: string; value: string }[]> {
    const section = this.page.locator("section", { hasText: "Metadata" }).first();
    if ((await section.count()) === 0) return [];
    const list = section.locator("dl").first();
    if ((await list.count()) === 0) return [];
    return list.evaluate((root) => {
      const out: { label: string; value: string }[] = [];
      for (const term of root.querySelectorAll(":scope > dt")) {
        const next = term.nextElementSibling;
        if (next === null || next.tagName !== "DD") continue;
        out.push({
          label: (term.textContent ?? "").trim(),
          value: (next.textContent ?? "").trim().replace(/\s+/g, " "),
        });
      }
      return out;
    });
  }

  async runnerDetailBackHref(): Promise<string | null> {
    const link = this.page.getByRole("link", { name: "Back to runners" }).first();
    if ((await link.count()) === 0) return null;
    return link.getAttribute("href");
  }

  async runnerDetailOpenChat(): Promise<void> {
    await this.page.getByRole("button", { name: "Open chat" }).first().click({ timeout: WebDriver.WAIT_MS });
    await this.page.waitForURL("**/chat/**", { timeout: WebDriver.WAIT_MS });
  }

  async runnerDetailPollCount(runnerId: string, windowMs: number): Promise<number> {
    let calls = 0;
    const pattern = `**/api/runners/${runnerId}/*`;
    const counter = async (route: Parameters<Parameters<Page["route"]>[1]>[0]) => {
      if (route.request().method() === "GET") calls += 1;
      await route.continue();
    };
    await this.page.route(pattern, counter);
    try {
      await this.page.waitForTimeout(windowMs);
      return calls;
    } finally {
      await this.page.unroute(pattern, counter).catch(() => undefined);
    }
  }

  private static runnerDetailIsShapedRequest(url: string, method: string): boolean {
    // Detail shape only (/api/runners/<id>/): the side nav's list GET and
    // the chat endpoints share the prefix and must never consume the stub.
    if (method !== "GET") return false;
    let path: string;
    try {
      path = new URL(url).pathname;
    } catch {
      return false;
    }
    const segment = /^\/api\/runners\/([^/]+)\/?$/.exec(path)?.[1];
    return segment !== undefined && segment !== "dev-machines" && segment !== "chat";
  }

  async runnerDetailDelayOnce(ms: number): Promise<void> {
    let armed = true;
    await this.page.route("**/api/runners/**", async (route) => {
      const request = route.request();
      if (armed && WebDriver.runnerDetailIsShapedRequest(request.url(), request.method())) {
        armed = false;
        await this.page.waitForTimeout(ms);
      }
      await route.continue();
    });
  }

  async runnerDetailFailOnce(): Promise<void> {
    let armed = true;
    await this.page.route("**/api/runners/**", async (route) => {
      const request = route.request();
      if (armed && WebDriver.runnerDetailIsShapedRequest(request.url(), request.method())) {
        armed = false;
        await route.fulfill({
          status: 500,
          contentType: "application/json",
          body: JSON.stringify({ error: "parity detail failure" }),
        });
        return;
      }
      await route.continue();
    });
  }

  private runnerActivityHeading(): Locator {
    return this.page.getByRole("heading", { name: "Agent", exact: true }).first();
  }

  async runnerActivityBadge(): Promise<string | null> {
    const heading = this.runnerActivityHeading();
    if ((await heading.count()) === 0) return null;
    const badge = heading.locator("xpath=..").getByRole("button").first();
    if ((await badge.count()) === 0) return null;
    return (((await badge.innerText()) ?? "").trim() || null) as string | null;
  }

  private runnerActivityGrid(): Locator {
    return this.runnerActivityHeading().locator("xpath=ancestor::div[2]/following-sibling::dl[1]");
  }

  async runnerActivityTelemetry(): Promise<{ label: string; value: string }[]> {
    const grid = this.runnerActivityGrid();
    if ((await grid.count()) === 0) return [];
    return grid.evaluate((root) => {
      const out: { label: string; value: string }[] = [];
      for (const term of root.querySelectorAll(":scope > dt")) {
        const next = term.nextElementSibling;
        if (next === null || next.tagName !== "DD") continue;
        out.push({
          label: (term.textContent ?? "").trim(),
          value: (next.textContent ?? "").trim().replace(/\s+/g, " "),
        });
      }
      return out;
    });
  }

  private async runnerActivityLastActivity(): Promise<string | null> {
    const grid = this.runnerActivityGrid();
    if ((await grid.count()) === 0) return null;
    const value = grid.locator("dt", { hasText: "Last activity" }).locator("xpath=following-sibling::dd[1]");
    if ((await value.count()) === 0) return null;
    return (((await value.first().innerText()) ?? "").trim() || null) as string | null;
  }

  async runnerActivityAgingObserved(): Promise<boolean> {
    // Refetches aborted: any label advance comes from the panel's own
    // tick, not from a fresh server payload.
    const pattern = "**/api/runners/**";
    const blocker = async (route: Parameters<Parameters<Page["route"]>[1]>[0]) => {
      const request = route.request();
      if (request.method() === "GET" && !request.url().includes("dev-machines")) {
        await route.abort();
      } else {
        await route.continue();
      }
    };
    await this.page.route(pattern, blocker);
    try {
      const first = await this.runnerActivityLastActivity();
      if (first === null) return false;
      await expect.poll(() => this.runnerActivityLastActivity(), { timeout: 25_000 }).not.toBe(first);
      return true;
    } catch {
      return false;
    } finally {
      await this.page.unroute(pattern, blocker).catch(() => undefined);
    }
  }

  // --- Project scheduler installs + detail + calendar (NEWFRONT-185).

  /** One project install-list row, matched on its exact handle cell. */
  private schedulerProjectRow(handle: string): Locator {
    const exact = new RegExp(`^${handle.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}$`);
    return this.page.locator("tbody tr").filter({ has: this.page.locator("code", { hasText: exact }) });
  }

  private async schedulerModalErrors(dialog: Locator): Promise<string[]> {
    // Immediate read: callers poll while a submit round-trips.
    const errors = dialog.locator(".text-danger-primary");
    const total = await errors.count();
    const texts: string[] = [];
    for (let i = 0; i < total; i++) {
      if (
        !(await errors
          .nth(i)
          .isVisible()
          .catch(() => false))
      )
        continue;
      const text = (
        (await errors
          .nth(i)
          .innerText()
          .catch(() => "")) ?? ""
      ).trim();
      if (text !== "") texts.push(text);
    }
    return texts;
  }

  private async schedulerScheduleValues(dialog: Locator): Promise<SchedulerScheduleValues> {
    const toggle = dialog.getByRole("switch");
    return {
      dtstart: await dialog.getByLabel("Starts at").inputValue(),
      tzid: await dialog.getByLabel("Time zone").inputValue(),
      rrule: await dialog.getByLabel("Recurrence (RRULE)").inputValue(),
      extraContext: await dialog.getByLabel("Project context (optional)").inputValue(),
      enabled: (await toggle.getAttribute("aria-checked")) === "true",
    };
  }

  private async schedulerFillSchedule(
    dialog: Locator,
    input: { dtstart?: string; tzid?: string; rrule?: string; extraContext?: string }
  ): Promise<void> {
    if (input.dtstart !== undefined) await dialog.getByLabel("Starts at").fill(input.dtstart);
    if (input.tzid !== undefined) await dialog.getByLabel("Time zone").selectOption(input.tzid);
    if (input.rrule !== undefined) await dialog.getByLabel("Recurrence (RRULE)").fill(input.rrule);
    if (input.extraContext !== undefined)
      await dialog.getByLabel("Project context (optional)").fill(input.extraContext);
  }

  private async schedulerHumanizer(dialog: Locator): Promise<string> {
    // The live sentence sits in the tinted span of the help line under the
    // RRULE field (the line also carries static RFC guidance text).
    const line = dialog
      .locator('label:has-text("Recurrence")')
      .locator("xpath=following-sibling::p[1]")
      .locator("span.text-primary");
    await line.waitFor({ timeout: WebDriver.OPEN_MS });
    return ((await line.innerText().catch(() => "")) ?? "").trim();
  }

  private async schedulerSetModalEnabled(dialog: Locator, enabled: boolean): Promise<void> {
    const toggle = dialog.getByRole("switch");
    const current = (await toggle.getAttribute("aria-checked")) === "true";
    if (current !== enabled) await toggle.click();
  }

  async schedulerOpenProjectList(workspaceSlug: string, projectId: string): Promise<void> {
    await this.page.goto(`/${workspaceSlug}/projects/${projectId}/schedulers/list`);
    await this.page.locator("#main-sidebar").waitFor({ state: "attached", timeout: WebDriver.OPEN_MS });
    await this.page
      .getByRole("columnheader", { name: "Scheduler", exact: true })
      .waitFor({ timeout: WebDriver.OPEN_MS });
  }

  async schedulerProjectRows(): Promise<SchedulerProjectRow[]> {
    const rows = this.page.locator("tbody tr");
    const total = await rows.count();
    const out: SchedulerProjectRow[] = [];
    for (let i = 0; i < total; i++) {
      const row = rows.nth(i);
      // The empty list renders one guidance row spanning every column;
      // data-row reads skip it (the empty read below owns it).
      if ((await row.locator("td[colspan]").count()) > 0) continue;
      const cells = row.locator("td");
      const cellText = async (index: number): Promise<string> =>
        (
          (await cells
            .nth(index)
            .innerText()
            .catch(() => "")) ?? ""
        ).trim();
      const first = cells.nth(0);
      const scheduleCell = cells.nth(1);
      out.push({
        name: (
          (await first
            .locator("div")
            .first()
            .innerText()
            .catch(() => "")) ?? ""
        ).trim(),
        handle: (
          (await first
            .locator("code")
            .innerText()
            .catch(() => "")) ?? ""
        ).trim(),
        schedule: await cellText(1),
        scheduleTitle: (await scheduleCell.getAttribute("title").catch(() => null)) ?? "",
        nextRun: await cellText(2),
        lastRun: await cellText(3),
        status: await cellText(4),
        updated: await cellText(5),
        manageVisible: (await cells.nth(6).getByRole("button").count()) > 0,
      });
    }
    return out;
  }

  async schedulerProjectEmptyVisible(): Promise<boolean> {
    return this.isShown(this.page.getByText("No schedulers installed on this project yet", { exact: false }));
  }

  async schedulerProjectNewVisible(): Promise<boolean> {
    return this.isShown(this.page.getByRole("button", { name: "New Scheduler", exact: true }));
  }

  async schedulerProjectRowOpen(handle: string): Promise<void> {
    await this.schedulerProjectRow(handle).click();
    await this.page
      .getByRole("heading", { name: "Configuration", exact: true })
      .waitFor({ timeout: WebDriver.OPEN_MS });
  }

  async schedulerProjectToggleState(handle: string): Promise<{ checked: boolean; disabled: boolean }> {
    const toggle = this.schedulerProjectRow(handle).getByRole("switch");
    return {
      checked: (await toggle.getAttribute("aria-checked")) === "true",
      disabled: await toggle.isDisabled().catch(() => false),
    };
  }

  async schedulerProjectToggle(handle: string): Promise<void> {
    // No outcome wait: the list applies the flip optimistically and the
    // scenario polls the switch plus the server row for the result.
    await this.schedulerProjectRow(handle).getByRole("switch").click();
  }

  async schedulerProjectToggleFlightGated(handle: string): Promise<boolean> {
    // Hold the toggle PATCH mid-flight so the in-flight gate (which a
    // full-speed flip would resolve before the read) stays observable.
    const pattern = "**/api/**/scheduler-bindings/*";
    await this.page.route(pattern, async (route) => {
      if (route.request().method() !== "PATCH") {
        await route.continue().catch(() => undefined);
        return;
      }
      await new Promise((resolve) => setTimeout(resolve, 2_000));
      await route.continue().catch(() => undefined);
    });
    try {
      await this.schedulerProjectRow(handle).getByRole("switch").click();
      const deadline = Date.now() + 10_000;
      for (;;) {
        const state = await this.schedulerProjectToggleState(handle).catch(() => null);
        if (state !== null && state.disabled) return true;
        if (Date.now() > deadline) return false;
        await this.page.waitForTimeout(100);
      }
    } finally {
      await this.page.unroute(pattern);
    }
  }

  async schedulerProjectOpenEdit(handle: string): Promise<void> {
    await this.schedulerProjectRow(handle).getByRole("button", { name: "Edit", exact: true }).click();
    const dialog = this.schedulerDialog();
    await dialog.getByText("Edit scheduler install", { exact: false }).waitFor({ timeout: WebDriver.OPEN_MS });
    await dialog.getByLabel("Starts at").waitFor({ timeout: WebDriver.OPEN_MS });
  }

  async schedulerProjectOpenUninstall(handle: string): Promise<void> {
    await this.schedulerProjectRow(handle).getByRole("button", { name: "Uninstall", exact: true }).click();
    await this.schedulerDialog()
      .getByRole("heading", { name: "Uninstall scheduler?", exact: true })
      .waitFor({ timeout: WebDriver.OPEN_MS });
  }

  async schedulerUninstallDialogText(): Promise<string> {
    return (
      (await this.schedulerDialog()
        .innerText()
        .catch(() => "")) ?? ""
    ).trim();
  }

  async schedulerConfirmUninstall(): Promise<void> {
    const dialog = this.schedulerDialog();
    await dialog.getByRole("button", { name: "Uninstall", exact: true }).click();
    // Either the dialog closes (uninstalled) or a failure notice shows while
    // it stays open. Race the two: waiting out the full close timeout first
    // would let the notice auto-dismiss before the scenario reads it.
    const heading = dialog.getByRole("heading", { name: "Uninstall scheduler?", exact: true });
    const deadline = Date.now() + WebDriver.OPEN_MS;
    for (;;) {
      if ((await heading.count()) === 0) return;
      const toast = await this.rulesLastToast();
      if (toast !== null && toast.title === "Something went wrong") {
        await expect
          .poll(() => this.rulesLastToast(), { timeout: WebDriver.OPEN_MS })
          .toEqual({ title: "Something went wrong", message: expect.any(String) });
        return;
      }
      if (Date.now() > deadline) throw new Error("[parity] uninstall confirmation settled on neither outcome.");
      await this.page.waitForTimeout(500);
    }
  }

  async schedulerUninstallOpen(): Promise<boolean> {
    return (
      (await this.schedulerDialog().getByRole("heading", { name: "Uninstall scheduler?", exact: true }).count()) > 0
    );
  }

  async schedulerCancelUninstall(): Promise<void> {
    const dialog = this.schedulerDialog();
    await dialog.getByRole("button", { name: "Cancel", exact: true }).click();
    await dialog
      .getByRole("heading", { name: "Uninstall scheduler?", exact: true })
      .waitFor({ state: "detached", timeout: WebDriver.OPEN_MS });
  }

  async schedulerOpenProjectInstall(): Promise<void> {
    await this.page.getByRole("button", { name: "New Scheduler", exact: true }).click();
    const dialog = this.schedulerDialog();
    await dialog
      .getByText("New Scheduler", { exact: true })
      .or(dialog.getByText("No schedulers available", { exact: false }))
      .first()
      .waitFor({ timeout: WebDriver.OPEN_MS });
  }

  async schedulerProjectInstallMode(): Promise<"install" | "create" | "dead-end"> {
    const dialog = this.schedulerDialog();
    if ((await dialog.getByText("No schedulers available", { exact: false }).count()) > 0) return "dead-end";
    const tabs = dialog.getByRole("tab");
    if ((await tabs.count()) === 0) return "install";
    const selected = dialog.getByRole("tab", { selected: true });
    return ((await selected.innerText().catch(() => "")) ?? "").trim() === "Create new" ? "create" : "install";
  }

  async schedulerProjectInstallDeadEndText(): Promise<string> {
    return (
      (await this.schedulerDialog()
        .innerText()
        .catch(() => "")) ?? ""
    ).trim();
  }

  async schedulerProjectInstallTabs(): Promise<string[]> {
    const tabs = this.schedulerDialog().getByRole("tab");
    const total = await tabs.count();
    const names: string[] = [];
    for (let i = 0; i < total; i++) {
      const text = (
        (await tabs
          .nth(i)
          .innerText()
          .catch(() => "")) ?? ""
      ).trim();
      if (text !== "") names.push(text);
    }
    return names;
  }

  // --- Runner chat on cloud/web (NEWFRONT-181, RUN-025–032). Selectors
  // --- follow the chat page behavior observed on the running old app:
  // --- side-nav contacts link to the chat route, the history panel
  // --- lists date-labelled sessions, and the composer is an icon-button
  // --- row (send/stop/mic distinguished by their Lucide glyphs). Route
  // --- patterns end in `**`: a trailing `*` never crosses `/`, so it
  // --- misses the API's trailing slashes.

  private runnerChatSideNav(): Locator {
    return this.page.locator("aside", { hasText: "AI Agents" });
  }

  private runnerChatHistoryPanel(): Locator {
    return this.page.locator("aside", { has: this.page.getByText("Chats", { exact: true }) });
  }

  private runnerChatComposerBox(): Locator {
    return this.page.locator('textarea[placeholder*="Message this runner"]');
  }

  // The chat thread column (header/list/composer host). The page nests
  // the side nav and history panel inside an outer main, so every chat
  // selector scopes here instead of matching shell internals.
  private runnerChatThreadColumn(): Locator {
    return this.page.locator("div.flex.min-w-0.flex-1.flex-col.overflow-hidden.px-4");
  }

  private runnerChatListColumn(): Locator {
    return this.runnerChatThreadColumn().locator("div.flex.flex-col.gap-3");
  }

  private runnerChatHeaderBar(): Locator {
    return this.runnerChatThreadColumn().locator("div.flex.h-12.shrink-0");
  }

  async runnerChatOpen(workspaceSlug: string, runnerId: string, sessionId?: string): Promise<void> {
    const suffix = sessionId === undefined ? "" : `?sessionId=${encodeURIComponent(sessionId)}`;
    await this.page.goto(`/${workspaceSlug}/runners/chat/${runnerId}${suffix}`);
    await this.page.waitForLoadState("domcontentloaded");
    // Settles on the history heading plus the composer: both render for
    // known and unknown runners, so the unavailable state settles too.
    // Generous bounds with one reload: the first hit compiles the route
    // on the dev server, which is slow under parallel oracle runs.
    await this.waitForContent("runner chat history", () =>
      this.runnerChatHistoryPanel().getByText("Chats", { exact: true }).first().waitFor({ timeout: 60_000 })
    );
    await this.waitForContent("runner chat composer", () =>
      this.runnerChatComposerBox().first().waitFor({ timeout: 60_000 })
    );
  }

  async runnerChatContactNames(): Promise<string[]> {
    const links = this.runnerChatSideNav().locator('a[href*="/chat/"]');
    const total = await links.count();
    const names: string[] = [];
    for (let i = 0; i < total; i++) {
      names.push((await links.nth(i).innerText()).trim());
    }
    return names;
  }

  async schedulerProjectInstallSelectTab(tab: "Install existing" | "Create new"): Promise<void> {
    const dialog = this.schedulerDialog();
    await dialog.getByRole("tab", { name: tab, exact: true }).click();
    if (tab === "Create new") {
      await dialog.getByLabel("Slug").waitFor({ timeout: WebDriver.OPEN_MS });
    } else {
      await dialog
        .locator("#binding-scheduler")
        .or(dialog.getByText("Every enabled workspace scheduler is already installed", { exact: false }))
        .first()
        .waitFor({ timeout: WebDriver.OPEN_MS });
    }
  }

  async schedulerProjectInstallOptions(): Promise<SchedulerProjectInstallOption[]> {
    const select = this.schedulerDialog().locator("#binding-scheduler");
    // With nothing installable the picker does not render at all (guidance
    // text takes its place); the count guard keeps that an empty read
    // instead of a wait for a node that never comes.
    if ((await select.count()) === 0) return [];
    const current = await select.inputValue().catch(() => "");
    const options = select.locator("option");
    const total = await options.count();
    const out: SchedulerProjectInstallOption[] = [];
    for (let i = 0; i < total; i++) {
      // Options render as "Display name (handle)".
      const text = (
        (await options
          .nth(i)
          .innerText()
          .catch(() => "")) ?? ""
      ).trim();
      const match = /^(.*)\s+\(([^()]+)\)$/.exec(text);
      if (match === null) continue;
      const value =
        (await options
          .nth(i)
          .getAttribute("value")
          .catch(() => null)) ?? "";
      out.push({ name: (match[1] ?? "").trim(), handle: (match[2] ?? "").trim(), selected: value === current });
    }
    return out;
  }

  async schedulerProjectInstallSelect(handle: string): Promise<void> {
    // Option values are definition ids (unknown to the scenario), so resolve
    // the value through the handle suffix of the visible label first.
    const select = this.schedulerDialog().locator("#binding-scheduler");
    const options = select.locator("option");
    const total = await options.count();
    for (let i = 0; i < total; i++) {
      const text = (
        (await options
          .nth(i)
          .innerText()
          .catch(() => "")) ?? ""
      ).trim();
      if (text.endsWith(`(${handle})`)) {
        const value = await options.nth(i).getAttribute("value");
        if (value !== null) {
          await select.selectOption(value);
          return;
        }
      }
    }
    throw new Error(`[parity] install picker offers no definition with handle ${handle}.`);
  }

  async schedulerProjectInstallFillSchedule(input: {
    dtstart?: string;
    tzid?: string;
    rrule?: string;
    extraContext?: string;
  }): Promise<void> {
    await this.schedulerFillSchedule(this.schedulerDialog(), input);
  }

  async schedulerProjectInstallScheduleValues(): Promise<SchedulerScheduleValues> {
    return this.schedulerScheduleValues(this.schedulerDialog());
  }

  async schedulerProjectInstallHumanizer(): Promise<string> {
    return this.schedulerHumanizer(this.schedulerDialog());
  }

  async schedulerProjectInstallSetEnabled(enabled: boolean): Promise<void> {
    await this.schedulerSetModalEnabled(this.schedulerDialog(), enabled);
  }

  async schedulerProjectInstallErrors(): Promise<string[]> {
    return this.schedulerModalErrors(this.schedulerDialog());
  }

  async schedulerProjectInstallSubmit(): Promise<void> {
    // No close wait: a rejected submit keeps the modal open with an error
    // toast, and the scenario asserts that half too.
    await this.schedulerDialog().locator('button[type="submit"]').click();
  }

  async schedulerProjectInstallSubmitDisabled(): Promise<boolean> {
    return this.schedulerDialog().locator('button[type="submit"]').isDisabled();
  }

  async schedulerProjectInstallOpen(): Promise<boolean> {
    const dialog = this.schedulerDialog();
    return (
      (await dialog.getByText("New Scheduler", { exact: true }).count()) > 0 ||
      (await dialog.getByText("No schedulers available", { exact: false }).count()) > 0
    );
  }

  async schedulerCloseProjectInstall(): Promise<void> {
    const dialog = this.schedulerDialog();
    await dialog.getByRole("button", { name: "Cancel", exact: true }).click();
    await expect.poll(() => this.schedulerProjectInstallOpen(), { timeout: WebDriver.OPEN_MS }).toBe(false);
  }

  async schedulerProjectCreateFillDescription(description: string): Promise<void> {
    await this.schedulerDialog().getByLabel("Description").fill(description);
  }

  async schedulerProjectCreateFillPrompt(prompt: string): Promise<void> {
    await this.schedulerDialog().getByLabel("Prompt").fill(prompt);
  }

  async schedulerProjectEditValues(): Promise<SchedulerBindingValues> {
    const dialog = this.schedulerDialog();
    const schedule = await this.schedulerScheduleValues(dialog);
    const checked = dialog.getByRole("radio", { checked: true });
    return {
      ...schedule,
      outcomeLabel: (
        (await checked
          .first()
          .innerText()
          .catch(() => "")) ?? ""
      ).trim(),
      pod: await dialog.getByLabel("Pod").inputValue(),
    };
  }

  async schedulerProjectEditFill(input: {
    dtstart?: string;
    tzid?: string;
    rrule?: string;
    extraContext?: string;
  }): Promise<void> {
    await this.schedulerFillSchedule(this.schedulerDialog(), input);
  }

  async schedulerProjectEditSetEnabled(enabled: boolean): Promise<void> {
    await this.schedulerSetModalEnabled(this.schedulerDialog(), enabled);
  }

  async schedulerProjectEditHumanizer(): Promise<string> {
    return this.schedulerHumanizer(this.schedulerDialog());
  }

  async schedulerProjectEditErrors(): Promise<string[]> {
    return this.schedulerModalErrors(this.schedulerDialog());
  }

  async schedulerProjectEditSubmit(): Promise<void> {
    // No close wait: a rejected submit keeps the dialog open with an error
    // toast, and the scenario asserts that half too.
    await this.schedulerDialog().locator('button[type="submit"]').click();
  }

  async schedulerProjectEditOpen(): Promise<boolean> {
    return (await this.schedulerDialog().getByText("Edit scheduler install", { exact: false }).count()) > 0;
  }

  async schedulerCloseProjectEdit(): Promise<void> {
    const dialog = this.schedulerDialog();
    await dialog.getByRole("button", { name: "Cancel", exact: true }).click();
    await expect.poll(() => this.schedulerProjectEditOpen(), { timeout: WebDriver.OPEN_MS }).toBe(false);
  }

  async schedulerOutcomeState(): Promise<{ options: { label: string; checked: boolean }[]; help: string }> {
    const dialog = this.schedulerDialog();
    const group = dialog.getByRole("radiogroup", { name: "What to do with findings" });
    const radios = group.getByRole("radio");
    const total = await radios.count();
    const options: { label: string; checked: boolean }[] = [];
    for (let i = 0; i < total; i++) {
      options.push({
        label: (
          (await radios
            .nth(i)
            .innerText()
            .catch(() => "")) ?? ""
        ).trim(),
        checked: (await radios.nth(i).getAttribute("aria-checked")) === "true",
      });
    }
    // The help line is the paragraph right after the option row.
    const help = group.locator("xpath=following-sibling::p[1]");
    return { options, help: ((await help.innerText().catch(() => "")) ?? "").trim() };
  }

  async schedulerOutcomeSelect(label: string): Promise<void> {
    await this.schedulerDialog()
      .getByRole("radiogroup", { name: "What to do with findings" })
      .getByRole("radio", { name: label, exact: true })
      .click();
  }

  async schedulerPodOptions(): Promise<{ value: string; label: string; selected: boolean }[]> {
    const select = this.schedulerDialog().getByLabel("Pod");
    const current = await select.inputValue();
    const options = select.locator("option");
    const total = await options.count();
    const out: { value: string; label: string; selected: boolean }[] = [];
    for (let i = 0; i < total; i++) {
      const value =
        (await options
          .nth(i)
          .getAttribute("value")
          .catch(() => null)) ?? "";
      out.push({
        value,
        label: (
          (await options
            .nth(i)
            .innerText()
            .catch(() => "")) ?? ""
        ).trim(),
        selected: value === current,
      });
    }
    return out;
  }

  async schedulerPodSelect(value: string): Promise<void> {
    await this.schedulerDialog().getByLabel("Pod").selectOption(value);
  }

  async schedulerPodDisabled(): Promise<boolean> {
    return this.schedulerDialog().getByLabel("Pod").isDisabled();
  }

  async schedulerPodLoadingObserved(): Promise<boolean> {
    // Hold the pod listing mid-flight so the loading gate (which a
    // full-speed fetch would resolve before the read) stays observable.
    const pattern = "**/api/runners/pods/*";
    await this.page.route(pattern, async (route) => {
      if (route.request().method() !== "GET") {
        await route.continue().catch(() => undefined);
        return;
      }
      await new Promise((resolve) => setTimeout(resolve, 2_000));
      await route.continue().catch(() => undefined);
    });
    try {
      await this.page.getByRole("button", { name: "New Scheduler", exact: true }).click();
      const dialog = this.schedulerDialog();
      await dialog.getByText("New Scheduler", { exact: true }).first().waitFor({ timeout: WebDriver.OPEN_MS });
      const deadline = Date.now() + 10_000;
      for (;;) {
        if (await this.schedulerPodDisabled().catch(() => false)) return true;
        if (Date.now() > deadline) return false;
        await this.page.waitForTimeout(100);
      }
    } finally {
      await this.page.unroute(pattern);
      if (await this.schedulerProjectInstallOpen().catch(() => false)) {
        await this.schedulerCloseProjectInstall().catch(() => undefined);
      }
    }
  }

  /** The install-detail content header (the scheduler name h1's own header, not the app chrome). */
  private schedulerDetailHeader(): Locator {
    return this.page
      .getByRole("heading", { name: "Configuration", exact: true })
      .locator("xpath=ancestor::section[1]")
      .locator("xpath=preceding-sibling::header[1]");
  }

  /** The run-history section (the ancestor of its heading). */
  private schedulerRunsSection(): Locator {
    return this.page.getByRole("heading", { name: "Run history", exact: true }).locator("xpath=ancestor::section[1]");
  }

  async schedulerOpenProjectBinding(workspaceSlug: string, projectId: string, bindingId: string): Promise<void> {
    await this.page.goto(`/${workspaceSlug}/projects/${projectId}/schedulers/${bindingId}`);
    await this.page.locator("#main-sidebar").waitFor({ state: "attached", timeout: WebDriver.OPEN_MS });
    await this.page
      .getByRole("heading", { name: "Configuration", exact: true })
      .or(this.page.getByText("This scheduler install is not available", { exact: false }))
      .first()
      .waitFor({ timeout: WebDriver.OPEN_MS });
  }

  async schedulerBindingHeader(): Promise<SchedulerBindingHeader> {
    const header = this.schedulerDetailHeader();
    const badges: string[] = [];
    // The origin mark renders the raw source value; the workspace-disabled
    // mark renders its sentence. Both are presence reads off known texts.
    for (const mark of ["builtin", "manifest", "Disabled for the whole workspace"]) {
      if ((await header.getByText(mark, { exact: true }).count()) > 0) badges.push(mark);
    }
    return {
      name: (
        (await header
          .getByRole("heading", { level: 1 })
          .innerText()
          .catch(() => "")) ?? ""
      ).trim(),
      handle: (
        (await header
          .locator("code")
          .innerText()
          .catch(() => "")) ?? ""
      ).trim(),
      badges,
      workspaceLinkVisible: await this.isShown(header.getByRole("link", { name: "View workspace definition" })),
      editVisible: await this.isShown(header.getByRole("button", { name: "Edit", exact: true })),
      uninstallVisible: await this.isShown(header.getByRole("button", { name: "Uninstall", exact: true })),
    };
  }

  async schedulerBindingConfig(): Promise<{ label: string; value: string }[]> {
    const grid = this.page
      .getByRole("heading", { name: "Configuration", exact: true })
      .locator("xpath=following-sibling::dl[1]");
    const labels = grid.locator(":scope > div > dt");
    const total = await labels.count();
    const out: { label: string; value: string }[] = [];
    for (let i = 0; i < total; i++) {
      // Labels render through a CSS uppercase transform, so the source text
      // comes from textContent (innerText would return SCHEDULE).
      const label = (
        (await labels
          .nth(i)
          .textContent()
          .catch(() => "")) ?? ""
      ).trim();
      // The Enabled row's value cell holds the switch plus the state word;
      // the switch contributes no text, so the cell read stays uniform.
      const valueCell = labels.nth(i).locator("xpath=following-sibling::dd[1]");
      const value = ((await valueCell.innerText().catch(() => "")) ?? "").trim();
      if (label !== "") out.push({ label, value });
    }
    return out;
  }

  async schedulerBindingScheduleTitle(): Promise<string> {
    const grid = this.page
      .getByRole("heading", { name: "Configuration", exact: true })
      .locator("xpath=following-sibling::dl[1]");
    const valueCell = grid.locator("dt", { hasText: "Schedule" }).locator("xpath=following-sibling::dd[1]");
    return (await valueCell.getAttribute("title").catch(() => null)) ?? "";
  }

  async schedulerBindingLastError(): Promise<string | null> {
    const mark = this.page.getByText("Last error", { exact: true });
    if ((await mark.count()) === 0) return null;
    const panel = mark.locator("xpath=ancestor::div[1]");
    return (
      (await panel
        .locator("pre")
        .innerText()
        .catch(() => "")) ?? ""
    ).trim();
  }

  async schedulerBindingExtraContext(): Promise<string | null> {
    // The label renders through a CSS uppercase transform; the insensitive
    // match reads it under either text engine.
    const mark = this.page.getByText(/project context/i);
    if ((await mark.count()) === 0) return null;
    const block = mark.first().locator("xpath=ancestor::div[1]");
    return (
      (await block
        .locator("pre")
        .innerText()
        .catch(() => "")) ?? ""
    ).trim();
  }

  async schedulerBindingPromptState(): Promise<{ toggleVisible: boolean; revealed: boolean }> {
    const show = this.page.getByRole("button", { name: "Show resolved prompt", exact: true });
    const hide = this.page.getByRole("button", { name: "Hide resolved prompt", exact: true });
    const revealed =
      (await hide.count()) > 0 &&
      (await hide
        .first()
        .isVisible()
        .catch(() => false));
    const toggleVisible =
      revealed ||
      ((await show.count()) > 0 &&
        (await show
          .first()
          .isVisible()
          .catch(() => false)));
    return { toggleVisible, revealed };
  }

  async schedulerBindingPromptToggle(): Promise<void> {
    const state = await this.schedulerBindingPromptState();
    if (state.revealed) {
      await this.page.getByRole("button", { name: "Hide resolved prompt", exact: true }).click();
    } else {
      await this.page.getByRole("button", { name: "Show resolved prompt", exact: true }).click();
    }
  }

  async schedulerBindingPromptText(): Promise<string> {
    const toggle = this.page.getByRole("button", { name: "Hide resolved prompt", exact: true });
    const body = toggle.locator("xpath=following-sibling::pre[1]");
    await body.waitFor({ timeout: WebDriver.OPEN_MS });
    return ((await body.innerText().catch(() => "")) ?? "").trim();
  }

  async schedulerBindingRuns(): Promise<SchedulerBindingRunRow[]> {
    const rows = this.schedulerRunsSection().locator("tbody tr");
    const total = await rows.count();
    const out: SchedulerBindingRunRow[] = [];
    for (let i = 0; i < total; i++) {
      const row = rows.nth(i);
      // Guidance/loader rows span every column; data-row reads skip them.
      if ((await row.locator("td[colspan]").count()) > 0) continue;
      const cells = row.locator("td");
      const cellText = async (index: number): Promise<string> =>
        (
          (await cells
            .nth(index)
            .innerText()
            .catch(() => "")) ?? ""
        ).trim();
      out.push({
        started: await cellText(0),
        ended: await cellText(1),
        status: await cellText(2),
        duration: await cellText(3),
        pod: await cellText(4),
        result: await cellText(5),
      });
    }
    return out;
  }

  async schedulerBindingRunsEmpty(): Promise<string | null> {
    // Immediate read: the loader row also spans every column, so callers
    // poll until the text settles on an empty state (or rows appear).
    const cell = this.schedulerRunsSection().locator("tbody tr td[colspan]");
    if ((await cell.count()) === 0) return null;
    return (
      (await cell
        .first()
        .innerText()
        .catch(() => "")) ?? ""
    ).trim();
  }

  async schedulerBindingRunsCount(): Promise<string> {
    const bar = this.page.getByRole("heading", { name: "Run history", exact: true }).locator("xpath=ancestor::div[1]");
    const count = bar.locator("span").first();
    return ((await count.innerText().catch(() => "")) ?? "").trim();
  }

  async schedulerBindingRunsPager(): Promise<{
    text: string;
    prevDisabled: boolean;
    nextDisabled: boolean;
  } | null> {
    const section = this.schedulerRunsSection();
    const mark = section.getByText(/^Page \d+ of \d+$/);
    if ((await mark.count()) === 0) return null;
    const bar = mark.locator("xpath=ancestor::div[1]");
    return {
      text: (
        (await mark
          .first()
          .innerText()
          .catch(() => "")) ?? ""
      ).trim(),
      prevDisabled: await bar.getByRole("button", { name: "Previous", exact: true }).isDisabled(),
      nextDisabled: await bar.getByRole("button", { name: "Next", exact: true }).isDisabled(),
    };
  }

  async schedulerBindingRunsPage(direction: "next" | "prev"): Promise<void> {
    const section = this.schedulerRunsSection();
    const before = (
      (await section
        .getByText(/^Page \d+ of \d+$/)
        .first()
        .innerText()
        .catch(() => "")) ?? ""
    ).trim();
    const button = section.getByRole("button", { name: direction === "next" ? "Next" : "Previous", exact: true });
    if (await button.isDisabled()) throw new Error(`[parity] run-history pager cannot step ${direction} here.`);
    await button.click();
    await expect
      .poll(
        () =>
          section
            .getByText(/^Page \d+ of \d+$/)
            .first()
            .innerText()
            .catch(() => before),
        {
          timeout: WebDriver.OPEN_MS,
        }
      )
      .not.toBe(before);
  }

  async schedulerBindingWaitRunsRefetch(): Promise<void> {
    await this.page.waitForResponse(/scheduler-bindings\/[^/]+\/runs\//, { timeout: WebDriver.OPEN_MS });
  }

  async schedulerBindingRemovedVisible(): Promise<boolean> {
    return this.isShown(this.page.getByText("This scheduler install is not available", { exact: false }));
  }

  async schedulerBindingBackToList(): Promise<void> {
    await this.page.getByRole("link", { name: "Back to schedulers" }).first().click();
    await this.page
      .getByRole("columnheader", { name: "Scheduler", exact: true })
      .waitFor({ timeout: WebDriver.OPEN_MS });
  }

  async schedulerBindingToggleState(): Promise<{ checked: boolean; disabled: boolean }> {
    // The detail carries exactly one switch (the config Enabled row).
    const toggle = this.page.getByRole("switch");
    return {
      checked: (await toggle.getAttribute("aria-checked")) === "true",
      disabled: await toggle.isDisabled().catch(() => false),
    };
  }

  async schedulerBindingToggle(): Promise<void> {
    // No outcome wait: the scenario polls the switch plus the server row.
    await this.page.getByRole("switch").click();
  }

  async schedulerBindingOpenEdit(): Promise<void> {
    await this.schedulerDetailHeader().getByRole("button", { name: "Edit", exact: true }).click();
    const dialog = this.schedulerDialog();
    await dialog.getByText("Edit scheduler install", { exact: false }).waitFor({ timeout: WebDriver.OPEN_MS });
    await dialog.getByLabel("Starts at").waitFor({ timeout: WebDriver.OPEN_MS });
  }

  async schedulerBindingOpenUninstall(): Promise<void> {
    await this.schedulerDetailHeader().getByRole("button", { name: "Uninstall", exact: true }).click();
    await this.schedulerDialog()
      .getByRole("heading", { name: "Uninstall scheduler?", exact: true })
      .waitFor({ timeout: WebDriver.OPEN_MS });
  }

  private schedulerCalendarScope(): Locator {
    // Section root: four levels above the header's Today button (left
    // group, header bar, column wrapper, section root).
    return this.page.getByRole("button", { name: "Today", exact: true }).locator("xpath=../../../..");
  }

  private schedulerMonthRoot(): Locator {
    // Month day cells carry a tall min-height class no other calendar
    // node uses; the view root is two levels above the first cell.
    return this.page.locator("div.min-h-\\[6rem\\]").first().locator("xpath=../..");
  }

  private schedulerWeekRoot(): Locator {
    // Week day columns fix an inline 24h x 48px height; the view root
    // is two levels above the first column.
    return this.page.locator('div[style*="1152px"]').first().locator("xpath=../..");
  }

  private schedulerMonthCells(): Locator {
    return this.schedulerMonthRoot().locator("div.min-h-\\[6rem\\]");
  }

  private schedulerWeekHeader(): Locator {
    return this.schedulerWeekRoot().locator(":scope > div").first();
  }

  private schedulerWeekColumns(): Locator {
    return this.schedulerWeekRoot().locator('div[style*="1152px"]');
  }

  private schedulerRail(): Locator {
    return this.page.locator("aside", { hasText: "Calendars" });
  }

  private schedulerDrawer(): Locator {
    // Fixed right-side panel; the edit modal never shares the screen
    // with it (opening the modal closes the drawer first).
    return this.page.locator("div.fixed.inset-y-0.right-0.z-40");
  }

  private async schedulerCalendarBlockRead(button: Locator, day: string): Promise<SchedulerCalendarBlock> {
    const spans = button.locator(":scope > span");
    const background =
      (await button.evaluate((el) => window.getComputedStyle(el).backgroundColor).catch(() => "")) ?? "";
    return {
      day,
      time: (
        (await spans
          .nth(0)
          .innerText()
          .catch(() => "")) ?? ""
      ).trim(),
      name: (
        (await spans
          .nth(1)
          .innerText()
          .catch(() => "")) ?? ""
      ).trim(),
      title: (await button.getAttribute("title").catch(() => null)) ?? "",
      background: String(background),
    };
  }

  /** Controls whose label offers an export-ish action (the negative row says none exist). */
  private async schedulerExportScan(scope: Locator): Promise<string[]> {
    const found: string[] = [];
    for (const role of ["button", "link"] as const) {
      const controls = scope.getByRole(role);
      const total = await controls.count();
      for (let i = 0; i < total; i++) {
        const control = controls.nth(i);
        if (!(await control.isVisible().catch(() => false))) continue;
        // Calendar occurrence blocks carry tooltips and scheduler names that
        // may contain export-ish words; they are click-to-inspect, not
        // export actions, so they stay out of the scan.
        if (((await control.getAttribute("title").catch(() => null)) ?? "") !== "") continue;
        const text = ((await control.innerText().catch(() => "")) ?? "").trim();
        if (text === "") continue;
        if (/export|download|print|share/i.test(text)) found.push(text);
      }
    }
    return found;
  }

  async schedulerOpenProjectCalendar(workspaceSlug: string, projectId: string): Promise<void> {
    await this.page.goto(`/${workspaceSlug}/projects/${projectId}/schedulers/calendar`);
    await this.page.locator("#main-sidebar").waitFor({ state: "attached", timeout: WebDriver.OPEN_MS });
    // The grid renders before its data arrives; settle on the occurrences
    // round-trip plus whichever surface (grid or empty state) follows it.
    await this.page.waitForResponse(/scheduler-bindings\/occurrences/, { timeout: WebDriver.OPEN_MS });
    await this.schedulerMonthRoot()
      .or(this.schedulerWeekRoot())
      .or(this.page.getByRole("heading", { name: "No schedulers installed", exact: true }))
      .first()
      .waitFor({ timeout: WebDriver.OPEN_MS });
  }

  async schedulerCalendarView(): Promise<"week" | "month"> {
    if ((await this.schedulerWeekRoot().count()) > 0) return "week";
    return "month";
  }

  async schedulerCalendarSetView(view: "week" | "month"): Promise<void> {
    // Every view switch refetches a new window; settle on that round-trip.
    const label = view === "week" ? "Week" : "Month";
    const target = view === "week" ? this.schedulerWeekRoot() : this.schedulerMonthRoot();
    await Promise.all([
      this.page.waitForResponse(/scheduler-bindings\/occurrences/, { timeout: WebDriver.OPEN_MS }),
      this.page.getByRole("button", { name: label, exact: true }).click(),
    ]);
    await target.waitFor({ timeout: WebDriver.OPEN_MS });
  }

  async schedulerCalendarTitle(): Promise<string> {
    // The month-year label is the trailing div of the header's left group.
    const label = this.page.getByRole("button", { name: "Today", exact: true }).locator("xpath=../div[last()]");
    await label.waitFor({ timeout: WebDriver.OPEN_MS });
    return ((await label.innerText().catch(() => "")) ?? "").trim();
  }

  async schedulerCalendarStep(direction: "prev" | "next"): Promise<void> {
    // Every period step refetches a new window; settle on that round-trip.
    const label = direction === "prev" ? "Previous" : "Next";
    await Promise.all([
      this.page.waitForResponse(/scheduler-bindings\/occurrences/, { timeout: WebDriver.OPEN_MS }),
      this.page.getByRole("button", { name: label, exact: true }).click(),
    ]);
  }

  async schedulerCalendarToday(): Promise<void> {
    await this.page.getByRole("button", { name: "Today", exact: true }).click();
    // Jumping from the current window refetches nothing, so the response
    // wait below is best-effort; the title poll is the real settle.
    await this.page.waitForResponse(/scheduler-bindings\/occurrences/, { timeout: 5_000 }).catch(() => null);
    const current = new Intl.DateTimeFormat(undefined, { month: "long", year: "numeric" }).format(new Date());
    await expect.poll(() => this.schedulerCalendarTitle(), { timeout: WebDriver.OPEN_MS }).toBe(current);
  }

  async schedulerCalendarEmptyVisible(): Promise<boolean> {
    return this.isShown(this.page.getByRole("heading", { name: "No schedulers installed", exact: true }));
  }

  async schedulerCalendarTruncatedVisible(): Promise<boolean> {
    return this.isShown(this.page.getByText("Too many occurrences in this window", { exact: false }));
  }

  async schedulerCalendarMonthBlocks(): Promise<SchedulerCalendarBlock[]> {
    const cells = this.schedulerMonthCells();
    const total = await cells.count();
    const out: SchedulerCalendarBlock[] = [];
    for (let i = 0; i < total; i++) {
      const cell = cells.nth(i);
      const day = (
        (await cell
          .locator(":scope > div > span")
          .first()
          .innerText()
          .catch(() => "")) ?? ""
      ).trim();
      // Titled buttons are occurrence blocks; the untitled ones are the
      // overflow/rollup controls (owned by the overflow read below).
      const buttons = cell.locator("button[title]");
      const count = await buttons.count();
      for (let j = 0; j < count; j++) {
        out.push(await this.schedulerCalendarBlockRead(buttons.nth(j), day));
      }
    }
    return out;
  }

  async schedulerCalendarMonthOverflow(): Promise<string[]> {
    const cells = this.schedulerMonthCells();
    const total = await cells.count();
    const out: string[] = [];
    for (let i = 0; i < total; i++) {
      const buttons = cells.nth(i).locator("button:not([title])");
      const count = await buttons.count();
      for (let j = 0; j < count; j++) {
        const text = (
          (await buttons
            .nth(j)
            .innerText()
            .catch(() => "")) ?? ""
        ).trim();
        if (text !== "") out.push(text);
      }
    }
    return out;
  }

  async schedulerCalendarMonthTodayMarked(): Promise<boolean> {
    // Today's numeral renders as a filled pill; no pill exists when today
    // is outside the visible grid.
    return (await this.schedulerMonthRoot().locator("span.rounded-full").count()) > 0;
  }

  async schedulerCalendarWeekBlocks(): Promise<SchedulerCalendarBlock[]> {
    const headerCells = this.schedulerWeekHeader().locator(":scope > div");
    const headerTotal = await headerCells.count();
    const days: string[] = [];
    for (let i = 0; i < headerTotal; i++) {
      // The first cell is the time-column spacer (no span at all); the rest
      // carry one day-header span each (plus a Today pill when current).
      // The count guard matters: innerText on a missing node waits out the
      // whole test timeout instead of failing fast.
      const span = headerCells.nth(i).locator("span").first();
      if ((await span.count()) === 0) continue;
      const text = ((await span.innerText().catch(() => "")) ?? "").trim();
      if (text !== "") days.push(text);
    }
    const columns = this.schedulerWeekColumns();
    const total = await columns.count();
    const out: SchedulerCalendarBlock[] = [];
    for (let i = 0; i < total; i++) {
      const buttons = columns.nth(i).locator("button[title]");
      const count = await buttons.count();
      for (let j = 0; j < count; j++) {
        out.push(await this.schedulerCalendarBlockRead(buttons.nth(j), days[i] ?? ""));
      }
    }
    return out;
  }

  async schedulerCalendarWeekTodayMarked(): Promise<boolean> {
    return this.isShown(this.schedulerWeekHeader().getByText("Today", { exact: true }));
  }

  async schedulerCalendarTimeLineTop(): Promise<number | null> {
    const line = this.schedulerWeekRoot().locator("div.border-t-2");
    if ((await line.count()) === 0) return null;
    const top = await line.evaluate((el) => (el as HTMLElement).style.top).catch(() => "");
    const parsed = Number.parseFloat(String(top));
    return Number.isFinite(parsed) ? parsed : null;
  }

  async schedulerCalendarClickBlock(name: string): Promise<void> {
    const exact = name.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
    const block = this.schedulerCalendarScope()
      .locator("button[title]")
      .filter({ hasText: new RegExp(exact) })
      .first();
    await block.click();
    await this.schedulerDrawer().waitFor({ timeout: WebDriver.OPEN_MS });
  }

  async schedulerCalendarAnyDraggable(): Promise<boolean> {
    return (await this.schedulerCalendarScope().locator('button[draggable="true"]').count()) > 0;
  }

  async schedulerCalendarExportControls(): Promise<string[]> {
    return this.schedulerExportScan(this.schedulerCalendarScope());
  }

  async schedulerRailVisible(): Promise<boolean> {
    return this.isShown(this.schedulerRail());
  }

  async schedulerRailRows(): Promise<{ name: string; checked: boolean }[]> {
    const rail = this.schedulerRail();
    const rows = rail.locator("label");
    const total = await rows.count();
    const out: { name: string; checked: boolean }[] = [];
    for (let i = 0; i < total; i++) {
      const row = rows.nth(i);
      if ((await row.locator('input[type="checkbox"]').count()) === 0) continue;
      out.push({
        name: (
          (await row
            .locator("span")
            .last()
            .innerText()
            .catch(() => "")) ?? ""
        ).trim(),
        checked: await row.locator('input[type="checkbox"]').isChecked(),
      });
    }
    return out;
  }

  async schedulerRailToggle(name: string): Promise<void> {
    const rail = this.schedulerRail();
    await rail.locator("label", { hasText: name }).locator('input[type="checkbox"]').click();
  }

  async schedulerRailShowAll(): Promise<void> {
    await this.schedulerRail().getByRole("button", { name: "Show all", exact: true }).click();
  }

  async schedulerRailHideAll(): Promise<void> {
    await this.schedulerRail().getByRole("button", { name: "Hide all", exact: true }).click();
  }

  async schedulerRailCrossTabPersists(workspaceSlug: string, projectId: string, name: string): Promise<boolean> {
    // Toggle off here first, then open a fresh tab in the same context
    // (same session, same local storage area): the rail's hidden set is
    // load-time state, so the second tab must load with it applied. There
    // is no live broadcast between open tabs.
    await this.schedulerRailToggle(name);
    const origin = new URL(this.page.url()).origin;
    const second = await this.page.context().newPage();
    try {
      await second.goto(`${origin}/${workspaceSlug}/projects/${projectId}/schedulers/calendar`);
      const rail = second.locator("aside", { hasText: "Calendars" });
      await rail.waitFor({ timeout: WebDriver.OPEN_MS });
      const row = rail.locator("label", { hasText: name }).locator('input[type="checkbox"]');
      await row.waitFor({ timeout: WebDriver.OPEN_MS });
      return !(await row.isChecked());
    } finally {
      await second.close().catch(() => undefined);
    }
  }

  async schedulerRailNarrowHidden(): Promise<boolean> {
    await this.page.setViewportSize({ width: 500, height: 800 });
    try {
      await expect.poll(() => this.schedulerRailVisible(), { timeout: WebDriver.OPEN_MS }).toBe(false);
      return true;
    } catch {
      return false;
    } finally {
      await this.page.setViewportSize({ width: 1280, height: 720 });
    }
  }

  async schedulerDrawerOpen(): Promise<boolean> {
    return (await this.schedulerDrawer().count()) > 0;
  }

  async schedulerDrawerRows(): Promise<{ label: string; value: string }[]> {
    const drawer = this.schedulerDrawer();
    const rows = drawer.locator("div.mb-3");
    const total = await rows.count();
    const out: { label: string; value: string }[] = [];
    for (let i = 0; i < total; i++) {
      const cells = rows.nth(i).locator(":scope > div");
      // Row labels render through a CSS uppercase transform, so the source
      // text comes from textContent (innerText would return WHEN).
      const label = (
        (await cells
          .nth(0)
          .textContent()
          .catch(() => "")) ?? ""
      ).trim();
      const value = (
        (await cells
          .nth(1)
          .innerText()
          .catch(() => "")) ?? ""
      ).trim();
      if (label !== "") out.push({ label, value });
    }
    return out;
  }

  async schedulerDrawerHeading(): Promise<{ state: string; name: string }> {
    const drawer = this.schedulerDrawer();
    // The state line is the uppercased tracking-wide line above the rows;
    // textContent returns its source text (innerText would return SCHEDULED).
    const state = drawer.locator("div.uppercase").first();
    return {
      state: ((await state.textContent().catch(() => "")) ?? "").trim(),
      name: (
        (await drawer
          .getByRole("heading", { level: 2 })
          .innerText()
          .catch(() => "")) ?? ""
      ).trim(),
    };
  }

  async schedulerDrawerLinks(): Promise<string[]> {
    const links = this.schedulerDrawer().getByRole("link");
    const total = await links.count();
    const out: string[] = [];
    for (let i = 0; i < total; i++) {
      const text = (
        (await links
          .nth(i)
          .innerText()
          .catch(() => "")) ?? ""
      ).trim();
      if (text !== "") out.push(text);
    }
    return out;
  }

  async schedulerDrawerClose(): Promise<void> {
    // The header dismiss carries an aria label; the footer Close (future +
    // admin) is text-only, so the header scope keeps the read unambiguous.
    const drawer = this.schedulerDrawer();
    await drawer.locator("header").getByRole("button", { name: "Close" }).click();
    await drawer.waitFor({ state: "detached", timeout: WebDriver.OPEN_MS });
  }

  async schedulerDrawerEditVisible(): Promise<boolean> {
    return this.isShown(this.schedulerDrawer().getByRole("button", { name: "Edit binding", exact: true }));
  }

  async schedulerDrawerEdit(): Promise<void> {
    await this.schedulerDrawer().getByRole("button", { name: "Edit binding", exact: true }).click();
    const dialog = this.schedulerDialog();
    await dialog.getByText("Edit scheduler install", { exact: false }).waitFor({ timeout: WebDriver.OPEN_MS });
    await dialog.getByLabel("Starts at").waitFor({ timeout: WebDriver.OPEN_MS });
  }

  async schedulerDrawerViewScheduler(): Promise<void> {
    await this.schedulerDrawer().getByRole("link", { name: "View scheduler" }).click();
    await this.page
      .getByRole("heading", { name: "Configuration", exact: true })
      .waitFor({ timeout: WebDriver.OPEN_MS });
  }

  async schedulerOpenProjectSection(workspaceSlug: string, projectId: string): Promise<void> {
    await this.page.goto(`/${workspaceSlug}/projects/${projectId}/schedulers`);
    await this.page.locator("#main-sidebar").waitFor({ state: "attached", timeout: WebDriver.OPEN_MS });
    // The bare section path redirects to the calendar tab.
    await this.page.waitForURL(/\/schedulers\/calendar$/, { timeout: WebDriver.OPEN_MS });
    await this.schedulerMonthRoot()
      .or(this.schedulerWeekRoot())
      .or(this.page.getByRole("heading", { name: "No schedulers installed", exact: true }))
      .first()
      .waitFor({ timeout: WebDriver.OPEN_MS });
  }

  async schedulerSectionTabs(): Promise<{ label: string; active: boolean }[]> {
    const out: { label: string; active: boolean }[] = [];
    for (const label of ["List", "Calendar"]) {
      const tab = this.page.getByRole("link", { name: label, exact: true });
      const classes = (await tab.getAttribute("class").catch(() => null)) ?? "";
      out.push({ label, active: classes.includes("border-accent-strong") });
    }
    return out;
  }

  async schedulerSectionOpenTab(tab: "List" | "Calendar"): Promise<void> {
    await this.page.getByRole("link", { name: tab, exact: true }).click();
    if (tab === "List") {
      await this.page
        .getByRole("columnheader", { name: "Scheduler", exact: true })
        .waitFor({ timeout: WebDriver.OPEN_MS });
    } else {
      // A quick tab-back may serve cached occurrences without a round-trip
      // (SWR dedupes), so the response wait is best-effort; the grid wait
      // below is the real settle, and scenarios poll for their content.
      await this.page.waitForResponse(/scheduler-bindings\/occurrences/, { timeout: 5_000 }).catch(() => null);
      await this.schedulerMonthRoot()
        .or(this.schedulerWeekRoot())
        .or(this.page.getByRole("heading", { name: "No schedulers installed", exact: true }))
        .first()
        .waitFor({ timeout: WebDriver.OPEN_MS });
    }
  }

  async schedulerOpenSettingsSchedulers(workspaceSlug: string, projectId: string): Promise<void> {
    await this.page.goto(`/${workspaceSlug}/settings/projects/${projectId}/schedulers`);
    // The settings shell carries no workspace sidebar; settle on the panel
    // headers for admins, or the refusal heading for everyone else.
    await this.page
      .getByRole("columnheader", { name: "Scheduler", exact: true })
      .or(this.schedulerGateHeading())
      .first()
      .waitFor({ timeout: WebDriver.OPEN_MS });
  }

  async schedulerSettingsPanelVisible(): Promise<boolean> {
    // Admins get the installs panel (its table headers); everyone else gets
    // the not-authorized panel instead.
    return this.isShown(this.page.getByRole("columnheader", { name: "Scheduler", exact: true }));
  }

  async schedulerRunsExportControls(): Promise<string[]> {
    return this.schedulerExportScan(this.schedulerRunsSection());
  }

  async runnerChatContactDotClass(runnerName: string): Promise<string> {
    const contact = this.runnerChatSideNav().locator('a[href*="/chat/"]', { hasText: runnerName });
    const dot = contact.locator("svg.lucide-circle");
    await dot.first().waitFor({ timeout: WebDriver.WAIT_MS });
    return (await dot.first().getAttribute("class")) ?? "";
  }

  async runnerChatOpenContact(runnerName: string): Promise<void> {
    const before = this.page.url();
    await this.runnerChatSideNav().locator('a[href*="/chat/"]', { hasText: runnerName }).first().click();
    await this.page.waitForFunction((previous) => window.location.href !== previous, before, {
      timeout: WebDriver.WAIT_MS,
    });
    await this.runnerChatComposerBox().first().waitFor({ timeout: WebDriver.WAIT_MS });
  }

  async runnerChatHeader(): Promise<{ name: string; secondary: string; badge: string }> {
    const header = this.runnerChatHeaderBar();
    const name = (await header.locator(".text-15").first().innerText()).trim();
    const secondary = await header
      .locator(".truncate.text-12")
      .first()
      .innerText()
      .then(
        (text) => text.trim(),
        () => ""
      );
    // The header carries the status badge (a text button) plus the
    // icon-only close control; the badge is the button with text.
    const buttons = header.locator("button");
    const total = await buttons.count();
    let badge = "";
    for (let i = 0; i < total; i++) {
      const text = (await buttons.nth(i).innerText()).trim();
      if (text !== "") {
        badge = text;
        break;
      }
    }
    return { name, secondary, badge };
  }

  async runnerChatHistoryEmptyVisible(): Promise<boolean> {
    return this.runnerChatHistoryPanel()
      .getByText("No chats yet.")
      .first()
      .isVisible()
      .then(
        (visible) => visible,
        () => false
      );
  }

  async runnerChatHistoryItems(): Promise<{ title: string; subtitle: string; active: boolean }[]> {
    const panel = this.runnerChatHistoryPanel();
    const buttons = panel.locator("nav button");
    const total = await buttons.count();
    const items: { title: string; subtitle: string; active: boolean }[] = [];
    for (let i = 0; i < total; i++) {
      const button = buttons.nth(i);
      const wrap = button.locator("span.flex-col");
      const title = (await wrap.locator(":scope > span").nth(0).innerText()).trim();
      const subtitle =
        (await wrap.locator(":scope > span").count()) > 1
          ? (await wrap.locator(":scope > span").nth(1).innerText()).trim()
          : "";
      const classes = (await button.getAttribute("class")) ?? "";
      items.push({ title, subtitle, active: classes.includes("font-medium") });
    }
    return items;
  }

  async runnerChatClickHistoryItem(index: number): Promise<void> {
    await this.runnerChatHistoryPanel().locator("nav button").nth(index).click();
  }

  async runnerChatNewChat(): Promise<void> {
    const before = new URL(this.page.url()).search;
    await this.runnerChatHistoryPanel().getByRole("button", { name: "New chat", exact: true }).click();
    // Resolves on selection (the session query lands) or on the error
    // toast: the error-state scenario asserts which one it got.
    await Promise.race([
      this.page
        .waitForFunction((previous) => new URL(window.location.href).search !== previous, before, {
          timeout: WebDriver.WAIT_MS,
        })
        .then(
          () => true,
          () => false
        ),
      this.page
        .locator("div.absolute.right-3.bottom-3")
        .first()
        .waitFor({ timeout: WebDriver.WAIT_MS })
        .then(
          () => true,
          () => false
        ),
    ]);
  }

  async runnerChatNewChatDisabled(): Promise<boolean> {
    return this.runnerChatHistoryPanel()
      .getByRole("button", { name: "New chat", exact: true })
      .isDisabled()
      .then(
        (disabled) => disabled,
        () => false
      );
  }

  async runnerChatFailSessionCreate(): Promise<void> {
    this.runnerChatSessionCreateFailRemaining = 1;
    await this.page.route("**/api/runners/chat/sessions**", async (route) => {
      const request = route.request();
      const pathname = new URL(request.url()).pathname;
      if (request.method() === "POST" && pathname.endsWith("/chat/sessions/")) {
        if (this.runnerChatSessionCreateFailRemaining > 0) {
          this.runnerChatSessionCreateFailRemaining -= 1;
          await route.fulfill({
            status: 500,
            contentType: "application/json",
            body: JSON.stringify({ error: "parity session-create failure" }),
          });
          return;
        }
      }
      await route.fallback();
    });
  }

  async runnerChatDelaySessionCreate(ms: number): Promise<void> {
    this.runnerChatSessionCreateDelayMs = ms;
    this.runnerChatSessionCreateDelayRemaining = 1;
    await this.page.route("**/api/runners/chat/sessions**", async (route) => {
      const request = route.request();
      const pathname = new URL(request.url()).pathname;
      if (
        request.method() === "POST" &&
        pathname.endsWith("/chat/sessions/") &&
        this.runnerChatSessionCreateDelayRemaining > 0
      ) {
        this.runnerChatSessionCreateDelayRemaining -= 1;
        await new Promise((resolve) => setTimeout(resolve, this.runnerChatSessionCreateDelayMs));
      }
      await route.fallback();
    });
  }

  async runnerChatClearSessionCreateStubs(): Promise<void> {
    this.runnerChatSessionCreateFailRemaining = 0;
    this.runnerChatSessionCreateDelayRemaining = 0;
    await this.page.unroute("**/api/runners/chat/sessions**");
  }

  async runnerChatFillDraft(text: string): Promise<void> {
    await this.runnerChatComposerBox().first().fill(text);
  }

  async runnerChatDraftValue(): Promise<string> {
    return this.runnerChatComposerBox().first().inputValue();
  }

  async runnerChatPressEnter(): Promise<void> {
    await this.runnerChatComposerBox().first().press("Enter");
  }

  async runnerChatPressShiftEnter(): Promise<void> {
    await this.runnerChatComposerBox().first().press("Shift+Enter");
  }

  async runnerChatSendEnabled(): Promise<boolean> {
    return this.runnerChatThreadColumn()
      .locator("button:has(svg.lucide-send)")
      .first()
      .isEnabled()
      .then(
        (enabled) => enabled,
        () => false
      );
  }

  async runnerChatClickSend(): Promise<void> {
    await this.runnerChatThreadColumn().locator("button:has(svg.lucide-send)").first().click();
  }

  async runnerChatComposerReason(): Promise<string | null> {
    const reason = this.runnerChatThreadColumn().locator("div.mb-2.text-12.text-secondary").first();
    if ((await reason.count()) === 0) return null;
    const text = (await reason.innerText()).trim();
    return text === "" ? null : text;
  }

  async runnerChatTextareaDisabled(): Promise<boolean> {
    return this.runnerChatComposerBox().first().isDisabled();
  }

  async runnerChatAlertText(): Promise<string | null> {
    const alert = this.runnerChatThreadColumn().locator('div[role="alert"]').first();
    if ((await alert.count()) === 0) return null;
    const text = (await alert.innerText()).trim();
    return text === "" ? null : text;
  }

  async runnerChatDismissAlert(): Promise<void> {
    await this.runnerChatThreadColumn().locator('div[role="alert"]').first().getByRole("button").click();
  }

  async runnerChatLastToast(): Promise<{ title: string; message: string } | null> {
    // Toasts stack bottom-right and auto-dismiss; only a currently
    // visible one with text is reported, newest first.
    const roots = this.page.locator("div.absolute.right-3.bottom-3");
    const total = await roots.count();
    for (let i = total - 1; i >= 0; i--) {
      const text = (await roots.nth(i).innerText()).trim();
      if (text === "") continue;
      const lines = text
        .split("\n")
        .map((line) => line.trim())
        .filter((line) => line.length > 0);
      return { title: lines[0] ?? "", message: lines.slice(1).join(" ") };
    }
    return null;
  }

  async runnerChatFailNextSend(): Promise<void> {
    this.runnerChatSendFailRemaining = 1;
    await this.page.route("**/api/runners/chat/sessions/*/messages**", async (route) => {
      const request = route.request();
      if (request.method() === "POST" && this.runnerChatSendFailRemaining > 0) {
        this.runnerChatSendFailRemaining -= 1;
        await route.fulfill({
          status: 500,
          contentType: "application/json",
          body: JSON.stringify({ error: "parity send failure" }),
        });
        return;
      }
      await route.fallback();
    });
  }

  async runnerChatClearSendFailure(): Promise<void> {
    this.runnerChatSendFailRemaining = 0;
    await this.page.unroute("**/api/runners/chat/sessions/*/messages**");
  }

  async runnerChatVoiceButtonLabel(): Promise<string | null> {
    const mic = this.runnerChatThreadColumn().locator("button:has(svg.lucide-mic)").first();
    if ((await mic.count()) === 0) return null;
    return await mic.getAttribute("aria-label");
  }

  async runnerChatClickVoiceButton(): Promise<void> {
    await this.runnerChatThreadColumn().locator("button:has(svg.lucide-mic)").first().click();
  }

  async runnerChatStartApiSpy(): Promise<void> {
    this.runnerChatSpyCounts = {
      warm: 0,
      sessionCreate: 0,
      send: 0,
      cancel: 0,
      close: 0,
      sessionList: 0,
      messageList: 0,
    };
    // One classifying route: counts the call, then falls through so the
    // request still reaches the server (or a later-registered stub).
    await this.page.route("**/api/runners/**", async (route) => {
      const request = route.request();
      const pathname = new URL(request.url()).pathname;
      const method = request.method();
      const counts = this.runnerChatSpyCounts;
      if (method === "POST" && pathname.endsWith("/warm/")) counts.warm += 1;
      else if (method === "POST" && pathname.endsWith("/chat/sessions/")) counts.sessionCreate += 1;
      else if (method === "POST" && pathname.endsWith("/messages/")) counts.send += 1;
      else if (method === "POST" && pathname.endsWith("/cancel/")) counts.cancel += 1;
      else if (method === "POST" && pathname.endsWith("/close/")) counts.close += 1;
      else if (method === "GET" && pathname.endsWith("/chat/sessions/")) counts.sessionList += 1;
      else if (method === "GET" && pathname.endsWith("/messages/")) counts.messageList += 1;
      await route.fallback();
    });
  }

  async runnerChatApiCounts(): Promise<{
    warm: number;
    sessionCreate: number;
    send: number;
    cancel: number;
    close: number;
    sessionList: number;
    messageList: number;
  }> {
    return { ...this.runnerChatSpyCounts };
  }

  async runnerChatStopApiSpy(): Promise<void> {
    await this.page.unroute("**/api/runners/**");
  }

  async runnerChatDelayRunnerDetail(runnerId: string, ms: number): Promise<void> {
    this.runnerChatRunnerDetailDelayMs = ms;
    this.runnerChatRunnerDetailDelayRemaining = 1;
    this.runnerChatRunnerDetailDelayPattern = `**/api/runners/${runnerId}**`;
    await this.page.route(this.runnerChatRunnerDetailDelayPattern, async (route) => {
      const request = route.request();
      const pathname = new URL(request.url()).pathname;
      if (
        request.method() === "GET" &&
        pathname === `/api/runners/${runnerId}/` &&
        this.runnerChatRunnerDetailDelayRemaining > 0
      ) {
        this.runnerChatRunnerDetailDelayRemaining -= 1;
        await new Promise((resolve) => setTimeout(resolve, this.runnerChatRunnerDetailDelayMs));
      }
      await route.fallback();
    });
  }

  async runnerChatClearRunnerDetailDelay(): Promise<void> {
    this.runnerChatRunnerDetailDelayRemaining = 0;
    if (this.runnerChatRunnerDetailDelayPattern !== null) {
      await this.page.unroute(this.runnerChatRunnerDetailDelayPattern);
      this.runnerChatRunnerDetailDelayPattern = null;
    }
  }

  async runnerChatStubStream(sessionId: string, frames: RunnerChatStreamFrame[]): Promise<void> {
    this.runnerChatStreamFrames.set(sessionId, frames);
    if (!this.runnerChatStreamUrls.has(sessionId)) {
      this.runnerChatStreamUrls.set(sessionId, []);
      // Installed once per session; re-stubbing swaps the frames the
      // handler serves, so the stream's natural reconnect picks them up.
      await this.page.route(`**/chat/sessions/${sessionId}/events**`, async (route) => {
        this.runnerChatStreamUrls.get(sessionId)?.push(route.request().url());
        const stamp = new Date().toISOString();
        const body = (this.runnerChatStreamFrames.get(sessionId) ?? [])
          .map((frame) => {
            // A deliberately unparsable data line: the page surfaces a
            // transient-error banner for it, cleared by the next frame.
            if (frame.kind === "raw-invalid") return `event: chat.event\ndata: not-json seq=${frame.seq}\n\n`;
            const event = {
              id: frame.seq,
              session: sessionId,
              message: frame.message ?? null,
              seq: frame.seq,
              kind: frame.kind,
              payload: frame.payload,
              created_at: stamp,
            };
            return `event: chat.event\nid: ${frame.seq}\ndata: ${JSON.stringify(event)}\n\n`;
          })
          .join("");
        await route.fulfill({
          status: 200,
          headers: { "content-type": "text/event-stream", "cache-control": "no-cache" },
          body,
        });
      });
    }
  }

  async runnerChatClearStreamStub(sessionId: string): Promise<void> {
    this.runnerChatStreamFrames.delete(sessionId);
    await this.page.unroute(`**/chat/sessions/${sessionId}/events**`);
  }

  async runnerChatStreamRequestUrls(sessionId: string): Promise<string[]> {
    return [...(this.runnerChatStreamUrls.get(sessionId) ?? [])];
  }

  async runnerChatMessageBubbles(): Promise<{ role: string; text: string }[]> {
    const column = this.runnerChatListColumn();
    if ((await column.count()) === 0) return [];
    return column.evaluate((element) => {
      const rows: { role: string; text: string }[] = [];
      for (const child of Array.from(element.children)) {
        const node = child as HTMLElement;
        // Activity-strip items are direct rounded children, not bubbles.
        if (node.classList.contains("rounded")) continue;
        const text = (node.innerText ?? "").trim();
        // The bottom scroll anchor carries no text.
        if (text === "") continue;
        if (node.querySelector(".justify-end") !== null) rows.push({ role: "user", text });
        else if (node.querySelector(".justify-start") !== null) rows.push({ role: "assistant", text });
        else rows.push({ role: "status", text });
      }
      return rows;
    });
  }

  async runnerChatAssistantBubbleHtml(index: number): Promise<string> {
    return this.runnerChatListColumn().locator("div.justify-start > div").nth(index).innerHTML();
  }

  async runnerChatActivityStrip(): Promise<string[]> {
    const items = this.runnerChatListColumn().locator(":scope > div.rounded");
    const total = await items.count();
    const labels: string[] = [];
    for (let i = 0; i < total; i++) {
      const text = (await items.nth(i).innerText()).trim();
      if (text !== "") labels.push(text);
    }
    return labels;
  }

  async runnerChatStopVisible(): Promise<boolean> {
    return this.runnerChatThreadColumn()
      .locator("button:has(svg.lucide-square)")
      .first()
      .isVisible()
      .then(
        (visible) => visible,
        () => false
      );
  }

  async runnerChatClickStop(): Promise<void> {
    await this.runnerChatThreadColumn().locator("button:has(svg.lucide-square)").first().click();
  }

  async runnerChatClickClose(): Promise<void> {
    await this.runnerChatHeaderBar().locator("button:has(svg.lucide-x)").first().click();
  }

  async runnerChatApprovalPromptVisible(): Promise<boolean> {
    return this.page
      .getByRole("alertdialog", { name: "Approval requested" })
      .first()
      .isVisible()
      .then(
        (visible) => visible,
        () => false
      );
  }

  async runnerChatHoldMessageList(ms: number): Promise<void> {
    // A stubbed stream ends as soon as its frames are served, and the
    // reconnect error refetches messages — resetting the live list to the
    // server state before the streamed bubble can be asserted. Holding the
    // refetch keeps the streamed bubble on screen while the scenario reads
    // it. Never combined with the send-failure stub on one page (both sit
    // on the messages route).
    this.runnerChatMessageListHoldMs = ms;
    if (!this.runnerChatMessageListHoldArmed) {
      this.runnerChatMessageListHoldArmed = true;
      await this.page.route("**/api/runners/chat/sessions/*/messages**", async (route) => {
        const request = route.request();
        const pathname = new URL(request.url()).pathname;
        if (request.method() === "GET" && pathname.endsWith("/messages/") && this.runnerChatMessageListHoldArmed) {
          await new Promise((resolve) => setTimeout(resolve, this.runnerChatMessageListHoldMs));
        }
        await route.fallback();
      });
    }
  }

  async runnerChatReleaseMessageList(): Promise<void> {
    this.runnerChatMessageListHoldArmed = false;
    await this.page.unroute("**/api/runners/chat/sessions/*/messages**");
  }

  // --- Prompts + project automations (NEWFRONT-186, AGT-023–037) ---
  // --- Every selector below was observed on the running old app: section
  // --- cards carry prompt-section-<key> ids with provenance badges, editors
  // --- are textareas scoped to their card, receipts are collapsible
  // --- prompt-receipt-<kind> sections, and automation rows are h4-titled
  // --- blocks with headless switches and combobox pickers.

  /** Route pattern for the prompt-sections list reads (no trailing slash). */
  private static readonly PROMPT_SECTIONS_PATTERN = "**/prompt-sections?*";

  private promptsTabButton(tab: "Sections" | "Receipt"): Locator {
    return this.page.getByRole("button", { name: tab, exact: true });
  }

  private promptsSectionsAside(): Locator {
    return this.page.locator("aside", { has: this.page.getByText("Sections", { exact: true }) });
  }

  private promptsReceiptAside(): Locator {
    return this.page.locator("aside", { has: this.page.getByText("Receipts", { exact: true }) });
  }

  private promptsCard(key: string): Locator {
    return this.page.locator(`div#prompt-section-${key}`);
  }

  private promptsCards(): Locator {
    return this.page.locator('div[id^="prompt-section-"]');
  }

  private promptsOpenEditorRoot(): Locator {
    return this.page.locator('div[id^="prompt-section-"]', { has: this.page.locator("textarea") });
  }

  private promptsKindLabel(kind: string): string {
    if (kind === "coding-task") return "Coding task";
    if (kind === "review") return "Review";
    return "Scheduler";
  }

  private promptsKindSlug(label: string): string {
    if (label === "Coding task") return "coding-task";
    if (label === "Review") return "review";
    return "scheduler";
  }

  private promptsReceiptSection(kind: string): Locator {
    return this.page.locator(`section#prompt-receipt-${kind}`);
  }

  private automationsRow(title: string): Locator {
    // Both the settings control item and the outer row block match
    // div.gap-4 with the heading; the outer row comes first in paint order.
    return this.page
      .locator("div.gap-4", { has: this.page.getByRole("heading", { name: title, exact: true }) })
      .first();
  }

  private automationsSection(): Locator {
    return this.page.locator("section", { has: this.page.getByRole("heading", { name: "Automations", exact: true }) });
  }

  async promptsOpen(workspaceSlug: string): Promise<void> {
    await this.page.goto(`/${workspaceSlug}/prompts`);
    await this.page.getByRole("heading", { name: "Prompts", exact: true }).waitFor({ timeout: WebDriver.OPEN_MS });
    await this.promptsSectionsAside()
      .or(this.page.getByText("Loading…", { exact: true }))
      .or(this.page.getByText("Could not load prompt sections for this workspace.", { exact: true }))
      .first()
      .waitFor({ timeout: WebDriver.OPEN_MS });
  }

  async promptsActiveTab(): Promise<"Sections" | "Receipt"> {
    if (await this.isShown(this.promptsReceiptAside())) return "Receipt";
    return "Sections";
  }

  async promptsOpenTab(tab: "Sections" | "Receipt"): Promise<void> {
    await this.promptsTabButton(tab).click();
    const aside = tab === "Sections" ? this.promptsSectionsAside() : this.promptsReceiptAside();
    await aside.waitFor({ timeout: WebDriver.OPEN_MS });
  }

  async promptsSectionCards(): Promise<PromptSectionCard[]> {
    const roots = this.promptsCards();
    const total = await roots.count();
    const out: PromptSectionCard[] = [];
    for (let i = 0; i < total; i++) {
      const card = await this.promptsReadCard(roots.nth(i));
      if (card !== null) out.push(card);
    }
    return out;
  }

  async promptsSectionCard(key: string): Promise<PromptSectionCard | null> {
    const root = this.promptsCard(key);
    if ((await root.count()) === 0) return null;
    return this.promptsReadCard(root.first());
  }

  private async promptsReadCard(root: Locator): Promise<PromptSectionCard | null> {
    const id = await root.getAttribute("id").catch(() => null);
    if (id === null || !id.startsWith("prompt-section-")) return null;
    const key = id.slice("prompt-section-".length);
    const header = root.locator("div.flex.items-start").first();
    const title = (
      (await header
        .locator("span.text-13")
        .first()
        .innerText()
        .catch(() => "")) ?? ""
    ).trim();
    // Badges render as disabled buttons; edit controls as enabled ones.
    const buttons = root.getByRole("button");
    const total = await buttons.count();
    let sourceBadge = "";
    const badges: string[] = [];
    const kinds: string[] = [];
    let workspaceEditLabel: string | null = null;
    let personalEditLabel: string | null = null;
    for (let i = 0; i < total; i++) {
      const text = (
        (await buttons
          .nth(i)
          .innerText()
          .catch(() => "")) ?? ""
      ).trim();
      if (text === "") continue;
      if (text === "Pi Dash default" || text === "Workspace override" || text === "Your override") {
        sourceBadge = text;
      } else if (text === "Coding task" || text === "Review" || text === "Scheduler") {
        kinds.push(text);
        badges.push(text);
      } else if (text === "Locked" || text === "Admin-managed") {
        badges.push(text);
      } else if (text === "Edit workspace default" || text === "Customize for workspace") {
        workspaceEditLabel = text;
      } else if (text === "Edit my override" || text === "Customize for me") {
        personalEditLabel = text;
      }
    }
    const staleWarning = await this.isShown(root.getByText("This override may no longer render", { exact: false }));
    const bodyPre = root.locator(":scope > pre");
    const body = ((await bodyPre.innerText().catch(() => "")) ?? "").trimEnd();
    return { key, title, sourceBadge, badges, kinds, staleWarning, body, workspaceEditLabel, personalEditLabel };
  }

  async promptsSectionNav(): Promise<{ title: string; key: string }[]> {
    const links = this.promptsSectionsAside().getByRole("link");
    const total = await links.count();
    const out: { title: string; key: string }[] = [];
    for (let i = 0; i < total; i++) {
      const href =
        (await links
          .nth(i)
          .getAttribute("href")
          .catch(() => null)) ?? "";
      const lines = (
        (await links
          .nth(i)
          .innerText()
          .catch(() => "")) ?? ""
      )
        .split("\n")
        .map((line) => line.trim())
        .filter((line) => line.length > 0);
      out.push({ title: lines[0] ?? "", key: href.replace(/^#prompt-section-/, "") });
    }
    return out;
  }

  async promptsSectionNavJump(key: string): Promise<string> {
    await this.promptsSectionsAside()
      .getByRole("link", { name: new RegExp(key.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")) })
      .click();
    await this.promptsCard(key).waitFor({ state: "visible", timeout: WebDriver.OPEN_MS });
    return this.page.evaluate(() => window.location.hash);
  }

  async promptsLoadingVisible(): Promise<boolean> {
    return this.isShown(this.page.getByText("Loading…", { exact: true }));
  }

  async promptsSectionsErrorVisible(): Promise<boolean> {
    return this.isShown(this.page.getByText("Could not load prompt sections for this workspace.", { exact: true }));
  }

  async promptsWorkspaceWarningVisible(): Promise<boolean> {
    return this.isShown(this.page.getByText("workspace editing is unavailable", { exact: false }));
  }

  async promptsFailSectionsStart(scope: "user" | "workspace"): Promise<void> {
    // Persistent (not once): SWR retries failed reads, so a single-shot
    // failure would flap the banner instead of holding it.
    await this.page.route(WebDriver.PROMPT_SECTIONS_PATTERN, async (route) => {
      const url = route.request().url();
      if (route.request().method() === "GET" && url.includes(`scope=${scope}`)) {
        await route.fulfill({
          status: 500,
          contentType: "application/json",
          body: JSON.stringify({ error: "parity sections failure" }),
        });
        return;
      }
      await route.continue();
    });
  }

  async promptsFailSectionsStop(): Promise<void> {
    await this.page.unroute(WebDriver.PROMPT_SECTIONS_PATTERN).catch(() => undefined);
  }

  async promptsDelaySectionsOnce(ms: number): Promise<void> {
    let armed = true;
    await this.page.route(WebDriver.PROMPT_SECTIONS_PATTERN, async (route) => {
      if (armed && route.request().method() === "GET") {
        armed = false;
        await this.page.waitForTimeout(ms);
      }
      await route.continue();
    });
  }

  async promptsFailUpsertOnce(): Promise<void> {
    let armed = true;
    await this.page.route("**/prompt-sections/*", async (route) => {
      if (armed && route.request().method() === "PUT") {
        armed = false;
        await route.fulfill({
          status: 500,
          contentType: "application/json",
          body: JSON.stringify({ error: "parity upsert failure" }),
        });
        return;
      }
      await route.continue();
    });
  }

  async promptsOpenSectionEditor(key: string, scope: "workspace" | "user"): Promise<void> {
    const card = this.promptsCard(key);
    const name =
      scope === "workspace" ? /Edit workspace default|Customize for workspace/ : /Edit my override|Customize for me/;
    await card.getByRole("button", { name }).click();
    await card.locator("textarea").waitFor({ timeout: WebDriver.OPEN_MS });
  }

  async promptsEditorState(): Promise<PromptEditorState | null> {
    const root = this.promptsOpenEditorRoot().first();
    if ((await root.count()) === 0) return null;
    // The scope caption is the first medium-weight small caption in the
    // editor (the card key above it is placeholder-weight; the draft
    // panel's own caption comes later).
    const scopeLabel = (
      (await root
        .locator("span.text-11.font-medium")
        .first()
        .innerText()
        .catch(() => "")) ?? ""
    ).trim();
    const draft =
      (await root
        .locator("textarea")
        .inputValue()
        .catch(() => "")) ?? "";
    const save = root.getByRole("button", { name: "Save", exact: true });
    const saveEnabled = await save.isEnabled().catch(() => false);
    const defaultPane = root.locator("pre").first();
    const defaultVisible = await this.isShown(defaultPane);
    const defaultBody = defaultVisible ? ((await defaultPane.innerText().catch(() => "")) ?? "").trimEnd() : null;
    const revertVisible = await this.isShown(root.getByRole("button", { name: "Revert to default", exact: true }));
    // Inline save errors render as danger text; preview errors live in the
    // nested draft panel and must not leak into this read.
    const errorBox = root.locator("div.text-danger-primary").first();
    const errorShown = await this.isShown(errorBox);
    let error: string | null = null;
    if (errorShown) {
      const panel = root.locator("div.rounded-md.border-subtle").last();
      const panelError = await panel
        .locator("div.text-danger-primary")
        .count()
        .catch(() => 0);
      const text = ((await errorBox.innerText().catch(() => "")) ?? "").trim();
      error = panelError > 0 && text === "" ? null : text === "" ? null : text;
      if (panelError > 0) {
        // The first danger box may be the preview's; prefer a box outside it.
        const boxes = root.locator(":scope > div.text-danger-primary, :scope div.flex-col > div.text-danger-primary");
        const boxTotal = await boxes.count().catch(() => 0);
        error = null;
        for (let i = 0; i < boxTotal; i++) {
          const candidate = (
            (await boxes
              .nth(i)
              .innerText()
              .catch(() => "")) ?? ""
          ).trim();
          if (candidate !== "") {
            error = candidate;
            break;
          }
        }
      }
    }
    return { scopeLabel, draft, saveEnabled, defaultVisible, defaultBody, revertVisible, error };
  }

  async promptsEditorFill(text: string): Promise<void> {
    await this.promptsOpenEditorRoot().first().locator("textarea").fill(text);
  }

  async promptsEditorSave(): Promise<void> {
    await this.promptsOpenEditorRoot().first().getByRole("button", { name: "Save", exact: true }).click();
  }

  async promptsEditorCancel(): Promise<void> {
    const root = this.promptsOpenEditorRoot().first();
    await root.getByRole("button", { name: "Cancel", exact: true }).click();
    await root.locator("textarea").waitFor({ state: "detached", timeout: WebDriver.OPEN_MS });
  }

  async promptsEditorToggleCompare(): Promise<void> {
    const root = this.promptsOpenEditorRoot().first();
    await root.getByRole("button", { name: /Compare with default|Hide default/ }).click();
  }

  async promptsEditorRevertOpen(): Promise<void> {
    await this.promptsOpenEditorRoot().first().getByRole("button", { name: "Revert to default", exact: true }).click();
    await this.page.getByRole("heading", { name: "Revert to default?", exact: true }).waitFor({
      timeout: WebDriver.OPEN_MS,
    });
  }

  async promptsRevertDialog(): Promise<PromptRevertDialog | null> {
    const heading = this.page.getByRole("heading", { name: "Revert to default?", exact: true });
    if (!(await this.isShown(heading))) return null;
    const dialog = this.page.getByRole("dialog").filter({ has: heading });
    const body = (
      (await dialog
        .locator("div.text-secondary")
        .first()
        .innerText()
        .catch(() => "")) ?? ""
    ).trim();
    const confirm = dialog.getByRole("button", { name: /^(Revert|Reverting)$/ });
    const confirmLabel = (
      (await confirm
        .first()
        .innerText()
        .catch(() => "")) ?? ""
    ).trim();
    return { title: "Revert to default?", body, confirmLabel };
  }

  async promptsRevertConfirm(): Promise<void> {
    const heading = this.page.getByRole("heading", { name: "Revert to default?", exact: true });
    await this.page
      .getByRole("dialog")
      .filter({ has: heading })
      .getByRole("button", { name: /^Revert$/ })
      .click();
  }

  async promptsRevertCancel(): Promise<void> {
    const heading = this.page.getByRole("heading", { name: "Revert to default?", exact: true });
    const dialog = this.page.getByRole("dialog").filter({ has: heading });
    await dialog.getByRole("button", { name: "Cancel", exact: true }).click();
    await heading.waitFor({ state: "detached", timeout: WebDriver.OPEN_MS });
  }

  async promptsReceiptCards(): Promise<PromptReceiptCard[]> {
    const roots = this.page.locator('section[id^="prompt-receipt-"]');
    const total = await roots.count();
    const out: PromptReceiptCard[] = [];
    for (let i = 0; i < total; i++) {
      const root = roots.nth(i);
      const id = (await root.getAttribute("id").catch(() => null)) ?? "";
      const kind = id.replace(/^prompt-receipt-/, "");
      const header = root.locator("button").first();
      const headerText = ((await header.innerText().catch(() => "")) ?? "").trim();
      const countBadge =
        headerText
          .split("\n")
          .map((line) => line.trim())
          .find((line) => /section/.test(line)) ?? "";
      const items = root.locator("ol > li");
      const itemTotal = await items.count();
      const sections: { num: string; title: string; key: string; sourceBadge: string }[] = [];
      for (let j = 0; j < itemTotal; j++) {
        const item = items.nth(j);
        const num = (
          (await item
            .locator("span.font-mono")
            .first()
            .innerText()
            .catch(() => "")) ?? ""
        ).trim();
        const title = (
          (await item
            .locator("span.text-12")
            .first()
            .innerText()
            .catch(() => "")) ?? ""
        ).trim();
        // The number span also carries text-10; the key span is the
        // truncated one.
        const key = (
          (await item
            .locator("span.text-10.truncate")
            .first()
            .innerText()
            .catch(() => "")) ?? ""
        ).trim();
        const sourceBadge = (
          (await item
            .getByRole("button")
            .first()
            .innerText()
            .catch(() => "")) ?? ""
        ).trim();
        sections.push({ num, title, key, sourceBadge });
      }
      out.push({ kind, countBadge, sections });
    }
    return out;
  }

  async promptsReceiptNav(): Promise<{ kind: string; count: string }[]> {
    const links = this.promptsReceiptAside().getByRole("link");
    const total = await links.count();
    const out: { kind: string; count: string }[] = [];
    for (let i = 0; i < total; i++) {
      const href =
        (await links
          .nth(i)
          .getAttribute("href")
          .catch(() => null)) ?? "";
      const lines = (
        (await links
          .nth(i)
          .innerText()
          .catch(() => "")) ?? ""
      )
        .split("\n")
        .map((line) => line.trim())
        .filter((line) => line.length > 0);
      out.push({ kind: href.replace(/^#prompt-receipt-/, ""), count: lines[1] ?? "" });
    }
    return out;
  }

  async promptsReceiptNavJump(kind: string): Promise<string> {
    await this.promptsReceiptAside()
      .getByRole("link", { name: new RegExp(this.promptsKindLabel(kind)) })
      .click();
    await this.promptsReceiptSection(kind).waitFor({ state: "visible", timeout: WebDriver.OPEN_MS });
    return this.page.evaluate(() => window.location.hash);
  }

  async promptsReceiptToggle(kind: string): Promise<void> {
    await this.promptsReceiptSection(kind).locator(":scope > button").first().click();
  }

  async promptsReceiptExpanded(kind: string): Promise<boolean> {
    const header = this.promptsReceiptSection(kind).locator(":scope > button").first();
    const text = ((await header.innerText().catch(() => "")) ?? "").trim();
    return text.includes("Hide");
  }

  async promptsReceiptTemplate(kind: string): Promise<string | null> {
    const section = this.promptsReceiptSection(kind);
    if (!(await this.promptsReceiptExpanded(kind))) return null;
    const first = section.locator("pre").first();
    if (!(await this.isShown(first))) return null;
    return ((await first.innerText().catch(() => "")) ?? "").trimEnd();
  }

  async promptsReceiptAutomatic(kind: string): Promise<string | null> {
    const section = this.promptsReceiptSection(kind);
    const marker = section.getByText("Automatic runs", { exact: false });
    if (!(await this.isShown(marker))) return null;
    const blocks = section.locator("pre");
    const total = await blocks.count();
    if (total < 2) return null;
    return (
      (await blocks
        .nth(1)
        .innerText()
        .catch(() => "")) ?? ""
    ).trimEnd();
  }

  async promptsSavedPreviewVisible(kind: string): Promise<boolean> {
    const section = this.promptsReceiptSection(kind);
    return this.isShown(section.getByRole("heading", { name: "Preview", exact: true }));
  }

  async promptsSavedPreviewSubmitEnabled(kind: string): Promise<boolean> {
    const section = this.promptsReceiptSection(kind);
    return section
      .getByRole("button", { name: "Preview", exact: true })
      .isEnabled()
      .catch(() => false);
  }

  async promptsSavedPreviewSubmit(kind: string, target: string): Promise<void> {
    const section = this.promptsReceiptSection(kind);
    await section.getByRole("textbox").fill(target);
    await section.getByRole("button", { name: "Preview", exact: true }).click();
  }

  async promptsSavedPreviewResult(kind: string): Promise<{ prompt: string | null; error: string | null }> {
    const section = this.promptsReceiptSection(kind);
    const errorBox = section.locator("div.text-danger-primary").first();
    const errorShown = await this.isShown(errorBox);
    const error = errorShown ? ((await errorBox.innerText().catch(() => "")) ?? "").trim() || null : null;
    // The receipt template pre always renders while expanded; the preview
    // result is the last pre (after the template and the automatic block).
    const blocks = section.locator("pre");
    const total = await blocks.count();
    const baseline = (await this.promptsReceiptAutomatic(kind)) === null ? 1 : 2;
    if (total <= baseline) return { prompt: null, error };
    const prompt =
      (
        (await blocks
          .nth(total - 1)
          .innerText()
          .catch(() => "")) ?? ""
      ).trimEnd() || null;
    return { prompt, error };
  }

  private promptsDraftPanel(): Locator {
    // The draft panel sits inside the open editor; scoping to it keeps the
    // card header's kind badges (same labels) and the draft textarea out
    // of the panel reads below.
    return this.promptsOpenEditorRoot().first().locator("div.rounded-md.border-subtle").last();
  }

  async promptsDraftPreviewKinds(): Promise<string[]> {
    const switcher = this.promptsDraftPanel().getByRole("button", { name: /^(Coding task|Review|Scheduler)$/ });
    const total = await switcher.count();
    const out: string[] = [];
    for (let i = 0; i < total; i++) {
      const label = (
        (await switcher
          .nth(i)
          .innerText()
          .catch(() => "")) ?? ""
      ).trim();
      if (label !== "") out.push(this.promptsKindSlug(label));
    }
    return out;
  }

  async promptsDraftPreviewSelectKind(kind: string): Promise<void> {
    await this.promptsDraftPanel()
      .getByRole("button", { name: this.promptsKindLabel(kind), exact: true })
      .click();
  }

  async promptsDraftPreviewSubmitEnabled(): Promise<boolean> {
    const root = this.promptsOpenEditorRoot().first();
    return root
      .getByRole("button", { name: "Preview draft", exact: true })
      .isEnabled()
      .catch(() => false);
  }

  async promptsDraftPreviewSubmit(target: string): Promise<void> {
    const panel = this.promptsDraftPanel();
    await panel.getByRole("textbox").fill(target);
    await panel.getByRole("button", { name: "Preview draft", exact: true }).click();
  }

  async promptsDraftPreviewResult(): Promise<{ prompt: string | null; error: string | null }> {
    const root = this.promptsOpenEditorRoot().first();
    const errorBox = root.locator("div.rounded-md div.text-danger-primary").first();
    const errorShown = await this.isShown(errorBox);
    const error = errorShown ? ((await errorBox.innerText().catch(() => "")) ?? "").trim() || null : null;
    // The draft panel result is the last pre in the editor (after the
    // compare pane, when it shows).
    const blocks = root.locator("div.rounded-md pre");
    const total = await blocks.count();
    if (total === 0) return { prompt: null, error };
    const prompt =
      (
        (await blocks
          .nth(total - 1)
          .innerText()
          .catch(() => "")) ?? ""
      ).trimEnd() || null;
    return { prompt, error };
  }

  async automationsOpen(workspaceSlug: string, projectId: string): Promise<void> {
    await this.page.goto(`/${workspaceSlug}/settings/projects/${projectId}/automations`);
    await this.page
      .getByRole("heading", { name: "Auto-archive closed work items", exact: true })
      .or(this.page.getByRole("heading", { name: "Oops! You are not authorized to view this page", exact: true }))
      .first()
      .waitFor({ timeout: WebDriver.OPEN_MS });
  }

  async automationsNotAuthorizedVisible(): Promise<boolean> {
    return this.isShown(
      this.page.getByRole("heading", { name: "Oops! You are not authorized to view this page", exact: true })
    );
  }

  private async automationsReadRow(title: string): Promise<AutomationRow> {
    const row = this.automationsRow(title);
    const toggle = row.getByRole("switch");
    const checked = (await toggle.getAttribute("aria-checked").catch(() => null)) ?? "";
    const toggleDisabled = await toggle.isDisabled().catch(() => true);
    const picker = row.getByRole("button", { name: /\d+ months?/ });
    const pickerVisible = await this.isShown(picker);
    const pickerLabel = pickerVisible
      ? (
          (await picker
            .first()
            .innerText()
            .catch(() => "")) ?? ""
        ).trim()
      : "";
    return { toggleOn: checked === "true", toggleDisabled, pickerVisible, pickerLabel };
  }

  async automationsArchiveRow(): Promise<AutomationRow> {
    return this.automationsReadRow("Auto-archive closed work items");
  }

  async automationsArchiveToggle(): Promise<void> {
    await this.automationsRow("Auto-archive closed work items").getByRole("switch").click();
  }

  private async automationsSetPreset(title: string, months: number): Promise<void> {
    const label = months === 1 ? "1 month" : `${months} months`;
    await this.automationsRow(title)
      .getByRole("button", { name: /\d+ months?/ })
      .click();
    await this.page.getByRole("option", { name: label, exact: true }).click();
  }

  async automationsArchiveSetPreset(months: number): Promise<void> {
    await this.automationsSetPreset("Auto-archive closed work items", months);
  }

  private async automationsOpenCustom(title: string): Promise<void> {
    await this.automationsRow(title)
      .getByRole("button", { name: /\d+ months?/ })
      .click();
    await this.page.getByRole("button", { name: "Customize time range", exact: true }).click();
    await this.page.getByRole("heading", { name: "Customize time range", exact: true }).waitFor({
      timeout: WebDriver.OPEN_MS,
    });
  }

  async automationsArchiveOpenCustom(): Promise<void> {
    await this.automationsOpenCustom("Auto-archive closed work items");
  }

  async automationsCloseRow(): Promise<AutomationCloseRow> {
    const base = await this.automationsReadRow("Auto-close work items");
    const row = this.automationsRow("Auto-close work items");
    // The state picker is the combobox button that is not the month picker.
    const buttons = row.getByRole("button");
    const total = await buttons.count();
    let stateLabel = "";
    let statePickerDisabled = true;
    for (let i = 0; i < total; i++) {
      const candidate = buttons.nth(i);
      const text = ((await candidate.innerText().catch(() => "")) ?? "").trim();
      if (text === "" || /\d+ months?/.test(text)) continue;
      stateLabel = text;
      statePickerDisabled = await candidate.isDisabled().catch(() => true);
    }
    return { ...base, stateLabel, statePickerDisabled };
  }

  async automationsCloseToggle(): Promise<void> {
    await this.automationsRow("Auto-close work items").getByRole("switch").click();
  }

  async automationsCloseSetPreset(months: number): Promise<void> {
    await this.automationsSetPreset("Auto-close work items", months);
  }

  async automationsCloseSetState(name: string): Promise<void> {
    await (await this.automationsCloseStateButton()).click();
    await this.page.getByRole("option", { name, exact: true }).click();
  }

  private async automationsCloseStateButton(): Promise<Locator> {
    const row = this.automationsRow("Auto-close work items");
    const buttons = row.getByRole("button");
    const total = await buttons.count();
    for (let i = 0; i < total; i++) {
      const candidate = buttons.nth(i);
      const text = ((await candidate.innerText().catch(() => "")) ?? "").trim();
      if (text !== "" && !/\d+ months?/.test(text)) return candidate;
    }
    throw new Error("[parity] auto-close state picker not found.");
  }

  async automationsCloseStateOptions(): Promise<string[]> {
    const button = await this.automationsCloseStateButton();
    await button.click();
    const options = this.page.getByRole("option");
    await options.first().waitFor({ timeout: WebDriver.OPEN_MS });
    const total = await options.count();
    const out: string[] = [];
    for (let i = 0; i < total; i++) {
      out.push(
        (
          (await options
            .nth(i)
            .innerText()
            .catch(() => "")) ?? ""
        ).trim()
      );
    }
    await button.click();
    return out;
  }

  async automationsCloseOpenCustom(): Promise<void> {
    await this.automationsOpenCustom("Auto-close work items");
  }

  async automationsMonthModal(): Promise<AutomationMonthModal | null> {
    const heading = this.page.getByRole("heading", { name: "Customize time range", exact: true });
    if (!(await this.isShown(heading))) return null;
    const dialog = this.page.getByRole("dialog").filter({ has: heading });
    const input = dialog.getByPlaceholder("Enter Months");
    const inputValue = (await input.inputValue().catch(() => "")) ?? "";
    const errorBox = dialog.getByText("Select a month between 1 and 12.", { exact: true });
    const error = (await this.isShown(errorBox)) ? "Select a month between 1 and 12." : null;
    return { title: "Customize time range", inputValue, error };
  }

  async automationsMonthFill(value: string): Promise<void> {
    const heading = this.page.getByRole("heading", { name: "Customize time range", exact: true });
    await this.page.getByRole("dialog").filter({ has: heading }).getByPlaceholder("Enter Months").fill(value);
  }

  async automationsMonthSubmit(): Promise<void> {
    const heading = this.page.getByRole("heading", { name: "Customize time range", exact: true });
    await this.page
      .getByRole("dialog")
      .filter({ has: heading })
      .getByRole("button", { name: "Submit", exact: true })
      .click();
  }

  async automationsMonthCancel(): Promise<void> {
    const heading = this.page.getByRole("heading", { name: "Customize time range", exact: true });
    const dialog = this.page.getByRole("dialog").filter({ has: heading });
    await dialog.getByRole("button", { name: "Cancel", exact: true }).click();
    await heading.waitFor({ state: "detached", timeout: WebDriver.OPEN_MS });
  }

  async automationsFailUpdateOnce(): Promise<void> {
    let armed = true;
    await this.page.route("**/projects/*/", async (route) => {
      if (armed && route.request().method() === "PATCH") {
        armed = false;
        await route.fulfill({
          status: 500,
          contentType: "application/json",
          body: JSON.stringify({ error: "parity update failure" }),
        });
        return;
      }
      await route.continue();
    });
  }

  async automationsBuiltInRows(): Promise<string[]> {
    const headings = this.automationsSection().getByRole("heading", { level: 4 });
    const total = await headings.count();
    const out: string[] = [];
    for (let i = 0; i < total; i++) {
      out.push(
        (
          (await headings
            .nth(i)
            .innerText()
            .catch(() => "")) ?? ""
        ).trim()
      );
    }
    return out;
  }

  async automationsHasExtensionRows(): Promise<boolean> {
    const known = new Set(["Auto-archive closed work items", "Auto-close work items"]);
    const rows = await this.automationsBuiltInRows();
    return rows.some((row) => row !== "" && !known.has(row));
  }

  // --- Add-runner modal + creation (NEWFRONT-179, RUN-006–009) ---
  // --- Appended; existing methods above are untouched per the shared driver
  // --- contract. Every selector was observed on the running old app: the
  // --- modal hangs under the z-30 panel wrapper, the pickers are
  // --- body-portalled comboboxes opened through their field buttons, and
  // --- the command panel renders the generated command in a pre.

  /** Matches the cloud-driven creation calls (create POST + status polls). */
  private static readonly ADD_RUNNER_PATTERN = "**/api/runners/dev-machines/**";

  /** Manual sentinel option label in the machine picker. */
  private static readonly ADD_RUNNER_MANUAL_LABEL = "Run `pidash runner add` manually";

  private static addRunnerEscapeRegExp(text: string): string {
    return text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  }

  /** The open add-runner modal's panel (only one modal opens at a time here). */
  private addRunnerDialog(): Locator {
    return this.page.locator("div.fixed.inset-0.z-30").last();
  }

  private async addRunnerWaitForm(): Promise<void> {
    const dialog = this.addRunnerDialog();
    await dialog.getByRole("button", { name: "Generate Runner", exact: true }).waitFor({ timeout: WebDriver.WAIT_MS });
  }

  private async addRunnerOpenModal(): Promise<void> {
    const open = this.page.getByRole("button", { name: "Add runner", exact: true }).first();
    await open.waitFor({ timeout: WebDriver.WAIT_MS });
    await open.click({ timeout: WebDriver.WAIT_MS });
    await this.addRunnerWaitForm();
  }

  async addRunnerOpenFromRunners(workspaceSlug: string, projectId?: string): Promise<void> {
    const base =
      projectId !== undefined ? `/${workspaceSlug}/projects/${projectId}/runners` : `/${workspaceSlug}/runners`;
    await this.page.goto(base);
    await this.addRunnerOpenModal();
  }

  async addRunnerOpenFromMachines(workspaceSlug: string): Promise<void> {
    await this.page.goto(`/${workspaceSlug}/ai-dev-machines`);
    await this.addRunnerOpenModal();
  }

  async addRunnerVisible(): Promise<boolean> {
    const dialog = this.addRunnerDialog();
    if ((await dialog.count()) === 0) return false;
    const title = dialog.getByText("Add runner", { exact: false }).first();
    const created = dialog.getByText("Runner created", { exact: false }).first();
    return (await this.isShown(title)) || (await this.isShown(created));
  }

  async addRunnerLayout(): Promise<"form" | "remote" | "command"> {
    const dialog = this.addRunnerDialog();
    if ((await dialog.getByRole("button", { name: "Generate Runner", exact: true }).count()) > 0) return "form";
    if ((await dialog.locator("pre").count()) > 0) return "command";
    return "remote";
  }

  /** The combobox trigger button inside the `labelText` field wrapper. */
  private addRunnerPickerButton(labelText: string): Locator {
    const exact = new RegExp(`^${WebDriver.addRunnerEscapeRegExp(labelText)}$`);
    return this.addRunnerDialog().locator("label", { hasText: exact }).locator("xpath=..").getByRole("button").first();
  }

  /**
   * Shut any open picker portal. Escapes ONLY while options render: a
   * bare Escape with no portal open reaches the modal itself and closes
   * it, while an Escape into an open portal stops at the portal.
   */
  private async addRunnerShutPicker(): Promise<void> {
    if ((await this.page.getByRole("option").count()) === 0) return;
    await this.page.keyboard.press("Escape");
    await this.page
      .getByRole("option")
      .first()
      .waitFor({ state: "detached", timeout: 5_000 })
      .catch(() => undefined);
  }

  /**
   * Open the `labelText` picker; resolves once its portalled options
   * render. Converges from every trigger state (closed, stale-open, or
   * expanded-but-empty): shut what is open, then toggle, then verify.
   */
  private async addRunnerOpenPicker(labelText: string): Promise<void> {
    const button = this.addRunnerPickerButton(labelText);
    for (let attempt = 0; attempt < 3; attempt += 1) {
      await this.addRunnerShutPicker();
      await button.click({ timeout: WebDriver.WAIT_MS });
      try {
        await this.page.getByRole("option").first().waitFor({ timeout: 5_000 });
        return;
      } catch {
        // A real outside pointerdown unsticks an expanded-but-empty
        // trigger: the custom open state only resyncs through the
        // outside-click detector, never through Escape alone.
        await this.addRunnerDialog()
          .getByText("Add runner", { exact: false })
          .first()
          .click({ timeout: 5_000 })
          .catch(() => undefined);
        if (attempt === 2) throw new Error(`[parity] the ${labelText} picker never rendered options.`);
      }
    }
  }

  private async addRunnerReadOptions(labelText: string): Promise<string[]> {
    await this.addRunnerOpenPicker(labelText);
    const labels = await this.page.getByRole("option").allInnerTexts();
    await this.addRunnerShutPicker();
    return labels.map((label) => label.trim());
  }

  private async addRunnerPickOption(labelText: string, optionName: string): Promise<void> {
    await this.addRunnerOpenPicker(labelText);
    const option = this.page.getByRole("option", { name: optionName, exact: true });
    await option.waitFor({ timeout: WebDriver.WAIT_MS });
    await option.click({ timeout: WebDriver.WAIT_MS });
    await this.addRunnerShutPicker();
  }

  async addRunnerForm(): Promise<AddRunnerFormState> {
    const dialog = this.addRunnerDialog();
    const buttonText = async (labelText: string): Promise<string> =>
      (
        (await this.addRunnerPickerButton(labelText)
          .innerText()
          .catch(() => "")) ?? ""
      ).trim();
    const projectButton = this.addRunnerPickerButton("Project");
    // The project picker disables while the projects list loads, which
    // reads exactly like the route lock. Settle first: an unlocked picker
    // enables, a locked one fills with the route project (fail-open — the
    // spec's own polls own the outcome, this just de-flakes the read).
    const settleDeadline = Date.now() + 15_000;
    for (;;) {
      const label = await buttonText("Project");
      const klass = (await projectButton.getAttribute("class").catch(() => "")) ?? "";
      const disabled = (await projectButton.isDisabled().catch(() => false)) || klass.includes("cursor-not-allowed");
      if (!disabled || label !== "Select a project" || Date.now() >= settleDeadline) break;
      await this.page.waitForTimeout(250);
    }
    const projectClass = (await projectButton.getAttribute("class").catch(() => "")) ?? "";
    const projectLocked =
      (await projectButton.isDisabled().catch(() => false)) || projectClass.includes("cursor-not-allowed");
    return {
      machine: await buttonText("Dev machine"),
      project: await buttonText("Project"),
      projectLocked,
      pod: await buttonText("Pod (optional)"),
      name: await dialog.locator("#add-runner-name").inputValue({ timeout: WebDriver.WAIT_MS }),
      workingDir: await dialog.locator("#add-runner-working-dir").inputValue({ timeout: WebDriver.WAIT_MS }),
      agent: await buttonText("Agent"),
      model: await buttonText("Model (optional)"),
    };
  }

  async addRunnerMachineOptions(): Promise<string[]> {
    return this.addRunnerReadOptions("Dev machine");
  }

  async addRunnerProjectOptions(): Promise<string[]> {
    return this.addRunnerReadOptions("Project");
  }

  async addRunnerPodOptions(): Promise<string[]> {
    return this.addRunnerReadOptions("Pod (optional)");
  }

  async addRunnerAgentOptions(): Promise<string[]> {
    return this.addRunnerReadOptions("Agent");
  }

  async addRunnerModelOptions(): Promise<string[]> {
    return this.addRunnerReadOptions("Model (optional)");
  }

  async addRunnerProjectError(): Promise<string | null> {
    const error = this.addRunnerDialog().getByText("Pick a project.", { exact: true });
    if ((await error.count()) === 0) return null;
    return ((
      (await error
        .first()
        .innerText()
        .catch(() => "")) ?? ""
    ).trim() || null) as string | null;
  }

  async addRunnerNameError(): Promise<string | null> {
    const error = this.addRunnerDialog().getByText("Runner name cannot contain spaces.", { exact: false });
    if ((await error.count()) === 0) return null;
    return ((
      (await error
        .first()
        .innerText()
        .catch(() => "")) ?? ""
    ).trim() || null) as string | null;
  }

  async addRunnerPickMachine(label: string): Promise<void> {
    // The picker auto-selects the first connected machine; spare the
    // portal a toggle when it already shows the target.
    const current = (
      (await this.addRunnerPickerButton("Dev machine")
        .innerText()
        .catch(() => "")) ?? ""
    ).trim();
    if (current === label) return;
    await this.addRunnerPickOption("Dev machine", label);
  }

  async addRunnerPickManual(): Promise<void> {
    await this.addRunnerPickOption("Dev machine", WebDriver.ADD_RUNNER_MANUAL_LABEL);
  }

  async addRunnerPickProject(name: string): Promise<void> {
    await this.addRunnerPickOption("Project", name);
  }

  async addRunnerPickPod(name: string): Promise<void> {
    if (name === "") {
      await this.addRunnerPickOption("Pod (optional)", "(default pod)");
      return;
    }
    // Pod options carry a project-identifier suffix; match the bare name or
    // the bare name plus that suffix.
    const pattern = new RegExp(`^${WebDriver.addRunnerEscapeRegExp(name)}( \\([^)]*\\)|$)`);
    await this.addRunnerOpenPicker("Pod (optional)");
    const option = this.page.getByRole("option", { name: pattern });
    await option.first().waitFor({ timeout: WebDriver.WAIT_MS });
    await option.first().click({ timeout: WebDriver.WAIT_MS });
    await this.addRunnerShutPicker();
  }

  async addRunnerSetName(name: string): Promise<void> {
    await this.addRunnerDialog().locator("#add-runner-name").fill(name, { timeout: WebDriver.WAIT_MS });
  }

  async addRunnerSetWorkingDir(dir: string): Promise<void> {
    await this.addRunnerDialog().locator("#add-runner-working-dir").fill(dir, { timeout: WebDriver.WAIT_MS });
  }

  async addRunnerPickAgent(label: string): Promise<void> {
    await this.addRunnerPickOption("Agent", label);
  }

  async addRunnerPickModel(label: string): Promise<void> {
    await this.addRunnerPickOption("Model (optional)", label);
  }

  async addRunnerSubmit(): Promise<void> {
    await this.addRunnerDialog()
      .getByRole("button", { name: "Generate Runner", exact: true })
      .click({ timeout: WebDriver.WAIT_MS });
  }

  async addRunnerClose(): Promise<void> {
    const dialog = this.addRunnerDialog();
    // Error/timeout panels offer no dismiss control of their own: step
    // back to the form first, then cancel from there.
    const phase = await this.addRunnerRemotePhase().catch(() => null);
    if (phase === "error" || phase === "timeout") {
      await dialog.getByRole("button", { name: "Back", exact: true }).click({ timeout: WebDriver.WAIT_MS });
      await dialog
        .getByRole("button", { name: "Generate Runner", exact: true })
        .waitFor({ timeout: WebDriver.WAIT_MS });
    }
    for (const name of ["Cancel", "Close", "Done"]) {
      const control = dialog.getByRole("button", { name, exact: true });
      if ((await control.count()) > 0) {
        await control.first().click({ timeout: WebDriver.WAIT_MS });
        return;
      }
    }
    throw new Error("[parity] add-runner modal shows no Cancel/Close/Done control.");
  }

  async addRunnerRemotePhase(): Promise<AddRunnerRemotePhase | null> {
    if ((await this.addRunnerLayout()) !== "remote") return null;
    const body =
      (await this.addRunnerDialog()
        .innerText()
        .catch(() => "")) ?? "";
    if (body.includes("Runner created")) return "ok";
    if (body.includes("Runner creation failed")) return "error";
    if (body.includes("did not report back in time")) return "timeout";
    return "creating";
  }

  async addRunnerRemoteText(): Promise<string | null> {
    if ((await this.addRunnerLayout()) !== "remote") return null;
    return ((
      (await this.addRunnerDialog()
        .innerText()
        .catch(() => "")) ?? ""
    ).trim() || null) as string | null;
  }

  async addRunnerRemoteRunnerName(): Promise<string | null> {
    if ((await this.addRunnerLayout()) !== "remote") return null;
    const code = this.addRunnerDialog().locator("code").first();
    if ((await code.count()) === 0) return null;
    return (((await code.innerText().catch(() => "")) ?? "").trim() || null) as string | null;
  }

  async addRunnerRemoteBack(): Promise<void> {
    await this.addRunnerDialog()
      .getByRole("button", { name: "Back", exact: true })
      .click({ timeout: WebDriver.WAIT_MS });
  }

  async addRunnerRemoteManual(): Promise<void> {
    await this.addRunnerDialog()
      .getByRole("button", { name: "Show manual command", exact: true })
      .click({ timeout: WebDriver.WAIT_MS });
  }

  // One shared route handler feeds both spies: separate handlers on the
  // same pattern starve each other (the later registration shadows the
  // earlier one), so the create and status halves multiplex here.
  private addRunnerSpyCreateBodies: string[] | null = null;
  private addRunnerSpyStatusUrls: string[] | null = null;
  private addRunnerSpyInstalled = false;

  private async addRunnerSpyEnsure(): Promise<void> {
    if (this.addRunnerSpyInstalled) return;
    this.addRunnerSpyInstalled = true;
    await this.page.route(WebDriver.ADD_RUNNER_PATTERN, async (route) => {
      const request = route.request();
      const pathname = new URL(request.url()).pathname;
      if (request.method() === "POST" && pathname.endsWith("/create-runner/")) {
        this.addRunnerSpyCreateBodies?.push(request.postData() ?? "");
      }
      if (request.method() === "GET" && pathname.includes("/create-runner/") && !pathname.endsWith("/create-runner/")) {
        this.addRunnerSpyStatusUrls?.push(request.url());
      }
      await route.continue();
    });
  }

  private async addRunnerSpyMaybeUninstall(): Promise<void> {
    if (this.addRunnerSpyCreateBodies !== null || this.addRunnerSpyStatusUrls !== null) return;
    this.addRunnerSpyInstalled = false;
    await this.page.unroute(WebDriver.ADD_RUNNER_PATTERN).catch(() => undefined);
  }

  async addRunnerCreateSpyStart(): Promise<void> {
    this.addRunnerSpyCreateBodies = [];
    await this.addRunnerSpyEnsure();
  }

  async addRunnerCreateSpyBodies(): Promise<string[]> {
    return [...(this.addRunnerSpyCreateBodies ?? [])];
  }

  async addRunnerCreateSpyStop(): Promise<void> {
    this.addRunnerSpyCreateBodies = null;
    await this.addRunnerSpyMaybeUninstall();
  }

  async addRunnerStatusSpyStart(): Promise<void> {
    this.addRunnerSpyStatusUrls = [];
    await this.addRunnerSpyEnsure();
  }

  async addRunnerStatusSpyUrls(): Promise<string[]> {
    return [...(this.addRunnerSpyStatusUrls ?? [])];
  }

  async addRunnerStatusSpyStop(): Promise<void> {
    this.addRunnerSpyStatusUrls = null;
    await this.addRunnerSpyMaybeUninstall();
  }

  async addRunnerCommandText(): Promise<string | null> {
    if ((await this.addRunnerLayout()) !== "command") return null;
    const pre = this.addRunnerDialog().locator("pre").first();
    if ((await pre.count()) === 0) return null;
    return ((await pre.textContent().catch(() => "")) ?? "") as string | null;
  }

  async addRunnerCommandHeader(): Promise<string | null> {
    if ((await this.addRunnerLayout()) !== "command") return null;
    const header = this.addRunnerDialog().locator("p").filter({ hasText: "Project" }).first();
    if ((await header.count()) === 0) return null;
    return (((await header.innerText().catch(() => "")) ?? "").trim() || null) as string | null;
  }

  /** The shell tab strip next to the "Shell" caption. */
  private addRunnerShellStrip(): Locator {
    return this.addRunnerDialog()
      .getByText("Shell", { exact: true })
      .locator("xpath=following-sibling::div[1]")
      .getByRole("button");
  }

  async addRunnerShellOptions(): Promise<string[]> {
    const labels = await this.addRunnerShellStrip().allInnerTexts();
    return labels.map((label) => label.trim());
  }

  async addRunnerActiveShell(): Promise<string | null> {
    const pressed = this.addRunnerDialog().locator('button[aria-pressed="true"]').first();
    if ((await pressed.count()) === 0) return null;
    return (((await pressed.innerText().catch(() => "")) ?? "").trim() || null) as string | null;
  }

  async addRunnerPickShell(label: string): Promise<void> {
    // The strip is already a button locator: filter it, never chain a
    // second getByRole (buttons contain no nested buttons).
    const exact = new RegExp(`^${WebDriver.addRunnerEscapeRegExp(label)}$`);
    await this.addRunnerShellStrip().filter({ hasText: exact }).click({ timeout: WebDriver.WAIT_MS });
  }

  private addRunnerCopyButton(): Locator {
    return this.addRunnerDialog().getByRole("button", { name: /^(Copy command|Copied!)$/ });
  }

  async addRunnerCopy(): Promise<void> {
    // The panel writes through the async clipboard API, which headless
    // Chromium denies without an explicit grant.
    await this.page.context().grantPermissions(["clipboard-read", "clipboard-write"]);
    await this.addRunnerCopyButton().first().click({ timeout: WebDriver.WAIT_MS });
  }

  async addRunnerCopyState(): Promise<string | null> {
    const button = this.addRunnerCopyButton().first();
    if ((await button.count()) === 0) return null;
    return (((await button.innerText().catch(() => "")) ?? "").trim() || null) as string | null;
  }

  async addRunnerReadClipboard(): Promise<string> {
    return this.readClipboard();
  }

  async addRunnerBreakClipboard(): Promise<void> {
    await this.page.evaluate(() => {
      window.Clipboard.prototype.writeText = () => Promise.reject(new Error("parity: clipboard blocked"));
    });
  }

  async addRunnerOriginNote(): Promise<string | null> {
    const note = this.addRunnerDialog().getByText("Using the current browser origin", { exact: false });
    if ((await note.count()) === 0) return null;
    return ((
      (await note
        .first()
        .innerText()
        .catch(() => "")) ?? ""
    ).trim() || null) as string | null;
  }

  async addRunnerCommandBack(): Promise<void> {
    await this.addRunnerDialog()
      .getByRole("button", { name: "Back", exact: true })
      .click({ timeout: WebDriver.WAIT_MS });
  }

  async addRunnerLastToast(): Promise<string | null> {
    return this.lastToast();
  }

  // --- Assistant chat core (NEWFRONT-187, AGT-038–049, AGT-053–057). ----

  /** The assistant layout's own sidebar (not the workspace shell nav). */
  private assistantSidebar(): Locator {
    return this.page.locator('aside[class*="w-[280px]"]');
  }

  /** The centered thread column: transcript, inline error, composer. */
  private assistantThreadColumn(): Locator {
    return this.page.locator("div.mx-auto.w-full.max-w-3xl");
  }

  /** The centered landing column: greeting or setup card, plus composer. */
  private assistantLandingColumn(): Locator {
    return this.page.locator("div.m-auto.w-full.max-w-2xl");
  }

  /** The composer textarea (landing and thread share one placeholder). */
  private assistantComposerBox(): Locator {
    return this.page.locator('textarea[placeholder*="Ask Pi Dash to do something"]');
  }

  /** The composer button row the textarea sits in (send/stop/mic host). */
  private assistantComposerRow(): Locator {
    return this.assistantComposerBox().locator("xpath=..");
  }

  /**
   * The transcript list once rows render (absent on the empty state). Exact
   * class match: the setup card shares the flex-col/gap-3 shape but carries
   * its own card classes.
   */
  private assistantListColumn(): Locator {
    return this.assistantThreadColumn().locator(
      'div.min-h-0.flex-1.overflow-auto.py-4 > div[class="flex flex-col gap-3"]'
    );
  }

  /** The dashboard card input; absent exactly when the card hides. */
  private assistantHomeCardInput(): Locator {
    return this.page.locator('input[placeholder*="Ask Pi Dash to do something"]');
  }

  /** The dashboard card root (the input row's parent). */
  private assistantHomeCard(): Locator {
    return this.assistantHomeCardInput().locator("xpath=../..");
  }

  async assistantOpenLanding(workspaceSlug: string): Promise<void> {
    await this.page.goto(`/${workspaceSlug}/assistant`);
    await this.page.waitForLoadState("domcontentloaded");
    // The greeting/setup swap rides on the config read, so settle on the
    // column itself; scenarios poll for the stable branch.
    await this.waitForContent("assistant landing", () =>
      this.assistantLandingColumn().first().waitFor({ timeout: 60_000 })
    );
  }

  async assistantOpenThread(workspaceSlug: string, threadId: string): Promise<void> {
    await this.page.goto(`/${workspaceSlug}/assistant/${threadId}`);
    await this.page.waitForLoadState("domcontentloaded");
    await this.waitForContent("assistant thread", () =>
      this.assistantThreadColumn().first().waitFor({ timeout: 60_000 })
    );
  }

  async assistantOpenHome(workspaceSlug: string): Promise<void> {
    await this.page.goto(`/${workspaceSlug}`);
    await this.page.waitForLoadState("domcontentloaded");
    await this.waitForContent("assistant dashboard", () =>
      this.page.getByRole("main").first().waitFor({ timeout: 60_000 })
    );
  }

  async assistantCurrentPath(): Promise<string> {
    return new URL(this.page.url()).pathname;
  }

  async assistantGoBack(): Promise<void> {
    await this.page.goBack();
    await this.page.waitForLoadState("domcontentloaded");
  }

  async assistantLandingGreeting(): Promise<AssistantLandingGreeting | null> {
    const headline = this.assistantLandingColumn().locator("h1").first();
    if ((await headline.count()) === 0) return null;
    const caption = this.assistantLandingColumn().locator("p").first();
    return {
      headline: (await headline.innerText()).trim(),
      caption: (await caption.count()) === 0 ? "" : (await caption.innerText()).trim(),
    };
  }

  async assistantLandingComposerVisible(): Promise<boolean> {
    return this.assistantLandingColumn()
      .locator('textarea[placeholder*="Ask Pi Dash to do something"]')
      .first()
      .isVisible()
      .then(
        (visible) => visible,
        () => false
      );
  }

  async assistantSetupCard(): Promise<{ title: string; body: string; button: string } | null> {
    const title = this.page.getByText("Set up your AI assistant", { exact: true }).first();
    if ((await title.count()) === 0) return null;
    const card = title.locator("xpath=..");
    return {
      title: (await title.innerText()).trim(),
      body: ((await card.locator("p").first().innerText()).trim() ?? "").trim(),
      button: (await card.getByRole("button").first().innerText()).trim(),
    };
  }

  async assistantSetupCardClick(): Promise<void> {
    const title = this.page.getByText("Set up your AI assistant", { exact: true }).first();
    await title.locator("xpath=..").getByRole("button").first().click();
  }

  async assistantFillDraft(text: string): Promise<void> {
    await this.assistantComposerBox().first().fill(text);
  }

  async assistantDraftValue(): Promise<string> {
    return this.assistantComposerBox().first().inputValue();
  }

  async assistantPressEnter(): Promise<void> {
    await this.assistantComposerBox().first().press("Enter");
  }

  async assistantPressShiftEnter(): Promise<void> {
    await this.assistantComposerBox().first().press("Shift+Enter");
  }

  async assistantPressControlEnter(): Promise<void> {
    await this.assistantComposerBox().first().press("Control+Enter");
  }

  async assistantSendVisible(): Promise<boolean> {
    return this.assistantComposerRow()
      .locator("button:has(svg.lucide-send)")
      .first()
      .isVisible()
      .then(
        (visible) => visible,
        () => false
      );
  }

  async assistantSendEnabled(): Promise<boolean> {
    return this.assistantComposerRow()
      .locator("button:has(svg.lucide-send)")
      .first()
      .isEnabled()
      .then(
        (enabled) => enabled,
        () => false
      );
  }

  async assistantClickSend(): Promise<void> {
    await this.assistantComposerRow().locator("button:has(svg.lucide-send)").first().click();
  }

  async assistantStopVisible(): Promise<boolean> {
    return this.assistantComposerRow()
      .locator("button:has(svg.lucide-square)")
      .first()
      .isVisible()
      .then(
        (visible) => visible,
        () => false
      );
  }

  async assistantClickStop(): Promise<void> {
    await this.assistantComposerRow().locator("button:has(svg.lucide-square)").first().click();
  }

  async assistantComposerReason(): Promise<string | null> {
    const reason = this.assistantComposerBox()
      .locator("xpath=../..")
      .locator("div.mb-2.text-12.text-secondary")
      .first();
    if ((await reason.count()) === 0) return null;
    const text = (await reason.innerText()).trim();
    return text === "" ? null : text;
  }

  async assistantTextareaDisabled(): Promise<boolean> {
    return this.assistantComposerBox().first().isDisabled();
  }

  async assistantErrorLine(): Promise<string | null> {
    const line = this.assistantThreadColumn().locator("div.text-danger.mb-2.text-12").first();
    if ((await line.count()) === 0) return null;
    const text = (await line.innerText()).trim();
    return text === "" ? null : text;
  }

  async assistantMicLabel(): Promise<string | null> {
    const mic = this.assistantComposerRow().locator("button:has(svg.lucide-mic)").first();
    if ((await mic.count()) === 0) return null;
    return mic.getAttribute("aria-label");
  }

  async assistantClickMic(): Promise<void> {
    await this.assistantComposerRow().locator("button:has(svg.lucide-mic)").first().click();
  }

  async assistantDictationHint(): Promise<string | null> {
    // The lockdown line shares the mb-2/text-12 wrapper shape, so it is
    // excluded by its own marker class.
    const hint = this.assistantComposerBox()
      .locator("xpath=../..")
      .locator("div.mb-2.text-12:not(.text-secondary)")
      .first();
    if ((await hint.count()) === 0) return null;
    const text = (await hint.innerText()).trim();
    return text === "" ? null : text;
  }

  async assistantBubbles(): Promise<AssistantBubble[]> {
    const column = this.assistantListColumn();
    if ((await column.count()) === 0) return [];
    return column.evaluate((element) => {
      const rows: { role: string; text: string }[] = [];
      for (const child of Array.from(element.children)) {
        const node = child as HTMLElement;
        const text = (node.innerText ?? "").trim();
        // The bottom scroll anchor carries no text.
        if (text === "") continue;
        if (node.querySelector(".justify-end") !== null) rows.push({ role: "user", text });
        else if (node.querySelector("svg.lucide-plug-zap") !== null) rows.push({ role: "notice", text });
        else if (node.querySelector("svg.lucide-wrench") !== null) rows.push({ role: "tool", text });
        else if (node.querySelector(".justify-start") !== null) rows.push({ role: "assistant", text });
        else if ((node.firstElementChild as HTMLElement | null)?.className.includes("text-danger") === true) {
          rows.push({ role: "error", text });
        } else rows.push({ role: "unknown", text });
      }
      return rows;
    });
  }

  async assistantBubbleHtml(index: number): Promise<string> {
    return this.assistantListColumn().locator(":scope > div").nth(index).innerHTML();
  }

  async assistantToolActivities(): Promise<AssistantToolActivity[]> {
    const rows = this.assistantListColumn().locator(":scope > div:has(svg.lucide-wrench)");
    const total = await rows.count();
    const out: AssistantToolActivity[] = [];
    for (let i = 0; i < total; i++) {
      const row = rows.nth(i);
      const text = (await row.locator(":scope span").first().innerText()).trim();
      const links = row.locator(":scope a");
      const linkTotal = await links.count();
      const items: { label: string; href: string }[] = [];
      for (let j = 0; j < linkTotal; j++) {
        const link = links.nth(j);
        items.push({
          label: (await link.innerText()).trim(),
          href: (await link.getAttribute("href")) ?? "",
        });
      }
      out.push({ text, links: items });
    }
    return out;
  }

  async assistantNoticeLines(): Promise<string[]> {
    const lines = this.assistantListColumn().locator(":scope > div:has(svg.lucide-plug-zap) span");
    const total = await lines.count();
    const out: string[] = [];
    for (let i = 0; i < total; i++) {
      const text = (await lines.nth(i).innerText()).trim();
      if (text !== "") out.push(text);
    }
    return out;
  }

  async assistantEmptyState(): Promise<string | null> {
    if ((await this.assistantListColumn().count()) > 0) return null;
    const slot = this.assistantThreadColumn().locator("div.min-h-0.flex-1.overflow-auto.py-4").first();
    if ((await slot.count()) === 0) return null;
    const text = (await slot.innerText()).trim();
    return text === "" ? null : text;
  }

  async assistantIsScrolledToBottom(): Promise<boolean> {
    const slot = this.assistantThreadColumn().locator("div.min-h-0.flex-1.overflow-auto.py-4").first();
    if ((await slot.count()) === 0) return false;
    return slot.evaluate((element) => {
      const node = element as HTMLElement;
      // The auto-scroll anchors the tail marker to the scrollport's end,
      // which leaves the slot's own bottom padding (~16px) visible below
      // the newest row; anything within that is "following the tail".
      return Math.abs(node.scrollHeight - node.scrollTop - node.clientHeight) < 20;
    });
  }

  async assistantClickToolLink(activityIndex: number, linkIndex: number): Promise<void> {
    await this.assistantListColumn()
      .locator(":scope > div:has(svg.lucide-wrench)")
      .nth(activityIndex)
      .locator(":scope a")
      .nth(linkIndex)
      .click();
  }

  async assistantSidebarThreads(): Promise<AssistantSidebarThread[]> {
    const links = this.assistantSidebar().locator("nav a");
    const total = await links.count();
    const out: AssistantSidebarThread[] = [];
    for (let i = 0; i < total; i++) {
      const link = links.nth(i);
      out.push({
        title: (await link.innerText()).trim(),
        href: (await link.getAttribute("href")) ?? "",
        active: (await link.getAttribute("aria-current")) === "page",
      });
    }
    return out;
  }

  async assistantSidebarEmptyVisible(): Promise<boolean> {
    return this.assistantSidebar()
      .getByText("No conversations yet.", { exact: true })
      .first()
      .isVisible()
      .then(
        (visible) => visible,
        () => false
      );
  }

  async assistantClickNewChat(): Promise<void> {
    await this.assistantSidebar().getByRole("link", { name: "New chat" }).click();
  }

  async assistantClickSidebarThread(index: number): Promise<void> {
    await this.assistantSidebar().locator("nav a").nth(index).click();
  }

  async assistantCardVisible(): Promise<boolean> {
    return (await this.assistantHomeCardInput().count()) > 0;
  }

  async assistantCardFillDraft(text: string): Promise<void> {
    await this.assistantHomeCardInput().first().fill(text);
  }

  async assistantCardDraftValue(): Promise<string> {
    return this.assistantHomeCardInput().first().inputValue();
  }

  async assistantCardPressEnter(): Promise<void> {
    await this.assistantHomeCardInput().first().press("Enter");
  }

  async assistantCardAskDisabled(): Promise<boolean> {
    return this.assistantHomeCard().getByRole("button", { name: "Ask", exact: true }).first().isDisabled();
  }

  async assistantCardClickAsk(): Promise<void> {
    await this.assistantHomeCard().getByRole("button", { name: "Ask", exact: true }).first().click();
  }

  async assistantCardClickSuggestion(text: string): Promise<void> {
    await this.assistantHomeCard().getByRole("button", { name: text, exact: true }).first().click();
  }

  async assistantCardSuggestions(): Promise<string[]> {
    if (!(await this.assistantCardVisible())) return [];
    const texts = await this.assistantHomeCard().getByRole("button").allTextContents();
    return texts.map((text) => text.trim()).filter((text) => text.length > 0 && text !== "Ask");
  }

  async assistantCardRecents(): Promise<{ title: string; href: string }[]> {
    if (!(await this.assistantCardVisible())) return [];
    const links = this.assistantHomeCard().locator("a");
    const total = await links.count();
    const out: { title: string; href: string }[] = [];
    for (let i = 0; i < total; i++) {
      const link = links.nth(i);
      out.push({
        title: (await link.innerText()).trim(),
        href: (await link.getAttribute("href")) ?? "",
      });
    }
    return out;
  }

  async assistantCardClickRecent(index: number): Promise<void> {
    await this.assistantHomeCard().locator("a").nth(index).click();
  }

  async assistantStubStream(threadId: string, frames: AssistantStreamFrame[]): Promise<void> {
    this.assistantStreamFrames.set(threadId, frames);
    if (!this.assistantStreamUrls.has(threadId)) {
      this.assistantStreamUrls.set(threadId, []);
      // Installed once per thread; re-stubbing swaps the frames the
      // handler serves, so the stream's natural reconnect picks them up.
      await this.page.route(`**/ai-assistant/threads/${threadId}/events**`, async (route) => {
        this.assistantStreamUrls.get(threadId)?.push(route.request().url());
        const stamp = new Date().toISOString();
        const body = (this.assistantStreamFrames.get(threadId) ?? [])
          .map((frame) => {
            const event = {
              thread: threadId,
              message: frame.message ?? null,
              seq: frame.seq,
              kind: frame.kind,
              payload: frame.payload,
              created_at: stamp,
            };
            return `event: chat.event\ndata: ${JSON.stringify(event)}\n\n`;
          })
          .join("");
        await route.fulfill({
          status: 200,
          headers: { "content-type": "text/event-stream", "cache-control": "no-cache" },
          body,
        });
      });
    }
  }

  async assistantClearStreamStub(threadId: string): Promise<void> {
    this.assistantStreamFrames.delete(threadId);
    await this.page.unroute(`**/ai-assistant/threads/${threadId}/events**`);
  }

  async assistantStreamRequestUrls(threadId: string): Promise<string[]> {
    return [...(this.assistantStreamUrls.get(threadId) ?? [])];
  }

  async assistantBlockStream(threadId: string): Promise<void> {
    if (!this.assistantStreamUrls.has(threadId)) {
      this.assistantStreamUrls.set(threadId, []);
    }
    await this.page.route(`**/ai-assistant/threads/${threadId}/events**`, async (route) => {
      this.assistantStreamUrls.get(threadId)?.push(route.request().url());
      await route.abort("failed");
    });
  }

  async assistantClearStreamBlock(threadId: string): Promise<void> {
    await this.page.unroute(`**/ai-assistant/threads/${threadId}/events**`);
  }

  async assistantStartApiSpy(): Promise<void> {
    this.assistantSpyCounts = { threadCreate: 0, send: 0, cancel: 0, threadList: 0, messageList: 0 };
    // One classifying route: counts the call, then falls through so the
    // request still reaches the server (or a later-registered stub).
    await this.page.route("**/api/workspaces/*/ai-assistant/**", async (route) => {
      const request = route.request();
      const pathname = new URL(request.url()).pathname;
      const method = request.method();
      const counts = this.assistantSpyCounts;
      if (method === "POST" && pathname.endsWith("/ai-assistant/threads/")) counts.threadCreate += 1;
      else if (method === "POST" && pathname.endsWith("/messages/")) counts.send += 1;
      else if (method === "POST" && pathname.endsWith("/cancel/")) counts.cancel += 1;
      else if (method === "GET" && pathname.endsWith("/ai-assistant/threads/")) counts.threadList += 1;
      else if (method === "GET" && pathname.endsWith("/messages/")) counts.messageList += 1;
      await route.fallback();
    });
  }

  async assistantApiCounts(): Promise<AssistantApiCounts> {
    return { ...this.assistantSpyCounts };
  }

  async assistantStopApiSpy(): Promise<void> {
    await this.page.unroute("**/api/workspaces/*/ai-assistant/**");
  }

  async assistantFailThreadCreateOnce(): Promise<void> {
    this.assistantThreadCreateFailRemaining = 1;
    await this.page.route("**/api/workspaces/*/ai-assistant/threads/", async (route) => {
      const request = route.request();
      if (request.method() === "POST" && this.assistantThreadCreateFailRemaining > 0) {
        this.assistantThreadCreateFailRemaining -= 1;
        await route.fulfill({
          status: 500,
          contentType: "application/json",
          body: JSON.stringify({ error: "parity thread-create failure" }),
        });
        return;
      }
      await route.fallback();
    });
  }

  async assistantDelayThreadCreate(ms: number): Promise<void> {
    this.assistantThreadCreateDelayMs = ms;
    this.assistantThreadCreateDelayRemaining = 1;
    await this.page.route("**/api/workspaces/*/ai-assistant/threads/", async (route) => {
      const request = route.request();
      const pathname = new URL(request.url()).pathname;
      if (
        request.method() === "POST" &&
        pathname.endsWith("/ai-assistant/threads/") &&
        this.assistantThreadCreateDelayRemaining > 0
      ) {
        this.assistantThreadCreateDelayRemaining -= 1;
        await new Promise((resolve) => setTimeout(resolve, this.assistantThreadCreateDelayMs));
      }
      await route.fallback();
    });
  }

  async assistantClearThreadCreateStubs(): Promise<void> {
    this.assistantThreadCreateFailRemaining = 0;
    this.assistantThreadCreateDelayRemaining = 0;
    await this.page.unroute("**/api/workspaces/*/ai-assistant/threads/");
  }

  async assistantDelaySend(ms: number): Promise<void> {
    this.assistantSendDelayMs = ms;
    this.assistantSendDelayRemaining = 1;
    await this.page.route("**/api/workspaces/*/ai-assistant/threads/*/messages/", async (route) => {
      const request = route.request();
      if (request.method() === "POST" && this.assistantSendDelayRemaining > 0) {
        this.assistantSendDelayRemaining -= 1;
        await new Promise((resolve) => setTimeout(resolve, this.assistantSendDelayMs));
      }
      await route.fallback();
    });
  }

  async assistantClearSendDelay(): Promise<void> {
    this.assistantSendDelayRemaining = 0;
    await this.page.unroute("**/api/workspaces/*/ai-assistant/threads/*/messages/");
  }

  async assistantLastToast(): Promise<{ title: string; message: string } | null> {
    // Toasts stack bottom-right and auto-dismiss; only a currently
    // visible one with text is reported, newest first.
    const roots = this.page.locator("div.absolute.right-3.bottom-3");
    const total = await roots.count();
    for (let i = total - 1; i >= 0; i--) {
      const text = (await roots.nth(i).innerText()).trim();
      if (text === "") continue;
      const lines = text
        .split("\n")
        .map((line) => line.trim())
        .filter((line) => line.length > 0);
      return { title: lines[0] ?? "", message: lines.slice(1).join(" ") };
    }
    return null;
  }

  // -- Assistant voice/keys/tools/negatives/editor (NEWFRONT-188) --------

  async assistantCurrentHash(): Promise<string> {
    return new URL(this.page.url()).hash;
  }

  private assistantMicButton(): Locator {
    return this.assistantComposerRow().locator("button:has(svg.lucide-mic)").first();
  }

  async assistantMicDisabled(): Promise<boolean> {
    return this.assistantMicButton().isDisabled();
  }

  async assistantMicHold(ms: number): Promise<void> {
    // Push-to-talk rides on pointer down/up (a click's fast up lands under
    // the tap floor and discards), so hold through the mouse explicitly.
    const mic = this.assistantMicButton();
    await mic.hover();
    await this.page.mouse.down();
    await this.page.waitForTimeout(ms);
    await this.page.mouse.up();
  }

  async assistantMicDown(): Promise<void> {
    await this.assistantMicButton().hover();
    await this.page.mouse.down();
  }

  async assistantMicUp(): Promise<void> {
    await this.page.mouse.up();
  }

  async assistantMicUpAfter(ms: number): Promise<void> {
    // The tap floor counts from recorder start, so a down/poll/up cycle
    // can release inside it; holding past the observation clears it.
    await this.page.waitForTimeout(ms);
    await this.page.mouse.up();
  }

  async assistantSetMicrophonePermission(state: "granted" | "denied"): Promise<void> {
    if (state === "granted") await this.page.context().grantPermissions(["microphone"]);
    else await this.page.context().clearPermissions();
  }

  async assistantSimulateUnsupportedCapture(): Promise<void> {
    // A capture-less browser: the composer hides the mic and all hints.
    await this.page.addInitScript(() => {
      Object.defineProperty(window, "MediaRecorder", { value: undefined, configurable: true });
      Object.defineProperty(navigator, "mediaDevices", { value: undefined, configurable: true });
    });
  }

  async assistantSimulateMicDenial(): Promise<void> {
    // Headless prompts never surface a real NotAllowedError, so reject
    // with the spec's denial name and prove the hook's mapping of it.
    await this.page.addInitScript(() => {
      const media = navigator.mediaDevices;
      if (media === undefined) return;
      const denial = () => Promise.reject(Object.assign(new Error("Permission denied"), { name: "NotAllowedError" }));
      try {
        Object.defineProperty(media, "getUserMedia", { value: denial, configurable: true });
      } catch {
        media.getUserMedia = denial;
      }
    });
  }

  async assistantStubTranscribeText(text: string): Promise<void> {
    this.assistantTranscribeTextStub = text;
    this.assistantTranscribeFailure = null;
    await this.routeTranscribeStub();
  }

  async assistantFailTranscribe(status: number, body: Record<string, string>): Promise<void> {
    this.assistantTranscribeTextStub = null;
    this.assistantTranscribeFailure = { status, body };
    await this.routeTranscribeStub();
  }

  private async routeTranscribeStub(): Promise<void> {
    await this.page.unroute("**/api/users/me/ai-assistant/transcribe/");
    await this.page.route("**/api/users/me/ai-assistant/transcribe/", async (route) => {
      const request = route.request();
      if (request.method() !== "POST") {
        await route.fallback();
        return;
      }
      const buffer = request.postDataBuffer();
      const raw = buffer === null ? "" : buffer.toString("latin1");
      this.assistantTranscribeSeen.push({
        contentType: (await request.headerValue("content-type")) ?? "",
        hasFilePart: raw.includes("filename="),
        byteLength: buffer === null ? 0 : buffer.length,
      });
      const failure = this.assistantTranscribeFailure;
      if (failure !== null) {
        await route.fulfill({
          status: failure.status,
          contentType: "application/json",
          body: JSON.stringify(failure.body),
        });
        return;
      }
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({ text: this.assistantTranscribeTextStub ?? "" }),
      });
    });
  }

  async assistantClearTranscribeStubs(): Promise<void> {
    this.assistantTranscribeTextStub = null;
    this.assistantTranscribeFailure = null;
    await this.page.unroute("**/api/users/me/ai-assistant/transcribe/");
  }

  async assistantTranscribeRequests(): Promise<{ contentType: string; hasFilePart: boolean; byteLength: number }[]> {
    return [...this.assistantTranscribeSeen];
  }

  async assistantSidebarButtons(): Promise<string[]> {
    const sidebar = this.assistantSidebar();
    if ((await sidebar.count()) === 0) return [];
    return sidebar.locator("button").allInnerTexts();
  }

  async assistantThreadManagementControls(): Promise<string[]> {
    // The assistant layout (sidebar + outlet) must offer no thread
    // rename/archive/delete affordance; the report names any violator.
    const layout = this.page.locator("div.flex.h-full.w-full.overflow-hidden").first();
    if ((await layout.count()) === 0) return [];
    const candidates = layout.locator("button, a");
    const total = await candidates.count();
    const hits: string[] = [];
    for (let i = 0; i < total; i += 1) {
      const text = ((await candidates.nth(i).textContent()) ?? "").trim().toLowerCase();
      if (text.includes("rename") || text.includes("archive") || text.includes("delete")) hits.push(text);
    }
    return hits;
  }

  async assistantSidebarHeader(): Promise<string | null> {
    const header = this.assistantSidebar().locator("div.h-12").first();
    if ((await header.count()) === 0) return null;
    // textContent: innerText would apply CSS text transforms.
    const text = ((await header.textContent()) ?? "").trim();
    return text === "" ? null : text;
  }

  async assistantSidebarRowKinds(): Promise<{ newChat: string | null; rows: string[] }> {
    const sidebar = this.assistantSidebar();
    if ((await sidebar.count()) === 0) return { newChat: null, rows: [] };
    const links = sidebar.locator(":scope a, :scope button");
    const total = await links.count();
    let newChat: string | null = null;
    const rows: string[] = [];
    for (let i = 0; i < total; i += 1) {
      const node = links.nth(i);
      const tag = await node.evaluate((element) => element.tagName.toLowerCase());
      const text = ((await node.textContent()) ?? "").trim();
      if (text === "New chat") newChat = tag;
      else rows.push(tag);
    }
    return { newChat, rows };
  }

  async assistantSkippedNoticeActions(): Promise<{ kind: string; text: string; href: string | null }[]> {
    const column = this.assistantThreadColumn();
    if ((await column.count()) === 0) return [];
    // One icon per notice; its parent div is the notice root.
    const icons = column.locator("svg.lucide-plug-zap");
    const actions: { kind: string; text: string; href: string | null }[] = [];
    const total = await icons.count();
    for (let i = 0; i < total; i += 1) {
      const interactive = icons.nth(i).locator("xpath=..").locator("a, button");
      const count = await interactive.count();
      for (let j = 0; j < count; j += 1) {
        const node = interactive.nth(j);
        actions.push({
          kind: await node.evaluate((element) => element.tagName.toLowerCase()),
          text: ((await node.textContent()) ?? "").trim(),
          href: await node.getAttribute("href"),
        });
      }
    }
    return actions;
  }

  async assistantChatSettingsLinks(): Promise<string[]> {
    // Registry chrome lives in settings: the chat surface must not link
    // there for tool servers (the dictation hint may link while unsetup).
    const layout = this.page.locator("div.flex.h-full.w-full.overflow-hidden").first();
    if ((await layout.count()) === 0) return [];
    const links = layout.locator('a[href*="/settings"]');
    const total = await links.count();
    const out: string[] = [];
    for (let i = 0; i < total; i += 1) out.push((await links.nth(i).getAttribute("href")) ?? "");
    return out;
  }

  async assistantStartDesktopCallWatch(): Promise<void> {
    await this.assistantStopDesktopCallWatch();
    this.assistantDesktopCalls = [];
    const handler = (request: { method(): string; url(): string }) => {
      const url = request.url();
      if (url.includes("/ai-assistant/agent-profile/") || url.includes("/ai-assistant/agent-token/")) {
        this.assistantDesktopCalls.push({ method: request.method(), url });
      }
    };
    this.assistantDesktopWatchHandler = handler;
    this.page.on("request", handler as (request: never) => void);
  }

  async assistantDesktopCallsObserved(): Promise<{ method: string; url: string }[]> {
    return [...this.assistantDesktopCalls];
  }

  async assistantStopDesktopCallWatch(): Promise<void> {
    if (this.assistantDesktopWatchHandler !== null) {
      this.page.off("request", this.assistantDesktopWatchHandler as (request: never) => void);
      this.assistantDesktopWatchHandler = null;
    }
  }

  async assistantStubInstanceLlm(configured: boolean): Promise<void> {
    this.assistantInstanceLlmStub = configured;
    // Eagerly read the real payload once (same origin, same session) and
    // serve the patched copy: patching inside the handler stalls app boot.
    const live = await this.page.request.get("/api/instances/");
    const payload = (await live.json()) as Record<string, unknown>;
    // The app reads the nested config object, not the top level.
    const nested = payload["config"] as Record<string, unknown> | undefined;
    if (nested !== undefined) nested["has_llm_configured"] = configured;
    else payload["has_llm_configured"] = configured;
    const status = live.status();
    await this.page.unroute("**/api/instances/");
    await this.page.route("**/api/instances/", async (route) => {
      if (route.request().method() !== "GET") {
        await route.fallback();
        return;
      }
      await route.fulfill({ status, contentType: "application/json", json: payload });
    });
  }

  async assistantClearInstanceStub(): Promise<void> {
    this.assistantInstanceLlmStub = null;
    await this.page.unroute("**/api/instances/");
  }

  async assistantStubGptAnswer(response: { response: string; response_html: string }): Promise<void> {
    this.assistantGptAnswerStub = response;
    this.assistantGptFailure = null;
    await this.routeGptStub();
  }

  async assistantFailGptAnswer(status: number, body: Record<string, string>): Promise<void> {
    this.assistantGptAnswerStub = null;
    this.assistantGptFailure = { status, body };
    await this.routeGptStub();
  }

  private async routeGptStub(): Promise<void> {
    // Exact-path glob: thread/Message routes share the workspace prefix
    // but carry more segments, so only the editor endpoint matches.
    await this.page.unroute("**/api/workspaces/*/ai-assistant/");
    await this.page.route("**/api/workspaces/*/ai-assistant/", async (route) => {
      const request = route.request();
      if (request.method() !== "POST") {
        await route.fallback();
        return;
      }
      const body = (request.postDataJSON() ?? {}) as { prompt?: unknown; task?: unknown };
      this.assistantGptSeen.push({
        prompt: typeof body["prompt"] === "string" ? body["prompt"] : "",
        task: typeof body["task"] === "string" ? body["task"] : "",
      });
      const failure = this.assistantGptFailure;
      if (failure !== null) {
        await route.fulfill({
          status: failure.status,
          contentType: "application/json",
          body: JSON.stringify(failure.body),
        });
        return;
      }
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify(this.assistantGptAnswerStub ?? { response: "", response_html: "" }),
      });
    });
  }

  async assistantClearGptStubs(): Promise<void> {
    this.assistantGptAnswerStub = null;
    this.assistantGptFailure = null;
    await this.page.unroute("**/api/workspaces/*/ai-assistant/");
  }

  async assistantGptRequests(): Promise<{ prompt: string; task: string }[]> {
    return [...this.assistantGptSeen];
  }

  private issueModalAiPanel(): Locator {
    // The helper panel is the wide fixed popover hosting the task box.
    return this.page.locator("div.min-w-\\[50rem\\]").first();
  }

  async issueModalAiEntryVisible(): Promise<boolean> {
    const entry = this.modalScope().getByRole("button", { name: "AI", exact: true });
    return (await entry.count()) > 0 && (await entry.first().isVisible());
  }

  async issueModalAiOpen(): Promise<void> {
    await this.modalScope().getByRole("button", { name: "AI", exact: true }).first().click();
    await this.waitForContent("AI helper popover", () =>
      this.page.locator("input#task").first().waitFor({ timeout: WebDriver.OPEN_MS })
    );
  }

  async issueModalAiFillTask(text: string): Promise<void> {
    await this.page.locator("input#task").first().fill(text);
  }

  async issueModalAiGenerate(): Promise<void> {
    await this.issueModalAiPanel()
      .getByRole("button", { name: /Generate (response|again)/ })
      .first()
      .click();
  }

  async issueModalAiResponse(): Promise<string | null> {
    const review = this.issueModalAiPanel().locator("div.page-block-section").first();
    if ((await review.count()) === 0) return null;
    const text = (await review.innerText()).trim().replace(/^Response:\s*/, "");
    return text === "" ? null : text;
  }

  async issueModalAiInvalidVisible(): Promise<boolean> {
    const invalid = this.issueModalAiPanel().getByText("No response could be generated.", { exact: false });
    return (await invalid.count()) > 0 && (await invalid.first().isVisible());
  }

  async issueModalAiUseResponse(): Promise<void> {
    await this.issueModalAiPanel().getByRole("button", { name: "Use this response" }).first().click();
  }

  async issueModalAiClose(): Promise<void> {
    await this.issueModalAiPanel().getByRole("button", { name: "Close" }).first().click();
  }

  async issueModalDescriptionText(): Promise<string | null> {
    const editor = this.modalScope().locator('[contenteditable="true"]').first();
    if ((await editor.count()) === 0) return null;
    return ((await editor.textContent()) ?? "").trim();
  }

  async pageEditorOpen(workspaceSlug: string, projectId: string, pageId: string): Promise<void> {
    // Watch the dead endpoint from navigation on: the menu posts here.
    this.pageEditorRephraseSeen = [];
    await this.page.unroute("**/api/workspaces/*/rephrase-grammar/");
    await this.page.route("**/api/workspaces/*/rephrase-grammar/", async (route) => {
      this.pageEditorRephraseSeen.push(route.request().url());
      await route.fallback();
    });
    await this.page.goto(`/${workspaceSlug}/projects/${projectId}/pages/${pageId}`);
    await this.page.waitForLoadState("domcontentloaded");
    await this.waitForContent("page editor", () =>
      this.page.locator(".frame-renderer [contenteditable='true']").first().waitFor({ timeout: 60_000 })
    );
  }

  async pageEditorAiHandleCount(): Promise<number> {
    // The side menu (with the AI handle, when enabled) reveals on block
    // hover; sweep the blocks, settle, then count what revealed.
    const blocks = this.page.locator(".frame-renderer [contenteditable='true'] p");
    const total = await blocks.count();
    for (let i = 0; i < Math.min(total, 5); i += 1) {
      await blocks.nth(i).hover();
      await this.page.waitForTimeout(500);
    }
    await this.page.waitForTimeout(2000);
    return this.page.locator("#ai-handle").count();
  }

  async pageEditorAiMenuVisible(): Promise<boolean> {
    const entry = this.page.getByRole("button", { name: "Ask Pi" });
    return (await entry.count()) > 0 && (await entry.first().isVisible());
  }

  async pageEditorRephraseRequests(): Promise<string[]> {
    return [...this.pageEditorRephraseSeen];
  }

  // --- Notifications inbox foundation (NEWFRONT-198, NTF-001..006).
  // --- Appended; existing methods above are untouched per the shared
  // --- driver contract. Selectors use the notifications test hooks the
  // --- area added to the old app plus structural reads within a card.

  private notificationsTabLocator(tab: NotificationsTab): Locator {
    return this.page.getByTestId(`notifications-tab-${tab}`);
  }

  private static cleanText(raw: string | null): string {
    return (raw ?? "").replace(/\s+/g, " ").trim();
  }

  private static badgeLike(token: string): boolean {
    return token !== "" && /^[\d@+,.kKmM]+$/.test(token);
  }

  async notificationsOpenInbox(workspaceSlug: string): Promise<void> {
    await this.page.goto(`/${workspaceSlug}/notifications/`, { timeout: 60_000 });
    await this.page.waitForLoadState("domcontentloaded");
    await this.notificationsTabLocator("all").first().waitFor({ timeout: WebDriver.WAIT_MS });
  }

  async notificationsListPaneVisible(): Promise<boolean> {
    const pane = this.page.getByTestId("notifications-list-pane").first();
    if ((await pane.count()) === 0) return false;
    return pane.isVisible().catch(() => false);
  }

  async notificationsDetailPaneVisible(): Promise<boolean> {
    const pane = this.page.getByTestId("notifications-detail-pane").first();
    if ((await pane.count()) === 0) return false;
    return pane.isVisible().catch(() => false);
  }

  async notificationsPaneWidths(): Promise<{ list: number; detail: number }> {
    const list = this.page.getByTestId("notifications-list-pane").first();
    const detail = this.page.getByTestId("notifications-detail-pane").first();
    const listBox = await list.boundingBox().catch(() => null);
    const detailBox = await detail.boundingBox().catch(() => null);
    return { list: listBox?.width ?? 0, detail: detailBox?.width ?? 0 };
  }

  async notificationsSelectCard(index: number): Promise<void> {
    const detail = this.page.getByTestId("notifications-detail-pane").first();
    const before = WebDriver.cleanText(await detail.innerText().catch(() => ""));
    const card = this.page.getByTestId("notification-card").nth(index);
    await card.scrollIntoViewIfNeeded().catch(() => undefined);
    await card.click({ timeout: WebDriver.WAIT_MS });
    // Selecting swaps the detail pane from its empty state to the issue
    // peek overview, so its text changing settles the selection.
    await this.page.waitForFunction(
      (expected: string) => {
        const pane = document.querySelector('[data-testid="notifications-detail-pane"]');
        const text = (pane?.textContent ?? "").replace(/\s+/g, " ").trim();
        return text !== expected;
      },
      before,
      { timeout: WebDriver.WAIT_MS }
    );
  }

  async notificationsTabNames(): Promise<string[]> {
    const tabs = this.page.getByTestId(/notifications-tab-(all|mentions)/);
    const count = await tabs.count();
    const names: string[] = [];
    for (let i = 0; i < count; i++) {
      // The badge (when present) renders after the label; strip it.
      const tokens = WebDriver.cleanText(
        await tabs
          .nth(i)
          .innerText()
          .catch(() => "")
      ).split(" ");
      const last = tokens[tokens.length - 1] ?? "";
      const labelTokens = tokens.length > 1 && WebDriver.badgeLike(last) ? tokens.slice(0, -1) : tokens;
      names.push(labelTokens.join(" "));
    }
    return names;
  }

  private notificationsTabUnderline(tab: NotificationsTab): Locator {
    // The underline marker renders only inside the active tab.
    return this.notificationsTabLocator(tab).first().locator("div.absolute.bottom-0");
  }

  async notificationsActiveTab(): Promise<NotificationsTab> {
    await this.page
      .locator('[data-testid^="notifications-tab-"] div.absolute.bottom-0')
      .first()
      .waitFor({ timeout: WebDriver.WAIT_MS });
    const tabs: NotificationsTab[] = ["all", "mentions"];
    for (const tab of tabs) {
      if ((await this.notificationsTabUnderline(tab).count()) > 0) return tab;
    }
    return "all";
  }

  async notificationsSelectTab(tab: NotificationsTab): Promise<void> {
    await this.notificationsTabLocator(tab).first().click({ timeout: WebDriver.WAIT_MS });
    await this.notificationsTabUnderline(tab).waitFor({ timeout: WebDriver.WAIT_MS });
  }

  async notificationsTabBadge(tab: NotificationsTab): Promise<string | null> {
    const tabRoot = this.notificationsTabLocator(tab).first();
    if ((await tabRoot.count()) === 0) return null;
    // Label and badge render as successive text runs; a lone run means no badge.
    const full = WebDriver.cleanText(await tabRoot.innerText().catch(() => ""));
    if (full === "") return null;
    const tokens = full.split(" ");
    if (tokens.length < 2) return null;
    const badge = tokens[tokens.length - 1] ?? "";
    return WebDriver.badgeLike(badge) ? badge : null;
  }

  async notificationsNavBadge(): Promise<string | null> {
    const badge = this.page.getByTestId("notifications-nav-badge").first();
    if ((await badge.count()) === 0) return null;
    if (!(await badge.isVisible().catch(() => false))) return null;
    const text = WebDriver.cleanText(await badge.innerText().catch(() => ""));
    return text === "" ? null : text;
  }

  async notificationsProjectNavBadge(
    workspaceSlug: string,
    projectId: string,
    cookies: ParityBrowserCookie[]
  ): Promise<string | null> {
    const unreadPath = `/api/workspaces/${workspaceSlug}/users/notifications/unread/`;
    const fetchWait = this.page
      .waitForResponse(
        (response) => response.request().method() === "GET" && response.url().includes(unreadPath) && response.ok(),
        { timeout: WebDriver.WAIT_MS }
      )
      .then(
        () => true,
        () => false
      );
    await this.openAuthenticated(`/${workspaceSlug}/projects/${projectId}/issues`, cookies);
    await fetchWait;
    return this.notificationsNavBadge();
  }

  async notificationsCards(): Promise<NotificationsCard[]> {
    const cards = this.page.getByTestId("notification-card");
    const count = await cards.count();
    const out: NotificationsCard[] = [];
    for (let i = 0; i < count; i++) {
      const card = cards.nth(i);
      const summaryRoot = card.getByTestId("notification-summary").first();
      const summary = WebDriver.cleanText(await summaryRoot.innerText().catch(() => ""));
      const actor = WebDriver.cleanText(
        await summaryRoot
          .locator("span")
          .first()
          .innerText()
          .catch(() => "")
      );
      const itemLine = WebDriver.cleanText(
        await card
          .getByTestId("notification-item-line")
          .first()
          .innerText()
          .catch(() => "")
      );
      const age = WebDriver.cleanText(
        await card
          .getByTestId("notification-age")
          .first()
          .innerText()
          .catch(() => "")
      );
      const unread = (await card.getByTestId("notification-unread-dot").count()) > 0;
      const firstSpace = itemLine.indexOf(" ");
      out.push({
        actor,
        summary,
        reference: firstSpace === -1 ? itemLine : itemLine.slice(0, firstSpace),
        title: firstSpace === -1 ? "" : itemLine.slice(firstSpace + 1),
        age,
        unread,
      });
    }
    return out;
  }

  async notificationsCardBackgrounds(): Promise<string[]> {
    const cards = this.page.getByTestId("notification-card");
    const count = await cards.count();
    const out: string[] = [];
    for (let i = 0; i < count; i++) {
      const background = await cards
        .nth(i)
        .evaluate((node) => window.getComputedStyle(node).backgroundColor)
        .catch(() => "");
      out.push(background ?? "");
    }
    return out;
  }

  async notificationsEntryFetches(workspaceSlug: string): Promise<{ list: boolean; unread: boolean }> {
    const listPath = `/api/workspaces/${workspaceSlug}/users/notifications`;
    const seen = { list: false, unread: false };
    const listWait = this.page
      .waitForResponse(
        (response) => {
          const url = response.url();
          return (
            response.request().method() === "GET" &&
            url.includes(listPath) &&
            !url.includes("/unread/") &&
            response.ok()
          );
        },
        { timeout: WebDriver.WAIT_MS }
      )
      .then(
        () => true,
        () => false
      );
    const unreadWait = this.page
      .waitForResponse(
        (response) =>
          response.request().method() === "GET" && response.url().includes(`${listPath}/unread/`) && response.ok(),
        { timeout: WebDriver.WAIT_MS }
      )
      .then(
        () => true,
        () => false
      );
    await this.page.goto(`/${workspaceSlug}/notifications/`, { timeout: 60_000 });
    await this.page.waitForLoadState("domcontentloaded");
    await this.notificationsTabLocator("all")
      .first()
      .waitFor({ timeout: WebDriver.WAIT_MS })
      .catch(() => undefined);
    seen.list = await listWait;
    seen.unread = await unreadWait;
    return seen;
  }

  // --- Desktop agent-runtime web-observable sides (NEWFRONT-207, DESK-001–010,
  // --- DESK-026). Appended; existing methods above are untouched per the
  // --- shared driver contract.

  async deskRuntimePageText(): Promise<string> {
    return (await this.page.locator("body").innerText()).trim();
  }

  async deskRuntimeDispatchWindowFocus(): Promise<void> {
    await this.page.evaluate(() => window.dispatchEvent(new FocusEvent("focus")));
  }

  async deskRuntimeIndexedDatabaseNames(): Promise<string[]> {
    return this.page.evaluate(async () => {
      const factory = window.indexedDB as unknown as
        | { databases?: () => Promise<{ name?: string | null }[]> }
        | undefined;
      if (typeof factory?.databases !== "function") return [];
      const infos = await factory.databases();
      return infos.map((info) => info.name ?? "").filter((name) => name !== "");
    });
  }

  // --- Notifications snooze + email preferences (NEWFRONT-201, NTF-020..022,
  // --- NTF-024..025). Selectors use the snooze/prefs test hooks this area
  // --- added to the old app; the portaled calendar and time options follow
  // --- the same structural reads the date-picker drivers already use.

  private notificationsCardAt(index: number): Locator {
    return this.page.getByTestId("notification-card").nth(index);
  }

  private async notificationsOpenSnoozePicker(index: number): Promise<Locator> {
    const card = this.notificationsCardAt(index);
    await card.scrollIntoViewIfNeeded().catch(() => undefined);
    // Card actions reveal on hover (NTF-023); the trigger is inert at rest.
    await card.hover({ timeout: WebDriver.WAIT_MS });
    await card.getByTestId("snooze-trigger").first().click({ timeout: WebDriver.WAIT_MS });
    const panel = card.getByTestId("snooze-panel").first();
    await panel.waitFor({ timeout: WebDriver.WAIT_MS });
    return panel;
  }

  private async notificationsCloseSnoozePicker(panel: Locator): Promise<void> {
    await this.page.keyboard.press("Escape");
    await panel.waitFor({ state: "detached", timeout: WebDriver.WAIT_MS }).catch(() => undefined);
  }

  private notificationsItemPatchWait(): Promise<unknown> {
    return this.page.waitForResponse(
      (response) => response.request().method() === "PATCH" && response.url().includes("/users/notifications/"),
      { timeout: WebDriver.WAIT_MS }
    );
  }

  async notificationsSnoozePresets(index: number): Promise<string[]> {
    const panel = await this.notificationsOpenSnoozePicker(index);
    const buttons = panel.locator('button[data-testid^="snooze-option-"]');
    const count = await buttons.count();
    const presets: string[] = [];
    for (let i = 0; i < count; i++) {
      const testid =
        (await buttons
          .nth(i)
          .getAttribute("data-testid")
          .catch(() => "")) ?? "";
      if (testid === "snooze-option-unsnooze" || testid === "snooze-option-custom") continue;
      presets.push(
        WebDriver.cleanText(
          await buttons
            .nth(i)
            .innerText()
            .catch(() => "")
        )
      );
    }
    await this.notificationsCloseSnoozePicker(panel);
    return presets;
  }

  async notificationsSnoozeWithPreset(index: number, preset: string): Promise<void> {
    const panel = await this.notificationsOpenSnoozePicker(index);
    const patchWait = this.notificationsItemPatchWait();
    await panel.getByRole("button", { name: preset, exact: true }).click({ timeout: WebDriver.WAIT_MS });
    await patchWait;
  }

  async notificationsSnoozeRemovalOffered(index: number): Promise<boolean> {
    const panel = await this.notificationsOpenSnoozePicker(index);
    const offered = (await panel.getByTestId("snooze-option-unsnooze").count()) > 0;
    await this.notificationsCloseSnoozePicker(panel);
    return offered;
  }

  async notificationsUnsnooze(index: number): Promise<void> {
    const panel = await this.notificationsOpenSnoozePicker(index);
    const patchWait = this.notificationsItemPatchWait();
    await panel.getByTestId("snooze-option-unsnooze").first().click({ timeout: WebDriver.WAIT_MS });
    await patchWait;
  }

  async notificationsFailItemWrites(status: number): Promise<void> {
    await this.page.route("**/api/workspaces/*/users/notifications/*/", async (route) => {
      if (route.request().method() === "PATCH") {
        await route.fulfill({ status, body: JSON.stringify({ detail: "parity snooze failure" }) });
      } else {
        await route.continue();
      }
    });
  }

  async notificationsClearItemWriteFailure(): Promise<void> {
    await this.page.unroute("**/api/workspaces/*/users/notifications/*/");
  }

  async notificationsOpenCustomSnooze(index: number): Promise<void> {
    const panel = await this.notificationsOpenSnoozePicker(index);
    await panel.getByTestId("snooze-option-custom").first().click({ timeout: WebDriver.WAIT_MS });
    await this.page.getByTestId("snooze-custom-dialog").first().waitFor({ timeout: WebDriver.WAIT_MS });
  }

  async notificationsCustomSnoozeVisible(): Promise<boolean> {
    const dialog = this.page.getByTestId("snooze-custom-dialog").first();
    if ((await dialog.count()) === 0) return false;
    return dialog.isVisible().catch(() => false);
  }

  private notificationsCustomDialogSection(heading: string): Locator {
    const dialog = this.page.getByTestId("snooze-custom-dialog").first();
    // Each section heads its picker with an h6; the heading's parent is the
    // section that owns the dropdown button.
    return dialog.getByText(heading, { exact: true }).locator("xpath=..");
  }

  async notificationsCustomSnoozePickDay(offsetDays: number): Promise<void> {
    // The date dropdown button carries the section; the calendar itself is
    // portaled, so the day grid is read page-wide like the other pickers.
    const section = this.notificationsCustomDialogSection("Pick a date");
    await section.getByRole("button").first().click({ timeout: WebDriver.WAIT_MS });
    const calendar = this.page.locator(".rdp-root").last();
    await calendar.waitFor({ state: "visible", timeout: WebDriver.WAIT_MS });
    const target = new Date();
    target.setDate(target.getDate() + offsetDays);
    const selects = calendar.locator("select");
    await selects.nth(0).selectOption({ label: target.toLocaleString("en-US", { month: "long" }) });
    await selects.nth(1).selectOption({ label: String(target.getFullYear()) });
    await calendar
      .locator("td:not(.rdp-outside) button", { hasText: new RegExp(`^${target.getDate()}$`) })
      .first()
      .click({ timeout: WebDriver.WAIT_MS });
    await calendar.waitFor({ state: "detached", timeout: WebDriver.WAIT_MS }).catch(() => undefined);
  }

  private async notificationsCustomTimePanel(): Promise<Locator> {
    const section = this.notificationsCustomDialogSection("Pick a time");
    await section.getByRole("button").first().click({ timeout: WebDriver.WAIT_MS });
    const panel = this.page.getByRole("listbox").last();
    // The list element itself is zero-height (its options ride in an
    // absolutely-positioned popper child), so settle on the placed panel
    // instead of the list's own visibility; this also covers the empty
    // "no available time" branch, which renders no options at all.
    await panel.locator("div[data-popper-placement]").first().waitFor({ timeout: WebDriver.WAIT_MS });
    return panel;
  }

  async notificationsCustomSnoozeTimeSlots(period: "AM" | "PM"): Promise<string[]> {
    const panel = await this.notificationsCustomTimePanel();
    await panel.getByText(period, { exact: true }).click({ timeout: WebDriver.WAIT_MS });
    const options = panel.getByRole("option");
    const count = await options.count();
    const slots: string[] = [];
    for (let i = 0; i < count; i++) {
      slots.push(
        WebDriver.cleanText(
          await options
            .nth(i)
            .innerText()
            .catch(() => "")
        )
      );
    }
    // Dismiss via an in-dialog outside click: Escape would also dismiss the
    // resume dialog itself, stranding the next read with no dialog at all.
    const dialog = this.page.getByTestId("snooze-custom-dialog").first();
    await dialog.getByText("Pick a time", { exact: true }).click({ timeout: WebDriver.WAIT_MS });
    await panel.waitFor({ state: "detached", timeout: WebDriver.WAIT_MS }).catch(() => undefined);
    return slots;
  }

  async notificationsCustomSnoozePickTime(period: "AM" | "PM", slot: string): Promise<void> {
    const panel = await this.notificationsCustomTimePanel();
    await panel.getByText(period, { exact: true }).click({ timeout: WebDriver.WAIT_MS });
    await panel.getByRole("option", { name: slot, exact: true }).click({ timeout: WebDriver.WAIT_MS });
    await panel.waitFor({ state: "detached", timeout: WebDriver.WAIT_MS }).catch(() => undefined);
  }

  async notificationsCustomSnoozeSubmit(): Promise<void> {
    const dialog = this.page.getByTestId("snooze-custom-dialog").first();
    await dialog.getByRole("button", { name: "Submit" }).click({ timeout: WebDriver.WAIT_MS });
  }

  async notificationsSetSnoozedMode(on: boolean): Promise<void> {
    const menuButton = this.page.getByTestId("notifications-overflow-button").first();
    await menuButton.click({ timeout: WebDriver.WAIT_MS });
    const item = this.page.getByText("Show snoozed", { exact: true });
    await item.waitFor({ timeout: WebDriver.WAIT_MS });
    // The active entry carries a trailing check icon (precedent: the tab
    // underline and other structural markers elsewhere in this driver).
    const row = item.locator("xpath=ancestor::div[contains(@class, 'cursor-pointer')][1]");
    const active = (await row.locator("div.ml-auto").count()) > 0;
    if (active !== on) {
      const listWait = this.page
        .waitForResponse(
          (response) =>
            response.request().method() === "GET" &&
            response.url().includes("/users/notifications") &&
            !response.url().includes("/unread/"),
          { timeout: WebDriver.WAIT_MS }
        )
        .catch(() => undefined);
      await item.click({ timeout: WebDriver.WAIT_MS });
      await listWait;
    }
    await this.page.keyboard.press("Escape");
  }

  async notificationsOpenEmailPreferences(): Promise<void> {
    await this.page.goto("/settings/profile/notifications", { timeout: 60_000 });
    await this.page.waitForLoadState("domcontentloaded");
    await this.page
      .getByTestId("email-prefs-form")
      .first()
      .getByRole("switch")
      .first()
      .waitFor({ timeout: WebDriver.WAIT_MS });
  }

  async notificationsEmailPreferencesLoaderShown(): Promise<boolean> {
    const pattern = "**/api/users/me/notification-preferences/";
    await this.page.route(pattern, async (route) => {
      if (route.request().method() === "GET") {
        await new Promise((resolve) => setTimeout(resolve, 1500));
      }
      await route.continue();
    });
    try {
      await this.page.goto("/settings/profile/notifications", { timeout: 60_000 });
      const seen = await this.page
        .getByTestId("email-prefs-loader")
        .first()
        .waitFor({ timeout: 10_000 })
        .then(
          () => true,
          () => false
        );
      await this.page
        .getByTestId("email-prefs-form")
        .first()
        .getByRole("switch")
        .first()
        .waitFor({ timeout: WebDriver.WAIT_MS });
      return seen;
    } finally {
      await this.page.unroute(pattern);
    }
  }

  private static readonly notificationsEmailPrefTitles: Record<NotificationsEmailPref, string> = {
    property_change: "Property changes",
    state_change: "State change",
    issue_completed: "Work item completed",
    comment: "Comments",
    mention: "Mentions",
  };

  private notificationsEmailPrefSwitch(pref: NotificationsEmailPref): Locator {
    const form = this.page.getByTestId("email-prefs-form").first();
    // Each control pairs a heading with its switch inside one row: the
    // heading's grandparent is the row that owns the switch.
    return form
      .getByRole("heading", { name: WebDriver.notificationsEmailPrefTitles[pref], exact: true })
      .locator("xpath=../..")
      .getByRole("switch")
      .first();
  }

  async notificationsEmailPreferences(): Promise<Record<NotificationsEmailPref, boolean>> {
    const prefs: NotificationsEmailPref[] = [
      "property_change",
      "state_change",
      "issue_completed",
      "comment",
      "mention",
    ];
    const out = {} as Record<NotificationsEmailPref, boolean>;
    for (const pref of prefs) {
      const checked = await this.notificationsEmailPrefSwitch(pref)
        .getAttribute("aria-checked")
        .catch(() => null);
      out[pref] = checked === "true";
    }
    return out;
  }

  async notificationsEmailPreferencesToggle(pref: NotificationsEmailPref): Promise<void> {
    const saveWait = this.page.waitForResponse(
      (response) =>
        response.request().method() === "PATCH" && response.url().includes("/users/me/notification-preferences/"),
      { timeout: WebDriver.WAIT_MS }
    );
    await this.notificationsEmailPrefSwitch(pref).click({ timeout: WebDriver.WAIT_MS });
    await saveWait;
  }

  async notificationsEmailPreferencesCompletedNested(): Promise<boolean> {
    const nest = this.page.getByTestId("email-prefs-completed-nest").first();
    if ((await nest.getByRole("switch").count()) !== 1) return false;
    // The nested row sits indented to the right of the state row.
    const stateRow = this.page
      .getByTestId("email-prefs-form")
      .first()
      .getByRole("heading", { name: "State change", exact: true })
      .locator("xpath=../..");
    const completedRow = this.page
      .getByTestId("email-prefs-form")
      .first()
      .getByRole("heading", { name: "Work item completed", exact: true })
      .locator("xpath=../..");
    const stateBox = await stateRow.boundingBox().catch(() => null);
    const completedBox = await completedRow.boundingBox().catch(() => null);
    if (!stateBox || !completedBox) return false;
    return completedBox.x > stateBox.x;
  }

  async notificationsFailEmailPreferenceSaves(status: number): Promise<void> {
    await this.page.route("**/api/users/me/notification-preferences/", async (route) => {
      if (route.request().method() === "PATCH") {
        await route.fulfill({ status, body: JSON.stringify({ detail: "parity prefs failure" }) });
      } else {
        await route.continue();
      }
    });
  }

  async notificationsClearEmailPreferenceSaveFailure(): Promise<void> {
    await this.page.unroute("**/api/users/me/notification-preferences/");
  }

  // --- Notifications filters, modes, read/archive (NEWFRONT-200,
  // --- NTF-015..019, NTF-023). Appended; existing methods above are
  // --- untouched per the shared driver contract.

  private static notificationsQueryOf(url: string): NotificationsListQuery {
    const params = new URL(url).searchParams;
    return {
      type: params.get("type"),
      read: params.get("read"),
      archived: params.get("archived"),
      snoozed: params.get("snoozed"),
      mentioned: params.get("mentioned"),
      cursor: params.get("cursor"),
    };
  }

  private static notificationsCardWritePath(url: string): boolean {
    return url.includes("/users/notifications/") && (url.endsWith("/read/") || url.endsWith("/archive/"));
  }

  private notificationsNextListQuery(timeoutMs: number = WebDriver.WAIT_MS): Promise<NotificationsListQuery> {
    return this.page
      .waitForResponse(
        (response) => {
          const url = response.url();
          return (
            response.request().method() === "GET" &&
            url.includes("/users/notifications") &&
            !url.includes("/unread/") &&
            response.ok()
          );
        },
        { timeout: timeoutMs }
      )
      .then((response) => WebDriver.notificationsQueryOf(response.url()));
  }

  async notificationsEntryListQuery(workspaceSlug: string): Promise<NotificationsListQuery> {
    // The window spans the navigation itself: on a loaded host the dev
    // server can take a while to compile and serve the inbox.
    const queryWait = this.notificationsNextListQuery(120_000);
    await this.page.goto(`/${workspaceSlug}/notifications/`, { timeout: 60_000 });
    await this.page.waitForLoadState("domcontentloaded");
    await this.notificationsTabLocator("all")
      .first()
      .waitFor({ timeout: WebDriver.WAIT_MS })
      .catch(() => undefined);
    return queryWait;
  }

  async notificationsOpenFilterMenu(): Promise<void> {
    await this.page.getByTestId("notifications-filter-button").first().click({ timeout: WebDriver.WAIT_MS });
    await this.page
      .getByTestId(/notifications-filter-option-/)
      .first()
      .waitFor({ timeout: WebDriver.WAIT_MS });
  }

  async notificationsFilterOptions(): Promise<NotificationsFilterOption[]> {
    const options = this.page.getByTestId(/notifications-filter-option-/);
    const count = await options.count();
    const out: NotificationsFilterOption[] = [];
    for (let i = 0; i < count; i++) {
      const option = options.nth(i);
      const hook = (await option.getAttribute("data-testid").catch(() => null)) ?? "";
      const value = hook.replace("notifications-filter-option-", "") as NotificationsOrigin;
      const label = WebDriver.cleanText(await option.innerText().catch(() => ""));
      // The checkmark icon renders only inside a selected option.
      const checked = (await option.locator("svg").count()) > 0;
      out.push({ value, label, checked });
    }
    return out;
  }

  async notificationsToggleFilterOrigin(origin: NotificationsOrigin): Promise<NotificationsListQuery> {
    const queryWait = this.notificationsNextListQuery();
    await this.page.getByTestId(`notifications-filter-option-${origin}`).first().click({ timeout: WebDriver.WAIT_MS });
    return queryWait;
  }

  async notificationsAppliedChips(): Promise<NotificationsAppliedChip[]> {
    const chips = this.page.getByTestId(/notifications-filter-chip-/);
    const count = await chips.count();
    const out: NotificationsAppliedChip[] = [];
    for (let i = 0; i < count; i++) {
      const chip = chips.nth(i);
      const hook = (await chip.getAttribute("data-testid").catch(() => null)) ?? "";
      const origin = hook.replace("notifications-filter-chip-", "") as NotificationsOrigin;
      const label = WebDriver.cleanText(await chip.innerText().catch(() => ""));
      out.push({ origin, label });
    }
    return out;
  }

  async notificationsRemoveFilterChip(origin: NotificationsOrigin): Promise<NotificationsListQuery> {
    const queryWait = this.notificationsNextListQuery();
    await this.page.getByTestId(`notifications-filter-chip-${origin}`).first().click({ timeout: WebDriver.WAIT_MS });
    return queryWait;
  }

  async notificationsClearFilters(): Promise<NotificationsListQuery> {
    const queryWait = this.notificationsNextListQuery();
    await this.page.getByTestId("notifications-filter-clear").first().click({ timeout: WebDriver.WAIT_MS });
    return queryWait;
  }

  async notificationsCloseMenus(): Promise<void> {
    await this.page.keyboard.press("Escape");
    await this.page
      .getByTestId(/notifications-filter-option-/)
      .first()
      .waitFor({ state: "hidden", timeout: WebDriver.WAIT_MS })
      .catch(() => undefined);
    await this.page
      .getByTestId("notifications-mode-option")
      .first()
      .waitFor({ state: "hidden", timeout: WebDriver.WAIT_MS })
      .catch(() => undefined);
  }

  async notificationsOpenOverflowMenu(): Promise<void> {
    await this.page.getByTestId("notifications-overflow-button").first().click({ timeout: WebDriver.WAIT_MS });
    await this.page.getByTestId("notifications-mode-option").first().waitFor({ timeout: WebDriver.WAIT_MS });
  }

  async notificationsOverflowOptions(): Promise<string[]> {
    const options = this.page.getByTestId("notifications-mode-option");
    const count = await options.count();
    const out: string[] = [];
    for (let i = 0; i < count; i++) {
      out.push(
        WebDriver.cleanText(
          await options
            .nth(i)
            .innerText()
            .catch(() => "")
        )
      );
    }
    return out;
  }

  async notificationsToggleMode(mode: NotificationsMode): Promise<NotificationsListQuery> {
    // Mode options render in menu order: unread, archived, snoozed.
    const index = mode === "unread" ? 0 : mode === "archived" ? 1 : 2;
    const queryWait = this.notificationsNextListQuery();
    await this.page.getByTestId("notifications-mode-option").nth(index).click({ timeout: WebDriver.WAIT_MS });
    return queryWait;
  }

  private notificationsCardActions(index: number): Locator {
    return this.page.getByTestId("notification-card").nth(index).getByTestId("notifications-card-actions");
  }

  async notificationsCardActionsVisible(index: number): Promise<boolean> {
    const actions = this.notificationsCardActions(index);
    if ((await actions.count()) === 0) return false;
    return actions.isVisible().catch(() => false);
  }

  async notificationsHoverCard(index: number): Promise<void> {
    const card = this.page.getByTestId("notification-card").nth(index);
    await card.scrollIntoViewIfNeeded().catch(() => undefined);
    await card.hover({ timeout: WebDriver.WAIT_MS });
    await this.notificationsCardActions(index).waitFor({ state: "visible", timeout: WebDriver.WAIT_MS });
  }

  private async notificationsClickCardAction(index: number, action: number): Promise<void> {
    await this.notificationsHoverCard(index);
    const actions = this.notificationsCardActions(index);
    const writeWait = this.page
      .waitForResponse(
        (response) => {
          const method = response.request().method();
          return (
            (method === "POST" || method === "DELETE") &&
            WebDriver.notificationsCardWritePath(response.url()) &&
            response.ok()
          );
        },
        { timeout: WebDriver.WAIT_MS }
      )
      .then(
        () => true,
        () => false
      );
    const abortWait = this.page
      .waitForEvent("requestfailed", {
        predicate: (request) => WebDriver.notificationsCardWritePath(request.url()),
        timeout: WebDriver.WAIT_MS,
      })
      .then(
        () => true,
        () => false
      );
    // Read, archive, then snooze: the fixed render order while the snooze
    // popover stays closed, so its panel buttons are absent.
    await actions.locator("button").nth(action).click({ timeout: WebDriver.WAIT_MS });
    await Promise.race([writeWait, abortWait]);
    await this.page.unrouteAll({ behavior: "wait" }).catch(() => undefined);
  }

  async notificationsToggleCardRead(index: number): Promise<void> {
    await this.notificationsClickCardAction(index, 0);
  }

  async notificationsToggleCardArchive(index: number): Promise<void> {
    await this.notificationsClickCardAction(index, 1);
  }

  async notificationsFailNextCardWrite(): Promise<void> {
    await this.page.route(
      (url) => WebDriver.notificationsCardWritePath(url.toString()),
      (route) => route.abort(),
      { times: 1 }
    );
  }
}
