// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// New-app driver skeleton (NEWFRONT-19). Implements the same interface as
// the oracle driver so scenarios compile against either target, but every
// action throws until the matching area lands in apps/web_new. Area issues
// fill these in method by method; the oracle driver stays untouched.
import { expect, type Locator, type Page, type Route } from "@playwright/test";

import { descriptionAfterHeading } from "./modal-copy.js";
import type {
  AdminAbsenceFacts,
  AdminAccountFacts,
  AdminAccountMenuFacts,
  AdminAiFacts,
  AdminAuthCopyFact,
  AdminAuthDiscardFacts,
  AdminAuthFormFacts,
  AdminAuthLandingFacts,
  AdminAuthProviderFacts,
  AdminBreadcrumbFacts,
  AdminCloudGateFetchCounts,
  AdminCloudHomeBranch,
  AdminCloudHomeFacts,
  AdminDeleteWatchFacts,
  AdminEmailFacts,
  AdminEmailFields,
  AdminEmailFormFacts,
  AdminEmailSecurity,
  AdminFailureFacts,
  AdminFooterControl,
  AdminGeneralFacts,
  AdminHamburgerFacts,
  AdminHelpFacts,
  AdminImageFacts,
  AdminLoopDetailFacts,
  AdminLoopListFacts,
  AdminLoopModalFacts,
  AdminLoopModalFields,
  AdminLoopRow,
  AdminLoopTargetRow,
  AdminNotFoundFacts,
  AdminRootBranch,
  AdminRunnersFacts,
  AdminSetupFacts,
  AdminSetupFields,
  AdminShortcutProbeFacts,
  AdminSidebarFacts,
  AdminSignInFacts,
  AdminTestEmailFacts,
  AdminTransportFacts,
  AdminWorkspaceCreateFacts,
  AdminWorkspaceDeleteFacts,
  AdminWorkspaceListFacts,
  AdminWorkspaceRow,
  ApprovalCard,
  ArchivesArchiveDialog,
  ModulesDeleteDialog,
  ModulesRowFacts,
  ArchivesFilterExpression,
  ArchivesListQuery,
  ArchivesMenuEntry,
  ArchivesModuleChip,
  ArchivesModulesEmptyKind,
  ArchivesPeekReadOnly,
  ArchivesTab,
  ArchivesTrafficCounts,
  ModulesDetailGroupedOption,
  ModulesDetailLinkRow,
  ModulesDetailStatRow,
  ModulesMenuEntry,
  ModulesTimelineRow,
  ModulesToast,
  AddRunnerFormState,
  CyclesProgressStatRow,
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
  LayoutsLayoutKey,
  BoardLayoutKey,
  DeskSigninUpdateLink,
  DevMachineInstallCard,
  DevMachineModal,
  DevMachineRow,
  GanttSidebarRow,
  GanttZoom,
  IntakeXApiRequest,
  IntakeXTab,
  IntakeShellListQuery,
  IntakeShellTab,
  KanbanCard,
  KanbanColumn,
  CyclesRowFacts,
  DocumentShellFacts,
  NotFoundFacts,
  ParityBrowserCookie,
  ParityDriver,
  ParityTarget,
  PromptEditorState,
  PromptReceiptCard,
  PromptRevertDialog,
  PromptSectionCard,
  RulesCommentMenuOption,
  RunsListRow,
  SchedulerBindingHeader,
  SchedulerBindingRunRow,
  SchedulerBindingValues,
  SchedulerCalendarBlock,
  RunnerChatStreamFrame,
  NotificationsAppliedChip,
  NotificationsCard,
  NotificationsEmailPref,
  NotificationsFilterOption,
  NotificationsListQuery,
  NotificationsMode,
  NotificationsOrigin,
  NotificationsDetailVariant,
  NotificationsTab,
  SchedulerCatalogRow,
  SchedulerDefinitionValues,
  SchedulerInstallOption,
  SchedulerProjectInstallOption,
  SchedulerProjectRow,
  SchedulerScheduleValues,
  RunnersEditPodForm,
  RunnersPodTile,
  RunnersRail,
  RunnersRowActions,
  RunnersTab,
  RunnersTableRow,
  ServedShellMarkers,
  IntakeDetailProperty,
  CyclesHeroProgressGroup,
  CyclesHeroTrafficCounts,
  CyclesHeroUpsell,
  WorkspaceOnboardingView,
  PagesHeaderDownload,
  PagesHeaderExportForm,
  PagesLiveSocketClose,
  PagesKeysToolbarTooltip,
  PagesPrefsLocalConfig,
  PagesPaneActor,
  PagesPaneAssetRow,
  PagesPaneOutlineHeading,
  PagesPaneOverlayHeader,
  PagesPaneStats,
  PagesPaneVersionEntry,
  IntakeListChip,
  IntakeListFilterKey,
  IntakeListOrderField,
  IntakeListOrderState,
  IntakeListTooltip,
  EstimatesActiveRow,
  EstimatesFamilyOption,
  EstimatesStarterCard,
  PagesGatingTab,
  PagesGatingListVariant,
  PagesGatingCreateState,
  PagesGatingErrorPanel,
  PagesGatingRequest,
  PagesGatingHeaderCluster,
  PagesGatingTitleFacts,
  ModulesBreadcrumb,
  ModulesChip,
  ModulesGateFacts,
  PagesListRowDetail,
  PagesListTab,
  PagesRowTab,
  ModulesListCardControl,
  ModulesListCardLead,
  ModulesListCardSegments,
  ModulesListEmptyKind,
  ModulesListLayout,
  ModulesListRowProgress,
  ModulesListSkeletonResult,
  ModulesListZeroStateCopy,
  PagesMarkCurrent,
  PagesToolbarColorCounts,
  PagesToolbarColorPreview,
  PagesToolbarTooltip,
  AnalyticsApiRequest,
  AnalyticsEmptyFacts,
  AnalyticsHeadlineCard,
  AnalyticsRadarRow,
  AnalyticsRadarFacts,
  AnalyticsActiveProject,
  AnalyticsCreatedResolvedFacts,
  AnalyticsCustomChartFacts,
  AnalyticsCustomTooltipRow,
  AnalyticsDialogAssignee,
  PagesBlocksAssetRow,
  PagesBlocksInfoStats,
  PagesBlocksInlineImage,
  ExportsSettingsEmptyFacts,
  ExportsSettingsHistoryRow,
  AnalyticsExportTable,
  LocalFileDownload,
  PageExportFile,
  PageExportFocusFacts,
  PageExportRow,
  SettingsProfileCustomTheme,
  SettingsProfileGeneralField,
  SettingsProfileLocale,
  SettingsProfileSecurityField,
  SettingsProfileTheme,
  WsSettingsMemberRowFacts,
  SpaceBoardGroupFacts,
  SpaceBoardCardFacts,
  SpaceBoardCardMarkers,
  SpaceBoardCardFooter,
  SpaceBoardDragFacts,
  SpaceBoardStability,
  SpaceChromeAccountIdentity,
  SpaceChromeBadgeFacts,
  SpaceChromeCrashFacts,
  SpaceChromeFailureFacts,
  SpaceChromeFramedFacts,
  SpaceChromeNavbarIdentity,
  SpaceChromeProgressProbe,
  SpaceChromeSignOutForm,
  SpaceChromeSignOutResult,
  SpaceChromeSpinnerProbe,
  SpaceChromeToastFacts,
  SpaceLocaleBoardFacts,
  SpaceLocaleLandingFacts,
  SpaceAccessHeadFacts,
  SpaceAuthLanding,
  SpaceCommentNotice,
  SpaceCommentRow,
  SpaceCommentSection,
  SpaceMentionResult,
  SpacePickedEmoji,
  SpaceReactionGroup,
  SpaceVoteState,
  CloudAppsCardFacts,
  CloudAppsDialogFieldFacts,
  CloudCliCardFacts,
  CloudDocsFacts,
  CloudDownloadCardFacts,
  CloudHomeSectionFacts,
  CloudLoginCardFacts,
  CloudNavLinkFacts,
  CloudPricingTierFacts,
  CloudProviderButtonFacts,
  CloudAssistantAddFill,
  CloudAssistantAddFormFacts,
  CloudAssistantConnectionFacts,
  WsSettingsApiTokenBackup,
  WsSettingsApiTokenRow,
  SettingsGeneralField,
} from "./parity-driver";

function todo(target: string): never {
  throw new Error(`[parity] drivers/web_new has no ${target} yet; the area that owns it has not landed.`);
}

export class WebNewDriver implements ParityDriver {
  readonly target: ParityTarget = "web_new";
  readonly page: Page;

  constructor(page: Page) {
    this.page = page;
  }

  // Runner-chat stubs and counters (NEWFRONT-549); set by the chat methods.
  private runnerChatSessionCreateFailRemaining = 0;
  private runnerChatSessionCreateDelayMs = 0;
  private runnerChatSessionCreateDelayRemaining = 0;
  private runnerChatSendFailRemaining = 0;
  private runnerChatRunnerDetailDelayMs = 0;
  private runnerChatRunnerDetailDelayRemaining = 0;
  private runnerChatRunnerDetailDelayPattern: string | null = null;
  private runnerChatStreamFrames = new Map<string, RunnerChatStreamFrame[]>();
  private runnerChatStreamUrls = new Map<string, string[]>();
  private runnerChatSpyCounts = {
    warm: 0,
    sessionCreate: 0,
    send: 0,
    cancel: 0,
    close: 0,
    sessionList: 0,
    messageList: 0,
  };

  // --- Auth area (NEWFRONT-491 gate). The card is email-first with
  // --- placeholder-pinned steps; submits are XHR plus a full navigation on
  // --- success, an inline banner plus step recovery on failure.

  /** Every wait below is explicitly bounded; a bare wait would hang to the test timeout. */
  private static readonly WAIT_MS = 30_000;

  private authEmailField() {
    return this.page.getByPlaceholder("name@company.com").first();
  }

  private authPasswordField() {
    return this.page.getByPlaceholder("Enter password").first();
  }

  private authCodeField() {
    return this.page.getByPlaceholder("123456").first();
  }

  private cardSubmit(form: Locator) {
    return form.locator('button[type="submit"]');
  }

  private cardSection() {
    return this.page.locator('section[aria-label="Sign in"], section[aria-label="Sign up"]').first();
  }

  /** Settle on the card in any step (or the no-methods notice), reloading once on a blank paint. */
  private async waitForCard(): Promise<void> {
    const card = this.cardSection();
    try {
      await card.waitFor({ timeout: WebNewDriver.WAIT_MS });
    } catch {
      await this.page.reload();
      await card.waitFor({ timeout: WebNewDriver.WAIT_MS });
    }
  }

  /** True once the password or code step shows (the email check round-tripped). */
  private async nextAuthStepShown(timeoutMs: number): Promise<boolean> {
    const password = this.authPasswordField()
      .waitFor({ state: "visible", timeout: timeoutMs })
      .then(
        () => true,
        () => false
      );
    const code = this.authCodeField()
      .waitFor({ state: "visible", timeout: timeoutMs })
      .then(
        () => true,
        () => false
      );
    const [passwordShown, codeShown] = await Promise.all([password, code]);
    return passwordShown || codeShown;
  }

  async openEntry(): Promise<void> {
    await this.page.goto("/");
    try {
      await this.cardSection().waitFor({ timeout: 10_000 });
      return;
    } catch {
      // A signed-in context bounces to the remembered workspace instead
      // of painting the card; that bounce is the entry working, not a
      // stale session, so it returns as-is (user switches reset first).
      // A slow first paint just falls through to the retry.
      if (new URL(this.page.url()).pathname !== "/") return;
      await this.waitForCard();
    }
  }

  async signInWithPassword(email: string, password: string): Promise<void> {
    // Shared-stack flake cover: a swallowed submit or a throttled minute
    // fails one pass, so the whole flow retries with quiet backoff.
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
      throw new Error("[parity] sign-in never left the card.");
    }
  }

  private async signInAttempt(email: string, password: string): Promise<void> {
    const page = this.page;
    // Stay on the entry when already there: callers stage return paths and
    // prefill in the entry query, and a fresh navigation would wipe them.
    if (new URL(page.url()).pathname !== "/") await page.goto("/");
    // Settle for either entry state: a retry after a partial pass lands
    // already signed in (the session cookie survived), so the card never
    // renders; signed-in visitors are redirected away from / by the guard.
    const deadline = Date.now() + 60_000;
    let settled: "signed-in" | "card" | null = null;
    while (Date.now() < deadline) {
      if (new URL(page.url()).pathname !== "/") {
        settled = "signed-in";
        break;
      }
      if ((await this.cardSection().count()) > 0) {
        settled = "card";
        break;
      }
      await page.waitForTimeout(1_000);
    }
    if (settled === "signed-in") return;
    if (settled === null) throw new Error("[parity] sign-in card never rendered.");
    await this.submitAuthEmail(email);
    const passwordField = page.getByPlaceholder("Enter password");
    await passwordField.waitFor({ timeout: WebNewDriver.WAIT_MS });
    await passwordField.fill(password);
    const passwordForm = page.locator("form", { has: passwordField });
    // Success navigates away (location.assign to the landing); the card
    // leaving is only half the story — on a slow API the mutation can
    // resolve after the test has already navigated on, and the belated
    // assign then clobbers the test's page. So mark this realm, submit,
    // wait for the card to go, then wait for the assign's load (marker
    // gone) and the entry bounce (URL leaves /): only then is sign-in
    // really complete. Landings differ per user, so the bounce target
    // itself is never asserted here.
    await page.evaluate(() => {
      (globalThis as { __paritySignInMark?: boolean }).__paritySignInMark = true;
    });
    await Promise.all([
      this.cardSection().waitFor({ state: "detached", timeout: 90_000 }),
      this.cardSubmit(passwordForm).click(),
    ]);
    const assigned = Date.now() + 90_000;
    for (;;) {
      if (new URL(page.url()).pathname !== "/") return;
      const marked = await page
        .evaluate(() => (globalThis as { __paritySignInMark?: boolean }).__paritySignInMark === true)
        .catch(() => false);
      if (!marked) break;
      if (Date.now() > assigned) throw new Error("[parity] sign-in submit never navigated.");
      await page.waitForTimeout(1_000);
    }
    await page.waitForFunction(() => new URL(globalThis.location.href).pathname !== "/", null, {
      timeout: 60_000,
    });
  }

  async openProjectIssues(workspaceSlug: string, projectId: string): Promise<void> {
    await this.page.goto(`/${workspaceSlug}/projects/${projectId}/issues`);
    // Settle on whatever the guard decides: the list in any settled state
    // (rows, empty, error), the project-gate denial screen, or the
    // signed-out entry after a bounce. Never throw on the bounce — callers
    // assert it (sign-out kills the session).
    const deadline = Date.now() + 60_000;
    let reloaded = false;
    for (;;) {
      const list = this.page.locator('section[aria-label="Issue list"], section[aria-label="Issues"]');
      if ((await list.count()) > 0 && (await list.first().isVisible())) return;
      if ((await this.authEmailField().count()) > 0) return;
      if ((await this.page.getByRole("heading", { name: "Workspace not found" }).count()) > 0) return;
      if (Date.now() > deadline) return;
      if (!reloaded && Date.now() > deadline - 30_000) {
        reloaded = true;
        await this.page.reload().catch(() => {});
      } else {
        await this.page.waitForTimeout(1_000);
      }
    }
  }

  async visibleIssueNames(): Promise<string[]> {
    // Scaffold-tier selector: each row is an article whose second span
    // carries the issue name (key first, priority badge last).
    const rows = this.page.locator('section[aria-label="Issue list"] article');
    const count = await rows.count();
    const names: string[] = [];
    for (let i = 0; i < count; i++) {
      names.push(((await rows.nth(i).locator("span").nth(1).innerText()) ?? "").trim());
    }
    return names;
  }

  // --- Mention flows (NEWFRONT-115). Throwing stubs per the shared driver
  // --- contract; the mentions area fills these in when it lands.

  async mentionsOpenIssueDetail(_workspaceSlug: string, _projectId: string, _issueId: string): Promise<void> {
    return todo("mentionsOpenIssueDetail");
  }

  async mentionsSuggestionsFor(_query: string): Promise<string[]> {
    return todo("mentionsSuggestionsFor");
  }

  async mentionsSuggestionsHaveAvatars(): Promise<boolean> {
    return todo("mentionsSuggestionsHaveAvatars");
  }

  async mentionsSuggestionSections(): Promise<string[]> {
    return todo("mentionsSuggestionSections");
  }

  async mentionsPostComment(_displayName: string, _bodyText: string): Promise<void> {
    return todo("mentionsPostComment");
  }

  async mentionsVisibleReferences(): Promise<{ text: string; href: string | null }[]> {
    return todo("mentionsVisibleReferences");
  }

  async mentionsEditRemovingMention(_oldBodyText: string, _plainText: string): Promise<void> {
    return todo("mentionsEditRemovingMention");
  }

  async mentionsEnsureSignedIn(_email: string, _password: string): Promise<void> {
    return todo("mentionsEnsureSignedIn");
  }

  // Workspace onboarding + creation (NEWFRONT-111, AUTH-034..043). Throwing
  // stubs until the onboarding and workspace-creation areas land in
  // apps/web_new; the oracle driver already implements these.
  async openAuthenticated(path: string, cookies: ParityBrowserCookie[]): Promise<void> {
    await this.page.context().addCookies(cookies);
    await this.page.goto(path, { waitUntil: "domcontentloaded" });
    await this.page.waitForLoadState("domcontentloaded");
  }
  async currentPath(): Promise<string> {
    const url = new URL(this.page.url());
    return `${url.pathname}${url.search}`;
  }
  async hasVisibleText(text: string): Promise<boolean> {
    const match = this.page.getByText(text, { exact: false }).first();
    if ((await match.count()) === 0) return false;
    return match.isVisible();
  }
  private async isShown(locator: Locator): Promise<boolean> {
    if ((await locator.count()) === 0) return false;
    return locator.first().isVisible();
  }

  async awaitWorkspaceStep(): Promise<void> {
    // The workspace step arrives after the profile save round-trips; the
    // spinner holds the route meanwhile. Bounded: 60 half-second polls.
    for (let i = 0; i < 60; i++) {
      if ((await this.visibleWorkspaceView()) !== "none") return;
      await this.page.waitForTimeout(500);
    }
    throw new Error("[parity] workspace step never rendered.");
  }
  async visibleWorkspaceView(): Promise<WorkspaceOnboardingView> {
    // Each view carries a distinct first-level heading; the creation-off
    // notice intentionally reads as none (it replaces the create form).
    const page = this.page;
    if (await this.isShown(page.getByRole("heading", { name: "Your request is pending", exact: true })))
      return "pending";
    if (await this.isShown(page.getByRole("heading", { name: "Join a workspace", exact: true }))) return "invites";
    if (await this.isShown(page.getByRole("heading", { name: "Request workspace access", exact: true })))
      return "join_by_email";
    if (await this.isShown(page.getByRole("heading", { name: "Create your workspace", exact: true }))) return "create";
    return "none";
  }
  async fillWorkspaceName(name: string): Promise<void> {
    await this.page.getByLabel("Workspace name").fill(name);
  }
  async fillWorkspaceSlug(slug: string): Promise<void> {
    await this.page.getByLabel("Workspace URL").fill(slug);
  }
  async workspaceSlugValue(): Promise<string> {
    return this.page.getByLabel("Workspace URL").inputValue();
  }
  async selectTeamSizePill(label: string): Promise<void> {
    await this.page.getByRole("button", { name: label, exact: true }).click();
  }
  async selectTeamSizeDropdown(label: string): Promise<void> {
    // The standalone form swaps the pills for a native select under the
    // same "Team size" label.
    await this.page.getByLabel("Team size").selectOption({ label });
  }
  async submitCreateWorkspace(): Promise<void> {
    await this.page.getByRole("button", { name: "Create workspace", exact: true }).click();
  }
  async isCreateWorkspaceSubmitDisabled(): Promise<boolean> {
    return this.page.getByRole("button", { name: "Create workspace", exact: true }).isDisabled();
  }
  async workspaceSlugErrorText(): Promise<string | null> {
    // The create form reports malformed, over-long, and taken slugs under
    // the URL field; return whichever is showing.
    for (const pin of ["URLs can contain only", "already taken", "within 48 characters"]) {
      const note = this.page.getByText(pin, { exact: false });
      if (await this.isShown(note)) return ((await note.first().textContent()) ?? pin).trim();
    }
    return null;
  }
  async gotoJoinByEmailFromCreate(): Promise<void> {
    await this.page.getByRole("button", { name: "Join by admin email", exact: true }).click();
  }
  async gotoInvitesFromCreate(): Promise<void> {
    return todo("gotoInvitesFromCreate");
  }
  async fillWorkspaceAdminEmail(email: string): Promise<void> {
    await this.page.getByLabel("Admin email address").fill(email);
  }
  async submitJoinRequest(): Promise<void> {
    await this.page.getByRole("button", { name: "Send join request", exact: true }).click();
  }
  async pendingApprovalNamesEmail(email: string): Promise<boolean> {
    await this.page.getByText("Your request is pending", { exact: false }).first().waitFor();
    return this.hasVisibleText(email);
  }
  async createInsteadFromPending(): Promise<void> {
    await this.page.getByRole("button", { name: "Create a workspace instead", exact: true }).click();
  }
  async selectInviteByWorkspace(_workspaceName: string): Promise<void> {
    return todo("selectInviteByWorkspace");
  }
  async continueWithSelectedInvites(): Promise<void> {
    return todo("continueWithSelectedInvites");
  }
  async awaitInviteMembersStep(): Promise<void> {
    await this.page.getByRole("heading", { name: "Invite your team", exact: true }).waitFor({ timeout: 30_000 });
  }
  async isInviteMembersStepVisible(): Promise<boolean> {
    return this.isShown(this.page.getByRole("heading", { name: "Invite your team", exact: true }));
  }
  async inviteRowCount(): Promise<number> {
    // One address input per row, labelled with its 1-based position.
    return this.page.getByRole("textbox", { name: /email address \d+/i }).count();
  }
  async fillInviteRow(index: number, email: string): Promise<void> {
    await this.page.getByRole("textbox", { name: `Email address ${index + 1}`, exact: true }).fill(email);
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
    await this.page.getByRole("button", { name: "Invite later", exact: true }).click();
  }
  async isOnboardingBackVisible(): Promise<boolean> {
    return this.isShown(this.page.getByRole("button", { name: "Back", exact: true }));
  }
  async clickOnboardingBack(): Promise<void> {
    await this.page.getByRole("button", { name: "Back", exact: true }).click();
  }
  async isTourWelcomeVisible(): Promise<boolean> {
    // The welcome stop offers the tour; its take-the-tour button marks it.
    return this.hasVisibleText("Take the tour");
  }
  async declineTour(): Promise<void> {
    await this.page.getByRole("button", { name: "Explore on my own", exact: true }).click();
  }
  async isStandaloneCreationDisabledVisible(): Promise<boolean> {
    return this.hasVisibleText("Only the instance admin can create workspaces");
  }
  async isRequestInstanceAdminLinkVisible(): Promise<boolean> {
    return this.isShown(this.page.getByRole("link", { name: "Ask the instance admin", exact: true }));
  }
  async isInOnboardingCreationDisabledNoticeVisible(): Promise<boolean> {
    return this.hasVisibleText("Workspace creation is turned off");
  }

  // --- Auth sign-in core (NEWFRONT-107). Throwing stubs per the shared
  // --- driver contract; the sign-in area fills these in when it lands.

  async openSignInWithParams(params: Record<string, string>): Promise<void> {
    const query = new URLSearchParams(params);
    const suffix = query.size > 0 ? `?${query.toString()}` : "";
    await this.page.goto(`/${suffix}`);
    await this.waitForCard();
  }

  async submitEmail(email: string): Promise<void> {
    const page = this.page;
    const emailField = page.getByPlaceholder("name@company.com").first();
    await emailField.fill(email);
    const emailForm = page.locator("form", { has: emailField });
    await this.cardSubmit(emailForm).click();
    // The email check round-trips to the server, then the card shows the
    // password or code step. Either placeholder marks the transition.
    await page
      .locator('input[placeholder="Enter password"], input[placeholder="123456"]')
      .first()
      .waitFor({ timeout: WebNewDriver.WAIT_MS });
  }

  async submitPassword(password: string): Promise<void> {
    const page = this.page;
    const passwordField = page.getByPlaceholder("Enter password");
    await passwordField.fill(password);
    // XHR submit: success navigates away, failure re-renders inline. The
    // scenario polls the step and banner, so no navigation wait here.
    await this.cardSubmit(page.locator("form", { has: passwordField })).click();
  }

  async authStep(): Promise<"email" | "password" | "code" | "unavailable" | "unknown"> {
    const page = this.page;
    if ((await page.getByText("Sign-in is unavailable").count()) > 0) return "unavailable";
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
    return (await this.page.getByLabel("Confirm password").count()) > 0;
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
    await this.page.getByPlaceholder("123456").waitFor({ timeout: WebNewDriver.WAIT_MS });
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
    // XHR submit like the password step: the scenario polls the outcome.
    await this.cardSubmit(page.locator("form", { has: codeField })).click();
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
    // Past the email step the locked address carries a step reset; on the
    // email step the clear control only empties the field.
    const reset = this.page.getByRole("button", { name: "Clear email" });
    if ((await reset.count()) > 0) {
      await reset.first().click();
    } else {
      await this.authEmailField().fill("");
    }
    await this.authEmailField().waitFor({ timeout: WebNewDriver.WAIT_MS });
  }

  async seesNoAuthMethods(): Promise<boolean> {
    return (await this.page.getByText("Sign-in is unavailable").count()) > 0;
  }

  async forgotPasswordPopoverText(): Promise<string | null> {
    // The mail-less explanation hides behind a popover toggle; open it,
    // then read the revealed text.
    await this.page.getByRole("button", { name: "Forgot your password?" }).click();
    const explanation = this.page.getByText(/SMTP mail configured/i);
    await explanation.waitFor({ timeout: WebNewDriver.WAIT_MS });
    const text = ((await explanation.first().textContent()) ?? "").trim().replace(/\s+/g, " ");
    return text === "" ? null : text;
  }

  // --- Auth sign-up, recovery, guards, landing (NEWFRONT-108, AUTH-009/016).
  // --- Throwing stubs per the shared driver contract; that area fills these
  // --- in when it lands. currentPath/authStep already stubbed above.

  async openSignUp(params?: { email?: string; nextPath?: string }): Promise<void> {
    const query = new URLSearchParams();
    if (params?.email !== undefined) query.set("email", params.email);
    if (params?.nextPath !== undefined) query.set("next_path", params.nextPath);
    const suffix = query.size > 0 ? `?${query.toString()}` : "";
    await this.page.goto(`/sign-up${suffix}`);
    await this.waitForCard();
  }

  async submitAuthEmail(email: string): Promise<void> {
    const field = this.authEmailField();
    const form = this.page.locator("form", { has: field });
    // A saturated throttle minute answers the check with a rate-limit
    // banner instead of advancing; wait out the window and resubmit.
    for (let attempt = 1; ; attempt += 1) {
      await field.fill(email);
      await this.cardSubmit(form).click();
      if (await this.nextAuthStepShown(30_000)) return;
      const banner = await this.authBanner();
      if (banner !== null && /rate limit/i.test(banner) && attempt < 3) {
        await this.page.waitForTimeout(65_000);
        continue;
      }
      if (await this.nextAuthStepShown(60_000)) return;
      throw new Error("[parity] email submit advanced to neither password nor code step.");
    }
  }

  async authEmailValue(): Promise<string> {
    return this.authEmailField().inputValue();
  }

  async authNextPathValue(): Promise<string | null> {
    const hidden = this.page.locator('input[type="hidden"][name="next_path"]');
    if ((await hidden.count()) === 0) return null;
    return hidden.first().getAttribute("value");
  }

  async signUpWithPassword(password: string, confirmPassword: string): Promise<void> {
    await this.authPasswordField().fill(password);
    await this.page.getByLabel("Confirm password").fill(confirmPassword);
    // Success navigates to the post-sign-up path; wait for the URL to move.
    const before = this.page.url();
    const form = this.page.locator("form", { has: this.authPasswordField() });
    await Promise.all([
      this.page.waitForURL((url) => url.href !== before, { timeout: 60_000 }),
      this.cardSubmit(form).click(),
    ]);
    await this.page.waitForLoadState("domcontentloaded");
  }

  async submitUniqueCode(code: string): Promise<void> {
    const field = this.authCodeField();
    await field.fill(code);
    const before = this.page.url();
    await Promise.all([
      this.page.waitForURL((url) => url.href !== before, { timeout: 60_000 }),
      this.cardSubmit(this.page.locator("form", { has: field })).click(),
    ]);
    await this.page.waitForLoadState("domcontentloaded");
  }

  async codeResendState(): Promise<{ disabled: boolean; label: string }> {
    const button = this.page.getByRole("button", { name: /resend|requesting/i });
    await button.waitFor({ timeout: WebNewDriver.WAIT_MS });
    const label = ((await button.first().textContent()) ?? "").trim().replace(/\s+/g, " ");
    return { disabled: await button.first().isDisabled(), label };
  }

  async requestNewCode(): Promise<void> {
    await this.page.getByRole("button", { name: /resend|requesting/i }).click();
  }

  async passwordSubmitEnabled(): Promise<boolean> {
    const form = this.page.locator("form", { has: this.authPasswordField() });
    return this.cardSubmit(form).isEnabled();
  }

  async fillPasswordFields(password: string, confirmPassword: string): Promise<void> {
    await this.authPasswordField().fill(password);
    await this.page.getByLabel("Confirm password").fill(confirmPassword);
  }

  async clickPasswordSubmit(): Promise<void> {
    const form = this.page.locator("form", { has: this.authPasswordField() });
    await this.cardSubmit(form).click();
    await this.page.waitForTimeout(2000);
  }

  async passwordMismatchError(): Promise<string | null> {
    // The mismatch note renders off the confirm field's focus state, so it
    // can lag the fill by a beat; wait briefly, then report absence.
    const note = this.page.getByText("The two passwords don't match.");
    try {
      await note.first().waitFor({ timeout: 5_000 });
    } catch {
      return null;
    }
    return ((await note.first().textContent()) ?? "").trim();
  }

  async authBanner(): Promise<string | null> {
    const banner = this.page.getByRole("alert");
    if ((await banner.count()) === 0) return null;
    return ((await banner.first().textContent()) ?? "").trim();
  }

  async waitForAuthBanner(): Promise<string> {
    // The banner renders after hydration plus the failure effect, so poll
    // instead of reading once. Bounded: a missing banner is a real failure.
    const banner = this.page.getByRole("alert");
    await banner.first().waitFor({ timeout: 60_000 });
    return ((await banner.first().textContent()) ?? "").trim();
  }

  async openForgotPassword(email?: string): Promise<void> {
    const suffix = email !== undefined ? `?email=${encodeURIComponent(email)}` : "";
    await this.page.goto(`/accounts/forgot-password${suffix}`);
    await this.page
      .getByRole("button", { name: /send reset link|resend in/i })
      .waitFor({ timeout: WebNewDriver.WAIT_MS });
  }

  async submitForgotPassword(email: string): Promise<string> {
    const field = this.authEmailField();
    await field.fill(email);
    const form = this.page.locator("form", { has: field });
    await this.cardSubmit(form).click();
    // Either the inbox-check toast or the failure toast appears; the
    // button label flips to the countdown on success.
    const toast = this.page.getByText(/check your inbox|could not send a reset link/i).first();
    await toast.waitFor({ timeout: WebNewDriver.WAIT_MS });
    return ((await toast.textContent()) ?? "").trim();
  }

  async forgotResendState(): Promise<{ disabled: boolean; label: string }> {
    const button = this.page.getByRole("button", { name: /send reset link|resend in/i });
    await button.waitFor({ timeout: WebNewDriver.WAIT_MS });
    const label = ((await button.first().textContent()) ?? "").trim().replace(/\s+/g, " ");
    return { disabled: await button.first().isDisabled(), label };
  }

  async openResetPassword(params: { uid: string; token: string; email: string }): Promise<void> {
    const query = new URLSearchParams({ uidb64: params.uid, token: params.token, email: params.email });
    await this.page.goto(`/accounts/reset-password?${query.toString()}`);
    // Exact: the show/hide toggle ("Show new password") would also match.
    await this.page.getByLabel("New password", { exact: true }).waitFor({ timeout: WebNewDriver.WAIT_MS });
  }

  async submitNewPassword(password: string, confirmPassword: string): Promise<void> {
    // The reset form's fields; the card's own confirm lives on another
    // route, so the label reads stay unambiguous per page.
    await this.page.getByLabel("New password", { exact: true }).fill(password);
    await this.page.getByLabel("Confirm password", { exact: true }).fill(confirmPassword);
    // Success navigates to the entry, failure renders an inline banner;
    // wait for either so callers read a settled page.
    const before = this.page.url();
    const submit = this.page.locator("form").getByRole("button", { name: "Reset password" });
    await submit.click();
    await Promise.race([
      this.page
        .waitForURL((url) => url.href !== before, { timeout: 60_000 })
        .then(
          () => true,
          () => false
        ),
      this.page
        .getByRole("alert")
        .first()
        .waitFor({ timeout: 60_000 })
        .then(
          () => true,
          () => false
        ),
    ]);
  }

  async openSetPassword(): Promise<void> {
    await this.page.goto("/accounts/set-password");
    await this.page.waitForLoadState("domcontentloaded");
  }

  async openPath(path: string): Promise<void> {
    await this.page.goto(path);
    await this.page.waitForLoadState("domcontentloaded");
  }

  // --- NEWFRONT-113 (rules) stubs: mirror of the oracle driver additions. ---

  async rulesOpenIssueDetail(_workspaceSlug: string, _projectId: string, _issueId: string): Promise<void> {
    return todo("rulesOpenIssueDetail");
  }

  async rulesOpenIntakeIssue(_workspaceSlug: string, _projectId: string, _inboxIssueId: string): Promise<void> {
    return todo("rulesOpenIntakeIssue");
  }

  async rulesCommentBodyText(_commentId: string): Promise<string | null> {
    return todo("rulesCommentBodyText");
  }

  async rulesCommentMenuOptions(_commentId: string): Promise<string[]> {
    return todo("rulesCommentMenuOptions");
  }

  async rulesChooseCommentMenuOption(_commentId: string, _option: RulesCommentMenuOption): Promise<void> {
    return todo("rulesChooseCommentMenuOption");
  }

  async rulesReadClipboard(): Promise<string> {
    return todo("rulesReadClipboard");
  }

  async rulesOpenDeepLink(_url: string): Promise<void> {
    return todo("rulesOpenDeepLink");
  }

  async rulesCommentHighlighted(_commentId: string): Promise<boolean> {
    return todo("rulesCommentHighlighted");
  }

  async rulesCommentAccessBadge(_commentId: string): Promise<"internal" | "public" | "hidden"> {
    return todo("rulesCommentAccessBadge");
  }

  async rulesLastToast(): Promise<{ title: string; message: string } | null> {
    return todo("rulesLastToast");
  }

  async rulesReload(): Promise<void> {
    return todo("rulesReload");
  }

  async rulesEnsureSignedIn(_email: string, _password: string, _workspaceSlug: string): Promise<void> {
    return todo("rulesEnsureSignedIn");
  }

  async rulesOpenIssueDetailRaw(_workspaceSlug: string, _projectId: string, _issueId: string): Promise<void> {
    return todo("rulesOpenIssueDetailRaw");
  }

  async rulesIssueMissingVisible(): Promise<boolean> {
    return todo("rulesIssueMissingVisible");
  }

  async rulesCommentComposerVisible(): Promise<boolean> {
    return todo("rulesCommentComposerVisible");
  }

  async rulesCommentCardVisible(_commentId: string): Promise<boolean> {
    return todo("rulesCommentCardVisible");
  }

  async rulesCommentCardText(_commentId: string): Promise<string> {
    return todo("rulesCommentCardText");
  }

  async rulesIntakeTriageVisible(): Promise<boolean> {
    return todo("rulesIntakeTriageVisible");
  }

  // Shell chrome (NEWFRONT-126, rows SHELL-001/002; NEWFRONT-288): the
  // route shell mounts the sidebar and the overlay portal on every
  // workspace page. Later chrome slices extend the probes they own
  // (rail, tabs, toggle, personalization) without touching these.
  private async gotoShell(url: string): Promise<void> {
    for (let attempt = 0; attempt < 3; attempt += 1) {
      await this.page.goto(url);
      await this.page.waitForLoadState("domcontentloaded");
      try {
        // Attached, not visible: a collapsed sidebar is in the DOM at zero
        // width, and navigation must still count it as mounted.
        await this.page.locator("#main-sidebar").waitFor({ state: "attached", timeout: 45_000 });
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

  async sidebarPresent(): Promise<boolean> {
    return (await this.page.locator("#main-sidebar").count()) > 0;
  }

  async sidebarWidth(): Promise<number | null> {
    const sidebar = this.page.locator("#main-sidebar");
    if ((await sidebar.count()) === 0) return null;
    const box = await sidebar.boundingBox();
    if (box === null) return null;
    // A collapsed sidebar keeps a one-pixel border in its box; report that
    // as closed so callers read a clean open/closed signal.
    const width = Math.round(box.width);
    return width <= 1 ? 0 : width;
  }

  async portalPresent(): Promise<boolean> {
    return (await this.page.locator("#full-screen-portal").count()) > 0;
  }

  async railPresent(): Promise<boolean> {
    // App rail (NEWFRONT-289, rows SHELL-063/099): the edition-gated
    // strip; suppressed builds never mount it.
    return (await this.page.locator('[data-testid="workspace-rail"]').count()) > 0;
  }

  async contentPaddingLeft(): Promise<number | null> {
    // Suppressed-rail pages reclaim the rail's padding on the content
    // landmark (SHELL-099); railed builds render the landmark flush.
    const main = this.page.locator('main[aria-label="Content"]');
    if ((await main.count()) === 0) return null;
    const raw = await main.first().evaluate((el) => getComputedStyle(el).paddingLeft);
    const parsed = Number.parseFloat(raw);
    return Number.isNaN(parsed) ? null : parsed;
  }

  private tabStrip() {
    return this.page.locator('[data-testid="project-tab-strip"]');
  }

  async projectTabs(): Promise<Array<{ name: string; href: string }>> {
    // Project tab strip (NEWFRONT-291): visible tabs only. The overflow
    // popover renders at the document level, outside the strip.
    const links = this.tabStrip().getByRole("link");
    const count = await links.count();
    const tabs: Array<{ name: string; href: string }> = [];
    for (let index = 0; index < count; index += 1) {
      const link = links.nth(index);
      const name = ((await link.textContent()) ?? "").trim().replace(/\s+/g, " ");
      if (name === "") continue;
      tabs.push({ name, href: (await link.getAttribute("href")) ?? "" });
    }
    return tabs;
  }

  async activeTabName(): Promise<string | null> {
    const active = this.tabStrip().locator('a[aria-current="page"]');
    if ((await active.count()) === 0) return null;
    const name = ((await active.first().textContent()) ?? "").trim().replace(/\s+/g, " ");
    return name === "" ? null : name;
  }

  async editionBadgePresent(): Promise<boolean> {
    // Negative row (SHELL-100): no edition badge mounts in any build.
    // Any future badge must carry this testid so the probe keeps
    // detecting it.
    return (await this.page.locator('[data-testid="edition-badge"]').count()) > 0;
  }

  async desktopUpdatePresent(): Promise<boolean> {
    // Desktop update control (SHELL-102): the Desktop area renders its
    // waiting-update button into the sidebar slot; the web sidebar
    // carries no update affordance.
    const sidebar = this.page.locator("#main-sidebar");
    return (await sidebar.getByRole("button", { name: /update/i }).count()) > 0;
  }

  async upgradePillCount(): Promise<number> {
    // Upgrade pill (SHELL-101): the kit pill renders the exact plan
    // marker as plain text, never a control.
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
    // Route shells (NEWFRONT-288): read what the chrome actually mounts.
    // The switcher, toggle, search, inbox, help, star and account controls
    // arrive with the top-bar slices, which extend these reads in place.
    // Sidebar chrome menus (NEWFRONT-287): the compact account control
    // mounts only where the sidebar is collapsed or unmounted.
    const page = this.page;
    return {
      workspaceMenu: (await page.getByRole("button", { name: "Open workspace switcher" }).count()) > 0,
      sidebarToggle: (await page.locator('[data-testid="sidebar-toggle"]').count()) > 0,
      search: (await page.locator('input[placeholder*="Search" i], input[type="search"]').count()) > 0,
      inbox: (await page.locator('a[href*="/notifications"]').count()) > 0,
      help: (await page.locator("button:has(svg.lucide-circle-help)").count()) > 0,
      starLink: (await page.getByRole("link", { name: "Star us on GitHub" }).count()) > 0,
      accountFallback: (await page.locator('[data-testid="topbar-user-menu"]').count()) > 0,
    };
  }

  async toggleSidebar(): Promise<void> {
    // Sidebar shell (NEWFRONT-290): the top-bar toggle flips collapse.
    await this.page.locator('[data-testid="sidebar-toggle"]').first().click();
  }

  /** The open personalization dialog. */
  private personalizeDialog(): Locator {
    return this.page.locator('[role="dialog"]');
  }

  async openPersonalizeDialog(): Promise<void> {
    // Sidebar shell (NEWFRONT-290): the header launcher opens the dialog.
    await this.page.locator('[data-testid="personalize-nav"]').click();
    await this.page.getByRole("heading", { name: "Personalize sidebar" }).waitFor({ timeout: 15_000 });
  }

  async personalizeDialogOpen(): Promise<boolean> {
    return (await this.page.getByRole("heading", { name: "Personalize sidebar" }).count()) > 0;
  }

  private personalItemRow(name: string): Locator {
    // The inner locator roots at the page: filter() re-roots it at
    // each candidate, so a dialog-rooted inner could never match a row.
    return this.personalizeDialog()
      .locator('[data-testid="personal-row"]')
      .filter({ has: this.page.getByText(name, { exact: true }) });
  }

  async personalItemChecked(name: string): Promise<boolean | null> {
    const box = this.personalItemRow(name).locator('input[type="checkbox"]');
    if ((await box.count()) === 0) return null;
    return await box.first().isChecked();
  }

  async setPersonalItemEnabled(name: string, enabled: boolean): Promise<void> {
    const box = this.personalItemRow(name).locator('input[type="checkbox"]').first();
    await box.scrollIntoViewIfNeeded();
    const checked = await box.isChecked();
    if (checked !== enabled) await box.click({ force: true });
  }

  async movePersonalItem(dragged: string, target: string): Promise<void> {
    const source = this.personalItemRow(dragged);
    const dest = this.personalItemRow(target);
    const box = await dest.boundingBox();
    // Land near the target's bottom edge so the drop resolves past the
    // target's slot instead of back into the dragged row's own.
    const targetPosition = box === null ? undefined : { x: box.width / 2, y: box.height - 4 };
    await source.dragTo(dest, { targetPosition });
  }

  async personalItemNames(): Promise<string[]> {
    const labels = this.personalizeDialog().locator('[data-testid="personal-row-label"]');
    const texts = await labels.allTextContents();
    return texts.map((text) => text.trim()).filter((text) => text.length > 0);
  }

  async projectNavMode(): Promise<"ACCORDION" | "TABBED" | null> {
    const dialog = this.personalizeDialog();
    if ((await dialog.locator('[data-testid="project-nav-accordion"]').count()) === 0) return null;
    const tabbed = dialog.locator('[data-testid="project-nav-tabbed"]');
    return (await tabbed.isChecked()) ? "TABBED" : "ACCORDION";
  }

  async setProjectNavMode(mode: "ACCORDION" | "TABBED"): Promise<void> {
    // Plain click, not check(): the radio is server-driven (the checked
    // state flips only after the mutation round-trips), and check()
    // fails when the state does not change synchronously. Callers poll
    // the server write after.
    const radio = this.personalizeDialog().locator(
      `[data-testid="project-nav-${mode === "TABBED" ? "tabbed" : "accordion"}"]`
    );
    await radio.click();
  }

  async projectCapInput(): Promise<string | null> {
    const dialog = this.personalizeDialog();
    const toggle = dialog.locator('[data-testid="project-cap-toggle"]');
    if ((await toggle.count()) === 0) return null;
    if (!(await toggle.isChecked())) return null;
    const input = dialog.locator('[data-testid="project-cap-input"]');
    if ((await input.count()) === 0) return null;
    return await input.first().inputValue();
  }

  async projectCapEnabled(): Promise<boolean | null> {
    const toggle = this.personalizeDialog().locator('[data-testid="project-cap-toggle"]');
    if ((await toggle.count()) === 0) return null;
    return await toggle.isChecked();
  }

  async setProjectCap(enabled: boolean, count?: number): Promise<void> {
    const dialog = this.personalizeDialog();
    const toggle = dialog.locator('[data-testid="project-cap-toggle"]');
    const checked = await toggle.isChecked();
    if (checked !== enabled) await toggle.click({ force: true });
    if (enabled && count !== undefined) {
      const input = dialog.locator('[data-testid="project-cap-input"]').first();
      await input.fill(String(count));
    }
  }

  async projectHeaderText(): Promise<string | null> {
    // Project header (NEWFRONT-291): the switcher trigger names the
    // current project, avatar initials included in the raw text.
    const trigger = this.page.locator('[data-testid="project-switcher-trigger"]');
    if ((await trigger.count()) === 0) return null;
    const text = ((await trigger.first().textContent()) ?? "").trim().replace(/\s+/g, " ");
    return text === "" ? null : text;
  }

  async projectHeaderTruncated(): Promise<boolean> {
    const line = this.page.locator('[data-testid="project-switcher-name"]');
    if ((await line.count()) === 0) return false;
    return await line.first().evaluate((node) => {
      const element = node as HTMLElement;
      return element.scrollWidth > element.clientWidth && getComputedStyle(element).textOverflow === "ellipsis";
    });
  }

  async openProjectSwitcher(): Promise<void> {
    await this.page.locator('[data-testid="project-switcher-trigger"]').first().click();
  }

  async switcherOptionNames(): Promise<string[]> {
    // Avatar initials ride in aria-hidden marks; only the label text
    // counts toward the option name.
    const names = await this.page.getByRole("menuitem").evaluateAll((items) =>
      items.map((item) => {
        const parts: string[] = [];
        item.childNodes.forEach((node) => {
          if (node.nodeType === Node.TEXT_NODE) parts.push(node.textContent ?? "");
          else if (node instanceof HTMLElement && node.getAttribute("aria-hidden") !== "true") {
            parts.push(node.textContent ?? "");
          }
        });
        return parts.join("").trim().replace(/\s+/g, " ");
      })
    );
    return names.filter((name) => name.length > 0);
  }

  async chooseSwitcherOption(name: string): Promise<void> {
    await this.page.getByRole("menuitem", { name }).first().click();
  }

  async openProjectActions(): Promise<void> {
    await this.page.locator('[data-testid="project-actions-trigger"]').first().click();
  }

  async projectActionNames(): Promise<string[]> {
    const texts = await this.page.getByRole("menuitem").allTextContents();
    return texts.map((text) => text.trim().replace(/\s+/g, " ")).filter((text) => text.length > 0);
  }

  async clickProjectAction(name: string): Promise<void> {
    await this.page.getByRole("menuitem", { name }).first().click();
  }

  async readClipboardText(): Promise<string> {
    return await this.page.evaluate(() => navigator.clipboard.readText());
  }

  async toastText(): Promise<string | null> {
    const stack = this.page.locator('[data-testid="toast-stack"]');
    if ((await stack.count()) === 0) return null;
    const text = ((await stack.first().textContent()) ?? "").trim().replace(/\s+/g, " ");
    return text === "" ? null : text;
  }

  async rightClickTab(name: string): Promise<void> {
    await this.tabStrip().getByRole("link", { name, exact: true }).click({ button: "right" });
  }

  async contextMenuItems(): Promise<string[]> {
    // Kit menus unmount with their trigger, so the open menu owns every
    // item on the page.
    const texts = await this.page.getByRole("menuitem").allTextContents();
    return texts.map((text) => text.trim().replace(/\s+/g, " ")).filter((text) => text.length > 0);
  }

  async clickContextMenuItem(name: string): Promise<void> {
    await this.page.getByRole("menuitem", { name }).first().click();
  }

  async openOverflowMenu(): Promise<void> {
    await this.page.locator('[data-testid="tab-overflow-trigger"]').first().click();
  }

  async overflowTriggerPresent(): Promise<boolean> {
    const trigger = this.page.locator('[data-testid="tab-overflow-trigger"]');
    if ((await trigger.count()) === 0) return false;
    return await trigger.first().isVisible();
  }

  async overflowRowNames(): Promise<string[]> {
    const menu = this.page.locator('[data-testid="tab-overflow-menu"]');
    if ((await menu.count()) === 0) return [];
    const texts = await menu.getByRole("link").allTextContents();
    return texts.map((text) => text.trim().replace(/\s+/g, " ")).filter((text) => text.length > 0);
  }

  async restoreOverflowTab(name: string): Promise<void> {
    // The row's Show control carries the tab name ("Show Pages"), so no
    // row filter is needed (a filter whose inner locator roots at an
    // ancestor of its candidates never matches).
    const menu = this.page.locator('[data-testid="tab-overflow-menu"]');
    const show = menu.getByRole("button", { name: `Show ${name}`, exact: true }).first();
    await show.hover();
    await show.click();
  }

  async setViewportSize(width: number, height: number): Promise<void> {
    await this.page.setViewportSize({ width, height });
  }

  async openNotifications(workspaceSlug: string): Promise<void> {
    // Sidebar chrome menus (NEWFRONT-287): the notifications address
    // renders without the app sidebar (SHELL-062), so this waits for
    // the document, not the sidebar mount.
    await this.page.goto(`/${workspaceSlug}/notifications/`, { timeout: 60_000 });
    await this.page.waitForLoadState("domcontentloaded", { timeout: 60_000 });
  }

  // --- Issues bulk-ops / modal / drafts stubs (NEWFRONT-120).
  async selectionCheckboxCount(): Promise<number> {
    return todo("selectionCheckboxCount");
  }

  async bulkBarVisible(): Promise<boolean> {
    return todo("bulkBarVisible");
  }

  async pressKey(key: string, shift?: boolean): Promise<void> {
    if (shift === true) await this.page.keyboard.down("Shift");
    try {
      await this.page.keyboard.press(key);
    } finally {
      if (shift === true) await this.page.keyboard.up("Shift");
    }
  }

  async reloadSawDialog(): Promise<boolean> {
    return todo("reloadSawDialog");
  }

  async dismissWelcomeDialog(): Promise<void> {
    return todo("dismissWelcomeDialog");
  }

  async openCreateModal(): Promise<void> {
    return todo("openCreateModal");
  }

  async createModalOpen(): Promise<boolean> {
    return todo("createModalOpen");
  }

  async createModalHeading(): Promise<string> {
    return todo("createModalHeading");
  }

  async fillCreateTitle(_title: string): Promise<void> {
    return todo("fillCreateTitle");
  }

  async createTitleValue(): Promise<string> {
    return todo("createTitleValue");
  }

  async createTitleError(): Promise<string> {
    return todo("createTitleError");
  }

  async submitCreateModal(): Promise<void> {
    return todo("submitCreateModal");
  }

  async clickModalDiscard(): Promise<void> {
    return todo("clickModalDiscard");
  }

  async enableCreateMore(): Promise<void> {
    return todo("enableCreateMore");
  }

  async modalPrimaryButtonLabel(): Promise<string> {
    return todo("modalPrimaryButtonLabel");
  }

  async gitBranchValue(): Promise<string> {
    return todo("gitBranchValue");
  }

  async createTitleFocused(): Promise<boolean> {
    return todo("createTitleFocused");
  }

  async modalTextContains(_text: string): Promise<boolean> {
    return todo("modalTextContains");
  }

  async confirmSaveDraft(): Promise<void> {
    return todo("confirmSaveDraft");
  }

  async cancelDiscardDialog(): Promise<void> {
    return todo("cancelDiscardDialog");
  }

  async discardDialogDiscard(): Promise<void> {
    return todo("discardDialogDiscard");
  }

  async openDraftForEdit(_name: string): Promise<void> {
    return todo("openDraftForEdit");
  }

  async publishDraft(): Promise<void> {
    return todo("publishDraft");
  }

  async openRowMenuEntry(_issueName: string, _entry: string): Promise<void> {
    return todo("openRowMenuEntry");
  }

  async modalButtonDisabled(_name: string): Promise<boolean> {
    return todo("modalButtonDisabled");
  }

  async openParentPicker(): Promise<void> {
    return todo("openParentPicker");
  }

  async searchParentInModal(_query: string): Promise<void> {
    return todo("searchParentInModal");
  }

  async selectParentResult(_issueName: string): Promise<void> {
    return todo("selectParentResult");
  }

  async parentResultNewTabLinks(): Promise<number> {
    return todo("parentResultNewTabLinks");
  }

  async removeParentInModal(_issueName: string): Promise<void> {
    return todo("removeParentInModal");
  }

  async openLabelsPicker(): Promise<void> {
    return todo("openLabelsPicker");
  }

  async createLabelInModal(_name: string): Promise<void> {
    return todo("createLabelInModal");
  }

  async selectedLabelVisible(_name: string): Promise<boolean> {
    return todo("selectedLabelVisible");
  }

  async hoverIssueRow(_issueName: string): Promise<void> {
    return todo("hoverIssueRow");
  }

  async openToastViewAction(): Promise<string> {
    return todo("openToastViewAction");
  }

  async confirmArchive(): Promise<void> {
    // Scoped to the dialog: the still-open card menu behind it
    // carries its own same-named entry.
    await this.projectDialog().getByRole("button", { name: "Archive", exact: true }).first().click();
  }

  async confirmDeleteIssue(): Promise<void> {
    return todo("confirmDeleteIssue");
  }

  async modalHasPlaceholder(_placeholder: string): Promise<boolean> {
    return todo("modalHasPlaceholder");
  }

  async fillModalPlaceholder(_placeholder: string, _text: string): Promise<void> {
    return todo("fillModalPlaceholder");
  }

  async expandListRows(): Promise<boolean> {
    return todo("expandListRows");
  }

  async switchIssueLayout(_label: string): Promise<void> {
    return todo("switchIssueLayout");
  }

  async ensureListLayout(): Promise<void> {
    return todo("ensureListLayout");
  }

  async toggleAdvancedGit(): Promise<void> {
    return todo("toggleAdvancedGit");
  }

  async fillGitBranch(_branch: string): Promise<void> {
    return todo("fillGitBranch");
  }

  async gitBranchError(): Promise<string> {
    return todo("gitBranchError");
  }

  async fillDescription(_text: string): Promise<void> {
    return todo("fillDescription");
  }

  async countText(_text: string): Promise<number> {
    return todo("countText");
  }

  async openDraftsPage(_workspaceSlug: string): Promise<void> {
    return todo("openDraftsPage");
  }

  async visibleDraftNames(): Promise<string[]> {
    return todo("visibleDraftNames");
  }

  async openCreateDraftModal(): Promise<void> {
    return todo("openCreateDraftModal");
  }

  async draftBlockCount(): Promise<number> {
    return todo("draftBlockCount");
  }

  async draftBlockText(_name: string): Promise<string> {
    return todo("draftBlockText");
  }

  async settleDraftsPage(_workspaceSlug: string): Promise<"empty" | "list"> {
    return todo("settleDraftsPage");
  }

  async pageTextContains(_text: string): Promise<boolean> {
    return todo("pageTextContains");
  }

  async deleteDraftByName(_name: string): Promise<void> {
    return todo("deleteDraftByName");
  }

  async signInWithPasswordRetry(_email: string, _password: string): Promise<void> {
    return todo("signInWithPasswordRetry");
  }

  async openProjectIssuesSettled(workspaceSlug: string, projectId: string): Promise<void> {
    // Land on a settled project page so the palette has full context;
    // openProjectIssues already navigates and waits for the settled
    // list, gate denial or signed-out entry.
    await this.openProjectIssues(workspaceSlug, projectId);
  }

  async copyDraftByName(_name: string): Promise<void> {
    return todo("copyDraftByName");
  }

  async moveDraftToProject(_name: string): Promise<void> {
    return todo("moveDraftToProject");
  }

  async confirmMoveToProject(): Promise<void> {
    return todo("confirmMoveToProject");
  }

  async modalProjectName(): Promise<string> {
    return todo("modalProjectName");
  }

  async selectModalProject(_name: string): Promise<void> {
    return todo("selectModalProject");
  }

  async hoverCardRead(_issueName: string): Promise<{ text: string; priorityIcon: string; dateColor: string }> {
    return todo("hoverCardRead");
  }

  async modalTabOrder(): Promise<string[]> {
    return todo("modalTabOrder");
  }

  async focusCreateTitle(): Promise<void> {
    return todo("focusCreateTitle");
  }

  async openCyclePage(_workspaceSlug: string, _projectId: string, _cycleId: string): Promise<void> {
    return todo("openCyclePage");
  }

  async openModulePage(_workspaceSlug: string, _projectId: string, _moduleId: string): Promise<void> {
    return todo("openModulePage");
  }

  // --- NEWFRONT-123 (home) stubs: mirror of the oracle driver additions. ---

  // Home top half (NEWFRONT-278, rows SHELL-003-010): greeting, tour,
  // assistant card and quickstart probes against the new dashboard DOM.
  // Widget, link, recents and chrome probes stay stubs for NEWFRONT-279
  // and NEWFRONT-280, which extend this section without touching these.
  private async gotoHome(url: string): Promise<void> {
    // Settles when the greeting, the assistant card, the quickstart
    // panel or the tour overlay shows up; the dev server compiles the
    // route on first visit, so the navigation is retried once on a
    // slow first paint.
    for (let attempt = 0; attempt < 2; attempt += 1) {
      await this.page.goto(url);
      await this.page.waitForLoadState("domcontentloaded");
      const settled = await this.page
        .locator('#home-greeting, #home-assistant-card, #home-quickstart, [role="dialog"]')
        .first()
        .waitFor({ timeout: 90_000 })
        .then(() => true)
        .catch(() => false);
      if (settled) return;
    }
    throw new Error(`[parity] home dashboard never settled at ${this.page.url()}.`);
  }

  async homeOpen(workspaceSlug: string): Promise<void> {
    await this.gotoHome(`/${workspaceSlug}/`);
  }

  async homeGreetingHeading(): Promise<string | null> {
    const heading = this.page.locator("#home-greeting").first();
    if ((await heading.count()) === 0) return null;
    const text = (await heading.innerText()).trim();
    return text === "" ? null : text;
  }

  async homeDateLine(): Promise<string | null> {
    const line = this.page.locator("#home-date-line").first();
    if ((await line.count()) === 0) return null;
    const text = (await line.innerText()).trim();
    return text === "" ? null : text;
  }

  private homeTourOverlay(): Locator {
    // The product-tour dialog: the welcome card offers starting or
    // declining, each stop offers Back/Next, and the last stop finishes
    // into project creation. Scoping on those controls keeps middle
    // stops visible too.
    return this.page.locator('[role="dialog"]').filter({
      has: this.page.getByRole("button", {
        name: /take the tour|explore on my own|close the tour|^next$|^back$|start your first project/i,
      }),
    });
  }

  async homeTourVisible(): Promise<boolean> {
    // Count plus visibility: a hidden remnant must not keep the
    // walk loop alive after the tour unmounts.
    const overlay = this.homeTourOverlay().first();
    return (await overlay.count()) > 0 && (await overlay.isVisible());
  }

  async homeTourAdvance(): Promise<void> {
    const overlay = this.homeTourOverlay().first();
    const start = overlay.getByRole("button", { name: /take the tour/i }).first();
    if ((await start.count()) > 0) {
      await start.click();
      return;
    }
    const next = overlay.getByRole("button", { name: /^next$/i }).first();
    if ((await next.count()) > 0) {
      await next.click();
      return;
    }
    // Only the last stop reaches here, and its click unmounts the tour:
    // a fast walk loop can re-enter while the stop is still painted, so
    // a mid-click unmount reads as completion instead of hanging.
    try {
      await overlay
        .getByRole("button", { name: /start your first project/i })
        .first()
        .click({ timeout: 15_000 });
    } catch (error) {
      if (!(await this.homeTourVisible())) return;
      throw error;
    }
  }

  async homeTourDismiss(): Promise<void> {
    const overlay = this.homeTourOverlay().first();
    const decline = overlay.getByRole("button", { name: /explore on my own/i }).first();
    if ((await decline.count()) > 0) {
      await decline.click();
      return;
    }
    await overlay
      .getByRole("button", { name: /close the tour/i })
      .first()
      .click();
  }

  private homeAssistantCard(): Locator {
    return this.page.locator("#home-assistant-card");
  }

  async homeAssistantState(): Promise<"hidden" | "setup" | "ready"> {
    const card = this.homeAssistantCard().first();
    if ((await card.count()) === 0) return "hidden";
    const state = await card.getAttribute("data-assistant-state");
    return state === "setup" ? "setup" : "ready";
  }

  async homeAssistantSuggestions(): Promise<string[]> {
    const card = this.homeAssistantCard().first();
    if ((await card.count()) === 0) return [];
    const texts = await card.locator("[data-suggestion]").allTextContents();
    return texts.map((text) => text.trim()).filter((text) => text.length > 0);
  }

  private homeQuickstartPanel(): Locator {
    return this.page.locator("#home-quickstart");
  }

  async homeQuickstartVisible(): Promise<boolean> {
    return (await this.homeQuickstartPanel().first().count()) > 0;
  }

  async homeQuickstartTitles(): Promise<string[]> {
    const panel = this.homeQuickstartPanel().first();
    if ((await panel.count()) === 0) return [];
    const texts = await panel.locator("h3").allTextContents();
    return [...new Set(texts.map((text) => text.trim()).filter((text) => text.length > 0))];
  }

  async homeQuickstartCreateEnabled(): Promise<boolean> {
    if (!(await this.homeQuickstartVisible())) return false;
    const actions = await this.homeQuickstartActionTexts();
    return actions.some((text) => /get started/i.test(text));
  }

  async homeQuickstartDismiss(): Promise<void> {
    const panel = this.homeQuickstartPanel().first();
    await panel
      .getByRole("button", { name: /^dismiss$/i })
      .first()
      .click();
  }

  async homeQuickstartCardDone(title: string): Promise<boolean> {
    // A finished card swaps its action for a done marker and carries the
    // verdict on a data flag; absence of any action is NOT done, since
    // forbidden actions also render nothing clickable.
    const panel = this.homeQuickstartPanel().first();
    if ((await panel.count()) === 0) return false;
    const card = panel
      .locator("[data-card]")
      .filter({ has: this.page.getByRole("heading", { name: title }) })
      .last();
    if ((await card.count()) === 0) return false;
    return (await card.getAttribute("data-done")) === "true";
  }

  async homeQuickstartActionTexts(): Promise<string[]> {
    const panel = this.homeQuickstartPanel().first();
    if ((await panel.count()) === 0) return [];
    const links = await panel.getByRole("link").allTextContents();
    const buttons = await panel.getByRole("button").allTextContents();
    return [...links, ...buttons]
      .map((text) => text.trim())
      .filter((text) => text.length > 0 && !/^dismiss$/i.test(text));
  }

  // Recents plus dashboard chrome (NEWFRONT-280, rows SHELL-014-017,
  // 021-023): widget, recents and chrome probes against the new dashboard
  // DOM. The manage-dialog probes assume NEWFRONT-279's dialog contract:
  // the widget name as exact text, a role="switch" toggle beside it, and
  // a drag-handle button in its row; 279's own probes follow below.

  private homeWidgetSectionTitles(): Locator {
    return this.page.locator("#home-widget-stack [data-widget-title]");
  }

  async homeWidgetTitles(): Promise<string[]> {
    // Section titles in render order, so reorder and refresh assertions
    // observe the sequence the user actually sees. Each widget section
    // carries its display title as data-widget-title.
    const nodes = this.homeWidgetSectionTitles();
    const titles: string[] = [];
    for (let index = 0; index < (await nodes.count()); index += 1) {
      const title = ((await nodes.nth(index).getAttribute("data-widget-title")) ?? "").trim();
      if (title !== "") titles.push(title);
    }
    return titles;
  }

  async homeOpenManageWidgets(): Promise<void> {
    await this.page
      .getByRole("button", { name: /manage widgets/i })
      .first()
      .click();
    await this.page.locator('[role="dialog"]').first().waitFor({ state: "visible", timeout: 15_000 });
  }

  async homeCloseManageWidgets(): Promise<void> {
    await this.page.keyboard.press("Escape");
  }

  private homeManageDialog(): Locator {
    return this.page.locator('[role="dialog"]').first();
  }

  async homeManageWidgetNames(): Promise<string[]> {
    const dialog = this.homeManageDialog();
    const texts = await dialog.locator("p, span, div").allTextContents();
    return [...new Set(texts.map((t) => t.trim()).filter((t) => t.length > 0 && t.length < 60))];
  }

  private homeManageName(name: string): Locator {
    return this.homeManageDialog().getByText(name, { exact: true }).first();
  }

  private homeManageSwitch(name: string): Locator {
    // The toggle sits beside the name cell inside their shared wrapper.
    return this.homeManageName(name).locator("xpath=ancestor::div[2]").getByRole("switch").first();
  }

  async homeManageWidgetEnabled(name: string): Promise<boolean> {
    const toggle = this.homeManageSwitch(name);
    await toggle.waitFor({ state: "visible", timeout: 15_000 });
    return (await toggle.getAttribute("aria-checked")) === "true";
  }

  async homeToggleManageWidget(name: string): Promise<void> {
    const toggle = this.homeManageSwitch(name);
    await toggle.waitFor({ state: "visible", timeout: 15_000 });
    await toggle.click();
  }

  async homeDragWidget(sourceName: string, targetName: string): Promise<void> {
    // The drag listener lives on the row's handle button, so the handle
    // starts the gesture; dropping near the target row's bottom edge
    // asks for a below-drop, which visibly moves a first-row widget.
    const source = this.homeManageName(sourceName).locator("xpath=ancestor::div[1]").getByRole("button").first();
    const targetRow = this.homeManageName(targetName).locator("xpath=ancestor::div[1]");
    const srcBox = await source.boundingBox();
    if (!srcBox) throw new Error("[parity] widget drag source has no layout box.");
    const dstBox = await targetRow.boundingBox();
    if (!dstBox) throw new Error("[parity] widget drag target has no layout box.");
    await this.page.mouse.move(srcBox.x + srcBox.width / 2, srcBox.y + srcBox.height / 2);
    await this.page.mouse.down();
    await this.page.mouse.move(dstBox.x + dstBox.width / 2, dstBox.y + dstBox.height - 6, { steps: 12 });
    await this.page.mouse.up();
  }

  // Quicklinks plus widget management (NEWFRONT-279, rows SHELL-011-013,
  // 018-020): all-off and quicklink probes against the new dashboard DOM.
  // Rows carry their title in a test id and open their address through a
  // new-tab anchor; copy, edit and delete run through the row menu, and
  // the link dialog reports validation through an alert.

  async homeAllOffVisible(): Promise<boolean> {
    const empty = this.page.getByTestId("home-all-off").first();
    if ((await empty.count()) === 0) return false;
    return empty.isVisible().catch(() => false);
  }

  private homeLinksSection(): Locator {
    return this.page.locator("#home-quicklinks");
  }

  async homeQuickLinkNames(): Promise<string[]> {
    if ((await this.homeLinksSection().count()) === 0) return [];
    const titles = await this.homeLinksSection().getByTestId("quicklink-title").allTextContents();
    return [...new Set(titles.map((title) => title.trim()).filter((title) => title.length > 0))];
  }

  async homeExpandQuickLinks(): Promise<void> {
    await this.homeLinksSection()
      .getByRole("button", { name: /show all/i })
      .first()
      .click();
  }

  async homeQuickLinksCollapsed(): Promise<boolean> {
    if ((await this.homeLinksSection().count()) === 0) return false;
    return (
      (await this.homeLinksSection()
        .getByRole("button", { name: /show all|show less/i })
        .count()) > 0
    );
  }

  private homeLinkDialog(): Locator {
    return this.page.getByRole("dialog", { name: /add link|edit link/i }).first();
  }

  private async homeLinkFormFill(title: string, url: string): Promise<void> {
    const dialog = this.homeLinkDialog();
    await dialog.getByLabel("Address").fill(url);
    await dialog.getByLabel("Name").fill(title);
  }

  async homeAddQuickLink(title: string, url: string): Promise<void> {
    await this.homeLinksSection()
      .getByRole("button", { name: /add link/i })
      .first()
      .click();
    await this.homeLinkDialog().waitFor({ state: "visible", timeout: 15_000 });
    await this.homeLinkFormFill(title, url);
    await this.homeLinkDialog()
      .getByRole("button", { name: /save link/i })
      .first()
      .click();
  }

  private homeLinkRow(title: string): Locator {
    const exact = title.replace(/"/g, "");
    return this.homeLinksSection()
      .locator(`xpath=.//li[.//*[@data-testid="quicklink-title" and normalize-space()="${exact}"]]`)
      .first();
  }

  private async homeLinkRowMenu(title: string): Promise<void> {
    const row = this.homeLinkRow(title);
    await row.waitFor({ state: "visible", timeout: 15_000 });
    await row
      .getByRole("button", { name: /link actions/i })
      .first()
      .click();
  }

  private async homeLinkMenuPick(name: RegExp): Promise<void> {
    const item = this.page.getByRole("menuitem", { name }).first();
    await item.waitFor({ state: "visible", timeout: 15_000 });
    await item.click();
  }

  async homeEditQuickLink(currentTitle: string, nextTitle: string, nextUrl: string): Promise<void> {
    await this.homeLinkRowMenu(currentTitle);
    await this.homeLinkMenuPick(/edit/i);
    await this.homeLinkDialog().waitFor({ state: "visible", timeout: 15_000 });
    await this.homeLinkFormFill(nextTitle, nextUrl);
    await this.homeLinkDialog()
      .getByRole("button", { name: /save link/i })
      .first()
      .click();
  }

  async homeDeleteQuickLink(title: string): Promise<void> {
    await this.homeLinkRowMenu(title);
    await this.homeLinkMenuPick(/delete|remove/i);
  }

  async homeLinkDialogError(): Promise<string | null> {
    const dialog = this.homeLinkDialog();
    if ((await dialog.count()) === 0) return null;
    const alert = dialog.getByRole("alert").first();
    if ((await alert.count()) === 0) return null;
    const text = (await alert.innerText()).trim();
    return text === "" ? null : text;
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
    // The row's anchor opens the saved address in a new tab.
    const row = this.homeLinkRow(title);
    await row.waitFor({ state: "visible", timeout: 15_000 });
    const anchor = row.getByRole("link").first();
    const [popup] = await Promise.all([
      this.page.waitForEvent("popup", { timeout: 15_000 }).catch(() => null),
      anchor.click(),
    ]);
    if (popup === null) return null;
    await popup.waitForLoadState("domcontentloaded").catch(() => undefined);
    return popup.url();
  }

  async homeLinkDialogOpen(): Promise<boolean> {
    return (await this.page.locator('[role="dialog"] input:visible').count()) > 0;
  }

  async homeCancelLinkDialog(): Promise<void> {
    await this.homeLinkDialog()
      .getByRole("button", { name: /cancel/i })
      .first()
      .click();
  }

  private homeRecentsSection(): Locator {
    return this.page.locator("#home-recents");
  }

  async homeSetRecentsFilter(name: "all" | "issue" | "page" | "project"): Promise<void> {
    // Native dropdown (a floating menu would cost the size budget): the
    // option values are the filter keys.
    const select = this.homeRecentsSection()
      .getByRole("combobox", { name: /filter recents/i })
      .first();
    await select.selectOption(name);
  }

  async homeRecentRowTexts(): Promise<string[]> {
    if ((await this.homeRecentsSection().count()) === 0) return [];
    const rows = await this.homeRecentsSection().getByRole("link").allTextContents();
    return [...new Set(rows.map((t) => t.trim()).filter((t) => t.length > 0))];
  }

  async homeRecentPriorityTooltips(): Promise<string[]> {
    // Priority markers carry the display label as a native tooltip
    // (SHELL-017 target per main#894); rows without a priority render
    // no marker, so only marked rows contribute.
    if ((await this.homeRecentsSection().count()) === 0) return [];
    const marks = this.homeRecentsSection().locator('[data-testid="recent-row"] span[title]');
    const count = await marks.count();
    const tooltips: string[] = [];
    for (let i = 0; i < count; i += 1) {
      const title = await marks.nth(i).getAttribute("title");
      if (title !== null && title.trim() !== "") tooltips.push(title.trim());
    }
    return tooltips;
  }

  async homeOpenRecentRow(text: string): Promise<void> {
    const section = this.homeRecentsSection();
    // Long feeds cap behind the expander, so expand first, then center
    // the row before clicking it.
    const expander = section.getByRole("button", { name: /show all/i }).first();
    if ((await expander.count()) > 0) {
      await expander.click();
    }
    const row = section
      .getByRole("link", { name: new RegExp(text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"), "i") })
      .first();
    await row.waitFor({ state: "visible", timeout: 15_000 });
    await row.evaluate((element) => element.scrollIntoView({ block: "center" }));
    await row.click();
  }

  async homeIssuePreviewVisible(): Promise<boolean> {
    // The work-item preview mounts as a side panel inside the fullscreen
    // portal without leaving home; absence is an honest negative signal.
    const portal = this.page.locator("#full-screen-portal").first();
    if ((await portal.count()) === 0) return false;
    return (await portal.locator("div:visible, aside:visible").first().count()) > 0;
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
    // control. The crumb is the exact Home label inside the same content
    // landmark as the control (the sidebar carries its own Home link in
    // a separate landmark).
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
    const candidates = this.page.locator('[data-testid="toast-notification"]');
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

  async homeSkeletonVisible(): Promise<boolean> {
    // Loaders render shimmering placeholder tiles with skeleton test ids
    // under an aria-busy region while the dashboard settles.
    const pulsing = this.page
      .locator("[data-testid*='skeleton' i], [aria-busy='true'], .animate-pulse, .animate-shimmer")
      .first();
    if ((await pulsing.count()) === 0) return false;
    return true;
  }

  async homeWaitForWidgets(): Promise<string[]> {
    // A failed widget fetch leaves the loader mounted with no retry, so a
    // scratch-stack hiccup bricks that paint until the next load.
    const read = (): Promise<string[]> => this.homeWidgetTitles();
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

  // ---- Issue detail (NEWFRONT-121): stubs until the area lands. ----
  async openIssueDetail(_workspaceSlug: string, _issueSeqOrProjectId: string, _issueId?: string): Promise<void> {
    return todo("openIssueDetail");
  }
  async openReadOnlyIssueDetail(_workspaceSlug: string, _issueSeq: string): Promise<void> {
    return todo("openReadOnlyIssueDetail");
  }
  async issueDetailTitle(): Promise<string | null> {
    return todo("issueDetailTitle");
  }
  async issueDetailIdentifier(): Promise<string | null> {
    return todo("issueDetailIdentifier");
  }
  async editIssueTitle(_name: string): Promise<void> {
    return todo("editIssueTitle");
  }
  async saveIndicator(): Promise<string | null> {
    return todo("saveIndicator");
  }
  async descriptionText(): Promise<string | null> {
    return todo("descriptionText");
  }
  async setDescription(_text: string): Promise<void> {
    return todo("setDescription");
  }
  async sidebarProperty(_label: string): Promise<string | null> {
    return todo("sidebarProperty");
  }
  async sidebarRowPresent(_label: string): Promise<boolean> {
    return todo("sidebarRowPresent");
  }
  async sidebarRowHasControl(_label: string): Promise<boolean> {
    return todo("sidebarRowHasControl");
  }
  async pickState(_name: string): Promise<void> {
    return todo("pickState");
  }
  async pickPriority(_name: string): Promise<void> {
    return todo("pickPriority");
  }
  async copyIssueLink(): Promise<void> {
    return todo("copyIssueLink");
  }
  async lastToast(): Promise<string | null> {
    return todo("lastToast");
  }
  async readClipboard(): Promise<string> {
    return this.page.evaluate(() => navigator.clipboard.readText());
  }
  async subscribeToggle(): Promise<string | null> {
    return todo("subscribeToggle");
  }
  async clickSubscribeToggle(): Promise<void> {
    return todo("clickSubscribeToggle");
  }
  async quickActionNames(): Promise<string[]> {
    return todo("quickActionNames");
  }
  async clickQuickAction(_name: string): Promise<void> {
    return todo("clickQuickAction");
  }
  async quickActionDisabled(_name: string): Promise<boolean> {
    return todo("quickActionDisabled");
  }
  async openLegacyIssueRoute(_workspaceSlug: string, _projectId: string, _issueId: string): Promise<void> {
    return todo("openLegacyIssueRoute");
  }
  async seesDetailMissing(): Promise<boolean> {
    return todo("seesDetailMissing");
  }
  async signedIn(): Promise<boolean> {
    return todo("signedIn");
  }
  async openDescriptionHistory(): Promise<void> {
    return todo("openDescriptionHistory");
  }
  async historyVersionNames(): Promise<string[]> {
    return todo("historyVersionNames");
  }
  async restoreHistoryVersion(_name: string): Promise<void> {
    return todo("restoreHistoryVersion");
  }

  // --- Comment composer and CRUD (NEWFRONT-112). Stubs mirror the
  // interface additions so scenarios compile against either target.

  async composerOpenIssue(_workspaceSlug: string, _issueRef: string): Promise<void> {
    return todo("composerOpenIssue");
  }

  async composerType(_text: string): Promise<void> {
    return todo("composerType");
  }

  async composerPasteHtml(_html: string): Promise<void> {
    return todo("composerPasteHtml");
  }

  async composerDraftText(): Promise<string> {
    return todo("composerDraftText");
  }

  async composerSubmitDisabled(): Promise<boolean> {
    return todo("composerSubmitDisabled");
  }

  async composerSubmit(): Promise<void> {
    return todo("composerSubmit");
  }

  async composerPressEnter(): Promise<void> {
    return todo("composerPressEnter");
  }

  async composerPressShiftEnter(): Promise<void> {
    return todo("composerPressShiftEnter");
  }

  async composerAttachFile(_path: string): Promise<void> {
    return todo("composerAttachFile");
  }

  async composerVisibleCommentTexts(): Promise<string[]> {
    return todo("composerVisibleCommentTexts");
  }

  async composerOpenCommentMenu(_text: string): Promise<void> {
    return todo("composerOpenCommentMenu");
  }

  async composerMenuClick(_item: string): Promise<void> {
    return todo("composerMenuClick");
  }

  async composerEditType(_text: string): Promise<void> {
    return todo("composerEditType");
  }

  async composerEditSaveDisabled(): Promise<boolean> {
    return todo("composerEditSaveDisabled");
  }

  async composerEditSave(): Promise<void> {
    return todo("composerEditSave");
  }

  async composerEditDiscard(): Promise<void> {
    return todo("composerEditDiscard");
  }

  async composerEditPressEnter(): Promise<void> {
    return todo("composerEditPressEnter");
  }

  async composerCommentMeta(_text: string): Promise<{
    author: string;
    time: string;
    edited: boolean;
    tooltip: string | null;
  }> {
    return todo("composerCommentMeta");
  }

  async composerCommentImageCount(_text: string): Promise<number> {
    return todo("composerCommentImageCount");
  }

  async composerVisibleNotices(): Promise<{ message: string; kind: "success" | "error" | "unknown" }[]> {
    return todo("composerVisibleNotices");
  }
  async pickAssignee(_displayName: string): Promise<void> {
    return todo("pickAssignee");
  }
  async pickRunsOn(_name: string): Promise<void> {
    return todo("pickRunsOn");
  }
  async runsOnOptions(): Promise<string[]> {
    return todo("runsOnOptions");
  }
  async pickDate(_label: string, _day: string): Promise<void> {
    return todo("pickDate");
  }
  async calendarDayDisabled(_day: string): Promise<boolean> {
    return todo("calendarDayDisabled");
  }
  async clearDate(_label: string): Promise<void> {
    return todo("clearDate");
  }
  async pickCycle(_name: string): Promise<void> {
    return todo("pickCycle");
  }
  async clearCycle(): Promise<void> {
    return todo("clearCycle");
  }
  async toggleModule(_name: string): Promise<void> {
    return todo("toggleModule");
  }
  async setParentByName(_name: string): Promise<void> {
    return todo("setParentByName");
  }
  async parentBanner(_childSeq: string): Promise<string | null> {
    return todo("parentBanner");
  }
  async bannerMenuNames(_childSeq: string): Promise<string[]> {
    return todo("bannerMenuNames");
  }
  async removeParent(): Promise<void> {
    return todo("removeParent");
  }
  async openParentFromBanner(): Promise<void> {
    return todo("openParentFromBanner");
  }
  async addLabel(_name: string): Promise<void> {
    return todo("addLabel");
  }
  async removeLabel(_name: string): Promise<void> {
    return todo("removeLabel");
  }
  async openPeek(_workspaceSlug: string, _projectId: string, _issueId: string): Promise<void> {
    return todo("openPeek");
  }
  async peekOpen(): Promise<boolean> {
    return todo("peekOpen");
  }
  async peekTitle(): Promise<string | null> {
    return todo("peekTitle");
  }
  async peekIdentifier(): Promise<string | null> {
    return todo("peekIdentifier");
  }
  async closePeek(): Promise<void> {
    return todo("closePeek");
  }
  async peekCloseVisible(): Promise<boolean> {
    return todo("peekCloseVisible");
  }
  async clickListRow(_name: string): Promise<void> {
    return todo("clickListRow");
  }
  async setPeekMode(_mode: string): Promise<void> {
    return todo("setPeekMode");
  }
  async peekPanelBox(): Promise<{ x: number; y: number; width: number; height: number } | null> {
    return todo("peekPanelBox");
  }
  async copyPeekLink(): Promise<void> {
    return todo("copyPeekLink");
  }
  async peekFullScreenHref(): Promise<string | null> {
    return todo("peekFullScreenHref");
  }
  async peekQuickActionNames(): Promise<string[]> {
    return todo("peekQuickActionNames");
  }
  async peekErrorTitle(): Promise<string | null> {
    return todo("peekErrorTitle");
  }
  async widgetTitles(): Promise<string[]> {
    return todo("widgetTitles");
  }
  async widgetRowNames(_widget: string): Promise<string[]> {
    return todo("widgetRowNames");
  }
  async widgetProgress(_widget: string): Promise<string | null> {
    return todo("widgetProgress");
  }
  async widgetGroupNames(_widget: string): Promise<string[]> {
    return todo("widgetGroupNames");
  }
  async openWidgetSection(_widget: string): Promise<void> {
    return todo("openWidgetSection");
  }
  async widgetExpanded(_widget: string): Promise<boolean> {
    return todo("widgetExpanded");
  }
  async widgetHeaderControlCount(_widget: string): Promise<number> {
    return todo("widgetHeaderControlCount");
  }
  async toggleWidgetSection(_widget: string): Promise<void> {
    return todo("toggleWidgetSection");
  }
  async widgetAddMenuNames(_widget: string): Promise<string[]> {
    return todo("widgetAddMenuNames");
  }
  async openSubIssueCreateModal(): Promise<void> {
    return todo("openSubIssueCreateModal");
  }
  async createModalParentName(): Promise<string | null> {
    return todo("createModalParentName");
  }
  async createModalProjectLocked(): Promise<boolean> {
    return todo("createModalProjectLocked");
  }
  async createModalSubmit(_name: string): Promise<void> {
    return todo("createModalSubmit");
  }
  async addExistingSubIssue(_search: string, _name: string): Promise<void> {
    return todo("addExistingSubIssue");
  }
  async clickWidgetRow(_widget: string, _rowName: string): Promise<void> {
    return todo("clickWidgetRow");
  }
  async widgetRowActionNames(_widget: string, _rowName: string): Promise<string[]> {
    return todo("widgetRowActionNames");
  }
  async clickWidgetRowAction(_widget: string, _rowName: string, _action: string): Promise<void> {
    return todo("clickWidgetRowAction");
  }
  async downloadWidgetRowFile(_widget: string, _rowName: string): Promise<{ fileName: string; bytes: Buffer }> {
    return todo("downloadWidgetRowFile");
  }
  async confirmModalTitle(): Promise<string | null> {
    return todo("confirmModalTitle");
  }
  async confirmModalText(): Promise<string | null> {
    return todo("confirmModalText");
  }
  async confirmModal(_label: string): Promise<void> {
    return todo("confirmModal");
  }
  async addRelationViaModal(_type: string, _search: string, _name: string): Promise<void> {
    return todo("addRelationViaModal");
  }
  async addLinkModal(_url: string, _title?: string): Promise<void> {
    return todo("addLinkModal");
  }
  async clickLinkCopy(_rowName: string): Promise<void> {
    return todo("clickLinkCopy");
  }
  async editLinkTitle(_rowName: string, _title: string): Promise<void> {
    return todo("editLinkTitle");
  }
  async linkRowTarget(_rowName: string): Promise<{ href: string; target: string | null } | null> {
    return todo("linkRowTarget");
  }
  async uploadAttachment(_file: { name: string; mime: string; bytes: Buffer }): Promise<void> {
    return todo("uploadAttachment");
  }
  async clickWidgetAction(_name: string): Promise<void> {
    return todo("clickWidgetAction");
  }
  async postComment(_text: string): Promise<void> {
    return todo("postComment");
  }
  async typeComment(_text: string): Promise<void> {
    return todo("typeComment");
  }
  async clickCommentAndRun(): Promise<void> {
    return todo("clickCommentAndRun");
  }
  async commentAndRunDisabled(): Promise<boolean> {
    return todo("commentAndRunDisabled");
  }

  async sidebarBrandVisible(): Promise<boolean> {
    // Sidebar shell (NEWFRONT-290): the header wordmark.
    const brand = this.page.locator('#main-sidebar [data-testid="sidebar-brand"]').first();
    return (await brand.count()) > 0 && (await brand.isVisible());
  }

  async sidebarQuickActionNames(): Promise<string[]> {
    const texts = await this.page.locator("#main-sidebar").getByRole("button").allTextContents();
    return texts.map((text) => text.trim().replace(/\s+/g, " ")).filter((text) => text.length > 0);
  }

  async sidebarAccountButtonCount(): Promise<number> {
    // Sidebar chrome menus (NEWFRONT-287): the account row is the user
    // menu card now, so the trigger carries the user-menu test id.
    return await this.page.locator('#main-sidebar [data-testid="sidebar-user-menu"]').count();
  }

  /** The resize grip on the sidebar's right edge. */
  private sidebarGrip(): Locator {
    return this.page.locator("#main-sidebar").getByRole("separator", { name: "Resize sidebar" }).first();
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
    return (await this.page.locator("#main-sidebar").innerText()).includes(name);
  }

  /** The listed-projects count input inside the open dialog. */
  private projectCapField(): Locator {
    return this.personalizeDialog().locator('[data-testid="project-cap-input"]').first();
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
    const error = this.personalizeDialog().getByText("Use at least 1.");
    return (await error.count()) > 0 && (await error.first().isVisible());
  }

  async railSettingsEntryPresent(): Promise<boolean> {
    // Pinned rail settings entry (SHELL-063): lives inside the rail
    // strip, so suppressed builds never show it.
    return (await this.page.locator('[data-testid="workspace-rail"] a[aria-label="Settings"]').count()) > 0;
  }

  async railContextMenuText(): Promise<string> {
    // The rail owns the far-left edge context menu (density/dock
    // controls); suppressed builds offer nothing there.
    await this.page.mouse.click(8, 400, { button: "right" });
    await this.page.waitForTimeout(1000);
    return (await this.page.locator("#context-menu-portal").textContent()) ?? "";
  }

  async inboxDotPresent(): Promise<boolean> {
    // The unread dot mounts inside the inbox link's icon wrapper only
    // with unread notifications (SHELL-065); an empty inbox carries no
    // marker element.
    return (await this.page.locator('a[aria-label="Inbox"] div span').count()) > 0;
  }

  async hoverProjectHeader(): Promise<void> {
    await this.page.locator('[data-testid="project-switcher-trigger"]').first().hover();
  }

  async projectNameVisibleCount(name: string): Promise<number> {
    return await this.page.getByText(name, { exact: false }).count();
  }

  async projectActionDialogHeading(): Promise<string | null> {
    // Kit dialog titles render h2; toasts share the dialog role but
    // never carry a heading, so the h2 scopes to real dialogs.
    const heading = this.page.locator('[role="dialog"] h2').first();
    if ((await heading.count()) === 0) return null;
    const text = ((await heading.textContent()) ?? "").trim().replace(/\s+/g, " ");
    return text === "" ? null : text;
  }

  async activeCyclesHeaderVisible(): Promise<boolean> {
    // Paywalled Cycles page header (SHELL-101): the header the upgrade
    // pill mounts beside. Lives in the Cycles area; shell builds
    // without it read absent.
    const header = this.page.getByText("Active cycles", { exact: false }).first();
    return (await header.count()) > 0 && (await header.isVisible());
  }

  async errorNoticeVisible(): Promise<boolean> {
    const notice = this.page.getByText("Something went wrong");
    return (await notice.count()) > 0 && (await notice.first().isVisible());
  }

  // Activity feed stubs (NEWFRONT-114). Mirror of the interface additions;
  // each throws until the activity area lands in apps/web_new.

  async activitySignIn(_email: string, _password: string, _workspaceSlug: string): Promise<void> {
    return todo("activitySignIn");
  }

  async activityOpenIssueDetail(_workspaceSlug: string, _projectId: string, _issueId: string): Promise<void> {
    return todo("activityOpenIssueDetail");
  }

  async activityEntryTexts(): Promise<string[]> {
    return todo("activityEntryTexts");
  }

  async activityToggleSort(): Promise<void> {
    return todo("activityToggleSort");
  }

  async activityOpenFilterMenu(): Promise<void> {
    return todo("activityOpenFilterMenu");
  }

  async activityFilterOptionLabels(): Promise<string[]> {
    return todo("activityFilterOptionLabels");
  }

  async activityToggleFilterOption(_label: string): Promise<void> {
    return todo("activityToggleFilterOption");
  }

  async activityFilterNarrowed(): Promise<boolean> {
    return todo("activityFilterNarrowed");
  }

  async activityComposerPosition(): Promise<"above" | "below" | "hidden"> {
    return todo("activityComposerPosition");
  }

  async activityComposerType(_text: string): Promise<void> {
    return todo("activityComposerType");
  }

  async activityComposerSubmit(): Promise<void> {
    return todo("activityComposerSubmit");
  }

  async activityRenameTitle(_title: string): Promise<void> {
    return todo("activityRenameTitle");
  }

  async activityLoadingVisible(): Promise<boolean> {
    return todo("activityLoadingVisible");
  }

  async activityStoredSort(): Promise<string | null> {
    return todo("activityStoredSort");
  }

  async activityStoredFilters(): Promise<string | null> {
    return todo("activityStoredFilters");
  }

  async activityOpenFirstEntryLink(): Promise<string> {
    return todo("activityOpenFirstEntryLink");
  }

  // --- Issue activity & comments (NEWFRONT-122, ISS-194–206). Stubs until
  // --- the matching area lands in apps/web_new; the oracle driver above
  // --- stays untouched.

  async activityCommentTexts(): Promise<string[]> {
    return todo("activityCommentTexts");
  }

  async activityHasCreationEntry(): Promise<boolean> {
    return todo("activityHasCreationEntry");
  }

  async activityFilterOptions(): Promise<{ label: string; selected: boolean }[]> {
    return todo("activityFilterOptions");
  }

  async activityToggleFilter(_label: string): Promise<void> {
    return todo("activityToggleFilter");
  }

  async activityFilterDotVisible(): Promise<boolean> {
    return todo("activityFilterDotVisible");
  }

  async activityComposerIsAboveFeed(): Promise<boolean> {
    return todo("activityComposerIsAboveFeed");
  }

  async activityComposerText(): Promise<string> {
    return todo("activityComposerText");
  }

  async activityPostComment(_bodyText: string): Promise<void> {
    return todo("activityPostComment");
  }

  async activityOpenCommentMenu(_cardText: string): Promise<void> {
    return todo("activityOpenCommentMenu");
  }

  async activityMenuItems(): Promise<string[]> {
    return todo("activityMenuItems");
  }

  async activityClickMenuItem(_name: string): Promise<void> {
    return todo("activityClickMenuItem");
  }

  async activityEditComment(_oldText: string, _newText: string): Promise<void> {
    return todo("activityEditComment");
  }

  async sawToast(_text: string): Promise<boolean> {
    return todo("sawToast");
  }

  async activityCancelEdit(_cardText: string): Promise<void> {
    return todo("activityCancelEdit");
  }

  async activityCommentHighlighted(_cardText: string): Promise<boolean> {
    return todo("activityCommentHighlighted");
  }

  async activityChipTooltipText(_cardText: string, _emoji: string, _expectedName: string): Promise<string> {
    return todo("activityChipTooltipText");
  }

  async activityCommentBodyVisible(_cardText: string): Promise<boolean> {
    return todo("activityCommentBodyVisible");
  }

  async activityExpandFoldedComment(_cardText: string): Promise<void> {
    return todo("activityExpandFoldedComment");
  }

  async activityCopyCommentLink(_cardText: string): Promise<string> {
    return todo("activityCopyCommentLink");
  }

  async activityAddCommentReaction(_cardText: string): Promise<{ emoji: string; code: string }> {
    return todo("activityAddCommentReaction");
  }

  async activityCommentReactionChips(_cardText: string): Promise<{ emoji: string; count: number; reacted: boolean }[]> {
    return todo("activityCommentReactionChips");
  }

  async activityClickCommentReactionChip(_cardText: string, _emoji: string): Promise<void> {
    return todo("activityClickCommentReactionChip");
  }

  async issueAddReaction(): Promise<{ emoji: string; code: string }> {
    return todo("issueAddReaction");
  }

  async issueReactionChips(): Promise<{ emoji: string; count: number; reacted: boolean }[]> {
    return todo("issueReactionChips");
  }

  async issueClickReactionChip(_emoji: string): Promise<void> {
    return todo("issueClickReactionChip");
  }

  async codeReviewsVisible(): Promise<boolean> {
    return todo("codeReviewsVisible");
  }

  async codeReviewLinks(): Promise<{ badge: string; title: string; href: string | null; target: string | null }[]> {
    return todo("codeReviewLinks");
  }

  async codeReviewAttach(_url: string): Promise<void> {
    return todo("codeReviewAttach");
  }

  async codeReviewAttemptAttach(_url: string): Promise<void> {
    return todo("codeReviewAttemptAttach");
  }

  async codeReviewInputValue(): Promise<string> {
    return todo("codeReviewInputValue");
  }

  async codeReviewDetach(_title: string): Promise<void> {
    return todo("codeReviewDetach");
  }

  async worklogCreateVisible(): Promise<boolean> {
    return todo("worklogCreateVisible");
  }

  // --- Shared property dropdowns (NEWFRONT-122, ISS-207–220). Stubs until
  // --- the matching area lands in apps/web_new; the oracle driver above
  // --- stays untouched.

  async propertyValueText(_label: string): Promise<string> {
    return todo("propertyValueText");
  }

  async propertyOpenPicker(_label: string): Promise<void> {
    return todo("propertyOpenPicker");
  }

  async propertyOpenPickerByKeyboard(_label: string): Promise<void> {
    return todo("propertyOpenPickerByKeyboard");
  }

  async propertyPickerDisabled(_label: string): Promise<boolean> {
    return todo("propertyPickerDisabled");
  }

  async propertyTriggerPresent(_label: string): Promise<boolean> {
    return todo("propertyTriggerPresent");
  }

  async pickerOpen(): Promise<boolean> {
    return todo("pickerOpen");
  }

  async pickerOptionTexts(): Promise<string[]> {
    return todo("pickerOptionTexts");
  }

  async pickerHasSearch(): Promise<boolean> {
    return todo("pickerHasSearch");
  }

  async pickerSearch(_query: string): Promise<void> {
    return todo("pickerSearch");
  }

  async pickerSearchValue(): Promise<string> {
    return todo("pickerSearchValue");
  }

  async pickerSearchFocused(): Promise<boolean> {
    return todo("pickerSearchFocused");
  }

  async pickerPick(_text: string): Promise<void> {
    return todo("pickerPick");
  }

  async pickerOptionDisabled(_text: string): Promise<boolean> {
    return todo("pickerOptionDisabled");
  }

  async pickerEmptyText(): Promise<string> {
    return todo("pickerEmptyText");
  }

  async pickerPressEscape(): Promise<void> {
    return todo("pickerPressEscape");
  }

  async pickerClickOutside(): Promise<void> {
    return todo("pickerClickOutside");
  }

  async openArchivedIssueDetail(_workspaceSlug: string, _projectId: string, _issueId: string): Promise<void> {
    return todo("openArchivedIssueDetail");
  }

  async datePickerOpen(_label: string): Promise<void> {
    return todo("datePickerOpen");
  }

  async datePickerVisible(): Promise<boolean> {
    return todo("datePickerVisible");
  }

  async datePickerVisibleMonth(): Promise<{ month: string; year: string }> {
    return todo("datePickerVisibleMonth");
  }

  async datePickerPickDay(_day: number): Promise<void> {
    return todo("datePickerPickDay");
  }

  async datePickerDayDisabled(_day: number): Promise<boolean> {
    return todo("datePickerDayDisabled");
  }

  async datePickerPortalAttached(_label: string): Promise<boolean> {
    return todo("datePickerPortalAttached");
  }

  async datePickerClear(_label: string): Promise<void> {
    return todo("datePickerClear");
  }

  async propertyRowPresent(_label: string): Promise<boolean> {
    return todo("propertyRowPresent");
  }

  // --- Create-issue modal project picker (NEWFRONT-122, ISS-211). Stubs
  // --- until the matching area lands in apps/web_new.

  async issueModalOpenCreate(_workspaceSlug: string, _projectId: string): Promise<void> {
    return todo("issueModalOpenCreate");
  }

  async issueModalProjectValue(): Promise<string> {
    return todo("issueModalProjectValue");
  }

  async issueModalProjectOpenPicker(): Promise<void> {
    return todo("issueModalProjectOpenPicker");
  }

  async issueModalProjectOptionTexts(): Promise<string[]> {
    return todo("issueModalProjectOptionTexts");
  }

  async issueModalProjectSearch(_query: string): Promise<void> {
    return todo("issueModalProjectSearch");
  }

  async issueModalProjectEmptyText(): Promise<string> {
    return todo("issueModalProjectEmptyText");
  }

  async issueModalProjectPick(_text: string): Promise<void> {
    return todo("issueModalProjectPick");
  }

  async issueModalProjectPressEscape(): Promise<void> {
    return todo("issueModalProjectPressEscape");
  }

  async issueModalFillTitle(_title: string): Promise<void> {
    return todo("issueModalFillTitle");
  }

  async issueModalSubmit(): Promise<void> {
    return todo("issueModalSubmit");
  }

  async rangeMergedCellText(_issueName: string): Promise<string> {
    return todo("rangeMergedCellText");
  }

  async rangeMergedCellOpen(_issueName: string): Promise<void> {
    return todo("rangeMergedCellOpen");
  }

  async rangeMergedCellClear(_issueName: string): Promise<void> {
    return todo("rangeMergedCellClear");
  }

  async rangeCalendarVisible(): Promise<boolean> {
    return todo("rangeCalendarVisible");
  }

  async rangeCalendarPickDay(_day: number): Promise<void> {
    return todo("rangeCalendarPickDay");
  }

  async rangeCalendarDayDisabled(_day: number): Promise<boolean> {
    return todo("rangeCalendarDayDisabled");
  }

  async rangeCalendarSelectMonth(_monthLabel: string): Promise<void> {
    return todo("rangeCalendarSelectMonth");
  }

  async rangeCalendarSelectYear(_yearLabel: string): Promise<void> {
    return todo("rangeCalendarSelectYear");
  }

  async cycleCreateOpen(_workspaceSlug: string, _projectId: string): Promise<void> {
    return todo("cycleCreateOpen");
  }

  async cycleFormRangePlaceholders(): Promise<{ from: string; to: string }> {
    return todo("cycleFormRangePlaceholders");
  }

  async cycleFormRangeOpen(): Promise<void> {
    return todo("cycleFormRangeOpen");
  }

  async cycleFormFillName(_name: string): Promise<void> {
    return todo("cycleFormFillName");
  }

  async cycleFormSubmit(): Promise<void> {
    return todo("cycleFormSubmit");
  }

  async intakeCreateOpen(_workspaceSlug: string, _projectId: string): Promise<void> {
    return todo("intakeCreateOpen");
  }

  async intakeStateValue(): Promise<string> {
    return todo("intakeStateValue");
  }

  async intakeStateOpenPicker(): Promise<void> {
    return todo("intakeStateOpenPicker");
  }

  async intakeStateOptionTexts(): Promise<string[]> {
    return todo("intakeStateOptionTexts");
  }

  async intakeStateSearch(_query: string): Promise<void> {
    return todo("intakeStateSearch");
  }

  async intakeStateEmptyText(): Promise<string> {
    return todo("intakeStateEmptyText");
  }

  async intakeStatePick(_text: string): Promise<void> {
    return todo("intakeStatePick");
  }

  async intakeCreateFillTitle(_title: string): Promise<void> {
    return todo("intakeCreateFillTitle");
  }

  async intakeCreateSubmit(): Promise<void> {
    return todo("intakeCreateSubmit");
  }

  async intakeTriageStateDisabled(): Promise<boolean> {
    return todo("intakeTriageStateDisabled");
  }

  async viewsOpenList(_workspaceSlug: string, _projectId: string): Promise<void> {
    return todo("viewsOpenList");
  }

  async viewsOpenCreate(): Promise<void> {
    return todo("viewsOpenCreate");
  }

  async viewsLayoutValue(): Promise<string> {
    return todo("viewsLayoutValue");
  }

  async viewsLayoutOpenPicker(): Promise<void> {
    return todo("viewsLayoutOpenPicker");
  }

  async viewsLayoutOptionTexts(): Promise<string[]> {
    return todo("viewsLayoutOptionTexts");
  }

  async viewsLayoutHasSearch(): Promise<boolean> {
    return todo("viewsLayoutHasSearch");
  }

  async viewsLayoutSelectedMarked(_text: string): Promise<boolean> {
    return todo("viewsLayoutSelectedMarked");
  }

  async viewsLayoutPick(_text: string): Promise<void> {
    return todo("viewsLayoutPick");
  }

  async viewsFillName(_name: string): Promise<void> {
    return todo("viewsFillName");
  }

  async viewsSubmit(): Promise<void> {
    return todo("viewsSubmit");
  }

  async subIssueFiltersOpen(_workspaceSlug: string, _projectId: string, _parentIssueId: string): Promise<void> {
    return todo("subIssueFiltersOpen");
  }

  async subIssueFiltersPanelText(): Promise<string> {
    return todo("subIssueFiltersPanelText");
  }

  async detailIdentifierText(): Promise<string> {
    return todo("detailIdentifierText");
  }

  async detailIdentifierCopy(): Promise<void> {
    return todo("detailIdentifierCopy");
  }

  async viewsOpenDetail(_workspaceSlug: string, _projectId: string, _viewId: string): Promise<void> {
    return todo("viewsOpenDetail");
  }

  async ganttShowsIssue(_issueName: string): Promise<boolean> {
    return todo("ganttShowsIssue");
  }

  async settingsLabelsOpen(_workspaceSlug: string, _projectId: string): Promise<void> {
    return todo("settingsLabelsOpen");
  }

  async settingsLabelsNames(): Promise<string[]> {
    return todo("settingsLabelsNames");
  }

  async settingsLabelsAddVisible(): Promise<boolean> {
    return todo("settingsLabelsAddVisible");
  }

  async settingsLabelsOpenCreate(): Promise<void> {
    return todo("settingsLabelsOpenCreate");
  }

  async settingsLabelsFormVisible(): Promise<boolean> {
    return todo("settingsLabelsFormVisible");
  }

  async settingsLabelsFillName(_name: string): Promise<void> {
    return todo("settingsLabelsFillName");
  }

  async settingsLabelsFormError(): Promise<string> {
    return todo("settingsLabelsFormError");
  }

  async settingsLabelsSubmitCreate(): Promise<void> {
    return todo("settingsLabelsSubmitCreate");
  }

  async settingsLabelsSubmitUpdate(): Promise<void> {
    return todo("settingsLabelsSubmitUpdate");
  }

  async settingsLabelsCancelForm(): Promise<void> {
    return todo("settingsLabelsCancelForm");
  }

  async settingsLabelsDotColor(): Promise<string> {
    return todo("settingsLabelsDotColor");
  }

  async settingsLabelsOpenColorPicker(): Promise<void> {
    return todo("settingsLabelsOpenColorPicker");
  }

  async settingsLabelsPickColor(_hex: string): Promise<void> {
    return todo("settingsLabelsPickColor");
  }

  async settingsLabelsOpenRowMenu(_name: string): Promise<void> {
    return todo("settingsLabelsOpenRowMenu");
  }

  async settingsLabelsMenuItems(): Promise<string[]> {
    return todo("settingsLabelsMenuItems");
  }

  async settingsLabelsMenuPick(_text: string): Promise<void> {
    return todo("settingsLabelsMenuPick");
  }

  async settingsLabelsIsGroup(_name: string): Promise<boolean> {
    return todo("settingsLabelsIsGroup");
  }

  async settingsLabelsDeleteViaTrash(_name: string): Promise<void> {
    return todo("settingsLabelsDeleteViaTrash");
  }

  async settingsLabelsDeleteModalText(): Promise<string> {
    return todo("settingsLabelsDeleteModalText");
  }

  async settingsLabelsDeleteConfirm(): Promise<void> {
    return todo("settingsLabelsDeleteConfirm");
  }

  async settingsLabelsDeleteCancel(): Promise<void> {
    return todo("settingsLabelsDeleteCancel");
  }

  async settingsLabelsDragOnto(_source: string, _target: string): Promise<void> {
    return todo("settingsLabelsDragOnto");
  }

  async settingsLabelsDragAbove(_source: string, _target: string): Promise<void> {
    return todo("settingsLabelsDragAbove");
  }

  async settingsLabelsEmptyTitle(): Promise<string> {
    return todo("settingsLabelsEmptyTitle");
  }

  async settingsLabelsEmptyAction(): Promise<void> {
    return todo("settingsLabelsEmptyAction");
  }

  async settingsLabelsSkeletonVisible(): Promise<boolean> {
    return todo("settingsLabelsSkeletonVisible");
  }

  async settingsLabelsDelayLoad(_ms: number): Promise<void> {
    return todo("settingsLabelsDelayLoad");
  }

  async issueLabelsOpenPicker(): Promise<void> {
    return todo("issueLabelsOpenPicker");
  }

  async issueLabelsOptionTexts(): Promise<string[]> {
    return todo("issueLabelsOptionTexts");
  }

  async issueLabelsRowText(): Promise<string> {
    return todo("issueLabelsRowText");
  }

  async settingsLabelsNameValue(): Promise<string> {
    return todo("settingsLabelsNameValue");
  }

  async settingsLabelsAttemptSubmit(): Promise<void> {
    return todo("settingsLabelsAttemptSubmit");
  }

  async settingsLabelsAttemptDeleteConfirm(): Promise<void> {
    return todo("settingsLabelsAttemptDeleteConfirm");
  }

  async reloadPage(): Promise<void> {
    return todo("reloadPage");
  }

  async projectQuickAddVisible(): Promise<boolean> {
    return todo("projectQuickAddVisible");
  }

  async issueTitleInputEnabled(): Promise<boolean> {
    return todo("issueTitleInputEnabled");
  }

  async settingsLabelsAccessDenied(): Promise<boolean> {
    return todo("settingsLabelsAccessDenied");
  }

  async globalViewIssueVisible(_name: string): Promise<boolean> {
    return todo("globalViewIssueVisible");
  }

  async openCycleIssues(_workspaceSlug: string, _projectId: string, _cycleId: string): Promise<void> {
    return todo("openCycleIssues");
  }

  async cycleTransferButtonVisible(): Promise<boolean> {
    return todo("cycleTransferButtonVisible");
  }

  async cycleTransferOpen(): Promise<void> {
    return todo("cycleTransferOpen");
  }

  async cycleTransferOptionNames(): Promise<string[]> {
    return todo("cycleTransferOptionNames");
  }

  async cycleTransferPick(_name: string): Promise<void> {
    return todo("cycleTransferPick");
  }

  async failNextIssuePatch(_status: number, _delayMs: number): Promise<void> {
    return todo("failNextIssuePatch");
  }

  async clearIssuePatchFailure(): Promise<void> {
    return todo("clearIssuePatchFailure");
  }

  /** Wait for the signed-out entry card after a sign-out round-trip. */
  private async waitForSignedOut(): Promise<void> {
    await this.authEmailField().waitFor({ timeout: 60_000 });
  }

  /** Dismiss the first-run tour overlay when it covers the shell chrome. */
  private async dismissTourIfShown(): Promise<void> {
    if (!(await this.isTourWelcomeVisible())) return;
    await this.declineTour();
    // The seam flips the profile flag over the API; wait for the overlay
    // to actually leave before touching shell chrome.
    await this.page
      .getByRole("dialog")
      .waitFor({ state: "detached", timeout: 15_000 })
      .catch(() => {});
  }

  /**
   * Click through the account menu, surviving the tour overlay popping in
   * mid-sequence: bounded attempts, Escape resets half-open menus, and the
   * tour is dismissed ahead of every attempt.
   */
  private async clickAccountMenuItem(name: string): Promise<void> {
    for (let attempt = 0; attempt < 3; attempt++) {
      await this.dismissTourIfShown();
      try {
        await this.page.getByRole("button", { name: "Account actions" }).click({ timeout: 15_000 });
        await this.page.getByRole("menuitem", { name }).click({ timeout: 15_000 });
        return;
      } catch {
        await this.page.keyboard.press("Escape").catch(() => {});
      }
    }
    await this.dismissTourIfShown();
    await this.page.getByRole("button", { name: "Account actions" }).click();
    await this.page.getByRole("menuitem", { name }).click();
  }

  async signOutViaAccountMenu(): Promise<void> {
    await this.clickAccountMenuItem("Sign out");
    await this.waitForSignedOut();
  }

  async signOutViaCommandPalette(): Promise<void> {
    for (let attempt = 0; attempt < 3; attempt++) {
      await this.dismissTourIfShown();
      try {
        await this.page.getByRole("button", { name: "Open command palette" }).click({ timeout: 15_000 });
        const filter = this.page.getByPlaceholder("Type a command…");
        await filter.fill("Sign out");
        await this.page.getByRole("option", { name: "Sign out" }).first().click({ timeout: 15_000 });
        await this.waitForSignedOut();
        return;
      } catch {
        await this.page.keyboard.press("Escape").catch(() => {});
      }
    }
    await this.dismissTourIfShown();
    await this.page.getByRole("button", { name: "Open command palette" }).click();
    const filter = this.page.getByPlaceholder("Type a command…");
    await filter.fill("Sign out");
    await this.page.getByRole("option", { name: "Sign out" }).first().click();
    await this.waitForSignedOut();
  }

  async isSignedOut(): Promise<boolean> {
    return this.authEmailField().isVisible();
  }

  async openSwitchAccount(): Promise<void> {
    // The onboarding header names the signed-in account in its menu
    // trigger; the menu carries the switch entry.
    await this.page.getByRole("button", { name: /^Account: / }).click();
    await this.page.getByRole("menuitem", { name: "Use a different email address" }).click();
    await this.page.getByRole("dialog").waitFor({ timeout: WebNewDriver.WAIT_MS });
  }

  async switchAccountEmail(): Promise<string> {
    // The dialog names the active account in its explanatory copy; the
    // sentence period after the address is not part of it.
    const dialog = this.page.getByRole("dialog");
    const body = (await dialog.innerText()).trim();
    const match = /[\w.+-]+@[\w-]+(?:\.[\w-]+)+/.exec(body);
    if (!match) throw new Error("[parity] switch-account dialog names no email.");
    return match[0];
  }

  async confirmSwitchAccount(): Promise<void> {
    await this.page.getByRole("dialog").getByRole("button", { name: "Switch account" }).click();
    await this.waitForSignedOut();
  }

  async openDeactivateAccount(): Promise<void> {
    await this.clickAccountMenuItem("Deactivate account");
    await this.page.getByRole("dialog").waitFor({ timeout: WebNewDriver.WAIT_MS });
  }

  async confirmDeactivation(): Promise<void> {
    await this.page.getByRole("dialog").getByRole("button", { name: "Deactivate account" }).click();
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
    const match = this.page.getByText(text, { exact: false }).first();
    if ((await match.count()) === 0) return false;
    return match.isVisible();
  }

  private deviceCodeField() {
    // The approval form carries a single code input; it auto-formats XXXX-YYYY.
    return this.page.getByLabel("Device code");
  }

  async typeDeviceCode(code: string): Promise<void> {
    const field = this.deviceCodeField();
    await field.waitFor({ timeout: WebNewDriver.WAIT_MS });
    await field.fill("");
    await field.pressSequentially(code, { delay: 20 });
  }

  async deviceCodeFieldValue(): Promise<string> {
    const field = this.deviceCodeField();
    await field.waitFor({ timeout: WebNewDriver.WAIT_MS });
    return field.inputValue();
  }

  async submitDeviceApproval(): Promise<void> {
    // The approval page carries a single submit button ("Approve",
    // "Approving…" while the request is in flight). The exact-name match
    // also waits out the in-flight state.
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

  // Sidebar + workspace navigation (NEWFRONT-125, SHELL-046..062).
  // NEWFRONT-285 implements the rows half (SHELL-046..051) plus the
  // shared navigation reads its scenarios use; NEWFRONT-286 the
  // overflow, More and favorites probes (SHELL-052..057) plus the
  // shared outside-click and section reads; the remaining stubs
  // (switcher, menus, logo) belong to NEWFRONT-287.
  private async screenAside(): Promise<Locator> {
    // Same non-waiting read as the oracle driver: measure with
    // getBoundingClientRect, which never waits, instead of boundingBox.
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
      const [x, width, height] = boxes[index] ?? [];
      if ((x ?? -1) >= 0 && (width ?? 0) > 50 && (height ?? 0) > 50) return asides.nth(index);
    }
    return asides.first();
  }

  /** The sidebar row carrying a project name. */
  private async projectRow(projectName: string): Promise<Locator> {
    const aside = await this.screenAside();
    return aside.locator("li[data-project-row]").filter({ hasText: projectName }).first();
  }

  async resetSession(): Promise<void> {
    await this.page.context().clearCookies();
    await this.page.goto("/", { waitUntil: "domcontentloaded", timeout: 60_000 });
  }

  async openWorkspacePath(path: string): Promise<void> {
    // Absolute (a copied deep link) or base-relative; the shell must
    // mount there either way — except the notifications address, which
    // renders shell-less by design (SHELL-062), where the sidebar wait
    // is skipped.
    if (path.includes("/notifications/")) {
      await this.page.goto(path, { timeout: 60_000 });
      await this.page.waitForLoadState("domcontentloaded", { timeout: 60_000 });
      return;
    }
    await this.gotoShell(path);
  }

  async isCreateProjectVisible(): Promise<boolean> {
    // The header affordance reveals on group hover; the empty-state one
    // renders in the open.
    try {
      await this.page.getByTestId("projects-group-toggle").hover({ timeout: 5_000 });
    } catch {
      return false;
    }
    const header = this.page.getByTestId("sidebar-create-project");
    if ((await header.count()) > 0 && (await header.first().isVisible())) return true;
    const empty = this.page.getByTestId("sidebar-empty-create-project");
    return (await empty.count()) > 0 && (await empty.first().isVisible());
  }

  async isSidebarOnScreen(): Promise<boolean> {
    // Any substantially-sized aside on screen counts, mirroring the
    // oracle read; off-screen asides never do.
    const boxes = await this.page
      .locator("aside")
      .evaluateAll((elements) =>
        elements.map((element) => {
          const box = element.getBoundingClientRect();
          return [box.x, box.width, box.height];
        })
      )
      .catch((): number[][] => []);
    return boxes.some(([x, width, height]) => (x ?? -1) >= 0 && (width ?? 0) > 50 && (height ?? 0) > 50);
  }

  async sidebarLinkTexts(): Promise<string[]> {
    // The main sidebar's anchor texts, rendered and whitespace-folded.
    const texts = await this.page.locator("#main-sidebar").locator("a").allInnerTexts();
    return texts.map((text) => text.trim().replace(/\s+/g, " ")).filter((text) => text.length > 0);
  }

  async sidebarRowTone(linkText: string): Promise<{ background: string; color: string }> {
    const aside = await this.screenAside();
    // Innermost non-transparent background from the link outward, plus
    // the text color: the same read the oracle makes on the old app.
    return aside
      .locator("a", { hasText: linkText })
      .first()
      .evaluate((link: HTMLAnchorElement) => {
        let background = "rgba(0, 0, 0, 0)";
        let node: HTMLElement | null = link;
        for (let depth = 0; depth < 3 && node !== null; depth += 1) {
          const fill = getComputedStyle(node).backgroundColor;
          if (fill !== "rgba(0, 0, 0, 0)" && fill !== "transparent") {
            background = fill;
            break;
          }
          node = node.parentElement;
        }
        const inner = link.querySelector("div");
        const color = getComputedStyle(inner ?? link).color;
        return { background, color };
      });
  }

  async isProjectsGroupOpen(): Promise<boolean> {
    const toggle = this.page.getByTestId("projects-group-toggle");
    try {
      return (await toggle.getAttribute("aria-expanded", { timeout: 5_000 })) === "true";
    } catch {
      return false;
    }
  }

  async setProjectsGroupOpen(open: boolean): Promise<void> {
    if ((await this.isProjectsGroupOpen()) === open) return;
    await this.page.getByTestId("projects-group-toggle").click();
  }

  async projectRowHref(projectName: string): Promise<string | null> {
    const aside = await this.screenAside();
    const link = aside.locator("a", { hasText: projectName }).first();
    try {
      return await link.getAttribute("href", { timeout: 5_000 });
    } catch {
      return null;
    }
  }

  async isProjectRowInViewport(projectName: string): Promise<boolean> {
    const row = await this.projectRow(projectName);
    const box = await row.boundingBox().catch(() => null);
    if (!box) return false;
    const viewport = this.page.viewportSize();
    if (!viewport) return false;
    return box.y >= 0 && box.y + box.height <= viewport.height;
  }

  async toggleProjectRow(projectName: string): Promise<void> {
    const row = await this.projectRow(projectName);
    await row.getByTestId("project-row-toggle").click();
  }

  async isProjectRowOpen(projectName: string): Promise<boolean> {
    const row = await this.projectRow(projectName);
    const toggle = row.getByTestId("project-row-toggle");
    if ((await toggle.count()) > 0) {
      return (await toggle.first().getAttribute("aria-expanded")) === "true";
    }
    return (await row.getByTestId("project-subnav-link").count()) > 0;
  }

  async setProjectRowOpen(projectName: string, open: boolean): Promise<void> {
    // Drive the toggle until the row agrees; the row mounts its toggle
    // asynchronously after the projects fetch, so a closed read may mean
    // "not rendered yet" on a freshly landed project route.
    for (let attempt = 0; attempt < 5; attempt += 1) {
      if ((await this.isProjectRowOpen(projectName)) === open) return;
      const row = await this.projectRow(projectName);
      await row.getByTestId("project-row-toggle").click({ timeout: 10_000 });
    }
    await expect.poll(() => this.isProjectRowOpen(projectName), { timeout: 10_000 }).toBe(open);
  }

  async projectSubnavLinks(): Promise<{ text: string; href: string | null }[]> {
    const links = this.page.locator(
      'a[href*="/projects/"][href$="/issues/"], a[href*="/projects/"][href$="/pages/"], a[href*="/projects/"][href$="/intake/"], a[href*="/projects/"][href$="/schedulers/"], a[href*="/projects/"][href$="/runners/"], a[href*="/projects/"][href$="/cycles/"], a[href*="/projects/"][href$="/modules/"], a[href*="/projects/"][href$="/views/"]'
    );
    const count = await links.count();
    const rows: { text: string; href: string | null }[] = [];
    for (let index = 0; index < count; index += 1) {
      const link = links.nth(index);
      rows.push({
        text: ((await link.innerText().catch(() => "")) ?? "").trim(),
        href: await link.getAttribute("href").catch(() => null),
      });
    }
    return rows;
  }

  async isQuickCreateEnabled(): Promise<boolean> {
    return this.page.getByRole("button", { name: "New work item" }).isEnabled();
  }

  async openQuickCreate(): Promise<void> {
    const entry = this.page.getByRole("button", { name: "New work item" });
    // The button enables asynchronously after the projects fetch; a
    // slow first paint may still show it disabled, so retry once.
    try {
      await entry.click({ timeout: 10_000 });
    } catch {
      await entry.click({ timeout: 10_000 });
    }
  }

  async isQuickCreateDialogOpen(): Promise<boolean> {
    return this.page
      .getByRole("dialog")
      .filter({ hasText: "Create new work item" })
      .isVisible()
      .catch(() => false);
  }

  async openProjectQuickMenu(projectName: string): Promise<void> {
    const row = await this.projectRow(projectName);
    await row.hover();
    await row.getByTestId("project-row-menu").click();
    await this.page.getByRole("menuitem").first().waitFor({ state: "visible", timeout: 10_000 });
  }

  async projectQuickMenuTexts(): Promise<string[]> {
    const texts = await this.page.getByRole("menuitem").allInnerTexts();
    return texts.map((text) => text.trim()).filter((text) => text.length > 0);
  }

  async isProjectsOverflowVisible(): Promise<boolean> {
    const toggle = this.page.getByTestId("projects-overflow-toggle");
    return (await toggle.count()) > 0 && (await toggle.first().isVisible());
  }

  async isProjectsOverflowOpen(): Promise<boolean> {
    return (await this.page.getByTestId("projects-overflow-panel").count()) > 0;
  }

  async setProjectsOverflowOpen(open: boolean): Promise<void> {
    if ((await this.isProjectsOverflowOpen()) === open) return;
    await this.page.getByTestId("projects-overflow-toggle").click();
  }

  async searchOverflowProjects(query: string): Promise<void> {
    await this.page.getByTestId("projects-overflow-search").fill(query);
  }

  async overflowProjectNames(): Promise<string[]> {
    const panel = this.page.getByTestId("projects-overflow-panel");
    const texts = await panel.locator("a").allInnerTexts();
    return texts.map((text) => text.trim()).filter((text) => text.length > 0);
  }

  async isOverflowEmptyStateVisible(): Promise<boolean> {
    const empty = this.page.getByTestId("projects-overflow-empty");
    return (await empty.count()) > 0 && (await empty.first().isVisible());
  }

  async isMoreSectionOpen(): Promise<boolean> {
    const toggle = this.page.getByTestId("more-section-toggle");
    try {
      return (await toggle.getAttribute("aria-expanded", { timeout: 5_000 })) === "true";
    } catch {
      return false;
    }
  }

  async setMoreSectionOpen(open: boolean): Promise<void> {
    if ((await this.isMoreSectionOpen()) === open) return;
    await this.page.getByTestId("more-section-toggle").click();
  }

  async moreSectionLinks(): Promise<{ text: string; href: string | null }[]> {
    const links = this.page.getByTestId("more-section-panel").locator("a");
    const count = await links.count();
    const rows: { text: string; href: string | null }[] = [];
    for (let index = 0; index < count; index += 1) {
      const link = links.nth(index);
      rows.push({
        text: ((await link.innerText().catch(() => "")) ?? "").trim(),
        href: await link.getAttribute("href").catch(() => null),
      });
    }
    return rows;
  }

  async sidebarSectionNames(): Promise<string[]> {
    const found: string[] = [];
    if ((await this.page.getByTestId("sidebar-projects-group").count()) > 0) found.push("Projects");
    if ((await this.page.getByTestId("sidebar-more-group").count()) > 0) found.push("More");
    if ((await this.page.getByTestId("sidebar-favorites-group").count()) > 0) found.push("Favorites");
    return found;
  }

  async isFavoritesOpen(): Promise<boolean> {
    // Absent while the section hides (empty favorites): that reads
    // closed, matching the oracle's pre-star assertion.
    const toggle = this.page.getByTestId("favorites-section-toggle");
    if ((await toggle.count()) === 0) return false;
    return (await toggle.first().getAttribute("aria-expanded")) === "true";
  }

  async setFavoritesOpen(open: boolean): Promise<void> {
    if ((await this.isFavoritesOpen()) === open) return;
    await this.page.getByTestId("favorites-section-toggle").click();
  }

  async isOverflowCreateVisible(): Promise<boolean> {
    const create = this.page.getByTestId("projects-overflow-create");
    return (await create.count()) > 0 && (await create.first().isVisible());
  }

  async activateHelpEntry(name: string): Promise<string | null> {
    // Sidebar chrome menus (NEWFRONT-287): the help menu is the one
    // holding Documentation; external entries resolve to their popup.
    const menus = this.page.getByRole("menu");
    for (let index = 0; index < (await menus.count()); index += 1) {
      const content = (
        (await menus
          .nth(index)
          .textContent({ timeout: 15_000 })
          .catch(() => null)) ?? ""
      ).replace(/\s+/g, " ");
      if (!content.includes("Documentation")) continue;
      const item = menus.nth(index).locator("button, a, [role='menuitem']").filter({ hasText: name }).last();
      const [popup] = await Promise.all([
        this.page.waitForEvent("popup", { timeout: 10_000 }).catch(() => null),
        item.click({ timeout: 30_000 }),
      ]);
      await this.page.waitForTimeout(1000);
      if (popup === null) return null;
      await popup.waitForFunction(() => window.location.href !== "about:blank", { timeout: 10_000 }).catch(() => {});
      return popup.url();
    }
    throw new Error(`[parity] help menu holding ${name} is not open.`);
  }

  async isToastVisible(text: string): Promise<boolean> {
    // A visible toast carrying the text.
    return this.page
      .getByText(text, { exact: false })
      .first()
      .isVisible()
      .catch(() => false);
  }

  async isDialogWithTextVisible(text: string): Promise<boolean> {
    // Sidebar chrome menus (NEWFRONT-287): an open dialog holding the text.
    const dialog = this.page.getByRole("dialog").filter({ hasText: text });
    if ((await dialog.count()) === 0) return false;
    return await dialog.first().isVisible();
  }

  async dismissTopmost(): Promise<void> {
    // Sidebar chrome menus (NEWFRONT-287): Escape dismisses the topmost layer.
    await this.page.keyboard.press("Escape");
  }

  async clickMainContent(): Promise<void> {
    // Center of the main region: a fixed slide-over can overlap
    // main's left edge, so a corner click lands on the slide-over.
    const main = this.page.getByRole("main").first();
    const box = await main.boundingBox();
    if (box === null) throw new Error("main region has no bounding box");
    await main.click({ position: { x: Math.round(box.width / 2), y: Math.round(box.height / 2) } });
  }

  async openSidebarLink(text: string): Promise<void> {
    // The sidebar anchor naming the entry, scrolled into view first.
    const aside = await this.screenAside();
    const link = aside.locator("a", { hasText: text }).first();
    await link.scrollIntoViewIfNeeded();
    await link.click({ timeout: 30_000 });
  }

  async switchWorkspace(name: string): Promise<void> {
    // Sidebar chrome menus (NEWFRONT-287): the outermost match in the
    // open switcher menu carries the selection handler.
    const menu = this.page.getByRole("menu").first();
    await menu.locator("button, a, [role='menuitem']").filter({ hasText: name }).first().click({ timeout: 30_000 });
  }

  async activateUserMenuItem(name: string): Promise<void> {
    // Sidebar chrome menus (NEWFRONT-287): the menu holding the entry
    // receives the activation; another open menu never does.
    const menus = this.page.getByRole("menu");
    const count = await menus.count();
    for (let index = 0; index < count; index += 1) {
      const item = menus.nth(index).locator("button, a, [role='menuitem']").filter({ hasText: name }).first();
      if ((await item.count()) === 0) continue;
      await item.click({ timeout: 30_000 });
      return;
    }
    throw new Error(`[parity] user menu entry ${name} is not open.`);
  }

  async dragSidebarProjectBefore(sourceName: string, targetName: string): Promise<void> {
    // Pointer drag through the hover handle: the pointerdown starts the
    // reorder, the release above the target commits it. The handle juts
    // outside its row and hides with row hover, so the grab lands on its
    // inner edge (still inside the row); the center misses the row.
    const source = await this.projectRow(sourceName);
    await source.hover();
    const handle = source.getByTestId("project-drag-handle");
    const from = await handle.boundingBox();
    if (!from) throw new Error(`[parity] no drag handle for ${sourceName}.`);
    const target = await this.projectRow(targetName);
    const to = await target.boundingBox();
    if (!to) throw new Error(`[parity] no sidebar row for ${targetName}.`);
    await this.page.mouse.move(from.x + from.width - 2, from.y + from.height / 2);
    await this.page.mouse.down();
    await this.page.mouse.move(to.x + to.width / 2, to.y + 2, { steps: 12 });
    await this.page.mouse.up();
  }

  /** The favorites row carrying an exact display name. */
  private favoriteRowByName(name: string): Locator {
    const escaped = name.replace(/\\/g, "\\\\").replace(/"/g, '\\"');
    return this.page
      .getByTestId("sidebar-favorites-group")
      .locator(`[data-testid="favorite-entry"][data-entry-name="${escaped}"]`)
      .first();
  }

  async openFavoritesFolderDialog(): Promise<void> {
    await this.page.getByTestId("favorites-folder-create").click();
  }

  async submitFavoritesFolderName(name: string): Promise<void> {
    await this.page.getByTestId("favorites-folder-input").fill(name);
    await this.page.keyboard.press("Enter");
  }

  async isFavoritesFolderDialogOpen(): Promise<boolean> {
    const form = this.page.getByTestId("favorites-folder-form");
    return (await form.count()) > 0 && (await form.first().isVisible());
  }

  async favoriteEntryNames(): Promise<string[]> {
    const rows = this.page.getByTestId("sidebar-favorites-group").locator('[data-testid="favorite-entry"]');
    const count = await rows.count();
    const names: string[] = [];
    for (let index = 0; index < count; index += 1) {
      names.push((await rows.nth(index).getAttribute("data-entry-name")) ?? "");
    }
    return names;
  }

  async openFavoriteEntry(name: string): Promise<void> {
    await this.favoriteRowByName(name).locator("a").first().click();
  }

  async openFavoritesFolder(name: string): Promise<void> {
    await this.favoriteRowByName(name).getByTestId("favorite-folder-toggle").click();
  }

  async openWorkspaceSwitcher(): Promise<void> {
    // Sidebar chrome menus (NEWFRONT-287): the sidebar trigger when the
    // sidebar is open, else the top-bar trigger.
    const sidebar = this.page.locator('[data-testid="sidebar-workspace-switcher"]');
    if ((await sidebar.count()) > 0 && (await sidebar.first().isVisible())) {
      await sidebar.first().click({ timeout: 30_000 });
      return;
    }
    await this.page.locator('[data-testid="topbar-workspace-switcher"]').first().click({ timeout: 30_000 });
  }

  async workspaceSwitcherTexts(): Promise<string[]> {
    // Sidebar chrome menus (NEWFRONT-287): every open menu and dialog
    // text, mirroring the oracle read.
    const texts: string[] = [];
    for (const role of ["menu", "dialog"] as const) {
      const els = this.page.getByRole(role);
      for (let index = 0; index < (await els.count()); index += 1) {
        texts.push(
          (
            (await els
              .nth(index)
              .textContent({ timeout: 15_000 })
              .catch(() => null)) ?? ""
          )
            .trim()
            .replace(/\s+/g, " ")
        );
      }
    }
    return texts.filter((text) => text.length > 0);
  }

  async openUserMenu(): Promise<void> {
    // Sidebar chrome menus (NEWFRONT-287): the sidebar account card.
    // Settles after clicking: the menu mounts a tick after the click,
    // and callers activate entries immediately.
    await this.page.locator('[data-testid="sidebar-user-menu"]').first().click({ timeout: 30_000 });
    const deadline = Date.now() + 15_000;
    for (;;) {
      if ((await this.userMenuTexts()).length > 0) return;
      if (Date.now() > deadline) throw new Error("[parity] user menu never opened.");
      await this.page.waitForTimeout(200);
    }
  }

  async userMenuTexts(): Promise<string[]> {
    // Sidebar chrome menus (NEWFRONT-287): the identity block is not a
    // button, so read the whole menu text plus the actionable items.
    // No menu reads empty at once: waiting out the text timeout here
    // would starve the callers' own polls.
    const menu = this.page.getByRole("menu").first();
    if ((await menu.count()) === 0) return [];
    const full = ((await menu.textContent({ timeout: 30_000 }).catch(() => null)) ?? "").trim().replace(/\s+/g, " ");
    const items = await menu.locator("button, a, [role='menuitem']").allTextContents();
    const cleaned = items.map((text) => text.trim().replace(/\s+/g, " ")).filter((text) => text.length > 0);
    return [...(full.length > 0 ? [full] : []), ...cleaned];
  }

  async openHelpMenu(): Promise<void> {
    // Sidebar chrome menus (NEWFRONT-287): the top-bar help trigger.
    // Idempotent: the trigger toggles, and the menu can survive a
    // dialog opened from it, so a blind click would close it again.
    // Settles after clicking: the menu mounts a tick after the click,
    // and callers activate entries immediately.
    if ((await this.helpMenuTexts()).length > 0) return;
    await this.page.locator('[data-testid="topbar-help-menu"]').first().click({ timeout: 30_000 });
    const deadline = Date.now() + 15_000;
    for (;;) {
      if ((await this.helpMenuTexts()).length > 0) return;
      if (Date.now() > deadline) throw new Error("[parity] help menu never opened.");
      await this.page.waitForTimeout(200);
    }
  }

  async helpMenuTexts(): Promise<string[]> {
    // Sidebar chrome menus (NEWFRONT-287): the menu holding
    // Documentation, with the version footer alongside the items.
    const menus = this.page.getByRole("menu");
    for (let index = 0; index < (await menus.count()); index += 1) {
      const content = (
        (await menus
          .nth(index)
          .textContent({ timeout: 15_000 })
          .catch(() => null)) ?? ""
      ).replace(/\s+/g, " ");
      if (content.includes("Documentation")) {
        const full = content.length > 0 ? [content] : [];
        const items = await menus.nth(index).locator("button, a, [role='menuitem']").allTextContents();
        const cleaned = items.map((text) => text.trim().replace(/\s+/g, " ")).filter((text) => text.length > 0);
        return [...full, ...cleaned];
      }
    }
    return [];
  }

  async workspaceLogoState(): Promise<{ hasImage: boolean; label: string | null; initial: string | null }> {
    // Sidebar chrome menus (NEWFRONT-287): the first visible logo mark.
    const marks = this.page.locator('[data-testid="workspace-logo"]');
    const count = await marks.count();
    for (let index = 0; index < count; index += 1) {
      const mark = marks.nth(index);
      if (!(await mark.isVisible())) continue;
      const hasImage = (await mark.locator("img").count()) > 0;
      const label = await mark.getAttribute("aria-label");
      const initial = hasImage ? null : ((await mark.textContent()) ?? "").trim() || null;
      return { hasImage, label, initial };
    }
    return { hasImage: false, label: null, initial: null };
  }

  // --- NEWFRONT-125 review-fix stubs. Mirror of the interface additions.

  async activateProjectQuickMenuItem(name: string): Promise<void> {
    const entry = this.page.getByRole("menuitem", { name }).first();
    await entry.scrollIntoViewIfNeeded().catch(() => undefined);
    await entry.click();
  }

  async dragFavoriteBefore(sourceName: string, targetName: string): Promise<void> {
    // Pointer drag through the hover handle: the pointerdown starts the
    // reorder, the release above the target commits it. The handle juts
    // outside its row, so the grab lands on its inner edge (still inside
    // the row); the center misses the row.
    await this.dragFavoriteTo(sourceName, targetName, "edge");
  }

  async dragFavoriteIntoFolder(sourceName: string, folderName: string): Promise<void> {
    // Same handle drag, landing on the folder header's center so the
    // drop reads as nesting instead of a sibling reorder. The header —
    // not the row, which spans open children — is the drop zone.
    const source = this.favoriteRowByName(sourceName);
    await source.hover();
    const handle = source.getByTestId("favorite-drag-handle");
    const from = await handle.boundingBox();
    if (!from) throw new Error(`[parity] no drag handle for ${sourceName}.`);
    const header = this.favoriteRowByName(folderName).getByTestId("favorite-folder-toggle");
    const to = await header.boundingBox();
    if (!to) throw new Error(`[parity] no favorite folder for ${folderName}.`);
    // Inner-edge grab like dragFavoriteTo: the handle juts outside its
    // row and its center misses (off-row), so the drag never starts.
    await this.page.mouse.move(from.x + from.width - 2, from.y + from.height / 2);
    await this.page.mouse.down();
    await this.page.mouse.move(to.x + to.width / 2, to.y + to.height / 2, { steps: 12 });
    await this.page.mouse.up();
  }

  async dragFavoriteOutOfFolder(sourceName: string, targetName: string): Promise<void> {
    // Unnesting is the sibling gesture: a nested row dropped above a
    // top-level row leaves its folder while landing in the top list.
    await this.dragFavoriteTo(sourceName, targetName, "edge");
  }

  private async dragFavoriteTo(sourceName: string, targetName: string, landing: "edge" | "center"): Promise<void> {
    const source = this.favoriteRowByName(sourceName);
    await source.hover();
    const handle = source.getByTestId("favorite-drag-handle");
    const from = await handle.boundingBox();
    if (!from) throw new Error(`[parity] no drag handle for ${sourceName}.`);
    const target = this.favoriteRowByName(targetName);
    const to = await target.boundingBox();
    if (!to) throw new Error(`[parity] no favorite row for ${targetName}.`);
    await this.page.mouse.move(from.x + from.width - 2, from.y + from.height / 2);
    await this.page.mouse.down();
    const landY = landing === "center" ? to.y + to.height / 2 : to.y + 2;
    await this.page.mouse.move(to.x + to.width / 2, landY, { steps: 12 });
    await this.page.mouse.up();
  }

  async openFavoriteQuickMenu(name: string): Promise<void> {
    const row = this.favoriteRowByName(name);
    await row.hover();
    await row.getByTestId("favorite-row-menu").click();
    await this.page.getByRole("menuitem").first().waitFor({ state: "visible", timeout: 10_000 });
  }

  async favoriteQuickMenuTexts(): Promise<string[]> {
    const texts = await this.page.getByRole("menuitem").allInnerTexts();
    return texts.map((text) => text.trim()).filter((text) => text.length > 0);
  }

  async activateFavoriteQuickMenuItem(name: string): Promise<void> {
    const entry = this.page.getByRole("menuitem", { name }).first();
    await entry.scrollIntoViewIfNeeded().catch(() => undefined);
    await entry.click();
  }

  async setSidebarCollapsed(collapsed: boolean): Promise<void> {
    // Sidebar chrome menus (NEWFRONT-287): the collapse flag persists
    // per panel in browser-local storage and the shell reads it on
    // mount, so writing it plus a reload applies the state.
    await this.page.evaluate((value) => {
      const key = "pidash:sidebar-panel:main";
      let snapshot: { sidebarCollapsed: boolean; peekOpen: boolean; sidebarWidth: number } | null = null;
      try {
        snapshot = JSON.parse(localStorage.getItem(key) ?? "null") as typeof snapshot;
      } catch {
        snapshot = null;
      }
      const next = {
        sidebarCollapsed: value,
        peekOpen: false,
        sidebarWidth:
          snapshot !== null && Number.isFinite(snapshot.sidebarWidth) && snapshot.sidebarWidth >= 236
            ? snapshot.sidebarWidth
            : 256,
      };
      localStorage.setItem(key, JSON.stringify(next));
    }, collapsed);
    await this.page.reload({ timeout: 60_000 }).catch(() => {});
    await this.page
      .locator("#main-sidebar")
      .waitFor({ state: "attached", timeout: 45_000 })
      .catch(() => {});
  }

  async isSidebarCollapsed(): Promise<boolean> {
    // Sidebar chrome menus (NEWFRONT-287): collapsed keeps a 1px
    // border, so the live width reads 1, not 0.
    const bar = this.page.locator("#main-sidebar").first();
    if ((await bar.count()) === 0) return false;
    const box = await bar.boundingBox({ timeout: 15_000 }).catch(() => null);
    if (box === null) return true;
    return box.width <= 1;
  }

  async openCompactUserMenu(): Promise<void> {
    // Sidebar chrome menus (NEWFRONT-287): the top-bar avatar trigger,
    // mounted only while the sidebar is collapsed or unmounted.
    await this.page.locator('[data-testid="topbar-user-menu"]').first().click({ timeout: 30_000 });
  }

  async profileSettingsActiveTab(): Promise<string | null> {
    return todo("profileSettingsActiveTab");
  }

  async isSidebarPeekVisible(): Promise<boolean> {
    // Sidebar chrome menus (NEWFRONT-287): the preview overlay mounts
    // only while open, so presence plus visibility reads shown.
    const peek = this.page.locator('[data-testid="sidebar-peek"]');
    return (await peek.count()) > 0 && (await peek.first().isVisible());
  }

  async dismissDialogByOverlayClick(): Promise<void> {
    // Sidebar chrome menus (NEWFRONT-287): some dialogs (product
    // updates) ignore Escape, so click the overlay outside the panel.
    await this.page.mouse.click(5, 200);
  }

  async sidebarProjectPlaceholderCount(): Promise<number> {
    const aside = await this.screenAside();
    const regions = aside.getByRole("status");
    if ((await regions.count()) === 0) return 0;
    return regions.first().locator(":scope > *").count();
  }

  async openDisplayOptions(): Promise<void> {
    return todo("openDisplayOptions");
  }

  async closeDisplayOptions(): Promise<void> {
    return todo("closeDisplayOptions");
  }

  async displayPanelText(): Promise<string> {
    return todo("displayPanelText");
  }

  async setDisplayGroupBy(_option: string): Promise<void> {
    return todo("setDisplayGroupBy");
  }

  async setDisplayOrderBy(_option: string): Promise<void> {
    return todo("setDisplayOrderBy");
  }

  async setDisplayExtraOption(_option: string, _enabled: boolean): Promise<void> {
    return todo("setDisplayExtraOption");
  }

  async isDisplayOptionChecked(_option: string): Promise<boolean> {
    return todo("isDisplayOptionChecked");
  }

  async toggleDisplayProperty(_option: string): Promise<void> {
    return todo("toggleDisplayProperty");
  }

  async isDisplayPropertyActive(_option: string): Promise<boolean> {
    return todo("isDisplayPropertyActive");
  }

  async toggleRichFilterRow(): Promise<void> {
    return todo("toggleRichFilterRow");
  }

  async isRichFilterRowVisible(): Promise<boolean> {
    return todo("isRichFilterRowVisible");
  }

  async richFilterRowText(): Promise<string> {
    return todo("richFilterRowText");
  }

  async addRichCondition(_property: string): Promise<void> {
    return todo("addRichCondition");
  }

  async pickRichValues(_values: string[]): Promise<void> {
    return todo("pickRichValues");
  }

  async pickRichValuesContaining(_values: string[]): Promise<void> {
    return todo("pickRichValuesContaining");
  }

  async listRichPickerOptions(): Promise<string[]> {
    return todo("listRichPickerOptions");
  }

  async richValueOptions(): Promise<string[]> {
    return todo("richValueOptions");
  }

  async richOperatorOptions(): Promise<string[]> {
    return todo("richOperatorOptions");
  }

  async pickRichOperator(_option: string): Promise<void> {
    return todo("pickRichOperator");
  }

  async isSingleRichOperatorLocked(): Promise<boolean> {
    return todo("isSingleRichOperatorLocked");
  }

  async isRichCalendarOpen(): Promise<boolean> {
    return todo("isRichCalendarOpen");
  }

  async pickRichDay(_day: string): Promise<void> {
    return todo("pickRichDay");
  }

  async richConditionCount(): Promise<number> {
    return todo("richConditionCount");
  }

  async removeRichCondition(_index: number): Promise<void> {
    return todo("removeRichCondition");
  }

  async clearRichFilters(): Promise<void> {
    return todo("clearRichFilters");
  }

  async openProjectView(_workspaceSlug: string, _projectId: string, _viewId: string): Promise<void> {
    return todo("openProjectView");
  }

  async saveRichViewAs(_name: string): Promise<void> {
    return todo("saveRichViewAs");
  }

  async updateRichView(): Promise<void> {
    return todo("updateRichView");
  }

  async openAnalytics(): Promise<void> {
    return todo("openAnalytics");
  }

  async closeAnalytics(): Promise<void> {
    return todo("closeAnalytics");
  }

  async analyticsDialogText(): Promise<string> {
    return todo("analyticsDialogText");
  }

  // Command palette / search / help / browse / repo-star (NEWFRONT-127).
  // Skeleton stubs: throw until the apps/web_new palette area lands.

  async currentUrlPath(): Promise<string> {
    return new URL(this.page.url()).pathname;
  }

  async goToPath(path: string): Promise<void> {
    // Settle on parsed DOM, not full load: late sub-resources can stall
    // the load event while the app itself is already interactive.
    await this.page.goto(path, { waitUntil: "domcontentloaded", timeout: 120_000 });
  }

  // Palette locators (SHELL-080-085, NEWFRONT-292/293/294). DOM contract
  // from CommandPalette.tsx: role=dialog; the filter input keeps its
  // label on root and sub-pages; rows are role=option with a single
  // aria-selected=true; group headings are plain text.
  private paletteModal(): Locator {
    // Scoped by the filter input (both root and sub-page views render
    // it), so sibling dialogs never masquerade as the palette.
    return this.page
      .locator('[role="dialog"]')
      .filter({ has: this.page.getByLabel(/Filter (commands|options)/) })
      .first();
  }

  private paletteInput(): Locator {
    return this.paletteModal().locator("input").first();
  }

  private paletteItems(): Locator {
    return this.paletteModal().locator('[role="option"]');
  }

  private async resetPaletteToClosed(): Promise<void> {
    // Two Escapes cover a leftover query (first clears, second
    // closes); blur then grounds focus stranded on the detached input
    // so the next chord reaches the document listener.
    for (let i = 0; i < 2 && (await this.isCommandPaletteOpen()); i++) {
      await this.pressInCommandPalette("Escape");
    }
    await this.page
      .waitForFunction(
        () =>
          [...document.querySelectorAll('[role="dialog"]')].every((dialog) => dialog.querySelector("input") === null),
        { timeout: 10_000 }
      )
      .catch(() => undefined);
    await this.page.evaluate(() => {
      if (document.activeElement instanceof HTMLElement) document.activeElement.blur();
    });
    await this.page.keyboard.press("Escape");
  }

  private async paletteSettledAtRoot(): Promise<boolean> {
    // Commands stream in as their data resolves, so require the root
    // prompt plus a stable non-empty option count across half a second.
    if ((await this.commandPalettePlaceholder()) !== "Type a command or search") return false;
    const first = await this.paletteItems().count();
    if (first === 0) return false;
    await this.page.waitForTimeout(500);
    if ((await this.commandPalettePlaceholder()) !== "Type a command or search") return false;
    const second = await this.paletteItems().count();
    return second === first && second > 0;
  }

  async pressPaletteOpenChord(): Promise<void> {
    // The shell catches mod+k on document before its typing guard, so
    // the chord opens the palette from anywhere including inputs
    // (SHELL-080). Retry until open, populated and back at root; the
    // store resets to fresh state on every open.
    for (let attempt = 0; attempt < 12; attempt++) {
      if (attempt > 0) await this.resetPaletteToClosed();
      if (!(await this.isCommandPaletteOpen())) {
        await this.page.keyboard.press("ControlOrMeta+k");
      }
      const deadline = Date.now() + 10_000;
      while (Date.now() < deadline) {
        if (await this.paletteSettledAtRoot()) return;
        await this.page.waitForTimeout(250);
      }
    }
    throw new Error("[parity] command palette never settled at root.");
  }

  async isCommandPaletteOpen(): Promise<boolean> {
    return this.isShown(this.paletteInput());
  }

  async commandPalettePlaceholder(): Promise<string | null> {
    const input = this.paletteInput();
    if ((await input.count()) === 0) return null;
    return input.first().getAttribute("placeholder");
  }

  async focusAndTypeTopBarSearch(text: string): Promise<void> {
    // Focus the top-bar search field and type, proving the open chord
    // still fires while a text field holds focus (SHELL-080).
    const search = this.topBarSearchInput();
    await search.click();
    await search.pressSequentially(text);
  }

  async closeCommandPaletteViaBackdrop(): Promise<void> {
    // The dialog backdrop is a full-screen layer behind the centered
    // panel; a top-left click lands on it.
    await this.page.mouse.click(5, 5);
  }

  async typeInCommandPalette(text: string): Promise<void> {
    await this.paletteInput().pressSequentially(text);
  }

  async commandPaletteQueryValue(): Promise<string> {
    const input = this.paletteInput();
    if ((await input.count()) === 0) return "";
    return (await input.first().inputValue()) ?? "";
  }

  async pressInCommandPalette(key: string): Promise<void> {
    await this.paletteInput().press(key);
  }

  async paletteGroupHeadings(): Promise<string[]> {
    // Root listbox groups render a heading paragraph above each item
    // list; the server-results section is scoped out.
    const texts = await this.paletteModal().locator('div[role="listbox"] > div > p').allTextContents();
    return texts.map((t) => t.replace(/\s+/g, " ").trim()).filter((t) => t.length > 0);
  }

  async paletteCommandTitles(): Promise<string[]> {
    // Each option renders its title plus hint/shortcut badges; callers
    // match by substring rather than exact equality.
    const texts = await this.paletteItems().allTextContents();
    return texts.map((t) => t.replace(/\s+/g, " ").trim()).filter((t) => t.length > 0);
  }

  async paletteHasCommand(title: string): Promise<boolean> {
    return this.isShown(this.paletteItems().filter({ hasText: title }));
  }

  async activatePaletteCommand(title: string): Promise<void> {
    // Bounded click with one re-settle retry: the palette can detach
    // between the caller's settle and this click, and an unbounded
    // click then hangs until the test timeout.
    try {
      await this.paletteItems().filter({ hasText: title }).first().click({ timeout: 15_000 });
    } catch {
      await this.pressPaletteOpenChord();
      await this.paletteItems().filter({ hasText: title }).first().click({ timeout: 15_000 });
    }
  }

  async paletteSelectedItemText(): Promise<string | null> {
    const selected = this.paletteModal().locator('[role="option"][aria-selected="true"]').first();
    if ((await selected.count()) === 0) return null;
    const text = ((await selected.textContent()) ?? "").replace(/\s+/g, " ").trim();
    return text === "" ? null : text;
  }

  private searchResultsHeading(): Locator {
    return this.paletteModal().locator('section[aria-label="Server results"] > p').first();
  }

  async paletteSearchResultsHeading(): Promise<string | null> {
    const heading = this.searchResultsHeading();
    if ((await heading.count()) === 0) return null;
    const text = ((await heading.textContent()) ?? "").replace(/\s+/g, " ").trim();
    return text === "" ? null : text;
  }

  async isPaletteSearchHeadingPulsing(): Promise<boolean> {
    return todo("isPaletteSearchHeadingPulsing");
  }

  private workspaceLevelToggle(): Locator {
    // Footer scope control labelled "Workspace level".
    return this.paletteModal()
      .getByRole("switch", { name: /Workspace level/i })
      .first();
  }

  async paletteHasWorkspaceLevelToggle(): Promise<boolean> {
    return this.isShown(this.workspaceLevelToggle());
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

  private shortcutsDialog(): Locator {
    // DOM contract from ShortcutsDialog.tsx: role=dialog titled
    // "Keyboard shortcuts" with a "Search for shortcuts" filter.
    return this.page
      .locator('[role="dialog"]')
      .filter({ has: this.page.getByPlaceholder("Search for shortcuts") })
      .first();
  }

  async isShortcutsDialogOpen(): Promise<boolean> {
    return this.isShown(this.shortcutsDialog());
  }

  async pressShortcutsDialogChord(): Promise<void> {
    await this.page.keyboard.press("ControlOrMeta+/");
  }

  async typeShortcutsFilter(text: string): Promise<void> {
    await this.page.getByPlaceholder("Search for shortcuts").fill(text);
  }

  async shortcutsDialogCommandTitles(): Promise<string[]> {
    return todo("shortcutsDialogCommandTitles");
  }

  // Repo-star action (NEWFRONT-294, SHELL-103). The new top bar mounts
  // the same named link with a theme-adaptive icon image.
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

  async repoStarIconThemed(): Promise<boolean> {
    // Web_new renders a Lucide vector icon, not a swapped png: its
    // currentColor stroke inherits the theme's muted text color, which is
    // the redesign's equivalent of the theme-adaptive asset (SHELL-103).
    const icon = this.page
      .getByRole("link", { name: /Star us on GitHub/i })
      .first()
      .locator("svg")
      .first();
    if ((await icon.count()) === 0) return false;
    const stroke = await icon.getAttribute("stroke");
    const fill = await icon.getAttribute("fill");
    return stroke === "currentColor" || fill === "currentColor";
  }

  // Preferences (NEWFRONT-294, SHELL-095/096). The applied theme is the
  // <html> data-theme attribute; the interface language is the root lang.
  async documentTheme(): Promise<string> {
    return (await this.page.locator("html").getAttribute("data-theme")) ?? "";
  }

  async documentLang(): Promise<string> {
    return (await this.page.locator("html").getAttribute("lang")) ?? "";
  }

  async paletteHasText(text: string): Promise<boolean> {
    return this.isShown(this.paletteModal().getByText(text, { exact: false }));
  }

  // Browse route (NEWFRONT-288, row SHELL-106): the key address renders
  // the project-scoped detail, never a workspace-wide browser.
  async openBrowseWorkItem(workspaceSlug: string, identifier: string): Promise<void> {
    await this.goToPath(`/${workspaceSlug}/browse/${identifier}`);
  }

  async browseShowsWorkItemDetail(): Promise<boolean> {
    // The unknown-key branch renders an explicit empty state; its
    // absence (paired with the caller's route-URL and title assertions)
    // proves the project-scoped detail rendered instead.
    const missing = this.page.getByText("No work item matches this address");
    if ((await missing.count()) === 0) return true;
    return !(await missing
      .first()
      .isVisible()
      .catch(() => false));
  }

  async browseShowsWorkspaceWideList(): Promise<boolean> {
    // A cross-project browser would render many project/work-item cards
    // or a list grid; the negative row asserts none exists.
    const grid = this.page.locator('[class*="grid-cols-"]').filter({
      has: this.page.locator('a[href*="/projects/"]'),
    });
    if ((await grid.count()) === 0) return false;
    return await grid
      .first()
      .isVisible()
      .catch(() => false);
  }

  // Top-bar search box (NEWFRONT-127, SHELL-081). Throwing stubs per the
  // shared driver contract.
  // Top-bar search box (SHELL-081). DOM contract from
  // TopBarSearch.tsx: the labeled field plus an inline role=listbox
  // panel named "Top bar search results" (never a dialog).
  private topBarSearchInput(): Locator {
    return this.page.getByLabel("Search commands").first();
  }

  private topBarResultsPanel(): Locator {
    return this.page.getByRole("listbox", { name: "Top bar search results" }).first();
  }

  async topBarSearchPlaceholder(): Promise<string | null> {
    const input = this.topBarSearchInput();
    if ((await input.count()) === 0) return null;
    return input.getAttribute("placeholder");
  }

  async focusTopBarSearch(): Promise<void> {
    await this.topBarSearchInput().click();
  }

  async isTopBarResultsOpen(): Promise<boolean> {
    if (await this.isCommandPaletteOpen()) return false;
    return this.isShown(this.topBarResultsPanel());
  }

  async typeInTopBarSearch(text: string): Promise<void> {
    await this.topBarSearchInput().pressSequentially(text);
  }

  async topBarSearchValue(): Promise<string> {
    const input = this.topBarSearchInput();
    if ((await input.count()) === 0) return "";
    return (await input.inputValue()) ?? "";
  }

  async topBarResultsCommandTitles(): Promise<string[]> {
    const texts = await this.topBarResultsPanel().locator('[role="option"]').allTextContents();
    return texts.map((t) => t.replace(/\s+/g, " ").trim()).filter((t) => t.length > 0);
  }

  async pressInTopBarSearch(key: string): Promise<void> {
    await this.topBarSearchInput().press(key);
  }

  async closeTopBarViaOutsideClick(): Promise<void> {
    // The inline panel closes on outside mousedown; a top-left click
    // lands far from the centered top-bar field.
    await this.page.mouse.click(5, 5);
  }

  // Shared empty-state kit tiers (NEWFRONT-294, SHELL-104). The reader
  // scopes to the box carrying the given exact title text, mirroring the
  // oracle queries against the new kit's rendered structure.
  private emptyStateBox(title: string): Locator {
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
    const input = this.page.getByPlaceholder("Type to search").first();
    await input.waitFor({ timeout: 60_000 });
    await input.pressSequentially(text);
  }

  // Cover-image primitive (NEWFRONT-294, SHELL-105). The new app renders no
  // cover slots (target per main#523), so these reads observe whatever the
  // projects list renders — the negative row asserts they stay empty.
  private coverProjectCards(): Locator {
    return this.page
      .locator('a[href*="/projects/"][href*="/issues"]')
      .filter({ has: this.page.locator("img, .animate-pulse") });
  }

  async projectCardCoverSrcs(): Promise<(string | null)[]> {
    const cards = this.coverProjectCards();
    const count = await cards.count();
    const srcs: (string | null)[] = [];
    for (let i = 0; i < count; i += 1) {
      const img = cards.nth(i).locator("img").first();
      srcs.push((await this.isShown(img)) ? await img.getAttribute("src") : null);
    }
    return srcs;
  }

  async projectCardCoverShimmerVisible(): Promise<boolean> {
    return this.isShown(this.coverProjectCards().locator(".animate-pulse").first());
  }

  // --- Projects list + lifecycle (NEWFRONT-124, SHELL-024..045). ---
  // Implemented by the shell area gate (NEWFRONT-295) against the
  // ProjectsListScreen/View/Header/Card components.
  private projectCardByName(name: string): Locator {
    return this.page
      .locator('[data-testid="project-card"]')
      .filter({ has: this.page.getByRole("heading", { name, exact: true }) });
  }

  private headerCreateButton(): Locator {
    // Header create button: label "Add Project" (>=sm) or "Project" (below sm).
    return this.page.getByRole("button", { name: /^(Add Project|Project)$/ });
  }

  private emptyStateCreateButton(): Locator {
    return this.page.getByRole("button", { name: "Start your first project", exact: true });
  }

  async openArchivedProjects(workspaceSlug: string): Promise<void> {
    await this.page.goto(`/${workspaceSlug}/projects/archives`);
    await this.page.waitForLoadState("domcontentloaded");
    await this.page.locator("#main-sidebar").waitFor({ state: "attached", timeout: 45_000 });
  }

  async visibleProjectCardNames(): Promise<string[]> {
    const headings = this.page.locator('[data-testid="project-card"]').getByRole("heading");
    const texts = await headings.allTextContents();
    return texts.map((t) => t.trim()).filter((t) => t.length > 0);
  }

  async awaitProjectCard(name: string): Promise<void> {
    await this.projectCardByName(name).first().waitFor({ timeout: 60_000 });
  }

  async gridColumnCount(): Promise<number> {
    // The card grid sets grid-template-columns; count the resolved tracks.
    const grid = this.page.locator('[data-testid="projects-grid"]');
    const cols = await grid.first().evaluate((el) => {
      const tpl = getComputedStyle(el).gridTemplateColumns;
      return tpl.split(" ").filter((t) => t.trim().length > 0).length;
    });
    return cols;
  }

  async setViewportWidth(width: number): Promise<void> {
    const current = this.page.viewportSize();
    await this.page.setViewportSize({ width, height: current?.height ?? 900 });
  }

  async isProjectsSkeletonVisible(): Promise<boolean> {
    return this.isShown(this.page.locator('[data-testid="projects-skeleton"]'));
  }

  async emptyStateHeading(): Promise<string | null> {
    for (const text of ["No active projects", "No matching results.", "No projects archived"]) {
      const loc = this.page.getByRole("heading", { name: text, exact: false });
      if (await this.isShown(loc)) return (await loc.first().textContent())?.trim() ?? text;
    }
    return null;
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
    // block; signature the biggest illustration (icons are an order of
    // magnitude smaller than the art).
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

  async isHeaderCreateButtonVisible(): Promise<boolean> {
    return this.isShown(this.headerCreateButton());
  }

  async headerCreateButtonLabel(): Promise<string | null> {
    // The button dual-mounts its label (full + short) with one hidden
    // per breakpoint; textContent would concatenate both, so read the
    // visible span only.
    const button = this.headerCreateButton();
    if (!(await this.isShown(button))) return null;
    const label = button.first().locator("span").filter({ visible: true }).first();
    if ((await label.count()) === 0) return null;
    const text = ((await label.textContent()) ?? "").replace(/\s+/g, " ").trim();
    return text === "" ? null : text;
  }

  async clickHeaderCreateButton(): Promise<void> {
    await this.headerCreateButton().first().click();
  }

  async breadcrumbLabels(): Promise<string[]> {
    const crumbs = this.page.locator('nav[aria-label="Breadcrumb"] li');
    const texts = await crumbs.allTextContents();
    return texts.map((t) => t.replace(/\s+/g, " ").trim()).filter((t) => t.length > 0);
  }

  async isMobileListHeaderVisible(): Promise<boolean> {
    return this.isShown(this.page.locator('[data-testid="projects-mobile-bar"]'));
  }

  async isDesktopFilterRowVisible(): Promise<boolean> {
    return this.isShown(this.page.locator('[data-testid="projects-desktop-filters"]'));
  }

  async breadcrumbTerminalIsLink(): Promise<boolean> {
    const crumbs = this.page.locator('nav[aria-label="Breadcrumb"] li');
    const count = await crumbs.count();
    if (count === 0) return false;
    return (
      (await crumbs
        .nth(count - 1)
        .locator("a")
        .count()) > 0
    );
  }
  private sortTrigger(): Locator {
    // The header dual-mounts sort/filter (desktop row + mobile bar) with
    // one hidden per breakpoint; always act on the visible instance.
    return this.page.locator('[data-testid="projects-sort-trigger"]').filter({ visible: true }).first();
  }

  private filterTrigger(): Locator {
    return this.page.locator('[data-testid="projects-filter-trigger"]').filter({ visible: true }).first();
  }

  private filterPanel(): Locator {
    return this.page.locator('[data-testid="projects-filter-panel"]').filter({ visible: true }).first();
  }

  private appliedFilterStrip(): Locator {
    return this.page.locator('[data-testid="projects-applied-filters"]').first();
  }

  private listSearchScope(): Locator {
    return this.page.locator('[data-testid="projects-list-search"]').first();
  }

  private listSearchInput(): Locator {
    return this.listSearchScope().locator('input[aria-label="Search projects"]');
  }

  async openSortMenu(): Promise<void> {
    await this.sortTrigger().click();
  }

  async selectSortOption(label: string): Promise<void> {
    await this.page.getByRole("menuitem", { name: label, exact: true }).first().click();
  }

  async currentSortLabel(): Promise<string> {
    return ((await this.sortTrigger().textContent()) ?? "").trim();
  }

  async isSortDirectionDisabled(): Promise<boolean> {
    const asc = this.page.getByRole("menuitem", { name: "Ascending", exact: true }).first();
    return asc.isDisabled().catch(() => true);
  }

  async closeMenu(): Promise<void> {
    await this.page.keyboard.press("Escape");
  }

  async openFilterMenu(): Promise<void> {
    await this.filterTrigger().click();
  }

  async typeFilterSearch(text: string): Promise<void> {
    await this.filterPanel().getByLabel("Search filters").fill(text);
  }

  async filterMenuHasOption(text: string): Promise<boolean> {
    return this.isShown(this.filterPanel().getByText(text, { exact: true }));
  }

  async selectFilterOption(label: string): Promise<void> {
    // Every filter value is a labeled checkbox. Person rows carry a
    // decorative avatar beside their text, so text matching would miss
    // "You" (the label reads "<initials>You"); role matching is exact.
    await this.filterPanel().getByRole("checkbox", { name: label, exact: true }).first().click();
  }

  async isFilterBadgeVisible(): Promise<boolean> {
    // The active-filter badge is aria-hidden decoration on the trigger,
    // so presence (not visibility) is the signal.
    return (await this.page.locator('[data-testid="projects-filter-badge"]').count()) > 0;
  }

  async appliedFilterChipTexts(): Promise<string[]> {
    const strip = this.appliedFilterStrip();
    if (!(await this.isShown(strip))) return [];
    const texts = await strip.locator("span, div").allTextContents();
    return texts.map((t) => t.trim()).filter((t) => t.length > 0);
  }

  async removeAppliedFilterChip(text: string): Promise<void> {
    await this.appliedFilterStrip()
      .getByRole("button", { name: `Remove ${text}`, exact: true })
      .click();
  }

  async clickClearAllFilters(): Promise<void> {
    await this.appliedFilterStrip().getByText("Clear all", { exact: true }).first().click();
  }

  async filterMatchCountText(): Promise<string | null> {
    const count = this.page.locator('[data-testid="projects-filter-count"]').first();
    if (!(await this.isShown(count))) return null;
    return (await count.textContent())?.trim() ?? null;
  }

  async openListSearch(): Promise<void> {
    if (await this.isShown(this.listSearchInput())) return;
    // Collapsed until the search button expands it; settle on the open
    // field so a second opener sees it instead of double-toggling.
    await this.listSearchScope().getByRole("button", { name: "Search projects" }).click();
    await this.listSearchInput().waitFor({ state: "visible", timeout: 10_000 });
  }

  async typeListSearch(text: string): Promise<void> {
    await this.openListSearch();
    await this.listSearchInput().pressSequentially(text, { delay: 60 });
  }

  async listSearchValue(): Promise<string> {
    // The clear button unmounts the field as it resets the text, so a
    // missing field reads as blank: every collapse path (clear, second
    // escape, outside click on empty) leaves the query empty.
    const input = this.listSearchInput();
    if ((await input.count()) === 0 || !(await this.isShown(input))) return "";
    return input.inputValue();
  }

  async isListSearchExpanded(): Promise<boolean> {
    return this.isShown(this.listSearchInput());
  }

  async pressEscapeInListSearch(): Promise<void> {
    await this.listSearchInput().press("Escape");
  }

  async clickListSearchClear(): Promise<void> {
    await this.listSearchScope().getByRole("button", { name: "Clear search" }).click();
  }

  async clickOutsideListSearch(): Promise<void> {
    // The leading breadcrumb is plain text with no handlers: a click
    // there is purely "outside" the search field without navigating.
    await this.page.locator('nav[aria-label="Breadcrumb"] li').first().click();
  }
  private projectDialog(): Locator {
    // Card-lifecycle dialogs (create/join/leave/delete/archive/restore)
    // render through the kit dialog; the first open one scopes reads
    // and clicks away from same-named menu entries behind them.
    return this.page.locator('[role="dialog"]').first();
  }

  async cardShortCode(name: string): Promise<string | null> {
    const card = this.projectCardByName(name).first();
    if (!(await this.isShown(card))) return null;
    const code = card.locator("h2 + span > span").first();
    if ((await code.count()) === 0) return null;
    return (await code.textContent())?.trim() ?? null;
  }

  async cardHasPrivateMark(name: string): Promise<boolean> {
    // The lock icon is aria-hidden beside a screen-reader-only "Private
    // project" marker; presence (not visibility) is the signal.
    const card = this.projectCardByName(name).first();
    return (await card.getByText("Private project", { exact: true }).count()) > 0;
  }

  async cardSubText(name: string): Promise<string | null> {
    const card = this.projectCardByName(name).first();
    const sub = card.locator('[data-testid="project-card-subtext"]').first();
    if (!(await this.isShown(sub))) return null;
    return (await sub.textContent())?.trim() ?? null;
  }

  async cardHasFavoriteStar(name: string): Promise<boolean> {
    return this.isShown(this.projectCardByName(name).first().locator('[data-testid="project-card-star"]'));
  }

  async clickFavoriteStar(name: string): Promise<void> {
    await this.projectCardByName(name).first().locator('[data-testid="project-card-star"]').click();
  }

  async cardHasCoverImage(name: string): Promise<boolean> {
    // Cards carry no cover slot (target per main#523): a cover would
    // render as the project-named image, so its absence reads false.
    const card = this.projectCardByName(name).first();
    return this.isShown(card.getByRole("img", { name, exact: true }));
  }

  async cardHasLogo(name: string): Promise<boolean> {
    // The logo box is aria-hidden decoration holding the emoji glyph
    // or the short-code initial; presence plus content is the signal.
    const card = this.projectCardByName(name).first();
    const logo = card.locator('[data-testid="project-logo"]').first();
    if ((await logo.count()) === 0) return false;
    return (((await logo.textContent()) ?? "").trim().length ?? 0) > 0;
  }

  async cardAvatarStack(name: string): Promise<string[]> {
    // Member avatars expose their names with an overflow "+N" bubble
    // past the display cap; an empty team renders guidance instead.
    const card = this.projectCardByName(name).first();
    const stack = card.locator('[data-testid="project-member-stack"]').first();
    if ((await stack.count()) === 0) return [];
    const names: string[] = [];
    const avatars = stack.locator('[role="img"]');
    for (let i = 0, n = await avatars.count(); i < n; i += 1) {
      const label = await avatars.nth(i).getAttribute("aria-label");
      if (label !== null && label.trim() !== "") names.push(label.trim());
    }
    const overflow = stack.locator('[data-testid="project-member-stack-overflow"]').first();
    if ((await overflow.count()) > 0) {
      const text = ((await overflow.textContent()) ?? "").trim();
      if (text !== "") names.push(text);
    }
    return names;
  }

  async clickProjectCard(name: string): Promise<void> {
    await this.projectCardByName(name).first().click();
  }

  async openCardContextMenu(name: string): Promise<void> {
    await this.projectCardByName(name).first().click({ button: "right" });
  }

  async contextMenuItemLabels(): Promise<string[]> {
    const texts = await this.page.getByRole("menuitem").allTextContents();
    return texts.map((t) => t.replace(/\s+/g, " ").trim()).filter((t) => t.length > 0);
  }

  async clickCardContextMenuItem(label: string): Promise<void> {
    await this.page.getByRole("menuitem", { name: label, exact: true }).first().click();
  }
  async cardFooterLabels(_name: string): Promise<string[]> {
    return todo("cardFooterLabels");
  }
  async clickCardJoin(name: string): Promise<void> {
    await this.projectCardByName(name).first().getByRole("button", { name: "Join", exact: true }).click();
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
    await this.projectDialog().getByRole("button", { name: "Join Project", exact: true }).click();
  }

  async openLeaveProjectDialog(projectName: string): Promise<void> {
    // Leave is offered from the sidebar project quick menu for members
    // without an admin/member project role.
    await this.openProjectQuickMenu(projectName);
    await this.page.getByRole("menuitem", { name: "Leave project", exact: true }).click();
  }

  async fillLeaveProjectName(text: string): Promise<void> {
    await this.projectDialog().getByPlaceholder("Enter the project name").fill(text);
  }

  async fillLeaveConfirmPhrase(text: string): Promise<void> {
    await this.projectDialog().getByPlaceholder("Enter 'leave project'").fill(text);
  }

  async submitLeave(): Promise<void> {
    // Scoped to the dialog: the still-open quick menu behind it
    // carries its own same-named entry.
    await this.projectDialog().getByRole("button", { name: "Leave project", exact: true }).click();
  }

  async leaveErrorText(): Promise<string | null> {
    const alert = this.projectDialog().locator('[role="alert"]').first();
    if (!(await this.isShown(alert))) return null;
    return (await alert.textContent())?.trim() ?? null;
  }

  async isLeaveDialogVisible(): Promise<boolean> {
    return this.isShown(this.page.getByRole("heading", { name: "Leave project", exact: true }));
  }

  async openArchiveProjectDialog(_workspaceSlug: string, projectId: string): Promise<void> {
    // Archive is offered from the card menu for project admins; find
    // the card by its work-items address since only the id is known.
    const card = this.page.locator(`[data-testid="project-card"][href*="/projects/${projectId}/"]`).first();
    await card.locator('[data-testid="project-card-actions"]').click();
    await this.page.getByRole("menuitem", { name: "Archive", exact: true }).click();
  }

  async archiveDialogBodyText(): Promise<string | null> {
    const dialog = this.projectDialog();
    if (!(await this.isShown(dialog))) return null;
    const text = ((await dialog.textContent()) ?? "").replace(/\s+/g, " ").trim();
    return text === "" ? null : text;
  }

  async clickCardRestore(name: string): Promise<void> {
    await this.projectCardByName(name).first().getByRole("button", { name: "Restore", exact: true }).click();
  }

  async confirmRestore(): Promise<void> {
    // Scoped to the dialog: the archived card behind it carries its
    // own restore control.
    await this.projectDialog().getByRole("button", { name: "Restore", exact: true }).first().click();
  }

  async archivedCardHasAdminActions(name: string): Promise<boolean> {
    const card = this.projectCardByName(name).first();
    return this.isShown(card.getByText("Restore", { exact: true }));
  }

  async cardShowsArchivedMarker(name: string): Promise<boolean> {
    return this.isShown(this.projectCardByName(name).first().getByText("Archived", { exact: false }));
  }

  async isRestoreDialogVisible(): Promise<boolean> {
    return this.isShown(this.projectDialog().getByRole("heading", { name: /^Restore / }));
  }

  async openDeleteProjectDialog(name: string): Promise<void> {
    await this.openCardContextMenu(name);
    await this.clickCardContextMenuItem("Delete");
  }

  async fillDeleteProjectName(text: string): Promise<void> {
    await this.projectDialog().getByPlaceholder("Project name").fill(text);
  }

  async fillDeleteConfirmPhrase(text: string): Promise<void> {
    await this.projectDialog().getByPlaceholder("Enter 'delete my project'").fill(text);
  }

  async isDeleteSubmitDisabled(): Promise<boolean> {
    return this.projectDialog().getByRole("button", { name: "Delete project", exact: true }).isDisabled();
  }

  async submitDelete(): Promise<void> {
    await this.projectDialog().getByRole("button", { name: "Delete project", exact: true }).click();
  }

  async isCreateProjectDialogVisible(): Promise<boolean> {
    return this.isShown(this.page.getByPlaceholder("Project name"));
  }

  async fillCreateProjectName(text: string): Promise<void> {
    await this.projectDialog().getByPlaceholder("Project name").fill(text);
  }

  async createProjectShortCodeValue(): Promise<string> {
    return this.projectDialog().getByPlaceholder("Project ID").inputValue();
  }

  async fillCreateProjectShortCode(text: string): Promise<void> {
    await this.projectDialog().getByPlaceholder("Project ID").fill(text);
  }

  async submitCreateProject(): Promise<void> {
    // A sticky app-level toast can sit over the submit button and
    // intercept pointer clicks; the button itself is verified present,
    // so dispatch the click when the pointer cannot land.
    const btn = this.projectDialog().getByRole("button", { name: "Create project", exact: true });
    try {
      await btn.click({ timeout: 10_000 });
    } catch {
      await btn.dispatchEvent("click");
    }
  }

  async createProjectErrorText(): Promise<string | null> {
    const alert = this.projectDialog().locator('[role="alert"]').first();
    if (!(await this.isShown(alert))) return null;
    return (await alert.textContent())?.trim() ?? null;
  }

  async createFormCoverVisible(): Promise<boolean> {
    // No cover step exists on the creation path (target per
    // main#523): neither a cover image nor an upload control mounts.
    const dialog = this.projectDialog();
    const img = dialog.getByRole("img", { name: /cover/i });
    const upload = dialog.locator('input[type="file"]');
    return (await img.count()) > 0 || (await upload.count()) > 0;
  }

  async createFormIconVisible(): Promise<boolean> {
    // The prefilled icon button (the picker group behind it opens from
    // a popover only on demand).
    return this.isShown(this.projectDialog().getByRole("button", { name: "Project icon" }));
  }

  // --- Invitation inbox + onboarding start (NEWFRONT-110, AUTH-026/033).
  // --- Throwing stubs per the shared driver contract; the invitations and
  // --- onboarding areas fill these in when they land.
  async openInvitations(): Promise<void> {
    await this.page.goto("/invitations");
    // Any settled inbox state: the list header, the no-invites state, or
    // the load-error state.
    await this.page
      .locator('h1, h2:has-text("No pending invites"), h2:has-text("Could not load invitations")')
      .first()
      .waitFor({ timeout: WebNewDriver.WAIT_MS });
  }

  async invitationWorkspaceNames(): Promise<string[]> {
    // Each invite card is a toggle button whose medium-weight span carries
    // the workspace name (role and member count ride alongside it).
    const names = this.page.locator("main ul li button span.font-medium");
    return (await names.allInnerTexts()).map((name) => name.trim());
  }

  async toggleInvitation(workspaceName: string): Promise<void> {
    await this.page.locator("main ul li button", { hasText: workspaceName }).first().click();
  }

  async acceptSelectedInvitations(): Promise<void> {
    // The batch join round-trips, then lands on the joined workspace; the
    // scenario polls the landing URL.
    await this.page.getByRole("button", { name: "Accept selected", exact: true }).click();
  }

  async invitationsEmptyStateVisible(): Promise<boolean> {
    return todo("invitationsEmptyStateVisible");
  }

  async openInvitationLink(workspaceSlug: string, invitationId: string, token: string): Promise<void> {
    const query = new URLSearchParams({ invitation_id: invitationId, slug: workspaceSlug, token });
    await this.page.goto(`/workspace-invitations?${query.toString()}`);
    // Any settled link state: the invitation, a terminal notice, or the
    // signed-out entry bounce.
    await this.page.locator("h1").first().waitFor({ timeout: WebNewDriver.WAIT_MS });
  }

  async pageText(): Promise<string> {
    await this.page.locator("body").waitFor();
    return ((await this.page.locator("body").innerText()) ?? "").trim();
  }

  async acceptSingleInvitation(): Promise<void> {
    await this.page.getByRole("button", { name: "Accept", exact: true }).click();
  }

  async declineSingleInvitation(): Promise<void> {
    await this.page.getByRole("button", { name: "Ignore", exact: true }).click();
  }

  async openOnboarding(): Promise<void> {
    await this.page.goto("/onboarding");
    await this.page.waitForLoadState("domcontentloaded");
  }

  async advanceCliInstall(): Promise<void> {
    // The step's primary action; the OS pills and copy control share the
    // step but carry their own names.
    await this.page.getByRole("button", { name: "Continue", exact: true }).click();
  }

  async skipCliInstall(): Promise<void> {
    await this.page.getByRole("button", { name: "Skip for now", exact: true }).click();
  }

  async submitProfileStep(displayName: string): Promise<void> {
    await this.page.getByLabel("Your name").fill(displayName);
    // The profile save round-trips, then the workspace step renders; the
    // scenario polls for it.
    await this.page.getByRole("button", { name: "Continue", exact: true }).click();
  }

  async submitRoleStep(_roleLabel: string): Promise<void> {
    return todo("submitRoleStep");
  }

  async skipRoleStep(): Promise<void> {
    return todo("skipRoleStep");
  }

  async submitUseCaseStep(_useCaseLabels: string[]): Promise<void> {
    return todo("submitUseCaseStep");
  }

  async skipUseCaseStep(): Promise<void> {
    return todo("skipUseCaseStep");
  }

  async goBackOnboardingStep(): Promise<void> {
    return todo("goBackOnboardingStep");
  }

  // --- NEWFRONT-117 (layouts A). Throwing stubs per the shared driver
  // --- contract; the layouts area fills these in when it lands.

  async layoutsOfferedLayouts(): Promise<LayoutsLayoutKey[]> {
    return todo("layoutsOfferedLayouts");
  }

  async layoutsActiveLayout(): Promise<LayoutsLayoutKey> {
    return todo("layoutsActiveLayout");
  }

  async layoutsSwitchTo(_layout: LayoutsLayoutKey): Promise<void> {
    return todo("layoutsSwitchTo");
  }

  async layoutsReloadIssues(): Promise<void> {
    return todo("layoutsReloadIssues");
  }

  async layoutsListVisible(): Promise<boolean> {
    return todo("layoutsListVisible");
  }

  async layoutsCalendarVisible(): Promise<boolean> {
    return todo("layoutsCalendarVisible");
  }

  async layoutsSpreadsheetVisible(): Promise<boolean> {
    return todo("layoutsSpreadsheetVisible");
  }

  async layoutsKanbanVisible(): Promise<boolean> {
    return todo("layoutsKanbanVisible");
  }

  async layoutsGanttVisible(): Promise<boolean> {
    return todo("layoutsGanttVisible");
  }

  async layoutsListGroups(): Promise<string[]> {
    return todo("layoutsListGroups");
  }

  async layoutsListGroupExpanded(_title: string): Promise<boolean> {
    return todo("layoutsListGroupExpanded");
  }

  async layoutsListToggleGroup(_title: string): Promise<void> {
    return todo("layoutsListToggleGroup");
  }

  async layoutsListGroupIssueNames(_title: string): Promise<string[]> {
    return todo("layoutsListGroupIssueNames");
  }

  async layoutsListGroupHasLoadMore(_title: string): Promise<boolean> {
    return todo("layoutsListGroupHasLoadMore");
  }

  async layoutsListGroupLoadMore(_title: string): Promise<void> {
    return todo("layoutsListGroupLoadMore");
  }

  async layoutsListScrollEnd(): Promise<void> {
    return todo("layoutsListScrollEnd");
  }

  async layoutsListQuickAdd(_title: string, _groupTitle?: string): Promise<void> {
    return todo("layoutsListQuickAdd");
  }

  async layoutsRowCanEditState(_issueName: string): Promise<boolean> {
    return todo("layoutsRowCanEditState");
  }

  async layoutsRowHref(_issueName: string): Promise<string | null> {
    return todo("layoutsRowHref");
  }

  async layoutsRowOpenPeek(_issueName: string): Promise<void> {
    return todo("layoutsRowOpenPeek");
  }

  async layoutsPeekVisible(): Promise<boolean> {
    return todo("layoutsPeekVisible");
  }

  async layoutsPeekTitle(): Promise<string | null> {
    return todo("layoutsPeekTitle");
  }

  async layoutsPeekClose(): Promise<void> {
    return todo("layoutsPeekClose");
  }

  async layoutsRowHasSubIssueToggle(_issueName: string): Promise<boolean> {
    return todo("layoutsRowHasSubIssueToggle");
  }

  async layoutsRowExpandSubIssues(_issueName: string): Promise<void> {
    return todo("layoutsRowExpandSubIssues");
  }

  async layoutsRowSubIssueNames(_issueName: string): Promise<string[]> {
    return todo("layoutsRowSubIssueNames");
  }

  async layoutsRowState(_issueName: string): Promise<string> {
    return todo("layoutsRowState");
  }

  async layoutsRowSetState(_issueName: string, _stateName: string): Promise<void> {
    return todo("layoutsRowSetState");
  }

  async layoutsRowPriority(_issueName: string): Promise<string> {
    return todo("layoutsRowPriority");
  }

  async layoutsRowSetPriority(_issueName: string, _priorityName: string): Promise<void> {
    return todo("layoutsRowSetPriority");
  }

  async layoutsRowMenuItems(_issueName: string): Promise<string[]> {
    return todo("layoutsRowMenuItems");
  }

  async layoutsRowMenuChoose(_issueName: string, _item: string): Promise<void> {
    return todo("layoutsRowMenuChoose");
  }

  async layoutsRowContextMenuItems(_issueName: string): Promise<string[]> {
    return todo("layoutsRowContextMenuItems");
  }

  async layoutsSheetHeaders(): Promise<string[]> {
    return todo("layoutsSheetHeaders");
  }

  async layoutsSheetRowNames(): Promise<string[]> {
    return todo("layoutsSheetRowNames");
  }

  async layoutsSheetFirstColumnSticky(): Promise<boolean> {
    return todo("layoutsSheetFirstColumnSticky");
  }

  async layoutsSheetFirstColumnShadowed(): Promise<boolean> {
    return todo("layoutsSheetFirstColumnShadowed");
  }

  async layoutsSheetScrollRight(): Promise<void> {
    return todo("layoutsSheetScrollRight");
  }

  async layoutsSheetHeaderSticky(): Promise<boolean> {
    return todo("layoutsSheetHeaderSticky");
  }

  async layoutsSheetCellText(_issueName: string, _column: string): Promise<string> {
    return todo("layoutsSheetCellText");
  }

  async layoutsSheetCellEditable(_issueName: string, _column: string): Promise<boolean> {
    return todo("layoutsSheetCellEditable");
  }

  async layoutsSheetCellSetState(_issueName: string, _stateName: string): Promise<void> {
    return todo("layoutsSheetCellSetState");
  }

  async layoutsSheetCellSetPriority(_issueName: string, _priorityName: string): Promise<void> {
    return todo("layoutsSheetCellSetPriority");
  }

  async layoutsSheetFocusCell(_issueName: string, _column: string): Promise<void> {
    return todo("layoutsSheetFocusCell");
  }

  async layoutsSheetPressArrow(_arrow: "up" | "down" | "left" | "right"): Promise<void> {
    return todo("layoutsSheetPressArrow");
  }

  async layoutsSheetFocusedCell(): Promise<{ issueName: string; column: string } | null> {
    return todo("layoutsSheetFocusedCell");
  }

  async layoutsSheetSortMenu(_column: string): Promise<string[]> {
    return todo("layoutsSheetSortMenu");
  }

  async layoutsSheetSort(_column: string, _direction: "ascending" | "descending"): Promise<void> {
    return todo("layoutsSheetSort");
  }

  async layoutsSheetClearSort(_column: string): Promise<void> {
    return todo("layoutsSheetClearSort");
  }

  async layoutsSheetSortMarker(_column: string): Promise<"ascending" | "descending" | "none"> {
    return todo("layoutsSheetSortMarker");
  }

  async layoutsSheetQuickAdd(_title: string): Promise<void> {
    return todo("layoutsSheetQuickAdd");
  }

  async layoutsSheetScrollEnd(): Promise<void> {
    return todo("layoutsSheetScrollEnd");
  }

  async layoutsSheetHasSubIssueToggle(_issueName: string): Promise<boolean> {
    return todo("layoutsSheetHasSubIssueToggle");
  }

  async layoutsSheetExpandSubIssues(_issueName: string): Promise<void> {
    return todo("layoutsSheetExpandSubIssues");
  }

  async layoutsSheetSubIssueNames(_issueName: string): Promise<string[]> {
    return todo("layoutsSheetSubIssueNames");
  }

  async layoutsSheetOpenSubIssueCount(_issueName: string): Promise<void> {
    return todo("layoutsSheetOpenSubIssueCount");
  }

  async layoutsCalMode(): Promise<"month" | "week"> {
    return todo("layoutsCalMode");
  }

  async layoutsCalTitle(): Promise<string> {
    return todo("layoutsCalTitle");
  }

  async layoutsCalPrev(): Promise<void> {
    return todo("layoutsCalPrev");
  }

  async layoutsCalNext(): Promise<void> {
    return todo("layoutsCalNext");
  }

  async layoutsCalToday(): Promise<void> {
    return todo("layoutsCalToday");
  }

  async layoutsCalMonthPickerMonths(): Promise<string[]> {
    return todo("layoutsCalMonthPickerMonths");
  }

  async layoutsCalMonthPickerYear(): Promise<number> {
    return todo("layoutsCalMonthPickerYear");
  }

  async layoutsCalMonthPickerYearStep(_direction: "prev" | "next"): Promise<void> {
    return todo("layoutsCalMonthPickerYearStep");
  }

  async layoutsCalMonthPickerChoose(_month: string): Promise<void> {
    return todo("layoutsCalMonthPickerChoose");
  }

  async layoutsCalMonthPickerEnabled(): Promise<boolean> {
    return todo("layoutsCalMonthPickerEnabled");
  }

  async layoutsCalSetMode(_mode: "month" | "week"): Promise<void> {
    return todo("layoutsCalSetMode");
  }

  async layoutsCalWeekendsVisible(): Promise<boolean> {
    return todo("layoutsCalWeekendsVisible");
  }

  async layoutsCalSetWeekends(_show: boolean): Promise<void> {
    return todo("layoutsCalSetWeekends");
  }

  async layoutsCalColumnCount(): Promise<number> {
    return todo("layoutsCalColumnCount");
  }

  async layoutsCalDayIssueNames(_dayNumber: number): Promise<string[]> {
    return todo("layoutsCalDayIssueNames");
  }

  async layoutsCalDayIsToday(_dayNumber: number): Promise<boolean> {
    return todo("layoutsCalDayIsToday");
  }

  async layoutsCalDayHasLoadMore(_dayNumber: number): Promise<boolean> {
    return todo("layoutsCalDayHasLoadMore");
  }

  async layoutsCalDayLoadMore(_dayNumber: number): Promise<void> {
    return todo("layoutsCalDayLoadMore");
  }

  async layoutsCalDragBlock(_issueName: string, _toDayNumber: number): Promise<void> {
    return todo("layoutsCalDragBlock");
  }

  async layoutsCalTileDrag(_fromDayNumber: number, _toDayNumber: number): Promise<void> {
    return todo("layoutsCalTileDrag");
  }

  async layoutsCalBlockText(_issueName: string): Promise<string> {
    return todo("layoutsCalBlockText");
  }

  async layoutsCalBlockHoverPreview(_issueName: string): Promise<boolean> {
    return todo("layoutsCalBlockHoverPreview");
  }

  async layoutsCalBlockOpenPeek(_issueName: string): Promise<void> {
    return todo("layoutsCalBlockOpenPeek");
  }

  async layoutsCalBlockQuickActions(_issueName: string): Promise<string[]> {
    return todo("layoutsCalBlockQuickActions");
  }

  async layoutsCalDayQuickAdd(_dayNumber: number, _title: string): Promise<void> {
    return todo("layoutsCalDayQuickAdd");
  }

  async layoutsCalDayAddMenu(_dayNumber: number): Promise<string[]> {
    return todo("layoutsCalDayAddMenu");
  }

  async layoutsCalTapDay(_dayNumber: number): Promise<void> {
    return todo("layoutsCalTapDay");
  }

  async layoutsCalDayDetailNames(): Promise<string[]> {
    return todo("layoutsCalDayDetailNames");
  }

  async layoutsRowMenuItemDisabled(_issueName: string, _item: string): Promise<boolean> {
    return todo("layoutsRowMenuItemDisabled");
  }

  async layoutsRowMenuItemNote(_issueName: string, _item: string): Promise<string | null> {
    return todo("layoutsRowMenuItemNote");
  }

  async layoutsWorkItemModalVisible(): Promise<boolean> {
    return todo("layoutsWorkItemModalVisible");
  }

  async layoutsWorkItemModalTitle(): Promise<string | null> {
    return todo("layoutsWorkItemModalTitle");
  }

  async layoutsWorkItemModalClose(): Promise<void> {
    return todo("layoutsWorkItemModalClose");
  }

  async layoutsWorkItemModalSetTitle(): Promise<void> {
    return todo("layoutsWorkItemModalSetTitle");
  }

  async layoutsWorkItemModalSubmit(): Promise<void> {
    return todo("layoutsWorkItemModalSubmit");
  }

  async layoutsDeleteModalVisible(): Promise<boolean> {
    return todo("layoutsDeleteModalVisible");
  }

  async layoutsDeleteModalConfirm(): Promise<void> {
    return todo("layoutsDeleteModalConfirm");
  }

  async layoutsArchiveModalVisible(): Promise<boolean> {
    return todo("layoutsArchiveModalVisible");
  }

  async layoutsArchiveModalConfirm(): Promise<void> {
    return todo("layoutsArchiveModalConfirm");
  }

  async layoutsMoveModalVisible(): Promise<boolean> {
    return todo("layoutsMoveModalVisible");
  }

  async layoutsMoveModalChoose(_projectName: string): Promise<void> {
    return todo("layoutsMoveModalChoose");
  }

  async layoutsAddExistingModalVisible(): Promise<boolean> {
    return todo("layoutsAddExistingModalVisible");
  }

  async layoutsAddExistingModalChoose(_issueName: string): Promise<void> {
    return todo("layoutsAddExistingModalChoose");
  }

  async layoutsDetailMenuItems(): Promise<string[]> {
    return todo("layoutsDetailMenuItems");
  }

  async layoutsDetailMenuChoose(_item: string): Promise<void> {
    return todo("layoutsDetailMenuChoose");
  }

  async layoutsPeekCopyLinkVisible(): Promise<boolean> {
    return todo("layoutsPeekCopyLinkVisible");
  }

  async layoutsListPageMenuItems(): Promise<string[]> {
    return todo("layoutsListPageMenuItems");
  }

  async layoutsGroupHeaderAddMenu(_groupTitle: string): Promise<string[] | null> {
    return todo("layoutsGroupHeaderAddMenu");
  }

  async layoutsEmptyTitle(): Promise<string | null> {
    return todo("layoutsEmptyTitle");
  }

  async layoutsEmptyActions(): Promise<Array<{ label: string; disabled: boolean }>> {
    return todo("layoutsEmptyActions");
  }

  async layoutsEmptyChoose(_label: string): Promise<void> {
    return todo("layoutsEmptyChoose");
  }

  async layoutsFilterAddConditionViaRow(_propertyLabel: string, _valueLabel: string): Promise<void> {
    return todo("layoutsFilterAddConditionViaRow");
  }

  async layoutsSeedArchivedLocalFilter(
    _workspaceSlug: string,
    _projectId: string,
    _expression: unknown
  ): Promise<void> {
    return todo("layoutsSeedArchivedLocalFilter");
  }

  async layoutsProfileActivityVisible(): Promise<boolean> {
    return todo("layoutsProfileActivityVisible");
  }

  async layoutsMobileOfferedLayouts(): Promise<LayoutsLayoutKey[]> {
    return todo("layoutsMobileOfferedLayouts");
  }

  async layoutsMobileDisplayVisible(): Promise<boolean> {
    return todo("layoutsMobileDisplayVisible");
  }

  async layoutsMobileAnalyticsVisible(): Promise<boolean> {
    return todo("layoutsMobileAnalyticsVisible");
  }

  async layoutsSkeletonVisible(): Promise<boolean> {
    return todo("layoutsSkeletonVisible");
  }

  async layoutsMutationSpinnerVisible(): Promise<boolean> {
    return todo("layoutsMutationSpinnerVisible");
  }

  async layoutsRowHighlighted(_issueName: string): Promise<boolean> {
    return todo("layoutsRowHighlighted");
  }

  async layoutsTempRowVisible(): Promise<boolean> {
    return todo("layoutsTempRowVisible");
  }

  async layoutsStallIssuesGet(_delayMs: number): Promise<void> {
    return todo("layoutsStallIssuesGet");
  }

  async layoutsStallIssueMutation(_delayMs: number): Promise<void> {
    return todo("layoutsStallIssueMutation");
  }

  async layoutsStalledMutationCount(): Promise<number> {
    return todo("layoutsStalledMutationCount");
  }

  async layoutsReleaseStalls(): Promise<void> {
    return todo("layoutsReleaseStalls");
  }

  async layoutsSheetCellSetDueDate(_issueName: string, _isoDate: string): Promise<void> {
    return todo("layoutsSheetCellSetDueDate");
  }

  async layoutsCurrentUrl(): Promise<string> {
    return todo("layoutsCurrentUrl");
  }

  async layoutsSheetCellSetAssignee(_issueName: string, _memberName: string): Promise<void> {
    return todo("layoutsSheetCellSetAssignee");
  }

  async layoutsCalDayAddExisting(_dayNumber: number): Promise<void> {
    return todo("layoutsCalDayAddExisting");
  }

  async layoutsAddExistingModalIssueNames(): Promise<string[]> {
    return todo("layoutsAddExistingModalIssueNames");
  }

  async layoutsMobileSwitchTo(_layout: LayoutsLayoutKey): Promise<void> {
    return todo("layoutsMobileSwitchTo");
  }

  async layoutsMobileDisplayCycleModuleDisabled(): Promise<{ cycleDisabled: boolean; moduleDisabled: boolean }> {
    return todo("layoutsMobileDisplayCycleModuleDisabled");
  }

  async layoutsRowMenuOpenNewTabUrl(_issueName: string): Promise<string> {
    return todo("layoutsRowMenuOpenNewTabUrl");
  }

  async layoutsWorkItemModalHasText(_text: string): Promise<boolean> {
    return todo("layoutsWorkItemModalHasText");
  }

  async layoutsListPageMenuChoose(_item: string): Promise<void> {
    return todo("layoutsListPageMenuChoose");
  }

  async layoutsGroupHeaderAddChoose(_groupTitle: string, _item: string | null): Promise<void> {
    return todo("layoutsGroupHeaderAddChoose");
  }

  async layoutsSheetToggleSubIssues(_issueName: string): Promise<void> {
    return todo("layoutsSheetToggleSubIssues");
  }
  // --- NEWFRONT-118 (layouts B): kanban board + gantt timeline stubs.
  // --- Appended; existing stubs above are untouched.

  async kanbanOpenBoard(): Promise<void> {
    return todo("kanbanOpenBoard");
  }

  async kanbanBoardVisible(): Promise<boolean> {
    return todo("kanbanBoardVisible");
  }

  // --- Issues timeline (NEWFRONT-624, ISS-044–049). The header switcher
  // --- marks the active layout with aria-pressed; the chart carries the
  // --- header/prose titles and day-indexed cells the scenarios read.
  // --- Drag, resize, add-block and quick-add stubs stay untouched: the
  // --- sibling slices own those rows.

  /** Ordered layout keys in the header switcher. */
  private static readonly BOARD_ORDER: readonly BoardLayoutKey[] = [
    "list",
    "kanban",
    "calendar",
    "spreadsheet",
    "gantt",
  ];

  /** Layout keys as the switcher's DOM markers spell them. */
  private static readonly BOARD_DOM_KEYS: readonly string[] = [
    "list",
    "kanban",
    "calendar",
    "spreadsheet",
    "gantt_chart",
  ];

  private boardSwitcherSegments(): Locator {
    return this.page.getByTestId("layout-switcher-segmented").getByRole("button");
  }

  private ganttContainer(): Locator {
    return this.page.locator("#gantt-container").first();
  }

  private ganttHeaderBand(): Locator {
    return this.page.getByTestId("gantt-header").first();
  }

  async ganttOpenTimeline(): Promise<void> {
    const segments = this.boardSwitcherSegments();
    await segments.first().waitFor({ timeout: WebNewDriver.WAIT_MS });
    const gantt = this.page.locator('[data-layout="gantt_chart"]').first();
    const candidates = [this.page.getByTestId("layout-switcher-dropdown").getByRole("button").first(), gantt];
    for (const candidate of candidates) {
      if (!(await candidate.isVisible().catch(() => false))) continue;
      await candidate.click({ timeout: WebNewDriver.WAIT_MS }).catch(() => undefined);
      if (await this.isShown(this.page.locator('[role="menuitem"], [role="menuitemradio"]').first())) {
        await this.page
          .getByRole("menuitem", { name: "Timeline", exact: true })
          .first()
          .click({ timeout: WebNewDriver.WAIT_MS });
      }
      if (await this.ganttTimelineVisible()) break;
    }
    await this.page.getByTestId("gantt-container").first().waitFor({ timeout: WebNewDriver.WAIT_MS });
  }

  async ganttTimelineVisible(): Promise<boolean> {
    const count = await this.page.locator("#gantt-container").count();
    if (count === 0) return false;
    return this.page.locator("#gantt-container").first().isVisible();
  }

  async boardActiveLayout(): Promise<BoardLayoutKey> {
    const segments = this.boardSwitcherSegments();
    const total = await segments.count();
    for (let index = 0; index < total; index += 1) {
      const pressed = await segments
        .nth(index)
        .getAttribute("aria-pressed")
        .catch(() => null);
      if (pressed !== "true") continue;
      const key =
        (await segments
          .nth(index)
          .getAttribute("data-layout")
          .catch(() => null)) ?? "";
      const found = WebNewDriver.BOARD_DOM_KEYS.indexOf(key);
      return WebNewDriver.BOARD_ORDER[found] ?? "list";
    }
    return "list";
  }

  async boardReloadIssues(): Promise<void> {
    await this.page.reload();
    await this.boardSwitcherSegments().first().waitFor({ timeout: WebNewDriver.WAIT_MS });
  }

  async kanbanColumns(): Promise<KanbanColumn[]> {
    return todo("kanbanColumns");
  }

  async kanbanSwimlanes(): Promise<KanbanColumn[]> {
    return todo("kanbanSwimlanes");
  }

  async kanbanCards(): Promise<KanbanCard[]> {
    return todo("kanbanCards");
  }

  async kanbanColumnCards(_columnName: string): Promise<string[]> {
    return todo("kanbanColumnCards");
  }

  async kanbanToggleColumn(_columnName: string): Promise<void> {
    return todo("kanbanToggleColumn");
  }

  async kanbanColumnCollapsed(_columnName: string): Promise<boolean> {
    return todo("kanbanColumnCollapsed");
  }

  async kanbanToggleSwimlane(_laneName: string): Promise<void> {
    return todo("kanbanToggleSwimlane");
  }

  async kanbanSwimlaneCollapsed(_laneName: string): Promise<boolean> {
    return todo("kanbanSwimlaneCollapsed");
  }

  async kanbanCardIdentifier(_issueName: string): Promise<string | null> {
    return todo("kanbanCardIdentifier");
  }

  async kanbanCardShowsProperties(_issueName: string): Promise<boolean> {
    return todo("kanbanCardShowsProperties");
  }

  async kanbanCardHover(_issueName: string): Promise<void> {
    return todo("kanbanCardHover");
  }

  async kanbanCardQuickActionsVisible(_issueName: string): Promise<boolean> {
    return todo("kanbanCardQuickActionsVisible");
  }

  async kanbanCardHref(_issueName: string): Promise<string | null> {
    return todo("kanbanCardHref");
  }

  async kanbanOpenCardPeek(_issueName: string): Promise<void> {
    return todo("kanbanOpenCardPeek");
  }

  private issuePeekPanel(): Locator {
    return this.page.getByTestId("issue-preview").first();
  }

  async issuePeekVisible(): Promise<boolean> {
    const panel = this.issuePeekPanel();
    if ((await panel.count()) === 0) return false;
    return panel.isVisible();
  }

  async issuePeekTitle(): Promise<string | null> {
    if (!(await this.issuePeekVisible())) return null;
    return this.issuePeekPanel().getByLabel("Work item title", { exact: true }).first().inputValue();
  }

  async issuePeekClose(): Promise<void> {
    await this.page.getByRole("button", { name: "Close preview", exact: true }).first().click({
      timeout: WebNewDriver.WAIT_MS,
    });
    await expect.poll(() => this.issuePeekVisible(), { timeout: WebNewDriver.WAIT_MS }).toBe(false);
  }

  async kanbanColumnHasQuickAdd(_columnName: string): Promise<boolean> {
    return todo("kanbanColumnHasQuickAdd");
  }

  async kanbanQuickAdd(_columnName: string, _title: string): Promise<void> {
    return todo("kanbanQuickAdd");
  }

  async kanbanHeaderCreateVisible(_columnName: string): Promise<boolean> {
    return todo("kanbanHeaderCreateVisible");
  }

  async kanbanHeaderCreate(_columnName: string): Promise<void> {
    return todo("kanbanHeaderCreate");
  }

  async kanbanCreateModalVisible(): Promise<boolean> {
    return todo("kanbanCreateModalVisible");
  }

  async kanbanHeaderMenuItems(_columnName: string): Promise<string[]> {
    return todo("kanbanHeaderMenuItems");
  }

  async kanbanHeaderMenuChoose(_columnName: string, _item: string): Promise<void> {
    return todo("kanbanHeaderMenuChoose");
  }

  async kanbanDragCardBefore(_sourceName: string, _targetName: string): Promise<void> {
    return todo("kanbanDragCardBefore");
  }

  async kanbanAttemptCardBefore(_sourceName: string, _targetName: string): Promise<void> {
    return todo("kanbanAttemptCardBefore");
  }

  async kanbanDragCardToColumnEnd(_sourceName: string, _columnName: string): Promise<void> {
    return todo("kanbanDragCardToColumnEnd");
  }

  async kanbanDragCardToDelete(_sourceName: string): Promise<void> {
    return todo("kanbanDragCardToDelete");
  }

  async kanbanDeleteModalVisible(): Promise<boolean> {
    return todo("kanbanDeleteModalVisible");
  }

  async kanbanConfirmDelete(): Promise<void> {
    return todo("kanbanConfirmDelete");
  }

  async kanbanDragHoldOverColumn(_sourceName: string, _columnName: string): Promise<{ overlay: string | null }> {
    return todo("kanbanDragHoldOverColumn");
  }

  async boardLastToast(): Promise<{ title: string; message: string } | null> {
    return todo("boardLastToast");
  }

  async kanbanColumnScrollEnd(_columnName: string): Promise<void> {
    return todo("kanbanColumnScrollEnd");
  }

  async kanbanColumnHasLoadMore(_columnName: string): Promise<boolean> {
    return todo("kanbanColumnHasLoadMore");
  }

  async kanbanColumnLoadMore(_columnName: string): Promise<void> {
    return todo("kanbanColumnLoadMore");
  }

  async kanbanColumnLoading(_columnName: string): Promise<boolean> {
    return todo("kanbanColumnLoading");
  }

  async kanbanCellCards(_columnName: string, _laneName: string): Promise<string[]> {
    return todo("kanbanCellCards");
  }

  async kanbanCellHasLoadMore(_columnName: string, _laneName: string): Promise<boolean> {
    return todo("kanbanCellHasLoadMore");
  }

  async kanbanCellLoadMore(_columnName: string, _laneName: string): Promise<void> {
    return todo("kanbanCellLoadMore");
  }

  async kanbanBoardScroll(): Promise<{ x: number; y: number }> {
    return todo("kanbanBoardScroll");
  }

  async kanbanColumnScroll(_columnName: string): Promise<{ x: number; y: number }> {
    return todo("kanbanColumnScroll");
  }

  async kanbanDragHoldNearEdge(
    _sourceName: string,
    _edge: "left" | "right" | "top" | "bottom",
    _holdMs: number
  ): Promise<void> {
    return todo("kanbanDragHoldNearEdge");
  }

  async ganttHeader(): Promise<{ count: number | null; views: string[]; hasToday: boolean; hasFullscreen: boolean }> {
    const band = this.ganttHeaderBand();
    const text = ((await band.innerText().catch(() => null)) ?? "").trim();
    const countMatch = /(\d+)\s+work items?/i.exec(text);
    const views: string[] = [];
    for (const view of ["Week", "Month", "Quarter"] as const) {
      const entry = band.getByRole("button", { name: view, exact: true });
      if ((await entry.count()) > 0 && (await entry.first().isVisible())) views.push(view);
    }
    const hasToday = (await this.page.getByTestId("gantt-today").count()) > 0;
    const hasFullscreen = (await this.page.getByTestId("gantt-fullscreen").count()) > 0;
    return { count: countMatch ? Number(countMatch[1]) : null, views, hasToday, hasFullscreen };
  }

  async ganttActiveZoom(): Promise<GanttZoom | "unknown"> {
    const fine = this.page.locator('[data-tier="fine"] [data-segment-key]');
    if ((await fine.count()) === 0) return "unknown";
    const first = (
      (await fine
        .first()
        .innerText()
        .catch(() => null)) ?? ""
    )
      .trim()
      .replace(/\s+/g, " ");
    if (/^\d+ - \d+/.test(first)) return "Month";
    if (/^(Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec)\b/i.exec(first)) return "Quarter";
    if (/^(Su|Mo|Tu|We|Th|Fr|Sa) \d+/i.test(first)) return "Week";
    return "unknown";
  }

  async ganttSetZoom(view: GanttZoom): Promise<void> {
    const entry = this.ganttHeaderBand().getByRole("button", { name: view, exact: true }).first();
    await entry.click({ timeout: WebNewDriver.WAIT_MS });
    await expect.poll(() => this.ganttActiveZoom(), { timeout: WebNewDriver.WAIT_MS }).toBe(view);
    // The zoom switch re-centers on a deferred tick; settle the offset.
    let last = await this.ganttScrollLeft();
    for (let attempt = 0; attempt < 40; attempt += 1) {
      await this.page.waitForTimeout(250);
      const now = await this.ganttScrollLeft();
      if (now === last) return;
      last = now;
    }
  }

  async ganttDayWidth(): Promise<number> {
    const zoom = await this.ganttActiveZoom();
    if (zoom === "unknown") throw new Error("[parity] cannot measure day width for an unknown zoom.");
    if (zoom === "Quarter") {
      const month = this.page.locator('[data-tier="fine"] [data-segment-key]').first();
      const box = await month.boundingBox();
      if (!box) throw new Error("[parity] quarter cell has no box.");
      const days = await month.evaluate((element) => {
        const key = element.getAttribute("data-segment-key") as string;
        const [year, month0] = key.split("-").slice(1).map(Number);
        return new Date(Date.UTC(year, (month0 ?? 0) + 1, 0)).getUTCDate();
      });
      return box.width / days;
    }
    const fine = this.page.locator('[data-tier="fine"] [data-segment-key]');
    if (zoom === "Week") {
      const box = await fine.first().boundingBox();
      if (!box) throw new Error("[parity] week cell has no box.");
      return box.width;
    }
    // Month zoom draws weeks: divide by the days the cell actually spans.
    const box = await fine.first().boundingBox();
    if (!box) throw new Error("[parity] week cell has no box.");
    const days = await fine.first().evaluate((element) => {
      const label = (element.textContent as string).trim();
      const [start, end] = label.split("-").map((part) => Number(part.trim()));
      if (start === undefined || end === undefined || Number.isNaN(start) || Number.isNaN(end)) return 7;
      return end >= start ? end - start + 1 : end + 31 - start;
    });
    return box.width / days;
  }

  async ganttWeekendTinted(): Promise<boolean> {
    const columns = this.page.getByTestId("gantt-day-columns").locator("[data-day]");
    const total = await columns.count();
    if (total === 0) return false;
    const weekend = await this.page.getByTestId("gantt-day-columns").locator('[data-weekend="true"]').count();
    return weekend > 0 && weekend < total;
  }

  /**
   * The weekday each rendered week row starts on, in strip order. Week
   * zoom lays out one column per day, so the row starts are the day cells
   * flagged as a week boundary (their weekday is the profile's start of
   * week); interior rows therefore share one label and the cut edge rows
   * may differ.
   */
  async ganttWeekRowStarts(): Promise<string[]> {
    if ((await this.ganttActiveZoom()) !== "Week") return [];
    const cells = this.page.locator('[data-testid="gantt-day-columns"] [data-day][data-week-start="true"]');
    const total = await cells.count();
    const starts: string[] = [];
    const labels = ["Su", "Mo", "Tu", "We", "Th", "Fr", "Sa"];
    for (let index = 0; index < total; index += 1) {
      const iso =
        (await cells
          .nth(index)
          .getAttribute("data-day")
          .catch(() => null)) ?? "";
      const parsed = new Date(`${iso}T00:00:00Z`).getUTCDay();
      starts.push(iso === "" || Number.isNaN(parsed) ? "" : labels[parsed]);
    }
    return starts;
  }

  async ganttClickToday(): Promise<void> {
    await this.page.getByTestId("gantt-today").first().click({ timeout: WebNewDriver.WAIT_MS });
    await expect.poll(() => this.ganttTodayVisible(), { timeout: WebNewDriver.WAIT_MS }).toBe(true);
  }

  async ganttTodayVisible(): Promise<boolean> {
    const highlight = this.page.getByTestId("gantt-today-highlight").first();
    if ((await highlight.count()) === 0) return false;
    const box = await highlight.boundingBox();
    if (!box) return false;
    const viewport = this.page.viewportSize() ?? { width: 1280, height: 720 };
    return box.x < viewport.width && box.x + box.width > 0;
  }

  async ganttTodayHighlighted(): Promise<boolean> {
    const marked = this.page.locator('[data-tier="fine"] [data-today-segment="true"]');
    return (await marked.count()) > 0;
  }

  async ganttToggleFullscreen(): Promise<void> {
    const before = await this.ganttFullscreenActive();
    await this.page.getByTestId("gantt-fullscreen").first().click({ timeout: WebNewDriver.WAIT_MS });
    await expect.poll(() => this.ganttFullscreenActive(), { timeout: WebNewDriver.WAIT_MS }).not.toBe(before);
  }

  async ganttFullscreenActive(): Promise<boolean> {
    return (await this.page.locator("#full-screen-portal #gantt-container").count()) > 0;
  }

  async ganttTimelineWidth(): Promise<number> {
    const strip = this.page.locator('[data-testid="gantt-day-columns"]').first();
    const box = await strip.boundingBox().catch(() => null);
    return box === null ? 0 : Math.round(box.width);
  }

  async ganttScrollLeft(): Promise<number> {
    return this.ganttContainer().evaluate((element) => element.scrollLeft);
  }

  async ganttScrollTo(x: number): Promise<void> {
    await this.ganttContainer().evaluate((element, target) => {
      element.scrollLeft = target;
    }, x);
    await this.page.waitForTimeout(2_000);
  }

  private async ganttRowLink(issueName: string): Promise<Locator> {
    const links = this.page.locator('#gantt-sidebar a[id^="issue-"]');
    const total = await links.count();
    for (let index = 0; index < total; index += 1) {
      const text = (
        (await links
          .nth(index)
          .innerText()
          .catch(() => null)) ?? ""
      )
        .trim()
        .replace(/\s+/g, " ");
      if (text.replace(/^[A-Z]+-\d+\s+/, "") === issueName) return links.nth(index);
    }
    throw new Error(`[parity] no timeline row titled ${JSON.stringify(issueName)}.`);
  }

  async ganttSidebarRows(): Promise<GanttSidebarRow[]> {
    const rows = this.page.getByTestId("gantt-sidebar-row");
    const total = await rows.count();
    const out: GanttSidebarRow[] = [];
    for (let index = 0; index < total; index += 1) {
      const row = rows.nth(index);
      const text = ((await row.innerText().catch(() => null)) ?? "").trim().replace(/\s+/g, " ");
      const identifier = /^([A-Z]+-\d+)\s+/.exec(text)?.[1] ?? null;
      const name = identifier === null ? text : text.replace(/^[A-Z]+-\d+\s+/, "").replace(/\s+\d+ days?$/, "");
      const duration = row.getByTestId("gantt-row-duration");
      const durationText = (await duration.count()) > 0 ? (await duration.innerText()).trim() || null : null;
      out.push({ identifier, name, duration: durationText });
    }
    return out;
  }

  async ganttOpenRowPeek(issueName: string): Promise<void> {
    const link = await this.ganttRowLink(issueName);
    await link.click({ timeout: WebNewDriver.WAIT_MS });
    await this.page.getByTestId("issue-preview").first().waitFor({ timeout: WebNewDriver.WAIT_MS });
    await expect.poll(() => this.issuePeekTitle(), { timeout: WebNewDriver.WAIT_MS }).toBe(issueName);
  }

  async ganttSidebarOrder(): Promise<string[]> {
    const rows = await this.ganttSidebarRows();
    return rows.map((row) => row.name);
  }

  async ganttDragRowBefore(_sourceName: string, _targetName: string): Promise<void> {
    return todo("ganttDragRowBefore");
  }

  async ganttAttemptRowBefore(_sourceName: string, _targetName: string): Promise<void> {
    return todo("ganttAttemptRowBefore");
  }

  private async ganttIssueIdByName(issueName: string): Promise<string> {
    const link = await this.ganttRowLink(issueName);
    const id = (await link.getAttribute("id")) as string;
    const match = /^issue-(.+)$/.exec(id);
    if (!match?.[1]) throw new Error(`[parity] unexpected timeline row id ${JSON.stringify(id)}.`);
    return match[1];
  }

  async ganttBarExists(issueName: string): Promise<boolean> {
    const issueId = await this.ganttIssueIdByName(issueName);
    const bar = this.page.locator(`button#gantt-block-${issueId}`).first();
    if ((await bar.count()) === 0) return false;
    const box = await bar.boundingBox();
    return box !== null && box.width > 0;
  }

  async ganttDragBar(_issueName: string, _dayDelta: number): Promise<void> {
    return todo("ganttDragBar");
  }

  async ganttAttemptBarMove(_issueName: string, _dayDelta: number): Promise<void> {
    return todo("ganttAttemptBarMove");
  }

  async ganttResizeBar(_issueName: string, _side: "left" | "right", _dayDelta: number): Promise<void> {
    return todo("ganttResizeBar");
  }

  async ganttResizePreview(_issueName: string, _side: "left" | "right"): Promise<string | null> {
    return todo("ganttResizePreview");
  }

  async ganttHandlesVisible(_issueName: string): Promise<boolean> {
    return todo("ganttHandlesVisible");
  }

  async ganttRowAddVisible(_issueName: string): Promise<boolean> {
    return todo("ganttRowAddVisible");
  }

  async ganttAddBlock(_issueName: string, _dayOffset: number): Promise<void> {
    return todo("ganttAddBlock");
  }

  async ganttQuickAdd(_title: string): Promise<void> {
    return todo("ganttQuickAdd");
  }

  async ganttHasQuickAdd(): Promise<boolean> {
    return todo("ganttHasQuickAdd");
  }

  async ganttBarInfo(_issueName: string): Promise<{ tinted: boolean; masked: boolean; namePinned: boolean } | null> {
    return todo("ganttBarInfo");
  }

  async ganttHoverBar(_issueName: string): Promise<void> {
    return todo("ganttHoverBar");
  }

  async ganttPreviewVisible(): Promise<boolean> {
    return todo("ganttPreviewVisible");
  }

  async ganttOpenBarPeek(_issueName: string): Promise<void> {
    return todo("ganttOpenBarPeek");
  }

  async ganttScrollArrowVisible(_issueName: string): Promise<boolean> {
    return todo("ganttScrollArrowVisible");
  }

  async ganttClickScrollArrow(_issueName: string): Promise<void> {
    return todo("ganttClickScrollArrow");
  }

  async ganttBarInView(_issueName: string): Promise<boolean> {
    return todo("ganttBarInView");
  }

  async ganttSidebarLoading(): Promise<boolean> {
    return (await this.page.locator("#gantt-sidebar .animate-pulse").count()) > 0;
  }

  async ganttLoadMoreVisible(): Promise<boolean> {
    const pulses = this.page.locator("#gantt-sidebar .animate-pulse");
    if ((await pulses.count()) === 0) return false;
    return pulses.last().isVisible();
  }

  async ganttLoadingObservedOnReload(): Promise<boolean> {
    const pattern = "**/api/**/issues**";
    await this.page.route(pattern, async (route) => {
      await new Promise((resolve) => setTimeout(resolve, 2_500));
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
    const heading = this.page.getByRole("heading", { name: "Start with your first work item." });
    if ((await heading.count()) === 0) return false;
    return heading.first().isVisible();
  }

  async ganttLoadMoreObservedOnScroll(): Promise<boolean> {
    const pattern = "**/api/**/issues**";
    await this.page.route(pattern, async (route) => {
      await new Promise((resolve) => setTimeout(resolve, 2_500));
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

  // --- Root document shell + not-found (NEWFRONT-173, rows SHELL-107/108;
  // --- implemented by NEWFRONT-288). Reads mirror the shared scenario
  // --- contract: served markers from raw markup, hydrated facts from the
  // --- live document, the illustrated surface by its semantic content.

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

  async overlayPortalsPresent(): Promise<{ contextMenu: boolean; editor: boolean }> {
    return await this.page.evaluate(() => ({
      contextMenu: document.getElementById("context-menu-portal") !== null,
      editor: document.getElementById("editor-portal") !== null,
    }));
  }

  async sessionRecorderPresent(): Promise<boolean> {
    // Absence marker: a tagged loader script plus any script fetched from
    // the recorder host. Verified absent on the oracle.
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
    const home = this.page.locator("main").locator('a[href="/"]').first();
    await home.click({ timeout: 30_000 });
    await expect.poll(() => this.page.url(), { timeout: 30_000 }).not.toBe(before);
  }

  async servedShellMarkers(path: string): Promise<ServedShellMarkers> {
    // Marker presence only: the served document is the crawler-facing
    // layer, so the scenario pins which markers it carries without
    // repeating its copy.
    const url = new URL(path, this.page.url()).toString();
    const response = await this.page.request.get(url, { timeout: 30_000 });
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
  // --- Runners overview + pods (NEWFRONT-540 shell; pods land with
  // --- NEWFRONT-542). One page serves the workspace route
  // --- (/{ws}/runners) and the project twin
  // --- (/{ws}/projects/{id}/runners): the project id in the route scopes
  // --- the reads and rewrites every link base. The area shell renders the
  // --- rail beside a content main that hosts the tab strip and the table.
  // --- The methods below cover the shell rows (RUN-001/002/003/004/005,
  // --- RUN-010/011); the pod methods stay stubs for the pods slice.

  /** The not-authorized heading the area shares with its dev-machines twin. */
  private runnersDeniedHeading(): Locator {
    return this.page.getByRole("heading", { name: "Oops! You are not authorized to view this page" });
  }

  /** Tab strip links: the content main is the innermost one, so the rail's
   *  own Overview link never matches. */
  private runnersTabLinks(): Locator {
    return this.page.locator("main:not(:has(main))").getByRole("link", { name: /^(Overview|Runs|Approvals)$/ });
  }

  async runnersOpenOverview(workspaceSlug: string, projectId?: string, subpath?: "runs" | "approvals"): Promise<void> {
    const base =
      projectId === undefined ? `/${workspaceSlug}/runners` : `/${workspaceSlug}/projects/${projectId}/runners`;
    const url = subpath === undefined ? base : `${base}/${subpath}`;
    // Arm the list waiter before navigating; a denied viewer never fetches,
    // so the gate heading is the alternate settle signal.
    const listSettled = this.page
      .waitForResponse(
        (response) => response.url().includes("/api/runners/") && response.request().method() === "GET",
        { timeout: WebNewDriver.WAIT_MS * 3 }
      )
      .then(() => true)
      .catch(() => false);
    await this.page.goto(url, { timeout: 60_000 });
    await this.runnersTabLinks()
      .first()
      .or(this.runnersDeniedHeading())
      .waitFor({ timeout: WebNewDriver.WAIT_MS * 3 });
    if (await this.runnersDeniedVisible()) return;
    if (!(await listSettled)) throw new Error("[parity] runners list fetch never settled.");
  }

  async runnersCurrentUrl(): Promise<string> {
    return this.page.url();
  }

  async runnersTabs(): Promise<RunnersTab[]> {
    const links = this.runnersTabLinks();
    await links.first().waitFor({ timeout: WebNewDriver.WAIT_MS });
    const out: RunnersTab[] = [];
    for (const link of await links.all()) {
      out.push({
        label: (await link.innerText()).trim(),
        href: await link.getAttribute("href"),
        active: (await link.getAttribute("aria-current")) !== null,
      });
    }
    return out;
  }

  async runnersOpenTab(label: string): Promise<void> {
    const suffix =
      label === "Overview" ? "/runners" : label === "Runs" ? "/runs" : label === "Approvals" ? "/approvals" : null;
    if (suffix === null) throw new Error(`[parity] unknown runners tab ${JSON.stringify(label)}.`);
    await this.runnersTabLinks()
      .filter({ hasText: new RegExp(`^${label}$`) })
      .click();
    await this.page.waitForURL((url) => new RegExp(`${suffix}/?$`).test(url.pathname), {
      timeout: WebNewDriver.WAIT_MS,
    });
    // Settle on the clicked tab being the only active marker.
    await this.page.waitForFunction(
      (want) => {
        const links = [...document.querySelectorAll("main:not(:has(main)) a")].filter((node) =>
          /^(Overview|Runs|Approvals)$/.test((node as HTMLElement).innerText.trim())
        );
        const active = links.filter((node) => node.getAttribute("aria-current") !== null);
        return active.length === 1 && (active[0] as HTMLElement).innerText.trim() === want;
      },
      label,
      { timeout: WebNewDriver.WAIT_MS }
    );
  }

  private runnersTable(): Locator {
    return this.page.getByRole("main").locator("table");
  }

  async runnersTableRows(): Promise<RunnersTableRow[]> {
    const table = this.runnersTable();
    await table.waitFor({ timeout: WebNewDriver.WAIT_MS });
    const out: RunnersTableRow[] = [];
    for (const row of await table.locator("tbody tr").all()) {
      const cells = row.locator("td");
      if ((await cells.count()) < 7) continue; // the empty-state row
      const text = await cells.allTextContents();
      out.push({
        name: (text[0] ?? "").trim(),
        pod: (text[1] ?? "").trim(),
        status: (text[2] ?? "").trim(),
        osArch: (text[3] ?? "").trim(),
        version: (text[4] ?? "").trim(),
        heartbeat: (text[5] ?? "").trim(),
      });
    }
    return out;
  }

  async runnersEmptyVisible(): Promise<boolean> {
    const table = this.runnersTable();
    await table.waitFor({ timeout: WebNewDriver.WAIT_MS });
    return (await table.getByText("No runners yet.").count()) > 0;
  }

  /** One exact row by name (runner names share long prefixes). */
  private async runnersRow(runnerName: string): Promise<Locator> {
    const table = this.runnersTable();
    await table.waitFor({ timeout: WebNewDriver.WAIT_MS });
    const rows = table.locator("tbody tr");
    for (let index = 0; index < (await rows.count()); index += 1) {
      const row = rows.nth(index);
      const first = row.locator("td").first();
      if ((await first.count()) === 0) continue;
      if ((await first.innerText()).trim() === runnerName) return row;
    }
    throw new Error(`[parity] runner row ${JSON.stringify(runnerName)} not found.`);
  }

  private async runnersHasRow(runnerName: string): Promise<boolean> {
    try {
      await this.runnersRow(runnerName);
      return true;
    } catch {
      return false;
    }
  }

  /** Named row the caller expects present: wait for a match, then resolve it. */
  private async runnersWaitRow(runnerName: string): Promise<Locator> {
    const table = this.runnersTable();
    await table.waitFor({ timeout: WebNewDriver.WAIT_MS });
    await table.locator("tbody tr", { hasText: runnerName }).first().waitFor({ timeout: WebNewDriver.WAIT_MS });
    return this.runnersRow(runnerName);
  }

  private async runnersRowStatus(runnerName: string): Promise<string> {
    const rows = await this.runnersTableRows();
    return rows.find((row) => row.name === runnerName)?.status ?? "";
  }

  async runnersRowActions(runnerName: string): Promise<RunnersRowActions> {
    const row = await this.runnersWaitRow(runnerName);
    return {
      hasDetails: (await row.getByRole("button", { name: "Details", exact: true }).count()) > 0,
      hasRevoke: (await row.getByRole("button", { name: "Revoke", exact: true }).count()) > 0,
      hasDelete: (await row.getByRole("button", { name: "Delete", exact: true }).count()) > 0,
    };
  }

  async runnersOpenDetails(runnerName: string): Promise<void> {
    const row = await this.runnersWaitRow(runnerName);
    await row.getByRole("button", { name: "Details", exact: true }).click();
    await this.page.waitForURL(/\/detail\//, { timeout: WebNewDriver.WAIT_MS });
  }

  private runnersDialog(): Locator {
    return this.page.getByRole("dialog");
  }

  /** The open confirmation with the given heading. */
  private async runnersWaitForConfirm(title: string): Promise<Locator> {
    const dialog = this.runnersDialog().filter({ has: this.page.getByRole("heading", { name: title, exact: true }) });
    await dialog.getByRole("heading", { name: title, exact: true }).waitFor({ timeout: WebNewDriver.WAIT_MS });
    return dialog;
  }

  async runnersDeleteRunner(runnerName: string, options?: { expectFailure?: boolean }): Promise<void> {
    const row = await this.runnersWaitRow(runnerName);
    await row.getByRole("button", { name: "Delete", exact: true }).click();
    const dialog = await this.runnersWaitForConfirm("Delete runner?");
    await dialog.getByRole("button", { name: "Delete", exact: true }).click();
    if (options?.expectFailure === true) {
      await expect.poll(() => this.runnersLastToast(), { timeout: WebNewDriver.WAIT_MS }).not.toBeNull();
      return;
    }
    await expect.poll(() => this.runnersHasRow(runnerName), { timeout: WebNewDriver.WAIT_MS }).toBe(false);
  }

  async runnersRevokeRunner(runnerName: string, options?: { expectFailure?: boolean }): Promise<void> {
    const row = await this.runnersWaitRow(runnerName);
    await row.getByRole("button", { name: "Revoke", exact: true }).click();
    const dialog = await this.runnersWaitForConfirm("Revoke runner?");
    await dialog.getByRole("button", { name: "Revoke", exact: true }).click();
    if (options?.expectFailure === true) {
      await expect.poll(() => this.runnersLastToast(), { timeout: WebNewDriver.WAIT_MS }).not.toBeNull();
      return;
    }
    await expect.poll(() => this.runnersRowStatus(runnerName), { timeout: WebNewDriver.WAIT_MS }).toBe("revoked");
  }

  async runnersModalCopy(): Promise<{ title: string; body: string } | null> {
    const dialogs = this.runnersDialog();
    if ((await dialogs.count()) === 0) return null;
    const dialog = dialogs.first();
    const heading = dialog.getByRole("heading").first();
    if ((await heading.count()) === 0) return null;
    if (!(await heading.isVisible())) return null;
    const title = (await heading.innerText()).trim();
    // The consequence copy is the description directly under the heading.
    // Read it from the heading element so the kit's heading level and
    // wrapper nesting do not matter.
    const body = await heading.evaluate(descriptionAfterHeading).catch(() => "");
    return { title, body };
  }

  async runnersCancelModal(): Promise<void> {
    await this.runnersDialog().first().getByRole("button", { name: "Cancel", exact: true }).click();
  }

  async runnersStallMutations(delayMs: number): Promise<void> {
    await this.page.route("**/api/runners/**", async (route) => {
      if (route.request().method() === "GET") {
        await route.continue();
        return;
      }
      // Hold the mutation so in-flight states stay observable; the polls
      // pass straight through.
      await new Promise((resolve) => setTimeout(resolve, delayMs));
      await route.continue().catch(() => undefined);
    });
  }

  async runnersFailNextMutation(): Promise<void> {
    let failed = false;
    await this.page.route("**/api/runners/**", async (route) => {
      if (route.request().method() !== "GET" && !failed) {
        failed = true;
        await route.abort("failed");
        return;
      }
      await route.continue().catch(() => undefined);
    });
  }

  async runnersReleaseMutationShaping(): Promise<void> {
    await this.page.unroute("**/api/runners/**");
  }

  private runnersRailRoot(): Locator {
    // The workspace shell already renders a `<main>` (AppShell) and the
    // area nests its own content main, so a bare `main` resolves to two
    // rows and `xpath=../aside` also picks up the workspace sidebar. Anchor
    // on the area's named content main so the sibling is the runners rail.
    return this.page.getByRole("main", { name: "Runners content" }).locator("xpath=../aside");
  }

  async runnersRail(): Promise<RunnersRail> {
    const rail = this.runnersRailRoot();
    await rail.waitFor({ timeout: WebNewDriver.WAIT_MS });
    const header = (await rail.locator("div").first().innerText()).trim();
    const overview = rail.getByRole("link", { name: "Overview", exact: true });
    const contacts: RunnersRailContact[] = [];
    for (const link of await rail.getByRole("link").all()) {
      const href = await link.getAttribute("href");
      if (href === null || !href.includes("/chat/")) continue;
      contacts.push({
        name: (await link.innerText()).trim(),
        href,
        // The contact carries its glyph and a status dot.
        hasDot: (await link.locator("svg").count()) >= 2,
      });
    }
    return {
      header,
      overviewHref: (await overview.count()) > 0 ? await overview.first().getAttribute("href") : null,
      contacts,
      emptyVisible: (await rail.getByText("No runners connected").count()) > 0,
    };
  }

  // Pod tiles, the create/edit modals and the delete confirm (NEWFRONT-542,
  // RUN-012–015). Tiles are the labelled filter buttons; each sits in a
  // container with its overflow menu. The dialogs are the area's own; the
  // project picker and the default switch are kit controls.

  async runnersPodTiles(): Promise<RunnersPodTile[]> {
    const buttons = this.page.getByRole("main").locator('button[aria-label^="Filter runners by pod "]');
    const out: RunnersPodTile[] = [];
    for (const button of await buttons.all()) {
      const label = (await button.getAttribute("aria-label")) ?? "";
      const name = label.replace(/^Filter runners by pod /, "");
      const container = button.locator("xpath=..");
      const count = container.getByText(/\d+ runner\(s\)/);
      out.push({
        name,
        isDefault: (await container.getByText("default", { exact: true }).count()) > 0,
        runnerCount: (await count.count()) > 0 ? (await count.first().innerText()).trim() : "",
      });
    }
    return out;
  }

  private runnersPodTile(podName: string): Locator {
    return this.page.getByRole("main").getByRole("button", { name: `Filter runners by pod ${podName}`, exact: true });
  }

  async runnersSelectPod(podName: string): Promise<void> {
    await this.runnersPodTile(podName).click();
  }

  async runnersFilterText(): Promise<string | null> {
    const clear = this.page.getByRole("main").getByRole("button", { name: "Clear filter", exact: true });
    if ((await clear.count()) === 0) return null;
    const line = clear.locator("xpath=../span").first();
    return (await line.count()) > 0 ? (await line.innerText()).trim() : null;
  }

  async runnersClearPodFilter(): Promise<void> {
    const clear = this.page.getByRole("main").getByRole("button", { name: "Clear filter", exact: true });
    await clear.click();
    await clear.waitFor({ state: "hidden", timeout: WebNewDriver.WAIT_MS });
  }

  /** The open dialog whose heading carries this text. */
  private async runnersPodModal(title: string): Promise<Locator> {
    const dialog = this.runnersDialog().filter({ has: this.page.getByRole("heading", { name: title, exact: true }) });
    await dialog.getByRole("heading", { name: title, exact: true }).waitFor({ timeout: WebNewDriver.WAIT_MS });
    return dialog;
  }

  /** A pod dialog is closed once its form's submit button is gone. */
  private async runnersWaitPodModalClosed(dialog: Locator, timeoutMs: number): Promise<void> {
    await dialog.locator('button[type="submit"]').first().waitFor({ state: "hidden", timeout: timeoutMs });
  }

  async runnersOpenCreatePod(): Promise<void> {
    await this.page.getByRole("main").getByRole("button", { name: "Create new pod", exact: true }).click();
    await this.runnersPodModal("Create new pod");
  }

  async runnersPodFormErrors(): Promise<string[]> {
    const dialog = this.runnersDialog().first();
    if ((await dialog.count()) === 0) return [];
    // Field messages render as alert elements inside the form.
    const texts = await dialog.locator('form [role="alert"]').allTextContents();
    return texts.map((text) => text.trim()).filter((text) => text.length > 0);
  }

  async runnersCreatePod(
    input: { projectName: string; name: string; description?: string },
    options?: { expectFailure?: boolean }
  ): Promise<void> {
    const dialog = await this.runnersPodModal("Create new pod");
    await dialog.getByRole("button", { name: "Select a project", exact: true }).click();
    await this.page.getByRole("option", { name: input.projectName, exact: true }).click();
    await dialog.getByLabel("Name", { exact: true }).fill(input.name);
    if (input.description !== undefined) {
      await dialog.getByLabel("Description (optional)", { exact: true }).fill(input.description);
    }
    await dialog.locator('button[type="submit"]').click();
    if (options?.expectFailure === true) {
      await expect.poll(() => this.runnersLastToast(), { timeout: WebNewDriver.WAIT_MS }).not.toBeNull();
      return;
    }
    await this.runnersWaitPodModalClosed(dialog, WebNewDriver.WAIT_MS);
  }

  async runnersPodFilterIsClientSide(podName: string): Promise<boolean> {
    let fetches = 0;
    const handler = async (route: Route): Promise<void> => {
      if (route.request().method() === "GET") fetches += 1;
      await route.continue();
    };
    await this.page.route("**/api/runners/?*", handler);
    try {
      await this.runnersSelectPod(podName);
      await this.page.waitForTimeout(1_500);
      await this.runnersSelectPod(podName);
      await this.page.waitForTimeout(1_500);
    } finally {
      await this.page.unroute("**/api/runners/?*", handler);
    }
    return fetches === 0;
  }

  async runnersSubmitPodForm(): Promise<void> {
    await this.runnersDialog().first().locator('button[type="submit"]').click();
  }

  async runnersCreatePodSubmitting(): Promise<boolean> {
    const dialog = this.runnersDialog().first();
    if ((await dialog.count()) === 0) return false;
    const submit = dialog.locator('button[type="submit"]');
    if ((await submit.count()) === 0) return false;
    return await submit.first().isDisabled();
  }

  async runnersPodCancelEnabled(): Promise<boolean> {
    const dialog = this.runnersDialog().first();
    if ((await dialog.count()) === 0) return false;
    const cancel = dialog.getByRole("button", { name: "Cancel", exact: true });
    if ((await cancel.count()) === 0) return false;
    return await cancel.first().isEnabled();
  }

  async runnersCreatePodForm(): Promise<{ project: string; name: string; description: string }> {
    const dialog = await this.runnersPodModal("Create new pod");
    const picker = dialog.locator("form").getByRole("button").first();
    const lines = (await picker.innerText()).split("\n").map((line) => line.trim());
    return {
      project: lines[0]!,
      name: await dialog.getByLabel("Name", { exact: true }).inputValue(),
      description: await dialog.getByLabel("Description (optional)", { exact: true }).inputValue(),
    };
  }

  async runnersPodModalOpen(): Promise<boolean> {
    const dialogs = this.runnersDialog();
    if ((await dialogs.count()) === 0) return false;
    const submit = dialogs.first().locator('button[type="submit"]');
    if ((await submit.count()) > 0) return await submit.first().isVisible();
    const action = dialogs.first().locator("button").first();
    if ((await action.count()) === 0) return false;
    return await action.isVisible();
  }

  async runnersPodsError(): Promise<string | null> {
    const message = this.page.getByRole("main").getByText("Failed to load pods");
    if ((await message.count()) === 0) return null;
    return (await message.first().innerText()).trim();
  }

  /** Open one pod tile's overflow menu. */
  private async runnersOpenPodMenu(podName: string): Promise<void> {
    await this.runnersPodTile(podName)
      .locator("xpath=..")
      .getByRole("button", { name: `Pod actions for ${podName}`, exact: true })
      .first()
      .click();
    await this.page.getByRole("menuitem").first().waitFor({ state: "visible", timeout: WebNewDriver.WAIT_MS });
  }

  async runnersOpenEditPod(podName: string): Promise<void> {
    await this.runnersOpenPodMenu(podName);
    await this.page.getByRole("menuitem", { name: "Edit", exact: true }).click();
    await this.runnersPodModal("Edit pod");
  }

  async runnersEditPodForm(): Promise<RunnersEditPodForm> {
    const dialog = await this.runnersPodModal("Edit pod");
    const toggle = dialog.getByRole("switch", { name: "Project default" });
    return {
      name: await dialog.getByLabel("Name", { exact: true }).inputValue(),
      description: await dialog.getByLabel("Description (optional)", { exact: true }).inputValue(),
      isDefault: (await toggle.getAttribute("aria-checked")) === "true",
      defaultDisabled: (await toggle.isDisabled()) || (await toggle.getAttribute("aria-disabled")) === "true",
    };
  }

  /** Fill the open edit form; shared by the save helpers. */
  private async runnersFillPodEdit(input: {
    name?: string;
    description?: string;
    makeDefault?: boolean;
  }): Promise<Locator> {
    const dialog = await this.runnersPodModal("Edit pod");
    if (input.name !== undefined) {
      await dialog.getByLabel("Name", { exact: true }).fill(input.name);
    }
    if (input.description !== undefined) {
      await dialog.getByLabel("Description (optional)", { exact: true }).fill(input.description);
    }
    if (input.makeDefault === true) {
      const toggle = dialog.getByRole("switch", { name: "Project default" });
      if ((await toggle.getAttribute("aria-checked")) !== "true") await toggle.click();
    }
    return dialog;
  }

  async runnersSavePodEdit(input: { name?: string; description?: string; makeDefault?: boolean }): Promise<void> {
    const dialog = await this.runnersFillPodEdit(input);
    await dialog.locator('button[type="submit"]').click();
    await this.runnersWaitPodModalClosed(dialog, WebNewDriver.WAIT_MS);
  }

  async runnersUnchangedEditSkipsSave(): Promise<boolean> {
    const dialog = await this.runnersPodModal("Edit pod");
    let patches = 0;
    const pattern = "**/api/runners/pods/**";
    await this.page.route(pattern, async (route) => {
      if (route.request().method() === "PATCH") patches += 1;
      await route.continue();
    });
    try {
      await dialog.locator('button[type="submit"]').click();
      await this.runnersWaitPodModalClosed(dialog, WebNewDriver.WAIT_MS);
    } finally {
      await this.page.unroute(pattern);
    }
    return patches === 0;
  }

  async runnersSavePodEditCapturing(input: {
    name?: string;
    description?: string;
    makeDefault?: boolean;
  }): Promise<Record<string, unknown>[]> {
    const bodies: Record<string, unknown>[] = [];
    const pattern = "**/api/runners/pods/**";
    await this.page.route(pattern, async (route) => {
      if (route.request().method() === "PATCH") {
        try {
          const raw = route.request().postData();
          bodies.push(raw === null ? {} : (JSON.parse(raw) as Record<string, unknown>));
        } catch {
          bodies.push({});
        }
      }
      await route.continue();
    });
    try {
      const dialog = await this.runnersFillPodEdit(input);
      await dialog.locator('button[type="submit"]').click();
      await this.runnersWaitPodModalClosed(dialog, WebNewDriver.WAIT_MS);
    } finally {
      await this.page.unroute(pattern);
    }
    return bodies;
  }

  async runnersDeletePod(podName: string, options?: { expectFailure?: boolean }): Promise<void> {
    await this.runnersOpenPodMenu(podName);
    await this.page.getByRole("menuitem", { name: "Delete", exact: true }).click();
    const dialog = await this.runnersWaitForConfirm("Delete pod?");
    await dialog.getByRole("button", { name: "Delete", exact: true }).click();
    if (options?.expectFailure === true) {
      await expect.poll(() => this.runnersLastToast(), { timeout: WebNewDriver.WAIT_MS }).not.toBeNull();
      return;
    }
    await this.runnersWaitPodModalClosed(dialog, WebNewDriver.WAIT_MS);
    const tile = this.runnersPodTile(podName);
    await tile.waitFor({ state: "hidden", timeout: WebNewDriver.WAIT_MS });
  }

  async runnersLastToast(): Promise<string | null> {
    const titles = this.page.getByText("Error!", { exact: true });
    if ((await titles.count()) === 0) return null;
    // Climb from the title to the toast root carrying its message.
    const text = await titles.first().evaluate((element) => {
      let node: HTMLElement | null = element instanceof HTMLElement ? element : element.parentElement;
      while (node !== null && node !== document.body) {
        const candidate = (node.innerText ?? "").trim();
        if (candidate.length > "Error!".length + 8) return candidate;
        node = node.parentElement;
      }
      return (element.textContent ?? "").trim();
    });
    return text;
  }

  async runnersPageTitle(): Promise<string> {
    return this.page.title();
  }

  async runnersSectionHeadings(): Promise<string[]> {
    const main = this.page.getByRole("main");
    const out: string[] = [];
    for (const title of ["Add runner", "Pods", "Runners"]) {
      if ((await main.getByText(title, { exact: true }).count()) > 0) out.push(title);
    }
    return out;
  }

  async runnersBreadcrumbLeaf(): Promise<string | null> {
    // The project chrome's area link carries a trailing slash, which also
    // separates it from the tab and rail links pointing at the same route.
    const crumb = this.page.locator('a[href$="/runners/"]');
    await crumb
      .first()
      .waitFor({ state: "visible", timeout: WebNewDriver.WAIT_MS })
      .catch(() => undefined);
    if ((await crumb.count()) === 0) return null;
    const lines = (await crumb.first().innerText()).split("\n").map((line) => line.trim());
    const leaf = lines.find((line) => line !== "");
    return leaf === undefined ? null : leaf;
  }

  async runnersDeniedVisible(): Promise<boolean> {
    return (await this.runnersDeniedHeading().count()) > 0;
  }

  async runnersChromePresent(): Promise<boolean> {
    if ((await this.page.getByRole("button", { name: "New work item" }).count()) > 0) return true;
    return (await this.page.getByRole("complementary").count()) > 0;
  }

  async runnersFailPodsLoad(): Promise<void> {
    await this.page.route("**/api/runners/pods**", async (route) => {
      if (route.request().method() === "GET") {
        await route.fulfill({
          status: 500,
          contentType: "application/json",
          body: '{"error": "parity pods outage"}',
        });
      } else await route.continue();
    });
  }

  async runnersReleasePodsFailure(): Promise<void> {
    await this.page.unroute("**/api/runners/pods**");
  }

  async runnersWaitPodDeleteWorking(): Promise<void> {
    const dialog = await this.runnersWaitForConfirm("Delete pod?");
    const remove = dialog.getByRole("button", { name: "Delete", exact: true });
    await expect.poll(() => remove.isDisabled(), { timeout: WebNewDriver.WAIT_MS }).toBe(true);
  }

  async runnersWaitRunnerDeleteWorking(): Promise<void> {
    const dialog = await this.runnersWaitForConfirm("Delete runner?");
    const remove = dialog.getByRole("button", { name: "Delete", exact: true });
    await expect.poll(() => remove.isDisabled(), { timeout: WebNewDriver.WAIT_MS }).toBe(true);
  }

  async runnersWaitRunnerRevokeWorking(): Promise<void> {
    const dialog = await this.runnersWaitForConfirm("Revoke runner?");
    const revoke = dialog.getByRole("button", { name: "Revoke", exact: true });
    await expect.poll(() => revoke.isDisabled(), { timeout: WebNewDriver.WAIT_MS }).toBe(true);
  }

  // --- Desktop-only chat + agent runtime (NEWFRONT-182). Throwing stubs
  // --- per the shared driver contract; the runners area fills these in.

  async desktopRuntimeOpenRunners(_workspaceSlug: string): Promise<void> {
    return todo("desktopRuntimeOpenRunners");
  }
  async desktopRuntimeRailSectionHeaders(): Promise<string[]> {
    return todo("desktopRuntimeRailSectionHeaders");
  }
  async desktopRuntimeRailChatLinks(): Promise<{ name: string; href: string }[]> {
    return todo("desktopRuntimeRailChatLinks");
  }
  async desktopRuntimeOpenChat(_workspaceSlug: string, _runnerId: string, _sessionId?: string): Promise<void> {
    return todo("desktopRuntimeOpenChat");
  }
  async desktopRuntimeApprovalPromptVisible(): Promise<boolean> {
    return todo("desktopRuntimeApprovalPromptVisible");
  }
  async desktopRuntimeApprovalModeVisible(): Promise<boolean> {
    return todo("desktopRuntimeApprovalModeVisible");
  }
  async desktopRuntimeRuntimeBannerVisible(): Promise<boolean> {
    return todo("desktopRuntimeRuntimeBannerVisible");
  }
  async desktopRuntimeIsTauriPresent(): Promise<boolean> {
    return todo("desktopRuntimeIsTauriPresent");
  }
  async desktopRuntimeChatBubbles(): Promise<{ role: string; text: string }[]> {
    return todo("desktopRuntimeChatBubbles");
  }
  async desktopRuntimeStorageKeys(): Promise<{ local: string[]; session: string[] }> {
    return todo("desktopRuntimeStorageKeys");
  }
  async desktopRuntimeStartRequestSpy(): Promise<void> {
    return todo("desktopRuntimeStartRequestSpy");
  }
  async desktopRuntimeSpyUrls(): Promise<string[]> {
    return todo("desktopRuntimeSpyUrls");
  }
  async desktopRuntimeStopRequestSpy(): Promise<void> {
    return todo("desktopRuntimeStopRequestSpy");
  }

  // --- Scheduler catalog + definitions (NEWFRONT-184). Throwing stubs
  // --- per the shared driver contract; the agents area fills these in.

  async schedulerOpenCatalog(_workspaceSlug: string): Promise<void> {
    return todo("schedulerOpenCatalog");
  }
  async schedulerCatalogRows(): Promise<SchedulerCatalogRow[]> {
    return todo("schedulerCatalogRows");
  }
  async schedulerCatalogEmptyVisible(): Promise<boolean> {
    return todo("schedulerCatalogEmptyVisible");
  }
  async schedulerPageTitle(): Promise<string> {
    return todo("schedulerPageTitle");
  }
  async schedulerCreateVisible(): Promise<boolean> {
    return todo("schedulerCreateVisible");
  }
  async schedulerRowActions(_handle: string): Promise<string[]> {
    return todo("schedulerRowActions");
  }
  async schedulerOpenCreate(): Promise<void> {
    return todo("schedulerOpenCreate");
  }
  async schedulerFillDefinition(_input: {
    name?: string;
    handle?: string;
    description?: string;
    prompt?: string;
    color?: string;
  }): Promise<void> {
    return todo("schedulerFillDefinition");
  }
  async schedulerSetDefinitionEnabled(_enabled: boolean): Promise<void> {
    return todo("schedulerSetDefinitionEnabled");
  }
  async schedulerDefinitionValues(): Promise<SchedulerDefinitionValues> {
    return todo("schedulerDefinitionValues");
  }
  async schedulerDefinitionHandleLocked(): Promise<boolean> {
    return todo("schedulerDefinitionHandleLocked");
  }
  async schedulerSubmitDefinition(): Promise<void> {
    return todo("schedulerSubmitDefinition");
  }
  async schedulerDefinitionOpen(): Promise<boolean> {
    return todo("schedulerDefinitionOpen");
  }
  async schedulerDefinitionErrors(): Promise<string[]> {
    return todo("schedulerDefinitionErrors");
  }
  async schedulerCloseDefinition(): Promise<void> {
    return todo("schedulerCloseDefinition");
  }
  async schedulerOpenEdit(_handle: string): Promise<void> {
    return todo("schedulerOpenEdit");
  }
  async schedulerOpenDelete(_handle: string): Promise<void> {
    return todo("schedulerOpenDelete");
  }
  async schedulerDeleteDialogText(): Promise<string> {
    return todo("schedulerDeleteDialogText");
  }
  async schedulerConfirmDelete(): Promise<void> {
    return todo("schedulerConfirmDelete");
  }
  async schedulerDeleteOpen(): Promise<boolean> {
    return todo("schedulerDeleteOpen");
  }
  async schedulerCancelDelete(): Promise<void> {
    return todo("schedulerCancelDelete");
  }
  async schedulerOpenProjectSchedulers(_workspaceSlug: string, _projectId: string): Promise<void> {
    return todo("schedulerOpenProjectSchedulers");
  }
  async schedulerOpenProjectCreate(): Promise<void> {
    return todo("schedulerOpenProjectCreate");
  }
  async schedulerProjectCreateSubmit(): Promise<void> {
    return todo("schedulerProjectCreateSubmit");
  }
  async schedulerProjectCreateFillName(_name: string): Promise<void> {
    return todo("schedulerProjectCreateFillName");
  }
  async schedulerProjectCreateHandleValue(): Promise<string> {
    return todo("schedulerProjectCreateHandleValue");
  }
  async schedulerProjectCreateFillHandle(_handle: string): Promise<void> {
    return todo("schedulerProjectCreateFillHandle");
  }
  async schedulerProjectCreateErrors(): Promise<string[]> {
    return todo("schedulerProjectCreateErrors");
  }
  async schedulerCloseProjectCreate(): Promise<void> {
    return todo("schedulerCloseProjectCreate");
  }
  async schedulerOpenInstall(_handle: string): Promise<void> {
    return todo("schedulerOpenInstall");
  }
  async schedulerInstallPickerOptions(): Promise<SchedulerInstallOption[]> {
    return todo("schedulerInstallPickerOptions");
  }
  async schedulerInstallSearch(_query: string): Promise<void> {
    return todo("schedulerInstallSearch");
  }
  async schedulerInstallToggleSelectAll(): Promise<void> {
    return todo("schedulerInstallToggleSelectAll");
  }
  async schedulerInstallToggleProject(_name: string): Promise<void> {
    return todo("schedulerInstallToggleProject");
  }
  async schedulerInstallSelectedSummary(): Promise<string> {
    return todo("schedulerInstallSelectedSummary");
  }
  async schedulerInstallSubmit(): Promise<void> {
    return todo("schedulerInstallSubmit");
  }
  async schedulerInstallOpen(): Promise<boolean> {
    return todo("schedulerInstallOpen");
  }
  async schedulerCloseInstall(): Promise<void> {
    return todo("schedulerCloseInstall");
  }
  async schedulerVisibleToasts(): Promise<{ title: string; message: string }[]> {
    return todo("schedulerVisibleToasts");
  }
  async schedulerOpenPrompts(_workspaceSlug: string): Promise<void> {
    return todo("schedulerOpenPrompts");
  }
  async schedulerNotAuthorizedVisible(): Promise<boolean> {
    return todo("schedulerNotAuthorizedVisible");
  }
  async schedulerWorkspaceNotFoundVisible(): Promise<boolean> {
    return todo("schedulerWorkspaceNotFoundVisible");
  }
  async schedulerShellCount(): Promise<number> {
    return todo("schedulerShellCount");
  }

  // --- Dev machines, runner detail, agent activity (NEWFRONT-183, RUN-037–043) ---
  async devMachinesOpen(_workspaceSlug: string): Promise<void> {
    return todo("devMachinesOpen");
  }
  async devMachinesRows(): Promise<DevMachineRow[]> {
    return todo("devMachinesRows");
  }
  async devMachinesRowByName(_name: string): Promise<DevMachineRow | null> {
    return todo("devMachinesRowByName");
  }
  async devMachinesEmptyVisible(): Promise<boolean> {
    return todo("devMachinesEmptyVisible");
  }
  async devMachinesLoadingVisible(): Promise<boolean> {
    return todo("devMachinesLoadingVisible");
  }
  async devMachinesErrorVisible(): Promise<boolean> {
    return todo("devMachinesErrorVisible");
  }
  async devMachinesStubListOnce(_rows: unknown[]): Promise<void> {
    return todo("devMachinesStubListOnce");
  }
  async devMachinesFailListOnce(): Promise<void> {
    return todo("devMachinesFailListOnce");
  }
  async devMachinesDelayListOnce(_ms: number): Promise<void> {
    return todo("devMachinesDelayListOnce");
  }
  async devMachinesListPollCount(_windowMs: number): Promise<number> {
    return todo("devMachinesListPollCount");
  }
  async devMachinesOpenRotate(_name: string): Promise<void> {
    return todo("devMachinesOpenRotate");
  }
  async devMachinesOpenRevoke(_name: string): Promise<void> {
    return todo("devMachinesOpenRevoke");
  }
  async devMachinesOpenDelete(_name: string): Promise<void> {
    return todo("devMachinesOpenDelete");
  }
  async devMachinesModal(): Promise<DevMachineModal | null> {
    return todo("devMachinesModal");
  }
  async devMachinesModalVisible(): Promise<boolean> {
    return todo("devMachinesModalVisible");
  }
  async devMachinesModalConfirm(): Promise<void> {
    return todo("devMachinesModalConfirm");
  }
  async devMachinesModalCancel(): Promise<void> {
    return todo("devMachinesModalCancel");
  }
  async devMachinesModalPressEscape(): Promise<void> {
    return todo("devMachinesModalPressEscape");
  }
  async devMachinesDelayActionOnce(_ms: number): Promise<void> {
    return todo("devMachinesDelayActionOnce");
  }
  async devMachinesFailActionOnce(): Promise<void> {
    return todo("devMachinesFailActionOnce");
  }
  async devMachinesLastToast(): Promise<string | null> {
    return todo("devMachinesLastToast");
  }
  async devMachinesDeleteSpyStart(): Promise<void> {
    return todo("devMachinesDeleteSpyStart");
  }
  async devMachinesDeleteSpyUrls(): Promise<string[]> {
    return todo("devMachinesDeleteSpyUrls");
  }
  async devMachinesDeleteSpyStop(): Promise<void> {
    return todo("devMachinesDeleteSpyStop");
  }
  async devMachinesInstallCards(): Promise<DevMachineInstallCard[]> {
    return todo("devMachinesInstallCards");
  }
  async devMachinesInstallCopy(_label: string): Promise<void> {
    return todo("devMachinesInstallCopy");
  }
  async devMachinesInstallCopyState(_label: string): Promise<string | null> {
    return todo("devMachinesInstallCopyState");
  }
  async devMachinesInstallPrereq(): Promise<string | null> {
    return todo("devMachinesInstallPrereq");
  }
  async devMachinesInstallBreakClipboard(): Promise<void> {
    return todo("devMachinesInstallBreakClipboard");
  }
  async devMachinesReadClipboard(): Promise<string> {
    return todo("devMachinesReadClipboard");
  }
  async runnerDetailOpen(_workspaceSlug: string, _runnerId: string, _projectId?: string): Promise<void> {
    return todo("runnerDetailOpen");
  }
  async runnerDetailState(): Promise<"loaded" | "loading" | "error"> {
    return todo("runnerDetailState");
  }
  async runnerDetailHeader(): Promise<{ name: string; status: string } | null> {
    return todo("runnerDetailHeader");
  }
  async runnerDetailMeta(): Promise<{ label: string; value: string }[]> {
    return todo("runnerDetailMeta");
  }
  async runnerDetailBackHref(): Promise<string | null> {
    return todo("runnerDetailBackHref");
  }
  async runnerDetailOpenChat(): Promise<void> {
    return todo("runnerDetailOpenChat");
  }
  async runnerDetailPollCount(_runnerId: string, _windowMs: number): Promise<number> {
    return todo("runnerDetailPollCount");
  }
  async runnerDetailDelayOnce(_ms: number): Promise<void> {
    return todo("runnerDetailDelayOnce");
  }
  async runnerDetailFailOnce(): Promise<void> {
    return todo("runnerDetailFailOnce");
  }
  async runnerActivityBadge(): Promise<string | null> {
    return todo("runnerActivityBadge");
  }
  async runnerActivityTelemetry(): Promise<{ label: string; value: string }[]> {
    return todo("runnerActivityTelemetry");
  }
  async runnerActivityAgingObserved(): Promise<boolean> {
    return todo("runnerActivityAgingObserved");
  }

  async schedulerOpenProjectList(_workspaceSlug: string, _projectId: string): Promise<void> {
    return todo("schedulerOpenProjectList");
  }
  async schedulerProjectRows(): Promise<SchedulerProjectRow[]> {
    return todo("schedulerProjectRows");
  }
  async schedulerProjectEmptyVisible(): Promise<boolean> {
    return todo("schedulerProjectEmptyVisible");
  }
  async schedulerProjectNewVisible(): Promise<boolean> {
    return todo("schedulerProjectNewVisible");
  }
  async schedulerProjectRowOpen(_handle: string): Promise<void> {
    return todo("schedulerProjectRowOpen");
  }
  async schedulerProjectToggleState(_handle: string): Promise<{ checked: boolean; disabled: boolean }> {
    return todo("schedulerProjectToggleState");
  }
  async schedulerProjectToggle(_handle: string): Promise<void> {
    return todo("schedulerProjectToggle");
  }
  async schedulerProjectToggleFlightGated(_handle: string): Promise<boolean> {
    return todo("schedulerProjectToggleFlightGated");
  }
  async schedulerProjectOpenEdit(_handle: string): Promise<void> {
    return todo("schedulerProjectOpenEdit");
  }
  async schedulerProjectOpenUninstall(_handle: string): Promise<void> {
    return todo("schedulerProjectOpenUninstall");
  }
  async schedulerUninstallDialogText(): Promise<string> {
    return todo("schedulerUninstallDialogText");
  }
  async schedulerConfirmUninstall(): Promise<void> {
    return todo("schedulerConfirmUninstall");
  }
  async schedulerUninstallOpen(): Promise<boolean> {
    return todo("schedulerUninstallOpen");
  }
  async schedulerCancelUninstall(): Promise<void> {
    return todo("schedulerCancelUninstall");
  }
  async schedulerOpenProjectInstall(): Promise<void> {
    return todo("schedulerOpenProjectInstall");
  }
  async schedulerProjectInstallMode(): Promise<"install" | "create" | "dead-end"> {
    return todo("schedulerProjectInstallMode");
  }
  async schedulerProjectInstallDeadEndText(): Promise<string> {
    return todo("schedulerProjectInstallDeadEndText");
  }
  async schedulerProjectInstallTabs(): Promise<string[]> {
    return todo("schedulerProjectInstallTabs");
  }
  async schedulerProjectInstallSelectTab(_tab: "Install existing" | "Create new"): Promise<void> {
    return todo("schedulerProjectInstallSelectTab");
  }
  async schedulerProjectInstallOptions(): Promise<SchedulerProjectInstallOption[]> {
    return todo("schedulerProjectInstallOptions");
  }
  async schedulerProjectInstallSelect(_handle: string): Promise<void> {
    return todo("schedulerProjectInstallSelect");
  }
  async schedulerProjectInstallFillSchedule(_input: {
    dtstart?: string;
    tzid?: string;
    rrule?: string;
    extraContext?: string;
  }): Promise<void> {
    return todo("schedulerProjectInstallFillSchedule");
  }
  async schedulerProjectInstallScheduleValues(): Promise<SchedulerScheduleValues> {
    return todo("schedulerProjectInstallScheduleValues");
  }
  async schedulerProjectInstallHumanizer(): Promise<string> {
    return todo("schedulerProjectInstallHumanizer");
  }
  async schedulerProjectInstallSetEnabled(_enabled: boolean): Promise<void> {
    return todo("schedulerProjectInstallSetEnabled");
  }
  async schedulerProjectInstallErrors(): Promise<string[]> {
    return todo("schedulerProjectInstallErrors");
  }
  async schedulerProjectInstallSubmit(): Promise<void> {
    return todo("schedulerProjectInstallSubmit");
  }
  async schedulerProjectInstallSubmitDisabled(): Promise<boolean> {
    return todo("schedulerProjectInstallSubmitDisabled");
  }
  async schedulerProjectInstallOpen(): Promise<boolean> {
    return todo("schedulerProjectInstallOpen");
  }
  async schedulerCloseProjectInstall(): Promise<void> {
    return todo("schedulerCloseProjectInstall");
  }
  async schedulerProjectCreateFillDescription(_description: string): Promise<void> {
    return todo("schedulerProjectCreateFillDescription");
  }
  async schedulerProjectCreateFillPrompt(_prompt: string): Promise<void> {
    return todo("schedulerProjectCreateFillPrompt");
  }
  async schedulerProjectEditValues(): Promise<SchedulerBindingValues> {
    return todo("schedulerProjectEditValues");
  }
  async schedulerProjectEditFill(_input: {
    dtstart?: string;
    tzid?: string;
    rrule?: string;
    extraContext?: string;
  }): Promise<void> {
    return todo("schedulerProjectEditFill");
  }
  async schedulerProjectEditSetEnabled(_enabled: boolean): Promise<void> {
    return todo("schedulerProjectEditSetEnabled");
  }
  async schedulerProjectEditHumanizer(): Promise<string> {
    return todo("schedulerProjectEditHumanizer");
  }
  async schedulerProjectEditErrors(): Promise<string[]> {
    return todo("schedulerProjectEditErrors");
  }
  async schedulerProjectEditSubmit(): Promise<void> {
    return todo("schedulerProjectEditSubmit");
  }
  async schedulerProjectEditOpen(): Promise<boolean> {
    return todo("schedulerProjectEditOpen");
  }
  async schedulerCloseProjectEdit(): Promise<void> {
    return todo("schedulerCloseProjectEdit");
  }
  async schedulerOutcomeState(): Promise<{ options: { label: string; checked: boolean }[]; help: string }> {
    return todo("schedulerOutcomeState");
  }
  async schedulerOutcomeSelect(_label: string): Promise<void> {
    return todo("schedulerOutcomeSelect");
  }
  async schedulerPodOptions(): Promise<{ value: string; label: string; selected: boolean }[]> {
    return todo("schedulerPodOptions");
  }
  async schedulerPodSelect(_value: string): Promise<void> {
    return todo("schedulerPodSelect");
  }
  async schedulerPodDisabled(): Promise<boolean> {
    return todo("schedulerPodDisabled");
  }
  async schedulerPodLoadingObserved(): Promise<boolean> {
    return todo("schedulerPodLoadingObserved");
  }
  async schedulerOpenProjectBinding(_workspaceSlug: string, _projectId: string, _bindingId: string): Promise<void> {
    return todo("schedulerOpenProjectBinding");
  }
  async schedulerBindingHeader(): Promise<SchedulerBindingHeader> {
    return todo("schedulerBindingHeader");
  }
  async schedulerBindingConfig(): Promise<{ label: string; value: string }[]> {
    return todo("schedulerBindingConfig");
  }
  async schedulerBindingScheduleTitle(): Promise<string> {
    return todo("schedulerBindingScheduleTitle");
  }
  async schedulerBindingLastError(): Promise<string | null> {
    return todo("schedulerBindingLastError");
  }
  async schedulerBindingExtraContext(): Promise<string | null> {
    return todo("schedulerBindingExtraContext");
  }
  async schedulerBindingPromptState(): Promise<{ toggleVisible: boolean; revealed: boolean }> {
    return todo("schedulerBindingPromptState");
  }
  async schedulerBindingPromptToggle(): Promise<void> {
    return todo("schedulerBindingPromptToggle");
  }
  async schedulerBindingPromptText(): Promise<string> {
    return todo("schedulerBindingPromptText");
  }
  async schedulerBindingRuns(): Promise<SchedulerBindingRunRow[]> {
    return todo("schedulerBindingRuns");
  }
  async schedulerBindingRunsEmpty(): Promise<string | null> {
    return todo("schedulerBindingRunsEmpty");
  }
  async schedulerBindingRunsCount(): Promise<string> {
    return todo("schedulerBindingRunsCount");
  }
  async schedulerBindingRunsPager(): Promise<{
    text: string;
    prevDisabled: boolean;
    nextDisabled: boolean;
  } | null> {
    return todo("schedulerBindingRunsPager");
  }
  async schedulerBindingRunsPage(_direction: "next" | "prev"): Promise<void> {
    return todo("schedulerBindingRunsPage");
  }
  async schedulerBindingWaitRunsRefetch(): Promise<void> {
    return todo("schedulerBindingWaitRunsRefetch");
  }
  async schedulerBindingRemovedVisible(): Promise<boolean> {
    return todo("schedulerBindingRemovedVisible");
  }
  async schedulerBindingBackToList(): Promise<void> {
    return todo("schedulerBindingBackToList");
  }
  async schedulerBindingToggleState(): Promise<{ checked: boolean; disabled: boolean }> {
    return todo("schedulerBindingToggleState");
  }
  async schedulerBindingToggle(): Promise<void> {
    return todo("schedulerBindingToggle");
  }
  async schedulerBindingOpenEdit(): Promise<void> {
    return todo("schedulerBindingOpenEdit");
  }
  async schedulerBindingOpenUninstall(): Promise<void> {
    return todo("schedulerBindingOpenUninstall");
  }
  async schedulerOpenProjectCalendar(_workspaceSlug: string, _projectId: string): Promise<void> {
    return todo("schedulerOpenProjectCalendar");
  }
  async schedulerCalendarView(): Promise<"week" | "month"> {
    return todo("schedulerCalendarView");
  }
  async schedulerCalendarSetView(_view: "week" | "month"): Promise<void> {
    return todo("schedulerCalendarSetView");
  }
  async schedulerCalendarTitle(): Promise<string> {
    return todo("schedulerCalendarTitle");
  }
  async schedulerCalendarStep(_direction: "prev" | "next"): Promise<void> {
    return todo("schedulerCalendarStep");
  }
  async schedulerCalendarToday(): Promise<void> {
    return todo("schedulerCalendarToday");
  }
  async schedulerCalendarEmptyVisible(): Promise<boolean> {
    return todo("schedulerCalendarEmptyVisible");
  }
  async schedulerCalendarTruncatedVisible(): Promise<boolean> {
    return todo("schedulerCalendarTruncatedVisible");
  }
  async schedulerCalendarMonthBlocks(): Promise<SchedulerCalendarBlock[]> {
    return todo("schedulerCalendarMonthBlocks");
  }
  async schedulerCalendarMonthOverflow(): Promise<string[]> {
    return todo("schedulerCalendarMonthOverflow");
  }
  async schedulerCalendarMonthTodayMarked(): Promise<boolean> {
    return todo("schedulerCalendarMonthTodayMarked");
  }
  async schedulerCalendarWeekBlocks(): Promise<SchedulerCalendarBlock[]> {
    return todo("schedulerCalendarWeekBlocks");
  }
  async schedulerCalendarWeekTodayMarked(): Promise<boolean> {
    return todo("schedulerCalendarWeekTodayMarked");
  }
  async schedulerCalendarTimeLineTop(): Promise<number | null> {
    return todo("schedulerCalendarTimeLineTop");
  }
  async schedulerCalendarClickBlock(_name: string): Promise<void> {
    return todo("schedulerCalendarClickBlock");
  }
  async schedulerCalendarAnyDraggable(): Promise<boolean> {
    return todo("schedulerCalendarAnyDraggable");
  }
  async schedulerCalendarExportControls(): Promise<string[]> {
    return todo("schedulerCalendarExportControls");
  }
  async schedulerRailVisible(): Promise<boolean> {
    return todo("schedulerRailVisible");
  }
  async schedulerRailRows(): Promise<{ name: string; checked: boolean }[]> {
    return todo("schedulerRailRows");
  }
  async schedulerRailToggle(_name: string): Promise<void> {
    return todo("schedulerRailToggle");
  }
  async schedulerRailShowAll(): Promise<void> {
    return todo("schedulerRailShowAll");
  }
  async schedulerRailHideAll(): Promise<void> {
    return todo("schedulerRailHideAll");
  }
  async schedulerRailCrossTabPersists(_workspaceSlug: string, _projectId: string, _name: string): Promise<boolean> {
    return todo("schedulerRailCrossTabPersists");
  }
  async schedulerRailNarrowHidden(): Promise<boolean> {
    return todo("schedulerRailNarrowHidden");
  }
  async schedulerDrawerOpen(): Promise<boolean> {
    return todo("schedulerDrawerOpen");
  }
  async schedulerDrawerRows(): Promise<{ label: string; value: string }[]> {
    return todo("schedulerDrawerRows");
  }
  async schedulerDrawerHeading(): Promise<{ state: string; name: string }> {
    return todo("schedulerDrawerHeading");
  }
  async schedulerDrawerLinks(): Promise<string[]> {
    return todo("schedulerDrawerLinks");
  }
  async schedulerDrawerClose(): Promise<void> {
    return todo("schedulerDrawerClose");
  }
  async schedulerDrawerEditVisible(): Promise<boolean> {
    return todo("schedulerDrawerEditVisible");
  }
  async schedulerDrawerEdit(): Promise<void> {
    return todo("schedulerDrawerEdit");
  }
  async schedulerDrawerViewScheduler(): Promise<void> {
    return todo("schedulerDrawerViewScheduler");
  }
  async schedulerOpenProjectSection(_workspaceSlug: string, _projectId: string): Promise<void> {
    return todo("schedulerOpenProjectSection");
  }
  async schedulerSectionTabs(): Promise<{ label: string; active: boolean }[]> {
    return todo("schedulerSectionTabs");
  }
  async schedulerSectionOpenTab(_tab: "List" | "Calendar"): Promise<void> {
    return todo("schedulerSectionOpenTab");
  }
  async schedulerOpenSettingsSchedulers(_workspaceSlug: string, _projectId: string): Promise<void> {
    return todo("schedulerOpenSettingsSchedulers");
  }
  async schedulerSettingsPanelVisible(): Promise<boolean> {
    return todo("schedulerSettingsPanelVisible");
  }
  async schedulerRunsExportControls(): Promise<string[]> {
    return todo("schedulerRunsExportControls");
  }

  // --- Runner chat on cloud/web (NEWFRONT-549 shell; messaging lands with
  // --- NEWFRONT-551). The page renders the history column, the header and
  // --- the composer inside the area shell. Selectors pin the same roles
  // --- and copy the scenarios use against the old app.

  /** The rail (area side nav) that hosts the chat contacts. */
  private runnerChatSideNav(): Locator {
    return this.page.getByTestId("runners-rail");
  }

  private runnerChatHistoryPanel(): Locator {
    return this.page.getByTestId("runner-chat-history");
  }

  private runnerChatComposerBox(): Locator {
    return this.page.getByPlaceholder(/Message this runner/);
  }

  private runnerChatThread(): Locator {
    return this.page.getByTestId("runner-chat");
  }

  async runnerChatOpen(workspaceSlug: string, runnerId: string, sessionId?: string): Promise<void> {
    const suffix = sessionId === undefined ? "" : `?sessionId=${encodeURIComponent(sessionId)}`;
    await this.page.goto(`/${workspaceSlug}/runners/chat/${runnerId}${suffix}`, { timeout: 60_000 });
    // Both the history heading and the composer render for known and
    // unknown runners, so the unavailable state settles here too.
    await this.runnerChatHistoryPanel().waitFor({ timeout: WebNewDriver.WAIT_MS });
    await this.runnerChatComposerBox().first().waitFor({ timeout: WebNewDriver.WAIT_MS });
  }

  async runnerChatContactNames(): Promise<string[]> {
    const links = this.runnerChatSideNav().getByTestId("runners-rail-contact");
    const names: string[] = [];
    for (const link of await links.all()) names.push((await link.innerText()).trim());
    return names;
  }

  async runnerChatContactDotClass(runnerName: string): Promise<string> {
    const contact = this.runnerChatSideNav().getByTestId("runners-rail-contact").filter({ hasText: runnerName });
    const dot = contact.locator("svg[data-status]");
    await dot.first().waitFor({ timeout: WebNewDriver.WAIT_MS });
    return (await dot.first().getAttribute("class")) ?? "";
  }

  async runnerChatOpenContact(runnerName: string): Promise<void> {
    const before = this.page.url();
    await this.runnerChatSideNav().getByTestId("runners-rail-contact").filter({ hasText: runnerName }).first().click();
    await this.page.waitForURL((url) => url.toString() !== before, { timeout: WebNewDriver.WAIT_MS });
    await this.runnerChatComposerBox().first().waitFor({ timeout: WebNewDriver.WAIT_MS });
  }

  async runnerChatHeader(): Promise<{ name: string; secondary: string; badge: string }> {
    const header = this.page.getByTestId("runner-chat-header");
    const name = (await header.getByTestId("runner-chat-name").innerText()).trim();
    const secondary = await header
      .getByTestId("runner-chat-secondary")
      .innerText()
      .then(
        (text) => text.trim(),
        () => ""
      );
    // The badge is the only status-labelled element in the header.
    const badge = (
      (await header
        .getByTestId("runner-status-badge")
        .first()
        .innerText()
        .catch(() => "")) ?? ""
    ).trim();
    return { name, secondary, badge };
  }

  async runnerChatHistoryEmptyVisible(): Promise<boolean> {
    return this.runnerChatHistoryPanel()
      .getByTestId("runner-chat-history-empty")
      .isVisible()
      .then(
        (visible) => visible,
        () => false
      );
  }

  async runnerChatHistoryItems(): Promise<{ title: string; subtitle: string; active: boolean }[]> {
    const entries = this.runnerChatHistoryPanel().locator("nav button");
    const items: { title: string; subtitle: string; active: boolean }[] = [];
    for (const entry of await entries.all()) {
      const lines = await entry.locator(":scope > span > span").allInnerTexts();
      items.push({
        title: (lines[0] ?? "").trim(),
        subtitle: (lines[1] ?? "").trim(),
        active: (await entry.getAttribute("aria-current")) === "true",
      });
    }
    return items;
  }

  async runnerChatClickHistoryItem(index: number): Promise<void> {
    await this.runnerChatHistoryPanel().locator("nav button").nth(index).click();
  }

  async runnerChatNewChat(): Promise<void> {
    const before = new URL(this.page.url()).search;
    await this.runnerChatHistoryPanel().getByRole("button", { name: "New chat", exact: true }).click();
    // Resolves on selection (the address gains the session) or on the
    // error toast: the error-state scenario asserts which one it got.
    await Promise.race([
      this.page
        .waitForFunction((previous) => new URL(window.location.href).search !== previous, before, {
          timeout: WebNewDriver.WAIT_MS,
        })
        .then(
          () => true,
          () => false
        ),
      this.page
        .getByTestId("toast-notification")
        .first()
        .waitFor({ timeout: WebNewDriver.WAIT_MS })
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
      if (
        request.method() === "POST" &&
        pathname.endsWith("/chat/sessions/") &&
        this.runnerChatSessionCreateFailRemaining > 0
      ) {
        this.runnerChatSessionCreateFailRemaining -= 1;
        await route.fulfill({
          status: 500,
          contentType: "application/json",
          body: JSON.stringify({ error: "parity session-create failure" }),
        });
        return;
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
    return this.runnerChatThread()
      .getByRole("button", { name: "Send message" })
      .first()
      .isEnabled()
      .then(
        (enabled) => enabled,
        () => false
      );
  }

  async runnerChatClickSend(): Promise<void> {
    await this.runnerChatThread().getByRole("button", { name: "Send message" }).first().click();
  }

  async runnerChatComposerReason(): Promise<string | null> {
    const reason = this.runnerChatThread().getByTestId("runner-chat-block-reason").first();
    if ((await reason.count()) === 0) return null;
    const text = (await reason.innerText()).trim();
    return text === "" ? null : text;
  }

  async runnerChatTextareaDisabled(): Promise<boolean> {
    return this.runnerChatComposerBox().first().isDisabled();
  }

  async runnerChatAlertText(): Promise<string | null> {
    const alert = this.runnerChatThread().locator('[role="alert"]').first();
    if ((await alert.count()) === 0) return null;
    const text = (await alert.innerText()).trim();
    return text === "" ? null : text;
  }

  async runnerChatDismissAlert(): Promise<void> {
    await this.runnerChatThread().locator('[role="alert"]').first().getByRole("button").first().click();
  }

  async runnerChatLastToast(): Promise<{ title: string; message: string } | null> {
    const candidates = this.page.getByTestId("toast-notification");
    const count = await candidates.count();
    if (count === 0) return null;
    // Toasts prepend newest-first, so the first root carries the latest.
    for (let index = 0; index < count; index += 1) {
      const lines = ((await candidates.nth(index).innerText()) ?? "")
        .split("\n")
        .map((line) => line.trim())
        .filter((line) => line.length > 0);
      if (lines.length === 0) continue;
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
    // Dictation ships with the messaging slice; until then there is none.
    const mic = this.runnerChatThread()
      .getByRole("button", { name: /dictat|voice/i })
      .first();
    if ((await mic.count()) === 0) return null;
    return await mic.getAttribute("aria-label");
  }

  async runnerChatClickVoiceButton(): Promise<void> {
    await this.runnerChatThread()
      .getByRole("button", { name: /dictat|voice/i })
      .first()
      .click();
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
    const list = this.page.getByTestId("runner-chat-transcript").getByRole("list", { name: "Messages" });
    if ((await list.count()) === 0) return [];
    const rows: { role: string; text: string }[] = [];
    for (const item of await list.getByRole("listitem").all()) {
      const text = ((await item.innerText()) ?? "").trim();
      if (text === "") continue;
      const role = (await item.locator("[data-role]").first().getAttribute("data-role")) ?? "status";
      rows.push({ role, text });
    }
    return rows;
  }

  async runnerChatAssistantBubbleHtml(index: number): Promise<string> {
    const bubble = this.page.getByTestId("runner-chat-transcript").locator('[data-role="assistant"]').nth(index);
    return bubble.innerHTML();
  }

  async runnerChatActivityStrip(): Promise<string[]> {
    // The activity strip is derived from the event feed and lands with the
    // messaging slice; the shell renders none.
    const items = this.runnerChatThread().getByTestId("runner-chat-activity");
    const total = await items.count();
    const labels: string[] = [];
    for (let i = 0; i < total; i += 1) {
      const text = ((await items.nth(i).innerText()) ?? "").trim();
      if (text !== "") labels.push(text);
    }
    return labels;
  }

  async runnerChatStopVisible(): Promise<boolean> {
    return this.runnerChatThread()
      .getByRole("button", { name: "Stop response" })
      .count()
      .then((count) => count > 0);
  }

  async runnerChatClickStop(): Promise<void> {
    await this.runnerChatThread().getByRole("button", { name: "Stop response" }).first().click();
  }

  async runnerChatClickClose(): Promise<void> {
    await this.page.getByTestId("runner-chat-header").getByRole("button", { name: "Close chat" }).first().click();
  }

  async runnerChatApprovalPromptVisible(): Promise<boolean> {
    return this.runnerChatThread()
      .getByTestId("runner-chat-approval")
      .count()
      .then((count) => count > 0);
  }

  async runnerChatHoldMessageList(_ms: number): Promise<void> {
    return todo("runnerChatHoldMessageList");
  }

  async runnerChatReleaseMessageList(): Promise<void> {
    return todo("runnerChatReleaseMessageList");
  }

  // --- Prompts + project automations (NEWFRONT-186, AGT-023–037) ---
  private static readonly PROMPT_SECTIONS_PATTERN = "**/api/workspaces/*/prompt-sections*";

  private promptsSectionsNav(): Locator {
    return this.page.getByRole("navigation", { name: "Prompt sections", exact: true });
  }

  private promptsReceiptNavigation(): Locator {
    return this.page.getByRole("navigation", { name: "Prompt receipt", exact: true });
  }

  private promptsCards(): Locator {
    return this.page.locator('article[id^="prompt-section-"]');
  }

  private promptsCard(key: string): Locator {
    return this.page.locator(`article[id="prompt-section-${key}"]`);
  }

  private promptsOpenEditorRoot(): Locator {
    return this.page.locator('article[id^="prompt-section-"]', { has: this.page.locator("textarea") });
  }

  private async promptsReadCard(root: Locator): Promise<PromptSectionCard | null> {
    const id = await root.getAttribute("id").catch(() => null);
    if (id === null || !id.startsWith("prompt-section-")) return null;

    const badgeLabels = new Set([
      "Pi Dash default",
      "Workspace override",
      "Your override",
      "Locked",
      "Admin-managed",
      "Coding task",
      "Review",
      "Scheduler",
    ]);
    const badgeSpans = root.locator(":scope > div").first().locator("span");
    const badgeTexts: string[] = [];
    for (let index = 0; index < (await badgeSpans.count()); index++) {
      const text = (
        (await badgeSpans
          .nth(index)
          .innerText()
          .catch(() => "")) ?? ""
      ).trim();
      if (badgeLabels.has(text)) badgeTexts.push(text);
    }

    const sourceBadge =
      badgeTexts.find(
        (text) => text === "Pi Dash default" || text === "Workspace override" || text === "Your override"
      ) ?? "";
    const kinds = badgeTexts.filter((text) => text === "Coding task" || text === "Review" || text === "Scheduler");
    const badges = badgeTexts.filter((text) => text !== sourceBadge);
    const buttons = root.getByRole("button");
    let workspaceEditLabel: string | null = null;
    let personalEditLabel: string | null = null;
    for (let index = 0; index < (await buttons.count()); index++) {
      const text = (
        (await buttons
          .nth(index)
          .innerText()
          .catch(() => "")) ?? ""
      ).trim();
      if (text === "Edit workspace default" || text === "Customize for workspace") workspaceEditLabel = text;
      if (text === "Edit my override" || text === "Customize for me") personalEditLabel = text;
    }

    return {
      key: id.slice("prompt-section-".length),
      title: (
        (await root
          .getByRole("heading", { level: 2 })
          .first()
          .innerText()
          .catch(() => "")) ?? ""
      ).trim(),
      sourceBadge,
      badges,
      kinds,
      staleWarning: await this.isShown(
        root.getByText("The default changed after this override was saved", { exact: false })
      ),
      body: (
        (await root
          .locator(":scope > pre")
          .innerText()
          .catch(() => "")) ?? ""
      ).trimEnd(),
      workspaceEditLabel,
      personalEditLabel,
    };
  }

  async promptsOpen(workspaceSlug: string): Promise<void> {
    await this.page.goto(`/${workspaceSlug}/prompts`);
    await this.page.getByRole("heading", { name: "Prompts", exact: true }).waitFor({ timeout: WebNewDriver.WAIT_MS });
    await this.promptsSectionsNav()
      .or(this.page.getByText("Loading…", { exact: true }))
      .or(this.page.getByText("Could not load prompt sections for this workspace.", { exact: true }))
      .first()
      .waitFor({ timeout: WebNewDriver.WAIT_MS });
  }
  async promptsActiveTab(): Promise<"Sections" | "Receipt"> {
    return (await this.isShown(this.promptsReceiptNavigation())) ? "Receipt" : "Sections";
  }
  async promptsOpenTab(tab: "Sections" | "Receipt"): Promise<void> {
    const destination = tab === "Sections" ? this.promptsSectionsNav() : this.promptsReceiptNavigation();
    if (await this.isShown(destination)) return;
    // The tabs carry role=tab, not button; the button role never matches.
    await this.page.getByRole("tab", { name: tab, exact: true }).click();
    await destination.waitFor({ timeout: WebNewDriver.WAIT_MS });
  }
  async promptsSectionCards(): Promise<PromptSectionCard[]> {
    const roots = this.promptsCards();
    const cards: PromptSectionCard[] = [];
    for (let index = 0; index < (await roots.count()); index++) {
      const card = await this.promptsReadCard(roots.nth(index));
      if (card !== null) cards.push(card);
    }
    return cards;
  }
  async promptsSectionCard(key: string): Promise<PromptSectionCard | null> {
    const root = this.promptsCard(key);
    if ((await root.count()) === 0) return null;
    return this.promptsReadCard(root.first());
  }
  async promptsSectionNav(): Promise<{ title: string; key: string }[]> {
    const links = this.promptsSectionsNav().getByRole("link");
    const entries: { title: string; key: string }[] = [];
    for (let index = 0; index < (await links.count()); index++) {
      const link = links.nth(index);
      const href = (await link.getAttribute("href").catch(() => null)) ?? "";
      const title = (
        (await link
          .locator("span")
          .first()
          .innerText()
          .catch(() => "")) ?? ""
      ).trim();
      entries.push({ title, key: href.replace(/^#prompt-section-/, "") });
    }
    return entries;
  }
  async promptsSectionNavJump(key: string): Promise<string> {
    await this.promptsSectionsNav().locator(`a[href="#prompt-section-${key}"]`).click();
    await this.promptsCard(key).waitFor({ state: "visible", timeout: WebNewDriver.WAIT_MS });
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
    await this.page.route(WebNewDriver.PROMPT_SECTIONS_PATTERN, async (route) => {
      const request = route.request();
      if (request.method() === "GET" && request.url().includes(`scope=${scope}`)) {
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
    await this.page.unroute(WebNewDriver.PROMPT_SECTIONS_PATTERN).catch(() => undefined);
  }
  async promptsDelaySectionsOnce(ms: number): Promise<void> {
    let armed = true;
    await this.page.route(WebNewDriver.PROMPT_SECTIONS_PATTERN, async (route) => {
      if (armed && route.request().method() === "GET") {
        armed = false;
        await this.page.waitForTimeout(ms);
      }
      await route.continue();
    });
  }
  async promptsFailUpsertOnce(): Promise<void> {
    let armed = true;
    await this.page.route(WebNewDriver.PROMPT_SECTIONS_PATTERN, async (route) => {
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
    const label =
      scope === "workspace"
        ? /^(Edit workspace default|Customize for workspace)$/
        : /^(Edit my override|Customize for me)$/;
    const root = this.promptsCard(key);
    await root.getByRole("button", { name: label }).click();
    await root.locator("textarea").waitFor({ timeout: WebNewDriver.WAIT_MS });
  }
  async promptsEditorState(): Promise<PromptEditorState | null> {
    const root = this.promptsOpenEditorRoot().first();
    if ((await root.count()) === 0) return null;

    const textarea = root.locator("textarea");
    const defaultPane = root.locator("pre").first();
    const defaultVisible = await this.isShown(defaultPane);
    const errorBox = root.getByRole("alert").first();
    const errorVisible = await this.isShown(errorBox);
    return {
      scopeLabel: (
        (await root
          .getByText(/^(Workspace default|Your personal override)$/, { exact: true })
          .first()
          .innerText()
          .catch(() => "")) ?? ""
      ).trim(),
      draft: (await textarea.inputValue().catch(() => "")) ?? "",
      saveEnabled: await root
        .getByRole("button", { name: "Save", exact: true })
        .isEnabled()
        .catch(() => false),
      defaultVisible,
      defaultBody: defaultVisible ? ((await defaultPane.innerText().catch(() => "")) ?? "").trimEnd() : null,
      revertVisible: await this.isShown(root.getByRole("button", { name: "Revert to default", exact: true })),
      error: errorVisible ? ((await errorBox.innerText().catch(() => "")) ?? "").trim() || null : null,
    };
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
    await root.locator("textarea").waitFor({ state: "detached", timeout: WebNewDriver.WAIT_MS });
  }
  async promptsEditorToggleCompare(): Promise<void> {
    await this.promptsOpenEditorRoot()
      .first()
      .getByRole("button", { name: /^(Show default|Hide default)$/ })
      .click();
  }
  async promptsEditorRevertOpen(): Promise<void> {
    await this.promptsOpenEditorRoot().first().getByRole("button", { name: "Revert to default", exact: true }).click();
    await this.page
      .getByRole("heading", { name: "Revert to default?", exact: true })
      .waitFor({ timeout: WebNewDriver.WAIT_MS });
  }
  async promptsRevertDialog(): Promise<PromptRevertDialog | null> {
    const heading = this.page.getByRole("heading", { name: "Revert to default?", exact: true });
    if (!(await this.isShown(heading))) return null;
    const dialog = this.page.getByRole("dialog").filter({ has: heading });
    const confirm = dialog.getByRole("button", { name: /^Revert/ }).first();
    return {
      title: ((await heading.innerText().catch(() => "")) ?? "").trim(),
      body: (
        (await dialog
          .locator("p")
          .first()
          .innerText()
          .catch(() => "")) ?? ""
      ).trim(),
      confirmLabel: ((await confirm.innerText().catch(() => "")) ?? "").trim(),
    };
  }
  async promptsRevertConfirm(): Promise<void> {
    const heading = this.page.getByRole("heading", { name: "Revert to default?", exact: true });
    await this.page
      .getByRole("dialog")
      .filter({ has: heading })
      .getByRole("button", { name: "Revert", exact: true })
      .click();
  }
  async promptsRevertCancel(): Promise<void> {
    const heading = this.page.getByRole("heading", { name: "Revert to default?", exact: true });
    await this.page
      .getByRole("dialog")
      .filter({ has: heading })
      .getByRole("button", { name: "Cancel", exact: true })
      .click();
    await heading.waitFor({ state: "detached", timeout: WebNewDriver.WAIT_MS });
  }
  // Receipt composition and previews (NEWFRONT-599, AGT-028–031). The
  // receipt cards are articles anchored `#prompt-receipt-<kind>`; the
  // assembled templates and preview outcomes carry stable test ids so the
  // reads never depend on paint order.
  private promptsReceiptCardsLocator(): Locator {
    return this.page.locator('article[id^="prompt-receipt-"]');
  }

  private promptsReceiptCard(kind: string): Locator {
    return this.page.locator(`article[id="prompt-receipt-${kind}"]`);
  }

  private promptsReceiptHeader(kind: string): Locator {
    return this.promptsReceiptCard(kind).getByRole("button", { name: /^(Coding task|Review|Scheduler)$/ });
  }

  private async promptsReceiptKind(root: Locator): Promise<string> {
    const id = await root.getAttribute("id").catch(() => null);
    if (id === null || !id.startsWith("prompt-receipt-")) return "";
    return id.slice("prompt-receipt-".length);
  }

  /** Template text as the oracle reads it: trailing blank lines dropped. */
  private promptsTrimmed(template: string): string {
    return template.replace(/\s+$/, "");
  }

  async promptsReceiptCards(): Promise<PromptReceiptCard[]> {
    const roots = this.promptsReceiptCardsLocator();
    const cards: PromptReceiptCard[] = [];
    for (let index = 0; index < (await roots.count()); index++) {
      const root = roots.nth(index);
      const kind = await this.promptsReceiptKind(root);
      if (kind === "") continue;
      const countBadge = (
        await root
          .getByTestId("receipt-count")
          .innerText()
          .catch(() => "")
      ).trim();
      const items = root.locator("ol > li");
      const sections: PromptReceiptCard["sections"] = [];
      for (let itemIndex = 0; itemIndex < (await items.count()); itemIndex++) {
        const item = items.nth(itemIndex);
        const num = (
          await item
            .locator("span")
            .first()
            .innerText()
            .catch(() => "")
        ).trim();
        const title = (
          await item
            .locator("span > span")
            .first()
            .innerText()
            .catch(() => "")
        ).trim();
        const key = (
          await item
            .locator("span > span")
            .nth(1)
            .innerText()
            .catch(() => "")
        ).trim();
        const sourceBadge = (
          await item
            .locator("span")
            .last()
            .innerText()
            .catch(() => "")
        ).trim();
        sections.push({ num, title, key, sourceBadge });
      }
      cards.push({ kind, countBadge, sections });
    }
    return cards;
  }

  async promptsReceiptNav(): Promise<{ kind: string; count: string }[]> {
    const links = this.promptsReceiptNavigation().getByRole("link");
    const entries: { kind: string; count: string }[] = [];
    for (let index = 0; index < (await links.count()); index++) {
      const link = links.nth(index);
      const href = (await link.getAttribute("href").catch(() => null)) ?? "";
      const lines = (await link.innerText().catch(() => ""))
        .split("\n")
        .map((line) => line.trim())
        .filter((line) => line.length > 0);
      entries.push({ kind: href.replace(/^#prompt-receipt-/, ""), count: lines[1] ?? "" });
    }
    return entries;
  }

  async promptsReceiptNavJump(kind: string): Promise<string> {
    await this.promptsReceiptNavigation().locator(`a[href="#prompt-receipt-${kind}"]`).click();
    await this.promptsReceiptCard(kind).waitFor({ state: "visible", timeout: WebNewDriver.WAIT_MS });
    return this.page.evaluate(() => window.location.hash);
  }

  async promptsReceiptToggle(kind: string): Promise<void> {
    await this.promptsReceiptHeader(kind).click();
  }

  async promptsReceiptExpanded(kind: string): Promise<boolean> {
    const expanded = await this.promptsReceiptHeader(kind)
      .getAttribute("aria-expanded")
      .catch(() => null);
    return expanded === "true";
  }

  async promptsReceiptTemplate(kind: string): Promise<string | null> {
    if (!(await this.promptsReceiptExpanded(kind))) return null;
    const block = this.promptsReceiptCard(kind).getByTestId(`receipt-template-${kind}`);
    if (!(await this.isShown(block))) return null;
    return this.promptsTrimmed(await block.innerText().catch(() => ""));
  }

  async promptsReceiptAutomatic(kind: string): Promise<string | null> {
    if (!(await this.promptsReceiptExpanded(kind))) return null;
    const block = this.promptsReceiptCard(kind).getByTestId(`receipt-automatic-${kind}`);
    if (!(await this.isShown(block))) return null;
    return this.promptsTrimmed(await block.innerText().catch(() => ""));
  }

  async promptsSavedPreviewVisible(kind: string): Promise<boolean> {
    return this.isShown(this.promptsReceiptCard(kind).getByRole("heading", { name: "Preview", exact: true }));
  }

  async promptsSavedPreviewSubmitEnabled(kind: string): Promise<boolean> {
    return this.promptsReceiptCard(kind)
      .getByRole("button", { name: "Preview", exact: true })
      .isEnabled()
      .catch(() => false);
  }

  async promptsSavedPreviewSubmit(kind: string, target: string): Promise<void> {
    const card = this.promptsReceiptCard(kind);
    // Scope to the preview form so the read never depends on card order.
    await card.locator("form").getByRole("textbox").fill(target);
    await card.getByRole("button", { name: "Preview", exact: true }).click();
  }

  private async promptsPreviewOutcome(root: Locator, prefix: string) {
    const errorBox = root.getByTestId(`${prefix}-error`);
    const errorShown = await this.isShown(errorBox);
    const error = errorShown ? (await errorBox.innerText().catch(() => "")).trim() || null : null;
    const resultBox = root.getByTestId(`${prefix}-result`);
    const resultShown = await this.isShown(resultBox);
    const prompt = resultShown ? this.promptsTrimmed(await resultBox.innerText().catch(() => "")) || null : null;
    return { prompt, error };
  }

  async promptsSavedPreviewResult(kind: string): Promise<{ prompt: string | null; error: string | null }> {
    return this.promptsPreviewOutcome(this.promptsReceiptCard(kind), `receipt-preview-${kind}`);
  }

  async promptsDraftPreviewKinds(): Promise<string[]> {
    const root = this.promptsOpenEditorRoot().first();
    const kinds: string[] = [];
    for (const kind of ["coding-task", "review", "scheduler"] as const) {
      if (await this.isShown(root.getByTestId(`draft-preview-kind-${kind}`))) kinds.push(kind);
    }
    return kinds;
  }

  async promptsDraftPreviewSelectKind(kind: string): Promise<void> {
    await this.promptsOpenEditorRoot().first().getByTestId(`draft-preview-kind-${kind}`).click();
  }

  async promptsDraftPreviewSubmitEnabled(): Promise<boolean> {
    return this.promptsOpenEditorRoot()
      .first()
      .getByRole("button", { name: "Preview draft", exact: true })
      .isEnabled()
      .catch(() => false);
  }

  async promptsDraftPreviewSubmit(target: string): Promise<void> {
    const root = this.promptsOpenEditorRoot().first();
    // The open editor also holds the section textarea; the target is the
    // one textbox inside the preview form.
    await root.locator("form").getByRole("textbox").fill(target);
    await root.getByRole("button", { name: "Preview draft", exact: true }).click();
  }

  async promptsDraftPreviewResult(): Promise<{ prompt: string | null; error: string | null }> {
    return this.promptsPreviewOutcome(this.promptsOpenEditorRoot().first(), "draft-preview");
  }
  async automationsOpen(_workspaceSlug: string, _projectId: string): Promise<void> {
    return todo("automationsOpen");
  }
  async automationsNotAuthorizedVisible(): Promise<boolean> {
    return todo("automationsNotAuthorizedVisible");
  }
  async automationsArchiveRow(): Promise<AutomationRow> {
    return todo("automationsArchiveRow");
  }
  async automationsArchiveToggle(): Promise<void> {
    return todo("automationsArchiveToggle");
  }
  async automationsArchiveSetPreset(_months: number): Promise<void> {
    return todo("automationsArchiveSetPreset");
  }
  async automationsArchiveOpenCustom(): Promise<void> {
    return todo("automationsArchiveOpenCustom");
  }
  async automationsCloseRow(): Promise<AutomationCloseRow> {
    return todo("automationsCloseRow");
  }
  async automationsCloseToggle(): Promise<void> {
    return todo("automationsCloseToggle");
  }
  async automationsCloseSetPreset(_months: number): Promise<void> {
    return todo("automationsCloseSetPreset");
  }
  async automationsCloseSetState(_name: string): Promise<void> {
    return todo("automationsCloseSetState");
  }
  async automationsCloseStateOptions(): Promise<string[]> {
    return todo("automationsCloseStateOptions");
  }
  async automationsCloseOpenCustom(): Promise<void> {
    return todo("automationsCloseOpenCustom");
  }
  async automationsMonthModal(): Promise<AutomationMonthModal | null> {
    return todo("automationsMonthModal");
  }
  async automationsMonthFill(_value: string): Promise<void> {
    return todo("automationsMonthFill");
  }
  async automationsMonthSubmit(): Promise<void> {
    return todo("automationsMonthSubmit");
  }
  async automationsMonthCancel(): Promise<void> {
    return todo("automationsMonthCancel");
  }
  async automationsFailUpdateOnce(): Promise<void> {
    return todo("automationsFailUpdateOnce");
  }
  async automationsBuiltInRows(): Promise<string[]> {
    return todo("automationsBuiltInRows");
  }
  async automationsHasExtensionRows(): Promise<boolean> {
    return todo("automationsHasExtensionRows");
  }

  // --- Add-runner modal + creation (NEWFRONT-179, RUN-006–009) ---

  async addRunnerOpenFromRunners(_workspaceSlug: string, _projectId?: string): Promise<void> {
    return todo("addRunnerOpenFromRunners");
  }
  async addRunnerOpenFromMachines(_workspaceSlug: string): Promise<void> {
    return todo("addRunnerOpenFromMachines");
  }
  async addRunnerVisible(): Promise<boolean> {
    return todo("addRunnerVisible");
  }
  async addRunnerLayout(): Promise<"form" | "remote" | "command"> {
    return todo("addRunnerLayout");
  }
  async addRunnerForm(): Promise<AddRunnerFormState> {
    return todo("addRunnerForm");
  }
  async addRunnerMachineOptions(): Promise<string[]> {
    return todo("addRunnerMachineOptions");
  }
  async addRunnerProjectOptions(): Promise<string[]> {
    return todo("addRunnerProjectOptions");
  }
  async addRunnerPodOptions(): Promise<string[]> {
    return todo("addRunnerPodOptions");
  }
  async addRunnerAgentOptions(): Promise<string[]> {
    return todo("addRunnerAgentOptions");
  }
  async addRunnerModelOptions(): Promise<string[]> {
    return todo("addRunnerModelOptions");
  }
  async addRunnerProjectError(): Promise<string | null> {
    return todo("addRunnerProjectError");
  }
  async addRunnerNameError(): Promise<string | null> {
    return todo("addRunnerNameError");
  }
  async addRunnerPickMachine(_label: string): Promise<void> {
    return todo("addRunnerPickMachine");
  }
  async addRunnerPickManual(): Promise<void> {
    return todo("addRunnerPickManual");
  }
  async addRunnerPickProject(_name: string): Promise<void> {
    return todo("addRunnerPickProject");
  }
  async addRunnerPickPod(_name: string): Promise<void> {
    return todo("addRunnerPickPod");
  }
  async addRunnerSetName(_name: string): Promise<void> {
    return todo("addRunnerSetName");
  }
  async addRunnerSetWorkingDir(_dir: string): Promise<void> {
    return todo("addRunnerSetWorkingDir");
  }
  async addRunnerPickAgent(_label: string): Promise<void> {
    return todo("addRunnerPickAgent");
  }
  async addRunnerPickModel(_label: string): Promise<void> {
    return todo("addRunnerPickModel");
  }
  async addRunnerSubmit(): Promise<void> {
    return todo("addRunnerSubmit");
  }
  async addRunnerClose(): Promise<void> {
    return todo("addRunnerClose");
  }
  async addRunnerRemotePhase(): Promise<AddRunnerRemotePhase | null> {
    return todo("addRunnerRemotePhase");
  }
  async addRunnerRemoteText(): Promise<string | null> {
    return todo("addRunnerRemoteText");
  }
  async addRunnerRemoteRunnerName(): Promise<string | null> {
    return todo("addRunnerRemoteRunnerName");
  }
  async addRunnerRemoteBack(): Promise<void> {
    return todo("addRunnerRemoteBack");
  }
  async addRunnerRemoteManual(): Promise<void> {
    return todo("addRunnerRemoteManual");
  }
  async addRunnerCreateSpyStart(): Promise<void> {
    return todo("addRunnerCreateSpyStart");
  }
  async addRunnerCreateSpyBodies(): Promise<string[]> {
    return todo("addRunnerCreateSpyBodies");
  }
  async addRunnerCreateSpyStop(): Promise<void> {
    return todo("addRunnerCreateSpyStop");
  }
  async addRunnerStatusSpyStart(): Promise<void> {
    return todo("addRunnerStatusSpyStart");
  }
  async addRunnerStatusSpyUrls(): Promise<string[]> {
    return todo("addRunnerStatusSpyUrls");
  }
  async addRunnerStatusSpyStop(): Promise<void> {
    return todo("addRunnerStatusSpyStop");
  }
  async addRunnerCommandText(): Promise<string | null> {
    return todo("addRunnerCommandText");
  }
  async addRunnerCommandHeader(): Promise<string | null> {
    return todo("addRunnerCommandHeader");
  }
  async addRunnerShellOptions(): Promise<string[]> {
    return todo("addRunnerShellOptions");
  }
  async addRunnerActiveShell(): Promise<string | null> {
    return todo("addRunnerActiveShell");
  }
  async addRunnerPickShell(_label: string): Promise<void> {
    return todo("addRunnerPickShell");
  }
  async addRunnerCopy(): Promise<void> {
    return todo("addRunnerCopy");
  }
  async addRunnerCopyState(): Promise<string | null> {
    return todo("addRunnerCopyState");
  }
  async addRunnerReadClipboard(): Promise<string> {
    return todo("addRunnerReadClipboard");
  }
  async addRunnerBreakClipboard(): Promise<void> {
    return todo("addRunnerBreakClipboard");
  }
  async addRunnerOriginNote(): Promise<string | null> {
    return todo("addRunnerOriginNote");
  }
  async addRunnerCommandBack(): Promise<void> {
    return todo("addRunnerCommandBack");
  }
  async addRunnerLastToast(): Promise<string | null> {
    return todo("addRunnerLastToast");
  }

  // --- Assistant entry surfaces (NEWFRONT-603): landing, setup card,
  // --- composer gates, conversation rail and the dashboard card.

  /** Spy counters and one-shot stubs for the assistant scenarios. */
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

  /** The shared composer textarea (landing and conversation share it). */
  private assistantComposerBox() {
    return this.page.locator('textarea[placeholder*="Ask Pi Dash to do something"]');
  }

  /** The composer's send action (icon-only, so it is labelled). */
  private assistantSendButton() {
    return this.page.locator('button[aria-label="Send message"]').first();
  }

  /** The assistant rail beside the landing and conversation routes. */
  private assistantSidebar() {
    return this.page.locator('[data-testid="assistant-rail"]').first();
  }

  /**
   * Wait for one assistant surface that starts on a config read: present,
   * then past its pending branch. The cold open paints the column before
   * the read resolves, so waiting for the column alone lets a scenario
   * read "no greeting / no setup card" off the spinner (AGT-038–041).
   */
  private async assistantWaitSettled(selector: string): Promise<void> {
    const surface = this.page.locator(selector).first();
    await surface.waitFor({ timeout: WebNewDriver.WAIT_MS });
    await this.page.locator(`${selector}[data-settled="true"]`).first().waitFor({ timeout: WebNewDriver.WAIT_MS });
  }

  /** The dashboard card's single-line entry input. */
  private assistantHomeCardInput() {
    return this.page.locator('input[placeholder*="Ask Pi Dash to do something"]');
  }

  /** The dashboard card root: the card shell around the entry input. */
  private assistantHomeCard() {
    return this.page.locator("#home-assistant-card").first();
  }

  // --- Assistant chat core (NEWFRONT-187). Throwing stubs per the shared
  // --- driver contract; the assistant area fills these in when it lands.

  async assistantOpenLanding(workspaceSlug: string): Promise<void> {
    await this.page.goto(`/${workspaceSlug}/assistant`);
    await this.page.waitForLoadState("domcontentloaded");
    // The greeting/setup swap rides on the config read, so settle on the
    // landing column; scenarios poll for the stable branch.
    await this.assistantWaitSettled('[data-testid="assistant-landing"]');
  }
  async assistantOpenThread(workspaceSlug: string, threadId: string): Promise<void> {
    await this.page.goto(`/${workspaceSlug}/assistant/${threadId}`);
    await this.page.waitForLoadState("domcontentloaded");
    await this.assistantWaitSettled('[data-testid="assistant-thread"]');
  }
  async assistantOpenHome(workspaceSlug: string): Promise<void> {
    await this.page.goto(`/${workspaceSlug}`);
    await this.page.waitForLoadState("domcontentloaded");
    await this.page.getByRole("main").first().waitFor({ timeout: WebNewDriver.WAIT_MS });
  }
  async assistantCurrentPath(): Promise<string> {
    return new URL(this.page.url()).pathname;
  }
  async assistantGoBack(): Promise<void> {
    await this.page.goBack();
    await this.page.waitForLoadState("domcontentloaded");
  }
  async assistantLandingGreeting(): Promise<AssistantLandingGreeting | null> {
    const column = this.page.locator('[data-testid="assistant-landing"]').first();
    const headline = column.locator("h1").first();
    if ((await headline.count()) === 0) return null;
    const caption = column.locator("p").first();
    return {
      headline: (await headline.innerText()).trim(),
      caption: (await caption.count()) === 0 ? "" : (await caption.innerText()).trim(),
    };
  }
  async assistantLandingComposerVisible(): Promise<boolean> {
    return this.isShown(
      this.page.locator('[data-testid="assistant-landing"] textarea[placeholder*="Ask Pi Dash to do something"]')
    );
  }
  async assistantSetupCard(): Promise<{ title: string; body: string; button: string } | null> {
    const title = this.page.getByText("Set up your AI assistant", { exact: true }).first();
    if ((await title.count()) === 0) return null;
    const card = this.page.locator('[data-testid="assistant-setup-card"]').first();
    const root = (await card.count()) > 0 ? card : title.locator("xpath=..");
    return {
      title: (await title.innerText()).trim(),
      body: (
        (await root
          .locator("p")
          .first()
          .innerText()
          .catch(() => "")) ?? ""
      ).trim(),
      button: (
        await root
          .getByRole("button", { name: "Configure provider", exact: true })
          .first()
          .innerText()
          .catch(() => "")
      ).trim(),
    };
  }
  async assistantSetupCardClick(): Promise<void> {
    await this.page
      .locator('[data-testid="assistant-setup-card"]')
      .first()
      .getByRole("button", { name: "Configure provider", exact: true })
      .click();
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
    return this.isShown(this.assistantSendButton());
  }
  async assistantSendEnabled(): Promise<boolean> {
    return this.assistantSendButton()
      .isEnabled()
      .then(
        (enabled) => enabled,
        () => false
      );
  }
  async assistantClickSend(): Promise<void> {
    await this.assistantSendButton().click();
  }
  async assistantStopVisible(): Promise<boolean> {
    return this.isShown(this.page.locator('button[aria-label="Stop response"]'));
  }
  async assistantClickStop(): Promise<void> {
    return todo("assistantClickStop");
  }
  async assistantComposerReason(): Promise<string | null> {
    const reason = this.page.locator('[data-testid="assistant-composer-reason"]').first();
    if ((await reason.count()) === 0) return null;
    const text = (await reason.innerText()).trim();
    return text === "" ? null : text;
  }
  async assistantTextareaDisabled(): Promise<boolean> {
    return this.assistantComposerBox()
      .isDisabled()
      .then(
        (disabled) => disabled,
        () => false
      );
  }
  async assistantErrorLine(): Promise<string | null> {
    return todo("assistantErrorLine");
  }
  async assistantMicLabel(): Promise<string | null> {
    return todo("assistantMicLabel");
  }
  async assistantClickMic(): Promise<void> {
    return todo("assistantClickMic");
  }
  async assistantDictationHint(): Promise<string | null> {
    return todo("assistantDictationHint");
  }
  async assistantBubbles(): Promise<AssistantBubble[]> {
    return todo("assistantBubbles");
  }
  async assistantBubbleHtml(_index: number): Promise<string> {
    return todo("assistantBubbleHtml");
  }
  async assistantToolActivities(): Promise<AssistantToolActivity[]> {
    return todo("assistantToolActivities");
  }
  async assistantNoticeLines(): Promise<string[]> {
    return todo("assistantNoticeLines");
  }
  async assistantEmptyState(): Promise<string | null> {
    return todo("assistantEmptyState");
  }
  async assistantIsScrolledToBottom(): Promise<boolean> {
    return todo("assistantIsScrolledToBottom");
  }
  async assistantClickToolLink(_activityIndex: number, _linkIndex: number): Promise<void> {
    return todo("assistantClickToolLink");
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
    return this.page
      .locator('[data-testid="assistant-rail"]')
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
    const texts = await this.assistantHomeCard().locator("[data-suggestion]").allTextContents();
    return texts.map((text) => text.trim()).filter((text) => text.length > 0);
  }
  async assistantCardRecents(): Promise<{ title: string; href: string }[]> {
    if (!(await this.assistantCardVisible())) return [];
    // Only the recents list counts: the keyless setup reminder is also an
    // anchor inside the card and must not displace the newest thread.
    const links = this.assistantHomeCard().locator("ul a");
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
    await this.assistantHomeCard().locator("ul a").nth(index).click();
  }
  async assistantStubStream(_threadId: string, _frames: AssistantStreamFrame[]): Promise<void> {
    return todo("assistantStubStream");
  }
  async assistantClearStreamStub(_threadId: string): Promise<void> {
    return todo("assistantClearStreamStub");
  }
  async assistantStreamRequestUrls(_threadId: string): Promise<string[]> {
    return todo("assistantStreamRequestUrls");
  }
  async assistantBlockStream(_threadId: string): Promise<void> {
    return todo("assistantBlockStream");
  }
  async assistantClearStreamBlock(_threadId: string): Promise<void> {
    return todo("assistantClearStreamBlock");
  }
  async assistantStartApiSpy(): Promise<void> {
    this.assistantSpyCounts = { threadCreate: 0, send: 0, cancel: 0, threadList: 0, messageList: 0 };
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
  async assistantDelaySend(_ms: number): Promise<void> {
    return todo("assistantDelaySend");
  }
  async assistantClearSendDelay(): Promise<void> {
    return todo("assistantClearSendDelay");
  }
  async assistantLastToast(): Promise<{ title: string; message: string } | null> {
    const candidates = this.page.locator('[data-testid="toast-notification"]');
    const count = await candidates.count();
    if (count === 0) return null;
    const lines = ((await candidates.last().innerText()).trim() || "")
      .split("\n")
      .map((line) => line.trim())
      .filter((line) => line.length > 0);
    if (lines.length === 0) return null;
    return { title: lines[0] ?? "", message: lines.slice(1).join(" ") };
  }
  async assistantCurrentHash(): Promise<string> {
    return todo("assistantCurrentHash");
  }
  async assistantMicDisabled(): Promise<boolean> {
    return todo("assistantMicDisabled");
  }
  async assistantMicHold(_ms: number): Promise<void> {
    return todo("assistantMicHold");
  }
  async assistantMicDown(): Promise<void> {
    return todo("assistantMicDown");
  }
  async assistantMicUp(): Promise<void> {
    return todo("assistantMicUp");
  }
  async assistantMicUpAfter(_ms: number): Promise<void> {
    return todo("assistantMicUpAfter");
  }
  async assistantSetMicrophonePermission(_state: "granted" | "denied"): Promise<void> {
    return todo("assistantSetMicrophonePermission");
  }
  async assistantSimulateUnsupportedCapture(): Promise<void> {
    return todo("assistantSimulateUnsupportedCapture");
  }
  async assistantSimulateMicDenial(): Promise<void> {
    return todo("assistantSimulateMicDenial");
  }
  async assistantStubTranscribeText(_text: string): Promise<void> {
    return todo("assistantStubTranscribeText");
  }
  async assistantFailTranscribe(_status: number, _body: Record<string, string>): Promise<void> {
    return todo("assistantFailTranscribe");
  }
  async assistantClearTranscribeStubs(): Promise<void> {
    return todo("assistantClearTranscribeStubs");
  }
  async assistantTranscribeRequests(): Promise<{ contentType: string; hasFilePart: boolean; byteLength: number }[]> {
    return todo("assistantTranscribeRequests");
  }
  async assistantSidebarButtons(): Promise<string[]> {
    return todo("assistantSidebarButtons");
  }
  async assistantThreadManagementControls(): Promise<string[]> {
    return todo("assistantThreadManagementControls");
  }
  async assistantSidebarHeader(): Promise<string | null> {
    return todo("assistantSidebarHeader");
  }
  async assistantSidebarRowKinds(): Promise<{ newChat: string | null; rows: string[] }> {
    return todo("assistantSidebarRowKinds");
  }
  async assistantSkippedNoticeActions(): Promise<{ kind: string; text: string; href: string | null }[]> {
    return todo("assistantSkippedNoticeActions");
  }
  async assistantChatSettingsLinks(): Promise<string[]> {
    return todo("assistantChatSettingsLinks");
  }
  async assistantStartDesktopCallWatch(): Promise<void> {
    return todo("assistantStartDesktopCallWatch");
  }
  async assistantDesktopCallsObserved(): Promise<{ method: string; url: string }[]> {
    return todo("assistantDesktopCallsObserved");
  }
  async assistantStopDesktopCallWatch(): Promise<void> {
    return todo("assistantStopDesktopCallWatch");
  }
  async assistantStubInstanceLlm(_configured: boolean): Promise<void> {
    return todo("assistantStubInstanceLlm");
  }
  async assistantClearInstanceStub(): Promise<void> {
    return todo("assistantClearInstanceStub");
  }
  async assistantStubGptAnswer(_response: { response: string; response_html: string }): Promise<void> {
    return todo("assistantStubGptAnswer");
  }
  async assistantFailGptAnswer(_status: number, _body: Record<string, string>): Promise<void> {
    return todo("assistantFailGptAnswer");
  }
  async assistantClearGptStubs(): Promise<void> {
    return todo("assistantClearGptStubs");
  }
  async assistantGptRequests(): Promise<{ prompt: string; task: string }[]> {
    return todo("assistantGptRequests");
  }
  async issueModalAiEntryVisible(): Promise<boolean> {
    return todo("issueModalAiEntryVisible");
  }
  async issueModalAiOpen(): Promise<void> {
    return todo("issueModalAiOpen");
  }
  async issueModalAiFillTask(_text: string): Promise<void> {
    return todo("issueModalAiFillTask");
  }
  async issueModalAiGenerate(): Promise<void> {
    return todo("issueModalAiGenerate");
  }
  async issueModalAiResponse(): Promise<string | null> {
    return todo("issueModalAiResponse");
  }
  async issueModalAiInvalidVisible(): Promise<boolean> {
    return todo("issueModalAiInvalidVisible");
  }
  async issueModalAiUseResponse(): Promise<void> {
    return todo("issueModalAiUseResponse");
  }
  async issueModalAiClose(): Promise<void> {
    return todo("issueModalAiClose");
  }
  async issueModalDescriptionText(): Promise<string | null> {
    return todo("issueModalDescriptionText");
  }
  async pageEditorOpen(_workspaceSlug: string, _projectId: string, _pageId: string): Promise<void> {
    return todo("pageEditorOpen");
  }
  async pageEditorAiHandleCount(): Promise<number> {
    return todo("pageEditorAiHandleCount");
  }
  async pageEditorAiMenuVisible(): Promise<boolean> {
    return todo("pageEditorAiMenuVisible");
  }
  async pageEditorRephraseRequests(): Promise<string[]> {
    return todo("pageEditorRephraseRequests");
  }

  // --- Notifications inbox foundation (NEWFRONT-198, NTF-001..006).
  // --- Throwing stubs per the shared driver contract; the notifications
  // --- area fills these in when it lands in apps/web_new.
  async notificationsOpenInbox(_workspaceSlug: string): Promise<void> {
    return todo("notificationsOpenInbox");
  }
  async notificationsListPaneVisible(): Promise<boolean> {
    return todo("notificationsListPaneVisible");
  }
  async notificationsDetailPaneVisible(): Promise<boolean> {
    return todo("notificationsDetailPaneVisible");
  }
  async notificationsPaneWidths(): Promise<{ list: number; detail: number }> {
    return todo("notificationsPaneWidths");
  }
  async notificationsSelectCard(_index: number): Promise<void> {
    return todo("notificationsSelectCard");
  }
  async notificationsTabNames(): Promise<string[]> {
    return todo("notificationsTabNames");
  }
  async notificationsActiveTab(): Promise<NotificationsTab> {
    return todo("notificationsActiveTab");
  }
  async notificationsSelectTab(_tab: NotificationsTab): Promise<void> {
    return todo("notificationsSelectTab");
  }
  async notificationsTabBadge(_tab: NotificationsTab): Promise<string | null> {
    return todo("notificationsTabBadge");
  }
  async notificationsNavBadge(): Promise<string | null> {
    return todo("notificationsNavBadge");
  }
  async notificationsProjectNavBadge(
    _workspaceSlug: string,
    _projectId: string,
    _cookies: ParityBrowserCookie[]
  ): Promise<string | null> {
    return todo("notificationsProjectNavBadge");
  }
  async notificationsCards(): Promise<NotificationsCard[]> {
    return todo("notificationsCards");
  }
  async notificationsCardBackgrounds(): Promise<string[]> {
    return todo("notificationsCardBackgrounds");
  }
  async notificationsEntryFetches(_workspaceSlug: string): Promise<{ list: boolean; unread: boolean }> {
    return todo("notificationsEntryFetches");
  }

  async deskRuntimePageText(): Promise<string> {
    return todo("deskRuntimePageText");
  }
  async deskRuntimeDispatchWindowFocus(): Promise<void> {
    return todo("deskRuntimeDispatchWindowFocus");
  }
  async deskRuntimeIndexedDatabaseNames(): Promise<string[]> {
    return todo("deskRuntimeIndexedDatabaseNames");
  }

  // --- Notifications snooze + email preferences (NEWFRONT-201, NTF-020..022,
  // --- NTF-024..025). Throwing stubs per the shared driver contract.
  async notificationsSnoozePresets(_index: number): Promise<string[]> {
    return todo("notificationsSnoozePresets");
  }
  async notificationsSnoozeWithPreset(_index: number, _preset: string): Promise<void> {
    return todo("notificationsSnoozeWithPreset");
  }
  async notificationsSnoozeRemovalOffered(_index: number): Promise<boolean> {
    return todo("notificationsSnoozeRemovalOffered");
  }
  async notificationsUnsnooze(_index: number): Promise<void> {
    return todo("notificationsUnsnooze");
  }
  async notificationsFailItemWrites(_status: number): Promise<void> {
    return todo("notificationsFailItemWrites");
  }
  async notificationsClearItemWriteFailure(): Promise<void> {
    return todo("notificationsClearItemWriteFailure");
  }
  async notificationsOpenCustomSnooze(_index: number): Promise<void> {
    return todo("notificationsOpenCustomSnooze");
  }
  async notificationsCustomSnoozeVisible(): Promise<boolean> {
    return todo("notificationsCustomSnoozeVisible");
  }
  async notificationsCustomSnoozePickDay(_offsetDays: number): Promise<void> {
    return todo("notificationsCustomSnoozePickDay");
  }
  async notificationsCustomSnoozeTimeSlots(_period: "AM" | "PM"): Promise<string[]> {
    return todo("notificationsCustomSnoozeTimeSlots");
  }
  async notificationsCustomSnoozePickTime(_period: "AM" | "PM", _slot: string): Promise<void> {
    return todo("notificationsCustomSnoozePickTime");
  }
  async notificationsCustomSnoozeSubmit(): Promise<void> {
    return todo("notificationsCustomSnoozeSubmit");
  }
  async notificationsSetSnoozedMode(_on: boolean): Promise<void> {
    return todo("notificationsSetSnoozedMode");
  }
  // --- Notifications email preferences (NEWFRONT-575, NTF-024/025/031).
  // --- The settings page mounts one switch per topic behind a loader;
  // --- switches carry their topic heading as the accessible name and the
  // --- completed-only refinement sits in its own indented nest.

  /** Accessible names of the email toggles, keyed by preference. */
  private static readonly emailPrefNames: Record<NotificationsEmailPref, string> = {
    property_change: "Property changes",
    state_change: "State changes",
    issue_completed: "Completed items only",
    comment: "Comments",
    mention: "Mentions",
  };

  private emailPrefSwitch(pref: NotificationsEmailPref): Locator {
    return this.page
      .getByTestId("email-prefs-form")
      .first()
      .getByRole("switch", { name: WebNewDriver.emailPrefNames[pref], exact: true });
  }

  async notificationsOpenEmailPreferences(): Promise<void> {
    await this.page.goto("/settings/profile/notifications", { timeout: 60_000 });
    await this.page.waitForLoadState("domcontentloaded");
    await this.page
      .getByTestId("email-prefs-form")
      .first()
      .getByRole("switch")
      .first()
      .waitFor({ timeout: WebNewDriver.WAIT_MS });
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
        .waitFor({ timeout: WebNewDriver.WAIT_MS });
      return seen;
    } finally {
      await this.page.unroute(pattern);
    }
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
      out[pref] = (await this.emailPrefSwitch(pref).getAttribute("aria-checked")) === "true";
    }
    return out;
  }

  async notificationsEmailPreferencesToggle(pref: NotificationsEmailPref): Promise<void> {
    const saveWait = this.page.waitForResponse(
      (response) =>
        response.request().method() === "PATCH" && response.url().includes("/users/me/notification-preferences/"),
      { timeout: WebNewDriver.WAIT_MS }
    );
    await this.emailPrefSwitch(pref).click({ timeout: WebNewDriver.WAIT_MS });
    await saveWait;
  }

  async notificationsEmailPreferencesCompletedNested(): Promise<boolean> {
    const nest = this.page.getByTestId("email-prefs-completed-nest").first();
    if ((await nest.getByRole("switch").count()) !== 1) return false;
    // Both rows spread their switch to the trailing edge, so the switches
    // sit at the same x; the nesting shows in the headings instead.
    const form = this.page.getByTestId("email-prefs-form").first();
    const stateBox = await form
      .getByRole("heading", { name: WebNewDriver.emailPrefNames.state_change, exact: true })
      .boundingBox()
      .catch(() => null);
    const completedBox = await nest
      .getByRole("heading", { name: WebNewDriver.emailPrefNames.issue_completed, exact: true })
      .boundingBox()
      .catch(() => null);
    if (stateBox === null || completedBox === null) return false;
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

  async notificationsEntryListQuery(_workspaceSlug: string): Promise<NotificationsListQuery> {
    return todo("notificationsEntryListQuery");
  }
  async notificationsOpenFilterMenu(): Promise<void> {
    return todo("notificationsOpenFilterMenu");
  }
  async notificationsFilterOptions(): Promise<NotificationsFilterOption[]> {
    return todo("notificationsFilterOptions");
  }
  async notificationsToggleFilterOrigin(_origin: NotificationsOrigin): Promise<NotificationsListQuery> {
    return todo("notificationsToggleFilterOrigin");
  }
  async notificationsAppliedChips(): Promise<NotificationsAppliedChip[]> {
    return todo("notificationsAppliedChips");
  }
  async notificationsRemoveFilterChip(_origin: NotificationsOrigin): Promise<NotificationsListQuery> {
    return todo("notificationsRemoveFilterChip");
  }
  async notificationsClearFilters(): Promise<NotificationsListQuery> {
    return todo("notificationsClearFilters");
  }
  async notificationsCloseMenus(): Promise<void> {
    return todo("notificationsCloseMenus");
  }
  async notificationsOpenOverflowMenu(): Promise<void> {
    return todo("notificationsOpenOverflowMenu");
  }
  async notificationsOverflowOptions(): Promise<string[]> {
    return todo("notificationsOverflowOptions");
  }
  async notificationsToggleMode(_mode: NotificationsMode): Promise<NotificationsListQuery> {
    return todo("notificationsToggleMode");
  }
  async notificationsCardActionsVisible(_index: number): Promise<boolean> {
    return todo("notificationsCardActionsVisible");
  }
  async notificationsHoverCard(_index: number): Promise<void> {
    return todo("notificationsHoverCard");
  }
  async notificationsToggleCardRead(_index: number): Promise<void> {
    return todo("notificationsToggleCardRead");
  }
  async notificationsToggleCardArchive(_index: number): Promise<void> {
    return todo("notificationsToggleCardArchive");
  }
  async notificationsFailNextCardWrite(): Promise<void> {
    return todo("notificationsFailNextCardWrite");
  }
  // --- Archived cycles (NEWFRONT-224, ARCH-014–019). Oracle-stage stubs;
  // --- the new-app implementation lands with the area.
  async archivesCyclesOpenTabViaLive(_workspaceSlug: string, _projectId: string): Promise<void> {
    return todo("archivesCyclesOpenTabViaLive");
  }
  async archivesCyclesOpenTabRaw(_workspaceSlug: string, _projectId: string): Promise<void> {
    return todo("archivesCyclesOpenTabRaw");
  }
  async archivesCyclesSkeletonVisible(): Promise<boolean> {
    return todo("archivesCyclesSkeletonVisible");
  }
  async archivesCyclesOpenLive(_workspaceSlug: string, _projectId: string): Promise<void> {
    return todo("archivesCyclesOpenLive");
  }
  async archivesCyclesReload(): Promise<void> {
    return todo("archivesCyclesReload");
  }
  async archivesCyclesVisibleNames(): Promise<string[]> {
    return todo("archivesCyclesVisibleNames");
  }
  async archivesCyclesLiveGroupOpen(_section: string): Promise<void> {
    return todo("archivesCyclesLiveGroupOpen");
  }
  async archivesCyclesGroupHeadings(): Promise<string[]> {
    return todo("archivesCyclesGroupHeadings");
  }
  async archivesCyclesOpenPeek(_name: string): Promise<void> {
    return todo("archivesCyclesOpenPeek");
  }
  async archivesCyclesOpenPeekLink(_workspaceSlug: string, _projectId: string, _cycleId: string): Promise<void> {
    return todo("archivesCyclesOpenPeekLink");
  }
  async archivesCyclesPeekName(): Promise<string | null> {
    return todo("archivesCyclesPeekName");
  }
  async archivesCyclesPeekParam(): Promise<string | null> {
    return todo("archivesCyclesPeekParam");
  }
  async archivesCyclesClosePeek(): Promise<void> {
    return todo("archivesCyclesClosePeek");
  }
  async archivesCyclesSearchExpanded(): Promise<boolean> {
    return todo("archivesCyclesSearchExpanded");
  }
  async archivesCyclesSearchOpen(): Promise<void> {
    return todo("archivesCyclesSearchOpen");
  }
  async archivesCyclesSearchFocused(): Promise<boolean> {
    return todo("archivesCyclesSearchFocused");
  }
  async archivesCyclesSearchFill(_text: string): Promise<void> {
    return todo("archivesCyclesSearchFill");
  }
  async archivesCyclesSearchText(): Promise<string> {
    return todo("archivesCyclesSearchText");
  }
  async archivesCyclesSearchEscape(): Promise<void> {
    return todo("archivesCyclesSearchEscape");
  }
  async archivesCyclesSearchClear(): Promise<void> {
    return todo("archivesCyclesSearchClear");
  }
  async archivesCyclesClickAway(): Promise<void> {
    return todo("archivesCyclesClickAway");
  }
  async archivesCyclesFiltersOpen(): Promise<void> {
    return todo("archivesCyclesFiltersOpen");
  }
  async archivesCyclesFiltersClose(): Promise<void> {
    return todo("archivesCyclesFiltersClose");
  }
  async archivesCyclesFilterSections(): Promise<string[]> {
    return todo("archivesCyclesFilterSections");
  }
  async archivesCyclesFilterOptionNames(): Promise<string[]> {
    return todo("archivesCyclesFilterOptionNames");
  }
  async archivesCyclesFilterPick(_section: string, _optionName: string): Promise<void> {
    return todo("archivesCyclesFilterPick");
  }
  async archivesCyclesFilterChipTexts(): Promise<string[]> {
    return todo("archivesCyclesFilterChipTexts");
  }
  async archivesCyclesFilterRemoveChip(_chipText: string): Promise<void> {
    return todo("archivesCyclesFilterRemoveChip");
  }
  async archivesCyclesFiltersClearAll(): Promise<void> {
    return todo("archivesCyclesFiltersClearAll");
  }
  async archivesCyclesFiltersActive(): Promise<boolean> {
    return todo("archivesCyclesFiltersActive");
  }
  async archivesCyclesEmptyHeading(): Promise<string | null> {
    return todo("archivesCyclesEmptyHeading");
  }
  async archivesCyclesNoMatchHint(): Promise<string | null> {
    return todo("archivesCyclesNoMatchHint");
  }
  async archivesCyclesSkeletonShownOnSlowFetchViaLive(_workspaceSlug: string, _projectId: string): Promise<boolean> {
    return todo("archivesCyclesSkeletonShownOnSlowFetchViaLive");
  }
  async archivesCyclesOpenRowMenu(_name: string): Promise<void> {
    return todo("archivesCyclesOpenRowMenu");
  }
  async archivesCyclesMenuEntries(): Promise<{ title: string; disabled: boolean; description: string | null }[]> {
    return todo("archivesCyclesMenuEntries");
  }
  async archivesCyclesMenuPick(_title: string): Promise<void> {
    return todo("archivesCyclesMenuPick");
  }
  async archivesCyclesArchiveDialogText(): Promise<{ heading: string; body: string } | null> {
    return todo("archivesCyclesArchiveDialogText");
  }
  async archivesCyclesArchiveDialogConfirm(): Promise<void> {
    return todo("archivesCyclesArchiveDialogConfirm");
  }
  async archivesCyclesArchiveDialogCancel(): Promise<void> {
    return todo("archivesCyclesArchiveDialogCancel");
  }

  // --- Notifications detail, pagination, refresh, mark-all-read
  // --- (NEWFRONT-199, NTF-007..014). Stubs; the new app implements them.

  async notificationsDetailVariant(): Promise<NotificationsDetailVariant> {
    return todo("notificationsDetailVariant");
  }

  async notificationsDetailText(): Promise<string> {
    return todo("notificationsDetailText");
  }

  async notificationsCloseDetail(): Promise<void> {
    return todo("notificationsCloseDetail");
  }

  async notificationsSelectCardPostedRead(_index: number): Promise<boolean> {
    return todo("notificationsSelectCardPostedRead");
  }

  async notificationsSelectCardHeldAccess(_index: number, _holdMs: number): Promise<{ spinnerShown: boolean }> {
    return todo("notificationsSelectCardHeldAccess");
  }

  async notificationsNextPageLabel(): Promise<string | null> {
    return todo("notificationsNextPageLabel");
  }

  async notificationsLoadNextPage(): Promise<void> {
    return todo("notificationsLoadNextPage");
  }

  async notificationsLoadNextPageHeld(
    _holdMs: number
  ): Promise<{ loadingShown: boolean; before: number; after: number }> {
    return todo("notificationsLoadNextPageHeld");
  }

  async notificationsSkeletonOnDelayedEntry(
    _workspaceSlug: string,
    _holdMs: number
  ): Promise<{ skeletonShown: boolean; settledCards: number }> {
    return todo("notificationsSkeletonOnDelayedEntry");
  }

  async notificationsEmptyText(): Promise<string | null> {
    return todo("notificationsEmptyText");
  }

  async notificationsRefresh(): Promise<void> {
    return todo("notificationsRefresh");
  }

  async notificationsRefreshHeld(_holdMs: number): Promise<{ spinning: boolean; requests: string[] }> {
    return todo("notificationsRefreshHeld");
  }

  async notificationsMarkAllRead(): Promise<void> {
    return todo("notificationsMarkAllRead");
  }

  async notificationsMarkAllReadHeld(
    _holdMs: number
  ): Promise<{ progress: boolean; requests: number; scopeBody: string | null }> {
    return todo("notificationsMarkAllReadHeld");
  }

  // --- Archived modules (NEWFRONT-225, ARCH-020..025). Throwing stubs;
  // --- the new-app area issue implements them against apps/web_new.
  async archivesOpenModulesTab(_workspaceSlug: string, _projectId: string): Promise<void> {
    return todo("archivesOpenModulesTab");
  }
  async archivesOpenCyclesTab(_workspaceSlug: string, _projectId: string): Promise<void> {
    return todo("archivesOpenCyclesTab");
  }
  async archivesGoToProjectArchives(_projectId: string): Promise<void> {
    return todo("archivesGoToProjectArchives");
  }
  async archivesSelectArchivesTab(_tab: "Modules" | "Cycles"): Promise<void> {
    return todo("archivesSelectArchivesTab");
  }
  async archivesClientNavigate(_path: string): Promise<void> {
    return todo("archivesClientNavigate");
  }
  async archivesOpenLiveModulePeek(_name: string): Promise<void> {
    return todo("archivesOpenLiveModulePeek");
  }
  async archivesOpenLiveModules(_workspaceSlug: string, _projectId: string): Promise<void> {
    return todo("archivesOpenLiveModules");
  }
  async archivesOpenLiveCycles(_workspaceSlug: string, _projectId: string): Promise<void> {
    return todo("archivesOpenLiveCycles");
  }
  async archivesDirectLoadRenders(_workspaceSlug: string, _projectId: string): Promise<boolean> {
    return todo("archivesDirectLoadRenders");
  }
  async archivesAwaitModuleRow(_name: string): Promise<void> {
    return todo("archivesAwaitModuleRow");
  }
  async archivesModuleRowNames(): Promise<string[]> {
    return todo("archivesModuleRowNames");
  }
  async archivesCycleRowNames(): Promise<string[]> {
    return todo("archivesCycleRowNames");
  }
  async archivesModuleSortLabel(): Promise<string> {
    return todo("archivesModuleSortLabel");
  }
  async archivesSetModuleSort(_label: string): Promise<void> {
    return todo("archivesSetModuleSort");
  }
  async archivesOpenModuleSearch(): Promise<void> {
    return todo("archivesOpenModuleSearch");
  }
  async archivesModuleSearchVisible(): Promise<boolean> {
    return todo("archivesModuleSearchVisible");
  }
  async archivesTypeModuleSearch(_text: string): Promise<void> {
    return todo("archivesTypeModuleSearch");
  }
  async archivesModuleSearchText(): Promise<string> {
    return todo("archivesModuleSearchText");
  }
  async archivesEscapeModuleSearch(): Promise<void> {
    return todo("archivesEscapeModuleSearch");
  }
  async archivesClearModuleSearch(): Promise<void> {
    return todo("archivesClearModuleSearch");
  }
  async archivesCollapseSearchOutside(): Promise<void> {
    return todo("archivesCollapseSearchOutside");
  }
  async archivesOpenModuleFilters(): Promise<void> {
    return todo("archivesOpenModuleFilters");
  }
  async archivesModuleFilterGroups(): Promise<string[]> {
    return todo("archivesModuleFilterGroups");
  }
  async archivesToggleLeadFilter(_label: string): Promise<void> {
    return todo("archivesToggleLeadFilter");
  }
  async archivesModuleChips(): Promise<ArchivesModuleChip[]> {
    return todo("archivesModuleChips");
  }
  async archivesRemoveModuleChip(_key: string): Promise<void> {
    return todo("archivesRemoveModuleChip");
  }
  async archivesClearModuleFilters(): Promise<void> {
    return todo("archivesClearModuleFilters");
  }
  async archivesModuleFiltersActive(): Promise<boolean> {
    return todo("archivesModuleFiltersActive");
  }
  async archivesCloseModuleFilters(): Promise<void> {
    return todo("archivesCloseModuleFilters");
  }
  async archivesModulesEmptyKind(): Promise<ArchivesModulesEmptyKind> {
    return todo("archivesModulesEmptyKind");
  }
  async archivesModulesShowsSkeleton(_workspaceSlug: string, _projectId: string): Promise<boolean> {
    return todo("archivesModulesShowsSkeleton");
  }
  async archivesOpenModulePeek(_name: string): Promise<void> {
    return todo("archivesOpenModulePeek");
  }
  async archivesModulePeekName(): Promise<string | null> {
    return todo("archivesModulePeekName");
  }
  async archivesCloseModulePeek(): Promise<void> {
    return todo("archivesCloseModulePeek");
  }
  async archivesOpenCyclePeek(_name: string): Promise<void> {
    return todo("archivesOpenCyclePeek");
  }
  async archivesCyclePeekName(): Promise<string | null> {
    return todo("archivesCyclePeekName");
  }
  async archivesCloseCyclePeek(): Promise<void> {
    return todo("archivesCloseCyclePeek");
  }
  async archivesModulePeekReadOnly(): Promise<ArchivesPeekReadOnly> {
    return todo("archivesModulePeekReadOnly");
  }
  async archivesCyclePeekReadOnly(): Promise<ArchivesPeekReadOnly> {
    return todo("archivesCyclePeekReadOnly");
  }
  async archivesLiveModuleMenuEntries(_name: string): Promise<ArchivesMenuEntry[]> {
    return todo("archivesLiveModuleMenuEntries");
  }
  async archivesChooseLiveModuleMenuEntry(_name: string, _title: string): Promise<void> {
    return todo("archivesChooseLiveModuleMenuEntry");
  }
  async archivesArchivedModuleMenuEntries(_name: string): Promise<ArchivesMenuEntry[]> {
    return todo("archivesArchivedModuleMenuEntries");
  }
  async archivesChooseArchivedModuleMenuEntry(_name: string, _title: string): Promise<void> {
    return todo("archivesChooseArchivedModuleMenuEntry");
  }
  async archivesArchiveDialog(): Promise<ArchivesArchiveDialog | null> {
    return todo("archivesArchiveDialog");
  }
  async archivesConfirmArchiveDialog(): Promise<void> {
    return todo("archivesConfirmArchiveDialog");
  }
  async archivesCancelArchiveDialog(): Promise<void> {
    return todo("archivesCancelArchiveDialog");
  }
  async archivesFailNextModuleWrite(): Promise<void> {
    return todo("archivesFailNextModuleWrite");
  }

  // Archived work-items list, filters, display, peek (NEWFRONT-222,
  // ARCH-001..007). Oracle stage: web_new has no archives screens yet.
  async archivesOpenIssuesList(_workspaceSlug: string, _projectId: string): Promise<void> {
    return todo("archivesOpenIssuesList");
  }
  async archivesOpenTab(_tab: "issues" | "cycles" | "modules"): Promise<void> {
    return todo("archivesOpenTab");
  }
  async archivesTabNames(): Promise<string[]> {
    return todo("archivesTabNames");
  }
  async archivesActiveTab(): Promise<string> {
    return todo("archivesActiveTab");
  }
  async archivesBackPresent(): Promise<boolean> {
    return todo("archivesBackPresent");
  }
  async archivesClickBack(): Promise<void> {
    return todo("archivesClickBack");
  }
  async archivesCountBadge(): Promise<string | null> {
    return todo("archivesCountBadge");
  }
  async archivesCountBadgeTooltip(): Promise<string | null> {
    return todo("archivesCountBadgeTooltip");
  }
  async archivesPageTitle(): Promise<string> {
    return todo("archivesPageTitle");
  }
  async archivesVisibleIssueNames(): Promise<string[]> {
    return todo("archivesVisibleIssueNames");
  }
  async archivesGroupHeadings(): Promise<string[]> {
    return todo("archivesGroupHeadings");
  }
  async archivesOpenRowMenu(_name: string): Promise<void> {
    return todo("archivesOpenRowMenu");
  }
  async archivesRowMenuEntries(): Promise<string[]> {
    return todo("archivesRowMenuEntries");
  }
  async archivesCloseMenus(): Promise<void> {
    return todo("archivesCloseMenus");
  }
  async archivesInlineEditorOpens(_name: string): Promise<boolean> {
    return todo("archivesInlineEditorOpens");
  }
  async archivesWaitForListQuery(): Promise<ArchivesListQuery> {
    return todo("archivesWaitForListQuery");
  }
  async archivesRowText(_name: string): Promise<string> {
    return todo("archivesRowText");
  }
  async archivesPeekTitle(): Promise<string | null> {
    return todo("archivesPeekTitle");
  }
  async archivesPeekTitleLocked(): Promise<boolean> {
    return todo("archivesPeekTitleLocked");
  }
  async archivesPeekDescriptionText(): Promise<string> {
    return todo("archivesPeekDescriptionText");
  }
  async archivesPeekDescriptionEditable(): Promise<boolean> {
    return todo("archivesPeekDescriptionEditable");
  }
  async archivesPeekActivityEditable(): Promise<boolean> {
    return todo("archivesPeekActivityEditable");
  }
  async archivesPeekQueryParams(): Promise<{ issue: string | null; project: string | null; nesting: string | null }> {
    return todo("archivesPeekQueryParams");
  }
  async archivesSeedStoredExpression(
    _workspaceSlug: string,
    _projectId: string,
    _expression: ArchivesFilterExpression
  ): Promise<void> {
    return todo("archivesSeedStoredExpression");
  }

  async viewsListNames(): Promise<string[]> {
    return todo("viewsListNames");
  }
  async viewsListBreadcrumb(): Promise<string[]> {
    return todo("viewsListBreadcrumb");
  }
  async viewsListTabTitle(): Promise<string> {
    return todo("viewsListTabTitle");
  }
  async viewsHeaderAddVisible(): Promise<boolean> {
    return todo("viewsHeaderAddVisible");
  }
  async viewsOpenCreateFromHeader(): Promise<void> {
    return todo("viewsOpenCreateFromHeader");
  }
  async viewsListSkeletonVisible(): Promise<boolean> {
    return todo("viewsListSkeletonVisible");
  }
  async viewsDelayListLoad(_ms: number): Promise<void> {
    return todo("viewsDelayListLoad");
  }
  async viewsEmptyTitle(): Promise<string> {
    return todo("viewsEmptyTitle");
  }
  async viewsEmptyCreateVisible(): Promise<boolean> {
    return todo("viewsEmptyCreateVisible");
  }
  async viewsEmptyCreateEnabled(): Promise<boolean> {
    return todo("viewsEmptyCreateEnabled");
  }
  async viewsEmptyCreateOpen(): Promise<void> {
    return todo("viewsEmptyCreateOpen");
  }
  async viewsNoMatchTitle(): Promise<string> {
    return todo("viewsNoMatchTitle");
  }
  async viewsRowHref(_name: string): Promise<string | null> {
    return todo("viewsRowHref");
  }
  async viewsRowAccess(_name: string): Promise<string> {
    return todo("viewsRowAccess");
  }
  async viewsRowOwnerAvatar(_name: string): Promise<boolean> {
    return todo("viewsRowOwnerAvatar");
  }
  async viewsRowLiveVisible(_name: string): Promise<boolean> {
    return todo("viewsRowLiveVisible");
  }
  async viewsRowStarVisible(_name: string): Promise<boolean> {
    return todo("viewsRowStarVisible");
  }
  async viewsRowStarSelected(_name: string): Promise<boolean> {
    return todo("viewsRowStarSelected");
  }
  async viewsToggleStar(_name: string): Promise<void> {
    return todo("viewsToggleStar");
  }
  async viewsRowText(_name: string): Promise<string> {
    return todo("viewsRowText");
  }
  async viewsSearchTriggerVisible(): Promise<boolean> {
    return todo("viewsSearchTriggerVisible");
  }
  async viewsSearchOpen(): Promise<void> {
    return todo("viewsSearchOpen");
  }
  async viewsSearchExpanded(): Promise<boolean> {
    return todo("viewsSearchExpanded");
  }
  async viewsSearchType(_text: string): Promise<void> {
    return todo("viewsSearchType");
  }
  async viewsSearchValue(): Promise<string> {
    return todo("viewsSearchValue");
  }
  async viewsSearchFocused(): Promise<boolean> {
    return todo("viewsSearchFocused");
  }
  async viewsSearchEscape(): Promise<void> {
    return todo("viewsSearchEscape");
  }
  async viewsSearchClear(): Promise<void> {
    return todo("viewsSearchClear");
  }
  async viewsSearchClickOutside(): Promise<void> {
    return todo("viewsSearchClickOutside");
  }
  async viewsSortTriggerText(): Promise<string> {
    return todo("viewsSortTriggerText");
  }
  async viewsSortOpen(): Promise<void> {
    return todo("viewsSortOpen");
  }
  async viewsSortMenuTexts(): Promise<string[]> {
    return todo("viewsSortMenuTexts");
  }
  async viewsSortMenuSelected(_text: string): Promise<boolean> {
    return todo("viewsSortMenuSelected");
  }
  async viewsSortPick(_text: string): Promise<void> {
    return todo("viewsSortPick");
  }
  async viewsFiltersOpen(): Promise<void> {
    return todo("viewsFiltersOpen");
  }
  async viewsFiltersPanelText(): Promise<string> {
    return todo("viewsFiltersPanelText");
  }
  async viewsFiltersToggleFavorites(): Promise<void> {
    return todo("viewsFiltersToggleFavorites");
  }
  async viewsFiltersDateOptions(): Promise<string[]> {
    return todo("viewsFiltersDateOptions");
  }
  async viewsFiltersPickDate(_text: string): Promise<void> {
    return todo("viewsFiltersPickDate");
  }
  async viewsFiltersCreatorOptions(): Promise<string[]> {
    return todo("viewsFiltersCreatorOptions");
  }
  async viewsFiltersPickCreator(_name: string): Promise<void> {
    return todo("viewsFiltersPickCreator");
  }
  async viewsFiltersAccessPresent(): Promise<boolean> {
    return todo("viewsFiltersAccessPresent");
  }
  async viewsFiltersSearchType(_text: string): Promise<void> {
    return todo("viewsFiltersSearchType");
  }
  async viewsFiltersClose(): Promise<void> {
    return todo("viewsFiltersClose");
  }
  async viewsChipsVisible(): Promise<boolean> {
    return todo("viewsChipsVisible");
  }
  async viewsChipTexts(): Promise<string[]> {
    return todo("viewsChipTexts");
  }
  async viewsChipRemoveValue(_dimension: string, _value: string): Promise<void> {
    return todo("viewsChipRemoveValue");
  }
  async viewsChipRemoveDimension(_dimension: string): Promise<void> {
    return todo("viewsChipRemoveDimension");
  }
  async viewsChipsClearAll(): Promise<void> {
    return todo("viewsChipsClearAll");
  }
  async viewsGateTitle(): Promise<string> {
    return todo("viewsGateTitle");
  }
  async viewsGateManageVisible(): Promise<boolean> {
    return todo("viewsGateManageVisible");
  }
  async viewsGateManageEnabled(): Promise<boolean> {
    return todo("viewsGateManageEnabled");
  }
  async viewsGateManageOpen(): Promise<void> {
    return todo("viewsGateManageOpen");
  }
  async viewsDialogHeading(): Promise<string | null> {
    return todo("viewsDialogHeading");
  }
  async viewsDialogFillTitle(_text: string): Promise<void> {
    return todo("viewsDialogFillTitle");
  }
  async viewsDialogTitleValue(): Promise<string> {
    return todo("viewsDialogTitleValue");
  }
  async viewsDialogTitleError(): Promise<string> {
    return todo("viewsDialogTitleError");
  }
  async viewsDialogFillDescription(_text: string): Promise<void> {
    return todo("viewsDialogFillDescription");
  }
  async viewsDialogDescriptionValue(): Promise<string> {
    return todo("viewsDialogDescriptionValue");
  }
  async viewsDialogAccessPresent(): Promise<boolean> {
    return todo("viewsDialogAccessPresent");
  }
  async viewsDialogIconOpen(): Promise<void> {
    return todo("viewsDialogIconOpen");
  }
  async viewsDialogIconTabs(): Promise<string[]> {
    return todo("viewsDialogIconTabs");
  }
  async viewsDialogPickFirstIcon(): Promise<void> {
    return todo("viewsDialogPickFirstIcon");
  }
  async viewsDialogIconPreview(): Promise<string> {
    return todo("viewsDialogIconPreview");
  }
  async viewsDialogDisplayOpen(): Promise<void> {
    return todo("viewsDialogDisplayOpen");
  }
  async viewsDialogDisplayTexts(): Promise<string[]> {
    return todo("viewsDialogDisplayTexts");
  }
  async viewsDialogFiltersExpanded(): Promise<boolean> {
    return todo("viewsDialogFiltersExpanded");
  }
  async viewsDialogLayoutValue(): Promise<string> {
    return todo("viewsDialogLayoutValue");
  }
  async viewsDialogPickLayout(_label: string): Promise<void> {
    return todo("viewsDialogPickLayout");
  }
  async viewsDialogEscape(): Promise<void> {
    return todo("viewsDialogEscape");
  }
  async viewsDialogCancel(): Promise<void> {
    return todo("viewsDialogCancel");
  }
  async viewsDialogSubmit(): Promise<void> {
    return todo("viewsDialogSubmit");
  }
  async viewsDialogSubmitAttempt(): Promise<void> {
    return todo("viewsDialogSubmitAttempt");
  }
  async viewsDialogOpen(): Promise<boolean> {
    return todo("viewsDialogOpen");
  }
  async viewsFailNextWrite(_status: number): Promise<void> {
    return todo("viewsFailNextWrite");
  }
  async viewsRowMenuOpen(_name: string): Promise<void> {
    return todo("viewsRowMenuOpen");
  }
  async viewsRowMenuItems(): Promise<string[]> {
    return todo("viewsRowMenuItems");
  }
  async viewsRowMenuPick(_item: string): Promise<void> {
    return todo("viewsRowMenuPick");
  }
  async viewsRowCopyLink(_name: string): Promise<string> {
    return todo("viewsRowCopyLink");
  }
  async viewsRowMenuPublishPresent(): Promise<boolean> {
    return todo("viewsRowMenuPublishPresent");
  }
  async viewsRowOpenNewTabHref(_name: string): Promise<string> {
    return todo("viewsRowOpenNewTabHref");
  }
  async viewsDeleteTitle(): Promise<string> {
    return todo("viewsDeleteTitle");
  }
  async viewsDeleteBody(): Promise<string> {
    return todo("viewsDeleteBody");
  }
  async viewsDeleteConfirm(): Promise<void> {
    return todo("viewsDeleteConfirm");
  }
  async viewsDeleteConfirmAttempt(): Promise<void> {
    return todo("viewsDeleteConfirmAttempt");
  }
  async viewsDeleteCancel(): Promise<void> {
    return todo("viewsDeleteCancel");
  }
  async viewsDetailBreadcrumb(): Promise<string[]> {
    return todo("viewsDetailBreadcrumb");
  }
  async viewsDetailErrorTitle(): Promise<string> {
    return todo("viewsDetailErrorTitle");
  }
  async viewsDetailErrorBack(): Promise<void> {
    return todo("viewsDetailErrorBack");
  }
  async viewsDetailSwitcherOpen(_name: string): Promise<void> {
    return todo("viewsDetailSwitcherOpen");
  }
  async viewsDetailSwitcherOptions(): Promise<string[]> {
    return todo("viewsDetailSwitcherOptions");
  }
  async viewsDetailSwitcherSearchVisible(): Promise<boolean> {
    return todo("viewsDetailSwitcherSearchVisible");
  }
  async viewsDetailSwitcherSearch(_text: string): Promise<void> {
    return todo("viewsDetailSwitcherSearch");
  }
  async viewsDetailSwitcherPick(_name: string): Promise<void> {
    return todo("viewsDetailSwitcherPick");
  }
  async viewsDetailLockVisible(): Promise<boolean> {
    return todo("viewsDetailLockVisible");
  }
  async viewsDetailDisplayOptions(): Promise<string[]> {
    return todo("viewsDetailDisplayOptions");
  }
  async viewsDetailFilterAdd(_property: string, _value: string): Promise<void> {
    return todo("viewsDetailFilterAdd");
  }
  async viewsDetailUpdateView(): Promise<void> {
    return todo("viewsDetailUpdateView");
  }
  async viewsDetailSaveAsVisible(): Promise<boolean> {
    return todo("viewsDetailSaveAsVisible");
  }
  async viewsDetailSaveAsClick(): Promise<void> {
    return todo("viewsDetailSaveAsClick");
  }
  async viewsDetailAddClick(): Promise<void> {
    return todo("viewsDetailAddClick");
  }
  async viewsDetailShowsIssue(_name: string): Promise<boolean> {
    return todo("viewsDetailShowsIssue");
  }
  async wsViewsListNames(): Promise<string[]> {
    return todo("wsViewsListNames");
  }
  async wsViewsDefaultNames(): Promise<string[]> {
    return todo("wsViewsDefaultNames");
  }
  async wsViewsListTabTitle(): Promise<string> {
    return todo("wsViewsListTabTitle");
  }
  async wsViewsSearchFill(_text: string): Promise<void> {
    return todo("wsViewsSearchFill");
  }
  async wsViewsRowDescription(_name: string): Promise<string> {
    return todo("wsViewsRowDescription");
  }
  async wsViewsRowMenuOpen(_name: string): Promise<void> {
    return todo("wsViewsRowMenuOpen");
  }
  async wsViewsRowMenuEntries(): Promise<string[]> {
    return todo("wsViewsRowMenuEntries");
  }
  async wsViewsRowMenuPick(_entry: string): Promise<void> {
    return todo("wsViewsRowMenuPick");
  }
  async wsViewsSkeletonVisible(): Promise<boolean> {
    return todo("wsViewsSkeletonVisible");
  }
  async wsViewsSkeletonFlashed(_windowMs: number): Promise<boolean> {
    return todo("wsViewsSkeletonFlashed");
  }
  async wsViewsDelayNextList(_ms: number): Promise<void> {
    return todo("wsViewsDelayNextList");
  }
  async wsViewsFailNextWrite(_status: number): Promise<void> {
    return todo("wsViewsFailNextWrite");
  }
  async wsViewsStripVisible(): Promise<boolean> {
    return todo("wsViewsStripVisible");
  }
  async wsViewsListStarVisible(): Promise<boolean> {
    return todo("wsViewsListStarVisible");
  }
  async wsViewsDefaultRowHasMenu(_name: string): Promise<boolean> {
    return todo("wsViewsDefaultRowHasMenu");
  }
  async wsViewsRowOpen(_name: string): Promise<void> {
    return todo("wsViewsRowOpen");
  }
  async wsViewsDialogDisplayOptions(): Promise<string[]> {
    return todo("wsViewsDialogDisplayOptions");
  }
  async wsViewsDetailTabTitle(): Promise<string> {
    return todo("wsViewsDetailTabTitle");
  }
  async wsViewsDetailCrumbs(): Promise<string[]> {
    return todo("wsViewsDetailCrumbs");
  }
  async wsViewsDetailSwitcherOpen(_name: string): Promise<void> {
    return todo("wsViewsDetailSwitcherOpen");
  }
  async wsViewsDetailSwitcherOptions(): Promise<string[]> {
    return todo("wsViewsDetailSwitcherOptions");
  }
  async wsViewsDetailSwitcherSearchVisible(): Promise<boolean> {
    return todo("wsViewsDetailSwitcherSearchVisible");
  }
  async wsViewsDetailSwitcherSearch(_text: string): Promise<void> {
    return todo("wsViewsDetailSwitcherSearch");
  }
  async wsViewsDetailSwitcherPick(_name: string): Promise<void> {
    return todo("wsViewsDetailSwitcherPick");
  }
  async wsViewsDetailLayoutVisible(): Promise<boolean> {
    return todo("wsViewsDetailLayoutVisible");
  }
  async wsViewsDetailDisplayVisible(): Promise<boolean> {
    return todo("wsViewsDetailDisplayVisible");
  }
  async wsViewsDetailFiltersToggleVisible(): Promise<boolean> {
    return todo("wsViewsDetailFiltersToggleVisible");
  }
  async wsViewsDetailAddVisible(): Promise<boolean> {
    return todo("wsViewsDetailAddVisible");
  }
  async wsViewsDetailAddClick(): Promise<void> {
    return todo("wsViewsDetailAddClick");
  }
  async wsViewsDetailMenuOpen(): Promise<void> {
    return todo("wsViewsDetailMenuOpen");
  }
  async wsViewsDetailMenuEntries(): Promise<string[]> {
    return todo("wsViewsDetailMenuEntries");
  }
  async wsViewsDetailCopyLink(): Promise<string> {
    return todo("wsViewsDetailCopyLink");
  }
  async wsViewsDetailOpenNewTabHref(): Promise<string> {
    return todo("wsViewsDetailOpenNewTabHref");
  }
  async wsViewsDetailErrorTitle(): Promise<string> {
    return todo("wsViewsDetailErrorTitle");
  }
  async wsViewsDetailErrorBack(): Promise<void> {
    return todo("wsViewsDetailErrorBack");
  }
  async viewsSettingsNotAuthorized(): Promise<boolean> {
    return todo("viewsSettingsNotAuthorized");
  }
  async viewsSettingsViewsToggleValue(): Promise<boolean> {
    return todo("viewsSettingsViewsToggleValue");
  }
  async viewsSettingsViewsToggleFlip(): Promise<void> {
    return todo("viewsSettingsViewsToggleFlip");
  }
  async viewsDialogTabOrder(): Promise<string[]> {
    return todo("viewsDialogTabOrder");
  }
  async viewsListDragRow(_from: string, _to: string): Promise<void> {
    return todo("viewsListDragRow");
  }
  async viewsPageExportImportVisible(): Promise<boolean> {
    return todo("viewsPageExportImportVisible");
  }
  async viewsDetailLayoutActive(): Promise<number> {
    return todo("viewsDetailLayoutActive");
  }
  async viewsDetailLayoutPick(_index: number): Promise<void> {
    return todo("viewsDetailLayoutPick");
  }
  async viewsDetailLayoutVisible(): Promise<boolean> {
    return todo("viewsDetailLayoutVisible");
  }
  async viewsDetailDisplayVisible(): Promise<boolean> {
    return todo("viewsDetailDisplayVisible");
  }
  async viewsDetailFiltersToggleVisible(): Promise<boolean> {
    return todo("viewsDetailFiltersToggleVisible");
  }
  async viewsDetailAddVisible(): Promise<boolean> {
    return todo("viewsDetailAddVisible");
  }
  async viewsDetailEmptyTitle(): Promise<string> {
    return todo("viewsDetailEmptyTitle");
  }
  async viewsDetailTabTitle(): Promise<string> {
    return todo("viewsDetailTabTitle");
  }
  async notificationsCurrentUrl(): Promise<string> {
    return todo("notificationsCurrentUrl");
  }

  async notificationsFailListFetches(_status: number): Promise<void> {
    return todo("notificationsFailListFetches");
  }

  async notificationsClearListFetchFailure(): Promise<void> {
    return todo("notificationsClearListFetchFailure");
  }

  async notificationsRetryControlVisible(): Promise<boolean> {
    return todo("notificationsRetryControlVisible");
  }

  async notificationsPressKey(_key: string): Promise<void> {
    return todo("notificationsPressKey");
  }

  async notificationsDragCard(_fromIndex: number, _toIndex: number): Promise<void> {
    return todo("notificationsDragCard");
  }

  async notificationsExportImportVisible(): Promise<boolean> {
    return todo("notificationsExportImportVisible");
  }

  async notificationsEntryRequestPaths(_workspaceSlug: string): Promise<string[]> {
    return todo("notificationsEntryRequestPaths");
  }

  async notificationsArmNotificationRequestSpy(): Promise<void> {
    return todo("notificationsArmNotificationRequestSpy");
  }

  async notificationsNotificationRequestCount(): Promise<number> {
    return todo("notificationsNotificationRequestCount");
  }

  // --- Archives cross-cutting (NEWFRONT-226, ARCH-026–032). Appended; the
  // --- stubs above are untouched per the shared driver contract.
  async archivesProjectConfirmLabel(): Promise<string> {
    return todo("archivesProjectConfirmLabel");
  }
  async archivesProjectConfirmBusy(): Promise<boolean> {
    return todo("archivesProjectConfirmBusy");
  }
  async archivesProjectDelayNextWrite(_ms: number): Promise<void> {
    return todo("archivesProjectDelayNextWrite");
  }
  async archivesProjectFailNextWrite(): Promise<void> {
    return todo("archivesProjectFailNextWrite");
  }
  async archivesItemFailNextWrite(): Promise<void> {
    return todo("archivesItemFailNextWrite");
  }
  async archivesTabOpen(_workspaceSlug: string, _projectId: string, _tab: ArchivesTab): Promise<void> {
    return todo("archivesTabOpen");
  }
  async archivesActiveTabKey(): Promise<ArchivesTab> {
    return todo("archivesActiveTabKey");
  }
  async archivesRowMenuOpenFirst(_name: string): Promise<void> {
    return todo("archivesRowMenuOpenFirst");
  }
  async archivesRowMenuEntryTitles(): Promise<string[]> {
    return todo("archivesRowMenuEntryTitles");
  }
  async archivesRowMenuClick(_entry: string): Promise<void> {
    return todo("archivesRowMenuClick");
  }
  async archivesPeekOpenFirst(_name: string): Promise<void> {
    return todo("archivesPeekOpenFirst");
  }
  async archivesPeekVisible(): Promise<boolean> {
    return todo("archivesPeekVisible");
  }
  async archivesPeekClose(): Promise<void> {
    return todo("archivesPeekClose");
  }
  async archivesDetailBannerText(): Promise<string | null> {
    return todo("archivesDetailBannerText");
  }
  async archivesSkeletonVisible(): Promise<boolean> {
    return todo("archivesSkeletonVisible");
  }
  async archivesDelayNextListReads(_ms: number): Promise<void> {
    return todo("archivesDelayNextListReads");
  }
  async archivesBeginTrafficSpy(): Promise<void> {
    return todo("archivesBeginTrafficSpy");
  }
  async archivesTrafficCounts(): Promise<ArchivesTrafficCounts> {
    return todo("archivesTrafficCounts");
  }
  async archivesFilterControlVisible(): Promise<boolean> {
    return todo("archivesFilterControlVisible");
  }
  async archivesSearchType(_text: string): Promise<void> {
    return todo("archivesSearchType");
  }
  async archivesSearchText(): Promise<string> {
    return todo("archivesSearchText");
  }
  async archivesRowDraggable(_name: string): Promise<boolean> {
    return todo("archivesRowDraggable");
  }

  async archivesDragReorders(_source: string, _target: string): Promise<boolean> {
    return todo("archivesDragReorders");
  }

  async archivesArmSkeletonObserver(): Promise<void> {
    return todo("archivesArmSkeletonObserver");
  }

  async archivesSkeletonWasSeen(): Promise<boolean> {
    return todo("archivesSkeletonWasSeen");
  }
  async archivesExportControlVisible(): Promise<boolean> {
    return todo("archivesExportControlVisible");
  }
  async archivesPressKey(_key: string): Promise<void> {
    return todo("archivesPressKey");
  }
  async archivesRowPresent(_name: string): Promise<boolean> {
    return todo("archivesRowPresent");
  }
  async archivesPageTextPresent(_text: string): Promise<boolean> {
    return todo("archivesPageTextPresent");
  }
  async archivesPrimeAndEnter(_workspaceSlug: string, _projectId: string, _liveCycleName: string): Promise<void> {
    return todo("archivesPrimeAndEnter");
  }
  async archivesTabClick(_tab: ArchivesTab): Promise<void> {
    return todo("archivesTabClick");
  }

  async cyclesEditOpenUpdateDialog(_name: string): Promise<void> {
    return todo("cyclesEditOpenUpdateDialog");
  }

  async cyclesEditOpenListRaw(_workspaceSlug: string, _projectId: string): Promise<void> {
    return todo("cyclesEditOpenListRaw");
  }

  async cyclesEditDialogHeading(): Promise<string | null> {
    return todo("cyclesEditDialogHeading");
  }

  async cyclesEditFillName(_text: string): Promise<void> {
    return todo("cyclesEditFillName");
  }

  async cyclesEditFillDescription(_text: string): Promise<void> {
    return todo("cyclesEditFillDescription");
  }

  async cyclesEditRangeOpen(): Promise<void> {
    return todo("cyclesEditRangeOpen");
  }

  async cyclesEditSubmitUpdate(): Promise<void> {
    return todo("cyclesEditSubmitUpdate");
  }

  async cyclesEditSubmitCountingDateChecks(): Promise<{ dateChecks: number }> {
    return todo("cyclesEditSubmitCountingDateChecks");
  }

  async cyclesEditStoredCycleTab(): Promise<string | null> {
    return todo("cyclesEditStoredCycleTab");
  }

  async cyclesEditClearStoredCycleTab(): Promise<void> {
    return todo("cyclesEditClearStoredCycleTab");
  }

  async cyclesEditHeroCycleName(): Promise<string | null> {
    return todo("cyclesEditHeroCycleName");
  }

  async cyclesEditPressEscape(): Promise<void> {
    return todo("cyclesEditPressEscape");
  }

  async cyclesEditTitleFocused(): Promise<boolean> {
    return todo("cyclesEditTitleFocused");
  }

  async cyclesEditFocusTrail(_steps: number): Promise<string[]> {
    return todo("cyclesEditFocusTrail");
  }

  async cyclesEditCreateButtonVisible(): Promise<boolean> {
    return todo("cyclesEditCreateButtonVisible");
  }

  async cyclesEditEmptyCreateState(): Promise<{ visible: boolean; disabled: boolean }> {
    return todo("cyclesEditEmptyCreateState");
  }

  async cyclesEditOpenPeek(_name: string): Promise<void> {
    return todo("cyclesEditOpenPeek");
  }

  async cyclesEditOpenDetailMenu(): Promise<void> {
    return todo("cyclesEditOpenDetailMenu");
  }

  async cyclesEditDeleteDialogText(): Promise<{ heading: string; body: string } | null> {
    return todo("cyclesEditDeleteDialogText");
  }

  async cyclesEditDeleteConfirm(): Promise<void> {
    return todo("cyclesEditDeleteConfirm");
  }

  async cyclesEditDeleteCancel(): Promise<void> {
    return todo("cyclesEditDeleteCancel");
  }

  async cyclesEditFailNextDeleteWrite(): Promise<void> {
    return todo("cyclesEditFailNextDeleteWrite");
  }

  async cyclesEditDetailReadOnlyNotice(): Promise<string | null> {
    return todo("cyclesEditDetailReadOnlyNotice");
  }

  // --- Intake detail (NEWFRONT-258, INT-019/024/026-029). Stubs: the area
  // --- has not landed in apps/web_new, so every action throws.

  async intakeDetailTitle(): Promise<string> {
    return todo("intakeDetailTitle");
  }
  async intakeDetailTitleEditable(): Promise<boolean> {
    return todo("intakeDetailTitleEditable");
  }
  async intakeDetailSetTitle(_title: string): Promise<void> {
    return todo("intakeDetailSetTitle");
  }
  async intakeDetailFocusTitle(): Promise<void> {
    return todo("intakeDetailFocusTitle");
  }
  async intakeDetailDescriptionText(): Promise<string> {
    return todo("intakeDetailDescriptionText");
  }
  async intakeDetailDescriptionEditable(): Promise<boolean> {
    return todo("intakeDetailDescriptionEditable");
  }
  async intakeDetailSetDescription(_text: string): Promise<void> {
    return todo("intakeDetailSetDescription");
  }
  async intakeDetailFocusDescription(): Promise<void> {
    return todo("intakeDetailFocusDescription");
  }
  async intakeDetailReactions(): Promise<string[]> {
    return todo("intakeDetailReactions");
  }
  async intakeDetailAddReaction(_emoji: string): Promise<void> {
    return todo("intakeDetailAddReaction");
  }
  async intakeDetailAttachmentNames(): Promise<string[]> {
    return todo("intakeDetailAttachmentNames");
  }
  async intakeDetailPropertyDisabled(_label: IntakeDetailProperty): Promise<boolean> {
    return todo("intakeDetailPropertyDisabled");
  }
  async intakeDetailPriorityText(): Promise<string> {
    return todo("intakeDetailPriorityText");
  }
  async intakeDetailSetPriority(_priority: string): Promise<void> {
    return todo("intakeDetailSetPriority");
  }
  async intakeDetailAssigneeTexts(): Promise<string[]> {
    return todo("intakeDetailAssigneeTexts");
  }
  async intakeDetailAddAssignee(_displayName: string): Promise<void> {
    return todo("intakeDetailAddAssignee");
  }
  async intakeDetailDueDateText(): Promise<string> {
    return todo("intakeDetailDueDateText");
  }
  async intakeDetailSetDueDate(_daysFromNow: number): Promise<void> {
    return todo("intakeDetailSetDueDate");
  }
  async intakeDetailLabelTexts(): Promise<string[]> {
    return todo("intakeDetailLabelTexts");
  }
  async intakeDetailAddLabel(_labelName: string): Promise<void> {
    return todo("intakeDetailAddLabel");
  }
  async intakeDetailCopyLink(): Promise<void> {
    return todo("intakeDetailCopyLink");
  }
  async intakeDetailOpenWorkItemVisible(): Promise<boolean> {
    return todo("intakeDetailOpenWorkItemVisible");
  }
  async intakeDetailOpenWorkItem(): Promise<void> {
    return todo("intakeDetailOpenWorkItem");
  }
  async intakeDetailCurrentInboxIssueId(): Promise<string | null> {
    return todo("intakeDetailCurrentInboxIssueId");
  }
  async intakeDetailCurrentUrl(): Promise<string> {
    return todo("intakeDetailCurrentUrl");
  }
  async intakeDetailPressArrow(_direction: "up" | "down"): Promise<void> {
    return todo("intakeDetailPressArrow");
  }
  async intakeDetailClickChevron(_direction: "prev" | "next"): Promise<void> {
    return todo("intakeDetailClickChevron");
  }
  async intakeDetailOpenRaw(_path: string): Promise<void> {
    return todo("intakeDetailOpenRaw");
  }
  async intakeDetailListVisible(): Promise<boolean> {
    return todo("intakeDetailListVisible");
  }
  async intakeDetailSelectListItem(_name: string): Promise<void> {
    return todo("intakeDetailSelectListItem");
  }
  async intakeDetailVersionsVisible(): Promise<boolean> {
    return todo("intakeDetailVersionsVisible");
  }
  async intakeDetailOpenVersions(): Promise<void> {
    return todo("intakeDetailOpenVersions");
  }
  async intakeDetailVersionTexts(): Promise<string[]> {
    return todo("intakeDetailVersionTexts");
  }
  async intakeDetailOpenVersion(_index: number): Promise<void> {
    return todo("intakeDetailOpenVersion");
  }
  async intakeDetailVersionModalText(): Promise<string | null> {
    return todo("intakeDetailVersionModalText");
  }
  async intakeDetailRestoreVersion(): Promise<void> {
    return todo("intakeDetailRestoreVersion");
  }
  async intakeDetailCloseVersionModal(): Promise<void> {
    return todo("intakeDetailCloseVersionModal");
  }
  // --- Archived work-item mutations (NEWFRONT-223, ARCH-008..013). ---

  async archivesOpenList(_workspaceSlug: string, _projectId: string): Promise<void> {
    return todo("archivesOpenList");
  }

  async archivesMutationVisibleIssueNames(): Promise<string[]> {
    return todo("archivesMutationVisibleIssueNames");
  }

  async archivesFailNextMutation(_method: "POST" | "DELETE", _urlPart: string): Promise<void> {
    return todo("archivesFailNextMutation");
  }

  async archivesClearMutationFailure(): Promise<void> {
    return todo("archivesClearMutationFailure");
  }

  async archivesArchiveModalTitle(): Promise<string | null> {
    return todo("archivesArchiveModalTitle");
  }

  async archivesArchiveModalBody(): Promise<string | null> {
    return todo("archivesArchiveModalBody");
  }

  async archivesArchiveModalCancel(): Promise<void> {
    return todo("archivesArchiveModalCancel");
  }

  async archivesDetailMenuChoose(_item: string): Promise<void> {
    return todo("archivesDetailMenuChoose");
  }

  async archivesOpenDetailRaw(_workspaceSlug: string, _projectId: string, _issueId: string): Promise<void> {
    return todo("archivesOpenDetailRaw");
  }

  async archivesDetailBreadcrumbText(): Promise<string> {
    return todo("archivesDetailBreadcrumbText");
  }

  async archivesMutationDetailBannerText(): Promise<string | null> {
    return todo("archivesMutationDetailBannerText");
  }

  async archivesDetailBannerBack(): Promise<void> {
    return todo("archivesDetailBannerBack");
  }

  async archivesDetailNotFoundVisible(): Promise<boolean> {
    return todo("archivesDetailNotFoundVisible");
  }

  async archivesDetailPendingStateOnHeldFetch(
    _workspaceSlug: string,
    _projectId: string,
    _issueId: string
  ): Promise<{
    heldRequests: number;
    bannerDuringHold: string | null;
    breadcrumbDuringHold: string;
    activityDuringHold: boolean;
    settled: boolean;
  }> {
    return todo("archivesDetailPendingStateOnHeldFetch");
  }

  async archivesDetailComposerVisible(): Promise<boolean> {
    return todo("archivesDetailComposerVisible");
  }

  async archivesDetailReactionControlEnabled(): Promise<boolean> {
    return todo("archivesDetailReactionControlEnabled");
  }

  async intakeCreateDialogOpen(): Promise<boolean> {
    return todo("intakeCreateDialogOpen");
  }

  async intakeCreateTitleValue(): Promise<string> {
    return todo("intakeCreateTitleValue");
  }

  async intakeCreateTitleHint(): Promise<string> {
    return todo("intakeCreateTitleHint");
  }

  async intakeCreateSubmitDisabled(): Promise<boolean> {
    return todo("intakeCreateSubmitDisabled");
  }

  async intakeCreateFillDescription(_text: string): Promise<void> {
    return todo("intakeCreateFillDescription");
  }

  async intakeCreateDescriptionText(): Promise<string> {
    return todo("intakeCreateDescriptionText");
  }

  async intakeCreatePriorityValue(): Promise<string> {
    return todo("intakeCreatePriorityValue");
  }

  async intakeCreateSetPriority(_label: string): Promise<void> {
    return todo("intakeCreateSetPriority");
  }

  async intakeCreateMoreOn(): Promise<boolean> {
    return todo("intakeCreateMoreOn");
  }

  async intakeCreateToggleMore(): Promise<void> {
    return todo("intakeCreateToggleMore");
  }

  async intakeCreateDiscard(): Promise<void> {
    return todo("intakeCreateDiscard");
  }

  async intakeCreatePressEscape(): Promise<void> {
    return todo("intakeCreatePressEscape");
  }

  async intakeCreateSubmitStayingOpen(): Promise<void> {
    return todo("intakeCreateSubmitStayingOpen");
  }

  async intakeCreateStallNextUpload(): Promise<void> {
    return todo("intakeCreateStallNextUpload");
  }

  async intakeCreateReleaseUpload(): Promise<void> {
    return todo("intakeCreateReleaseUpload");
  }

  async intakeCreateInsertImage(_file: { name: string; mime: string; bytes: Buffer }): Promise<void> {
    return todo("intakeCreateInsertImage");
  }

  async intakeCreateEditorImageCount(): Promise<number> {
    return todo("intakeCreateEditorImageCount");
  }

  // --- NEWFRONT-253 (cycles detail/sidebar, CYC-032-039). Stage 0
  // --- oracle convention: todo() stubs until the area lands in web_new.
  async cyclesDetailSidebarName(): Promise<string | null> {
    return todo("cyclesDetailSidebarName");
  }
  async cyclesToggleSidebarViaHeader(): Promise<void> {
    return todo("cyclesToggleSidebarViaHeader");
  }
  async cyclesCloseSidebarViaPanel(): Promise<void> {
    return todo("cyclesCloseSidebarViaPanel");
  }
  async cyclesSidebarCollapsedStored(): Promise<boolean> {
    return todo("cyclesSidebarCollapsedStored");
  }
  async cyclesHeaderBadgeText(): Promise<string | null> {
    return todo("cyclesHeaderBadgeText");
  }
  async cyclesHeaderAddVisible(): Promise<boolean> {
    return todo("cyclesHeaderAddVisible");
  }
  async cyclesHeaderAnalyticsVisible(): Promise<boolean> {
    return todo("cyclesHeaderAnalyticsVisible");
  }
  async cyclesOpenAnalytics(): Promise<void> {
    return todo("cyclesOpenAnalytics");
  }
  async cyclesAnalyticsDialogVisible(): Promise<boolean> {
    return todo("cyclesAnalyticsDialogVisible");
  }
  async cyclesDismissAnalytics(): Promise<void> {
    return todo("cyclesDismissAnalytics");
  }
  async cyclesSwitcherOpen(_currentName: string): Promise<void> {
    return todo("cyclesSwitcherOpen");
  }
  async cyclesSwitcherOptions(): Promise<string[]> {
    return todo("cyclesSwitcherOptions");
  }
  async cyclesSwitcherPick(_name: string): Promise<void> {
    return todo("cyclesSwitcherPick");
  }
  async cyclesClickCyclesCrumb(): Promise<void> {
    return todo("cyclesClickCyclesCrumb");
  }
  async cyclesClickProjectCrumb(_projectName: string): Promise<void> {
    return todo("cyclesClickProjectCrumb");
  }
  async cyclesMobileBackVisible(): Promise<boolean> {
    return todo("cyclesMobileBackVisible");
  }
  async cyclesMobileBack(): Promise<void> {
    return todo("cyclesMobileBack");
  }
  async cyclesSidebarDateText(): Promise<string> {
    return todo("cyclesSidebarDateText");
  }
  async cyclesSidebarDateDisabled(): Promise<boolean> {
    return todo("cyclesSidebarDateDisabled");
  }
  async cyclesSidebarOpenDatePicker(): Promise<void> {
    return todo("cyclesSidebarOpenDatePicker");
  }
  async cyclesPickDateDay(_label: string): Promise<void> {
    return todo("cyclesPickDateDay");
  }
  async cyclesDismissDatePicker(): Promise<void> {
    return todo("cyclesDismissDatePicker");
  }
  async cyclesSidebarDescription(): Promise<string | null> {
    return todo("cyclesSidebarDescription");
  }
  async cyclesSidebarLead(): Promise<{ name: string; avatar: string }> {
    return todo("cyclesSidebarLead");
  }
  async cyclesProgressWorkItemsText(): Promise<string> {
    return todo("cyclesProgressWorkItemsText");
  }
  async cyclesProgressPointsText(): Promise<string | null> {
    return todo("cyclesProgressPointsText");
  }
  async cyclesProgressChartVisible(): Promise<boolean> {
    return todo("cyclesProgressChartVisible");
  }
  async cyclesProgressMeasureValue(): Promise<string | null> {
    return todo("cyclesProgressMeasureValue");
  }
  async cyclesProgressMeasureOptions(): Promise<string[]> {
    return todo("cyclesProgressMeasureOptions");
  }
  async cyclesProgressPickMeasure(_label: string): Promise<void> {
    return todo("cyclesProgressPickMeasure");
  }
  async cyclesProgressStatsTab(_tab: "States" | "Assignees" | "Labels"): Promise<void> {
    return todo("cyclesProgressStatsTab");
  }
  async cyclesProgressStatsRows(): Promise<CyclesProgressStatRow[]> {
    return todo("cyclesProgressStatsRows");
  }
  async cyclesProgressEmptyText(): Promise<string | null> {
    return todo("cyclesProgressEmptyText");
  }
  async cyclesProgressEntryClick(_title: string): Promise<void> {
    return todo("cyclesProgressEntryClick");
  }
  async cyclesMobileOpenAnalytics(): Promise<void> {
    return todo("cyclesMobileOpenAnalytics");
  }
  async cyclesSwitchLayout(_layout: LayoutsLayoutKey): Promise<void> {
    return todo("cyclesSwitchLayout");
  }
  async cyclesMobileOfferedLayouts(): Promise<LayoutsLayoutKey[]> {
    return todo("cyclesMobileOfferedLayouts");
  }
  async cyclesMobileSwitchTo(_layout: LayoutsLayoutKey): Promise<void> {
    return todo("cyclesMobileSwitchTo");
  }

  // --- Cycles empty states, gone-away, create, validation (NEWFRONT-250,
  // --- CYC-009..016). Oracle-stage stubs; the new-app driver implements
  // --- them when the area is built.

  async cyclesEmptyOpenList(_workspaceSlug: string, _projectId: string): Promise<void> {
    return todo("cyclesEmptyOpenList");
  }

  async cyclesEmptyVisibleNames(): Promise<string[]> {
    return todo("cyclesEmptyVisibleNames");
  }

  async cyclesEmptySkeletonVisible(): Promise<boolean> {
    return todo("cyclesEmptySkeletonVisible");
  }

  async cyclesEmptyNoMatchHeading(): Promise<string | null> {
    return todo("cyclesEmptyNoMatchHeading");
  }

  async cyclesEmptyNoMatchHint(): Promise<string | null> {
    return todo("cyclesEmptyNoMatchHint");
  }

  async cyclesEmptySearchOpen(): Promise<void> {
    return todo("cyclesEmptySearchOpen");
  }

  async cyclesEmptySearchFill(_text: string): Promise<void> {
    return todo("cyclesEmptySearchFill");
  }

  async cyclesEmptySearchClear(): Promise<void> {
    return todo("cyclesEmptySearchClear");
  }

  async cyclesEmptyFiltersOpen(): Promise<void> {
    return todo("cyclesEmptyFiltersOpen");
  }

  async cyclesEmptyFiltersClose(): Promise<void> {
    return todo("cyclesEmptyFiltersClose");
  }

  async cyclesEmptyFilterSections(): Promise<string[]> {
    return todo("cyclesEmptyFilterSections");
  }

  async cyclesEmptyFilterOptionNames(): Promise<string[]> {
    return todo("cyclesEmptyFilterOptionNames");
  }

  async cyclesEmptyFilterPick(_section: string, _optionName: string): Promise<void> {
    return todo("cyclesEmptyFilterPick");
  }

  async cyclesEmptyFiltersClearAll(): Promise<void> {
    return todo("cyclesEmptyFiltersClearAll");
  }

  async cyclesEmptySkeletonShownOnSlowFetch(_workspaceSlug: string, _projectId: string): Promise<boolean> {
    return todo("cyclesEmptySkeletonShownOnSlowFetch");
  }

  async cyclesEmptyFeatureOffVisible(): Promise<boolean> {
    return todo("cyclesEmptyFeatureOffVisible");
  }

  async cyclesEmptyFeatureOffActionDisabled(): Promise<boolean> {
    return todo("cyclesEmptyFeatureOffActionDisabled");
  }

  async cyclesEmptyFeatureOffActionOpen(): Promise<void> {
    return todo("cyclesEmptyFeatureOffActionOpen");
  }

  async cyclesEmptyZeroStateVisible(): Promise<boolean> {
    return todo("cyclesEmptyZeroStateVisible");
  }

  async cyclesEmptyZeroStateCreateDisabled(): Promise<boolean> {
    return todo("cyclesEmptyZeroStateCreateDisabled");
  }

  async cyclesEmptyZeroStateCreateOpen(): Promise<void> {
    return todo("cyclesEmptyZeroStateCreateOpen");
  }

  async cyclesEmptyOpenDetail(_workspaceSlug: string, _projectId: string, _cycleId: string): Promise<void> {
    return todo("cyclesEmptyOpenDetail");
  }

  async cyclesEmptyGoneAwayVisible(): Promise<boolean> {
    return todo("cyclesEmptyGoneAwayVisible");
  }

  async cyclesEmptyGoneAwayBack(): Promise<void> {
    return todo("cyclesEmptyGoneAwayBack");
  }

  async cyclesCreateOpenFromHeader(): Promise<void> {
    return todo("cyclesCreateOpenFromHeader");
  }

  async cyclesCreateDialogOpen(): Promise<boolean> {
    return todo("cyclesCreateDialogOpen");
  }

  async cyclesCreateDialogCancel(): Promise<void> {
    return todo("cyclesCreateDialogCancel");
  }

  async cyclesCreateFillTitle(_title: string): Promise<void> {
    return todo("cyclesCreateFillTitle");
  }

  async cyclesCreateFillDescription(_text: string): Promise<void> {
    return todo("cyclesCreateFillDescription");
  }

  async cyclesCreateTitleError(): Promise<string | null> {
    return todo("cyclesCreateTitleError");
  }

  async cyclesCreatePickDateRange(_startISO: string, _endISO: string): Promise<void> {
    return todo("cyclesCreatePickDateRange");
  }

  async cyclesCreatePastDayDisabled(): Promise<boolean> {
    return todo("cyclesCreatePastDayDisabled");
  }

  async cyclesCreatePickProject(_projectName: string): Promise<void> {
    return todo("cyclesCreatePickProject");
  }

  async cyclesCreateSubmit(): Promise<void> {
    return todo("cyclesCreateSubmit");
  }

  // --- Cycles archive/menu/transfer acceptance (NEWFRONT-252, CYC-025-031
  // --- plus CYC-046). Stage 0 oracle stubs; the cycles build slice lands them.

  async cyclesRowFavoriteState(_name: string): Promise<"starred" | "unstarred" | "absent"> {
    return todo("cyclesRowFavoriteState");
  }
  async cyclesRowFavoriteToggle(_name: string): Promise<void> {
    return todo("cyclesRowFavoriteToggle");
  }
  async cyclesRowFavoriteToggleConverge(
    _name: string,
    _want: "starred" | "unstarred",
    _isServerFavorite: () => Promise<boolean>,
    _rowPollMs: number
  ): Promise<void> {
    return todo("cyclesRowFavoriteToggleConverge");
  }
  async cyclesRowFavoriteSettleAgreement(
    _name: string,
    _want: "starred" | "unstarred",
    _isServerFavorite: () => Promise<boolean>,
    _rowPollMs: number
  ): Promise<void> {
    return todo("cyclesRowFavoriteSettleAgreement");
  }
  async cyclesDelayFavoriteWrites(_delayMs: number): Promise<void> {
    return todo("cyclesDelayFavoriteWrites");
  }
  async cyclesClearFavoriteDelays(): Promise<void> {
    return todo("cyclesClearFavoriteDelays");
  }
  async cyclesRowMenuTriggerVisible(_name: string): Promise<boolean> {
    return todo("cyclesRowMenuTriggerVisible");
  }
  async cyclesRowHover(_name: string): Promise<void> {
    return todo("cyclesRowHover");
  }
  async cyclesRowMenuOpenNewTab(_name: string): Promise<string> {
    return todo("cyclesRowMenuOpenNewTab");
  }
  async cyclesTransferSearchFill(_text: string): Promise<void> {
    return todo("cyclesTransferSearchFill");
  }
  async cyclesTransferEmptyText(): Promise<string | null> {
    return todo("cyclesTransferEmptyText");
  }
  async cyclesArchivedEmptyDetail(): Promise<string | null> {
    return todo("cyclesArchivedEmptyDetail");
  }
  async cyclesGrantClipboardAccess(): Promise<void> {
    return todo("cyclesGrantClipboardAccess");
  }

  async modulesOpenList(_workspaceSlug: string, _projectId: string, _cookies: ParityBrowserCookie[]): Promise<void> {
    return todo("modulesOpenList");
  }
  async modulesAwaitRow(_name: string): Promise<void> {
    return todo("modulesAwaitRow");
  }
  async modulesCurrentUrl(): Promise<string> {
    return todo("modulesCurrentUrl");
  }
  async modulesSetLayout(_layout: "list" | "board" | "timeline"): Promise<void> {
    return todo("modulesSetLayout");
  }
  async modulesSetSort(_label: string): Promise<void> {
    return todo("modulesSetSort");
  }
  async modulesSortLabel(): Promise<string> {
    return todo("modulesSortLabel");
  }
  async modulesRowDateText(_name: string): Promise<string> {
    return todo("modulesRowDateText");
  }
  async modulesRowDateDisabled(_name: string): Promise<boolean> {
    return todo("modulesRowDateDisabled");
  }
  async modulesRowOpenDates(_name: string): Promise<void> {
    return todo("modulesRowOpenDates");
  }
  async modulesCalendarVisible(): Promise<boolean> {
    return todo("modulesCalendarVisible");
  }
  async modulesCalendarPickDay(_day: number): Promise<void> {
    return todo("modulesCalendarPickDay");
  }
  async modulesCalendarGotoMonth(_monthLabel: string, _yearLabel: string): Promise<void> {
    return todo("modulesCalendarGotoMonth");
  }
  async modulesCardDateText(_name: string): Promise<string> {
    return todo("modulesCardDateText");
  }
  async modulesCardDateDisabled(_name: string): Promise<boolean> {
    return todo("modulesCardDateDisabled");
  }
  async modulesCardOpenDates(_name: string): Promise<void> {
    return todo("modulesCardOpenDates");
  }
  async modulesLastToast(): Promise<ModulesToast | null> {
    return todo("modulesLastToast");
  }
  async modulesAwaitToast(_text: string): Promise<void> {
    return todo("modulesAwaitToast");
  }
  async modulesPillText(_name: string): Promise<string> {
    return todo("modulesPillText");
  }
  async modulesPillColor(_name: string): Promise<string> {
    return todo("modulesPillColor");
  }
  async modulesStatusDisabled(_name: string): Promise<boolean> {
    return todo("modulesStatusDisabled");
  }
  async modulesOpenStatus(_name: string): Promise<void> {
    return todo("modulesOpenStatus");
  }
  async modulesStatusOptions(): Promise<string[]> {
    return todo("modulesStatusOptions");
  }
  async modulesPickStatus(_label: string): Promise<void> {
    return todo("modulesPickStatus");
  }
  async modulesLeadState(_name: string): Promise<"avatar" | "none"> {
    return todo("modulesLeadState");
  }
  async modulesLeadTooltip(_name: string): Promise<string | null> {
    return todo("modulesLeadTooltip");
  }
  async modulesStarVisible(_name: string): Promise<boolean> {
    return todo("modulesStarVisible");
  }
  async modulesStarSelected(_name: string): Promise<boolean> {
    return todo("modulesStarSelected");
  }
  async modulesToggleStar(_name: string): Promise<void> {
    return todo("modulesToggleStar");
  }
  async modulesFavoritesExpanded(): Promise<boolean> {
    return todo("modulesFavoritesExpanded");
  }
  async modulesOpenRowMenu(_name: string): Promise<void> {
    return todo("modulesOpenRowMenu");
  }
  async modulesMenuItems(): Promise<ModulesMenuEntry[]> {
    return todo("modulesMenuItems");
  }
  async modulesMenuChoose(_title: string): Promise<void> {
    return todo("modulesMenuChoose");
  }
  async modulesMenuChooseOpenTab(_titlePart: string): Promise<{ url: string; title: string }> {
    return todo("modulesMenuChooseOpenTab");
  }
  async modulesDialogVisible(_kind: "edit" | "archive" | "delete"): Promise<boolean> {
    return todo("modulesDialogVisible");
  }
  async modulesDialogHeading(): Promise<string | null> {
    return todo("modulesDialogHeading");
  }
  async modulesReadClipboard(): Promise<string> {
    return todo("modulesReadClipboard");
  }
  async modulesOpenDetail(
    _workspaceSlug: string,
    _projectId: string,
    _moduleId: string,
    _cookies: ParityBrowserCookie[]
  ): Promise<void> {
    return todo("modulesOpenDetail");
  }
  async modulesDetailOpenMenu(): Promise<void> {
    return todo("modulesDetailOpenMenu");
  }
  async modulesTogglePeek(_name: string): Promise<void> {
    return todo("modulesTogglePeek");
  }
  async modulesPeekVisible(): Promise<boolean> {
    return todo("modulesPeekVisible");
  }
  async modulesPeekName(): Promise<string | null> {
    return todo("modulesPeekName");
  }
  async modulesClosePeek(): Promise<void> {
    return todo("modulesClosePeek");
  }
  async modulesBoardColumnCount(): Promise<number> {
    return todo("modulesBoardColumnCount");
  }
  async modulesClickRow(_name: string): Promise<void> {
    return todo("modulesClickRow");
  }
  async modulesTimelineRows(): Promise<ModulesTimelineRow[]> {
    return todo("modulesTimelineRows");
  }
  async modulesTimelineBarColor(_name: string): Promise<string> {
    return todo("modulesTimelineBarColor");
  }
  async modulesTimelineHoverBar(_name: string): Promise<void> {
    return todo("modulesTimelineHoverBar");
  }
  async modulesTimelineTooltip(): Promise<string | null> {
    return todo("modulesTimelineTooltip");
  }
  async modulesTimelineBarExists(_name: string): Promise<boolean> {
    return todo("modulesTimelineBarExists");
  }
  async modulesTimelineDragBar(_name: string, _dayDelta: number): Promise<void> {
    return todo("modulesTimelineDragBar");
  }
  async modulesTimelineResizeBar(_name: string, _side: "left" | "right", _dayDelta: number): Promise<void> {
    return todo("modulesTimelineResizeBar");
  }
  async modulesTimelineOrder(): Promise<string[]> {
    return todo("modulesTimelineOrder");
  }
  async modulesTimelineDragRowBefore(_sourceName: string, _targetName: string): Promise<void> {
    return todo("modulesTimelineDragRowBefore");
  }
  async modulesTimelineAttemptRowBefore(_sourceName: string, _targetName: string): Promise<void> {
    return todo("modulesTimelineAttemptRowBefore");
  }
  async modulesTimelineAttemptBarMove(_name: string, _dayDelta: number): Promise<void> {
    return todo("modulesTimelineAttemptBarMove");
  }

  // --- Pages realtime collaboration + live server (NEWFRONT-319,
  // --- PAGE-065-071). Stage 0 oracle stubs; the pages build slice lands them.

  async pagesLiveOpenPageDetail(_workspaceSlug: string, _projectId: string, _pageId: string): Promise<void> {
    return todo("pagesLiveOpenPageDetail");
  }
  async pagesLiveSyncBadge(): Promise<string | null> {
    return todo("pagesLiveSyncBadge");
  }
  async pagesLiveWaitSynced(): Promise<void> {
    return todo("pagesLiveWaitSynced");
  }
  async pagesLiveBodyText(): Promise<string> {
    return todo("pagesLiveBodyText");
  }
  async pagesLiveTitleText(): Promise<string> {
    return todo("pagesLiveTitleText");
  }
  async pagesLiveTypeInBody(_text: string): Promise<void> {
    return todo("pagesLiveTypeInBody");
  }
  async pagesLiveSetTitle(_text: string): Promise<void> {
    return todo("pagesLiveSetTitle");
  }
  async pagesLiveSocketUrls(): Promise<string[]> {
    return todo("pagesLiveSocketUrls");
  }
  async pagesLiveSocketAuthTokens(): Promise<string[]> {
    return todo("pagesLiveSocketAuthTokens");
  }
  async pagesLiveSocketCloseEvents(): Promise<PagesLiveSocketClose[]> {
    return todo("pagesLiveSocketCloseEvents");
  }
  async pagesLiveSocketReceivedFrames(): Promise<string[]> {
    return todo("pagesLiveSocketReceivedFrames");
  }
  async pagesLiveSetOffline(_offline: boolean): Promise<void> {
    return todo("pagesLiveSetOffline");
  }
  async pagesLiveBlockCollaborationSocket(): Promise<void> {
    return todo("pagesLiveBlockCollaborationSocket");
  }
  async pagesLiveUnblockCollaborationSocket(): Promise<void> {
    return todo("pagesLiveUnblockCollaborationSocket");
  }
  async pagesLiveDescriptionFetchCount(): Promise<number> {
    return todo("pagesLiveDescriptionFetchCount");
  }
  async pagesLiveSyncLoaderVisible(): Promise<boolean> {
    return todo("pagesLiveSyncLoaderVisible");
  }

  // --- Intake triage actions (NEWFRONT-259, INT-020/021/022/023/025).

  async intakeTriageAcceptVisible(): Promise<boolean> {
    return todo("intakeTriageAcceptVisible");
  }

  async intakeTriageDeclineVisible(): Promise<boolean> {
    return todo("intakeTriageDeclineVisible");
  }

  async intakeTriageClickAccept(): Promise<void> {
    return todo("intakeTriageClickAccept");
  }

  async intakeTriageAcceptDialogVisible(): Promise<boolean> {
    return todo("intakeTriageAcceptDialogVisible");
  }

  async intakeTriageAcceptDialogText(): Promise<{ heading: string; confirm: string }> {
    return todo("intakeTriageAcceptDialogText");
  }

  async intakeTriageAcceptDialogConfirm(): Promise<void> {
    return todo("intakeTriageAcceptDialogConfirm");
  }

  async intakeTriageAcceptDialogCancel(): Promise<void> {
    return todo("intakeTriageAcceptDialogCancel");
  }

  async intakeTriageClickDecline(): Promise<void> {
    return todo("intakeTriageClickDecline");
  }

  async intakeTriageDeclineDialogVisible(): Promise<boolean> {
    return todo("intakeTriageDeclineDialogVisible");
  }

  async intakeTriageDeclineDialogText(): Promise<{ heading: string; body: string }> {
    return todo("intakeTriageDeclineDialogText");
  }

  async intakeTriageDeclineConfirm(): Promise<void> {
    return todo("intakeTriageDeclineConfirm");
  }

  async intakeTriageDeclineCancel(): Promise<void> {
    return todo("intakeTriageDeclineCancel");
  }

  async intakeTriageOverflowVisible(): Promise<boolean> {
    return todo("intakeTriageOverflowVisible");
  }

  async intakeTriageOpenOverflow(): Promise<void> {
    return todo("intakeTriageOpenOverflow");
  }

  async intakeTriageOverflowOptions(): Promise<string[]> {
    return todo("intakeTriageOverflowOptions");
  }

  async intakeTriageClickSnooze(): Promise<void> {
    return todo("intakeTriageClickSnooze");
  }

  async intakeTriageSnoozeDialogVisible(): Promise<boolean> {
    return todo("intakeTriageSnoozeDialogVisible");
  }

  async intakeTriageSnoozePastDisabled(): Promise<boolean> {
    return todo("intakeTriageSnoozePastDisabled");
  }

  async intakeTriageSnoozePickFuture(_daysAhead: number): Promise<void> {
    return todo("intakeTriageSnoozePickFuture");
  }

  async intakeTriageSnoozeConfirm(): Promise<void> {
    return todo("intakeTriageSnoozeConfirm");
  }

  async intakeTriageSnoozeCancel(): Promise<void> {
    return todo("intakeTriageSnoozeCancel");
  }

  async intakeTriageStatusChipText(): Promise<string | null> {
    return todo("intakeTriageStatusChipText");
  }

  async intakeTriageClickUnsnooze(): Promise<void> {
    return todo("intakeTriageClickUnsnooze");
  }

  async intakeTriageClickMarkDuplicate(): Promise<void> {
    return todo("intakeTriageClickMarkDuplicate");
  }

  async intakeTriageDuplicateDialogVisible(): Promise<boolean> {
    return todo("intakeTriageDuplicateDialogVisible");
  }

  async intakeTriageDuplicateSearch(_query: string): Promise<void> {
    return todo("intakeTriageDuplicateSearch");
  }

  async intakeTriageDuplicateOptions(): Promise<string[]> {
    return todo("intakeTriageDuplicateOptions");
  }

  async intakeTriageDuplicatePick(_name: string): Promise<void> {
    return todo("intakeTriageDuplicatePick");
  }

  async intakeTriageDuplicateOfText(): Promise<string | null> {
    return todo("intakeTriageDuplicateOfText");
  }

  async intakeTriageClickDelete(): Promise<void> {
    return todo("intakeTriageClickDelete");
  }

  async intakeTriageDeleteDialogVisible(): Promise<boolean> {
    return todo("intakeTriageDeleteDialogVisible");
  }

  async intakeTriageDeleteDialogText(): Promise<{ heading: string; body: string }> {
    return todo("intakeTriageDeleteDialogText");
  }

  async intakeTriageDeleteConfirm(): Promise<void> {
    return todo("intakeTriageDeleteConfirm");
  }

  async intakeTriageDeleteCancel(): Promise<void> {
    return todo("intakeTriageDeleteCancel");
  }

  async intakeTriageCurrentIssueId(): Promise<string | null> {
    return todo("intakeTriageCurrentIssueId");
  }

  async intakeTriageCurrentTab(): Promise<string | null> {
    return todo("intakeTriageCurrentTab");
  }

  async intakeTriageOpenIssueOnTab(
    _workspaceSlug: string,
    _projectId: string,
    _issueId: string,
    _tab: string
  ): Promise<void> {
    return todo("intakeTriageOpenIssueOnTab");
  }

  async intakeTriageFilterOpen(): Promise<void> {
    return todo("intakeTriageFilterOpen");
  }

  async intakeTriageFilterToggleStatus(_name: string): Promise<void> {
    return todo("intakeTriageFilterToggleStatus");
  }

  async intakeTriageFilterClose(): Promise<void> {
    return todo("intakeTriageFilterClose");
  }

  async intakeTriageListOpenIssue(_name: string): Promise<void> {
    return todo("intakeTriageListOpenIssue");
  }

  // --- Pages prefs, key map, assistant, cloud stubs, desktop, negatives
  // --- (NEWFRONT-320, PAGE-072-073, PAGE-075-078, PAGE-081-083).
  // --- Stage 0 oracle convention: stubs until the area lands.
  async pagesPrefsOpenList(_workspaceSlug: string, _projectId: string): Promise<void> {
    return todo("pagesPrefsOpenList");
  }
  async pagesPrefsOpenDetail(_workspaceSlug: string, _projectId: string, _pageId: string): Promise<void> {
    return todo("pagesPrefsOpenDetail");
  }
  async pagesPrefsReload(): Promise<void> {
    return todo("pagesPrefsReload");
  }
  async pagesPrefsEditorHtml(): Promise<string> {
    return todo("pagesPrefsEditorHtml");
  }
  async pagesPrefsEditorText(): Promise<string> {
    return todo("pagesPrefsEditorText");
  }
  async pagesPrefsEditorClickEnd(): Promise<void> {
    return todo("pagesPrefsEditorClickEnd");
  }
  async pagesPrefsEditorType(_text: string): Promise<void> {
    return todo("pagesPrefsEditorType");
  }
  async pagesPrefsEditorClear(): Promise<void> {
    return todo("pagesPrefsEditorClear");
  }
  async pagesPrefsEditorSelectAll(): Promise<void> {
    return todo("pagesPrefsEditorSelectAll");
  }
  async pagesPrefsTitleValue(): Promise<string> {
    return todo("pagesPrefsTitleValue");
  }
  async pagesPrefsTitleFill(_text: string): Promise<void> {
    return todo("pagesPrefsTitleFill");
  }
  async pagesPrefsTitleFocus(): Promise<void> {
    return todo("pagesPrefsTitleFocus");
  }
  async pagesPrefsTitlePressEnter(): Promise<void> {
    return todo("pagesPrefsTitlePressEnter");
  }
  async pagesPrefsTitleClass(): Promise<string> {
    return todo("pagesPrefsTitleClass");
  }
  async pagesPrefsFocusedRegion(): Promise<"title" | "body" | "other"> {
    return todo("pagesPrefsFocusedRegion");
  }
  async pagesPrefsOptionsOpen(): Promise<void> {
    return todo("pagesPrefsOptionsOpen");
  }
  async pagesPrefsOptionsItems(): Promise<string[]> {
    return todo("pagesPrefsOptionsItems");
  }
  async pagesPrefsOptionsToggle(_label: string): Promise<void> {
    return todo("pagesPrefsOptionsToggle");
  }
  async pagesPrefsLocalConfig(): Promise<PagesPrefsLocalConfig | null> {
    return todo("pagesPrefsLocalConfig");
  }
  async pagesPrefsSeedLocalConfig(_config: PagesPrefsLocalConfig): Promise<void> {
    return todo("pagesPrefsSeedLocalConfig");
  }
  async pagesPrefsToolbarVisible(): Promise<boolean> {
    return todo("pagesPrefsToolbarVisible");
  }
  async pagesPrefsToolbarWide(): Promise<boolean> {
    return todo("pagesPrefsToolbarWide");
  }
  async pagesPrefsCanvasMaxWidth(): Promise<string> {
    return todo("pagesPrefsCanvasMaxWidth");
  }
  async pagesPrefsEditorClass(): Promise<string> {
    return todo("pagesPrefsEditorClass");
  }
  async pagesPrefsSpyPageWrites(): Promise<void> {
    return todo("pagesPrefsSpyPageWrites");
  }
  async pagesPrefsPageWriteCount(): Promise<number> {
    return todo("pagesPrefsPageWriteCount");
  }
  async pagesPrefsPageWritePayloads(): Promise<string[]> {
    return todo("pagesPrefsPageWritePayloads");
  }
  async pagesKeysToolbarTooltips(): Promise<PagesKeysToolbarTooltip[]> {
    return todo("pagesKeysToolbarTooltips");
  }
  async pagesKeysPress(_chord: string): Promise<void> {
    return todo("pagesKeysPress");
  }
  async pagesKeysSearchOpen(): Promise<void> {
    return todo("pagesKeysSearchOpen");
  }
  async pagesKeysSearchFill(_text: string): Promise<void> {
    return todo("pagesKeysSearchFill");
  }
  async pagesKeysSearchValue(): Promise<string> {
    return todo("pagesKeysSearchValue");
  }
  async pagesKeysSearchPressEscape(): Promise<void> {
    return todo("pagesKeysSearchPressEscape");
  }
  async pagesKeysSearchExpanded(): Promise<boolean> {
    return todo("pagesKeysSearchExpanded");
  }
  async pagesKeysListRowTitles(): Promise<string[]> {
    return todo("pagesKeysListRowTitles");
  }
  async pagesKeysShortcutReferenceVisible(): Promise<boolean> {
    return todo("pagesKeysShortcutReferenceVisible");
  }
  async pagesAssistAiHandleVisible(): Promise<boolean> {
    return todo("pagesAssistAiHandleVisible");
  }
  async pagesAssistAiMenuVisible(): Promise<boolean> {
    return todo("pagesAssistAiMenuVisible");
  }
  async pagesCloudHeaderActionNames(): Promise<string[]> {
    return todo("pagesCloudHeaderActionNames");
  }
  async pagesCloudToolbarEndNames(): Promise<string[]> {
    return todo("pagesCloudToolbarEndNames");
  }
  async pagesCloudPaneOpen(): Promise<void> {
    return todo("pagesCloudPaneOpen");
  }
  async pagesCloudPaneTabNames(): Promise<string[]> {
    return todo("pagesCloudPaneTabNames");
  }
  async pagesCloudSlashEntries(): Promise<string[]> {
    return todo("pagesCloudSlashEntries");
  }
  async pagesDeskIsMobile(): Promise<boolean> {
    return todo("pagesDeskIsMobile");
  }
  async pagesDeskPlatform(): Promise<string> {
    return todo("pagesDeskPlatform");
  }
  async pagesNegListTabNames(): Promise<string[]> {
    return todo("pagesNegListTabNames");
  }
  async pagesNegRowMenuItems(_title: string): Promise<string[]> {
    return todo("pagesNegRowMenuItems");
  }
  async pagesNegCoverPresent(): Promise<boolean> {
    return todo("pagesNegCoverPresent");
  }
  async pagesNegConflictDialogVisible(): Promise<boolean> {
    return todo("pagesNegConflictDialogVisible");
  }
  async pagesNegMarkPresent(): Promise<boolean> {
    return todo("pagesNegMarkPresent");
  }

  // --- Intake cross-cutting (NEWFRONT-260, INT-018/030/031/032/034).
  // --- Appended; existing stubs above are untouched per the shared driver
  // --- contract.

  async intakeXOpen(_workspaceSlug: string, _projectId: string): Promise<void> {
    return todo("intakeXOpen");
  }

  async intakeXOpenDetail(_workspaceSlug: string, _projectId: string, _issueId: string): Promise<void> {
    return todo("intakeXOpenDetail");
  }

  async intakeXListTitles(): Promise<string[]> {
    return todo("intakeXListTitles");
  }

  async intakeXActiveTab(): Promise<IntakeXTab> {
    return todo("intakeXActiveTab");
  }

  async intakeXClickTab(_tab: IntakeXTab): Promise<void> {
    return todo("intakeXClickTab");
  }

  async intakeXDetailTitle(): Promise<string | null> {
    return todo("intakeXDetailTitle");
  }

  async intakeXOpenCreate(): Promise<void> {
    return todo("intakeXOpenCreate");
  }

  async intakeXCreateFillTitle(_title: string): Promise<void> {
    return todo("intakeXCreateFillTitle");
  }

  async intakeXCreateDuplicateDetectionVisible(): Promise<boolean> {
    return todo("intakeXCreateDuplicateDetectionVisible");
  }

  async intakeXDetailDuplicateDetectionVisible(): Promise<boolean> {
    return todo("intakeXDetailDuplicateDetectionVisible");
  }

  async intakeXSetViewport(_width: number, _height: number): Promise<void> {
    return todo("intakeXSetViewport");
  }

  async intakeXMobileHeaderVisible(): Promise<boolean> {
    return todo("intakeXMobileHeaderVisible");
  }

  async intakeXListPaneVisible(): Promise<boolean> {
    return todo("intakeXListPaneVisible");
  }

  async intakeXToggleMobileSidebar(): Promise<void> {
    return todo("intakeXToggleMobileSidebar");
  }

  async intakeXMobileMenuItems(): Promise<string[]> {
    return todo("intakeXMobileMenuItems");
  }

  async intakeXBeginApiSpy(): Promise<void> {
    return todo("intakeXBeginApiSpy");
  }

  async intakeXApiRequests(): Promise<IntakeXApiRequest[]> {
    return todo("intakeXApiRequests");
  }

  async intakeXRefocusPage(): Promise<void> {
    return todo("intakeXRefocusPage");
  }

  async intakeXSourceIndicatorVisible(): Promise<boolean> {
    return todo("intakeXSourceIndicatorVisible");
  }

  // --- Desktop bare sign-in, deep links, onboarding seam (NEWFRONT-208,
  // --- DESK-011–015, DESK-027). Throwing stubs per the shared driver contract.
  async deskSigninMarketingFrameVisible(): Promise<boolean> {
    return todo("deskSigninMarketingFrameVisible");
  }
  async deskSigninOnboardingProgress(): Promise<string | null> {
    return todo("deskSigninOnboardingProgress");
  }
  async deskSigninUpdateDialogLinks(): Promise<DeskSigninUpdateLink[]> {
    return todo("deskSigninUpdateDialogLinks");
  }
  async deskSigninOpenUpdateDialogLink(_label: string): Promise<string | null> {
    return todo("deskSigninOpenUpdateDialogLink");
  }

  // --- NEWFRONT-317 (pages navigation pane + versions, PAGE-045-054).
  // --- Appended stubs; existing stubs above are untouched per the shared
  // --- driver contract. The new app has no pages surface yet (Stage 0
  // --- oracle convention: every method throws until the area lands).

  async pagesPaneOpenDetail(_workspaceSlug: string, _projectId: string, _pageId: string): Promise<void> {
    return todo("pagesPaneOpenDetail");
  }

  async pagesPaneOpenDetailQuery(
    _workspaceSlug: string,
    _projectId: string,
    _pageId: string,
    _query: string
  ): Promise<void> {
    return todo("pagesPaneOpenDetailQuery");
  }

  async pagesPaneOpenDetailReadOnly(_workspaceSlug: string, _projectId: string, _pageId: string): Promise<void> {
    return todo("pagesPaneOpenDetailReadOnly");
  }

  async pagesPaneEditorText(): Promise<string> {
    return todo("pagesPaneEditorText");
  }

  async pagesPaneEditorClickEnd(): Promise<void> {
    return todo("pagesPaneEditorClickEnd");
  }

  async pagesPaneEditorType(_text: string): Promise<void> {
    return todo("pagesPaneEditorType");
  }

  async pagesPaneEditorPressEnter(): Promise<void> {
    return todo("pagesPaneEditorPressEnter");
  }

  async pagesPaneEditorScrollTop(): Promise<number> {
    return todo("pagesPaneEditorScrollTop");
  }

  async pagesPaneEditorPasteHtml(_html: string): Promise<void> {
    return todo("pagesPaneEditorPasteHtml");
  }

  async pagesPaneQueryParam(_key: string): Promise<string | null> {
    return todo("pagesPaneQueryParam");
  }

  async pagesPaneIsOpen(): Promise<boolean> {
    return todo("pagesPaneIsOpen");
  }

  async pagesPaneOpenViaToolbar(): Promise<void> {
    return todo("pagesPaneOpenViaToolbar");
  }

  async pagesPaneClose(): Promise<void> {
    return todo("pagesPaneClose");
  }

  async pagesPaneWidth(): Promise<number> {
    return todo("pagesPaneWidth");
  }

  async pagesPaneTabNames(): Promise<string[]> {
    return todo("pagesPaneTabNames");
  }

  async pagesPaneActiveTab(): Promise<string | null> {
    return todo("pagesPaneActiveTab");
  }

  async pagesPaneSelectTab(_name: string): Promise<void> {
    return todo("pagesPaneSelectTab");
  }

  async pagesPaneCanvasWidth(): Promise<number> {
    return todo("pagesPaneCanvasWidth");
  }

  async pagesPaneSlideTransition(): Promise<boolean> {
    return todo("pagesPaneSlideTransition");
  }

  async pagesPanePreviewVisible(): Promise<boolean> {
    return todo("pagesPanePreviewVisible");
  }

  async pagesPanePreviewBarCount(): Promise<number> {
    return todo("pagesPanePreviewBarCount");
  }

  async pagesPanePreviewHover(): Promise<void> {
    return todo("pagesPanePreviewHover");
  }

  async pagesPanePreviewCardVisible(): Promise<boolean> {
    return todo("pagesPanePreviewCardVisible");
  }

  async pagesPanePreviewCardHeadings(): Promise<string[]> {
    return todo("pagesPanePreviewCardHeadings");
  }

  async pagesPanePreviewOpen(): Promise<void> {
    return todo("pagesPanePreviewOpen");
  }

  async pagesPaneOutlineHeadings(): Promise<PagesPaneOutlineHeading[]> {
    return todo("pagesPaneOutlineHeadings");
  }

  async pagesPaneOutlineClick(_text: string): Promise<void> {
    return todo("pagesPaneOutlineClick");
  }

  async pagesPaneOutlineEmptyVisible(): Promise<boolean> {
    return todo("pagesPaneOutlineEmptyVisible");
  }

  async pagesPaneAssetsRows(): Promise<PagesPaneAssetRow[]> {
    return todo("pagesPaneAssetsRows");
  }

  async pagesPaneAssetsDownloadVisible(_name: string): Promise<boolean> {
    return todo("pagesPaneAssetsDownloadVisible");
  }

  async pagesPaneAssetsEmptyVisible(): Promise<boolean> {
    return todo("pagesPaneAssetsEmptyVisible");
  }

  async pagesPaneStats(): Promise<PagesPaneStats> {
    return todo("pagesPaneStats");
  }

  async pagesPaneActors(): Promise<PagesPaneActor[]> {
    return todo("pagesPaneActors");
  }

  async pagesPaneVersionEntries(): Promise<PagesPaneVersionEntry[]> {
    return todo("pagesPaneVersionEntries");
  }

  async pagesPaneSelectVersion(_versionId: string): Promise<void> {
    return todo("pagesPaneSelectVersion");
  }

  async pagesPaneSelectCurrentVersion(): Promise<void> {
    return todo("pagesPaneSelectCurrentVersion");
  }

  async pagesPaneOverlayOpen(): Promise<boolean> {
    return todo("pagesPaneOverlayOpen");
  }

  async pagesPaneOverlayHeader(): Promise<PagesPaneOverlayHeader> {
    return todo("pagesPaneOverlayHeader");
  }

  async pagesPaneOverlayText(): Promise<string> {
    return todo("pagesPaneOverlayText");
  }

  async pagesPaneOverlaySkeletonVisible(): Promise<boolean> {
    return todo("pagesPaneOverlaySkeletonVisible");
  }

  async pagesPaneOverlayStallDetail(): Promise<void> {
    return todo("pagesPaneOverlayStallDetail");
  }

  async pagesPaneOverlayReleaseDetail(): Promise<void> {
    return todo("pagesPaneOverlayReleaseDetail");
  }

  async pagesPaneRestoreVisible(): Promise<boolean> {
    return todo("pagesPaneRestoreVisible");
  }

  async pagesPaneRestore(): Promise<void> {
    return todo("pagesPaneRestore");
  }

  async pagesPaneOverlayErrorVisible(): Promise<boolean> {
    return todo("pagesPaneOverlayErrorVisible");
  }

  async pagesPaneOverlayErrorTexts(): Promise<{
    heading: string | null;
    body: string | null;
    retry: string | null;
  }> {
    return todo("pagesPaneOverlayErrorTexts");
  }

  async pagesPaneOverlayRetry(): Promise<void> {
    return todo("pagesPaneOverlayRetry");
  }

  async pagesPaneFailVersionDetail(): Promise<void> {
    return todo("pagesPaneFailVersionDetail");
  }

  async pagesPaneUnfailVersionDetail(): Promise<void> {
    return todo("pagesPaneUnfailVersionDetail");
  }

  async pagesPaneVersionDetailRequests(): Promise<number> {
    return todo("pagesPaneVersionDetailRequests");
  }

  async pagesPaneEditorClickText(_text: string): Promise<void> {
    return todo("pagesPaneEditorClickText");
  }

  async pagesPaneEditorClear(): Promise<void> {
    return todo("pagesPaneEditorClear");
  }

  async pagesPaneEditorDeleteImage(): Promise<void> {
    return todo("pagesPaneEditorDeleteImage");
  }

  async pagesPaneOmitWorkspaceMember(_userId: string): Promise<void> {
    return todo("pagesPaneOmitWorkspaceMember");
  }

  async pagesPaneRestoreWorkspaceMembers(): Promise<void> {
    return todo("pagesPaneRestoreWorkspaceMembers");
  }

  async pagesPaneProfileRoundtrip(): Promise<void> {
    return todo("pagesPaneProfileRoundtrip");
  }

  // --- Estimates screen shell (NEWFRONT-321, EST-001-003, EST-026-028).
  // --- Stubs; the new app implements them when the area is built.
  async estimatesOpenSettings(_workspaceSlug: string, _projectId: string): Promise<void> {
    return todo("estimatesOpenSettings");
  }

  async estimatesNotAuthorized(): Promise<boolean> {
    return todo("estimatesNotAuthorized");
  }

  async estimatesHeadingVisible(): Promise<boolean> {
    return todo("estimatesHeadingVisible");
  }

  async estimatesBreadcrumbText(): Promise<string> {
    return todo("estimatesBreadcrumbText");
  }

  async estimatesSkeletonVisible(): Promise<boolean> {
    return todo("estimatesSkeletonVisible");
  }

  async estimatesDelayListFetch(_ms: number): Promise<void> {
    return todo("estimatesDelayListFetch");
  }

  async estimatesEmptyVisible(): Promise<boolean> {
    return todo("estimatesEmptyVisible");
  }

  async estimatesEmptyTitle(): Promise<string> {
    return todo("estimatesEmptyTitle");
  }

  async estimatesEmptyAddClick(): Promise<void> {
    return todo("estimatesEmptyAddClick");
  }

  async estimatesCreateDialogOpen(): Promise<boolean> {
    return todo("estimatesCreateDialogOpen");
  }

  async estimatesScreenText(): Promise<string> {
    return todo("estimatesScreenText");
  }

  async estimatesListedNames(): Promise<string[]> {
    return todo("estimatesListedNames");
  }

  async estimatesTabTitle(): Promise<string> {
    return todo("estimatesTabTitle");
  }

  async estimatesExportImportVisible(): Promise<boolean> {
    return todo("estimatesExportImportVisible");
  }

  async estimatesFilterSearchVisible(): Promise<boolean> {
    return todo("estimatesFilterSearchVisible");
  }

  async estimatesBulkVisible(): Promise<boolean> {
    return todo("estimatesBulkVisible");
  }

  async estimatesOpenSystemGuess(_workspaceSlug: string, _projectId: string, _estimateId: string): Promise<void> {
    return todo("estimatesOpenSystemGuess");
  }

  async estimatesNotFoundVisible(): Promise<boolean> {
    return todo("estimatesNotFoundVisible");
  }

  async estimatesOpenAnalytics(): Promise<void> {
    return todo("estimatesOpenAnalytics");
  }
  async estimatesAnalyticsYAxisValue(): Promise<string> {
    return todo("estimatesAnalyticsYAxisValue");
  }
  async estimatesAnalyticsYAxisOptions(): Promise<string[]> {
    return todo("estimatesAnalyticsYAxisOptions");
  }
  async estimatesAnalyticsPickYAxis(_label: string): Promise<void> {
    return todo("estimatesAnalyticsPickYAxis");
  }
  async estimatesAnalyticsXAxisValue(): Promise<string> {
    return todo("estimatesAnalyticsXAxisValue");
  }
  async estimatesAnalyticsXAxisOptions(): Promise<string[]> {
    return todo("estimatesAnalyticsXAxisOptions");
  }
  async estimatesAnalyticsPickXAxis(_label: string): Promise<void> {
    return todo("estimatesAnalyticsPickXAxis");
  }
  async estimatesAnalyticsTableRows(): Promise<string[][]> {
    return todo("estimatesAnalyticsTableRows");
  }
  // --- Workspace drafts oracle (NEWFRONT-32). Throwing stubs per the
  // --- shared driver contract; the Drafts area fills these in.

  async draftsPageTitle(): Promise<string> {
    return todo("draftsPageTitle");
  }
  async draftsHeaderText(): Promise<string> {
    return todo("draftsHeaderText");
  }
  async draftsCountChip(): Promise<string | null> {
    return todo("draftsCountChip");
  }
  async draftsHeaderCreateState(): Promise<"enabled" | "disabled" | "absent"> {
    return todo("draftsHeaderCreateState");
  }
  async draftsMainText(): Promise<string> {
    return todo("draftsMainText");
  }
  async draftRowNames(): Promise<string[]> {
    return todo("draftRowNames");
  }
  async draftRowProjectMarker(_name: string): Promise<string> {
    return todo("draftRowProjectMarker");
  }
  async draftRowTypeMarkPresent(_name: string): Promise<boolean> {
    return todo("draftRowTypeMarkPresent");
  }
  async draftRowMenuEntries(_name: string): Promise<string[]> {
    return todo("draftRowMenuEntries");
  }
  async draftContextMenuEntries(_name: string): Promise<string[]> {
    return todo("draftContextMenuEntries");
  }
  async draftRowMenuClick(_name: string, _entry: string): Promise<void> {
    return todo("draftRowMenuClick");
  }
  async draftRowQuickActionVisible(_name: string): Promise<boolean> {
    return todo("draftRowQuickActionVisible");
  }
  async editDraftByName(_name: string): Promise<void> {
    return todo("editDraftByName");
  }
  async cancelDraftDelete(_name: string): Promise<void> {
    return todo("cancelDraftDelete");
  }
  async confirmDraftDelete(): Promise<void> {
    return todo("confirmDraftDelete");
  }
  async draftStateText(_name: string): Promise<string> {
    return todo("draftStateText");
  }
  async draftOpenStatePicker(_name: string): Promise<void> {
    return todo("draftOpenStatePicker");
  }
  async draftPriorityText(_name: string): Promise<string> {
    return todo("draftPriorityText");
  }
  async draftOpenPriorityPicker(_name: string): Promise<void> {
    return todo("draftOpenPriorityPicker");
  }
  async draftLabelsText(_name: string): Promise<string> {
    return todo("draftLabelsText");
  }
  async draftOpenLabelsPicker(_name: string): Promise<void> {
    return todo("draftOpenLabelsPicker");
  }
  async draftAssigneesText(_name: string): Promise<string> {
    return todo("draftAssigneesText");
  }
  async draftOpenAssigneesPicker(_name: string): Promise<void> {
    return todo("draftOpenAssigneesPicker");
  }
  async draftDatesText(_name: string): Promise<string> {
    return todo("draftDatesText");
  }
  async draftOpenStartDatePicker(_name: string): Promise<void> {
    return todo("draftOpenStartDatePicker");
  }
  async draftOpenDueDatePicker(_name: string): Promise<void> {
    return todo("draftOpenDueDatePicker");
  }
  async draftPickerOptionSelected(_text: string): Promise<boolean> {
    return todo("draftPickerOptionSelected");
  }
  async draftEstimateText(_name: string): Promise<string> {
    return todo("draftEstimateText");
  }
  async draftOpenEstimatePicker(_name: string): Promise<void> {
    return todo("draftOpenEstimatePicker");
  }
  async draftEstimatePickerVisible(_name: string): Promise<boolean> {
    return todo("draftEstimatePickerVisible");
  }
  async draftCycleText(_name: string): Promise<string> {
    return todo("draftCycleText");
  }
  async draftOpenCyclePicker(_name: string): Promise<void> {
    return todo("draftOpenCyclePicker");
  }
  async draftCyclePickerVisible(_name: string): Promise<boolean> {
    return todo("draftCyclePickerVisible");
  }
  async draftModuleText(_name: string): Promise<string> {
    return todo("draftModuleText");
  }
  async draftOpenModulePicker(_name: string): Promise<void> {
    return todo("draftOpenModulePicker");
  }
  async draftModulePickerVisible(_name: string): Promise<boolean> {
    return todo("draftModulePickerVisible");
  }
  async draftFailNextPatch(_status: number, _delayMs: number): Promise<void> {
    return todo("draftFailNextPatch");
  }
  async draftClearPatchFailure(): Promise<void> {
    return todo("draftClearPatchFailure");
  }
  async draftsEntryRequests(_workspaceSlug: string): Promise<string[]> {
    return todo("draftsEntryRequests");
  }
  async draftRowsDraggableCount(): Promise<number> {
    return todo("draftRowsDraggableCount");
  }
  async draftsPressKey(_key: string): Promise<void> {
    return todo("draftsPressKey");
  }
  async draftPublishError(): Promise<string | null> {
    return todo("draftPublishError");
  }
  async draftsEmptyCreateState(): Promise<"enabled" | "disabled" | "absent"> {
    return todo("draftsEmptyCreateState");
  }
  async draftsNoProjectCreateState(): Promise<"enabled" | "disabled" | "absent"> {
    return todo("draftsNoProjectCreateState");
  }
  async draftsDelayList(_ms: number): Promise<void> {
    return todo("draftsDelayList");
  }
  async draftsSkeletonVisible(): Promise<boolean> {
    return todo("draftsSkeletonVisible");
  }
  async draftsLoadMoreVisible(): Promise<boolean> {
    return todo("draftsLoadMoreVisible");
  }
  async draftsLoadMore(): Promise<void> {
    return todo("draftsLoadMore");
  }
  async draftsListFetchCountOnRefocus(): Promise<number> {
    return todo("draftsListFetchCountOnRefocus");
  }
  async draftsMainCheckboxCount(): Promise<number> {
    return todo("draftsMainCheckboxCount");
  }

  // Modules dialogs, archive and archived screen (NEWFRONT-309, MOD-022..028).
  // Oracle-only drivers; the new app has no modules surface yet.

  async modulesOpenCreateFromHeader(): Promise<void> {
    return todo("modulesOpenCreateFromHeader");
  }

  async modulesOpenCreateFromEmpty(): Promise<void> {
    return todo("modulesOpenCreateFromEmpty");
  }

  async modulesDialogOpen(): Promise<boolean> {
    return todo("modulesDialogOpen");
  }

  async modulesDialogHasProjectPicker(): Promise<boolean> {
    return todo("modulesDialogHasProjectPicker");
  }

  async modulesDialogProjectOptions(): Promise<string[]> {
    return todo("modulesDialogProjectOptions");
  }

  async modulesDialogPickProject(_name: string): Promise<void> {
    return todo("modulesDialogPickProject");
  }

  async modulesDialogTitle(): Promise<string> {
    return todo("modulesDialogTitle");
  }

  async modulesDialogFillTitle(_text: string): Promise<void> {
    return todo("modulesDialogFillTitle");
  }

  async modulesDialogTitleError(): Promise<string | null> {
    return todo("modulesDialogTitleError");
  }

  async modulesDialogDescription(): Promise<string> {
    return todo("modulesDialogDescription");
  }

  async modulesDialogFillDescription(_text: string): Promise<void> {
    return todo("modulesDialogFillDescription");
  }

  async modulesDialogStatus(): Promise<string> {
    return todo("modulesDialogStatus");
  }

  async modulesDialogPickStatus(_label: string): Promise<void> {
    return todo("modulesDialogPickStatus");
  }

  async modulesDialogLead(): Promise<string> {
    return todo("modulesDialogLead");
  }

  async modulesDialogLeadOptions(): Promise<string[]> {
    return todo("modulesDialogLeadOptions");
  }

  async modulesDialogPickLead(_label: string): Promise<void> {
    return todo("modulesDialogPickLead");
  }

  async modulesDialogMembers(): Promise<string> {
    return todo("modulesDialogMembers");
  }

  async modulesDialogPickMembers(_labels: string[]): Promise<void> {
    return todo("modulesDialogPickMembers");
  }

  async modulesDialogDateRange(): Promise<string> {
    return todo("modulesDialogDateRange");
  }

  async modulesDialogPickDateRange(_startISO: string, _endISO: string): Promise<void> {
    return todo("modulesDialogPickDateRange");
  }

  async modulesDialogTabOrder(): Promise<(number | null)[]> {
    return todo("modulesDialogTabOrder");
  }

  async modulesDialogProjectPickerTabIndex(): Promise<number | null> {
    return todo("modulesDialogProjectPickerTabIndex");
  }

  async modulesDialogCancel(): Promise<void> {
    return todo("modulesDialogCancel");
  }

  async modulesDialogEscape(): Promise<void> {
    return todo("modulesDialogEscape");
  }

  async modulesDialogSubmitCreate(): Promise<void> {
    return todo("modulesDialogSubmitCreate");
  }

  async modulesDialogSubmitUpdate(): Promise<void> {
    return todo("modulesDialogSubmitUpdate");
  }

  async modulesFailNextCreate(): Promise<void> {
    return todo("modulesFailNextCreate");
  }

  async modulesFailNextUpdate(): Promise<void> {
    return todo("modulesFailNextUpdate");
  }

  async modulesOpenDetailMenu(): Promise<void> {
    return todo("modulesOpenDetailMenu");
  }

  async modulesChooseDetailMenuEntry(_title: string): Promise<void> {
    return todo("modulesChooseDetailMenuEntry");
  }

  async modulesDeleteDialog(): Promise<ModulesDeleteDialog | null> {
    return todo("modulesDeleteDialog");
  }

  async modulesDeleteConfirm(): Promise<void> {
    return todo("modulesDeleteConfirm");
  }

  async modulesDeleteCancel(): Promise<void> {
    return todo("modulesDeleteCancel");
  }

  async modulesFailDeletePermission(): Promise<void> {
    return todo("modulesFailDeletePermission");
  }

  async modulesFailNextDelete(): Promise<void> {
    return todo("modulesFailNextDelete");
  }

  async modulesRowFacts(_name: string): Promise<ModulesRowFacts> {
    return todo("modulesRowFacts");
  }

  // --- Intake list content (NEWFRONT-256, INT-006/007/009/010/011). Throwing
  // --- stubs: the web_new driver implements these when the area is built.

  async intakeListOpen(_workspaceSlug: string, _projectId: string, _tab: "open" | "closed"): Promise<void> {
    return todo("intakeListOpen");
  }
  async intakeListRowIds(): Promise<string[]> {
    return todo("intakeListRowIds");
  }
  async intakeListRowIdLabel(_issueId: string): Promise<string> {
    return todo("intakeListRowIdLabel");
  }
  async intakeListRowStatusChip(_issueId: string): Promise<string | null> {
    return todo("intakeListRowStatusChip");
  }
  async intakeListRowTitle(_issueId: string): Promise<string> {
    return todo("intakeListRowTitle");
  }
  async intakeListRowCreatedText(_issueId: string): Promise<string> {
    return todo("intakeListRowCreatedText");
  }
  async intakeListRowCreatedTooltip(_issueId: string): Promise<IntakeListTooltip> {
    return todo("intakeListRowCreatedTooltip");
  }
  async intakeListRowPriority(_issueId: string): Promise<string> {
    return todo("intakeListRowPriority");
  }
  async intakeListRowLabels(_issueId: string): Promise<string[]> {
    return todo("intakeListRowLabels");
  }
  async intakeListRowAvatarKind(_issueId: string): Promise<"member" | "intake" | "none"> {
    return todo("intakeListRowAvatarKind");
  }
  async intakeListScrollToBottom(): Promise<void> {
    return todo("intakeListScrollToBottom");
  }
  async intakeListSkeletonsVisible(): Promise<boolean> {
    return todo("intakeListSkeletonsVisible");
  }
  async intakeListSyncingVisible(): Promise<boolean> {
    return todo("intakeListSyncingVisible");
  }
  async intakeListWaitForRowCount(_count: number): Promise<void> {
    return todo("intakeListWaitForRowCount");
  }
  async intakeListDelayNextListReads(_ms: number): Promise<void> {
    return todo("intakeListDelayNextListReads");
  }
  async intakeListOpenFilters(): Promise<void> {
    return todo("intakeListOpenFilters");
  }
  async intakeListCloseFilters(): Promise<void> {
    return todo("intakeListCloseFilters");
  }
  async intakeListFilterSearch(_query: string): Promise<void> {
    return todo("intakeListFilterSearch");
  }
  async intakeListFilterSections(): Promise<IntakeListFilterKey[]> {
    return todo("intakeListFilterSections");
  }
  async intakeListFilterOptions(_key: IntakeListFilterKey): Promise<string[]> {
    return todo("intakeListFilterOptions");
  }
  async intakeListFilterPick(_key: IntakeListFilterKey, _option: string): Promise<void> {
    return todo("intakeListFilterPick");
  }
  async intakeListFilterChecked(_key: IntakeListFilterKey, _option: string): Promise<boolean> {
    return todo("intakeListFilterChecked");
  }
  async intakeListFilterSectionCount(_key: IntakeListFilterKey): Promise<number> {
    return todo("intakeListFilterSectionCount");
  }
  async intakeListChips(): Promise<IntakeListChip[]> {
    return todo("intakeListChips");
  }
  async intakeListChipRemove(_key: IntakeListFilterKey, _value: string): Promise<void> {
    return todo("intakeListChipRemove");
  }
  async intakeListChipClearGroup(_key: IntakeListFilterKey): Promise<void> {
    return todo("intakeListChipClearGroup");
  }
  async intakeListBeginRequestSpy(): Promise<void> {
    return todo("intakeListBeginRequestSpy");
  }
  async intakeListLastRequestParams(): Promise<Record<string, string> | null> {
    return todo("intakeListLastRequestParams");
  }
  async intakeListOpenOrderMenu(): Promise<void> {
    return todo("intakeListOpenOrderMenu");
  }
  async intakeListOrderState(): Promise<IntakeListOrderState> {
    return todo("intakeListOrderState");
  }
  async intakeListPickOrderField(_field: IntakeListOrderField): Promise<void> {
    return todo("intakeListPickOrderField");
  }
  async intakeListPickOrderDirection(_direction: "asc" | "desc"): Promise<void> {
    return todo("intakeListPickOrderDirection");
  }
  async intakeListOpenCustomDate(_key: "createdAt" | "updatedAt"): Promise<void> {
    return todo("intakeListOpenCustomDate");
  }
  async intakeListCustomDateApply(_fromISO: string, _toISO: string): Promise<void> {
    return todo("intakeListCustomDateApply");
  }

  // --- Active-cycle hero and cycles cross-cutting (NEWFRONT-254,
  // --- CYC-040..045, CYC-047..048). Appended; existing stubs above are
  // --- untouched per the shared driver contract.
  async cyclesHeroOpenList(_workspaceSlug: string, _projectId: string): Promise<void> {
    return todo("cyclesHeroOpenList");
  }
  async cyclesHeroOpenDetail(_workspaceSlug: string, _projectId: string, _cycleId: string): Promise<void> {
    return todo("cyclesHeroOpenDetail");
  }
  async cyclesHeroOpenPeekRaw(_workspaceSlug: string, _projectId: string, _cycleId: string): Promise<void> {
    return todo("cyclesHeroOpenPeekRaw");
  }
  async cyclesHeroReload(): Promise<void> {
    return todo("cyclesHeroReload");
  }
  async cyclesHeroDelayDetailReads(_ms: number): Promise<void> {
    return todo("cyclesHeroDelayDetailReads");
  }
  async cyclesHeroHideCycleFromListReads(_cycleId: string): Promise<void> {
    return todo("cyclesHeroHideCycleFromListReads");
  }
  async cyclesHeroSidebarSkeletonVisible(): Promise<boolean> {
    return todo("cyclesHeroSidebarSkeletonVisible");
  }
  async cyclesHeroSidebarName(): Promise<string | null> {
    return todo("cyclesHeroSidebarName");
  }
  async cyclesHeroPeekSkeletonVisible(): Promise<boolean> {
    return todo("cyclesHeroPeekSkeletonVisible");
  }
  async cyclesHeroPeekName(): Promise<string | null> {
    return todo("cyclesHeroPeekName");
  }
  async cyclesHeroGroupHeading(): Promise<string | null> {
    return todo("cyclesHeroGroupHeading");
  }
  async cyclesHeroActiveName(): Promise<string | null> {
    return todo("cyclesHeroActiveName");
  }
  async cyclesHeroEmptyCopy(): Promise<{ title: string; description: string } | null> {
    return todo("cyclesHeroEmptyCopy");
  }
  async cyclesHeroProgressGroups(): Promise<{ closed: string | null; groups: CyclesHeroProgressGroup[] }> {
    return todo("cyclesHeroProgressGroups");
  }
  async cyclesHeroProgressEmptyVisible(): Promise<boolean> {
    return todo("cyclesHeroProgressEmptyVisible");
  }
  async cyclesHeroBurndown(): Promise<{ heading: string | null; pending: string | null; chart: boolean }> {
    return todo("cyclesHeroBurndown");
  }
  async cyclesHeroBurndownEmptyVisible(): Promise<boolean> {
    return todo("cyclesHeroBurndownEmptyVisible");
  }
  async cyclesHeroBreakdownTabs(): Promise<string[]> {
    return todo("cyclesHeroBreakdownTabs");
  }
  async cyclesHeroBreakdownSelectTab(_label: string): Promise<void> {
    return todo("cyclesHeroBreakdownSelectTab");
  }
  async cyclesHeroBreakdownEntries(): Promise<string[]> {
    return todo("cyclesHeroBreakdownEntries");
  }
  async cyclesHeroBreakdownEmptyVisible(): Promise<boolean> {
    return todo("cyclesHeroBreakdownEmptyVisible");
  }
  async cyclesHeroClickProgressGroup(_group: string): Promise<void> {
    return todo("cyclesHeroClickProgressGroup");
  }
  async cyclesHeroTapProgressGroup(_group: string): Promise<void> {
    return todo("cyclesHeroTapProgressGroup");
  }
  async cyclesHeroClickBreakdownEntry(_index: number): Promise<void> {
    return todo("cyclesHeroClickBreakdownEntry");
  }
  async cyclesHeroOpenWorkspaceActives(_workspaceSlug: string): Promise<void> {
    return todo("cyclesHeroOpenWorkspaceActives");
  }
  async cyclesHeroUpsell(): Promise<CyclesHeroUpsell> {
    return todo("cyclesHeroUpsell");
  }
  async cyclesHeroUpsellBadgeVisible(): Promise<boolean> {
    return todo("cyclesHeroUpsellBadgeVisible");
  }
  async cyclesHeroActiveRowOffset(): Promise<string | null> {
    return todo("cyclesHeroActiveRowOffset");
  }
  async cyclesHeroHoverActiveRowDates(): Promise<void> {
    return todo("cyclesHeroHoverActiveRowDates");
  }
  async cyclesHeroSidebarOffset(): Promise<string | null> {
    return todo("cyclesHeroSidebarOffset");
  }
  async cyclesHeroHoverSidebarDates(): Promise<void> {
    return todo("cyclesHeroHoverSidebarDates");
  }
  async cyclesHeroTooltipText(): Promise<string | null> {
    return todo("cyclesHeroTooltipText");
  }
  async cyclesHeroBeginTrafficSpy(): Promise<void> {
    return todo("cyclesHeroBeginTrafficSpy");
  }
  async cyclesHeroTrafficCounts(): Promise<CyclesHeroTrafficCounts> {
    return todo("cyclesHeroTrafficCounts");
  }
  async cyclesHeroOpenCreate(): Promise<void> {
    return todo("cyclesHeroOpenCreate");
  }
  async cyclesHeroFillCreateName(_name: string): Promise<void> {
    return todo("cyclesHeroFillCreateName");
  }
  async cyclesHeroSubmitCreate(): Promise<void> {
    return todo("cyclesHeroSubmitCreate");
  }
  async cyclesHeroCreateOpen(): Promise<boolean> {
    return todo("cyclesHeroCreateOpen");
  }
  async cyclesHeroVisibleNames(): Promise<string[]> {
    return todo("cyclesHeroVisibleNames");
  }
  async cyclesHeroTransferBanner(_name: string): Promise<string | null> {
    return todo("cyclesHeroTransferBanner");
  }
  async cyclesHeroOpenTransfer(_name: string): Promise<void> {
    return todo("cyclesHeroOpenTransfer");
  }
  async cyclesHeroTransferSearch(_text: string): Promise<void> {
    return todo("cyclesHeroTransferSearch");
  }
  async cyclesHeroTransferOptions(): Promise<string[]> {
    return todo("cyclesHeroTransferOptions");
  }
  async cyclesHeroTransferPick(_name: string): Promise<void> {
    return todo("cyclesHeroTransferPick");
  }
  async cyclesHeroFocusWindow(): Promise<void> {
    return todo("cyclesHeroFocusWindow");
  }
  async cyclesHeroCreateButtonState(): Promise<"absent" | "disabled" | "enabled"> {
    return todo("cyclesHeroCreateButtonState");
  }
  async cyclesHeroOpenRowMenu(_name: string): Promise<void> {
    return todo("cyclesHeroOpenRowMenu");
  }
  async cyclesHeroRowMenuEntries(): Promise<string[]> {
    return todo("cyclesHeroRowMenuEntries");
  }
  async cyclesHeroCloseMenus(): Promise<void> {
    return todo("cyclesHeroCloseMenus");
  }
  async cyclesHeroSidebarDateDisabled(): Promise<boolean> {
    return todo("cyclesHeroSidebarDateDisabled");
  }
  async cyclesHeroFavoriteVisible(_name: string): Promise<boolean> {
    return todo("cyclesHeroFavoriteVisible");
  }
  async cyclesHeroSearchVisible(): Promise<boolean> {
    return todo("cyclesHeroSearchVisible");
  }
  async cyclesHeroFilterVisible(): Promise<boolean> {
    return todo("cyclesHeroFilterVisible");
  }
  async cyclesHeroInlineActionsVisible(_name: string): Promise<boolean> {
    return todo("cyclesHeroInlineActionsVisible");
  }
  async cyclesHeroHoverActionsVisible(_name: string): Promise<boolean> {
    return todo("cyclesHeroHoverActionsVisible");
  }
  async cyclesHeroLayoutMenuVisible(): Promise<boolean> {
    return todo("cyclesHeroLayoutMenuVisible");
  }
  async cyclesHeroLayoutOptions(): Promise<string[]> {
    return todo("cyclesHeroLayoutOptions");
  }
  async cyclesHeroPickLayout(_option: string): Promise<void> {
    return todo("cyclesHeroPickLayout");
  }
  async cyclesHeroDetailLayoutMenuVisible(): Promise<boolean> {
    return todo("cyclesHeroDetailLayoutMenuVisible");
  }
  async cyclesHeroSearchOpen(): Promise<void> {
    return todo("cyclesHeroSearchOpen");
  }
  async cyclesHeroSearchType(_text: string): Promise<void> {
    return todo("cyclesHeroSearchType");
  }
  async cyclesHeroSearchText(): Promise<string> {
    return todo("cyclesHeroSearchText");
  }
  async cyclesHeroFilterMenuTexts(): Promise<string[]> {
    return todo("cyclesHeroFilterMenuTexts");
  }
  async cyclesHeroOpenPeek(_name: string): Promise<void> {
    return todo("cyclesHeroOpenPeek");
  }
  async cyclesHeroRowMenuClick(_entry: string): Promise<void> {
    return todo("cyclesHeroRowMenuClick");
  }
  async cyclesHeroGrantClipboard(): Promise<void> {
    return todo("cyclesHeroGrantClipboard");
  }
  async cyclesHeroReadClipboard(): Promise<string> {
    return todo("cyclesHeroReadClipboard");
  }
  async cyclesHeroDismissSidebar(): Promise<void> {
    return todo("cyclesHeroDismissSidebar");
  }
  async cyclesHeroDetailLayoutOptions(): Promise<string[]> {
    return todo("cyclesHeroDetailLayoutOptions");
  }
  async cyclesHeroDetailPickLayout(_option: string): Promise<void> {
    return todo("cyclesHeroDetailPickLayout");
  }

  // --- Estimates permissions matrix + edition split (NEWFRONT-326,
  // --- EST-024 + EST-029). Stage 0 oracle convention: todo() stubs until
  // --- the estimates area lands in apps/web_new. The shell child's
  // --- navigation (NEWFRONT-321, merged) is stubbed above.

  async estimatesEnableSwitchVisible(): Promise<boolean> {
    return todo("estimatesEnableSwitchVisible");
  }
  async estimatesActiveSystemName(): Promise<string> {
    return todo("estimatesActiveSystemName");
  }
  async estimatesActiveRowButtonCount(): Promise<number> {
    return todo("estimatesActiveRowButtonCount");
  }
  async estimatesOpenCreateDialog(): Promise<void> {
    return todo("estimatesOpenCreateDialog");
  }
  async estimatesDialogOpen(): Promise<boolean> {
    return todo("estimatesDialogOpen");
  }
  async estimatesFamilyOptions(): Promise<EstimatesFamilyOption[]> {
    return todo("estimatesFamilyOptions");
  }
  async estimatesSelectFamily(_family: "points" | "categories"): Promise<void> {
    return todo("estimatesSelectFamily");
  }
  async estimatesPickStarter(_title: string): Promise<void> {
    return todo("estimatesPickStarter");
  }
  async estimatesSubmit(): Promise<void> {
    return todo("estimatesSubmit");
  }
  async estimatesActiveRowDeleteClick(): Promise<void> {
    return todo("estimatesActiveRowDeleteClick");
  }
  async estimatesConfirmDialogOpen(): Promise<boolean> {
    return todo("estimatesConfirmDialogOpen");
  }
  async estimatesConfirmDialogCancel(): Promise<void> {
    return todo("estimatesConfirmDialogCancel");
  }
  async estimatesEditDeleteConfirmDialogTitle(): Promise<string> {
    return todo("estimatesEditDeleteConfirmDialogTitle");
  }
  async estimatesEditDeleteConfirmDialogBody(): Promise<string> {
    return todo("estimatesEditDeleteConfirmDialogBody");
  }
  async estimatesEditDeleteConfirmDialogConfirm(): Promise<void> {
    return todo("estimatesEditDeleteConfirmDialogConfirm");
  }
  async estimatesEditDeleteActiveRowEditControlVisible(): Promise<boolean> {
    return todo("estimatesEditDeleteActiveRowEditControlVisible");
  }
  // --- Pages gating, list states, URLs, detail shell (NEWFRONT-314,
  // --- PAGE-018-025,079-080). Throwing stubs per the shared driver contract.
  async pagesGatingOpenList(
    _workspaceSlug: string,
    _projectId: string,
    _query?: string,
    _settle?: boolean
  ): Promise<void> {
    return todo("pagesGatingOpenList");
  }
  async pagesGatingOpenDetail(
    _workspaceSlug: string,
    _projectId: string,
    _pageId: string,
    _query?: string
  ): Promise<void> {
    return todo("pagesGatingOpenDetail");
  }
  async pagesGatingCurrentUrl(): Promise<string> {
    return todo("pagesGatingCurrentUrl");
  }
  async pagesGatingQueryParam(_key: string): Promise<string | null> {
    return todo("pagesGatingQueryParam");
  }
  async pagesGatingDocTitle(): Promise<string> {
    return todo("pagesGatingDocTitle");
  }
  async pagesGatingRowTitles(): Promise<string[]> {
    return todo("pagesGatingRowTitles");
  }
  async pagesGatingRowHref(_title: string): Promise<string | null> {
    return todo("pagesGatingRowHref");
  }
  async pagesGatingActiveTab(): Promise<PagesGatingTab> {
    return todo("pagesGatingActiveTab");
  }
  async pagesGatingClickTab(_tab: PagesGatingTab): Promise<void> {
    return todo("pagesGatingClickTab");
  }
  async pagesGatingCreateOffered(): Promise<boolean> {
    return todo("pagesGatingCreateOffered");
  }
  async pagesGatingRowStarOffered(_title: string): Promise<boolean> {
    return todo("pagesGatingRowStarOffered");
  }
  async pagesGatingRowMenuItems(_title: string): Promise<string[]> {
    return todo("pagesGatingRowMenuItems");
  }
  async pagesGatingDetailCluster(): Promise<PagesGatingHeaderCluster> {
    return todo("pagesGatingDetailCluster");
  }
  async pagesGatingDetailMenuItems(): Promise<string[]> {
    return todo("pagesGatingDetailMenuItems");
  }
  async pagesGatingEditableCount(): Promise<number> {
    return todo("pagesGatingEditableCount");
  }
  async pagesGatingErrorPanelVisible(): Promise<boolean> {
    return todo("pagesGatingErrorPanelVisible");
  }
  async pagesGatingErrorPanel(): Promise<PagesGatingErrorPanel> {
    return todo("pagesGatingErrorPanel");
  }
  async pagesGatingListVariant(): Promise<PagesGatingListVariant> {
    return todo("pagesGatingListVariant");
  }
  async pagesGatingSkeletonCount(): Promise<number> {
    return todo("pagesGatingSkeletonCount");
  }
  async pagesGatingEmptyCreateState(): Promise<PagesGatingCreateState> {
    return todo("pagesGatingEmptyCreateState");
  }
  async pagesGatingStallListFetch(): Promise<void> {
    return todo("pagesGatingStallListFetch");
  }
  async pagesGatingReleaseListFetch(): Promise<void> {
    return todo("pagesGatingReleaseListFetch");
  }
  async pagesGatingFailListFetch(): Promise<void> {
    return todo("pagesGatingFailListFetch");
  }
  async pagesGatingUnfailListFetch(): Promise<void> {
    return todo("pagesGatingUnfailListFetch");
  }
  async pagesGatingFailListCreates(): Promise<void> {
    return todo("pagesGatingFailListCreates");
  }
  async pagesGatingUnfailListCreates(): Promise<void> {
    return todo("pagesGatingUnfailListCreates");
  }
  async pagesGatingToastTexts(): Promise<string[]> {
    return todo("pagesGatingToastTexts");
  }
  async pagesGatingStartRequestSpy(): Promise<void> {
    return todo("pagesGatingStartRequestSpy");
  }
  async pagesGatingLoggedRequests(): Promise<PagesGatingRequest[]> {
    return todo("pagesGatingLoggedRequests");
  }
  async pagesGatingResetRequestSpy(): Promise<void> {
    return todo("pagesGatingResetRequestSpy");
  }
  async pagesGatingSearchType(_text: string): Promise<void> {
    return todo("pagesGatingSearchType");
  }
  async pagesGatingSearchValue(): Promise<string | null> {
    return todo("pagesGatingSearchValue");
  }
  async pagesGatingSortLabel(): Promise<string> {
    return todo("pagesGatingSortLabel");
  }
  async pagesGatingSortCycle(): Promise<void> {
    return todo("pagesGatingSortCycle");
  }
  async pagesGatingFilterToggleFavorites(): Promise<void> {
    return todo("pagesGatingFilterToggleFavorites");
  }
  async pagesGatingChips(): Promise<string[]> {
    return todo("pagesGatingChips");
  }
  async pagesGatingQuickCreate(): Promise<string> {
    return todo("pagesGatingQuickCreate");
  }
  async pagesGatingWaitDetailSettled(): Promise<"editor" | "panel"> {
    return todo("pagesGatingWaitDetailSettled");
  }
  async pagesGatingDetailSpinnerVisible(): Promise<boolean> {
    return todo("pagesGatingDetailSpinnerVisible");
  }
  async pagesGatingStallDetailFetch(_pageId: string): Promise<void> {
    return todo("pagesGatingStallDetailFetch");
  }
  async pagesGatingReleaseDetailFetch(): Promise<void> {
    return todo("pagesGatingReleaseDetailFetch");
  }
  async pagesGatingBreadcrumbProject(): Promise<string | null> {
    return todo("pagesGatingBreadcrumbProject");
  }
  async pagesGatingBackLinkHref(): Promise<string | null> {
    return todo("pagesGatingBackLinkHref");
  }
  async pagesGatingSwitcherOptions(): Promise<string[]> {
    return todo("pagesGatingSwitcherOptions");
  }
  async pagesGatingSwitcherSearch(_text: string): Promise<string[]> {
    return todo("pagesGatingSwitcherSearch");
  }
  async pagesGatingSwitcherSelect(_name: string): Promise<void> {
    return todo("pagesGatingSwitcherSelect");
  }
  async pagesGatingTitleFacts(_pageId: string): Promise<PagesGatingTitleFacts> {
    return todo("pagesGatingTitleFacts");
  }
  async pagesGatingBodySkeletonVisible(): Promise<boolean> {
    return todo("pagesGatingBodySkeletonVisible");
  }
  async pagesGatingPaneActiveTab(): Promise<string | null> {
    return todo("pagesGatingPaneActiveTab");
  }
  async pagesGatingClickVersionLink(_versionId: string): Promise<void> {
    return todo("pagesGatingClickVersionLink");
  }
  async pagesGatingOverlayOpen(): Promise<boolean> {
    return todo("pagesGatingOverlayOpen");
  }
  async pagesGatingOverlayTitle(): Promise<string> {
    return todo("pagesGatingOverlayTitle");
  }
  async pagesGatingOverlayText(): Promise<string> {
    return todo("pagesGatingOverlayText");
  }
  async pagesGatingOverlaySkeletonVisible(): Promise<boolean> {
    return todo("pagesGatingOverlaySkeletonVisible");
  }
  async pagesGatingStallVersionDetail(): Promise<void> {
    return todo("pagesGatingStallVersionDetail");
  }
  async pagesGatingReleaseVersionDetail(): Promise<void> {
    return todo("pagesGatingReleaseVersionDetail");
  }
  async pagesGatingClickOptionsItem(_name: string): Promise<void> {
    return todo("pagesGatingClickOptionsItem");
  }
  async pagesGatingCopyLinkText(): Promise<string> {
    return todo("pagesGatingCopyLinkText");
  }
  async pagesGatingSignOut(): Promise<void> {
    return todo("pagesGatingSignOut");
  }
  async pagesGatingClickCreate(): Promise<void> {
    return todo("pagesGatingClickCreate");
  }
  async pagesGatingOpenRow(_title: string): Promise<void> {
    return todo("pagesGatingOpenRow");
  }
  async pagesGatingGoBack(): Promise<void> {
    return todo("pagesGatingGoBack");
  }

  // Modules list frame and header controls (NEWFRONT-306, MOD-001..007). Stubs;
  // the modules area lands on web_new later. Appended per the shared contract.

  async modulesRowNames(): Promise<string[]> {
    return todo("modulesRowNames");
  }

  async modulesBreadcrumb(): Promise<ModulesBreadcrumb> {
    return todo("modulesBreadcrumb");
  }

  async modulesPageTitle(): Promise<string> {
    return todo("modulesPageTitle");
  }

  async modulesAddLabel(): Promise<string | null> {
    return todo("modulesAddLabel");
  }

  async modulesClickAdd(): Promise<void> {
    return todo("modulesClickAdd");
  }

  async modulesCreateDialogOpen(): Promise<boolean> {
    return todo("modulesCreateDialogOpen");
  }

  async modulesSearchVisible(): Promise<boolean> {
    return todo("modulesSearchVisible");
  }

  async modulesOpenSearch(): Promise<void> {
    return todo("modulesOpenSearch");
  }

  async modulesTypeSearch(_text: string): Promise<void> {
    return todo("modulesTypeSearch");
  }

  async modulesSearchText(): Promise<string> {
    return todo("modulesSearchText");
  }

  async modulesSearchFocused(): Promise<boolean> {
    return todo("modulesSearchFocused");
  }

  async modulesEscapeSearch(): Promise<void> {
    return todo("modulesEscapeSearch");
  }

  async modulesClickOutsideSearch(): Promise<void> {
    return todo("modulesClickOutsideSearch");
  }

  async modulesSortTriggerLabel(): Promise<string> {
    return todo("modulesSortTriggerLabel");
  }

  async modulesSetSortViaMenu(_label: string): Promise<void> {
    return todo("modulesSetSortViaMenu");
  }

  async modulesSortMenuItems(): Promise<string[]> {
    return todo("modulesSortMenuItems");
  }

  async modulesOpenFilters(): Promise<void> {
    return todo("modulesOpenFilters");
  }

  async modulesCloseFilters(): Promise<void> {
    return todo("modulesCloseFilters");
  }

  async modulesFilterGroups(): Promise<string[]> {
    return todo("modulesFilterGroups");
  }

  async modulesFilterGroupOptions(_group: string): Promise<string[]> {
    return todo("modulesFilterGroupOptions");
  }

  async modulesToggleFilterOption(_group: string, _label: string): Promise<void> {
    return todo("modulesToggleFilterOption");
  }

  async modulesExpandFilterGroup(_group: string): Promise<void> {
    return todo("modulesExpandFilterGroup");
  }

  async modulesFilterGroupExpandable(_group: string): Promise<boolean> {
    return todo("modulesFilterGroupExpandable");
  }

  async modulesMenuSearch(_text: string): Promise<void> {
    return todo("modulesMenuSearch");
  }

  async modulesFiltersActive(): Promise<boolean> {
    return todo("modulesFiltersActive");
  }

  async modulesOpenCustomDate(_group: "Start date" | "Due date"): Promise<void> {
    return todo("modulesOpenCustomDate");
  }

  async modulesCustomDateOpen(): Promise<boolean> {
    return todo("modulesCustomDateOpen");
  }

  async modulesApplyCustomDate(): Promise<void> {
    return todo("modulesApplyCustomDate");
  }

  async modulesChips(): Promise<ModulesChip[]> {
    return todo("modulesChips");
  }

  async modulesRemoveChipValue(_key: string, _value: string): Promise<void> {
    return todo("modulesRemoveChipValue");
  }

  async modulesRemoveChipDimension(_key: string): Promise<void> {
    return todo("modulesRemoveChipDimension");
  }

  async modulesClearFilters(): Promise<void> {
    return todo("modulesClearFilters");
  }

  async modulesChipsVisible(): Promise<boolean> {
    return todo("modulesChipsVisible");
  }

  async modulesGateFacts(): Promise<ModulesGateFacts> {
    return todo("modulesGateFacts");
  }

  async modulesGateToFeatures(): Promise<void> {
    return todo("modulesGateToFeatures");
  }

  async modulesTimelineRowNames(): Promise<string[]> {
    return todo("modulesTimelineRowNames");
  }

  async modulesDragTimelineRowToSelf(_name: string): Promise<void> {
    return todo("modulesDragTimelineRowToSelf");
  }

  async modulesManualSortWarningVisible(): Promise<boolean> {
    return todo("modulesManualSortWarningVisible");
  }

  // --- Pages list discovery (NEWFRONT-312, PAGE-001-006).

  async pagesListOpenProjectPages(_workspaceSlug: string, _projectId: string, _query?: string): Promise<void> {
    return todo("pagesListOpenProjectPages");
  }

  async pagesListGateVisible(): Promise<boolean> {
    return todo("pagesListGateVisible");
  }

  async pagesListGateManageEnabled(): Promise<boolean> {
    return todo("pagesListGateManageEnabled");
  }

  async pagesListGateOpenManage(): Promise<void> {
    return todo("pagesListGateOpenManage");
  }

  async pagesListTabsVisible(): Promise<boolean> {
    return todo("pagesListTabsVisible");
  }

  async pagesListTabs(): Promise<string[]> {
    return todo("pagesListTabs");
  }

  async pagesListActiveTab(): Promise<PagesListTab> {
    return todo("pagesListActiveTab");
  }

  async pagesListMarkedTab(): Promise<PagesListTab | null> {
    return todo("pagesListMarkedTab");
  }

  async pagesListTabHref(_tab: PagesListTab): Promise<string | null> {
    return todo("pagesListTabHref");
  }

  async pagesListClickTab(_tab: PagesListTab): Promise<void> {
    return todo("pagesListClickTab");
  }

  async pagesListRowTitles(): Promise<string[]> {
    return todo("pagesListRowTitles");
  }

  async pagesListSearchOpen(): Promise<void> {
    return todo("pagesListSearchOpen");
  }

  async pagesListSearchExpanded(): Promise<boolean> {
    return todo("pagesListSearchExpanded");
  }

  async pagesListSearchType(_text: string): Promise<void> {
    return todo("pagesListSearchType");
  }

  async pagesListSearchValue(): Promise<string | null> {
    return todo("pagesListSearchValue");
  }

  async pagesListSearchEscape(): Promise<void> {
    return todo("pagesListSearchEscape");
  }

  async pagesListSearchClear(): Promise<void> {
    return todo("pagesListSearchClear");
  }

  async pagesListClickAway(): Promise<void> {
    return todo("pagesListClickAway");
  }

  async pagesListNoResultsVisible(): Promise<boolean> {
    return todo("pagesListNoResultsVisible");
  }

  async pagesListSortState(): Promise<{ label: string; descending: boolean }> {
    return todo("pagesListSortState");
  }

  async pagesListSortOpen(): Promise<void> {
    return todo("pagesListSortOpen");
  }

  async pagesListSortOptions(): Promise<string[]> {
    return todo("pagesListSortOptions");
  }

  async pagesListSortChooseKey(_label: string): Promise<void> {
    return todo("pagesListSortChooseKey");
  }

  async pagesListSortChooseDirection(_label: string): Promise<void> {
    return todo("pagesListSortChooseDirection");
  }

  async pagesListFiltersOpen(): Promise<void> {
    return todo("pagesListFiltersOpen");
  }

  async pagesListFiltersClose(): Promise<void> {
    return todo("pagesListFiltersClose");
  }

  async pagesListFilterToggleFavorites(): Promise<void> {
    return todo("pagesListFilterToggleFavorites");
  }

  async pagesListFilterChooseDate(_label: string): Promise<void> {
    return todo("pagesListFilterChooseDate");
  }

  async pagesListFilterSearch(_text: string): Promise<void> {
    return todo("pagesListFilterSearch");
  }

  async pagesListFilterCreatorOptions(): Promise<string[]> {
    return todo("pagesListFilterCreatorOptions");
  }

  async pagesListFilterPickCreator(_label: string): Promise<void> {
    return todo("pagesListFilterPickCreator");
  }

  async pagesListChips(): Promise<string[]> {
    return todo("pagesListChips");
  }

  async pagesListChipRemove(_keyLabel: string): Promise<void> {
    return todo("pagesListChipRemove");
  }

  async pagesListClearAll(): Promise<void> {
    return todo("pagesListClearAll");
  }

  async pagesListRowDetail(_title: string): Promise<PagesListRowDetail> {
    return todo("pagesListRowDetail");
  }

  async pagesListClickRow(_title: string): Promise<void> {
    return todo("pagesListClickRow");
  }

  async pagesListDismissMenus(): Promise<void> {
    return todo("pagesListDismissMenus");
  }

  async pagesListRowMenuItems(_title: string): Promise<string[]> {
    return todo("pagesListRowMenuItems");
  }

  async pagesListRowRightClick(_title: string): Promise<void> {
    return todo("pagesListRowRightClick");
  }

  async pagesListContextMenuItems(): Promise<string[]> {
    return todo("pagesListContextMenuItems");
  }

  // --- Pages create, star, row actions, move (NEWFRONT-313, PAGE-007-017). ---

  async pagesRowOpenList(_workspaceSlug: string, _projectId: string, _tab?: PagesRowTab): Promise<void> {
    return todo("pagesRowOpenList");
  }

  async pagesRowOpenDetail(_workspaceSlug: string, _projectId: string, _pageId: string): Promise<void> {
    return todo("pagesRowOpenDetail");
  }

  async pagesRowTitles(): Promise<string[]> {
    return todo("pagesRowTitles");
  }

  async pagesRowActiveTab(): Promise<PagesRowTab | null> {
    return todo("pagesRowActiveTab");
  }

  async pagesRowSelectTab(_tab: PagesRowTab): Promise<void> {
    return todo("pagesRowSelectTab");
  }

  async pagesCreateQuickVisible(): Promise<boolean> {
    return todo("pagesCreateQuickVisible");
  }

  async pagesCreateQuick(): Promise<string> {
    return todo("pagesCreateQuick");
  }

  async pagesCreateFailNextCreate(_status?: number): Promise<void> {
    return todo("pagesCreateFailNextCreate");
  }

  async pagesCreateOpenDialog(): Promise<void> {
    return todo("pagesCreateOpenDialog");
  }

  async pagesCreateDialogVisible(): Promise<boolean> {
    return todo("pagesCreateDialogVisible");
  }

  async pagesCreateDialogSetTitle(_text: string): Promise<void> {
    return todo("pagesCreateDialogSetTitle");
  }

  async pagesCreateDialogTitleValue(): Promise<string> {
    return todo("pagesCreateDialogTitleValue");
  }

  async pagesCreateDialogOverLengthVisible(): Promise<boolean> {
    return todo("pagesCreateDialogOverLengthVisible");
  }

  async pagesCreateDialogSubmitEnabled(): Promise<boolean> {
    return todo("pagesCreateDialogSubmitEnabled");
  }

  async pagesCreateDialogAccess(): Promise<"open" | "restricted"> {
    return todo("pagesCreateDialogAccess");
  }

  async pagesCreateDialogSetAccess(_which: "open" | "restricted"): Promise<void> {
    return todo("pagesCreateDialogSetAccess");
  }

  async pagesCreateDialogPickIconMark(): Promise<void> {
    return todo("pagesCreateDialogPickIconMark");
  }

  async pagesCreateDialogHasMark(): Promise<boolean> {
    return todo("pagesCreateDialogHasMark");
  }

  async pagesCreateDialogCancel(): Promise<void> {
    return todo("pagesCreateDialogCancel");
  }

  async pagesCreateDialogSubmit(): Promise<string> {
    return todo("pagesCreateDialogSubmit");
  }

  async pagesRowStarVisible(_title: string): Promise<boolean> {
    return todo("pagesRowStarVisible");
  }

  async pagesRowStarSelected(_title: string): Promise<boolean> {
    return todo("pagesRowStarSelected");
  }

  async pagesRowToggleStar(_title: string): Promise<void> {
    return todo("pagesRowToggleStar");
  }

  async pagesRowFilterFavoritesOnly(): Promise<void> {
    return todo("pagesRowFilterFavoritesOnly");
  }

  async pagesRowClearFilters(): Promise<void> {
    return todo("pagesRowClearFilters");
  }

  async pagesRowFavoritesNavOpen(): Promise<boolean> {
    return todo("pagesRowFavoritesNavOpen");
  }

  async pagesRowOpenMenu(_title: string): Promise<void> {
    return todo("pagesRowOpenMenu");
  }

  async pagesRowOpenContextMenu(_title: string): Promise<void> {
    return todo("pagesRowOpenContextMenu");
  }

  async pagesRowMenuItems(): Promise<string[]> {
    return todo("pagesRowMenuItems");
  }

  async pagesRowMenuClick(_item: string): Promise<void> {
    return todo("pagesRowMenuClick");
  }

  async pagesRowOpenInNewTab(_title: string): Promise<string> {
    return todo("pagesRowOpenInNewTab");
  }

  async pagesRowCopyLink(_title: string): Promise<void> {
    return todo("pagesRowCopyLink");
  }

  async pagesRowDuplicate(_title: string): Promise<void> {
    return todo("pagesRowDuplicate");
  }

  async pagesRowToggleAccess(_title: string): Promise<void> {
    return todo("pagesRowToggleAccess");
  }

  async pagesRowToggleLock(_title: string): Promise<void> {
    return todo("pagesRowToggleLock");
  }

  async pagesRowToggleArchive(_title: string): Promise<void> {
    return todo("pagesRowToggleArchive");
  }

  async pagesRowDeleteModalVisible(): Promise<boolean> {
    return todo("pagesRowDeleteModalVisible");
  }

  async pagesRowDeleteModalText(): Promise<string> {
    return todo("pagesRowDeleteModalText");
  }

  async pagesRowDeleteConfirmBusy(): Promise<boolean> {
    return todo("pagesRowDeleteConfirmBusy");
  }

  async pagesRowDeleteModalCancel(): Promise<void> {
    return todo("pagesRowDeleteModalCancel");
  }

  async pagesRowDeleteModalConfirm(): Promise<void> {
    return todo("pagesRowDeleteModalConfirm");
  }

  async pagesRowSlowNextDelete(_ms?: number): Promise<void> {
    return todo("pagesRowSlowNextDelete");
  }

  async pagesRowDetailOpenOptions(): Promise<void> {
    return todo("pagesRowDetailOpenOptions");
  }

  async pagesRowDetailReadOnly(): Promise<boolean> {
    return todo("pagesRowDetailReadOnly");
  }

  async pagesRowDetailMoveVisible(): Promise<boolean> {
    return todo("pagesRowDetailMoveVisible");
  }

  async pagesRowDetailNotFoundVisible(): Promise<boolean> {
    return todo("pagesRowDetailNotFoundVisible");
  }

  async pagesRowDetailNotFoundBackToList(): Promise<void> {
    return todo("pagesRowDetailNotFoundBackToList");
  }

  // --- Estimates enable switch, active/archived display, row gating
  // --- (NEWFRONT-322, EST-004-007). Stubs; the new app implements
  // --- them when the area is built.
  async estimatesEnableSwitchOn(): Promise<boolean> {
    return todo("estimatesEnableSwitchOn");
  }

  async estimatesEnableSwitchDisabled(): Promise<boolean> {
    return todo("estimatesEnableSwitchDisabled");
  }

  async estimatesEnableSwitchToggle(): Promise<void> {
    return todo("estimatesEnableSwitchToggle");
  }

  async estimatesFailProjectUpdate(): Promise<void> {
    return todo("estimatesFailProjectUpdate");
  }

  async estimatesActiveSystemValues(): Promise<string> {
    return todo("estimatesActiveSystemValues");
  }

  async estimatesArchivedSectionVisible(): Promise<boolean> {
    return todo("estimatesArchivedSectionVisible");
  }

  async estimatesArchivedNames(): Promise<string[]> {
    return todo("estimatesArchivedNames");
  }

  async estimatesArchivedExplainerText(): Promise<string> {
    return todo("estimatesArchivedExplainerText");
  }

  async estimatesArchivedLinkHref(): Promise<string> {
    return todo("estimatesArchivedLinkHref");
  }

  async estimatesArchivedLinkTarget(): Promise<string> {
    return todo("estimatesArchivedLinkTarget");
  }

  async estimatesArchivedOpenLinkNewPage(): Promise<string> {
    return todo("estimatesArchivedOpenLinkNewPage");
  }

  async estimatesArchivedRowButtonCount(_name: string): Promise<number> {
    return todo("estimatesArchivedRowButtonCount");
  }

  // --- Modules list states, layouts, rows and cards (NEWFRONT-307,
  // --- MOD-008..012, MOD-020). Throwing stubs until the area lands.

  async modulesListOpenList(_workspaceSlug: string, _projectId: string): Promise<void> {
    return todo("modulesListOpenList");
  }

  async modulesListVisibleNames(): Promise<string[]> {
    return todo("modulesListVisibleNames");
  }

  async modulesListBoardCardNames(): Promise<string[]> {
    return todo("modulesListBoardCardNames");
  }

  async modulesListTimelineNames(): Promise<string[]> {
    return todo("modulesListTimelineNames");
  }

  async modulesListEmptyKind(): Promise<ModulesListEmptyKind> {
    return todo("modulesListEmptyKind");
  }

  async modulesListNoMatchHeading(): Promise<string | null> {
    return todo("modulesListNoMatchHeading");
  }

  async modulesListNoMatchHint(): Promise<string | null> {
    return todo("modulesListNoMatchHint");
  }

  async modulesListSkeletonVisible(): Promise<boolean> {
    return todo("modulesListSkeletonVisible");
  }

  async modulesListSkeletonShownOnSlowFetch(
    _workspaceSlug: string,
    _projectId: string,
    _layout: ModulesListLayout
  ): Promise<ModulesListSkeletonResult> {
    return todo("modulesListSkeletonShownOnSlowFetch");
  }

  async modulesListZeroStateVisible(): Promise<boolean> {
    return todo("modulesListZeroStateVisible");
  }

  async modulesListZeroStateArtVisible(): Promise<boolean> {
    return todo("modulesListZeroStateArtVisible");
  }

  async modulesListZeroStateCopy(): Promise<ModulesListZeroStateCopy> {
    return todo("modulesListZeroStateCopy");
  }

  async modulesListZeroStateCreateDisabled(): Promise<boolean> {
    return todo("modulesListZeroStateCreateDisabled");
  }

  async modulesListZeroStateCreateOpen(): Promise<void> {
    return todo("modulesListZeroStateCreateOpen");
  }

  async modulesListCreateDialogVisible(): Promise<boolean> {
    return todo("modulesListCreateDialogVisible");
  }

  async modulesListNoMatchShowViaSearch(_text: string): Promise<void> {
    return todo("modulesListNoMatchShowViaSearch");
  }

  async modulesListNoMatchClearSearch(): Promise<void> {
    return todo("modulesListNoMatchClearSearch");
  }

  async modulesListLayoutOptionCount(): Promise<number> {
    return todo("modulesListLayoutOptionCount");
  }

  async modulesListActiveLayout(): Promise<ModulesListLayout> {
    return todo("modulesListActiveLayout");
  }

  async modulesListSwitcherVisible(): Promise<boolean> {
    return todo("modulesListSwitcherVisible");
  }

  async modulesListSwitchLayout(_layout: ModulesListLayout): Promise<void> {
    return todo("modulesListSwitchLayout");
  }

  async modulesListReload(): Promise<void> {
    return todo("modulesListReload");
  }

  async modulesListCompactMenuVisible(): Promise<boolean> {
    return todo("modulesListCompactMenuVisible");
  }

  async modulesListCompactMenuOptions(): Promise<string[]> {
    return todo("modulesListCompactMenuOptions");
  }

  async modulesListCompactSwitchLayout(_layout: "list" | "board"): Promise<void> {
    return todo("modulesListCompactSwitchLayout");
  }

  async modulesListRowProgress(_name: string): Promise<ModulesListRowProgress> {
    return todo("modulesListRowProgress");
  }

  async modulesListRowDetailHref(_name: string): Promise<string> {
    return todo("modulesListRowDetailHref");
  }

  async modulesListRowOpenPeek(_name: string): Promise<void> {
    return todo("modulesListRowOpenPeek");
  }

  async modulesListPeekName(): Promise<string | null> {
    return todo("modulesListPeekName");
  }

  async modulesListPeekClose(): Promise<void> {
    return todo("modulesListPeekClose");
  }

  async modulesListRowMenuVisible(_name: string): Promise<boolean> {
    return todo("modulesListRowMenuVisible");
  }

  async modulesListArchivedRowClickOpensPeek(_name: string): Promise<void> {
    return todo("modulesListArchivedRowClickOpensPeek");
  }

  async modulesListCardNameTruncated(_name: string): Promise<boolean> {
    return todo("modulesListCardNameTruncated");
  }

  async modulesListCardNameTooltip(_name: string): Promise<string | null> {
    return todo("modulesListCardNameTooltip");
  }

  async modulesListCardStatusText(_name: string): Promise<string> {
    return todo("modulesListCardStatusText");
  }

  async modulesListCardOpenPeek(_name: string): Promise<void> {
    return todo("modulesListCardOpenPeek");
  }

  async modulesListCardIssueCount(_name: string): Promise<string> {
    return todo("modulesListCardIssueCount");
  }

  async modulesListCardLeadKind(_name: string): Promise<ModulesListCardLead> {
    return todo("modulesListCardLeadKind");
  }

  async modulesListCardProgressSegments(_name: string): Promise<ModulesListCardSegments> {
    return todo("modulesListCardProgressSegments");
  }

  async modulesListCardDatesVisible(_name: string): Promise<boolean> {
    return todo("modulesListCardDatesVisible");
  }

  async modulesListCardFavoriteVisible(_name: string): Promise<boolean> {
    return todo("modulesListCardFavoriteVisible");
  }

  async modulesListCardQuickMenuVisible(_name: string): Promise<boolean> {
    return todo("modulesListCardQuickMenuVisible");
  }

  async modulesListCardClickBody(_name: string): Promise<void> {
    return todo("modulesListCardClickBody");
  }

  async modulesListCardClickControl(_name: string, _control: ModulesListCardControl): Promise<void> {
    return todo("modulesListCardClickControl");
  }

  // --- Estimates creation dialog (NEWFRONT-323, EST-008..013). Appended;
  // --- sibling stubs above are untouched per the shared driver contract.
  // --- The shell child's navigation (NEWFRONT-321, merged) and the dialog
  // --- verbs shared with the permissions child (NEWFRONT-326, merged) are
  // --- reused as-is; only the dialog-specific stubs below are new.

  async estimatesDialogStep(): Promise<"1" | "2" | null> {
    return todo("estimatesDialogStep");
  }
  async estimatesDialogBack(): Promise<void> {
    return todo("estimatesDialogBack");
  }
  async estimatesBackVisible(): Promise<boolean> {
    return todo("estimatesBackVisible");
  }
  async estimatesDialogCancel(): Promise<void> {
    return todo("estimatesDialogCancel");
  }
  async estimatesStarterCards(): Promise<EstimatesStarterCard[]> {
    return todo("estimatesStarterCards");
  }
  async estimatesStepTwoValues(): Promise<string[]> {
    return todo("estimatesStepTwoValues");
  }
  async estimatesStepTwoFamily(): Promise<string> {
    return todo("estimatesStepTwoFamily");
  }
  async estimatesDragValueBefore(_sourceIndex: number, _targetIndex: number): Promise<void> {
    return todo("estimatesDragValueBefore");
  }
  async estimatesEditValueStart(_index: number, _via: "double-click" | "control"): Promise<void> {
    return todo("estimatesEditValueStart");
  }
  async estimatesOpenFormValue(): Promise<string | null> {
    return todo("estimatesOpenFormValue");
  }
  async estimatesOpenFormSave(_value: string): Promise<void> {
    return todo("estimatesOpenFormSave");
  }
  async estimatesOpenFormFill(_value: string): Promise<void> {
    return todo("estimatesOpenFormFill");
  }
  async estimatesOpenFormSubmit(): Promise<void> {
    return todo("estimatesOpenFormSubmit");
  }
  async estimatesOpenFormCancel(): Promise<void> {
    return todo("estimatesOpenFormCancel");
  }
  async estimatesOpenFormCount(): Promise<number> {
    return todo("estimatesOpenFormCount");
  }
  async estimatesRemoveValue(_index: number): Promise<void> {
    return todo("estimatesRemoveValue");
  }
  async estimatesRemoveVisible(_index: number): Promise<boolean> {
    return todo("estimatesRemoveVisible");
  }
  async estimatesAddOpen(): Promise<void> {
    return todo("estimatesAddOpen");
  }
  async estimatesAddVisible(): Promise<boolean> {
    return todo("estimatesAddVisible");
  }
  async estimatesAddType(_value: string): Promise<void> {
    return todo("estimatesAddType");
  }
  async estimatesValueErrors(): Promise<string[]> {
    return todo("estimatesValueErrors");
  }
  async estimatesValueErrorCount(): Promise<number> {
    return todo("estimatesValueErrorCount");
  }
  async estimatesActiveRow(): Promise<EstimatesActiveRow | null> {
    return todo("estimatesActiveRow");
  }

  async estimatesEditDeleteFailEstimateDelete(): Promise<void> {
    return todo("estimatesEditDeleteFailEstimateDelete");
  }

  // --- Module detail/analytics/links (NEWFRONT-310, MOD-029-035). Appended stubs.

  async modulesDetailSidebarName(): Promise<string | null> {
    return todo("modulesDetailSidebarName");
  }

  async modulesDetailBadgeText(): Promise<string | null> {
    return todo("modulesDetailBadgeText");
  }

  async modulesDetailBadgeTooltip(): Promise<string | null> {
    return todo("modulesDetailBadgeTooltip");
  }

  async modulesDetailPageTitle(): Promise<string> {
    return todo("modulesDetailPageTitle");
  }

  async modulesDetailCrumbs(): Promise<string[]> {
    return todo("modulesDetailCrumbs");
  }

  async modulesDetailClickModulesCrumb(): Promise<void> {
    return todo("modulesDetailClickModulesCrumb");
  }

  async modulesDetailClickProjectCrumb(_projectName: string): Promise<void> {
    return todo("modulesDetailClickProjectCrumb");
  }

  async modulesDetailSwitcherOpen(): Promise<void> {
    return todo("modulesDetailSwitcherOpen");
  }

  async modulesDetailSwitcherOptions(): Promise<string[]> {
    return todo("modulesDetailSwitcherOptions");
  }

  async modulesDetailSwitcherSearch(_text: string): Promise<void> {
    return todo("modulesDetailSwitcherSearch");
  }

  async modulesDetailSwitcherPick(_name: string): Promise<void> {
    return todo("modulesDetailSwitcherPick");
  }

  async modulesDetailUnknownTitle(): Promise<string | null> {
    return todo("modulesDetailUnknownTitle");
  }

  async modulesDetailUnknownBack(): Promise<void> {
    return todo("modulesDetailUnknownBack");
  }

  async modulesDetailSwitchLayout(_layout: LayoutsLayoutKey): Promise<void> {
    return todo("modulesDetailSwitchLayout");
  }

  async modulesDetailFilterToggleVisible(): Promise<boolean> {
    return todo("modulesDetailFilterToggleVisible");
  }

  async modulesDetailDisplayGroupedOptions(): Promise<ModulesDetailGroupedOption[]> {
    return todo("modulesDetailDisplayGroupedOptions");
  }

  async modulesDetailMobileOfferedLayouts(): Promise<LayoutsLayoutKey[]> {
    return todo("modulesDetailMobileOfferedLayouts");
  }

  async modulesDetailMobileSwitchTo(_layout: LayoutsLayoutKey): Promise<void> {
    return todo("modulesDetailMobileSwitchTo");
  }

  async modulesDetailMobileDisplayVisible(): Promise<boolean> {
    return todo("modulesDetailMobileDisplayVisible");
  }

  async modulesDetailMobileFilterToggleVisible(): Promise<boolean> {
    return todo("modulesDetailMobileFilterToggleVisible");
  }

  async modulesDetailHeaderAddVisible(): Promise<boolean> {
    return todo("modulesDetailHeaderAddVisible");
  }

  async modulesDetailHeaderAddClick(): Promise<void> {
    return todo("modulesDetailHeaderAddClick");
  }

  async modulesDetailFailNextIssuesAdd(): Promise<void> {
    return todo("modulesDetailFailNextIssuesAdd");
  }

  async modulesDetailPickerSearch(_text: string): Promise<void> {
    return todo("modulesDetailPickerSearch");
  }

  async modulesDetailAnalyticsVisible(): Promise<boolean> {
    return todo("modulesDetailAnalyticsVisible");
  }

  async modulesDetailOpenAnalytics(): Promise<void> {
    return todo("modulesDetailOpenAnalytics");
  }

  async modulesDetailAnalyticsDialogVisible(): Promise<boolean> {
    return todo("modulesDetailAnalyticsDialogVisible");
  }

  async modulesDetailDismissAnalytics(): Promise<void> {
    return todo("modulesDetailDismissAnalytics");
  }

  async modulesDetailToggleSidebar(): Promise<void> {
    return todo("modulesDetailToggleSidebar");
  }

  async modulesDetailSidebarVisible(): Promise<boolean> {
    return todo("modulesDetailSidebarVisible");
  }

  async modulesDetailSidebarCollapsedStored(): Promise<boolean> {
    return todo("modulesDetailSidebarCollapsedStored");
  }

  async modulesDetailCloseSidebarViaPanel(): Promise<void> {
    return todo("modulesDetailCloseSidebarViaPanel");
  }

  async modulesDetailMobileOpenAnalytics(): Promise<void> {
    return todo("modulesDetailMobileOpenAnalytics");
  }

  async modulesDetailSidebarStatus(): Promise<string | null> {
    return todo("modulesDetailSidebarStatus");
  }

  async modulesDetailSidebarSetStatus(_status: string): Promise<void> {
    return todo("modulesDetailSidebarSetStatus");
  }

  async modulesDetailSidebarDescription(): Promise<string | null> {
    return todo("modulesDetailSidebarDescription");
  }

  async modulesDetailSidebarDateText(): Promise<string> {
    return todo("modulesDetailSidebarDateText");
  }

  async modulesDetailSidebarDateDisabled(): Promise<boolean> {
    return todo("modulesDetailSidebarDateDisabled");
  }

  async modulesDetailSidebarOpenDatePicker(): Promise<void> {
    return todo("modulesDetailSidebarOpenDatePicker");
  }

  async modulesDetailPickDateDay(_label: string): Promise<void> {
    return todo("modulesDetailPickDateDay");
  }

  async modulesDetailDismissDatePicker(): Promise<void> {
    return todo("modulesDetailDismissDatePicker");
  }

  async modulesDetailSidebarLead(): Promise<string | null> {
    return todo("modulesDetailSidebarLead");
  }

  async modulesDetailSidebarSetLead(_name: string): Promise<void> {
    return todo("modulesDetailSidebarSetLead");
  }

  async modulesDetailSidebarMembers(): Promise<string[]> {
    return todo("modulesDetailSidebarMembers");
  }

  async modulesDetailSidebarAddMember(_name: string): Promise<void> {
    return todo("modulesDetailSidebarAddMember");
  }

  async modulesDetailSidebarStatusDisabled(): Promise<boolean> {
    return todo("modulesDetailSidebarStatusDisabled");
  }

  async modulesDetailSidebarLeadDisabled(): Promise<boolean> {
    return todo("modulesDetailSidebarLeadDisabled");
  }

  async modulesDetailSidebarMembersDisabled(): Promise<boolean> {
    return todo("modulesDetailSidebarMembersDisabled");
  }

  async modulesDetailSidebarWorkItemsText(): Promise<string> {
    return todo("modulesDetailSidebarWorkItemsText");
  }

  async modulesDetailSidebarPointsText(): Promise<string | null> {
    return todo("modulesDetailSidebarPointsText");
  }

  async modulesDetailSlowFetchShowsLoader(): Promise<boolean> {
    return todo("modulesDetailSlowFetchShowsLoader");
  }

  async modulesDetailProgressOpen(): Promise<boolean> {
    return todo("modulesDetailProgressOpen");
  }

  async modulesDetailProgressHeaderPercent(): Promise<number | null> {
    return todo("modulesDetailProgressHeaderPercent");
  }

  async modulesDetailProgressNotice(): Promise<string | null> {
    return todo("modulesDetailProgressNotice");
  }

  async modulesDetailProgressChartVisible(): Promise<boolean> {
    return todo("modulesDetailProgressChartVisible");
  }

  async modulesDetailProgressMeasureValue(): Promise<string | null> {
    return todo("modulesDetailProgressMeasureValue");
  }

  async modulesDetailProgressMeasureOptions(): Promise<string[]> {
    return todo("modulesDetailProgressMeasureOptions");
  }

  async modulesDetailMeasureSwitchGetCount(_label: string): Promise<number> {
    return todo("modulesDetailMeasureSwitchGetCount");
  }

  async modulesDetailProgressStatsTab(_tab: "States" | "Assignees" | "Labels"): Promise<void> {
    return todo("modulesDetailProgressStatsTab");
  }

  async modulesDetailActiveStatsTab(): Promise<string> {
    return todo("modulesDetailActiveStatsTab");
  }

  async modulesDetailProgressStatsRows(): Promise<ModulesDetailStatRow[]> {
    return todo("modulesDetailProgressStatsRows");
  }

  async modulesDetailProgressEntryClick(_title: string): Promise<void> {
    return todo("modulesDetailProgressEntryClick");
  }

  async modulesDetailProgressEntryClickable(_title: string): Promise<boolean> {
    return todo("modulesDetailProgressEntryClickable");
  }

  async modulesDetailProgressEntrySelected(_title: string): Promise<boolean> {
    return todo("modulesDetailProgressEntrySelected");
  }

  async modulesDetailLinksOpen(): Promise<boolean> {
    return todo("modulesDetailLinksOpen");
  }

  async modulesDetailLinksToggle(): Promise<void> {
    return todo("modulesDetailLinksToggle");
  }

  async modulesDetailLinksEmptyNotice(): Promise<string | null> {
    return todo("modulesDetailLinksEmptyNotice");
  }

  async modulesDetailLinksAddVisible(): Promise<boolean> {
    return todo("modulesDetailLinksAddVisible");
  }

  async modulesDetailLinksAddClick(): Promise<void> {
    return todo("modulesDetailLinksAddClick");
  }

  async modulesDetailLinksRows(): Promise<ModulesDetailLinkRow[]> {
    return todo("modulesDetailLinksRows");
  }

  async modulesDetailLinkDialogOpen(): Promise<boolean> {
    return todo("modulesDetailLinkDialogOpen");
  }

  async modulesDetailLinkDialogTitle(): Promise<string | null> {
    return todo("modulesDetailLinkDialogTitle");
  }

  async modulesDetailLinkFillUrl(_url: string): Promise<void> {
    return todo("modulesDetailLinkFillUrl");
  }

  async modulesDetailLinkFillTitle(_title: string): Promise<void> {
    return todo("modulesDetailLinkFillTitle");
  }

  async modulesDetailLinkSubmit(): Promise<void> {
    return todo("modulesDetailLinkSubmit");
  }

  async modulesDetailLinkUrlInvalid(): Promise<boolean> {
    return todo("modulesDetailLinkUrlInvalid");
  }

  async modulesDetailLinkCancel(): Promise<void> {
    return todo("modulesDetailLinkCancel");
  }

  async modulesDetailLinkCopy(_title: string): Promise<void> {
    return todo("modulesDetailLinkCopy");
  }

  async modulesDetailLinkEdit(_title: string): Promise<void> {
    return todo("modulesDetailLinkEdit");
  }

  async modulesDetailLinkDelete(_title: string): Promise<void> {
    return todo("modulesDetailLinkDelete");
  }

  async modulesDetailLinkMeta(_title: string): Promise<string | null> {
    return todo("modulesDetailLinkMeta");
  }

  async modulesDetailFailNextLinkWrite(): Promise<void> {
    return todo("modulesDetailFailNextLinkWrite");
  }

  async pagesHeaderCopyLinkLabel(): Promise<string | null> {
    return todo("pagesHeaderCopyLinkLabel");
  }
  async pagesHeaderClickCopyLink(): Promise<void> {
    return todo("pagesHeaderClickCopyLink");
  }
  async pagesHeaderReadClipboard(): Promise<string> {
    return todo("pagesHeaderReadClipboard");
  }
  async pagesHeaderFavoriteLabel(): Promise<string | null> {
    return todo("pagesHeaderFavoriteLabel");
  }
  async pagesHeaderFavoriteFilled(): Promise<boolean> {
    return todo("pagesHeaderFavoriteFilled");
  }
  async pagesHeaderToggleFavorite(): Promise<void> {
    return todo("pagesHeaderToggleFavorite");
  }
  async pagesHeaderFavoritesExpanded(): Promise<boolean> {
    return todo("pagesHeaderFavoritesExpanded");
  }
  async pagesHeaderFavoritesNames(): Promise<string[]> {
    return todo("pagesHeaderFavoritesNames");
  }
  async pagesHeaderFailFavoriteWrites(): Promise<void> {
    return todo("pagesHeaderFailFavoriteWrites");
  }
  async pagesHeaderUnfailFavoriteWrites(): Promise<void> {
    return todo("pagesHeaderUnfailFavoriteWrites");
  }
  async pagesHeaderOpenOptions(): Promise<void> {
    return todo("pagesHeaderOpenOptions");
  }
  async pagesHeaderMenuItems(): Promise<string[]> {
    return todo("pagesHeaderMenuItems");
  }
  async pagesHeaderChooseMenuItem(_label: string): Promise<void> {
    return todo("pagesHeaderChooseMenuItem");
  }
  async pagesHeaderLockLabel(): Promise<string | null> {
    return todo("pagesHeaderLockLabel");
  }
  async pagesHeaderToggleLock(): Promise<void> {
    return todo("pagesHeaderToggleLock");
  }
  async pagesHeaderEditorEditable(): Promise<boolean> {
    return todo("pagesHeaderEditorEditable");
  }
  async pagesHeaderFailLockWrites(): Promise<void> {
    return todo("pagesHeaderFailLockWrites");
  }
  async pagesHeaderUnfailLockWrites(): Promise<void> {
    return todo("pagesHeaderUnfailLockWrites");
  }
  async pagesHeaderArchivedMarker(): Promise<string | null> {
    return todo("pagesHeaderArchivedMarker");
  }
  async pagesHeaderDeleteDialogVisible(): Promise<boolean> {
    return todo("pagesHeaderDeleteDialogVisible");
  }
  async pagesHeaderDeleteDialogText(): Promise<string> {
    return todo("pagesHeaderDeleteDialogText");
  }
  async pagesHeaderConfirmDelete(): Promise<void> {
    return todo("pagesHeaderConfirmDelete");
  }
  async pagesHeaderCancelDelete(): Promise<void> {
    return todo("pagesHeaderCancelDelete");
  }
  async pagesHeaderCanvasMaxWidth(): Promise<string> {
    return todo("pagesHeaderCanvasMaxWidth");
  }
  async pagesHeaderToolbarVisible(): Promise<boolean> {
    return todo("pagesHeaderToolbarVisible");
  }
  async pagesHeaderPaneTabParam(): Promise<string | null> {
    return todo("pagesHeaderPaneTabParam");
  }
  async pagesHeaderPaneOpen(): Promise<boolean> {
    return todo("pagesHeaderPaneOpen");
  }
  async pagesHeaderSyncBadgeTooltip(): Promise<string | null> {
    return todo("pagesHeaderSyncBadgeTooltip");
  }
  async pagesHeaderOfflinePill(): Promise<string | null> {
    return todo("pagesHeaderOfflinePill");
  }
  async pagesHeaderOversizedStrip(): Promise<string | null> {
    return todo("pagesHeaderOversizedStrip");
  }
  async pagesHeaderSyncBadgeText(): Promise<string | null> {
    return todo("pagesHeaderSyncBadgeText");
  }
  async pagesHeaderToastText(): Promise<string | null> {
    return todo("pagesHeaderToastText");
  }
  async pagesHeaderSpyDescriptionWrites(): Promise<void> {
    return todo("pagesHeaderSpyDescriptionWrites");
  }
  async pagesHeaderDescriptionWriteCount(): Promise<number> {
    return todo("pagesHeaderDescriptionWriteCount");
  }
  async pagesHeaderDescriptionWriteTimes(): Promise<number[]> {
    return todo("pagesHeaderDescriptionWriteTimes");
  }
  async pagesHeaderDescriptionWriteStatuses(): Promise<number[]> {
    return todo("pagesHeaderDescriptionWriteStatuses");
  }
  async pagesHeaderDescriptionWriteKeys(): Promise<string[]> {
    return todo("pagesHeaderDescriptionWriteKeys");
  }
  async pagesHeaderSpyPageWrites(): Promise<void> {
    return todo("pagesHeaderSpyPageWrites");
  }
  async pagesHeaderPageWriteCount(): Promise<number> {
    return todo("pagesHeaderPageWriteCount");
  }
  async pagesHeaderPressSave(): Promise<void> {
    return todo("pagesHeaderPressSave");
  }
  async pagesHeaderExportDialogVisible(): Promise<boolean> {
    return todo("pagesHeaderExportDialogVisible");
  }
  async pagesHeaderExportForm(): Promise<PagesHeaderExportForm> {
    return todo("pagesHeaderExportForm");
  }
  async pagesHeaderExportSetFormat(_label: string): Promise<void> {
    return todo("pagesHeaderExportSetFormat");
  }
  async pagesHeaderExportSetScope(_label: string): Promise<void> {
    return todo("pagesHeaderExportSetScope");
  }
  async pagesHeaderExportSetPageFormat(_label: string): Promise<void> {
    return todo("pagesHeaderExportSetPageFormat");
  }
  async pagesHeaderExportConfirm(): Promise<PagesHeaderDownload> {
    return todo("pagesHeaderExportConfirm");
  }
  async pagesHeaderExportCancel(): Promise<void> {
    return todo("pagesHeaderExportCancel");
  }

  // --- Pages title, mark + formatting toolbar (NEWFRONT-315, PAGE-026-036).
  // --- Stage 0 oracle stubs; the pages build slice lands them.

  async pagesTitlePressEnter(): Promise<void> {
    return todo("pagesTitlePressEnter");
  }
  async pagesTitleFocusInBody(): Promise<boolean> {
    return todo("pagesTitleFocusInBody");
  }
  async pagesTitleBodyCaretAtTop(): Promise<boolean> {
    return todo("pagesTitleBodyCaretAtTop");
  }
  async pagesTitleEditable(): Promise<boolean> {
    return todo("pagesTitleEditable");
  }
  async pagesTitleEmptyFallback(): Promise<string | null> {
    return todo("pagesTitleEmptyFallback");
  }
  async pagesTitleCountBadge(): Promise<string | null> {
    return todo("pagesTitleCountBadge");
  }
  async pagesTitleFocus(): Promise<void> {
    return todo("pagesTitleFocus");
  }
  async pagesTitleSetTitle(_text: string, _delayMs: number): Promise<void> {
    return todo("pagesTitleSetTitle");
  }
  async pagesTitleIsFocused(): Promise<boolean> {
    return todo("pagesTitleIsFocused");
  }
  async pagesMarkOpenPicker(): Promise<void> {
    return todo("pagesMarkOpenPicker");
  }
  async pagesMarkTabs(): Promise<string[]> {
    return todo("pagesMarkTabs");
  }
  async pagesMarkSelectTab(_name: string): Promise<void> {
    return todo("pagesMarkSelectTab");
  }
  async pagesMarkSearchPick(_emoji: string, _query: string): Promise<void> {
    return todo("pagesMarkSearchPick");
  }
  async pagesMarkPickIcon(_index: number): Promise<void> {
    return todo("pagesMarkPickIcon");
  }
  async pagesMarkCurrent(): Promise<PagesMarkCurrent | null> {
    return todo("pagesMarkCurrent");
  }
  async pagesMarkPickerDisabled(): Promise<boolean> {
    return todo("pagesMarkPickerDisabled");
  }
  async pagesMarkAffordanceOpacity(): Promise<string> {
    return todo("pagesMarkAffordanceOpacity");
  }
  async pagesMarkClearControl(): Promise<boolean> {
    return todo("pagesMarkClearControl");
  }
  async pagesMarkHoverHeader(): Promise<void> {
    return todo("pagesMarkHoverHeader");
  }
  async pagesMarkListGlyph(_workspaceSlug: string, _projectId: string, _pageName: string): Promise<string | null> {
    return todo("pagesMarkListGlyph");
  }
  async pagesMarkBreadcrumbGlyph(_pageName: string): Promise<string | null> {
    return todo("pagesMarkBreadcrumbGlyph");
  }
  async pagesMarkSwitcherGlyph(_pageName: string): Promise<string | null> {
    return todo("pagesMarkSwitcherGlyph");
  }
  async pagesToolbarSettle(_workspaceSlug: string, _projectId: string, _pageId: string): Promise<void> {
    return todo("pagesToolbarSettle");
  }
  async pagesToolbarIconCount(): Promise<number> {
    return todo("pagesToolbarIconCount");
  }
  async pagesToolbarClick(_index: number): Promise<void> {
    return todo("pagesToolbarClick");
  }
  async pagesToolbarActive(_index: number): Promise<boolean> {
    return todo("pagesToolbarActive");
  }
  async pagesToolbarTooltip(_index: number): Promise<PagesToolbarTooltip> {
    return todo("pagesToolbarTooltip");
  }
  async pagesToolbarTypography(): Promise<string> {
    return todo("pagesToolbarTypography");
  }
  async pagesToolbarTypographyItems(): Promise<string[]> {
    return todo("pagesToolbarTypographyItems");
  }
  async pagesToolbarTypographyPick(_name: string): Promise<void> {
    return todo("pagesToolbarTypographyPick");
  }
  async pagesToolbarColorOpen(): Promise<void> {
    return todo("pagesToolbarColorOpen");
  }
  async pagesToolbarColorCounts(): Promise<PagesToolbarColorCounts> {
    return todo("pagesToolbarColorCounts");
  }
  async pagesToolbarColorPick(_kind: "text" | "background", _index: number): Promise<void> {
    return todo("pagesToolbarColorPick");
  }
  async pagesToolbarColorClear(_kind: "text" | "background"): Promise<void> {
    return todo("pagesToolbarColorClear");
  }
  async pagesToolbarColorPreview(): Promise<PagesToolbarColorPreview> {
    return todo("pagesToolbarColorPreview");
  }
  async pagesToolbarBodyHtml(): Promise<string> {
    return todo("pagesToolbarBodyHtml");
  }
  async pagesToolbarSelectAll(): Promise<void> {
    return todo("pagesToolbarSelectAll");
  }
  async pagesToolbarTypeText(_text: string): Promise<void> {
    return todo("pagesToolbarTypeText");
  }
  async pagesToolbarSave(): Promise<void> {
    return todo("pagesToolbarSave");
  }
  async pagesToolbarPressKey(_key: string): Promise<void> {
    return todo("pagesToolbarPressKey");
  }
  async pagesToolbarTableDims(): Promise<string | null> {
    return todo("pagesToolbarTableDims");
  }
  async pagesToolbarCaretCell(): Promise<string> {
    return todo("pagesToolbarCaretCell");
  }
  async pagesToolbarImageUpload(_filePath: string): Promise<void> {
    return todo("pagesToolbarImageUpload");
  }
  async pagesToolbarEditorHasImage(): Promise<boolean> {
    return todo("pagesToolbarEditorHasImage");
  }
  async pagesToolbarImageSrc(): Promise<string | null> {
    return todo("pagesToolbarImageSrc");
  }
  async pagesToolbarVisible(): Promise<boolean> {
    return todo("pagesToolbarVisible");
  }
  async pagesToolbarFloatingToggle(): Promise<boolean> {
    return todo("pagesToolbarFloatingToggle");
  }
  async pagesToolbarSeedStickyPref(_value: boolean): Promise<void> {
    return todo("pagesToolbarSeedStickyPref");
  }
  // --- Runs, approvals, run dispatch, re-tick (NEWFRONT-180). Stubs until
  // --- the area lands in apps/web_new; the oracle driver stays untouched.

  async openRunsList(_workspaceSlug: string, _projectId?: string): Promise<void> {
    return todo("openRunsList");
  }

  async openRunsListAtPage(_workspaceSlug: string, _page: number, _projectId?: string): Promise<void> {
    return todo("openRunsListAtPage");
  }

  async runsHistoryLength(): Promise<number> {
    return todo("runsHistoryLength");
  }

  async openRunDetail(_workspaceSlug: string, _runId: string, _projectId?: string): Promise<void> {
    return todo("openRunDetail");
  }

  async openApprovals(_workspaceSlug: string, _projectId?: string): Promise<void> {
    return todo("openApprovals");
  }

  async runsRows(): Promise<RunsListRow[]> {
    return todo("runsRows");
  }

  async runsPager(): Promise<{ label: string; prevDisabled: boolean; nextDisabled: boolean } | null> {
    return todo("runsPager");
  }

  async runsEmptyVisible(): Promise<boolean> {
    return todo("runsEmptyVisible");
  }

  async runsNextPage(): Promise<void> {
    return todo("runsNextPage");
  }

  async runsPrevPage(): Promise<void> {
    return todo("runsPrevPage");
  }

  async runsSelectRun(_promptMarker: string): Promise<void> {
    return todo("runsSelectRun");
  }

  async runDetailState(): Promise<"none" | "loading" | "loaded" | "unavailable"> {
    return todo("runDetailState");
  }

  async runDetailHeader(): Promise<{ id: string; statusLabel: string; executor: string } | null> {
    return todo("runDetailHeader");
  }

  async runDetailPrompt(): Promise<string | null> {
    return todo("runDetailPrompt");
  }

  async runDetailScheduler(): Promise<{ name: string; href: string } | null> {
    return todo("runDetailScheduler");
  }

  async runDetailError(): Promise<{
    raw: string;
    source: string | null;
    kind: string | null;
    summary: string | null;
    action: string | null;
  } | null> {
    return todo("runDetailError");
  }

  async runDetailResult(): Promise<{ summary: string | null; raw: string } | null> {
    return todo("runDetailResult");
  }

  async runCloudPanel(): Promise<{
    tools: string[];
    calls: Array<{ tool: string; risk: string; status: string }>;
  } | null> {
    return todo("runCloudPanel");
  }

  async runEvents(): Promise<Array<{ seq: string; kind: string; narrative: string | null }>> {
    return todo("runEvents");
  }

  async runCancelAvailable(): Promise<boolean> {
    return todo("runCancelAvailable");
  }

  async runCancelOpen(): Promise<void> {
    return todo("runCancelOpen");
  }

  async runCancelDialog(): Promise<{ title: string; body: string } | null> {
    return todo("runCancelDialog");
  }

  async runCancelConfirm(): Promise<void> {
    return todo("runCancelConfirm");
  }

  async runsListPollCount(_windowMs: number): Promise<number> {
    return todo("runsListPollCount");
  }

  async runDetailPollCount(_runId: string, _windowMs: number): Promise<number> {
    return todo("runDetailPollCount");
  }

  async approvalsCards(): Promise<ApprovalCard[]> {
    return todo("approvalsCards");
  }

  async approvalsEmptyVisible(): Promise<boolean> {
    return todo("approvalsEmptyVisible");
  }

  async approvalsDecide(_payloadMarker: string, _decision: "accept" | "accept_for_session" | "decline"): Promise<void> {
    return todo("approvalsDecide");
  }

  async issueRunAiDispatch(): Promise<{ toast: string | null; requestUrls: string[] }> {
    return todo("issueRunAiDispatch");
  }

  async issueReTickVisible(): Promise<boolean> {
    return todo("issueReTickVisible");
  }

  async issueReTickClick(): Promise<string | null> {
    return todo("issueReTickClick");
  }

  async runDetailLoadingObserved(_workspaceSlug: string, _runId: string): Promise<boolean> {
    return todo("runDetailLoadingObserved");
  }

  // --- Analytics shell, scoping, permissions (NEWFRONT-375, ANL-001..004 + ANL-018). ---

  async analyticsOpenTab(_workspaceSlug: string, _tabId: string, _cookies: ParityBrowserCookie[]): Promise<void> {
    return todo("analyticsOpenTab");
  }

  async analyticsOpenRoot(_workspaceSlug: string, _cookies: ParityBrowserCookie[]): Promise<void> {
    return todo("analyticsOpenRoot");
  }

  async analyticsCurrentPath(): Promise<string> {
    return todo("analyticsCurrentPath");
  }

  async analyticsTabIds(): Promise<string[]> {
    return todo("analyticsTabIds");
  }

  async analyticsSelectedTab(): Promise<string | null> {
    return todo("analyticsSelectedTab");
  }

  async analyticsSelectTab(_tabId: string): Promise<void> {
    return todo("analyticsSelectTab");
  }

  async analyticsPanelVisible(_tabId: string): Promise<boolean> {
    return todo("analyticsPanelVisible");
  }

  async analyticsBreadcrumb(): Promise<string[]> {
    return todo("analyticsBreadcrumb");
  }

  async analyticsPageTitle(): Promise<string> {
    return todo("analyticsPageTitle");
  }

  async analyticsEmptyFacts(): Promise<AnalyticsEmptyFacts | null> {
    return todo("analyticsEmptyFacts");
  }

  async analyticsEmptyCreateProject(): Promise<void> {
    return todo("analyticsEmptyCreateProject");
  }

  async analyticsCreateAffordances(): Promise<string[]> {
    return todo("analyticsCreateAffordances");
  }

  async analyticsPickerLabel(): Promise<string> {
    return todo("analyticsPickerLabel");
  }

  async analyticsPickerOpen(): Promise<void> {
    return todo("analyticsPickerOpen");
  }

  async analyticsPickerClose(): Promise<void> {
    return todo("analyticsPickerClose");
  }

  async analyticsPickerOptions(): Promise<string[]> {
    return todo("analyticsPickerOptions");
  }

  async analyticsPickerSearch(_text: string): Promise<void> {
    return todo("analyticsPickerSearch");
  }

  async analyticsPickerToggle(_name: string): Promise<void> {
    return todo("analyticsPickerToggle");
  }

  async analyticsPickerChecked(): Promise<string[]> {
    return todo("analyticsPickerChecked");
  }

  async analyticsStartRequestSpy(): Promise<void> {
    return todo("analyticsStartRequestSpy");
  }

  async analyticsStopRequestSpy(): Promise<void> {
    return todo("analyticsStopRequestSpy");
  }

  async analyticsSpyRequests(): Promise<AnalyticsApiRequest[]> {
    return todo("analyticsSpyRequests");
  }

  // --- Analytics overview and work-items headline charts (NEWFRONT-376, ANL-005..009). Appended stubs.
  async analyticsOverviewCards(): Promise<AnalyticsHeadlineCard[]> {
    return todo("analyticsOverviewCards");
  }
  async analyticsWorkItemsCards(): Promise<AnalyticsHeadlineCard[]> {
    return todo("analyticsWorkItemsCards");
  }
  async analyticsHeadlinePlaceholders(): Promise<number> {
    return todo("analyticsHeadlinePlaceholders");
  }
  async analyticsTotalsStall(_tab: "overview" | "work-items"): Promise<void> {
    return todo("analyticsTotalsStall");
  }
  async analyticsTotalsRelease(): Promise<void> {
    return todo("analyticsTotalsRelease");
  }
  async analyticsChartsStall(_chart: "projects" | "work-items"): Promise<void> {
    return todo("analyticsChartsStall");
  }
  async analyticsChartsRelease(): Promise<void> {
    return todo("analyticsChartsRelease");
  }
  async analyticsProjectStatsStall(): Promise<void> {
    return todo("analyticsProjectStatsStall");
  }
  async analyticsProjectStatsRelease(): Promise<void> {
    return todo("analyticsProjectStatsRelease");
  }
  async analyticsRadarSummary(): Promise<AnalyticsRadarRow[]> {
    return todo("analyticsRadarSummary");
  }
  async analyticsRadarFacts(): Promise<AnalyticsRadarFacts> {
    return todo("analyticsRadarFacts");
  }
  async analyticsRadarLoading(): Promise<boolean> {
    return todo("analyticsRadarLoading");
  }
  async analyticsActiveProjects(): Promise<AnalyticsActiveProject[]> {
    return todo("analyticsActiveProjects");
  }
  async analyticsActiveProjectsLoading(): Promise<boolean> {
    return todo("analyticsActiveProjectsLoading");
  }
  async analyticsActiveProjectTooltip(_name: string): Promise<string | null> {
    return todo("analyticsActiveProjectTooltip");
  }
  async analyticsCreatedResolvedFacts(): Promise<AnalyticsCreatedResolvedFacts> {
    return todo("analyticsCreatedResolvedFacts");
  }
  async analyticsCreatedResolvedLoading(): Promise<boolean> {
    return todo("analyticsCreatedResolvedLoading");
  }
  async analyticsSectionGuidance(_title: string): Promise<string | null> {
    return todo("analyticsSectionGuidance");
  }
  async analyticsDialogHeadlineCards(): Promise<AnalyticsHeadlineCard[]> {
    return todo("analyticsDialogHeadlineCards");
  }

  // --- Analytics custom insights, tables, dialog (NEWFRONT-377, ANL-010..017).
  async analyticsCustomPickerLabels(): Promise<{ metric: string; xAxis: string; split: string }> {
    return todo("analyticsCustomPickerLabels");
  }
  async analyticsCustomPickerOptions(_index: 0 | 1 | 2): Promise<string[]> {
    return todo("analyticsCustomPickerOptions");
  }
  async analyticsCustomPickerSelect(_index: 0 | 1 | 2, _label: string): Promise<void> {
    return todo("analyticsCustomPickerSelect");
  }
  async analyticsCustomChartFacts(): Promise<AnalyticsCustomChartFacts> {
    return todo("analyticsCustomChartFacts");
  }
  async analyticsCustomTooltipSegments(_barIndex: number): Promise<AnalyticsCustomTooltipRow[]> {
    return todo("analyticsCustomTooltipSegments");
  }
  async analyticsExportRowCount(_kind: AnalyticsExportTable): Promise<number> {
    return todo("analyticsExportRowCount");
  }
  async analyticsExportHeaders(_kind: AnalyticsExportTable): Promise<string[]> {
    return todo("analyticsExportHeaders");
  }
  async analyticsExportCountLabel(_kind: AnalyticsExportTable): Promise<string> {
    return todo("analyticsExportCountLabel");
  }
  async analyticsExportSearchExpanded(_kind: AnalyticsExportTable): Promise<boolean> {
    return todo("analyticsExportSearchExpanded");
  }
  async analyticsExportSearchClose(_kind: AnalyticsExportTable): Promise<void> {
    return todo("analyticsExportSearchClose");
  }
  async analyticsExportEmptyText(_kind: AnalyticsExportTable): Promise<string | null> {
    return todo("analyticsExportEmptyText");
  }
  async analyticsInsightTableStall(): Promise<void> {
    return todo("analyticsInsightTableStall");
  }
  async analyticsInsightTableRelease(): Promise<void> {
    return todo("analyticsInsightTableRelease");
  }
  async analyticsInsightSkeletonCells(): Promise<number> {
    return todo("analyticsInsightSkeletonCells");
  }
  async analyticsDialogOpenFromIssues(
    _workspaceSlug: string,
    _projectId: string,
    _cookies: ParityBrowserCookie[]
  ): Promise<void> {
    return todo("analyticsDialogOpenFromIssues");
  }
  async analyticsDialogOpenFromCycle(
    _workspaceSlug: string,
    _projectId: string,
    _cycleId: string,
    _cookies: ParityBrowserCookie[]
  ): Promise<void> {
    return todo("analyticsDialogOpenFromCycle");
  }
  async analyticsDialogOpenFromModule(
    _workspaceSlug: string,
    _projectId: string,
    _moduleId: string,
    _cookies: ParityBrowserCookie[]
  ): Promise<void> {
    return todo("analyticsDialogOpenFromModule");
  }
  async analyticsSurfaceTriggerCount(): Promise<number> {
    return todo("analyticsSurfaceTriggerCount");
  }
  async analyticsDialogTitle(): Promise<string> {
    return todo("analyticsDialogTitle");
  }
  async analyticsDialogSpinnerSeen(): Promise<boolean> {
    return todo("analyticsDialogSpinnerSeen");
  }
  async analyticsDialogWideToggleVisible(): Promise<boolean> {
    return todo("analyticsDialogWideToggleVisible");
  }
  async analyticsDialogToggleWide(): Promise<void> {
    return todo("analyticsDialogToggleWide");
  }
  async analyticsDialogWidth(): Promise<number> {
    return todo("analyticsDialogWidth");
  }
  async analyticsDialogRowCount(): Promise<number> {
    return todo("analyticsDialogRowCount");
  }
  async analyticsDialogTableHeaders(): Promise<string[]> {
    return todo("analyticsDialogTableHeaders");
  }
  async analyticsDialogTableCells(): Promise<string[][]> {
    return todo("analyticsDialogTableCells");
  }
  async analyticsDialogAssigneeFacts(): Promise<AnalyticsDialogAssignee[]> {
    return todo("analyticsDialogAssigneeFacts");
  }
  async analyticsDialogExportDownload(_tableIndex: 0 | 1): Promise<LocalFileDownload> {
    return todo("analyticsDialogExportDownload");
  }

  // --- Pages blocks/embeds/mentions/links (NEWFRONT-316, PAGE-037-044).
  async pagesBlocksOpenDetail(_workspaceSlug: string, _projectId: string, _pageId: string): Promise<void> {
    return todo("pagesBlocksOpenDetail");
  }
  async pagesBlocksOpenDetailQuery(
    _workspaceSlug: string,
    _projectId: string,
    _pageId: string,
    _query: string
  ): Promise<void> {
    return todo("pagesBlocksOpenDetailQuery");
  }
  async pagesBlocksReload(): Promise<void> {
    return todo("pagesBlocksReload");
  }
  async pagesBlocksEditorText(): Promise<string> {
    return todo("pagesBlocksEditorText");
  }
  async pagesBlocksEditorHtml(): Promise<string> {
    return todo("pagesBlocksEditorHtml");
  }
  async pagesBlocksEditorClickEnd(): Promise<void> {
    return todo("pagesBlocksEditorClickEnd");
  }
  async pagesBlocksEditorType(_text: string): Promise<void> {
    return todo("pagesBlocksEditorType");
  }
  async pagesBlocksEditorPressKey(_chord: string): Promise<void> {
    return todo("pagesBlocksEditorPressKey");
  }
  async pagesBlocksEditorPressEnter(): Promise<void> {
    return todo("pagesBlocksEditorPressEnter");
  }
  async pagesBlocksEditorPressBackspace(): Promise<void> {
    return todo("pagesBlocksEditorPressBackspace");
  }
  async pagesBlocksEditorPasteHtml(_html: string): Promise<void> {
    return todo("pagesBlocksEditorPasteHtml");
  }
  async pagesBlocksEditorPasteText(_text: string): Promise<void> {
    return todo("pagesBlocksEditorPasteText");
  }
  async pagesBlocksEditorSelectAll(): Promise<void> {
    return todo("pagesBlocksEditorSelectAll");
  }
  async pagesBlocksEditorCopy(): Promise<void> {
    return todo("pagesBlocksEditorCopy");
  }
  async pagesBlocksEditorPaste(): Promise<void> {
    return todo("pagesBlocksEditorPaste");
  }
  async pagesBlocksParagraphSelectText(_text: string): Promise<void> {
    return todo("pagesBlocksParagraphSelectText");
  }
  async pagesBlocksSlashOpen(): Promise<boolean> {
    return todo("pagesBlocksSlashOpen");
  }
  async pagesBlocksSlashItems(): Promise<string[]> {
    return todo("pagesBlocksSlashItems");
  }
  async pagesBlocksSlashChoose(_entry: string, _occurrence?: number): Promise<void> {
    return todo("pagesBlocksSlashChoose");
  }
  async pagesBlocksSlashDismiss(): Promise<void> {
    return todo("pagesBlocksSlashDismiss");
  }
  async pagesBlocksDragHandleVisible(): Promise<boolean> {
    return todo("pagesBlocksDragHandleVisible");
  }
  async pagesBlocksDragOpenMenu(): Promise<void> {
    return todo("pagesBlocksDragOpenMenu");
  }
  async pagesBlocksDragMenuItems(): Promise<string[]> {
    return todo("pagesBlocksDragMenuItems");
  }
  async pagesBlocksDragMenuChoose(_label: string): Promise<void> {
    return todo("pagesBlocksDragMenuChoose");
  }
  async pagesBlocksUploadImage(_file: { name: string; mime: string; bytes: Buffer }): Promise<void> {
    return todo("pagesBlocksUploadImage");
  }
  async pagesBlocksInlineImages(): Promise<PagesBlocksInlineImage[]> {
    return todo("pagesBlocksInlineImages");
  }
  async pagesBlocksAssetsTabOpen(): Promise<void> {
    return todo("pagesBlocksAssetsTabOpen");
  }
  async pagesBlocksAssetsTabRows(): Promise<PagesBlocksAssetRow[]> {
    return todo("pagesBlocksAssetsTabRows");
  }
  async pagesBlocksAssetsTabEmptyVisible(): Promise<boolean> {
    return todo("pagesBlocksAssetsTabEmptyVisible");
  }
  async pagesBlocksEmbedUpgradeVisible(): Promise<boolean> {
    return todo("pagesBlocksEmbedUpgradeVisible");
  }
  async pagesBlocksEmbedUpgradeHref(): Promise<string | null> {
    return todo("pagesBlocksEmbedUpgradeHref");
  }
  async pagesBlocksMentionSuggest(_query: string): Promise<string[]> {
    return todo("pagesBlocksMentionSuggest");
  }
  async pagesBlocksMentionChoose(_displayName: string): Promise<void> {
    return todo("pagesBlocksMentionChoose");
  }
  async pagesBlocksMentionChips(): Promise<string[]> {
    return todo("pagesBlocksMentionChips");
  }
  async pagesBlocksEmojiOpen(): Promise<void> {
    return todo("pagesBlocksEmojiOpen");
  }
  async pagesBlocksEmojiItems(): Promise<string[]> {
    return todo("pagesBlocksEmojiItems");
  }
  async pagesBlocksEmojiChoose(_index: number): Promise<void> {
    return todo("pagesBlocksEmojiChoose");
  }
  async pagesBlocksMentionSections(): Promise<string[]> {
    return todo("pagesBlocksMentionSections");
  }
  async pagesBlocksMentionChipHrefs(): Promise<string[]> {
    return todo("pagesBlocksMentionChipHrefs");
  }
  async pagesBlocksLinkSelect(): Promise<void> {
    return todo("pagesBlocksLinkSelect");
  }
  async pagesBlocksLinkBubbleVisible(): Promise<boolean> {
    return todo("pagesBlocksLinkBubbleVisible");
  }
  async pagesBlocksLinkPrefill(): Promise<string> {
    return todo("pagesBlocksLinkPrefill");
  }
  async pagesBlocksLinkEdit(_href: string): Promise<void> {
    return todo("pagesBlocksLinkEdit");
  }
  async pagesBlocksLinkRemove(): Promise<void> {
    return todo("pagesBlocksLinkRemove");
  }
  async pagesBlocksLinkHref(): Promise<string | null> {
    return todo("pagesBlocksLinkHref");
  }
  async pagesBlocksBubbleFormat(_index: number): Promise<void> {
    return todo("pagesBlocksBubbleFormat");
  }
  async pagesBlocksOptionsOpen(): Promise<void> {
    return todo("pagesBlocksOptionsOpen");
  }
  async pagesBlocksOptionsItems(): Promise<string[]> {
    return todo("pagesBlocksOptionsItems");
  }
  async pagesBlocksOptionsCopyMarkdown(): Promise<void> {
    return todo("pagesBlocksOptionsCopyMarkdown");
  }
  async pagesBlocksToastText(): Promise<string | null> {
    return todo("pagesBlocksToastText");
  }
  async pagesBlocksClipboardText(): Promise<string> {
    return todo("pagesBlocksClipboardText");
  }
  async pagesBlocksHeaderLockCycle(): Promise<void> {
    return todo("pagesBlocksHeaderLockCycle");
  }
  async pagesBlocksInfoStats(_expectedCharacters?: number, _skipNavigate?: boolean): Promise<PagesBlocksInfoStats> {
    return todo("pagesBlocksInfoStats");
  }
  // Workspace issue exports settings (NEWFRONT-381, rows EXP-001..013).
  // Appended; existing stubs above are untouched per the shared driver
  // contract. Throw until the exports build slice lands.

  async exportsSettingsOpen(_workspaceSlug: string, _cookies: ParityBrowserCookie[]): Promise<void> {
    return todo("exportsSettingsOpen");
  }

  async exportsSettingsAccess(): Promise<"form" | "not-authorized" | "no-workspace"> {
    return todo("exportsSettingsAccess");
  }

  async exportsSettingsHeading(): Promise<{ title: string; description: string }> {
    return todo("exportsSettingsHeading");
  }

  async exportsSettingsBreadcrumb(): Promise<string[]> {
    return todo("exportsSettingsBreadcrumb");
  }

  async exportsSettingsPageTitle(): Promise<string> {
    return todo("exportsSettingsPageTitle");
  }

  async exportsSettingsCurrentPath(): Promise<string> {
    return todo("exportsSettingsCurrentPath");
  }

  async exportsSettingsProjectPickerOpen(): Promise<void> {
    return todo("exportsSettingsProjectPickerOpen");
  }

  async exportsSettingsProjectPickerOptions(): Promise<Array<{ key: string; name: string }>> {
    return todo("exportsSettingsProjectPickerOptions");
  }

  async exportsSettingsProjectPickerSearch(_query: string): Promise<void> {
    return todo("exportsSettingsProjectPickerSearch");
  }

  async exportsSettingsProjectPickerToggle(_name: string): Promise<void> {
    return todo("exportsSettingsProjectPickerToggle");
  }

  async exportsSettingsProjectPickerLabel(): Promise<string> {
    return todo("exportsSettingsProjectPickerLabel");
  }

  async exportsSettingsProjectPickerDisabled(): Promise<boolean> {
    return todo("exportsSettingsProjectPickerDisabled");
  }

  async exportsSettingsFormatOptions(): Promise<string[]> {
    return todo("exportsSettingsFormatOptions");
  }

  async exportsSettingsFormatSelect(_label: string): Promise<void> {
    return todo("exportsSettingsFormatSelect");
  }

  async exportsSettingsFormatLabel(): Promise<string> {
    return todo("exportsSettingsFormatLabel");
  }

  async exportsSettingsSubmit(): Promise<void> {
    return todo("exportsSettingsSubmit");
  }

  async exportsSettingsSubmitLabel(): Promise<string> {
    return todo("exportsSettingsSubmitLabel");
  }

  async exportsSettingsSubmitDisabled(): Promise<boolean> {
    return todo("exportsSettingsSubmitDisabled");
  }

  async exportsSettingsToast(): Promise<{ kind: "success" | "error"; title: string; message: string } | null> {
    return todo("exportsSettingsToast");
  }

  async exportsSettingsHistoryRows(): Promise<ExportsSettingsHistoryRow[]> {
    return todo("exportsSettingsHistoryRows");
  }

  async exportsSettingsHistoryCount(): Promise<number> {
    return todo("exportsSettingsHistoryCount");
  }

  async exportsSettingsRefresh(): Promise<void> {
    return todo("exportsSettingsRefresh");
  }

  async exportsSettingsRefreshLabel(): Promise<string | null> {
    return todo("exportsSettingsRefreshLabel");
  }

  async exportsSettingsPagination(): Promise<{ visible: boolean; prevDisabled: boolean; nextDisabled: boolean }> {
    return todo("exportsSettingsPagination");
  }

  async exportsSettingsGoPrev(): Promise<void> {
    return todo("exportsSettingsGoPrev");
  }

  async exportsSettingsGoNext(): Promise<void> {
    return todo("exportsSettingsGoNext");
  }

  async exportsSettingsSkeletonVisible(): Promise<boolean> {
    return todo("exportsSettingsSkeletonVisible");
  }

  async exportsSettingsEmptyFacts(): Promise<ExportsSettingsEmptyFacts | null> {
    return todo("exportsSettingsEmptyFacts");
  }

  async exportsSettingsWatchHistoryRequests(): Promise<void> {
    return todo("exportsSettingsWatchHistoryRequests");
  }

  async exportsSettingsHistoryRequestCount(): Promise<number> {
    return todo("exportsSettingsHistoryRequestCount");
  }

  async exportsSettingsCaptureSubmit(): Promise<void> {
    return todo("exportsSettingsCaptureSubmit");
  }

  async exportsSettingsCapturedSubmits(): Promise<Array<{ provider: string; project: string[]; multiple: boolean }>> {
    return todo("exportsSettingsCapturedSubmits");
  }

  async exportsSettingsStallSubmit(): Promise<void> {
    return todo("exportsSettingsStallSubmit");
  }

  async exportsSettingsReleaseSubmit(): Promise<void> {
    return todo("exportsSettingsReleaseSubmit");
  }

  async exportsSettingsFailSubmit(): Promise<void> {
    return todo("exportsSettingsFailSubmit");
  }

  async exportsSettingsUnfailSubmit(): Promise<void> {
    return todo("exportsSettingsUnfailSubmit");
  }

  async exportsSettingsStallHistory(): Promise<void> {
    return todo("exportsSettingsStallHistory");
  }

  async exportsSettingsReleaseHistory(): Promise<void> {
    return todo("exportsSettingsReleaseHistory");
  }

  async exportsSettingsDownloadPopupUrl(_rowIndex: number): Promise<string> {
    return todo("exportsSettingsDownloadPopupUrl");
  }

  // ---------------------------------------------------------------------------
  // Analytics and profile local-file downloads (NEWFRONT-382, EXP-014/015/022).
  // Appended; existing entries above are untouched per the shared contract.
  // ---------------------------------------------------------------------------

  async analyticsExportOpen(_workspaceSlug: string, _cookies: ParityBrowserCookie[]): Promise<void> {
    return todo("analyticsExportOpen");
  }

  async analyticsExportButtonCount(): Promise<number> {
    return todo("analyticsExportButtonCount");
  }

  async analyticsExportRowCells(_kind: AnalyticsExportTable): Promise<string[][]> {
    return todo("analyticsExportRowCells");
  }

  async analyticsExportSearch(_kind: AnalyticsExportTable, _query: string): Promise<void> {
    return todo("analyticsExportSearch");
  }

  async analyticsExportClearSearch(_kind: AnalyticsExportTable): Promise<void> {
    return todo("analyticsExportClearSearch");
  }

  async analyticsExportDownload(_kind: AnalyticsExportTable): Promise<LocalFileDownload> {
    return todo("analyticsExportDownload");
  }

  async analyticsExportYieldsNoDownload(_kind: AnalyticsExportTable): Promise<boolean> {
    return todo("analyticsExportYieldsNoDownload");
  }

  async profileActivityOpen(_workspaceSlug: string, _userId: string, _cookies: ParityBrowserCookie[]): Promise<void> {
    return todo("profileActivityOpen");
  }

  async profileActivityDownloadVisible(): Promise<boolean> {
    return todo("profileActivityDownloadVisible");
  }

  async profileActivityDownloadLabel(): Promise<string | null> {
    return todo("profileActivityDownloadLabel");
  }

  async profileActivityStallExport(): Promise<void> {
    return todo("profileActivityStallExport");
  }

  async profileActivityReleaseExport(): Promise<void> {
    return todo("profileActivityReleaseExport");
  }

  async profileActivityDownload(): Promise<{ file: LocalFileDownload; requestBody: { date: string } }> {
    return todo("profileActivityDownload");
  }

  // --- Reactions oracle (NEWFRONT-116, CMT-022/023/024). Throwing stubs per
  // --- the shared driver contract; the new app implements them later.
  async reactionsIssueChipTooltipText(_emoji: string, _expectedName: string): Promise<string> {
    return todo("reactionsIssueChipTooltipText");
  }
  async reactionsIssueAddEmoji(_emoji: string): Promise<void> {
    return todo("reactionsIssueAddEmoji");
  }
  async reactionsIssuePickerReady(): Promise<boolean> {
    return todo("reactionsIssuePickerReady");
  }
  async reactionsCommentAddEmoji(_cardText: string, _emoji: string): Promise<void> {
    return todo("reactionsCommentAddEmoji");
  }
  async reactionsArchivedCommentClusterVisible(_cardText: string): Promise<boolean> {
    return todo("reactionsArchivedCommentClusterVisible");
  }

  // --- Project lifecycle oracles (NEWFRONT-390, PSET-013..017). Throwing
  // --- stubs per the shared driver contract; the new app implements them later.
  async isCreateFeatureStepVisible(): Promise<boolean> {
    return todo("isCreateFeatureStepVisible");
  }
  async isProjectRestrictionVisible(): Promise<boolean> {
    return todo("isProjectRestrictionVisible");
  }
  async restrictionJoinLabel(): Promise<string | null> {
    return todo("restrictionJoinLabel");
  }
  async clickRestrictionJoin(): Promise<void> {
    return todo("clickRestrictionJoin");
  }
  async submitLeaveHoldingWrite(_holdMs: number): Promise<void> {
    return todo("submitLeaveHoldingWrite");
  }
  async openPublishProjectModal(_workspaceSlug: string, _projectId: string, _projectName: string): Promise<void> {
    return todo("openPublishProjectModal");
  }
  async isPublishModalVisible(): Promise<boolean> {
    return todo("isPublishModalVisible");
  }
  async isPublishModalLoading(): Promise<boolean> {
    return todo("isPublishModalLoading");
  }
  async publishLayoutOptions(): Promise<string[]> {
    return todo("publishLayoutOptions");
  }
  async publishSelectedLayouts(): Promise<string[]> {
    return todo("publishSelectedLayouts");
  }
  async publishToggleLayout(_label: string): Promise<void> {
    return todo("publishToggleLayout");
  }
  async publishToggleState(_name: "comments" | "reactions" | "voting"): Promise<boolean> {
    return todo("publishToggleState");
  }
  async setPublishToggle(_name: "comments" | "reactions" | "voting", _on: boolean): Promise<void> {
    return todo("setPublishToggle");
  }
  async submitPublish(): Promise<void> {
    return todo("submitPublish");
  }
  async publishLiveLink(): Promise<string | null> {
    return todo("publishLiveLink");
  }
  async publishLiveLinkTarget(): Promise<string | null> {
    return todo("publishLiveLinkTarget");
  }
  async clickPublishCopyLink(): Promise<void> {
    return todo("clickPublishCopyLink");
  }
  async isPublishUpdateVisible(): Promise<boolean> {
    return todo("isPublishUpdateVisible");
  }
  async submitPublishUpdate(): Promise<void> {
    return todo("submitPublishUpdate");
  }
  async isPublishUnpublishVisible(): Promise<boolean> {
    return todo("isPublishUnpublishVisible");
  }
  async clickPublishUnpublish(): Promise<void> {
    return todo("clickPublishUnpublish");
  }
  async isPublishUnpublishing(): Promise<boolean> {
    return todo("isPublishUnpublishing");
  }
  async armListSearchRequestSpy(): Promise<void> {
    return todo("armListSearchRequestSpy");
  }
  async listSearchRequestPaths(): Promise<string[]> {
    return todo("listSearchRequestPaths");
  }
  async delayNextPublishRead(_ms: number): Promise<void> {
    return todo("delayNextPublishRead");
  }
  async delayNextPublishWrite(_ms: number): Promise<void> {
    return todo("delayNextPublishWrite");
  }

  // --- Project-settings shell oracle (NEWFRONT-388, PSET-001/002/003/004/062/063).
  // --- The surface is the settings rail plus the per-tab gate; the driver
  // --- reads through it only, so tab bodies landing later never change a
  // --- shell assertion.

  private settingsLinks(): Locator {
    return this.page.locator('a[href*="/settings/projects/"]');
  }

  private async settingsVisibleLinkLabels(): Promise<string[]> {
    return this.settingsLinks().evaluateAll((links) =>
      links
        .filter((node) => {
          const rect = (node as HTMLElement).getBoundingClientRect();
          return rect.width > 0 && rect.height > 0;
        })
        .map((node) => ((node as HTMLElement).innerText ?? "").replace(/\s+/g, " ").trim())
        .filter((text) => text.length > 0)
    );
  }

  /** Settled settings state: denied, restriction, guidance, shell, or null while loading. */
  private async settingsState(): Promise<string | null> {
    return this.page.evaluate(() => {
      const visible = (node: Element | null): boolean => {
        if (node === null) return false;
        const rect = (node as HTMLElement).getBoundingClientRect();
        return rect.width > 0 && rect.height > 0;
      };
      const denial = document.querySelector('[data-testid="project-settings-denied"]');
      if (visible(denial)) return "denied";
      const restriction = document.querySelector('[data-testid="project-settings-restriction"]');
      if (visible(restriction)) return "restriction";
      const guidance = document.querySelector('[data-testid="project-settings-guidance"]');
      if (visible(guidance)) return "guidance";
      const links = Array.from(document.querySelectorAll('a[href*="/settings/projects/"]'));
      if (links.some(visible)) return "shell";
      return null;
    });
  }

  private async settingsWaitSettled(timeoutMs: number): Promise<void> {
    // The rail paints beside the gate, so a gated tab can read "shell"
    // for a beat before flipping to "denied": the state must rest before
    // it counts as settled.
    const deadline = Date.now() + timeoutMs;
    let state: string | null = null;
    let restSince = Date.now();
    for (;;) {
      const next = await this.settingsState();
      if (next !== state) {
        state = next;
        restSince = Date.now();
      } else if (state !== null && Date.now() - restSince >= 2000) {
        return;
      }
      if (Date.now() >= deadline) throw new Error("[parity] settings shell never settled.");
      await this.page.waitForTimeout(500);
    }
  }

  async settingsShellOpen(workspaceSlug: string, projectId: string, tabHref: string): Promise<void> {
    await this.page.goto(`/${workspaceSlug}/settings/projects/${projectId}${tabHref}`);
    await this.page.waitForLoadState("domcontentloaded");
    await this.settingsWaitSettled(60_000);
  }

  async settingsShellGroups(): Promise<string[]> {
    // Group captions are anchor-free divs; keep only the four registry
    // labels so body copy never leaks into the list.
    const known = new Set(["General", "Features", "Work structure", "Execution"]);
    const labels = await this.page.evaluate(() => {
      const out: string[] = [];
      document.querySelectorAll("div").forEach((div) => {
        if (div.querySelector("a") !== null) return;
        const rect = div.getBoundingClientRect();
        if (rect.width === 0 || rect.height === 0) return;
        out.push((div.textContent ?? "").replace(/\s+/g, " ").trim());
      });
      return out;
    });
    return [...new Set(labels.filter((label) => known.has(label)))];
  }

  async settingsShellItems(): Promise<string[]> {
    return this.settingsVisibleLinkLabels();
  }

  async settingsShellActiveItem(): Promise<string | null> {
    const labels = await this.settingsLinks().evaluateAll((links) =>
      links
        .filter((node) => {
          const rect = (node as HTMLElement).getBoundingClientRect();
          if (rect.width === 0 || rect.height === 0) return false;
          return ((node as HTMLElement).getAttribute("class") ?? "").includes("bg-layer-transparent-selected");
        })
        .map((node) => ((node as HTMLElement).innerText ?? "").replace(/\s+/g, " ").trim())
    );
    return labels[0] ?? null;
  }

  async settingsShellPath(): Promise<string> {
    // The app serves settings with and without a trailing slash; the
    // slash is incidental, so reads drop it.
    const path = new URL(this.page.url()).pathname;
    return path.length > 1 ? path.replace(/\/$/, "") : path;
  }

  async settingsShellDrawerTriggerVisible(): Promise<boolean> {
    await this.page.setViewportSize({ width: 390, height: 844 });
    const trigger = this.page.locator('[class*="md:hidden"]').getByRole("button").first();
    try {
      await trigger.waitFor({ timeout: 15_000 });
    } catch {
      return false;
    }
    return trigger.isVisible();
  }

  async settingsShellOpenDrawer(): Promise<void> {
    await this.page.setViewportSize({ width: 390, height: 844 });
    const before = await this.settingsVisibleLinkLabels();
    await this.page.locator('[class*="md:hidden"]').getByRole("button").first().click({ timeout: 15_000 });
    const deadline = Date.now() + 15_000;
    for (;;) {
      const labels = await this.settingsVisibleLinkLabels();
      if (labels.length > before.length) return;
      if (Date.now() >= deadline) throw new Error("[parity] settings drawer never opened.");
      await this.page.waitForTimeout(300);
    }
  }

  async settingsShellDrawerItems(): Promise<string[]> {
    return this.settingsVisibleLinkLabels();
  }

  async settingsShellWideViewport(): Promise<void> {
    await this.page.setViewportSize({ width: 1280, height: 800 });
  }

  async settingsShellDeniedVisible(): Promise<boolean> {
    return (await this.settingsState()) === "denied";
  }

  async settingsShellRestrictionVisible(): Promise<boolean> {
    return (await this.settingsState()) === "restriction";
  }

  async settingsShellRestrictionText(): Promise<string | null> {
    const text = await this.page.evaluate(() => (document.body.innerText ?? "").replace(/\s+/g, " ").trim());
    return text.length > 0 ? text.slice(0, 500) : null;
  }

  async settingsShellJoinVisible(): Promise<boolean> {
    const join = this.page.getByRole("button", { name: "Join project" });
    try {
      await join.first().waitFor({ timeout: 10_000 });
    } catch {
      return false;
    }
    return join.first().isVisible();
  }

  async settingsShellJoin(): Promise<void> {
    await this.page.getByRole("button", { name: "Join project" }).first().click({ timeout: 15_000 });
    const deadline = Date.now() + 60_000;
    for (;;) {
      if ((await this.settingsState()) === "shell") return;
      if (Date.now() >= deadline) throw new Error("[parity] settings shell never settled after join.");
      await this.page.waitForTimeout(500);
    }
  }

  async settingsShellNoProjectsVisible(): Promise<boolean> {
    return (await this.settingsState()) === "guidance";
  }

  async settingsShellGeneralFormVisible(): Promise<boolean> {
    const boxes = this.page.getByRole("textbox");
    try {
      await boxes.first().waitFor({ timeout: 15_000 });
    } catch {
      return false;
    }
    return (await boxes.count()) > 0;
  }

  async settingsShellFillGeneralName(name: string): Promise<void> {
    await this.page.getByRole("textbox").first().fill(name);
  }

  async settingsShellFollowItem(label: string): Promise<void> {
    const from = new URL(this.page.url()).pathname;
    const found = await this.page.evaluate((want) => {
      const match = Array.from(document.querySelectorAll('a[href*="/settings/projects/"]')).find((node) => {
        const rect = (node as HTMLElement).getBoundingClientRect();
        if (rect.width === 0 || rect.height === 0) return false;
        return ((node as HTMLElement).innerText ?? "").replace(/\s+/g, " ").trim() === want;
      });
      if (match === undefined) return false;
      (match as HTMLElement).click();
      return true;
    }, label);
    if (!found) throw new Error(`[parity] no visible settings item labelled ${label}.`);
    const deadline = Date.now() + 60_000;
    for (;;) {
      const now = new URL(this.page.url()).pathname;
      if (now !== from && (await this.settingsState()) !== null) return;
      if (Date.now() >= deadline) throw new Error(`[parity] following settings item ${label} never settled.`);
      await this.page.waitForTimeout(500);
    }
  }

  async settingsShellPressKeys(keys: string[]): Promise<{ dialogSeen: boolean; path: string }> {
    let dialogSeen = false;
    const handler = (): void => {
      dialogSeen = true;
    };
    this.page.on("dialog", handler);
    try {
      for (const key of keys) await this.page.keyboard.press(key);
      await this.page.waitForTimeout(500);
    } finally {
      this.page.off("dialog", handler);
    }
    return { dialogSeen, path: new URL(this.page.url()).pathname };
  }

  async settingsShellBulkExportImportVisible(): Promise<boolean> {
    const buttons = this.page.getByRole("button", { name: /bulk|export|import/i });
    const count = await buttons.count();
    for (let index = 0; index < count; index++) {
      if (await buttons.nth(index).isVisible()) return true;
    }
    return false;
  }

  async settingsShellEditionGateVisible(): Promise<boolean> {
    const text = await this.page.evaluate(() => (document.body.innerText ?? "").replace(/\s+/g, " ").trim());
    return /upgrade to|cloud only|enterprise plan|contact sales|self-hosted only/i.test(text);
  }

  async settingsShellCountSockets(tabHrefs: string[], workspaceSlug: string, projectId: string): Promise<number> {
    let count = 0;
    const handler = (): void => {
      count += 1;
    };
    this.page.on("websocket", handler);
    try {
      for (const href of tabHrefs) {
        await this.page.goto(`/${workspaceSlug}/settings/projects/${projectId}${href}`);
        await this.page.waitForLoadState("domcontentloaded");
        await this.settingsWaitSettled(60_000);
      }
    } finally {
      this.page.off("websocket", handler);
    }
    return count;
  }

  async settingsProjectsOpen(workspaceSlug: string): Promise<void> {
    await this.page.goto(`/${workspaceSlug}/settings/projects`);
    await this.page.waitForLoadState("domcontentloaded");
    await this.settingsWaitSettled(60_000);
  }

  async settingsShellOpenGuidanceCreate(): Promise<void> {
    await this.page.getByRole("button", { name: "Start your first project" }).first().click({ timeout: 15_000 });
    const deadline = Date.now() + 60_000;
    for (;;) {
      if (await this.isCreateProjectDialogVisible()) return;
      if (Date.now() >= deadline) throw new Error("[parity] project creation never opened from guidance.");
      await this.page.waitForTimeout(500);
    }
  }

  // --- Project-settings feature flags oracle (NEWFRONT-393, PSET-043..047,
  // --- PSET-061 slice). Throwing stubs per the shared driver contract; the
  // --- new app implements them later.
  async projectFeaturesOpenTab(
    _workspaceSlug: string,
    _projectId: string,
    _tab: "views" | "cycles" | "intake" | "modules" | "pages"
  ): Promise<void> {
    return todo("projectFeaturesOpenTab");
  }
  async projectFeaturesSwitchOn(): Promise<boolean> {
    return todo("projectFeaturesSwitchOn");
  }
  async projectFeaturesSwitchDisabled(): Promise<boolean> {
    return todo("projectFeaturesSwitchDisabled");
  }
  async projectFeaturesToggle(): Promise<void> {
    return todo("projectFeaturesToggle");
  }
  async projectFeaturesNotAuthorized(): Promise<boolean> {
    return todo("projectFeaturesNotAuthorized");
  }
  async projectFeaturesNavLinkVisible(
    _workspaceSlug: string,
    _projectId: string,
    _area: "cycles" | "modules" | "views" | "pages" | "intake"
  ): Promise<boolean> {
    return todo("projectFeaturesNavLinkVisible");
  }

  // --- Project-settings automations oracle (NEWFRONT-393, PSET-053/054,
  // --- PSET-061 slice). Throwing stubs per the shared driver contract; the
  // --- new app implements them later.
  async projectAutomationsOpenTab(_workspaceSlug: string, _projectId: string): Promise<void> {
    return todo("projectAutomationsOpenTab");
  }
  async projectAutomationsNotAuthorized(): Promise<boolean> {
    return todo("projectAutomationsNotAuthorized");
  }
  async projectAutomationsArchiveOn(): Promise<boolean> {
    return todo("projectAutomationsArchiveOn");
  }
  async projectAutomationsCloseOn(): Promise<boolean> {
    return todo("projectAutomationsCloseOn");
  }
  async projectAutomationsToggleArchive(): Promise<void> {
    return todo("projectAutomationsToggleArchive");
  }
  async projectAutomationsToggleClose(): Promise<void> {
    return todo("projectAutomationsToggleClose");
  }
  async projectAutomationsLoaderVisible(): Promise<boolean> {
    return todo("projectAutomationsLoaderVisible");
  }
  async projectAutomationsDelayDetailsFetch(_ms: number): Promise<void> {
    return todo("projectAutomationsDelayDetailsFetch");
  }
  async projectAutomationsDelayTriggerText(): Promise<string | null> {
    return todo("projectAutomationsDelayTriggerText");
  }
  async projectAutomationsOpenDelayOptions(): Promise<void> {
    return todo("projectAutomationsOpenDelayOptions");
  }
  async projectAutomationsDelayOptionLabels(): Promise<string[]> {
    return todo("projectAutomationsDelayOptionLabels");
  }
  async projectAutomationsPickDelayOption(_label: string): Promise<void> {
    return todo("projectAutomationsPickDelayOption");
  }
  async projectAutomationsOpenCustomize(): Promise<void> {
    return todo("projectAutomationsOpenCustomize");
  }
  async projectAutomationsMonthDialogOpen(): Promise<boolean> {
    return todo("projectAutomationsMonthDialogOpen");
  }
  async projectAutomationsMonthDialogSubmit(_months: string): Promise<void> {
    return todo("projectAutomationsMonthDialogSubmit");
  }
  async projectAutomationsMonthDialogError(): Promise<string | null> {
    return todo("projectAutomationsMonthDialogError");
  }
  async projectAutomationsCloseStateText(): Promise<string | null> {
    return todo("projectAutomationsCloseStateText");
  }
  async projectAutomationsCloseStateDisabled(): Promise<boolean> {
    return todo("projectAutomationsCloseStateDisabled");
  }

  // --- Project-settings github tab oracle (NEWFRONT-393, PSET-050..052,
  // --- PSET-061 slice). Throwing stubs per the shared driver contract; the
  // --- new app implements them later.
  async projectGithubOpenTab(_workspaceSlug: string, _projectId: string): Promise<void> {
    return todo("projectGithubOpenTab");
  }
  async projectGithubNotAuthorized(): Promise<boolean> {
    return todo("projectGithubNotAuthorized");
  }
  async projectGithubLoaderVisible(): Promise<boolean> {
    return todo("projectGithubLoaderVisible");
  }
  async projectGithubDelayBindingFetch(_ms: number): Promise<void> {
    return todo("projectGithubDelayBindingFetch");
  }
  async projectGithubBoundVisible(): Promise<boolean> {
    return todo("projectGithubBoundVisible");
  }
  async projectGithubProviderLabel(): Promise<string | null> {
    return todo("projectGithubProviderLabel");
  }
  async projectGithubRepoLinkText(): Promise<string | null> {
    return todo("projectGithubRepoLinkText");
  }
  async projectGithubContractVisible(): Promise<boolean> {
    return todo("projectGithubContractVisible");
  }
  async projectGithubCadenceText(): Promise<string | null> {
    return todo("projectGithubCadenceText");
  }
  async projectGithubLastError(): Promise<string | null> {
    return todo("projectGithubLastError");
  }
  async projectGithubSyncOn(): Promise<boolean> {
    return todo("projectGithubSyncOn");
  }
  async projectGithubSyncDisabled(): Promise<boolean> {
    return todo("projectGithubSyncDisabled");
  }
  async projectGithubToggleSync(): Promise<void> {
    return todo("projectGithubToggleSync");
  }
  async projectGithubUnbindVisible(): Promise<boolean> {
    return todo("projectGithubUnbindVisible");
  }
  async projectGithubUnbindDisabled(): Promise<boolean> {
    return todo("projectGithubUnbindDisabled");
  }
  async projectGithubUnbind(): Promise<void> {
    return todo("projectGithubUnbind");
  }
  async projectGithubGuidanceVisible(): Promise<boolean> {
    return todo("projectGithubGuidanceVisible");
  }
  async projectGithubTabText(): Promise<string> {
    return todo("projectGithubTabText");
  }

  // --- Workspace-settings members + invitations oracle (NEWFRONT-411,
  // --- WSET-008..015). Throwing stubs per the shared driver contract; the
  // --- new app implements them later.
  async wsSettingsMembersOpen(_workspaceSlug: string): Promise<void> {
    return todo("wsSettingsMembersOpen");
  }
  async wsSettingsMembersCountChip(): Promise<number | null> {
    return todo("wsSettingsMembersCountChip");
  }
  async wsSettingsMembersSearch(_query: string): Promise<void> {
    return todo("wsSettingsMembersSearch");
  }
  async wsSettingsMembersSearchValue(): Promise<string> {
    return todo("wsSettingsMembersSearchValue");
  }
  async wsSettingsMembersVisibleEmails(): Promise<string[]> {
    return todo("wsSettingsMembersVisibleEmails");
  }
  async wsSettingsMembersVisibleRoles(): Promise<{ email: string; role: string }[]> {
    return todo("wsSettingsMembersVisibleRoles");
  }
  async wsSettingsMembersSuspendedEmails(): Promise<string[]> {
    return todo("wsSettingsMembersSuspendedEmails");
  }
  async wsSettingsMembersOpenFilters(): Promise<void> {
    return todo("wsSettingsMembersOpenFilters");
  }
  async wsSettingsMembersFilterOptions(): Promise<string[]> {
    return todo("wsSettingsMembersFilterOptions");
  }
  async wsSettingsMembersToggleFilter(_label: string): Promise<void> {
    return todo("wsSettingsMembersToggleFilter");
  }
  async wsSettingsMembersCloseFilters(): Promise<void> {
    return todo("wsSettingsMembersCloseFilters");
  }
  async wsSettingsMembersFilterBadge(): Promise<boolean> {
    return todo("wsSettingsMembersFilterBadge");
  }
  async wsSettingsMembersNoMatchVisible(): Promise<boolean> {
    return todo("wsSettingsMembersNoMatchVisible");
  }
  async wsSettingsMembersNoMatchText(): Promise<string | null> {
    return todo("wsSettingsMembersNoMatchText");
  }
  async wsSettingsMembersLoaderVisible(): Promise<boolean> {
    return todo("wsSettingsMembersLoaderVisible");
  }
  async wsSettingsMembersDelayNextList(_ms: number): Promise<void> {
    return todo("wsSettingsMembersDelayNextList");
  }
  async wsSettingsMembersAddVisible(): Promise<boolean> {
    return todo("wsSettingsMembersAddVisible");
  }
  async wsSettingsInviteOpen(): Promise<void> {
    return todo("wsSettingsInviteOpen");
  }
  async wsSettingsInviteClose(): Promise<void> {
    return todo("wsSettingsInviteClose");
  }
  async wsSettingsInviteVisible(): Promise<boolean> {
    return todo("wsSettingsInviteVisible");
  }
  async wsSettingsInviteRowCount(): Promise<number> {
    return todo("wsSettingsInviteRowCount");
  }
  async wsSettingsInviteFillRow(_index: number, _email: string): Promise<void> {
    return todo("wsSettingsInviteFillRow");
  }
  async wsSettingsInviteRowValue(_index: number): Promise<string> {
    return todo("wsSettingsInviteRowValue");
  }
  async wsSettingsInviteRowRole(_index: number): Promise<string> {
    return todo("wsSettingsInviteRowRole");
  }
  async wsSettingsInviteSetRowRole(_index: number, _roleLabel: string): Promise<void> {
    return todo("wsSettingsInviteSetRowRole");
  }
  async wsSettingsInviteRowRoles(_index: number): Promise<string[]> {
    return todo("wsSettingsInviteRowRoles");
  }
  async wsSettingsInviteAddRow(): Promise<void> {
    return todo("wsSettingsInviteAddRow");
  }
  async wsSettingsInviteRemoveRow(_index: number): Promise<void> {
    return todo("wsSettingsInviteRemoveRow");
  }
  async wsSettingsInviteRowError(_index: number): Promise<string | null> {
    return todo("wsSettingsInviteRowError");
  }
  async wsSettingsInviteSubmit(): Promise<void> {
    return todo("wsSettingsInviteSubmit");
  }
  async wsSettingsInvitesGroupVisible(): Promise<boolean> {
    return todo("wsSettingsInvitesGroupVisible");
  }
  async wsSettingsInvitesCount(): Promise<number | null> {
    return todo("wsSettingsInvitesCount");
  }
  async wsSettingsInvitesVisibleEmails(): Promise<string[]> {
    return todo("wsSettingsInvitesVisibleEmails");
  }
  async wsSettingsInvitesToggleGroup(): Promise<void> {
    return todo("wsSettingsInvitesToggleGroup");
  }
  async wsSettingsInvitesExpanded(): Promise<boolean> {
    return todo("wsSettingsInvitesExpanded");
  }
  async wsSettingsInviteRole(_email: string): Promise<string> {
    return todo("wsSettingsInviteRole");
  }
  async wsSettingsInviteRoleOptions(_email: string): Promise<string[]> {
    return todo("wsSettingsInviteRoleOptions");
  }
  async wsSettingsInviteSetRole(_email: string, _roleLabel: string): Promise<void> {
    return todo("wsSettingsInviteSetRole");
  }
  async wsSettingsInviteMenuEntries(_email: string): Promise<string[]> {
    return todo("wsSettingsInviteMenuEntries");
  }
  async wsSettingsInviteCopyLink(_email: string): Promise<string> {
    return todo("wsSettingsInviteCopyLink");
  }
  async wsSettingsInviteOpenRemove(_email: string): Promise<void> {
    return todo("wsSettingsInviteOpenRemove");
  }
  async wsSettingsRemoveDialogVisible(): Promise<boolean> {
    return todo("wsSettingsRemoveDialogVisible");
  }
  async wsSettingsRemoveDialogTitle(): Promise<string | null> {
    return todo("wsSettingsRemoveDialogTitle");
  }
  async wsSettingsRemoveDialogText(): Promise<string | null> {
    return todo("wsSettingsRemoveDialogText");
  }
  async wsSettingsRemoveDialogConfirm(): Promise<void> {
    return todo("wsSettingsRemoveDialogConfirm");
  }
  async wsSettingsRemoveDialogCancel(): Promise<void> {
    return todo("wsSettingsRemoveDialogCancel");
  }
  async wsSettingsMemberRowFacts(_email: string): Promise<WsSettingsMemberRowFacts> {
    return todo("wsSettingsMemberRowFacts");
  }
  async wsSettingsMemberRoleEditable(_email: string): Promise<boolean> {
    return todo("wsSettingsMemberRoleEditable");
  }
  async wsSettingsMemberRoleOptions(_email: string): Promise<string[]> {
    return todo("wsSettingsMemberRoleOptions");
  }
  async wsSettingsMemberSetRole(_email: string, _roleLabel: string): Promise<void> {
    return todo("wsSettingsMemberSetRole");
  }
  async wsSettingsMemberMenuEntry(_email: string): Promise<string | null> {
    return todo("wsSettingsMemberMenuEntry");
  }
  async wsSettingsMemberOpenRemove(_email: string): Promise<void> {
    return todo("wsSettingsMemberOpenRemove");
  }
  async wsSettingsMembersActivityVisible(): Promise<boolean> {
    return todo("wsSettingsMembersActivityVisible");
  }
  async wsSettingsMembersBillingVisible(): Promise<boolean> {
    return todo("wsSettingsMembersBillingVisible");
  }

  // --- Project members tab (NEWFRONT-391, PSET-018–027). Throwing stubs per
  // --- the shared driver contract; the new app implements them later.
  async settingsMembersOpen(_workspaceSlug: string, _projectId: string): Promise<void> {
    return todo("settingsMembersOpen");
  }
  async settingsMembersDenied(): Promise<boolean> {
    return todo("settingsMembersDenied");
  }
  async settingsMembersRows(): Promise<
    {
      fullName: string;
      displayName: string;
      email: string;
      roleLabel: string;
      joinedText: string;
      profileHref: string | null;
    }[]
  > {
    return todo("settingsMembersRows");
  }
  async settingsMembersSearchFill(_query: string): Promise<void> {
    return todo("settingsMembersSearchFill");
  }
  async settingsMembersNoMatchVisible(): Promise<boolean> {
    return todo("settingsMembersNoMatchVisible");
  }
  async settingsMembersFiltersOpen(): Promise<void> {
    return todo("settingsMembersFiltersOpen");
  }
  async settingsMembersFilterOptions(): Promise<string[]> {
    return todo("settingsMembersFilterOptions");
  }
  async settingsMembersFilterChecked(_label: string): Promise<boolean> {
    return todo("settingsMembersFilterChecked");
  }
  async settingsMembersFilterToggle(_label: string): Promise<void> {
    return todo("settingsMembersFilterToggle");
  }
  async settingsMembersFiltersClose(): Promise<void> {
    return todo("settingsMembersFiltersClose");
  }
  async settingsMembersFilterActive(): Promise<boolean> {
    return todo("settingsMembersFilterActive");
  }
  async settingsMembersSortOpen(_column: string): Promise<void> {
    return todo("settingsMembersSortOpen");
  }
  async settingsMembersSortItems(): Promise<string[]> {
    return todo("settingsMembersSortItems");
  }
  async settingsMembersSortPick(_text: string): Promise<void> {
    return todo("settingsMembersSortPick");
  }
  async settingsMembersGotoTab(_tab: string): Promise<void> {
    return todo("settingsMembersGotoTab");
  }
  async settingsMembersAddVisible(): Promise<boolean> {
    return todo("settingsMembersAddVisible");
  }
  async settingsMembersOpenInvite(): Promise<void> {
    return todo("settingsMembersOpenInvite");
  }
  async settingsMembersInviteOpen(): Promise<boolean> {
    return todo("settingsMembersInviteOpen");
  }
  async settingsMembersInviteRowCount(): Promise<number> {
    return todo("settingsMembersInviteRowCount");
  }
  async settingsMembersInviteAddRow(): Promise<void> {
    return todo("settingsMembersInviteAddRow");
  }
  async settingsMembersInviteRemoveRow(_index: number): Promise<void> {
    return todo("settingsMembersInviteRemoveRow");
  }
  async settingsMembersInviteOpenMemberPicker(_index: number): Promise<void> {
    return todo("settingsMembersInviteOpenMemberPicker");
  }
  async settingsMembersInviteMemberOptions(): Promise<string[]> {
    return todo("settingsMembersInviteMemberOptions");
  }
  async settingsMembersInvitePickMember(_index: number, _text: string): Promise<void> {
    return todo("settingsMembersInvitePickMember");
  }
  async settingsMembersInviteRoleOptions(_index: number): Promise<string[]> {
    return todo("settingsMembersInviteRoleOptions");
  }
  async settingsMembersInvitePickRole(_index: number, _label: string): Promise<void> {
    return todo("settingsMembersInvitePickRole");
  }
  async settingsMembersInviteRowError(_index: number): Promise<string> {
    return todo("settingsMembersInviteRowError");
  }
  async settingsMembersInviteSubmit(): Promise<void> {
    return todo("settingsMembersInviteSubmit");
  }
  async settingsMembersInviteAttemptSubmit(): Promise<void> {
    return todo("settingsMembersInviteAttemptSubmit");
  }
  async settingsMembersInviteCancel(): Promise<void> {
    return todo("settingsMembersInviteCancel");
  }
  async settingsMembersRoleEditable(_displayName: string): Promise<boolean> {
    return todo("settingsMembersRoleEditable");
  }
  async settingsMembersRoleText(_displayName: string): Promise<string> {
    return todo("settingsMembersRoleText");
  }
  async settingsMembersRoleEditorOpen(_displayName: string): Promise<void> {
    return todo("settingsMembersRoleEditorOpen");
  }
  async settingsMembersRoleEditorOptions(): Promise<string[]> {
    return todo("settingsMembersRoleEditorOptions");
  }
  async settingsMembersRoleEditorPick(_label: string): Promise<void> {
    return todo("settingsMembersRoleEditorPick");
  }
  async settingsMembersOpenRowMenu(_displayName: string): Promise<void> {
    return todo("settingsMembersOpenRowMenu");
  }
  async settingsMembersRowMenuItems(): Promise<string[]> {
    return todo("settingsMembersRowMenuItems");
  }
  async settingsMembersRowMenuPick(_text: string): Promise<void> {
    return todo("settingsMembersRowMenuPick");
  }
  async settingsMembersConfirmText(): Promise<string> {
    return todo("settingsMembersConfirmText");
  }
  async settingsMembersConfirmSubmit(): Promise<void> {
    return todo("settingsMembersConfirmSubmit");
  }
  async settingsMembersConfirmCancel(): Promise<void> {
    return todo("settingsMembersConfirmCancel");
  }
  async settingsMembersSkeletonVisible(): Promise<boolean> {
    return todo("settingsMembersSkeletonVisible");
  }
  async settingsMembersDelayLoad(_ms: number): Promise<void> {
    return todo("settingsMembersDelayLoad");
  }
  async settingsMembersDefaultsVisible(): Promise<boolean> {
    return todo("settingsMembersDefaultsVisible");
  }
  async settingsMembersLeadText(): Promise<string> {
    return todo("settingsMembersLeadText");
  }
  async settingsMembersLeadPick(_displayName: string): Promise<void> {
    return todo("settingsMembersLeadPick");
  }
  async settingsMembersAssigneeText(): Promise<string> {
    return todo("settingsMembersAssigneeText");
  }
  async settingsMembersAssigneePick(_displayName: string): Promise<void> {
    return todo("settingsMembersAssigneePick");
  }
  async settingsMembersLeadDisabled(): Promise<boolean> {
    return todo("settingsMembersLeadDisabled");
  }
  async settingsMembersGuestToggleState(): Promise<boolean> {
    return todo("settingsMembersGuestToggleState");
  }
  async settingsMembersGuestToggle(): Promise<void> {
    return todo("settingsMembersGuestToggle");
  }
  async settingsMembersGuestToggleDisabled(): Promise<boolean> {
    return todo("settingsMembersGuestToggleDisabled");
  }
  async settingsMembersInviteCloseMemberPicker(_index: number): Promise<void> {
    return todo("settingsMembersInviteCloseMemberPicker");
  }
  async settingsMembersSortMarker(_column: string): Promise<"ascending" | "descending" | "none"> {
    return todo("settingsMembersSortMarker");
  }

  // --- Project settings workflow states (NEWFRONT-392, PSET-028–035).
  // --- Throwing stubs per the shared driver contract; the new app
  // --- implements them later.
  async settingsStatesOpen(_workspaceSlug: string, _projectId: string): Promise<void> {
    return todo("settingsStatesOpen");
  }
  async settingsStatesGroupNames(): Promise<string[]> {
    return todo("settingsStatesGroupNames");
  }
  async settingsStatesGroupExpanded(_group: string): Promise<boolean> {
    return todo("settingsStatesGroupExpanded");
  }
  async settingsStatesToggleGroup(_group: string): Promise<void> {
    return todo("settingsStatesToggleGroup");
  }
  async settingsStatesStateNames(_group: string): Promise<string[]> {
    return todo("settingsStatesStateNames");
  }
  async settingsStatesAddEnabled(_group: string): Promise<boolean> {
    return todo("settingsStatesAddEnabled");
  }
  async settingsStatesOpenCreate(_group: string): Promise<void> {
    return todo("settingsStatesOpenCreate");
  }
  async settingsStatesFormVisible(): Promise<boolean> {
    return todo("settingsStatesFormVisible");
  }
  async settingsStatesFillName(_name: string): Promise<void> {
    return todo("settingsStatesFillName");
  }
  async settingsStatesFillDescription(_text: string): Promise<void> {
    return todo("settingsStatesFillDescription");
  }
  async settingsStatesNameValue(): Promise<string> {
    return todo("settingsStatesNameValue");
  }
  async settingsStatesDescriptionValue(): Promise<string> {
    return todo("settingsStatesDescriptionValue");
  }
  async settingsStatesFormError(): Promise<string> {
    return todo("settingsStatesFormError");
  }
  async settingsStatesSubmitCreate(): Promise<void> {
    return todo("settingsStatesSubmitCreate");
  }
  async settingsStatesSubmitUpdate(): Promise<void> {
    return todo("settingsStatesSubmitUpdate");
  }
  async settingsStatesCancelForm(): Promise<void> {
    return todo("settingsStatesCancelForm");
  }
  async settingsStatesAttemptSubmit(): Promise<void> {
    return todo("settingsStatesAttemptSubmit");
  }
  async settingsStatesDotColor(): Promise<string> {
    return todo("settingsStatesDotColor");
  }
  async settingsStatesOpenColorPicker(): Promise<void> {
    return todo("settingsStatesOpenColorPicker");
  }
  async settingsStatesPickColor(_hex: string): Promise<void> {
    return todo("settingsStatesPickColor");
  }
  async settingsStatesOpenEdit(_name: string): Promise<void> {
    return todo("settingsStatesOpenEdit");
  }
  async settingsStatesGripVisible(_name: string): Promise<boolean> {
    return todo("settingsStatesGripVisible");
  }
  async settingsStatesDragAbove(_source: string, _target: string): Promise<void> {
    return todo("settingsStatesDragAbove");
  }
  async settingsStatesDragBelow(_source: string, _target: string): Promise<void> {
    return todo("settingsStatesDragBelow");
  }
  async settingsStatesDeleteDisabled(_name: string): Promise<boolean> {
    return todo("settingsStatesDeleteDisabled");
  }
  async settingsStatesDelete(_name: string): Promise<void> {
    return todo("settingsStatesDelete");
  }
  async settingsStatesDeleteModalText(): Promise<string> {
    return todo("settingsStatesDeleteModalText");
  }
  async settingsStatesDeleteConfirm(): Promise<void> {
    return todo("settingsStatesDeleteConfirm");
  }
  async settingsStatesDeleteCancel(): Promise<void> {
    return todo("settingsStatesDeleteCancel");
  }
  async settingsStatesAttemptDeleteConfirm(): Promise<void> {
    return todo("settingsStatesAttemptDeleteConfirm");
  }
  async settingsStatesMarkDefault(_name: string): Promise<void> {
    return todo("settingsStatesMarkDefault");
  }
  async settingsStatesIsDefault(_name: string): Promise<boolean> {
    return todo("settingsStatesIsDefault");
  }
  async settingsStatesMemberToggleVisible(): Promise<boolean> {
    return todo("settingsStatesMemberToggleVisible");
  }
  async settingsStatesMemberToggleOn(): Promise<boolean> {
    return todo("settingsStatesMemberToggleOn");
  }
  async settingsStatesSetMemberToggle(_on: boolean): Promise<void> {
    return todo("settingsStatesSetMemberToggle");
  }
  async settingsStatesAccessDenied(): Promise<boolean> {
    return todo("settingsStatesAccessDenied");
  }
  async settingsStatesSkeletonVisible(): Promise<boolean> {
    return todo("settingsStatesSkeletonVisible");
  }
  async settingsStatesDelayLoad(_ms: number): Promise<void> {
    return todo("settingsStatesDelayLoad");
  }
  async settingsStatesEmptyNote(_group: string): Promise<string> {
    return todo("settingsStatesEmptyNote");
  }

  // --- Project settings labels gaps (NEWFRONT-392, PSET-036–042). ---
  async settingsLabelsChildrenOf(_parent: string): Promise<string[]> {
    return todo("settingsLabelsChildrenOf");
  }
  async settingsLabelsToggleGroup(_name: string): Promise<void> {
    return todo("settingsLabelsToggleGroup");
  }
  async settingsLabelsGroupCollapsed(_name: string): Promise<boolean> {
    return todo("settingsLabelsGroupCollapsed");
  }
  async settingsLabelsGripVisible(_name: string): Promise<boolean> {
    return todo("settingsLabelsGripVisible");
  }
  async settingsLabelsDragFeedback(
    _source: string,
    _target: string,
    _edge: "nest" | "above"
  ): Promise<"line" | "ring" | "none"> {
    return todo("settingsLabelsDragFeedback");
  }
  async settingsLabelsListScrollable(): Promise<boolean> {
    return todo("settingsLabelsListScrollable");
  }
  async settingsLabelsDeleteAttemptConfirm(): Promise<void> {
    return todo("settingsLabelsDeleteAttemptConfirm");
  }

  // --- Workspace-settings cross-cutting oracle (NEWFRONT-415). Throwing
  // --- stubs per the shared driver contract; the area fills these in when
  // --- it lands on web_new.

  async wsSettingsOpenSection(
    _workspaceSlug: string,
    _section: "general" | "members" | "billing" | "exports" | "webhooks" | "integrations",
    _cookies: ParityBrowserCookie[]
  ): Promise<void> {
    return todo("wsSettingsOpenSection");
  }

  async wsSettingsNotAuthorized(): Promise<boolean> {
    return todo("wsSettingsNotAuthorized");
  }

  async wsSettingsSidebarVisible(): Promise<boolean> {
    return todo("wsSettingsSidebarVisible");
  }

  async wsSettingsOutletText(): Promise<string> {
    return todo("wsSettingsOutletText");
  }

  async wsSettingsCurrentPath(): Promise<string> {
    return todo("wsSettingsCurrentPath");
  }

  async wsSettingsOpenWebhookDetail(
    _workspaceSlug: string,
    _webhookId: string,
    _cookies?: ParityBrowserCookie[]
  ): Promise<void> {
    return todo("wsSettingsOpenWebhookDetail");
  }

  async wsSettingsWebhookDetailState(): Promise<"form" | "denied" | "loading"> {
    return todo("wsSettingsWebhookDetailState");
  }

  async wsSettingsDesktopOnlyCount(): Promise<number> {
    return todo("wsSettingsDesktopOnlyCount");
  }

  async wsSettingsFailInstanceInfo(): Promise<void> {
    return todo("wsSettingsFailInstanceInfo");
  }

  async wsSettingsServeInstanceNotReady(): Promise<void> {
    return todo("wsSettingsServeInstanceNotReady");
  }

  async wsSettingsResetInstanceRoutes(): Promise<void> {
    return todo("wsSettingsResetInstanceRoutes");
  }

  async wsSettingsMaintenanceVisible(): Promise<boolean> {
    return todo("wsSettingsMaintenanceVisible");
  }

  async wsSettingsMaintenanceIllustration(): Promise<string | null> {
    return todo("wsSettingsMaintenanceIllustration");
  }

  async wsSettingsMaintenanceSupportHref(): Promise<string | null> {
    return todo("wsSettingsMaintenanceSupportHref");
  }

  async wsSettingsNotReadyFacts(): Promise<{
    heading: string | null;
    getStartedHref: string | null;
    workspaceContent: boolean;
  }> {
    return todo("wsSettingsNotReadyFacts");
  }

  async wsSettingsSetColorScheme(_scheme: "light" | "dark"): Promise<void> {
    return todo("wsSettingsSetColorScheme");
  }

  async wsSettingsOpenInviteDialog(_workspaceSlug: string, _cookies: ParityBrowserCookie[]): Promise<void> {
    return todo("wsSettingsOpenInviteDialog");
  }

  async wsSettingsOpenWebhookCreateDialog(_workspaceSlug: string, _cookies: ParityBrowserCookie[]): Promise<void> {
    return todo("wsSettingsOpenWebhookCreateDialog");
  }

  async wsSettingsOpenTokenCreateDialog(_cookies: ParityBrowserCookie[]): Promise<void> {
    return todo("wsSettingsOpenTokenCreateDialog");
  }

  async wsSettingsOpenRemoveMemberDialog(
    _workspaceSlug: string,
    _memberEmail: string,
    _cookies: ParityBrowserCookie[]
  ): Promise<void> {
    return todo("wsSettingsOpenRemoveMemberDialog");
  }

  async wsSettingsOpenDeleteWebhookDialog(
    _workspaceSlug: string,
    _webhookId: string,
    _cookies: ParityBrowserCookie[]
  ): Promise<void> {
    return todo("wsSettingsOpenDeleteWebhookDialog");
  }

  async wsSettingsDialogOpen(): Promise<boolean> {
    return todo("wsSettingsDialogOpen");
  }

  async wsSettingsDialogButtons(): Promise<string[]> {
    return todo("wsSettingsDialogButtons");
  }

  async wsSettingsPressEscape(): Promise<void> {
    return todo("wsSettingsPressEscape");
  }

  async wsSettingsCloseDialogExplicit(): Promise<void> {
    return todo("wsSettingsCloseDialogExplicit");
  }

  async wsSettingsDialogTabOrder(_presses: number): Promise<{ order: string[]; escaped: boolean }> {
    return todo("wsSettingsDialogTabOrder");
  }

  async wsSettingsPressKeys(_keys: string[]): Promise<void> {
    return todo("wsSettingsPressKeys");
  }

  async wsSettingsMembersSearchEscapeProbe(_query: string): Promise<{ value: string; visible: boolean }> {
    return todo("wsSettingsMembersSearchEscapeProbe");
  }

  async wsSettingsWatchListRequests(): Promise<void> {
    return todo("wsSettingsWatchListRequests");
  }

  async wsSettingsListRequestCount(): Promise<number> {
    return todo("wsSettingsListRequestCount");
  }

  async wsSettingsWatchSockets(): Promise<void> {
    return todo("wsSettingsWatchSockets");
  }

  async wsSettingsSocketCount(): Promise<number> {
    return todo("wsSettingsSocketCount");
  }

  async wsSettingsMembersListText(): Promise<string[]> {
    return todo("wsSettingsMembersListText");
  }

  async wsSettingsWebhooksListText(): Promise<string[]> {
    return todo("wsSettingsWebhooksListText");
  }

  async wsSettingsDraggableCount(): Promise<number> {
    return todo("wsSettingsDraggableCount");
  }

  async wsSettingsDragFirstMemberRowToSecond(): Promise<void> {
    return todo("wsSettingsDragFirstMemberRowToSecond");
  }

  async wsSettingsFileInputCount(): Promise<number> {
    return todo("wsSettingsFileInputCount");
  }

  async wsSettingsImportMentions(): Promise<string[]> {
    return todo("wsSettingsImportMentions");
  }

  async wsSettingsWatchDownloads(): Promise<void> {
    return todo("wsSettingsWatchDownloads");
  }

  async wsSettingsDownloadCount(): Promise<number> {
    return todo("wsSettingsDownloadCount");
  }

  // --- Workspace billing oracle (NEWFRONT-414, WSET-029..032). Appended;
  // --- existing stubs above are untouched per the shared driver contract.
  async wsSettingsBillingOpen(_workspaceSlug: string): Promise<void> {
    return todo("wsSettingsBillingOpen");
  }
  async wsSettingsBillingNotAuthorized(): Promise<boolean> {
    return todo("wsSettingsBillingNotAuthorized");
  }
  async wsSettingsBillingCommunityTitle(): Promise<string | null> {
    return todo("wsSettingsBillingCommunityTitle");
  }
  async wsSettingsBillingCommunityDescription(): Promise<string | null> {
    return todo("wsSettingsBillingCommunityDescription");
  }
  async wsSettingsBillingPlanNames(): Promise<string[]> {
    return todo("wsSettingsBillingPlanNames");
  }
  async wsSettingsBillingPopularPlan(): Promise<string | null> {
    return todo("wsSettingsBillingPopularPlan");
  }
  async wsSettingsBillingPlanPrice(_planName: string): Promise<string | null> {
    return todo("wsSettingsBillingPlanPrice");
  }
  async wsSettingsBillingFrequencyVisible(_planName: string): Promise<boolean> {
    return todo("wsSettingsBillingFrequencyVisible");
  }
  async wsSettingsBillingSelectedFrequency(_planName: string): Promise<"month" | "year" | null> {
    return todo("wsSettingsBillingSelectedFrequency");
  }
  async wsSettingsBillingSelectFrequency(_planName: string, _frequency: "month" | "year"): Promise<void> {
    return todo("wsSettingsBillingSelectFrequency");
  }
  async wsSettingsBillingDiscountBadge(_planName: string): Promise<string | null> {
    return todo("wsSettingsBillingDiscountBadge");
  }
  async wsSettingsBillingCompareToggleLabel(): Promise<string | null> {
    return todo("wsSettingsBillingCompareToggleLabel");
  }
  async wsSettingsBillingToggleCompare(): Promise<void> {
    return todo("wsSettingsBillingToggleCompare");
  }
  async wsSettingsBillingComparisonVisible(): Promise<boolean> {
    return todo("wsSettingsBillingComparisonVisible");
  }
  async wsSettingsBillingComparisonSections(): Promise<string[]> {
    return todo("wsSettingsBillingComparisonSections");
  }
  async wsSettingsBillingPlanButtonLabel(_planName: string): Promise<string | null> {
    return todo("wsSettingsBillingPlanButtonLabel");
  }
  async wsSettingsBillingPlanButtonPopupUrl(_planName: string): Promise<string | null> {
    return todo("wsSettingsBillingPlanButtonPopupUrl");
  }
  async wsSettingsBillingPageText(): Promise<string> {
    return todo("wsSettingsBillingPageText");
  }
  async wsSettingsUpgradeModalVisible(): Promise<boolean> {
    return todo("wsSettingsUpgradeModalVisible");
  }
  async wsSettingsUpgradeModalTitle(): Promise<string | null> {
    return todo("wsSettingsUpgradeModalTitle");
  }
  async wsSettingsUserMenuOpenCommunity(_workspaceSlug: string): Promise<void> {
    return todo("wsSettingsUserMenuOpenCommunity");
  }
  async wsSettingsActiveCyclesOpen(_workspaceSlug: string): Promise<void> {
    return todo("wsSettingsActiveCyclesOpen");
  }
  async wsSettingsUpgradeModalClose(): Promise<void> {
    return todo("wsSettingsUpgradeModalClose");
  }
  async wsSettingsProBadgeCount(): Promise<number> {
    return todo("wsSettingsProBadgeCount");
  }
  async wsSettingsProBadgeClickFirst(): Promise<void> {
    return todo("wsSettingsProBadgeClickFirst");
  }

  // --- Workspace-settings shell + general (NEWFRONT-410, WSET-001..007). ---

  async wsSettingsOpen(_workspaceSlug: string, _section: string): Promise<void> {
    return todo("wsSettingsOpen");
  }
  async wsSettingsGroups(): Promise<string[]> {
    return todo("wsSettingsGroups");
  }
  async wsSettingsItems(): Promise<string[]> {
    return todo("wsSettingsItems");
  }
  async wsSettingsActiveItem(): Promise<string | null> {
    return todo("wsSettingsActiveItem");
  }
  async wsSettingsFollowItem(_label: string): Promise<void> {
    return todo("wsSettingsFollowItem");
  }
  async wsSettingsPath(): Promise<string> {
    return todo("wsSettingsPath");
  }
  async wsSettingsTitle(): Promise<string> {
    return todo("wsSettingsTitle");
  }
  async wsSettingsDeniedVisible(): Promise<boolean> {
    return todo("wsSettingsDeniedVisible");
  }
  async wsSettingsNarrowTriggerVisible(): Promise<boolean> {
    return todo("wsSettingsNarrowTriggerVisible");
  }
  async wsSettingsOpenDrawer(): Promise<void> {
    return todo("wsSettingsOpenDrawer");
  }
  async wsSettingsDrawerItems(): Promise<string[]> {
    return todo("wsSettingsDrawerItems");
  }
  async wsSettingsMobileCrumb(): Promise<string> {
    return todo("wsSettingsMobileCrumb");
  }
  async wsSettingsWideViewport(): Promise<void> {
    return todo("wsSettingsWideViewport");
  }
  async wsSettingsGeneralVisible(): Promise<boolean> {
    return todo("wsSettingsGeneralVisible");
  }
  async wsSettingsNameValue(): Promise<string> {
    return todo("wsSettingsNameValue");
  }
  async wsSettingsFillName(_name: string): Promise<void> {
    return todo("wsSettingsFillName");
  }
  async wsSettingsNameDisabled(): Promise<boolean> {
    return todo("wsSettingsNameDisabled");
  }
  async wsSettingsNameError(): Promise<string | null> {
    return todo("wsSettingsNameError");
  }
  async wsSettingsSizeLabel(): Promise<string> {
    return todo("wsSettingsSizeLabel");
  }
  async wsSettingsSizeOptions(): Promise<string[]> {
    return todo("wsSettingsSizeOptions");
  }
  async wsSettingsSelectSize(_label: string): Promise<void> {
    return todo("wsSettingsSelectSize");
  }
  async wsSettingsTimezoneLabel(): Promise<string> {
    return todo("wsSettingsTimezoneLabel");
  }
  async wsSettingsSelectTimezone(_query: string, _option: string): Promise<void> {
    return todo("wsSettingsSelectTimezone");
  }
  async wsSettingsUrlValue(): Promise<string> {
    return todo("wsSettingsUrlValue");
  }
  async wsSettingsSaveVisible(): Promise<boolean> {
    return todo("wsSettingsSaveVisible");
  }
  async wsSettingsSaveLabel(): Promise<string | null> {
    return todo("wsSettingsSaveLabel");
  }
  async wsSettingsSave(): Promise<void> {
    return todo("wsSettingsSave");
  }
  async wsSettingsDelaySave(_ms: number): Promise<void> {
    return todo("wsSettingsDelaySave");
  }
  async wsSettingsFailSave(): Promise<void> {
    return todo("wsSettingsFailSave");
  }
  async wsSettingsClearSaveRoutes(): Promise<void> {
    return todo("wsSettingsClearSaveRoutes");
  }
  async wsSettingsLogoState(): Promise<"fallback" | "image"> {
    return todo("wsSettingsLogoState");
  }
  async wsSettingsLogoSrc(): Promise<string | null> {
    return todo("wsSettingsLogoSrc");
  }
  async wsSettingsLogoAction(): Promise<"upload" | "edit" | "none"> {
    return todo("wsSettingsLogoAction");
  }
  async wsSettingsOpenLogoDialog(): Promise<void> {
    return todo("wsSettingsOpenLogoDialog");
  }
  async wsSettingsLogoDialogVisible(): Promise<boolean> {
    return todo("wsSettingsLogoDialogVisible");
  }
  async wsSettingsUploadLogo(_filePath: string): Promise<void> {
    return todo("wsSettingsUploadLogo");
  }
  async wsSettingsRemoveLogo(): Promise<void> {
    return todo("wsSettingsRemoveLogo");
  }
  async wsSettingsCloseLogoDialog(): Promise<void> {
    return todo("wsSettingsCloseLogoDialog");
  }
  async wsSettingsCopyUrl(): Promise<void> {
    return todo("wsSettingsCopyUrl");
  }
  async wsSettingsReadClipboard(): Promise<string> {
    return todo("wsSettingsReadClipboard");
  }
  async wsSettingsDeleteSectionVisible(): Promise<boolean> {
    return todo("wsSettingsDeleteSectionVisible");
  }
  async wsSettingsOpenDeleteDialog(): Promise<void> {
    return todo("wsSettingsOpenDeleteDialog");
  }
  async wsSettingsDeleteDialogVisible(): Promise<boolean> {
    return todo("wsSettingsDeleteDialogVisible");
  }
  async wsSettingsDeleteDialogText(): Promise<string> {
    return todo("wsSettingsDeleteDialogText");
  }
  async wsSettingsFillDeleteName(_name: string): Promise<void> {
    return todo("wsSettingsFillDeleteName");
  }
  async wsSettingsFillDeletePhrase(_text: string): Promise<void> {
    return todo("wsSettingsFillDeletePhrase");
  }
  async wsSettingsConfirmDeleteEnabled(): Promise<boolean> {
    return todo("wsSettingsConfirmDeleteEnabled");
  }
  async wsSettingsConfirmDelete(): Promise<void> {
    return todo("wsSettingsConfirmDelete");
  }
  async wsSettingsCancelDelete(): Promise<void> {
    return todo("wsSettingsCancelDelete");
  }

  // --- Workspace-settings developer oracle (NEWFRONT-412, WSET-016..024,
  // --- WSET-041). Throwing stubs per the shared driver contract; the new
  // --- app implements them later.
  async wsSettingsOpenIntegrations(_workspaceSlug: string): Promise<void> {
    return todo("wsSettingsOpenIntegrations");
  }
  async wsSettingsIntegrationsNotAuthorized(): Promise<boolean> {
    return todo("wsSettingsIntegrationsNotAuthorized");
  }
  async wsSettingsIntegrationsHeadingVisible(): Promise<boolean> {
    return todo("wsSettingsIntegrationsHeadingVisible");
  }
  async wsSettingsIntegrationsProviderTitles(): Promise<string[]> {
    return todo("wsSettingsIntegrationsProviderTitles");
  }
  async wsSettingsPageTitle(): Promise<string> {
    return todo("wsSettingsPageTitle");
  }
  async wsSettingsGithubStatusText(): Promise<string | null> {
    return todo("wsSettingsGithubStatusText");
  }
  async wsSettingsGithubTokenFill(_token: string): Promise<void> {
    return todo("wsSettingsGithubTokenFill");
  }
  async wsSettingsGithubTokenValue(): Promise<string> {
    return todo("wsSettingsGithubTokenValue");
  }
  async wsSettingsGithubConnectEnabled(): Promise<boolean> {
    return todo("wsSettingsGithubConnectEnabled");
  }
  async wsSettingsGithubConnect(): Promise<void> {
    return todo("wsSettingsGithubConnect");
  }
  async wsSettingsGithubConnected(): Promise<boolean> {
    return todo("wsSettingsGithubConnected");
  }
  async wsSettingsGithubDisconnectVisible(): Promise<boolean> {
    return todo("wsSettingsGithubDisconnectVisible");
  }
  async wsSettingsGithubDisconnect(): Promise<void> {
    return todo("wsSettingsGithubDisconnect");
  }
  async wsSettingsGitlabStatusText(): Promise<string | null> {
    return todo("wsSettingsGitlabStatusText");
  }
  async wsSettingsGitlabHostFill(_host: string): Promise<void> {
    return todo("wsSettingsGitlabHostFill");
  }
  async wsSettingsGitlabTokenFill(_token: string): Promise<void> {
    return todo("wsSettingsGitlabTokenFill");
  }
  async wsSettingsGitlabTokenValue(): Promise<string> {
    return todo("wsSettingsGitlabTokenValue");
  }
  async wsSettingsGitlabConnectEnabled(): Promise<boolean> {
    return todo("wsSettingsGitlabConnectEnabled");
  }
  async wsSettingsGitlabConnect(): Promise<void> {
    return todo("wsSettingsGitlabConnect");
  }
  async wsSettingsGitlabConnected(): Promise<boolean> {
    return todo("wsSettingsGitlabConnected");
  }
  async wsSettingsGitlabDisconnectVisible(): Promise<boolean> {
    return todo("wsSettingsGitlabDisconnectVisible");
  }
  async wsSettingsGitlabDisconnect(): Promise<void> {
    return todo("wsSettingsGitlabDisconnect");
  }
  async wsSettingsOpenWebhooks(_workspaceSlug: string): Promise<void> {
    return todo("wsSettingsOpenWebhooks");
  }
  async wsSettingsWebhooksNotAuthorized(): Promise<boolean> {
    return todo("wsSettingsWebhooksNotAuthorized");
  }
  async wsSettingsWebhooksHeadingVisible(): Promise<boolean> {
    return todo("wsSettingsWebhooksHeadingVisible");
  }
  async wsSettingsWebhooksAddVisible(): Promise<boolean> {
    return todo("wsSettingsWebhooksAddVisible");
  }
  async wsSettingsWebhooksEmptyVisible(): Promise<boolean> {
    return todo("wsSettingsWebhooksEmptyVisible");
  }
  async wsSettingsWebhooksLoaderVisible(): Promise<boolean> {
    return todo("wsSettingsWebhooksLoaderVisible");
  }
  async wsSettingsWebhooksDelayListFetch(_ms: number): Promise<void> {
    return todo("wsSettingsWebhooksDelayListFetch");
  }
  async wsSettingsWebhooksRowUrls(): Promise<string[]> {
    return todo("wsSettingsWebhooksRowUrls");
  }
  async wsSettingsWebhooksOpenRow(_url: string): Promise<void> {
    return todo("wsSettingsWebhooksOpenRow");
  }
  async wsSettingsWebhooksRowActiveOn(_url: string): Promise<boolean> {
    return todo("wsSettingsWebhooksRowActiveOn");
  }
  async wsSettingsWebhooksRowToggleActive(_url: string): Promise<void> {
    return todo("wsSettingsWebhooksRowToggleActive");
  }
  async wsSettingsWebhooksAddClick(): Promise<void> {
    return todo("wsSettingsWebhooksAddClick");
  }
  async wsSettingsWebhooksEmptyAddClick(): Promise<void> {
    return todo("wsSettingsWebhooksEmptyAddClick");
  }
  async wsSettingsWebhookDialogOpen(): Promise<boolean> {
    return todo("wsSettingsWebhookDialogOpen");
  }
  async wsSettingsWebhookDialogTitle(): Promise<string | null> {
    return todo("wsSettingsWebhookDialogTitle");
  }
  async wsSettingsWebhookUrlFill(_url: string): Promise<void> {
    return todo("wsSettingsWebhookUrlFill");
  }
  async wsSettingsWebhookUrlValue(): Promise<string> {
    return todo("wsSettingsWebhookUrlValue");
  }
  async wsSettingsWebhookUrlError(): Promise<string | null> {
    return todo("wsSettingsWebhookUrlError");
  }
  async wsSettingsWebhookEventMode(): Promise<"all" | "individual" | null> {
    return todo("wsSettingsWebhookEventMode");
  }
  async wsSettingsWebhookPickEventMode(_mode: "all" | "individual"): Promise<void> {
    return todo("wsSettingsWebhookPickEventMode");
  }
  async wsSettingsWebhookIndividualOptions(): Promise<{ label: string; checked: boolean }[]> {
    return todo("wsSettingsWebhookIndividualOptions");
  }
  async wsSettingsWebhookToggleIndividual(_label: string): Promise<void> {
    return todo("wsSettingsWebhookToggleIndividual");
  }
  async wsSettingsWebhookSubmit(): Promise<void> {
    return todo("wsSettingsWebhookSubmit");
  }
  async wsSettingsWebhookSubmitAndCaptureDownload(): Promise<LocalFileDownload> {
    return todo("wsSettingsWebhookSubmitAndCaptureDownload");
  }
  async wsSettingsWebhookCancel(): Promise<void> {
    return todo("wsSettingsWebhookCancel");
  }
  async wsSettingsWebhookPressEscape(): Promise<void> {
    return todo("wsSettingsWebhookPressEscape");
  }
  async wsSettingsWebhookRevealVisible(): Promise<boolean> {
    return todo("wsSettingsWebhookRevealVisible");
  }
  async wsSettingsWebhookRevealText(): Promise<string | null> {
    return todo("wsSettingsWebhookRevealText");
  }
  async wsSettingsWebhookSecretShown(): Promise<boolean> {
    return todo("wsSettingsWebhookSecretShown");
  }
  async wsSettingsWebhookSecretValue(): Promise<string | null> {
    return todo("wsSettingsWebhookSecretValue");
  }
  async wsSettingsWebhookSecretActionsVisible(): Promise<boolean> {
    return todo("wsSettingsWebhookSecretActionsVisible");
  }
  async wsSettingsWebhookSecretToggleView(): Promise<void> {
    return todo("wsSettingsWebhookSecretToggleView");
  }
  async wsSettingsWebhookSecretCopy(): Promise<void> {
    return todo("wsSettingsWebhookSecretCopy");
  }
  async wsSettingsWebhookRevealClose(): Promise<void> {
    return todo("wsSettingsWebhookRevealClose");
  }
  async wsSettingsWebhookDetailNotAuthorized(): Promise<boolean> {
    return todo("wsSettingsWebhookDetailNotAuthorized");
  }
  async wsSettingsWebhookDetailDenialKind(): Promise<"view" | "paragraph" | null> {
    return todo("wsSettingsWebhookDetailDenialKind");
  }
  async wsSettingsWebhookDetailLoaderVisible(): Promise<boolean> {
    return todo("wsSettingsWebhookDetailLoaderVisible");
  }
  async wsSettingsWebhookDetailDelayFetch(_ms: number): Promise<void> {
    return todo("wsSettingsWebhookDetailDelayFetch");
  }
  async wsSettingsWebhookDetailActiveOn(): Promise<boolean> {
    return todo("wsSettingsWebhookDetailActiveOn");
  }
  async wsSettingsWebhookDetailToggleActive(): Promise<void> {
    return todo("wsSettingsWebhookDetailToggleActive");
  }
  async wsSettingsWebhookDetailUpdate(): Promise<void> {
    return todo("wsSettingsWebhookDetailUpdate");
  }
  async wsSettingsWebhookDetailUpdateLabel(): Promise<string | null> {
    return todo("wsSettingsWebhookDetailUpdateLabel");
  }
  async wsSettingsWebhookDetailDelayUpdate(_ms: number): Promise<void> {
    return todo("wsSettingsWebhookDetailDelayUpdate");
  }
  async wsSettingsWebhookSecretBlockVisible(): Promise<boolean> {
    return todo("wsSettingsWebhookSecretBlockVisible");
  }
  async wsSettingsWebhookRegenerate(): Promise<void> {
    return todo("wsSettingsWebhookRegenerate");
  }
  async wsSettingsWebhookRegenerateLabel(): Promise<string | null> {
    return todo("wsSettingsWebhookRegenerateLabel");
  }
  async wsSettingsWebhookDelayRegenerate(_ms: number): Promise<void> {
    return todo("wsSettingsWebhookDelayRegenerate");
  }
  async wsSettingsWebhookRegenerateAndCaptureDownload(): Promise<LocalFileDownload | null> {
    return todo("wsSettingsWebhookRegenerateAndCaptureDownload");
  }
  async wsSettingsWebhookDeleteSectionVisible(): Promise<boolean> {
    return todo("wsSettingsWebhookDeleteSectionVisible");
  }
  async wsSettingsWebhookDeleteSectionExpand(): Promise<void> {
    return todo("wsSettingsWebhookDeleteSectionExpand");
  }
  async wsSettingsWebhookDeletePanelVisible(): Promise<boolean> {
    return todo("wsSettingsWebhookDeletePanelVisible");
  }
  async wsSettingsWebhookDeleteOpen(): Promise<void> {
    return todo("wsSettingsWebhookDeleteOpen");
  }
  async wsSettingsWebhookDeleteDialogOpen(): Promise<boolean> {
    return todo("wsSettingsWebhookDeleteDialogOpen");
  }
  async wsSettingsWebhookDeleteDialogText(): Promise<string | null> {
    return todo("wsSettingsWebhookDeleteDialogText");
  }
  async wsSettingsWebhookDeleteConfirm(): Promise<void> {
    return todo("wsSettingsWebhookDeleteConfirm");
  }
  async wsSettingsWebhookDeleteCancel(): Promise<void> {
    return todo("wsSettingsWebhookDeleteCancel");
  }
  // --- Profile-settings assistant/upkeep/integrations stubs (NEWFRONT-428,
  // --- PROF-021..025, PROF-027, PROF-028). The area gate implements these
  // --- against apps/web_new; until then every method throws todo().
  async settingsProfileOpenTab(
    _tab: "ai-assistant" | "auto-project-management" | "integrations",
    _cookies: ParityBrowserCookie[]
  ): Promise<void> {
    return todo("settingsProfileOpenTab");
  }
  async settingsProfileOpenTabWithQuery(
    _tab: "ai-assistant" | "auto-project-management" | "integrations",
    _query: string,
    _cookies: ParityBrowserCookie[]
  ): Promise<void> {
    return todo("settingsProfileOpenTabWithQuery");
  }
  async settingsProfileCurrentPath(): Promise<string> {
    return todo("settingsProfileCurrentPath");
  }
  async settingsProfileCurrentQuery(): Promise<string> {
    return todo("settingsProfileCurrentQuery");
  }
  async settingsProfileLastToast(): Promise<{ title: string; message: string } | null> {
    return todo("settingsProfileLastToast");
  }
  async settingsProfileAssistantFacts(): Promise<{
    provider: string;
    baseUrlVisible: boolean;
    baseUrl: string;
    model: string;
    keyValue: string;
    keyPlaceholder: string;
    saveDisabled: boolean;
    testDisabled: boolean;
    removeVisible: boolean;
    lastVerified: string | null;
  }> {
    return todo("settingsProfileAssistantFacts");
  }
  async settingsProfileAssistantSetProvider(_provider: "openai_compatible" | "anthropic"): Promise<void> {
    return todo("settingsProfileAssistantSetProvider");
  }
  async settingsProfileAssistantFill(_field: "baseUrl" | "model" | "key", _value: string): Promise<void> {
    return todo("settingsProfileAssistantFill");
  }
  async settingsProfileAssistantSave(): Promise<void> {
    return todo("settingsProfileAssistantSave");
  }
  async settingsProfileAssistantTest(): Promise<void> {
    return todo("settingsProfileAssistantTest");
  }
  async settingsProfileAssistantRemove(): Promise<void> {
    return todo("settingsProfileAssistantRemove");
  }
  async settingsProfileAssistantFailWrites(): Promise<void> {
    return todo("settingsProfileAssistantFailWrites");
  }
  async settingsProfileAssistantClearRoutes(): Promise<void> {
    return todo("settingsProfileAssistantClearRoutes");
  }
  async settingsProfileDictationFacts(): Promise<{
    sectionVisible: boolean;
    baseUrl: string;
    model: string;
    keyValue: string;
    keyPlaceholder: string;
    saveDisabled: boolean;
    testDisabled: boolean;
    removeVisible: boolean;
    lastVerified: string | null;
  }> {
    return todo("settingsProfileDictationFacts");
  }
  async settingsProfileDictationFill(_field: "baseUrl" | "model" | "key", _value: string): Promise<void> {
    return todo("settingsProfileDictationFill");
  }
  async settingsProfileDictationSave(): Promise<void> {
    return todo("settingsProfileDictationSave");
  }
  async settingsProfileDictationTest(): Promise<void> {
    return todo("settingsProfileDictationTest");
  }
  async settingsProfileDictationRemove(): Promise<void> {
    return todo("settingsProfileDictationRemove");
  }
  async settingsProfileToolServerRows(): Promise<
    { name: string; url: string; disabled: boolean; locked: boolean; prefix: string | null }[]
  > {
    return todo("settingsProfileToolServerRows");
  }
  async settingsProfileToolServerAddDisabled(): Promise<boolean> {
    return todo("settingsProfileToolServerAddDisabled");
  }
  async settingsProfileToolServerAdd(_name: string, _url: string, _authHeader?: string): Promise<void> {
    return todo("settingsProfileToolServerAdd");
  }
  async settingsProfileToolServerToggle(_name: string): Promise<void> {
    return todo("settingsProfileToolServerToggle");
  }
  async settingsProfileToolServerRemove(_name: string): Promise<void> {
    return todo("settingsProfileToolServerRemove");
  }
  async settingsProfileUpkeepFacts(): Promise<{
    emptyExplanation: string | null;
    masterPaused: boolean | null;
    jobs: { name: string; interval: string; description: string; enabled: boolean; toggleDisabled: boolean }[];
  }> {
    return todo("settingsProfileUpkeepFacts");
  }
  async settingsProfileUpkeepToggleMaster(_paused: boolean): Promise<void> {
    return todo("settingsProfileUpkeepToggleMaster");
  }
  async settingsProfileUpkeepToggleJob(_name: string, _enabled: boolean): Promise<void> {
    return todo("settingsProfileUpkeepToggleJob");
  }
  async settingsProfileUpkeepFailWrites(): Promise<void> {
    return todo("settingsProfileUpkeepFailWrites");
  }
  async settingsProfileUpkeepClearRoutes(): Promise<void> {
    return todo("settingsProfileUpkeepClearRoutes");
  }
  async settingsProfileUpkeepDelayWrites(_ms: number): Promise<void> {
    return todo("settingsProfileUpkeepDelayWrites");
  }
  async settingsProfileIntegrationsFacts(): Promise<{
    loading: boolean;
    noAdmin: boolean;
    configuredWarning: string | null;
    workspaceOptions: { slug: string; name: string }[];
    selectedSlug: string | null;
    panelHeading: string | null;
    panelLead: string | null;
    connected: boolean | null;
    installLabel: string | null;
    installDisabled: boolean | null;
    refreshVisible: boolean;
    statusFields: { label: string; value: string }[];
    unconnectedCopy: string | null;
    suspendedBanner: string | null;
    lastCheckError: string | null;
  }> {
    return todo("settingsProfileIntegrationsFacts");
  }
  async settingsProfileIntegrationsSelect(_slug: string): Promise<void> {
    return todo("settingsProfileIntegrationsSelect");
  }
  async settingsProfileIntegrationsRefresh(): Promise<void> {
    return todo("settingsProfileIntegrationsRefresh");
  }
  async settingsProfileIntegrationsFailRefresh(): Promise<void> {
    return todo("settingsProfileIntegrationsFailRefresh");
  }
  async settingsProfileIntegrationsClearRoutes(): Promise<void> {
    return todo("settingsProfileIntegrationsClearRoutes");
  }
  async settingsProfilePressKeys(_keys: string[]): Promise<void> {
    return todo("settingsProfilePressKeys");
  }
  async settingsProfileDialogOpen(): Promise<boolean> {
    return todo("settingsProfileDialogOpen");
  }
  async settingsProfileWatchReads(): Promise<void> {
    return todo("settingsProfileWatchReads");
  }
  async settingsProfileReadCount(): Promise<number> {
    return todo("settingsProfileReadCount");
  }
  async settingsProfileWatchSockets(): Promise<void> {
    return todo("settingsProfileWatchSockets");
  }
  async settingsProfileSocketCount(): Promise<number> {
    return todo("settingsProfileSocketCount");
  }
  async settingsProfileDraggableCount(): Promise<number> {
    return todo("settingsProfileDraggableCount");
  }
  async settingsProfileFileInputCount(): Promise<number> {
    return todo("settingsProfileFileInputCount");
  }
  async settingsProfileImportExportMentions(): Promise<string[]> {
    return todo("settingsProfileImportExportMentions");
  }
  async settingsProfileMemberControlCount(): Promise<number> {
    return todo("settingsProfileMemberControlCount");
  }

  // ---------------------------------------------------------------------------
  // Profile-settings shell, general/account tabs and security tab
  // (NEWFRONT-426, rows PROF-009..015, PROF-026). Stubs until the area lands
  // in apps/web_new. Appended; existing entries above are untouched per the
  // shared contract.
  // ---------------------------------------------------------------------------
  async settingsProfileOpen(_tabId: string, _cookies: ParityBrowserCookie[]): Promise<void> {
    return todo("settingsProfileOpen");
  }
  async settingsProfileShellVisible(): Promise<boolean> {
    return todo("settingsProfileShellVisible");
  }
  async settingsProfileLoaderOnly(): Promise<boolean> {
    return todo("settingsProfileLoaderOnly");
  }
  async settingsProfileGroupLabels(): Promise<string[]> {
    return todo("settingsProfileGroupLabels");
  }
  async settingsProfileGroupTabs(_group: string): Promise<string[]> {
    return todo("settingsProfileGroupTabs");
  }
  async settingsProfileActiveTab(): Promise<string | null> {
    return todo("settingsProfileActiveTab");
  }
  async settingsProfileBackLabel(): Promise<string | null> {
    return todo("settingsProfileBackLabel");
  }
  async settingsProfileSidebarTitle(): Promise<string | null> {
    return todo("settingsProfileSidebarTitle");
  }
  async settingsProfileWorkspaceShortcuts(): Promise<string[]> {
    return todo("settingsProfileWorkspaceShortcuts");
  }
  async settingsProfileSelectTab(_label: string): Promise<void> {
    return todo("settingsProfileSelectTab");
  }
  async settingsProfileOpenPath(_path: string, _cookies: ParityBrowserCookie[]): Promise<string> {
    return todo("settingsProfileOpenPath");
  }
  async settingsProfileGeneralFieldValue(_field: SettingsProfileGeneralField): Promise<string> {
    return todo("settingsProfileGeneralFieldValue");
  }
  async settingsProfileGeneralFillField(_field: SettingsProfileGeneralField, _value: string): Promise<void> {
    return todo("settingsProfileGeneralFillField");
  }
  async settingsProfileGeneralFieldError(_field: SettingsProfileGeneralField): Promise<string | null> {
    return todo("settingsProfileGeneralFieldError");
  }
  async settingsProfileGeneralEmailDisabled(): Promise<boolean> {
    return todo("settingsProfileGeneralEmailDisabled");
  }
  async settingsProfileGeneralSave(): Promise<void> {
    return todo("settingsProfileGeneralSave");
  }
  async settingsProfileGeneralSaveLabel(): Promise<string> {
    return todo("settingsProfileGeneralSaveLabel");
  }
  async settingsProfileGeneralIdentity(): Promise<{ name: string; email: string }> {
    return todo("settingsProfileGeneralIdentity");
  }
  async settingsProfileAvatarDialogOpen(): Promise<void> {
    return todo("settingsProfileAvatarDialogOpen");
  }
  async settingsProfileAvatarDialogVisible(): Promise<boolean> {
    return todo("settingsProfileAvatarDialogVisible");
  }
  async settingsProfileAvatarPick(_filePath: string): Promise<void> {
    return todo("settingsProfileAvatarPick");
  }
  async settingsProfileAvatarUpload(_filePath: string): Promise<void> {
    return todo("settingsProfileAvatarUpload");
  }
  async settingsProfileAvatarRemove(): Promise<void> {
    return todo("settingsProfileAvatarRemove");
  }
  async settingsProfileAvatarRemoveDisabled(): Promise<boolean> {
    return todo("settingsProfileAvatarRemoveDisabled");
  }
  async settingsProfileAvatarPhotoVisible(): Promise<boolean> {
    return todo("settingsProfileAvatarPhotoVisible");
  }
  async settingsProfileAvatarCancel(): Promise<void> {
    return todo("settingsProfileAvatarCancel");
  }
  async settingsProfileAvatarRejection(): Promise<string | null> {
    return todo("settingsProfileAvatarRejection");
  }
  async settingsProfileCoverControlVisible(): Promise<boolean> {
    return todo("settingsProfileCoverControlVisible");
  }
  async settingsProfileChangeEmailVisible(): Promise<boolean> {
    return todo("settingsProfileChangeEmailVisible");
  }
  async settingsProfileChangeEmailOpen(): Promise<void> {
    return todo("settingsProfileChangeEmailOpen");
  }
  async settingsProfileEmailModalStep(): Promise<"email" | "code" | "closed"> {
    return todo("settingsProfileEmailModalStep");
  }
  async settingsProfileEmailModalFillEmail(_email: string): Promise<void> {
    return todo("settingsProfileEmailModalFillEmail");
  }
  async settingsProfileEmailModalSubmit(): Promise<void> {
    return todo("settingsProfileEmailModalSubmit");
  }
  async settingsProfileEmailModalFillCode(_code: string): Promise<void> {
    return todo("settingsProfileEmailModalFillCode");
  }
  async settingsProfileEmailModalError(_field: "email" | "code"): Promise<string | null> {
    return todo("settingsProfileEmailModalError");
  }
  async settingsProfileEmailModalHelper(): Promise<string | null> {
    return todo("settingsProfileEmailModalHelper");
  }
  async settingsProfileEmailModalEmailDisabled(): Promise<boolean> {
    return todo("settingsProfileEmailModalEmailDisabled");
  }
  async settingsProfileEmailModalClose(): Promise<void> {
    return todo("settingsProfileEmailModalClose");
  }
  async settingsProfileDeactivateVisible(): Promise<boolean> {
    return todo("settingsProfileDeactivateVisible");
  }
  async settingsProfileDeactivateDescription(): Promise<string | null> {
    return todo("settingsProfileDeactivateDescription");
  }
  async settingsProfileDeactivateOpen(): Promise<void> {
    return todo("settingsProfileDeactivateOpen");
  }
  async settingsProfileDeactivateTitle(): Promise<string | null> {
    return todo("settingsProfileDeactivateTitle");
  }
  async settingsProfileDeactivateConfirm(): Promise<void> {
    return todo("settingsProfileDeactivateConfirm");
  }
  async settingsProfileDeactivateCancel(): Promise<void> {
    return todo("settingsProfileDeactivateCancel");
  }
  async settingsProfileDeactivateWorking(): Promise<boolean> {
    return todo("settingsProfileDeactivateWorking");
  }
  async settingsProfileSecurityFieldVisible(_field: SettingsProfileSecurityField): Promise<boolean> {
    return todo("settingsProfileSecurityFieldVisible");
  }
  async settingsProfileSecurityFill(_field: SettingsProfileSecurityField, _value: string): Promise<void> {
    return todo("settingsProfileSecurityFill");
  }
  async settingsProfileSecurityFieldType(_field: SettingsProfileSecurityField): Promise<string> {
    return todo("settingsProfileSecurityFieldType");
  }
  async settingsProfileSecurityToggleReveal(_field: SettingsProfileSecurityField): Promise<void> {
    return todo("settingsProfileSecurityToggleReveal");
  }
  async settingsProfileSecurityFieldError(_field: SettingsProfileSecurityField): Promise<string | null> {
    return todo("settingsProfileSecurityFieldError");
  }
  async settingsProfileSecurityMismatchVisible(): Promise<boolean> {
    return todo("settingsProfileSecurityMismatchVisible");
  }
  async settingsProfileSecuritySameAsOldVisible(): Promise<boolean> {
    return todo("settingsProfileSecuritySameAsOldVisible");
  }
  async settingsProfileSecurityStrengthVisible(): Promise<boolean> {
    return todo("settingsProfileSecurityStrengthVisible");
  }
  async settingsProfileSecuritySubmitDisabled(): Promise<boolean> {
    return todo("settingsProfileSecuritySubmitDisabled");
  }
  async settingsProfileSecuritySubmit(): Promise<void> {
    return todo("settingsProfileSecuritySubmit");
  }
  async settingsProfileSecurityFieldValue(_field: SettingsProfileSecurityField): Promise<string> {
    return todo("settingsProfileSecurityFieldValue");
  }
  async settingsProfileSecurityBlur(_field: SettingsProfileSecurityField): Promise<void> {
    return todo("settingsProfileSecurityBlur");
  }

  // ---------------------------------------------------------------------------
  // Member profile pages (NEWFRONT-425, PROF-001..008). Stubs; the area has
  // not landed in apps/web_new yet. Appended; existing stubs above are
  // untouched per the shared contract.
  // ---------------------------------------------------------------------------

  async profileOpen(_workspaceSlug: string, _userId: string, _cookies: ParityBrowserCookie[]): Promise<void> {
    return todo("profileOpen");
  }
  async profileBreadcrumbLabel(): Promise<string> {
    return todo("profileBreadcrumbLabel");
  }
  async profileVisibleTabs(): Promise<string[]> {
    return todo("profileVisibleTabs");
  }
  async profileTabHref(_tab: string): Promise<string | null> {
    return todo("profileTabHref");
  }
  async profileOpenTab(
    _workspaceSlug: string,
    _userId: string,
    _tab: string,
    _cookies: ParityBrowserCookie[]
  ): Promise<void> {
    return todo("profileOpenTab");
  }
  async profileAccessDenied(): Promise<boolean> {
    return todo("profileAccessDenied");
  }
  async profileMobileTabPickerLabel(): Promise<string | null> {
    return todo("profileMobileTabPickerLabel");
  }
  async profileMobileTabPickerOptions(): Promise<string[]> {
    return todo("profileMobileTabPickerOptions");
  }
  async profileOverviewCards(): Promise<Array<{ title: string; value: string; href: string | null }>> {
    return todo("profileOverviewCards");
  }
  async profileOverviewOpenCard(_route: string): Promise<void> {
    return todo("profileOverviewOpenCard");
  }
  async profileOverviewStatsLoading(): Promise<boolean> {
    return todo("profileOverviewStatsLoading");
  }
  async profileOverviewStallStats(): Promise<void> {
    return todo("profileOverviewStallStats");
  }
  async profileOverviewReleaseStats(): Promise<void> {
    return todo("profileOverviewReleaseStats");
  }
  async profileWorkloadGroups(): Promise<Array<{ label: string; count: string }>> {
    return todo("profileWorkloadGroups");
  }
  async profilePriorityChartCategories(): Promise<string[]> {
    return todo("profilePriorityChartCategories");
  }
  async profileStateChartLegend(): Promise<Array<{ label: string; count: string }>> {
    return todo("profileStateChartLegend");
  }
  async profileChartsEmptyCount(): Promise<number> {
    return todo("profileChartsEmptyCount");
  }
  async profileOverviewActivityEntries(): Promise<string[]> {
    return todo("profileOverviewActivityEntries");
  }
  async profileOverviewActivityEntryHrefs(): Promise<string[]> {
    return todo("profileOverviewActivityEntryHrefs");
  }
  async profileOverviewActivityEmpty(): Promise<boolean> {
    return todo("profileOverviewActivityEmpty");
  }
  async profileIssuesOfferedLayouts(): Promise<Array<"list" | "kanban">> {
    return todo("profileIssuesOfferedLayouts");
  }
  async profileMobileOfferedLayouts(): Promise<Array<"list" | "kanban">> {
    return todo("profileMobileOfferedLayouts");
  }
  async profileMobileSwitchTo(_layout: "list" | "kanban"): Promise<void> {
    return todo("profileMobileSwitchTo");
  }
  async profileMobileKanbanVisible(): Promise<boolean> {
    return todo("profileMobileKanbanVisible");
  }
  async profileMobileActiveLayout(): Promise<"list" | "kanban"> {
    return todo("profileMobileActiveLayout");
  }
  async profileMobileReloadIssues(): Promise<void> {
    return todo("profileMobileReloadIssues");
  }
  async profileTabRendersNothing(): Promise<boolean> {
    return todo("profileTabRendersNothing");
  }
  async profileActivityEntries(): Promise<string[]> {
    return todo("profileActivityEntries");
  }
  async profileActivityEntryHrefs(): Promise<string[]> {
    return todo("profileActivityEntryHrefs");
  }
  async profileActivityLoadMoreVisible(): Promise<boolean> {
    return todo("profileActivityLoadMoreVisible");
  }
  async profileActivityLoadMore(): Promise<void> {
    return todo("profileActivityLoadMore");
  }
  async profileSidebarVisible(): Promise<boolean> {
    return todo("profileSidebarVisible");
  }
  async profileSidebarCoverVisible(): Promise<boolean> {
    return todo("profileSidebarCoverVisible");
  }
  async profileSidebarAvatar(): Promise<{ kind: "image"; src: string } | { kind: "initial"; initial: string }> {
    return todo("profileSidebarAvatar");
  }
  async profileSidebarName(): Promise<{ full: string; display: string }> {
    return todo("profileSidebarName");
  }
  async profileSidebarJoinDate(): Promise<string> {
    return todo("profileSidebarJoinDate");
  }
  async profileSidebarClock(): Promise<string> {
    return todo("profileSidebarClock");
  }
  async profileSidebarProjectRows(): Promise<Array<{ name: string; completion: string | null }>> {
    return todo("profileSidebarProjectRows");
  }
  async profileSidebarExpandProject(_name: string): Promise<void> {
    return todo("profileSidebarExpandProject");
  }
  async profileSidebarProjectCounts(_name: string): Promise<{
    created: string;
    assigned: string;
    due: string;
    completed: string;
  }> {
    return todo("profileSidebarProjectCounts");
  }
  async profileSidebarEditPencilVisible(): Promise<boolean> {
    return todo("profileSidebarEditPencilVisible");
  }
  async profileSidebarClickEditPencil(): Promise<void> {
    return todo("profileSidebarClickEditPencil");
  }
  async profileSidebarToggle(): Promise<void> {
    return todo("profileSidebarToggle");
  }

  // --- Desktop updater web-observable sides (NEWFRONT-209, DESK-016–018).
  // --- Throwing stubs per the shared driver contract.
  async deskUpdateNativePromptVisible(): Promise<boolean> {
    return todo("deskUpdateNativePromptVisible");
  }
  async deskUpdateBeginTrafficWatch(): Promise<void> {
    return todo("deskUpdateBeginTrafficWatch");
  }
  async deskUpdateTrafficUrls(): Promise<string[]> {
    return todo("deskUpdateTrafficUrls");
  }
  async deskUpdateEndTrafficWatch(): Promise<void> {
    return todo("deskUpdateEndTrafficWatch");
  }
  async deskUpdateSidebarButtonVisible(): Promise<boolean> {
    return todo("deskUpdateSidebarButtonVisible");
  }

  // === NEWFRONT-97 space navigation (shared; keep earliest-merged on dup). ===
  async spaceOpenBoard(_anchor: string, _query?: string): Promise<void> {
    return todo("spaceOpenBoard");
  }

  async spaceOpenLanding(): Promise<void> {
    return todo("spaceOpenLanding");
  }

  async spaceOpenLegacyLink(_workspaceSlug: string, _projectId: string): Promise<void> {
    return todo("spaceOpenLegacyLink");
  }
  // --- Space layouts & cards oracle (NEWFRONT-449, SPC-020..033). ---
  async spaceBoardLayoutOptions(): Promise<string[]> {
    return todo("spaceBoardLayoutOptions");
  }
  async spaceBoardActiveLayout(): Promise<string> {
    return todo("spaceBoardActiveLayout");
  }
  async spaceBoardSwitchLayout(_layout: string): Promise<void> {
    return todo("spaceBoardSwitchLayout");
  }
  async spaceBoardSwitchLayoutRefetches(_layout: string): Promise<number> {
    return todo("spaceBoardSwitchLayoutRefetches");
  }
  async spaceBoardListGroups(): Promise<SpaceBoardGroupFacts[]> {
    return todo("spaceBoardListGroups");
  }
  async spaceBoardKanbanColumns(): Promise<SpaceBoardGroupFacts[]> {
    return todo("spaceBoardKanbanColumns");
  }
  async spaceBoardKanbanColumnsFlow(): Promise<"row" | "column" | "single"> {
    return todo("spaceBoardKanbanColumnsFlow");
  }
  async spaceBoardEmptyVisible(): Promise<boolean> {
    return todo("spaceBoardEmptyVisible");
  }
  async spaceBoardListGroupLoadMoreVisible(_groupName: string): Promise<boolean> {
    return todo("spaceBoardListGroupLoadMoreVisible");
  }
  async spaceBoardListGroupLoadMore(_groupName: string): Promise<void> {
    return todo("spaceBoardListGroupLoadMore");
  }
  async spaceBoardListGroupShowsLoader(_groupName: string): Promise<boolean> {
    return todo("spaceBoardListGroupShowsLoader");
  }
  async spaceBoardKanbanColumnScrollLoad(_columnName: string): Promise<void> {
    return todo("spaceBoardKanbanColumnScrollLoad");
  }
  async spaceBoardKanbanColumnShowsLoader(_columnName: string): Promise<boolean> {
    return todo("spaceBoardKanbanColumnShowsLoader");
  }
  async spaceBoardKanbanScrollBurstFetches(_columnName: string, _scrolls: number): Promise<number> {
    return todo("spaceBoardKanbanScrollBurstFetches");
  }
  async spaceBoardCardFacts(_issueName: string): Promise<SpaceBoardCardFacts> {
    return todo("spaceBoardCardFacts");
  }
  async spaceBoardCardMarkers(_issueName: string): Promise<SpaceBoardCardMarkers> {
    return todo("spaceBoardCardMarkers");
  }
  async spaceBoardKanbanCardFooter(_issueName: string): Promise<SpaceBoardCardFooter> {
    return todo("spaceBoardKanbanCardFooter");
  }
  async spaceBoardListCardFooterVisible(_issueName: string): Promise<boolean> {
    return todo("spaceBoardListCardFooterVisible");
  }
  async spaceBoardOpenCard(_issueName: string): Promise<void> {
    return todo("spaceBoardOpenCard");
  }
  async spaceBoardPeekVisible(): Promise<boolean> {
    return todo("spaceBoardPeekVisible");
  }
  async spaceBoardClosePeek(): Promise<void> {
    return todo("spaceBoardClosePeek");
  }
  async spaceBoardHighlightedRow(): Promise<string | null> {
    return todo("spaceBoardHighlightedRow");
  }
  async spaceBoardCardDragFacts(): Promise<SpaceBoardDragFacts> {
    return todo("spaceBoardCardDragFacts");
  }
  async spaceBoardDragCardOrder(_issueName: string): Promise<{ before: string[]; after: string[] }> {
    return todo("spaceBoardDragCardOrder");
  }
  async spaceBoardVisibleOrder(): Promise<string[]> {
    return todo("spaceBoardVisibleOrder");
  }
  async spaceBoardRefocusStability(): Promise<SpaceBoardStability> {
    return todo("spaceBoardRefocusStability");
  }
  async spaceBoardReconnectStability(): Promise<{ before: string[]; after: string[] }> {
    return todo("spaceBoardReconnectStability");
  }

  // --- Instance-admin entry/session oracle (NEWFRONT-441, ADM-001..012).
  async adminOpenRoot(): Promise<void> {
    return todo("adminOpenRoot");
  }
  async adminOpenPath(_path: string): Promise<void> {
    return todo("adminOpenPath");
  }
  async adminRootBranch(): Promise<AdminRootBranch> {
    return todo("adminRootBranch");
  }
  async adminWaitForRootBranch(_branch: AdminRootBranch): Promise<void> {
    return todo("adminWaitForRootBranch");
  }
  async adminCurrentPath(): Promise<string> {
    return todo("adminCurrentPath");
  }
  async adminCurrentSearch(): Promise<string> {
    return todo("adminCurrentSearch");
  }
  async adminHistoryLength(): Promise<number> {
    return todo("adminHistoryLength");
  }
  async adminStartInstanceFetchSpy(): Promise<void> {
    return todo("adminStartInstanceFetchSpy");
  }
  async adminInstanceFetchCount(): Promise<number> {
    return todo("adminInstanceFetchCount");
  }
  async adminFailInstanceFetch(): Promise<void> {
    return todo("adminFailInstanceFetch");
  }
  async adminDelayInstanceFetch(_ms: number): Promise<void> {
    return todo("adminDelayInstanceFetch");
  }
  async adminClearInstanceFetchFault(): Promise<void> {
    return todo("adminClearInstanceFetchFault");
  }
  async adminFailureFacts(): Promise<AdminFailureFacts> {
    return todo("adminFailureFacts");
  }
  async adminFailureRetry(): Promise<void> {
    return todo("adminFailureRetry");
  }
  async adminLoaderFacts(): Promise<{ visible: boolean; formVisible: boolean }> {
    return todo("adminLoaderFacts");
  }
  async adminSetupFacts(): Promise<AdminSetupFacts> {
    return todo("adminSetupFacts");
  }
  async adminSetupFill(_fields: AdminSetupFields): Promise<void> {
    return todo("adminSetupFill");
  }
  async adminSetupSetTelemetry(_on: boolean): Promise<void> {
    return todo("adminSetupSetTelemetry");
  }
  async adminSetupFocus(_field: "password" | "confirm"): Promise<void> {
    return todo("adminSetupFocus");
  }
  async adminSetupBlur(): Promise<void> {
    return todo("adminSetupBlur");
  }
  async adminSetupTogglePassword(_field: "password" | "confirm"): Promise<void> {
    return todo("adminSetupTogglePassword");
  }
  async adminSetupSubmit(): Promise<void> {
    return todo("adminSetupSubmit");
  }
  async adminSignInFacts(): Promise<AdminSignInFacts> {
    return todo("adminSignInFacts");
  }
  async adminSignInFill(_email: string, _password: string): Promise<void> {
    return todo("adminSignInFill");
  }
  async adminSignInTogglePassword(): Promise<void> {
    return todo("adminSignInTogglePassword");
  }
  async adminSignInSubmit(): Promise<void> {
    return todo("adminSignInSubmit");
  }
  async adminSignInDismissMappedBanner(): Promise<void> {
    return todo("adminSignInDismissMappedBanner");
  }
  async adminEnsureSignedIn(_email: string, _password: string): Promise<void> {
    return todo("adminEnsureSignedIn");
  }
  async adminDashboardVisible(): Promise<boolean> {
    return todo("adminDashboardVisible");
  }
  async adminOpenAccountMenu(): Promise<void> {
    return todo("adminOpenAccountMenu");
  }
  async adminAccountMenuFacts(): Promise<AdminAccountMenuFacts> {
    return todo("adminAccountMenuFacts");
  }
  async adminSidebarSignOut(): Promise<void> {
    return todo("adminSidebarSignOut");
  }
  async adminThemeFacts(): Promise<{ stored: string | null; resolved: "light" | "dark" }> {
    return todo("adminThemeFacts");
  }
  async adminSetTheme(_mode: "light" | "dark"): Promise<void> {
    return todo("adminSetTheme");
  }
  async adminNotFoundFacts(): Promise<AdminNotFoundFacts> {
    return todo("adminNotFoundFacts");
  }
  async adminNotFoundFollowAction(): Promise<void> {
    return todo("adminNotFoundFollowAction");
  }
  async adminFirstWorkspacePopupFacts(): Promise<{ visible: boolean; createHref: string | null }> {
    return todo("adminFirstWorkspacePopupFacts");
  }
  async adminFirstWorkspacePopupDismiss(): Promise<void> {
    return todo("adminFirstWorkspacePopupDismiss");
  }
  async adminFirstWorkspacePopupFollowCreate(): Promise<void> {
    return todo("adminFirstWorkspacePopupFollowCreate");
  }
  async adminSetupToggleTabIndex(_field: "password" | "confirm"): Promise<string | null> {
    return todo("adminSetupToggleTabIndex");
  }
  async adminSignInEmailFocused(): Promise<boolean> {
    return todo("adminSignInEmailFocused");
  }
  async adminDelaySessionFetch(_ms: number): Promise<void> {
    return todo("adminDelaySessionFetch");
  }
  async adminClearSessionFetchDelay(): Promise<void> {
    return todo("adminClearSessionFetchDelay");
  }
  async adminStubNoWorkspaces(): Promise<void> {
    return todo("adminStubNoWorkspaces");
  }
  // --- Instance-admin general/email oracle (NEWFRONT-442, ADM-013..022). ---
  async adminGeneralFacts(): Promise<AdminGeneralFacts> {
    return todo("adminGeneralFacts");
  }
  async adminGeneralFillName(_name: string): Promise<void> {
    return todo("adminGeneralFillName");
  }
  async adminGeneralSetTelemetry(_on: boolean): Promise<void> {
    return todo("adminGeneralSetTelemetry");
  }
  async adminGeneralSave(): Promise<void> {
    return todo("adminGeneralSave");
  }
  async adminStartInstancePatchSpy(): Promise<void> {
    return todo("adminStartInstancePatchSpy");
  }
  async adminInstancePatchBodies(): Promise<unknown[]> {
    return todo("adminInstancePatchBodies");
  }
  async adminFailInstancePatch(): Promise<void> {
    return todo("adminFailInstancePatch");
  }
  async adminDelayInstancePatch(_ms: number): Promise<void> {
    return todo("adminDelayInstancePatch");
  }
  async adminClearInstancePatchFault(): Promise<void> {
    return todo("adminClearInstancePatchFault");
  }
  async adminLastToast(): Promise<{ title: string; message: string } | null> {
    return todo("adminLastToast");
  }
  async adminStartConsoleSpy(): Promise<void> {
    return todo("adminStartConsoleSpy");
  }
  async adminConsoleErrors(): Promise<string[]> {
    return todo("adminConsoleErrors");
  }
  async adminEmailFacts(): Promise<AdminEmailFacts> {
    return todo("adminEmailFacts");
  }
  async adminEmailSetEnabled(_on: boolean): Promise<void> {
    return todo("adminEmailSetEnabled");
  }
  async adminEmailFormFacts(): Promise<AdminEmailFormFacts> {
    return todo("adminEmailFormFacts");
  }
  async adminEmailFill(_fields: AdminEmailFields): Promise<void> {
    return todo("adminEmailFill");
  }
  async adminEmailTogglePassword(): Promise<void> {
    return todo("adminEmailTogglePassword");
  }
  async adminEmailSetSecurity(_which: AdminEmailSecurity): Promise<void> {
    return todo("adminEmailSetSecurity");
  }
  async adminEmailSave(): Promise<void> {
    return todo("adminEmailSave");
  }
  async adminStartConfigPatchSpy(): Promise<void> {
    return todo("adminStartConfigPatchSpy");
  }
  async adminConfigPatchBodies(): Promise<unknown[]> {
    return todo("adminConfigPatchBodies");
  }
  async adminFailConfigPatch(): Promise<void> {
    return todo("adminFailConfigPatch");
  }
  async adminDelayConfigPatch(_ms: number): Promise<void> {
    return todo("adminDelayConfigPatch");
  }
  async adminClearConfigPatchFault(): Promise<void> {
    return todo("adminClearConfigPatchFault");
  }
  async adminDelayConfigFetch(_ms: number): Promise<void> {
    return todo("adminDelayConfigFetch");
  }
  async adminFailConfigFetch(): Promise<void> {
    return todo("adminFailConfigFetch");
  }
  async adminClearConfigFetchFault(): Promise<void> {
    return todo("adminClearConfigFetchFault");
  }
  async adminStartEmailDisableSpy(): Promise<void> {
    return todo("adminStartEmailDisableSpy");
  }
  async adminEmailDisableCount(): Promise<number> {
    return todo("adminEmailDisableCount");
  }
  async adminFailEmailDisable(): Promise<void> {
    return todo("adminFailEmailDisable");
  }
  async adminClearEmailDisableFault(): Promise<void> {
    return todo("adminClearEmailDisableFault");
  }
  async adminTestEmailOpen(): Promise<void> {
    return todo("adminTestEmailOpen");
  }
  async adminTestEmailFacts(): Promise<AdminTestEmailFacts> {
    return todo("adminTestEmailFacts");
  }
  async adminTestEmailFillReceiver(_email: string): Promise<void> {
    return todo("adminTestEmailFillReceiver");
  }
  async adminTestEmailSend(): Promise<void> {
    return todo("adminTestEmailSend");
  }
  async adminTestEmailClose(): Promise<void> {
    return todo("adminTestEmailClose");
  }
  async adminStartEmailCheckSpy(): Promise<void> {
    return todo("adminStartEmailCheckSpy");
  }
  async adminEmailCheckBodies(): Promise<unknown[]> {
    return todo("adminEmailCheckBodies");
  }
  // --- Admin authentication landing + provider sub-pages (NEWFRONT-443, ADM-023..036). ---
  async adminAuthLandingFacts(): Promise<AdminAuthLandingFacts> {
    return todo("adminAuthLandingFacts");
  }
  async adminAuthSetSignup(_on: boolean): Promise<void> {
    return todo("adminAuthSetSignup");
  }
  async adminAuthSetMethodEnabled(_name: string, _on: boolean): Promise<void> {
    return todo("adminAuthSetMethodEnabled");
  }
  async adminAuthMethodIconSrc(_name: string): Promise<string | null> {
    return todo("adminAuthMethodIconSrc");
  }
  async adminAuthProviderFacts(): Promise<AdminAuthProviderFacts> {
    return todo("adminAuthProviderFacts");
  }
  async adminAuthProviderSetEnabled(_on: boolean): Promise<void> {
    return todo("adminAuthProviderSetEnabled");
  }
  async adminAuthFormFacts(): Promise<AdminAuthFormFacts> {
    return todo("adminAuthFormFacts");
  }
  async adminAuthFormFill(_values: Record<string, string>): Promise<void> {
    return todo("adminAuthFormFill");
  }
  async adminAuthFormSetSync(_on: boolean): Promise<void> {
    return todo("adminAuthFormSetSync");
  }
  async adminAuthFormSave(): Promise<void> {
    return todo("adminAuthFormSave");
  }
  async adminAuthFormBack(): Promise<void> {
    return todo("adminAuthFormBack");
  }
  async adminAuthDiscardFacts(): Promise<AdminAuthDiscardFacts> {
    return todo("adminAuthDiscardFacts");
  }
  async adminAuthDiscardKeep(): Promise<void> {
    return todo("adminAuthDiscardKeep");
  }
  async adminAuthDiscardLeave(): Promise<void> {
    return todo("adminAuthDiscardLeave");
  }
  async adminAuthCopyFacts(): Promise<AdminAuthCopyFact[]> {
    return todo("adminAuthCopyFacts");
  }
  async adminAuthCopyField(_label: string): Promise<void> {
    return todo("adminAuthCopyField");
  }
  async adminAuthCopyHelpHref(_label: string): Promise<string | null> {
    return todo("adminAuthCopyHelpHref");
  }
  async adminAuthReadClipboard(): Promise<string> {
    return todo("adminAuthReadClipboard");
  }
  // --- Admin workspaces + AI + image oracle (NEWFRONT-444, ADM-037..050). ---
  async adminWorkspaceListFacts(): Promise<AdminWorkspaceListFacts> {
    return todo("adminWorkspaceListFacts");
  }
  async adminWorkspaceRows(): Promise<AdminWorkspaceRow[]> {
    return todo("adminWorkspaceRows");
  }
  async adminWorkspaceSetCreationRestricted(_on: boolean): Promise<void> {
    return todo("adminWorkspaceSetCreationRestricted");
  }
  async adminWorkspaceLoadMore(): Promise<void> {
    return todo("adminWorkspaceLoadMore");
  }
  async adminStartWorkspacesFetchSpy(): Promise<void> {
    return todo("adminStartWorkspacesFetchSpy");
  }
  async adminWorkspacesFetchUrls(): Promise<string[]> {
    return todo("adminWorkspacesFetchUrls");
  }
  async adminDelayWorkspacesFetch(_ms: number): Promise<void> {
    return todo("adminDelayWorkspacesFetch");
  }
  async adminFailWorkspacesFetch(): Promise<void> {
    return todo("adminFailWorkspacesFetch");
  }
  async adminStubEmptyWorkspaces(): Promise<void> {
    return todo("adminStubEmptyWorkspaces");
  }
  async adminClearWorkspacesFetchFault(): Promise<void> {
    return todo("adminClearWorkspacesFetchFault");
  }
  async adminWorkspaceCreateFacts(): Promise<AdminWorkspaceCreateFacts> {
    return todo("adminWorkspaceCreateFacts");
  }
  async adminWorkspaceCreateFillName(_name: string): Promise<void> {
    return todo("adminWorkspaceCreateFillName");
  }
  async adminWorkspaceCreateFillSlug(_slug: string): Promise<void> {
    return todo("adminWorkspaceCreateFillSlug");
  }
  async adminWorkspaceCreateSelectOrgSize(_size: string): Promise<void> {
    return todo("adminWorkspaceCreateSelectOrgSize");
  }
  async adminWorkspaceCreateSubmit(): Promise<void> {
    return todo("adminWorkspaceCreateSubmit");
  }
  async adminStartSlugCheckSpy(): Promise<void> {
    return todo("adminStartSlugCheckSpy");
  }
  async adminSlugCheckSlugs(): Promise<string[]> {
    return todo("adminSlugCheckSlugs");
  }
  async adminFailSlugCheck(): Promise<void> {
    return todo("adminFailSlugCheck");
  }
  async adminClearSlugCheckFault(): Promise<void> {
    return todo("adminClearSlugCheckFault");
  }
  async adminStartWorkspaceCreateSpy(): Promise<void> {
    return todo("adminStartWorkspaceCreateSpy");
  }
  async adminWorkspaceCreateBodies(): Promise<unknown[]> {
    return todo("adminWorkspaceCreateBodies");
  }
  async adminFailWorkspaceCreate(): Promise<void> {
    return todo("adminFailWorkspaceCreate");
  }
  async adminDelayWorkspaceCreate(_ms: number): Promise<void> {
    return todo("adminDelayWorkspaceCreate");
  }
  async adminClearWorkspaceCreateFault(): Promise<void> {
    return todo("adminClearWorkspaceCreateFault");
  }
  async adminAiFacts(): Promise<AdminAiFacts> {
    return todo("adminAiFacts");
  }
  async adminAiFill(_model: string, _key: string): Promise<void> {
    return todo("adminAiFill");
  }
  async adminAiToggleKeyVisibility(): Promise<void> {
    return todo("adminAiToggleKeyVisibility");
  }
  async adminAiSave(): Promise<void> {
    return todo("adminAiSave");
  }
  async adminImageFacts(): Promise<AdminImageFacts> {
    return todo("adminImageFacts");
  }

  // --- Admin loop jobs + runners (NEWFRONT-445, ADM-051..061 + ADM-080). ---
  async adminLoopListFacts(): Promise<AdminLoopListFacts> {
    return todo("adminLoopListFacts");
  }
  async adminLoopRows(): Promise<AdminLoopRow[]> {
    return todo("adminLoopRows");
  }
  async adminLoopSetEnabled(_slug: string, _on: boolean): Promise<void> {
    return todo("adminLoopSetEnabled");
  }
  async adminLoopOpenCreate(): Promise<void> {
    return todo("adminLoopOpenCreate");
  }
  async adminLoopModalFacts(): Promise<AdminLoopModalFacts> {
    return todo("adminLoopModalFacts");
  }
  async adminLoopModalFill(_fields: Partial<AdminLoopModalFields>): Promise<void> {
    return todo("adminLoopModalFill");
  }
  async adminLoopModalSave(): Promise<void> {
    return todo("adminLoopModalSave");
  }
  async adminLoopModalCancel(): Promise<void> {
    return todo("adminLoopModalCancel");
  }
  async adminLoopDetailFacts(): Promise<AdminLoopDetailFacts> {
    return todo("adminLoopDetailFacts");
  }
  async adminLoopDetailRows(): Promise<AdminLoopTargetRow[]> {
    return todo("adminLoopDetailRows");
  }
  async adminLoopSetSkipFilter(_reason: string): Promise<void> {
    return todo("adminLoopSetSkipFilter");
  }
  async adminLoopOpenEdit(): Promise<void> {
    return todo("adminLoopOpenEdit");
  }
  async adminLoopDelete(): Promise<void> {
    return todo("adminLoopDelete");
  }
  async adminStartLoopJobsSpy(): Promise<void> {
    return todo("adminStartLoopJobsSpy");
  }
  async adminLoopJobsUrls(): Promise<string[]> {
    return todo("adminLoopJobsUrls");
  }
  async adminDelayLoopJobsFetch(_ms: number): Promise<void> {
    return todo("adminDelayLoopJobsFetch");
  }
  async adminFailLoopJobsFetch(): Promise<void> {
    return todo("adminFailLoopJobsFetch");
  }
  async adminStubEmptyLoopJobs(): Promise<void> {
    return todo("adminStubEmptyLoopJobs");
  }
  async adminClearLoopJobsFetchFault(): Promise<void> {
    return todo("adminClearLoopJobsFetchFault");
  }
  async adminStartLoopJobSpy(): Promise<void> {
    return todo("adminStartLoopJobSpy");
  }
  async adminLoopJobUrls(): Promise<string[]> {
    return todo("adminLoopJobUrls");
  }
  async adminDelayLoopJobFetch(_ms: number): Promise<void> {
    return todo("adminDelayLoopJobFetch");
  }
  async adminFailLoopJobFetch(): Promise<void> {
    return todo("adminFailLoopJobFetch");
  }
  async adminClearLoopJobFetchFault(): Promise<void> {
    return todo("adminClearLoopJobFetchFault");
  }
  async adminStartLoopTargetsSpy(): Promise<void> {
    return todo("adminStartLoopTargetsSpy");
  }
  async adminLoopTargetsUrls(): Promise<string[]> {
    return todo("adminLoopTargetsUrls");
  }
  async adminStartLoopJobCreateSpy(): Promise<void> {
    return todo("adminStartLoopJobCreateSpy");
  }
  async adminLoopJobCreateBodies(): Promise<unknown[]> {
    return todo("adminLoopJobCreateBodies");
  }
  async adminFailLoopJobCreate(): Promise<void> {
    return todo("adminFailLoopJobCreate");
  }
  async adminClearLoopJobCreateFault(): Promise<void> {
    return todo("adminClearLoopJobCreateFault");
  }
  async adminStartLoopJobUpdateSpy(): Promise<void> {
    return todo("adminStartLoopJobUpdateSpy");
  }
  async adminLoopJobUpdateBodies(): Promise<unknown[]> {
    return todo("adminLoopJobUpdateBodies");
  }
  async adminFailLoopJobUpdate(): Promise<void> {
    return todo("adminFailLoopJobUpdate");
  }
  async adminDelayLoopJobUpdate(_ms: number): Promise<void> {
    return todo("adminDelayLoopJobUpdate");
  }
  async adminClearLoopJobUpdateFault(): Promise<void> {
    return todo("adminClearLoopJobUpdateFault");
  }
  async adminStartLoopJobDeleteSpy(): Promise<void> {
    return todo("adminStartLoopJobDeleteSpy");
  }
  async adminLoopJobDeleteCount(): Promise<number> {
    return todo("adminLoopJobDeleteCount");
  }
  async adminFailLoopJobDelete(): Promise<void> {
    return todo("adminFailLoopJobDelete");
  }
  async adminClearLoopJobDeleteFault(): Promise<void> {
    return todo("adminClearLoopJobDeleteFault");
  }
  async adminRunnersFacts(): Promise<AdminRunnersFacts> {
    return todo("adminRunnersFacts");
  }
  // --- Admin dashboard shell + negatives (NEWFRONT-446, ADM-062..073). ---
  async adminSidebarFacts(): Promise<AdminSidebarFacts> {
    return todo("adminSidebarFacts");
  }
  async adminSidebarFollowEntry(_name: string): Promise<void> {
    return todo("adminSidebarFollowEntry");
  }
  async adminSidebarEntryTooltip(_href: string, _name: string): Promise<string | null> {
    return todo("adminSidebarEntryTooltip");
  }
  async adminFooterTooltip(_control: AdminFooterControl, _label: string): Promise<string | null> {
    return todo("adminFooterTooltip");
  }
  async adminSidebarToggleCollapse(): Promise<void> {
    return todo("adminSidebarToggleCollapse");
  }
  async adminSidebarOutsideClick(): Promise<void> {
    return todo("adminSidebarOutsideClick");
  }
  async adminAccountFacts(): Promise<AdminAccountFacts> {
    return todo("adminAccountFacts");
  }
  async adminAccountOpen(): Promise<void> {
    return todo("adminAccountOpen");
  }
  async adminBreadcrumbFacts(): Promise<AdminBreadcrumbFacts> {
    return todo("adminBreadcrumbFacts");
  }
  async adminBreadcrumbTooltip(_label: string): Promise<string | null> {
    return todo("adminBreadcrumbTooltip");
  }
  async adminHelpFacts(): Promise<AdminHelpFacts> {
    return todo("adminHelpFacts");
  }
  async adminHelpOpen(): Promise<void> {
    return todo("adminHelpOpen");
  }
  async adminHamburgerFacts(): Promise<AdminHamburgerFacts> {
    return todo("adminHamburgerFacts");
  }
  async adminHamburgerToggle(): Promise<void> {
    return todo("adminHamburgerToggle");
  }
  async adminStartConfigFetchSpy(): Promise<void> {
    return todo("adminStartConfigFetchSpy");
  }
  async adminConfigFetchCount(): Promise<number> {
    return todo("adminConfigFetchCount");
  }
  async adminStartTransportSpy(): Promise<void> {
    return todo("adminStartTransportSpy");
  }
  async adminTransportFacts(): Promise<AdminTransportFacts> {
    return todo("adminTransportFacts");
  }
  async adminAbsenceFacts(): Promise<AdminAbsenceFacts> {
    return todo("adminAbsenceFacts");
  }
  async adminShortcutProbe(_keys: string[]): Promise<AdminShortcutProbeFacts> {
    return todo("adminShortcutProbe");
  }
  async adminLoopDeleteWatchDialogs(): Promise<AdminDeleteWatchFacts> {
    return todo("adminLoopDeleteWatchDialogs");
  }
  async adminWorkspaceDeleteFacts(): Promise<AdminWorkspaceDeleteFacts> {
    return todo("adminWorkspaceDeleteFacts");
  }
  // --- Admin cloud entry gate (NEWFRONT-446, ADM-074..079). ---
  async adminCloudHomeFacts(): Promise<AdminCloudHomeFacts> {
    return todo("adminCloudHomeFacts");
  }
  async adminCloudWaitForHomeBranch(_branch: AdminCloudHomeBranch): Promise<void> {
    return todo("adminCloudWaitForHomeBranch");
  }
  async adminCloudOpenRootWithCookies(_cookies: ParityBrowserCookie[]): Promise<void> {
    return todo("adminCloudOpenRootWithCookies");
  }
  async adminCloudFailAdminCheck(_times: number, _status: number): Promise<void> {
    return todo("adminCloudFailAdminCheck");
  }
  async adminCloudFailAuthProbe(_times: number, _status: number): Promise<void> {
    return todo("adminCloudFailAuthProbe");
  }
  async adminCloudDelayAdminCheck(_ms: number): Promise<void> {
    return todo("adminCloudDelayAdminCheck");
  }
  async adminCloudGateFetchCounts(): Promise<AdminCloudGateFetchCounts> {
    return todo("adminCloudGateFetchCounts");
  }
  async adminCloudStartGateFetchSpy(): Promise<void> {
    return todo("adminCloudStartGateFetchSpy");
  }
  async adminCloudRetry(): Promise<void> {
    return todo("adminCloudRetry");
  }
  // --- Space navbar & chrome (NEWFRONT-448, SPC-010..019 + SPC-065). ---
  async spaceChromeNavbarIdentity(): Promise<SpaceChromeNavbarIdentity> {
    return todo("spaceChromeNavbarIdentity");
  }
  async spaceChromeNavbarWithoutProjectDetails(_anchor: string): Promise<SpaceChromeNavbarIdentity> {
    return todo("spaceChromeNavbarWithoutProjectDetails");
  }
  async spaceChromeNavbarWithLogo(_anchor: string, _kind: "icon" | "emoji"): Promise<SpaceChromeNavbarIdentity> {
    return todo("spaceChromeNavbarWithLogo");
  }
  async spaceChromeTheme(): Promise<"light" | "dark"> {
    return todo("spaceChromeTheme");
  }
  async spaceChromeToggleTheme(): Promise<void> {
    return todo("spaceChromeToggleTheme");
  }
  async spaceChromeBodyBackground(): Promise<string> {
    return todo("spaceChromeBodyBackground");
  }
  async spaceChromeClearStoredTheme(): Promise<void> {
    return todo("spaceChromeClearStoredTheme");
  }
  async spaceChromeEmulateColorScheme(_scheme: "light" | "dark"): Promise<void> {
    return todo("spaceChromeEmulateColorScheme");
  }
  async spaceChromeBadgeFacts(): Promise<SpaceChromeBadgeFacts | null> {
    return todo("spaceChromeBadgeFacts");
  }
  async spaceChromeBadgeNewTabUrl(): Promise<string> {
    return todo("spaceChromeBadgeNewTabUrl");
  }
  async spaceChromeAnonymousSignInHref(): Promise<string | null> {
    return todo("spaceChromeAnonymousSignInHref");
  }
  async spaceChromeAccountMenuIdentity(): Promise<SpaceChromeAccountIdentity> {
    return todo("spaceChromeAccountMenuIdentity");
  }
  async spaceChromeSignOutForm(): Promise<SpaceChromeSignOutForm> {
    return todo("spaceChromeSignOutForm");
  }
  async spaceChromeSignOut(): Promise<SpaceChromeSignOutResult> {
    return todo("spaceChromeSignOut");
  }
  async spaceChromeOpenBoardFramed(_anchor: string, _query?: string): Promise<void> {
    return todo("spaceChromeOpenBoardFramed");
  }
  async spaceChromeFramedFacts(): Promise<SpaceChromeFramedFacts> {
    return todo("spaceChromeFramedFacts");
  }
  async spaceChromeOpenIssuePeek(_anchor: string, _issueId: string, _query?: string): Promise<void> {
    return todo("spaceChromeOpenIssuePeek");
  }
  async spaceChromeEmojiControlCount(): Promise<number> {
    return todo("spaceChromeEmojiControlCount");
  }
  async spaceChromeSpinnerDuring(
    _stage: "instance" | "settings" | "issues",
    _anchor: string
  ): Promise<SpaceChromeSpinnerProbe> {
    return todo("spaceChromeSpinnerDuring");
  }
  async spaceChromeProgressDuringSlowTransition(
    _workspaceSlug: string,
    _projectId: string
  ): Promise<SpaceChromeProgressProbe> {
    return todo("spaceChromeProgressDuringSlowTransition");
  }
  async spaceChromeCopyIssueLinkToast(_anchor: string, _issueId: string): Promise<SpaceChromeToastFacts> {
    return todo("spaceChromeCopyIssueLinkToast");
  }
  async spaceChromeCommentFailureToast(
    _anchor: string,
    _issueId: string,
    _bodyText: string
  ): Promise<SpaceChromeToastFacts> {
    return todo("spaceChromeCommentFailureToast");
  }
  async spaceChromeInstanceFailureFacts(_anchor: string): Promise<SpaceChromeFailureFacts> {
    return todo("spaceChromeInstanceFailureFacts");
  }
  async spaceChromeRetryInstanceLoad(): Promise<void> {
    return todo("spaceChromeRetryInstanceLoad");
  }
  async spaceChromeCrashFacts(_anchor: string): Promise<SpaceChromeCrashFacts> {
    return todo("spaceChromeCrashFacts");
  }
  async spaceChromeRefreshAfterCrash(): Promise<void> {
    return todo("spaceChromeRefreshAfterCrash");
  }
  async spaceLocaleBoardFacts(_anchor: string, _issueId: string): Promise<SpaceLocaleBoardFacts> {
    return todo("spaceLocaleBoardFacts");
  }
  async spaceLocaleLandingFacts(): Promise<SpaceLocaleLandingFacts> {
    return todo("spaceLocaleLandingFacts");
  }
  async spaceLocaleSetStoredLocale(_locale: string | null): Promise<void> {
    return todo("spaceLocaleSetStoredLocale");
  }

  // --- Space board access & routing (NEWFRONT-447, SPC-001..009). ---
  async spaceAccessOpenPath(_path: string): Promise<void> {
    return todo("spaceAccessOpenPath");
  }

  async spaceAccessOpenLandingAuthed(_cookies: ParityBrowserCookie[], _query?: string): Promise<void> {
    return todo("spaceAccessOpenLandingAuthed");
  }

  async spaceAccessBoardShowsProject(_name: string): Promise<boolean> {
    return todo("spaceAccessBoardShowsProject");
  }

  async spaceAccessBoardShowsIssue(_name: string): Promise<boolean> {
    return todo("spaceAccessBoardShowsIssue");
  }

  async spaceAccessSignInHref(): Promise<string | null> {
    return todo("spaceAccessSignInHref");
  }

  async spaceAccessAnchorNotFoundVisible(): Promise<boolean> {
    return todo("spaceAccessAnchorNotFoundVisible");
  }

  async spaceAccessBoardErrorVisible(): Promise<boolean> {
    return todo("spaceAccessBoardErrorVisible");
  }

  async spaceAccessGenericNotFoundVisible(): Promise<boolean> {
    return todo("spaceAccessGenericNotFoundVisible");
  }

  async spaceAccessFailBoardSettings(_status?: number): Promise<void> {
    return todo("spaceAccessFailBoardSettings");
  }

  async spaceAccessFailBoardIssues(_status?: number): Promise<void> {
    return todo("spaceAccessFailBoardIssues");
  }

  async spaceAccessClearFailures(): Promise<void> {
    return todo("spaceAccessClearFailures");
  }

  async spaceAccessLandingView(): Promise<"auth" | "signed-in" | "other"> {
    return todo("spaceAccessLandingView");
  }

  async spaceAccessHeadFacts(): Promise<SpaceAccessHeadFacts> {
    return todo("spaceAccessHeadFacts");
  }

  // --- Spaces auth (NEWFRONT-452, SPC-058..064). ---
  async spaceAuthOpenLanding(_params?: { email?: string; errorCode?: string; nextPath?: string }): Promise<void> {
    return todo("spaceAuthOpenLanding");
  }

  async spaceAuthStep(): Promise<"email" | "password" | "code" | "signed-in" | "unknown"> {
    return todo("spaceAuthStep");
  }

  async spaceAuthMode(): Promise<"sign-in" | "sign-up" | null> {
    return todo("spaceAuthMode");
  }

  async spaceAuthFillEmail(_email: string): Promise<void> {
    return todo("spaceAuthFillEmail");
  }

  async spaceAuthSubmitEmail(_email: string): Promise<void> {
    return todo("spaceAuthSubmitEmail");
  }

  async spaceAuthBlurEmail(): Promise<void> {
    return todo("spaceAuthBlurEmail");
  }

  async spaceAuthEmailError(): Promise<string | null> {
    return todo("spaceAuthEmailError");
  }

  async spaceAuthEmailSubmitEnabled(): Promise<boolean> {
    return todo("spaceAuthEmailSubmitEnabled");
  }

  async spaceAuthBanner(): Promise<string | null> {
    return todo("spaceAuthBanner");
  }

  async spaceAuthDismissBanner(): Promise<void> {
    return todo("spaceAuthDismissBanner");
  }

  async spaceAuthClearEmail(): Promise<void> {
    return todo("spaceAuthClearEmail");
  }

  async spaceAuthFillPassword(_password: string, _confirmPassword?: string): Promise<void> {
    return todo("spaceAuthFillPassword");
  }

  async spaceAuthSubmitPassword(_password: string, _confirmPassword?: string): Promise<void> {
    return todo("spaceAuthSubmitPassword");
  }

  async spaceAuthPasswordSubmitEnabled(): Promise<boolean> {
    return todo("spaceAuthPasswordSubmitEnabled");
  }

  async spaceAuthTogglePasswordVisibility(_field: "password" | "confirm"): Promise<void> {
    return todo("spaceAuthTogglePasswordVisibility");
  }

  async spaceAuthPasswordFieldTypes(): Promise<{ password: string | null; confirm: string | null }> {
    return todo("spaceAuthPasswordFieldTypes");
  }

  async spaceAuthStrengthMeterVisible(): Promise<boolean> {
    return todo("spaceAuthStrengthMeterVisible");
  }

  async spaceAuthStrengthMessage(): Promise<string | null> {
    return todo("spaceAuthStrengthMessage");
  }

  async spaceAuthPasswordMismatchError(): Promise<string | null> {
    return todo("spaceAuthPasswordMismatchError");
  }

  async spaceAuthPasswordPrimaryLabel(): Promise<string | null> {
    return todo("spaceAuthPasswordPrimaryLabel");
  }

  async spaceAuthSeesSwitchToCode(): Promise<boolean> {
    return todo("spaceAuthSeesSwitchToCode");
  }

  async spaceAuthSwitchToCode(): Promise<void> {
    return todo("spaceAuthSwitchToCode");
  }

  async spaceAuthSeesConfirmPassword(): Promise<boolean> {
    return todo("spaceAuthSeesConfirmPassword");
  }

  async spaceAuthSubmitCode(_code: string): Promise<void> {
    return todo("spaceAuthSubmitCode");
  }

  async spaceAuthCodeResendState(): Promise<{ disabled: boolean; label: string | null }> {
    return todo("spaceAuthCodeResendState");
  }

  async spaceAuthClickResendCode(): Promise<void> {
    return todo("spaceAuthClickResendCode");
  }

  async spaceAuthProviderButtons(): Promise<string[]> {
    return todo("spaceAuthProviderButtons");
  }

  async spaceAuthProvidersCompact(): Promise<boolean> {
    return todo("spaceAuthProvidersCompact");
  }

  async spaceAuthProvidersDividerVisible(): Promise<boolean> {
    return todo("spaceAuthProvidersDividerVisible");
  }

  async spaceAuthClickProvider(_name: string): Promise<void> {
    return todo("spaceAuthClickProvider");
  }

  async spaceAuthHeader(): Promise<{ heading: string | null; subline: string | null }> {
    return todo("spaceAuthHeader");
  }

  async spaceAuthConsent(): Promise<{ text: string | null; termsHref: string | null; privacyHref: string | null }> {
    return todo("spaceAuthConsent");
  }

  // --- Space filters + peek shell (NEWFRONT-450, SPC-034–046).
  // --- Throwing stubs per the shared driver contract.
  // --- Space filters (NEWFRONT-450, SPC-034–039). ---
  async spaceFiltersOpenPanel(): Promise<void> {
    return todo("spaceFiltersOpenPanel");
  }
  async spaceFiltersPanelOpen(): Promise<boolean> {
    return todo("spaceFiltersPanelOpen");
  }
  async spaceFiltersFacetNames(): Promise<string[]> {
    return todo("spaceFiltersFacetNames");
  }
  async spaceFiltersSearchPanel(_query: string): Promise<void> {
    return todo("spaceFiltersSearchPanel");
  }
  async spaceFiltersPriorityOptions(): Promise<Array<{ name: string; checked: boolean }>> {
    return todo("spaceFiltersPriorityOptions");
  }
  async spaceFiltersStateOptions(): Promise<Array<{ name: string; checked: boolean }>> {
    return todo("spaceFiltersStateOptions");
  }
  async spaceFiltersPickPriority(_name: string): Promise<void> {
    return todo("spaceFiltersPickPriority");
  }
  async spaceFiltersPickState(_name: string): Promise<void> {
    return todo("spaceFiltersPickState");
  }
  async spaceFiltersNoMatchesVisible(): Promise<boolean> {
    return todo("spaceFiltersNoMatchesVisible");
  }
  async spaceFiltersSearchBoxCount(): Promise<number> {
    return todo("spaceFiltersSearchBoxCount");
  }
  async spaceFiltersSwitchLayout(_layout: "list" | "kanban"): Promise<void> {
    return todo("spaceFiltersSwitchLayout");
  }
  async spaceFiltersAppliedFacets(): Promise<Array<{ facet: string; values: string[] }>> {
    return todo("spaceFiltersAppliedFacets");
  }
  async spaceFiltersAppliedStripVisible(): Promise<boolean> {
    return todo("spaceFiltersAppliedStripVisible");
  }
  async spaceFiltersRemoveValue(_facet: string, _value: string): Promise<void> {
    return todo("spaceFiltersRemoveValue");
  }
  async spaceFiltersClearFacet(_facet: string): Promise<void> {
    return todo("spaceFiltersClearFacet");
  }
  async spaceFiltersClearAll(): Promise<void> {
    return todo("spaceFiltersClearAll");
  }
  async spaceFiltersVisibleIssueNames(): Promise<string[]> {
    return todo("spaceFiltersVisibleIssueNames");
  }
  async spaceFiltersBeginFetchWatch(): Promise<void> {
    return todo("spaceFiltersBeginFetchWatch");
  }
  async spaceFiltersFetchQueries(): Promise<string[]> {
    return todo("spaceFiltersFetchQueries");
  }
  async spaceFiltersEndFetchWatch(): Promise<void> {
    return todo("spaceFiltersEndFetchWatch");
  }

  // --- Space peek shell (NEWFRONT-450, SPC-040–046). ---
  async spacePeekOpen(): Promise<boolean> {
    return todo("spacePeekOpen");
  }
  async spacePeekMode(): Promise<"side" | "modal" | "full"> {
    return todo("spacePeekMode");
  }
  async spacePeekSwitchMode(_mode: "side" | "modal" | "full"): Promise<void> {
    return todo("spacePeekSwitchMode");
  }
  async spacePeekClose(_via?: "header" | "backdrop" | "escape"): Promise<void> {
    return todo("spacePeekClose");
  }
  async spacePeekKey(): Promise<string | null> {
    return todo("spacePeekKey");
  }
  async spacePeekTitle(): Promise<string | null> {
    return todo("spacePeekTitle");
  }
  async spacePeekDescriptionVisible(): Promise<boolean> {
    return todo("spacePeekDescriptionVisible");
  }
  async spacePeekDescriptionText(): Promise<string | null> {
    return todo("spacePeekDescriptionText");
  }
  async spacePeekDescriptionHtml(): Promise<string | null> {
    return todo("spacePeekDescriptionHtml");
  }
  async spacePeekDescriptionEditable(): Promise<boolean> {
    return todo("spacePeekDescriptionEditable");
  }
  async spacePeekProperties(): Promise<{
    state: string;
    priority: string;
    dueDate: string | null;
    dueDateEmphasised: boolean;
  } | null> {
    return todo("spacePeekProperties");
  }
  async spacePeekActivityVisible(): Promise<boolean> {
    return todo("spacePeekActivityVisible");
  }
  async spacePeekCopyLinkVisible(): Promise<boolean> {
    return todo("spacePeekCopyLinkVisible");
  }
  async spacePeekCopyLink(): Promise<void> {
    return todo("spacePeekCopyLink");
  }
  async spacePeekReadClipboard(): Promise<string> {
    return todo("spacePeekReadClipboard");
  }
  async spacePeekLastNotice(): Promise<{ title: string; message: string } | null> {
    return todo("spacePeekLastNotice");
  }
  async spacePeekSkeletonsVisible(): Promise<boolean> {
    return todo("spacePeekSkeletonsVisible");
  }
  async deskShellNativeHttpPresent(): Promise<boolean> {
    return todo("deskShellNativeHttpPresent");
  }
  async deskShellBeginTrafficWatch(): Promise<void> {
    return todo("deskShellBeginTrafficWatch");
  }
  async deskShellTrafficUrls(): Promise<string[]> {
    return todo("deskShellTrafficUrls");
  }
  async deskShellEndTrafficWatch(): Promise<void> {
    return todo("deskShellEndTrafficWatch");
  }
  async deskShellAbortSemantics(): Promise<{ aborted: string; timedOut: string; unroutable: string }> {
    return todo("deskShellAbortSemantics");
  }
  async deskShellZoomMenuVisible(): Promise<boolean> {
    return todo("deskShellZoomMenuVisible");
  }
  async deskShellDeviceAuthHeading(): Promise<string | null> {
    return todo("deskShellDeviceAuthHeading");
  }
  async deskShellDeviceApproveEnabled(): Promise<boolean> {
    return todo("deskShellDeviceApproveEnabled");
  }
  async deskShellDeviceSubmitCode(_code: string): Promise<void> {
    return todo("deskShellDeviceSubmitCode");
  }
  async deskShellCloseReopenPath(): Promise<string> {
    return todo("deskShellCloseReopenPath");
  }

  // --- Space engagement: votes, reactions, comments (NEWFRONT-451, SPC-047–057).
  // --- Throwing stubs per the shared driver contract.
  async spaceCommentOpenIssue(_anchor: string, _issueName: string, _view?: string): Promise<void> {
    return todo("spaceCommentOpenIssue");
  }

  async spaceCommentAuthLanding(): Promise<SpaceAuthLanding> {
    return todo("spaceCommentAuthLanding");
  }

  async spaceVoteOverlayVisible(): Promise<boolean> {
    return todo("spaceVoteOverlayVisible");
  }

  async spaceVoteOverlay(): Promise<SpaceVoteState> {
    return todo("spaceVoteOverlay");
  }

  async spaceVoteClick(_direction: "up" | "down"): Promise<void> {
    return todo("spaceVoteClick");
  }

  async spaceVoteCardVisible(_anchor: string, _issueName: string): Promise<boolean> {
    return todo("spaceVoteCardVisible");
  }

  async spaceVoteCard(_anchor: string, _issueName: string): Promise<SpaceVoteState> {
    return todo("spaceVoteCard");
  }

  async spaceVoteCardClick(_anchor: string, _issueName: string, _direction: "up" | "down"): Promise<void> {
    return todo("spaceVoteCardClick");
  }

  async spaceReactOverlayVisible(): Promise<boolean> {
    return todo("spaceReactOverlayVisible");
  }

  async spaceReactGroups(): Promise<SpaceReactionGroup[]> {
    return todo("spaceReactGroups");
  }

  async spaceReactPick(): Promise<SpacePickedEmoji> {
    return todo("spaceReactPick");
  }

  async spaceReactClickGroup(_emoji: string): Promise<void> {
    return todo("spaceReactClickGroup");
  }

  async spaceReactAddVisible(): Promise<boolean> {
    return todo("spaceReactAddVisible");
  }

  async spaceCommentSection(): Promise<SpaceCommentSection> {
    return todo("spaceCommentSection");
  }

  async spaceCommentRows(): Promise<SpaceCommentRow[]> {
    return todo("spaceCommentRows");
  }

  async spaceCommentToggleFold(_index: number): Promise<void> {
    return todo("spaceCommentToggleFold");
  }

  async spaceCommentType(_text: string): Promise<void> {
    return todo("spaceCommentType");
  }

  async spaceCommentPasteHtml(_html: string): Promise<void> {
    return todo("spaceCommentPasteHtml");
  }

  async spaceCommentDraftText(): Promise<string> {
    return todo("spaceCommentDraftText");
  }

  async spaceCommentSubmit(): Promise<void> {
    return todo("spaceCommentSubmit");
  }

  async spaceCommentAttachImage(_filePath: string): Promise<void> {
    return todo("spaceCommentAttachImage");
  }

  async spaceCommentMention(_memberQuery: string, _memberDisplayName: string): Promise<SpaceMentionResult> {
    return todo("spaceCommentMention");
  }

  async spaceCommentMentionOpen(): Promise<boolean> {
    return todo("spaceCommentMentionOpen");
  }

  async spaceCommentMenuVisible(_index: number): Promise<boolean> {
    return todo("spaceCommentMenuVisible");
  }

  async spaceCommentMenuOpen(_index: number): Promise<void> {
    return todo("spaceCommentMenuOpen");
  }

  async spaceCommentMenuItems(_index: number): Promise<string[]> {
    return todo("spaceCommentMenuItems");
  }

  async spaceCommentInviteReturn(): Promise<{
    anchor: string | null;
    view: string | null;
    issueId: string | null;
  } | null> {
    return todo("spaceCommentInviteReturn");
  }

  async spaceCommentEdit(_index: number, _newText: string): Promise<void> {
    return todo("spaceCommentEdit");
  }

  async spaceCommentDelete(_index: number): Promise<void> {
    return todo("spaceCommentDelete");
  }

  async spaceCommentNotices(): Promise<SpaceCommentNotice[]> {
    return todo("spaceCommentNotices");
  }

  async spaceCommentReactions(_index: number): Promise<SpaceReactionGroup[]> {
    return todo("spaceCommentReactions");
  }

  async spaceCommentReactPick(_index: number): Promise<SpacePickedEmoji> {
    return todo("spaceCommentReactPick");
  }

  async spaceCommentReactClick(_index: number, _emoji: string): Promise<void> {
    return todo("spaceCommentReactClick");
  }

  async spaceCommentReactAddVisible(_index: number): Promise<boolean> {
    return todo("spaceCommentReactAddVisible");
  }

  async spaceCommentPeekMode(_mode: "side" | "modal" | "full"): Promise<void> {
    return todo("spaceCommentPeekMode");
  }

  async spaceCommentOpenEmbed(_anchor: string, _issueName: string): Promise<void> {
    return todo("spaceCommentOpenEmbed");
  }

  async spaceCommentEmbedSection(): Promise<SpaceCommentSection> {
    return todo("spaceCommentEmbedSection");
  }

  async spaceReactEmbedAddVisible(): Promise<boolean> {
    return todo("spaceReactEmbedAddVisible");
  }

  async spaceCommentEmbedMenuVisible(_index: number): Promise<boolean> {
    return todo("spaceCommentEmbedMenuVisible");
  }

  async spaceCommentEmbedReactions(_index: number): Promise<{ groups: SpaceReactionGroup[]; addVisible: boolean }> {
    return todo("spaceCommentEmbedReactions");
  }

  // --- Cloud marketing + public surface (NEWFRONT-437, CLOUD-003/004/006–010).
  // --- Stubs: the cloud marketing area has not landed in apps/web_new yet.

  async cloudHomeHeroActionHrefs(): Promise<string[]> {
    return todo("cloudHomeHeroActionHrefs");
  }

  async cloudHomeClickHeroAction(_href: string): Promise<void> {
    return todo("cloudHomeClickHeroAction");
  }

  async cloudHomeSections(): Promise<CloudHomeSectionFacts> {
    return todo("cloudHomeSections");
  }

  async cloudHeaderLinks(): Promise<CloudNavLinkFacts[]> {
    return todo("cloudHeaderLinks");
  }

  async cloudFooterLinks(): Promise<CloudNavLinkFacts[]> {
    return todo("cloudFooterLinks");
  }

  async cloudHeaderLinksFocusVisible(): Promise<boolean> {
    return todo("cloudHeaderLinksFocusVisible");
  }

  async cloudFooterLinksFocusVisible(): Promise<boolean> {
    return todo("cloudFooterLinksFocusVisible");
  }

  async cloudDocsFacts(): Promise<CloudDocsFacts> {
    return todo("cloudDocsFacts");
  }

  async cloudDownloadCards(): Promise<CloudDownloadCardFacts[]> {
    return todo("cloudDownloadCards");
  }

  async cloudDownloadsSigningNoticeVisible(): Promise<boolean> {
    return todo("cloudDownloadsSigningNoticeVisible");
  }

  async cloudCliCardFacts(): Promise<CloudCliCardFacts> {
    return todo("cloudCliCardFacts");
  }

  async cloudPricingTiers(): Promise<CloudPricingTierFacts[]> {
    return todo("cloudPricingTiers");
  }

  async cloudPricingEnterpriseHref(): Promise<string | null> {
    return todo("cloudPricingEnterpriseHref");
  }

  async cloudPricingFaqCount(): Promise<number> {
    return todo("cloudPricingFaqCount");
  }

  async cloudPricingFreeCapMentioned(): Promise<boolean> {
    return todo("cloudPricingFreeCapMentioned");
  }

  async cloudArmConfirm(_action: "accept" | "dismiss"): Promise<void> {
    return todo("cloudArmConfirm");
  }

  async cloudConfirmMessages(): Promise<string[]> {
    return todo("cloudConfirmMessages");
  }

  async cloudUpgradePopupUrl(): Promise<string> {
    return todo("cloudUpgradePopupUrl");
  }

  // --- Cloud login, SSO + session (NEWFRONT-438, CLOUD-011..020).
  // --- Stubs: the cloud login area has not landed in apps/web_new yet.

  async cloudLoginCardFacts(): Promise<CloudLoginCardFacts> {
    return todo("cloudLoginCardFacts");
  }

  async cloudArmSignInCapture(): Promise<void> {
    return todo("cloudArmSignInCapture");
  }

  async cloudSignInRequests(): Promise<string[]> {
    return todo("cloudSignInRequests");
  }

  async cloudAwaitSignInRequest(_previousCount: number): Promise<string> {
    return todo("cloudAwaitSignInRequest");
  }

  async cloudClickLoginContinue(): Promise<void> {
    return todo("cloudClickLoginContinue");
  }

  async cloudExplicitLogoutMarker(): Promise<boolean> {
    return todo("cloudExplicitLogoutMarker");
  }

  async cloudSuppressSilentSso(): Promise<void> {
    return todo("cloudSuppressSilentSso");
  }

  async cloudArmRefreshCounter(): Promise<void> {
    return todo("cloudArmRefreshCounter");
  }

  async cloudRefreshHits(): Promise<number> {
    return todo("cloudRefreshHits");
  }

  async cloudSignOutViaAccountMenu(): Promise<void> {
    return todo("cloudSignOutViaAccountMenu");
  }

  async cloudFailRefresh(): Promise<void> {
    return todo("cloudFailRefresh");
  }

  async cloudArmConsoleErrors(): Promise<void> {
    return todo("cloudArmConsoleErrors");
  }

  async cloudConsoleErrors(): Promise<string[]> {
    return todo("cloudConsoleErrors");
  }

  async cloudStubTauri(): Promise<void> {
    return todo("cloudStubTauri");
  }

  async cloudProfilePasswordFieldCount(): Promise<number> {
    return todo("cloudProfilePasswordFieldCount");
  }

  async cloudExpireAccessCookie(): Promise<void> {
    return todo("cloudExpireAccessCookie");
  }

  async cloudCookieNames(): Promise<string[]> {
    return todo("cloudCookieNames");
  }

  async cloudProviderButtonFacts(): Promise<CloudProviderButtonFacts> {
    return todo("cloudProviderButtonFacts");
  }

  async cloudClickProviderButton(): Promise<void> {
    return todo("cloudClickProviderButton");
  }

  // --- Page-export dialog oracle reads (NEWFRONT-383, EXP-016..021 + 023/024).
  // --- Appended; existing stubs above are untouched per the shared driver
  // --- contract.
  async pageExportHeading(): Promise<string | null> {
    return todo("pageExportHeading");
  }

  async pageExportOfferedOptions(_row: PageExportRow): Promise<string[]> {
    return todo("pageExportOfferedOptions");
  }

  async pageExportConfirmFull(): Promise<PageExportFile> {
    return todo("pageExportConfirmFull");
  }

  async pageExportFailSave(): Promise<void> {
    return todo("pageExportFailSave");
  }

  async pageExportFocusedFacts(): Promise<PageExportFocusFacts | null> {
    return todo("pageExportFocusedFacts");
  }

  async pageExportGateMarkersPresent(): Promise<boolean> {
    return todo("pageExportGateMarkersPresent");
  }

  // --- Cloud Apps catalog (NEWFRONT-439, CLOUD-021–031). Appended;
  // --- existing stubs above are untouched per the shared driver contract.
  async cloudAppsSidebarHref(): Promise<string | null> {
    return todo("cloudAppsSidebarHref");
  }

  async cloudAppsSidebarIconPresent(): Promise<boolean> {
    return todo("cloudAppsSidebarIconPresent");
  }

  async cloudAppsOpenFromSidebar(): Promise<void> {
    return todo("cloudAppsOpenFromSidebar");
  }

  async cloudAppsIntroText(): Promise<string> {
    return todo("cloudAppsIntroText");
  }

  async cloudAppsSearchFill(_value: string): Promise<void> {
    return todo("cloudAppsSearchFill");
  }

  async cloudAppsSearchTypeText(_text: string): Promise<void> {
    return todo("cloudAppsSearchTypeText");
  }

  async cloudAppsSearchValue(): Promise<string> {
    return todo("cloudAppsSearchValue");
  }

  async cloudAppsSearchVisible(): Promise<boolean> {
    return todo("cloudAppsSearchVisible");
  }

  async cloudAppsCardNames(): Promise<string[]> {
    return todo("cloudAppsCardNames");
  }

  async cloudAppsCards(): Promise<CloudAppsCardFacts[]> {
    return todo("cloudAppsCards");
  }

  async cloudAppsClickCardAction(_name: string, _action: "Install" | "Connect" | "Uninstall"): Promise<void> {
    return todo("cloudAppsClickCardAction");
  }

  async cloudAppsCardActionDisabled(_name: string, _action: "Install" | "Connect" | "Uninstall"): Promise<boolean> {
    return todo("cloudAppsCardActionDisabled");
  }

  async cloudAppsNextPage(): Promise<void> {
    return todo("cloudAppsNextPage");
  }

  async cloudAppsPrevPage(): Promise<void> {
    return todo("cloudAppsPrevPage");
  }

  async cloudAppsPageIndicator(): Promise<string | null> {
    return todo("cloudAppsPageIndicator");
  }

  async cloudAppsPagerVisible(): Promise<boolean> {
    return todo("cloudAppsPagerVisible");
  }

  async cloudAppsDialogVisible(): Promise<boolean> {
    return todo("cloudAppsDialogVisible");
  }

  async cloudAppsDialogTitle(): Promise<string | null> {
    return todo("cloudAppsDialogTitle");
  }

  async cloudAppsDialogFields(): Promise<CloudAppsDialogFieldFacts[]> {
    return todo("cloudAppsDialogFields");
  }

  async cloudAppsDialogInstructions(): Promise<string | null> {
    return todo("cloudAppsDialogInstructions");
  }

  async cloudAppsDialogSetupHref(): Promise<string | null> {
    return todo("cloudAppsDialogSetupHref");
  }

  async cloudAppsDialogFill(_label: string, _value: string): Promise<void> {
    return todo("cloudAppsDialogFill");
  }

  async cloudAppsDialogSubmit(): Promise<void> {
    return todo("cloudAppsDialogSubmit");
  }

  async cloudAppsDialogCancel(): Promise<void> {
    return todo("cloudAppsDialogCancel");
  }

  async cloudAppsDialogErrors(): Promise<string[]> {
    return todo("cloudAppsDialogErrors");
  }

  async cloudAppsToggleState(): Promise<boolean | null> {
    return todo("cloudAppsToggleState");
  }

  async cloudAppsToggleText(): Promise<string> {
    return todo("cloudAppsToggleText");
  }

  async cloudAppsToggle(): Promise<void> {
    return todo("cloudAppsToggle");
  }

  async cloudAppsBodyText(): Promise<string> {
    return todo("cloudAppsBodyText");
  }

  async cloudAppsCatalogVisible(): Promise<boolean> {
    return todo("cloudAppsCatalogVisible");
  }

  async cloudAppsClickRetry(): Promise<void> {
    return todo("cloudAppsClickRetry");
  }

  async cloudAppsSidebarCreateOpensDialog(): Promise<boolean> {
    return todo("cloudAppsSidebarCreateOpensDialog");
  }

  // --- Cloud profile tabs + misc (NEWFRONT-440, CLOUD-032..040).
  // --- Stubs: the cloud profile area has not landed in apps/web_new yet.

  async cloudAssistantConnections(): Promise<CloudAssistantConnectionFacts[]> {
    return todo("cloudAssistantConnections");
  }

  async cloudAssistantEmptyState(): Promise<{ visible: boolean; addLabel: string | null }> {
    return todo("cloudAssistantEmptyState");
  }

  async cloudAssistantView(): Promise<"list" | "add"> {
    return todo("cloudAssistantView");
  }

  async cloudAssistantClickAdd(): Promise<void> {
    return todo("cloudAssistantClickAdd");
  }

  async cloudAssistantClickBackToList(): Promise<void> {
    return todo("cloudAssistantClickBackToList");
  }

  async cloudAssistantSelectConnection(_title: string): Promise<void> {
    return todo("cloudAssistantSelectConnection");
  }

  async cloudAssistantSetModel(_title: string, _model: string): Promise<void> {
    return todo("cloudAssistantSetModel");
  }

  async cloudAssistantClickRowAction(_title: string, _action: "Test" | "Remove" | "Disconnect"): Promise<void> {
    return todo("cloudAssistantClickRowAction");
  }

  async cloudAssistantAddFormFacts(): Promise<CloudAssistantAddFormFacts> {
    return todo("cloudAssistantAddFormFacts");
  }

  async cloudAssistantAddFill(_fill: CloudAssistantAddFill): Promise<void> {
    return todo("cloudAssistantAddFill");
  }

  async cloudAssistantAddSave(): Promise<void> {
    return todo("cloudAssistantAddSave");
  }

  async cloudAssistantClickProviderSignIn(_title: string): Promise<void> {
    return todo("cloudAssistantClickProviderSignIn");
  }

  async cloudAssistantProviderPopupUrl(): Promise<string> {
    return todo("cloudAssistantProviderPopupUrl");
  }

  async cloudAssistantArmStatusCounter(): Promise<void> {
    return todo("cloudAssistantArmStatusCounter");
  }

  async cloudAssistantStatusHits(): Promise<number> {
    return todo("cloudAssistantStatusHits");
  }

  async cloudAssistantBlockPopups(): Promise<void> {
    return todo("cloudAssistantBlockPopups");
  }

  async cloudAssistantStaleNotice(_title: string): Promise<string | null> {
    return todo("cloudAssistantStaleNotice");
  }

  async cloudProfileSidebarTabs(): Promise<string[]> {
    return todo("cloudProfileSidebarTabs");
  }

  async cloudProfileActiveTab(): Promise<string | null> {
    return todo("cloudProfileActiveTab");
  }

  async cloudAssistantMcpSectionVisible(): Promise<boolean> {
    return todo("cloudAssistantMcpSectionVisible");
  }

  async cloudAssistantAppsPointerVisible(): Promise<boolean> {
    return todo("cloudAssistantAppsPointerVisible");
  }

  async cloudPlanBadgeVisible(): Promise<boolean> {
    return todo("cloudPlanBadgeVisible");
  }

  async cloudFocusedControl(): Promise<{ role: string; label: string } | null> {
    return todo("cloudFocusedControl");
  }

  // --- Desktop sign-in halves (NEWFRONT-486, AUTH-022/AUTH-023). Stubs:
  // --- the area gate implements them against apps/web_new.
  async authDesktopUnavailableCardVisible(): Promise<boolean> {
    // Desktop-build only: the entry routes render the desktop screen
    // instead of the email card. The client renders after boot, so settle
    // briefly instead of reading the blank document.
    const card = this.page.getByRole("heading", { name: "The desktop app can't sign you in yet" });
    try {
      await card.first().waitFor({ timeout: 10_000 });
    } catch {
      return false;
    }
    return card.first().isVisible();
  }

  async authDesktopBrowserActionLabel(): Promise<string | null> {
    const action = this.page.getByRole("button", { name: "Continue in the browser" });
    try {
      await action.first().waitFor({ timeout: 10_000 });
    } catch {
      return null;
    }
    const label = ((await action.first().textContent()) ?? "").trim();
    return label === "" ? null : label;
  }

  async authDesktopClickBrowserAction(): Promise<void> {
    await this.page.getByRole("button", { name: "Continue in the browser" }).click();
  }

  async authDesktopFailureAlertText(): Promise<string | null> {
    const alert = this.page.getByRole("alert");
    try {
      await alert.first().waitFor({ timeout: 10_000 });
    } catch {
      return null;
    }
    return ((await alert.first().innerText()) ?? "").trim() || null;
  }

  // --- Personal access tokens oracle (NEWFRONT-413, WSET-025..028).
  // --- Appended; existing methods above are untouched per the shared
  // --- driver contract.
  async wsSettingsApiTokenOpenTokens(_cookies: ParityBrowserCookie[]): Promise<void> {
    return todo("wsSettingsApiTokenOpenTokens");
  }
  async wsSettingsApiTokenOpenViaSidebar(_cookies: ParityBrowserCookie[]): Promise<void> {
    return todo("wsSettingsApiTokenOpenViaSidebar");
  }
  async wsSettingsApiTokenHeading(): Promise<string | null> {
    return todo("wsSettingsApiTokenHeading");
  }
  async wsSettingsApiTokenAddVisible(): Promise<boolean> {
    return todo("wsSettingsApiTokenAddVisible");
  }
  async wsSettingsApiTokenLoaderSeen(): Promise<boolean> {
    return todo("wsSettingsApiTokenLoaderSeen");
  }
  async wsSettingsApiTokenEmptyVisible(): Promise<boolean> {
    return todo("wsSettingsApiTokenEmptyVisible");
  }
  async wsSettingsApiTokenEmptyCreate(): Promise<void> {
    return todo("wsSettingsApiTokenEmptyCreate");
  }
  async wsSettingsApiTokenOpenCreate(): Promise<void> {
    return todo("wsSettingsApiTokenOpenCreate");
  }
  async wsSettingsApiTokenRows(): Promise<WsSettingsApiTokenRow[]> {
    return todo("wsSettingsApiTokenRows");
  }
  async wsSettingsApiTokenPageText(): Promise<string> {
    return todo("wsSettingsApiTokenPageText");
  }
  async wsSettingsApiTokenDialogOpen(): Promise<boolean> {
    return todo("wsSettingsApiTokenDialogOpen");
  }
  async wsSettingsApiTokenFillTitle(_title: string): Promise<void> {
    return todo("wsSettingsApiTokenFillTitle");
  }
  async wsSettingsApiTokenFillDescription(_text: string): Promise<void> {
    return todo("wsSettingsApiTokenFillDescription");
  }
  async wsSettingsApiTokenTitleError(): Promise<string | null> {
    return todo("wsSettingsApiTokenTitleError");
  }
  async wsSettingsApiTokenPresetOptions(): Promise<string[]> {
    return todo("wsSettingsApiTokenPresetOptions");
  }
  async wsSettingsApiTokenPickPreset(_label: string): Promise<void> {
    return todo("wsSettingsApiTokenPickPreset");
  }
  async wsSettingsApiTokenExpiryTriggerText(): Promise<string> {
    return todo("wsSettingsApiTokenExpiryTriggerText");
  }
  async wsSettingsApiTokenExpiryTriggerDisabled(): Promise<boolean> {
    return todo("wsSettingsApiTokenExpiryTriggerDisabled");
  }
  async wsSettingsApiTokenExpiryPreview(): Promise<string | null> {
    return todo("wsSettingsApiTokenExpiryPreview");
  }
  async wsSettingsApiTokenToggleNeverExpires(): Promise<void> {
    return todo("wsSettingsApiTokenToggleNeverExpires");
  }
  async wsSettingsApiTokenPickCustomDayNextMonth(_day: string): Promise<void> {
    return todo("wsSettingsApiTokenPickCustomDayNextMonth");
  }
  async wsSettingsApiTokenCustomFloor(): Promise<{ todayDisabled: boolean; tomorrowEnabled: boolean }> {
    return todo("wsSettingsApiTokenCustomFloor");
  }
  async wsSettingsApiTokenSubmit(): Promise<void> {
    return todo("wsSettingsApiTokenSubmit");
  }
  async wsSettingsApiTokenSubmitAndCaptureBackup(): Promise<WsSettingsApiTokenBackup> {
    return todo("wsSettingsApiTokenSubmitAndCaptureBackup");
  }
  async wsSettingsApiTokenCancelCreate(): Promise<void> {
    return todo("wsSettingsApiTokenCancelCreate");
  }
  async wsSettingsApiTokenRevealHeading(): Promise<string | null> {
    return todo("wsSettingsApiTokenRevealHeading");
  }
  async wsSettingsApiTokenRevealValue(): Promise<string | null> {
    return todo("wsSettingsApiTokenRevealValue");
  }
  async wsSettingsApiTokenRevealExpiryLine(): Promise<string | null> {
    return todo("wsSettingsApiTokenRevealExpiryLine");
  }
  async wsSettingsApiTokenRevealCopy(): Promise<string> {
    return todo("wsSettingsApiTokenRevealCopy");
  }
  async wsSettingsApiTokenRevealClose(): Promise<void> {
    return todo("wsSettingsApiTokenRevealClose");
  }
  async wsSettingsApiTokenDeleteDialogText(_label: string): Promise<string> {
    return todo("wsSettingsApiTokenDeleteDialogText");
  }
  async wsSettingsApiTokenDeleteCancel(): Promise<void> {
    return todo("wsSettingsApiTokenDeleteCancel");
  }
  async wsSettingsApiTokenDeleteConfirm(_label: string): Promise<void> {
    return todo("wsSettingsApiTokenDeleteConfirm");
  }

  // --- Project settings General tab (NEWFRONT-389, PSET-005–012). Throwing
  // --- stubs per the shared driver contract; the new app implements them later.
  async settingsGeneralOpen(_workspaceSlug: string, _projectId: string): Promise<void> {
    return todo("settingsGeneralOpen");
  }
  async settingsGeneralFormVisible(): Promise<boolean> {
    return todo("settingsGeneralFormVisible");
  }
  async settingsGeneralSkeletonVisible(): Promise<boolean> {
    return todo("settingsGeneralSkeletonVisible");
  }
  async settingsGeneralDangerVisible(): Promise<boolean> {
    return todo("settingsGeneralDangerVisible");
  }
  async settingsGeneralFormDimmed(): Promise<boolean> {
    return todo("settingsGeneralFormDimmed");
  }
  async settingsGeneralFieldValue(_field: SettingsGeneralField): Promise<string> {
    return todo("settingsGeneralFieldValue");
  }
  async settingsGeneralFieldDisabled(_field: SettingsGeneralField): Promise<boolean> {
    return todo("settingsGeneralFieldDisabled");
  }
  async settingsGeneralFieldError(_field: SettingsGeneralField): Promise<string> {
    return todo("settingsGeneralFieldError");
  }
  async settingsGeneralFill(_field: SettingsGeneralField, _value: string): Promise<void> {
    return todo("settingsGeneralFill");
  }
  async settingsGeneralCoverVisible(): Promise<boolean> {
    return todo("settingsGeneralCoverVisible");
  }
  async settingsGeneralHeaderText(): Promise<string> {
    return todo("settingsGeneralHeaderText");
  }
  async settingsGeneralNetworkText(): Promise<string> {
    return todo("settingsGeneralNetworkText");
  }
  async settingsGeneralTimezoneText(): Promise<string> {
    return todo("settingsGeneralTimezoneText");
  }
  async settingsGeneralCreatedText(): Promise<string> {
    return todo("settingsGeneralCreatedText");
  }
  async settingsGeneralExecutorValue(): Promise<string> {
    return todo("settingsGeneralExecutorValue");
  }
  async settingsGeneralExecutorOptionDisabled(_value: string): Promise<boolean> {
    return todo("settingsGeneralExecutorOptionDisabled");
  }
  async settingsGeneralSetExecutor(_value: string): Promise<void> {
    return todo("settingsGeneralSetExecutor");
  }
  async settingsGeneralTickingOn(): Promise<boolean> {
    return todo("settingsGeneralTickingOn");
  }
  async settingsGeneralSetTicking(_on: boolean): Promise<void> {
    return todo("settingsGeneralSetTicking");
  }
  async settingsGeneralBudgetValue(): Promise<string> {
    return todo("settingsGeneralBudgetValue");
  }
  async settingsGeneralSetBudget(_value: string): Promise<void> {
    return todo("settingsGeneralSetBudget");
  }
  async settingsGeneralSetNoCap(_on: boolean): Promise<void> {
    return todo("settingsGeneralSetNoCap");
  }
  async settingsGeneralCadenceText(): Promise<string> {
    return todo("settingsGeneralCadenceText");
  }
  async settingsGeneralSetCadence(_label: string): Promise<void> {
    return todo("settingsGeneralSetCadence");
  }
  async settingsGeneralSave(): Promise<void> {
    return todo("settingsGeneralSave");
  }
  async settingsGeneralBindState(): Promise<"bound" | "bind"> {
    return todo("settingsGeneralBindState");
  }
  async settingsGeneralBind(): Promise<void> {
    return todo("settingsGeneralBind");
  }
  async settingsGeneralEmblemOpen(): Promise<void> {
    return todo("settingsGeneralEmblemOpen");
  }
  async settingsGeneralEmblemPickerVisible(): Promise<boolean> {
    return todo("settingsGeneralEmblemPickerVisible");
  }
  async settingsGeneralEmblemPick(_char: string): Promise<void> {
    return todo("settingsGeneralEmblemPick");
  }
  async settingsGeneralCoverOpen(): Promise<void> {
    return todo("settingsGeneralCoverOpen");
  }
  async settingsGeneralCoverTabNames(): Promise<string[]> {
    return todo("settingsGeneralCoverTabNames");
  }
  async settingsGeneralCoverGalleryCount(): Promise<number> {
    return todo("settingsGeneralCoverGalleryCount");
  }
  async settingsGeneralCoverPickGallery(_index: number): Promise<void> {
    return todo("settingsGeneralCoverPickGallery");
  }
  async settingsGeneralCoverUpload(_file: { name: string; mime: string; bytes: string }): Promise<void> {
    return todo("settingsGeneralCoverUpload");
  }
  async settingsGeneralOpenDelete(): Promise<void> {
    return todo("settingsGeneralOpenDelete");
  }
  async settingsGeneralFillDeleteName(_text: string): Promise<void> {
    return todo("settingsGeneralFillDeleteName");
  }
  async settingsGeneralConfirmModalText(): Promise<string> {
    return todo("settingsGeneralConfirmModalText");
  }
  async settingsGeneralStallDetails(): Promise<void> {
    return todo("settingsGeneralStallDetails");
  }
  async settingsGeneralReleaseStall(): Promise<void> {
    return todo("settingsGeneralReleaseStall");
  }
  async settingsGeneralStallCoverUpload(): Promise<void> {
    return todo("settingsGeneralStallCoverUpload");
  }
  async settingsGeneralReleaseCoverUpload(): Promise<void> {
    return todo("settingsGeneralReleaseCoverUpload");
  }

  // Profile preferences + own activity (NEWFRONT-427, PROF-016..019). Stubs:
  // the area gate implements these against apps/web_new.
  // ---------------------------------------------------------------------------

  async settingsProfileOpenPreferences(): Promise<void> {
    return todo("settingsProfileOpenPreferences");
  }
  async settingsProfileOpenActivity(): Promise<void> {
    return todo("settingsProfileOpenActivity");
  }
  async settingsProfileThemeValue(): Promise<SettingsProfileTheme | null> {
    return todo("settingsProfileThemeValue");
  }
  async settingsProfileSetTheme(_theme: SettingsProfileTheme): Promise<void> {
    return todo("settingsProfileSetTheme");
  }
  async settingsProfileAppliedTheme(): Promise<string | null> {
    return todo("settingsProfileAppliedTheme");
  }
  async settingsProfileCustomVarsApplied(): Promise<boolean> {
    return todo("settingsProfileCustomVarsApplied");
  }
  async settingsProfileCustomDesignerVisible(): Promise<boolean> {
    return todo("settingsProfileCustomDesignerVisible");
  }
  async settingsProfileCustomDesignerValues(): Promise<SettingsProfileCustomTheme> {
    return todo("settingsProfileCustomDesignerValues");
  }
  async settingsProfilePreviewCustomColor(_field: "primary" | "background", _hex: string): Promise<string | null> {
    return todo("settingsProfilePreviewCustomColor");
  }
  async settingsProfileSetCustomPalette(_dark: boolean): Promise<void> {
    return todo("settingsProfileSetCustomPalette");
  }
  async settingsProfileSaveCustomTheme(): Promise<void> {
    return todo("settingsProfileSaveCustomTheme");
  }
  async settingsProfileFailProfileSaves(_status: number): Promise<void> {
    return todo("settingsProfileFailProfileSaves");
  }
  async settingsProfileClearProfileSaveFailure(): Promise<void> {
    return todo("settingsProfileClearProfileSaveFailure");
  }
  async settingsProfileLocaleValues(): Promise<SettingsProfileLocale> {
    return todo("settingsProfileLocaleValues");
  }
  async settingsProfileSetTimezone(_timezone: string): Promise<void> {
    return todo("settingsProfileSetTimezone");
  }
  async settingsProfileSetLanguage(_language: string): Promise<void> {
    return todo("settingsProfileSetLanguage");
  }
  async settingsProfileSetWeekStart(_day: string): Promise<void> {
    return todo("settingsProfileSetWeekStart");
  }
  async settingsProfileOpenHome(_workspaceSlug: string): Promise<void> {
    return todo("settingsProfileOpenHome");
  }
  async settingsProfileGreetingClockText(): Promise<string | null> {
    return todo("settingsProfileGreetingClockText");
  }
  async settingsProfileOpenMemberProfile(_workspaceSlug: string, _userId: string): Promise<void> {
    return todo("settingsProfileOpenMemberProfile");
  }
  async settingsProfileSidebarClockText(): Promise<string | null> {
    return todo("settingsProfileSidebarClockText");
  }
  async settingsProfileCalendarDayOrder(): Promise<string[]> {
    return todo("settingsProfileCalendarDayOrder");
  }
  async settingsProfileOwnActivityEntryCount(): Promise<number> {
    return todo("settingsProfileOwnActivityEntryCount");
  }
  async settingsProfileOwnActivityLoadMoreVisible(): Promise<boolean> {
    return todo("settingsProfileOwnActivityLoadMoreVisible");
  }
  async settingsProfileOwnActivityLoadMore(): Promise<void> {
    return todo("settingsProfileOwnActivityLoadMore");
  }
  async settingsProfileOwnActivityEmptyIllustration(): Promise<string | null> {
    return todo("settingsProfileOwnActivityEmptyIllustration");
  }
  async settingsProfileCycleUserTimezoneDates(): Promise<{ start: string | null; end: string | null }> {
    return todo("settingsProfileCycleUserTimezoneDates");
  }

  // --- Intake list shell (NEWFRONT-255, INT-001-005/008/012). Throwing
  // --- stubs; the build slice that lands this area fills them in.

  async intakeShellOpen(_workspaceSlug: string, _projectId: string): Promise<void> {
    return todo("intakeShellOpen");
  }

  async intakeShellOpenRetiredInbox(_workspaceSlug: string, _projectId: string): Promise<void> {
    return todo("intakeShellOpenRetiredInbox");
  }

  async intakeShellTabLabels(): Promise<string[]> {
    return todo("intakeShellTabLabels");
  }

  async intakeShellActiveTab(): Promise<IntakeShellTab> {
    return todo("intakeShellActiveTab");
  }

  async intakeShellOpenCount(): Promise<string | null> {
    return todo("intakeShellOpenCount");
  }

  async intakeShellClickTab(_tab: IntakeShellTab): Promise<void> {
    return todo("intakeShellClickTab");
  }

  async intakeShellListTitles(): Promise<string[]> {
    return todo("intakeShellListTitles");
  }

  async intakeShellPlaceholderVisible(): Promise<boolean> {
    return todo("intakeShellPlaceholderVisible");
  }

  async intakeShellBreadcrumbTrail(): Promise<string[]> {
    return todo("intakeShellBreadcrumbTrail");
  }

  async intakeShellBreadcrumbIntakeHref(): Promise<string | null> {
    return todo("intakeShellBreadcrumbIntakeHref");
  }

  async intakeShellPageTitle(): Promise<string> {
    return todo("intakeShellPageTitle");
  }

  async intakeShellFeatureGateVisible(): Promise<boolean> {
    return todo("intakeShellFeatureGateVisible");
  }

  async intakeShellFeatureGateActionEnabled(): Promise<boolean> {
    return todo("intakeShellFeatureGateActionEnabled");
  }

  async intakeShellFeatureGateActionGo(): Promise<string> {
    return todo("intakeShellFeatureGateActionGo");
  }

  async intakeShellListEmptyHeading(): Promise<string | null> {
    return todo("intakeShellListEmptyHeading");
  }

  async intakeShellEmptyCreateClick(): Promise<string> {
    return todo("intakeShellEmptyCreateClick");
  }

  async intakeShellFilterToggleStatus(_option: "Pending" | "Snoozed"): Promise<void> {
    return todo("intakeShellFilterToggleStatus");
  }

  async intakeShellBeginListSpy(): Promise<void> {
    return todo("intakeShellBeginListSpy");
  }

  async intakeShellListQueries(): Promise<IntakeShellListQuery[]> {
    return todo("intakeShellListQueries");
  }

  // --- Live cycles list (NEWFRONT-249, CYC-001–008). Appended; the stubs
  // --- above are untouched per the shared driver contract.
  async cyclesOpenList(_workspaceSlug: string, _projectId: string): Promise<void> {
    return todo("cyclesOpenList");
  }
  async cyclesVisibleNames(): Promise<string[]> {
    return todo("cyclesVisibleNames");
  }
  async cyclesGroupHeadings(): Promise<string[]> {
    return todo("cyclesGroupHeadings");
  }
  async cyclesGroupCounts(): Promise<{ upcoming: number; completed: number }> {
    return todo("cyclesGroupCounts");
  }
  async cyclesGroupToggle(_section: string): Promise<void> {
    return todo("cyclesGroupToggle");
  }
  async cyclesGroupExpanded(_section: string): Promise<boolean> {
    return todo("cyclesGroupExpanded");
  }
  async cyclesHeroName(): Promise<string | null> {
    return todo("cyclesHeroName");
  }
  async cyclesRowHref(_name: string): Promise<string | null> {
    return todo("cyclesRowHref");
  }
  async cyclesClickRow(_name: string): Promise<void> {
    return todo("cyclesClickRow");
  }
  async cyclesOpenDetail(_workspaceSlug: string, _projectId: string, _cycleId: string): Promise<void> {
    return todo("cyclesOpenDetail");
  }
  async cyclesDetailName(): Promise<string | null> {
    return todo("cyclesDetailName");
  }
  async cyclesDetailIssueNames(): Promise<string[]> {
    return todo("cyclesDetailIssueNames");
  }
  async cyclesRowFacts(_name: string): Promise<CyclesRowFacts> {
    return todo("cyclesRowFacts");
  }
  async cyclesOpenPeek(_name: string): Promise<void> {
    return todo("cyclesOpenPeek");
  }
  async cyclesOpenPeekLink(_workspaceSlug: string, _projectId: string, _cycleId: string): Promise<void> {
    return todo("cyclesOpenPeekLink");
  }
  async cyclesPeekName(): Promise<string | null> {
    return todo("cyclesPeekName");
  }
  async cyclesPeekParam(): Promise<string | null> {
    return todo("cyclesPeekParam");
  }
  async cyclesClosePeek(): Promise<void> {
    return todo("cyclesClosePeek");
  }
  async cyclesReload(): Promise<void> {
    return todo("cyclesReload");
  }
  async cyclesGoBack(): Promise<void> {
    return todo("cyclesGoBack");
  }
  async cyclesSearchExpanded(): Promise<boolean> {
    return todo("cyclesSearchExpanded");
  }
  async cyclesSearchOpen(): Promise<void> {
    return todo("cyclesSearchOpen");
  }
  async cyclesSearchFill(_text: string): Promise<void> {
    return todo("cyclesSearchFill");
  }
  async cyclesSearchText(): Promise<string> {
    return todo("cyclesSearchText");
  }
  async cyclesSearchEscape(): Promise<void> {
    return todo("cyclesSearchEscape");
  }
  async cyclesSearchClear(): Promise<void> {
    return todo("cyclesSearchClear");
  }
  async cyclesClickAway(): Promise<void> {
    return todo("cyclesClickAway");
  }
  async cyclesFiltersOpen(): Promise<void> {
    return todo("cyclesFiltersOpen");
  }
  async cyclesFiltersClose(): Promise<void> {
    return todo("cyclesFiltersClose");
  }
  async cyclesFilterSections(): Promise<string[]> {
    return todo("cyclesFilterSections");
  }
  async cyclesFilterOptionNames(): Promise<string[]> {
    return todo("cyclesFilterOptionNames");
  }
  async cyclesFilterPick(_section: string, _optionName: string): Promise<void> {
    return todo("cyclesFilterPick");
  }
  async cyclesFilterChipTexts(): Promise<string[]> {
    return todo("cyclesFilterChipTexts");
  }
  async cyclesFilterRemoveChip(_chipText: string): Promise<void> {
    return todo("cyclesFilterRemoveChip");
  }
  async cyclesFiltersClearAll(): Promise<void> {
    return todo("cyclesFiltersClearAll");
  }
  async cyclesFiltersActive(): Promise<boolean> {
    return todo("cyclesFiltersActive");
  }
}
