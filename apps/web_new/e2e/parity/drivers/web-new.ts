// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// New-app driver skeleton (NEWFRONT-19). Implements the same interface as
// the oracle driver so scenarios compile against either target, but every
// action throws until the matching area lands in apps/web_new. Area issues
// fill these in method by method; the oracle driver stays untouched.
import type { Page } from "@playwright/test";
import type {
  ArchivesArchiveDialog,
  ArchivesFilterExpression,
  ArchivesListQuery,
  ArchivesMenuEntry,
  ArchivesModuleChip,
  ArchivesModulesEmptyKind,
  ArchivesPeekReadOnly,
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
  LayoutsLayoutKey,
  BoardLayoutKey,
  DevMachineInstallCard,
  DevMachineModal,
  DevMachineRow,
  GanttSidebarRow,
  GanttZoom,
  KanbanCard,
  KanbanColumn,
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
  ServedShellMarkers,
  WorkspaceOnboardingView,
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

  async openEntry(): Promise<void> {
    return todo("openEntry");
  }

  async signInWithPassword(_email: string, _password: string): Promise<void> {
    return todo("signInWithPassword");
  }

  async openProjectIssues(_workspaceSlug: string, _projectId: string): Promise<void> {
    return todo("openProjectIssues");
  }

  async visibleIssueNames(): Promise<string[]> {
    return todo("visibleIssueNames");
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
  async openAuthenticated(_path: string, _cookies: ParityBrowserCookie[]): Promise<void> {
    return todo("openAuthenticated");
  }
  async currentPath(): Promise<string> {
    return todo("currentPath");
  }
  async hasVisibleText(_text: string): Promise<boolean> {
    return todo("hasVisibleText");
  }
  async awaitWorkspaceStep(): Promise<void> {
    return todo("awaitWorkspaceStep");
  }
  async visibleWorkspaceView(): Promise<WorkspaceOnboardingView> {
    return todo("visibleWorkspaceView");
  }
  async fillWorkspaceName(_name: string): Promise<void> {
    return todo("fillWorkspaceName");
  }
  async fillWorkspaceSlug(_slug: string): Promise<void> {
    return todo("fillWorkspaceSlug");
  }
  async workspaceSlugValue(): Promise<string> {
    return todo("workspaceSlugValue");
  }
  async selectTeamSizePill(_label: string): Promise<void> {
    return todo("selectTeamSizePill");
  }
  async selectTeamSizeDropdown(_label: string): Promise<void> {
    return todo("selectTeamSizeDropdown");
  }
  async submitCreateWorkspace(): Promise<void> {
    return todo("submitCreateWorkspace");
  }
  async isCreateWorkspaceSubmitDisabled(): Promise<boolean> {
    return todo("isCreateWorkspaceSubmitDisabled");
  }
  async workspaceSlugErrorText(): Promise<string | null> {
    return todo("workspaceSlugErrorText");
  }
  async gotoJoinByEmailFromCreate(): Promise<void> {
    return todo("gotoJoinByEmailFromCreate");
  }
  async gotoInvitesFromCreate(): Promise<void> {
    return todo("gotoInvitesFromCreate");
  }
  async fillWorkspaceAdminEmail(_email: string): Promise<void> {
    return todo("fillWorkspaceAdminEmail");
  }
  async submitJoinRequest(): Promise<void> {
    return todo("submitJoinRequest");
  }
  async pendingApprovalNamesEmail(_email: string): Promise<boolean> {
    return todo("pendingApprovalNamesEmail");
  }
  async createInsteadFromPending(): Promise<void> {
    return todo("createInsteadFromPending");
  }
  async selectInviteByWorkspace(_workspaceName: string): Promise<void> {
    return todo("selectInviteByWorkspace");
  }
  async continueWithSelectedInvites(): Promise<void> {
    return todo("continueWithSelectedInvites");
  }
  async awaitInviteMembersStep(): Promise<void> {
    return todo("awaitInviteMembersStep");
  }
  async isInviteMembersStepVisible(): Promise<boolean> {
    return todo("isInviteMembersStepVisible");
  }
  async inviteRowCount(): Promise<number> {
    return todo("inviteRowCount");
  }
  async fillInviteRow(_index: number, _email: string): Promise<void> {
    return todo("fillInviteRow");
  }
  async clickAddAnotherInvite(): Promise<void> {
    return todo("clickAddAnotherInvite");
  }
  async isSendInvitesDisabled(): Promise<boolean> {
    return todo("isSendInvitesDisabled");
  }
  async sendInvites(): Promise<void> {
    return todo("sendInvites");
  }
  async deferInvites(): Promise<void> {
    return todo("deferInvites");
  }
  async isOnboardingBackVisible(): Promise<boolean> {
    return todo("isOnboardingBackVisible");
  }
  async clickOnboardingBack(): Promise<void> {
    return todo("clickOnboardingBack");
  }
  async isTourWelcomeVisible(): Promise<boolean> {
    return todo("isTourWelcomeVisible");
  }
  async declineTour(): Promise<void> {
    return todo("declineTour");
  }
  async isStandaloneCreationDisabledVisible(): Promise<boolean> {
    return todo("isStandaloneCreationDisabledVisible");
  }
  async isRequestInstanceAdminLinkVisible(): Promise<boolean> {
    return todo("isRequestInstanceAdminLinkVisible");
  }
  async isInOnboardingCreationDisabledNoticeVisible(): Promise<boolean> {
    return todo("isInOnboardingCreationDisabledNoticeVisible");
  }

  // --- Auth sign-in core (NEWFRONT-107). Throwing stubs per the shared
  // --- driver contract; the sign-in area fills these in when it lands.

  async openSignInWithParams(_params: Record<string, string>): Promise<void> {
    return todo("openSignInWithParams");
  }

  async submitEmail(_email: string): Promise<void> {
    return todo("submitEmail");
  }

  async submitPassword(_password: string): Promise<void> {
    return todo("submitPassword");
  }

  async authStep(): Promise<"email" | "password" | "code" | "unavailable" | "unknown"> {
    return todo("authStep");
  }

  async bannerText(): Promise<string | null> {
    return todo("bannerText");
  }

  async dismissBanner(): Promise<void> {
    return todo("dismissBanner");
  }

  async seesWorkspaceInviteHeader(_workspaceName: string): Promise<boolean> {
    return todo("seesWorkspaceInviteHeader");
  }

  async seesGenericSignInHeader(): Promise<boolean> {
    return todo("seesGenericSignInHeader");
  }

  async seesGenericSignUpHeader(): Promise<boolean> {
    return todo("seesGenericSignUpHeader");
  }

  async seesConfirmPassword(): Promise<boolean> {
    return todo("seesConfirmPassword");
  }

  async passwordPrimaryButtonLabel(): Promise<string | null> {
    return todo("passwordPrimaryButtonLabel");
  }

  async forgotPasswordEntry(): Promise<"link" | "popover" | "absent"> {
    return todo("forgotPasswordEntry");
  }

  async seesUniqueCodeButton(): Promise<boolean> {
    return todo("seesUniqueCodeButton");
  }

  async requestUniqueCode(): Promise<void> {
    return todo("requestUniqueCode");
  }

  async resendCodeLabel(): Promise<string | null> {
    return todo("resendCodeLabel");
  }

  async clickResendCode(): Promise<void> {
    return todo("clickResendCode");
  }

  async submitCode(_code: string): Promise<void> {
    return todo("submitCode");
  }

  async providerSignInButtons(): Promise<string[]> {
    return todo("providerSignInButtons");
  }

  async clickProviderButton(_name: string): Promise<void> {
    return todo("clickProviderButton");
  }

  async clearEmail(): Promise<void> {
    return todo("clearEmail");
  }

  async seesNoAuthMethods(): Promise<boolean> {
    return todo("seesNoAuthMethods");
  }

  async forgotPasswordPopoverText(): Promise<string | null> {
    return todo("forgotPasswordPopoverText");
  }

  // --- Auth sign-up, recovery, guards, landing (NEWFRONT-108, AUTH-009/016).
  // --- Throwing stubs per the shared driver contract; that area fills these
  // --- in when it lands. currentPath/authStep already stubbed above.

  async openSignUp(_params?: { email?: string; nextPath?: string }): Promise<void> {
    return todo("openSignUp");
  }

  async submitAuthEmail(_email: string): Promise<void> {
    return todo("submitAuthEmail");
  }

  async authEmailValue(): Promise<string> {
    return todo("authEmailValue");
  }

  async authNextPathValue(): Promise<string | null> {
    return todo("authNextPathValue");
  }

  async signUpWithPassword(_password: string, _confirmPassword: string): Promise<void> {
    return todo("signUpWithPassword");
  }

  async submitUniqueCode(_code: string): Promise<void> {
    return todo("submitUniqueCode");
  }

  async codeResendState(): Promise<{ disabled: boolean; label: string }> {
    return todo("codeResendState");
  }

  async requestNewCode(): Promise<void> {
    return todo("requestNewCode");
  }

  async passwordSubmitEnabled(): Promise<boolean> {
    return todo("passwordSubmitEnabled");
  }

  async fillPasswordFields(_password: string, _confirmPassword: string): Promise<void> {
    return todo("fillPasswordFields");
  }

  async clickPasswordSubmit(): Promise<void> {
    return todo("clickPasswordSubmit");
  }

  async passwordMismatchError(): Promise<string | null> {
    return todo("passwordMismatchError");
  }

  async authBanner(): Promise<string | null> {
    return todo("authBanner");
  }

  async waitForAuthBanner(): Promise<string> {
    return todo("waitForAuthBanner");
  }

  async openForgotPassword(_email?: string): Promise<void> {
    return todo("openForgotPassword");
  }

  async submitForgotPassword(_email: string): Promise<string> {
    return todo("submitForgotPassword");
  }

  async forgotResendState(): Promise<{ disabled: boolean; label: string }> {
    return todo("forgotResendState");
  }

  async openResetPassword(_params: { uid: string; token: string; email: string }): Promise<void> {
    return todo("openResetPassword");
  }

  async submitNewPassword(_password: string, _confirmPassword: string): Promise<void> {
    return todo("submitNewPassword");
  }

  async openSetPassword(): Promise<void> {
    return todo("openSetPassword");
  }

  async openPath(_path: string): Promise<void> {
    return todo("openPath");
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

  // Shell chrome (NEWFRONT-126): skeleton throws until the shell area lands.
  async openWorkspaceHome(): Promise<void> {
    return todo("openWorkspaceHome");
  }

  async openProjectsList(): Promise<void> {
    return todo("openProjectsList");
  }

  async openProjectTab(): Promise<void> {
    return todo("openProjectTab");
  }

  async sidebarPresent(): Promise<boolean> {
    return todo("sidebarPresent");
  }

  async sidebarWidth(): Promise<number | null> {
    return todo("sidebarWidth");
  }

  async portalPresent(): Promise<boolean> {
    return todo("portalPresent");
  }

  async railPresent(): Promise<boolean> {
    return todo("railPresent");
  }

  async contentPaddingLeft(): Promise<number | null> {
    return todo("contentPaddingLeft");
  }

  async projectTabs(): Promise<Array<{ name: string; href: string }>> {
    return todo("projectTabs");
  }

  async activeTabName(): Promise<string | null> {
    return todo("activeTabName");
  }

  async editionBadgePresent(): Promise<boolean> {
    return todo("editionBadgePresent");
  }

  async desktopUpdatePresent(): Promise<boolean> {
    return todo("desktopUpdatePresent");
  }

  async upgradePillCount(): Promise<number> {
    return todo("upgradePillCount");
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
    return todo("topBarControls");
  }

  async toggleSidebar(): Promise<void> {
    return todo("toggleSidebar");
  }

  async openPersonalizeDialog(): Promise<void> {
    return todo("openPersonalizeDialog");
  }

  async personalizeDialogOpen(): Promise<boolean> {
    return todo("personalizeDialogOpen");
  }

  async personalItemChecked(): Promise<boolean | null> {
    return todo("personalItemChecked");
  }

  async setPersonalItemEnabled(): Promise<void> {
    return todo("setPersonalItemEnabled");
  }

  async movePersonalItem(): Promise<void> {
    return todo("movePersonalItem");
  }

  async personalItemNames(): Promise<string[]> {
    return todo("personalItemNames");
  }

  async projectNavMode(): Promise<"ACCORDION" | "TABBED" | null> {
    return todo("projectNavMode");
  }

  async setProjectNavMode(): Promise<void> {
    return todo("setProjectNavMode");
  }

  async projectCapInput(): Promise<string | null> {
    return todo("projectCapInput");
  }

  async projectCapEnabled(): Promise<boolean | null> {
    return todo("projectCapEnabled");
  }

  async setProjectCap(): Promise<void> {
    return todo("setProjectCap");
  }

  async projectHeaderText(): Promise<string | null> {
    return todo("projectHeaderText");
  }

  async projectHeaderTruncated(): Promise<boolean> {
    return todo("projectHeaderTruncated");
  }

  async openProjectSwitcher(): Promise<void> {
    return todo("openProjectSwitcher");
  }

  async switcherOptionNames(): Promise<string[]> {
    return todo("switcherOptionNames");
  }

  async chooseSwitcherOption(): Promise<void> {
    return todo("chooseSwitcherOption");
  }

  async openProjectActions(): Promise<void> {
    return todo("openProjectActions");
  }

  async projectActionNames(): Promise<string[]> {
    return todo("projectActionNames");
  }

  async clickProjectAction(): Promise<void> {
    return todo("clickProjectAction");
  }

  async readClipboardText(): Promise<string> {
    return todo("readClipboardText");
  }

  async toastText(): Promise<string | null> {
    return todo("toastText");
  }

  async rightClickTab(): Promise<void> {
    return todo("rightClickTab");
  }

  async contextMenuItems(): Promise<string[]> {
    return todo("contextMenuItems");
  }

  async clickContextMenuItem(): Promise<void> {
    return todo("clickContextMenuItem");
  }

  async openOverflowMenu(): Promise<void> {
    return todo("openOverflowMenu");
  }

  async overflowTriggerPresent(): Promise<boolean> {
    return todo("overflowTriggerPresent");
  }

  async overflowRowNames(): Promise<string[]> {
    return todo("overflowRowNames");
  }

  async restoreOverflowTab(): Promise<void> {
    return todo("restoreOverflowTab");
  }

  async setViewportSize(): Promise<void> {
    return todo("setViewportSize");
  }

  async openNotifications(): Promise<void> {
    return todo("openNotifications");
  }

  // --- Issues bulk-ops / modal / drafts stubs (NEWFRONT-120).
  async selectionCheckboxCount(): Promise<number> {
    return todo("selectionCheckboxCount");
  }

  async bulkBarVisible(): Promise<boolean> {
    return todo("bulkBarVisible");
  }

  async pressKey(_key: string, _shift?: boolean): Promise<void> {
    return todo("pressKey");
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
    return todo("confirmArchive");
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

  async openProjectIssuesSettled(_workspaceSlug: string, _projectId: string): Promise<void> {
    return todo("openProjectIssuesSettled");
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

  async homeOpen(_workspaceSlug: string): Promise<void> {
    return todo("homeOpen");
  }

  async homeGreetingHeading(): Promise<string | null> {
    return todo("homeGreetingHeading");
  }

  async homeDateLine(): Promise<string | null> {
    return todo("homeDateLine");
  }

  async homeTourVisible(): Promise<boolean> {
    return todo("homeTourVisible");
  }

  async homeTourAdvance(): Promise<void> {
    return todo("homeTourAdvance");
  }

  async homeTourDismiss(): Promise<void> {
    return todo("homeTourDismiss");
  }

  async homeAssistantState(): Promise<"hidden" | "setup" | "ready"> {
    return todo("homeAssistantState");
  }

  async homeAssistantSuggestions(): Promise<string[]> {
    return todo("homeAssistantSuggestions");
  }

  async homeQuickstartVisible(): Promise<boolean> {
    return todo("homeQuickstartVisible");
  }

  async homeQuickstartTitles(): Promise<string[]> {
    return todo("homeQuickstartTitles");
  }

  async homeQuickstartCreateEnabled(): Promise<boolean> {
    return todo("homeQuickstartCreateEnabled");
  }

  async homeQuickstartDismiss(): Promise<void> {
    return todo("homeQuickstartDismiss");
  }

  async homeQuickstartCardDone(_title: string): Promise<boolean> {
    return todo("homeQuickstartCardDone");
  }

  async homeQuickstartActionTexts(): Promise<string[]> {
    return todo("homeQuickstartActionTexts");
  }

  async homeWidgetTitles(): Promise<string[]> {
    return todo("homeWidgetTitles");
  }

  async homeOpenManageWidgets(): Promise<void> {
    return todo("homeOpenManageWidgets");
  }

  async homeCloseManageWidgets(): Promise<void> {
    return todo("homeCloseManageWidgets");
  }

  async homeManageWidgetNames(): Promise<string[]> {
    return todo("homeManageWidgetNames");
  }

  async homeManageWidgetEnabled(_name: string): Promise<boolean> {
    return todo("homeManageWidgetEnabled");
  }

  async homeToggleManageWidget(_name: string): Promise<void> {
    return todo("homeToggleManageWidget");
  }

  async homeDragWidget(_sourceName: string, _targetName: string): Promise<void> {
    return todo("homeDragWidget");
  }

  async homeAllOffVisible(): Promise<boolean> {
    return todo("homeAllOffVisible");
  }

  async homeQuickLinkNames(): Promise<string[]> {
    return todo("homeQuickLinkNames");
  }

  async homeExpandQuickLinks(): Promise<void> {
    return todo("homeExpandQuickLinks");
  }

  async homeQuickLinksCollapsed(): Promise<boolean> {
    return todo("homeQuickLinksCollapsed");
  }

  async homeAddQuickLink(_title: string, _url: string): Promise<void> {
    return todo("homeAddQuickLink");
  }

  async homeEditQuickLink(_currentTitle: string, _nextTitle: string, _nextUrl: string): Promise<void> {
    return todo("homeEditQuickLink");
  }

  async homeDeleteQuickLink(_title: string): Promise<void> {
    return todo("homeDeleteQuickLink");
  }

  async homeLinkDialogError(): Promise<string | null> {
    return todo("homeLinkDialogError");
  }

  async homeCopyQuickLink(_title: string): Promise<void> {
    return todo("homeCopyQuickLink");
  }

  async homeReadClipboard(): Promise<string> {
    return todo("homeReadClipboard");
  }

  async homeOpenQuickLinkPopup(_title: string): Promise<string | null> {
    return todo("homeOpenQuickLinkPopup");
  }

  async homeLinkDialogOpen(): Promise<boolean> {
    return todo("homeLinkDialogOpen");
  }

  async homeCancelLinkDialog(): Promise<void> {
    return todo("homeCancelLinkDialog");
  }

  async homeSetRecentsFilter(_name: "all" | "issue" | "page" | "project"): Promise<void> {
    return todo("homeSetRecentsFilter");
  }

  async homeRecentRowTexts(): Promise<string[]> {
    return todo("homeRecentRowTexts");
  }

  async homeOpenRecentRow(_text: string): Promise<void> {
    return todo("homeOpenRecentRow");
  }

  async homeIssuePreviewVisible(): Promise<boolean> {
    return todo("homeIssuePreviewVisible");
  }

  async homeIssuePreviewText(): Promise<string> {
    return todo("homeIssuePreviewText");
  }

  async homeBreadcrumb(): Promise<string | null> {
    return todo("homeBreadcrumb");
  }

  async homeLastToast(): Promise<{ title: string; message: string } | null> {
    return todo("homeLastToast");
  }

  async homeReload(): Promise<void> {
    return todo("homeReload");
  }

  async homeOpenIssueDetail(_workspaceSlug: string, _projectId: string, _issueId: string): Promise<void> {
    return todo("homeOpenIssueDetail");
  }

  async homeSkeletonVisible(): Promise<boolean> {
    return todo("homeSkeletonVisible");
  }

  async homeWaitForWidgets(): Promise<string[]> {
    return todo("homeWaitForWidgets");
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
    return todo("readClipboard");
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
    return todo("sidebarBrandVisible");
  }

  async sidebarQuickActionNames(): Promise<string[]> {
    return todo("sidebarQuickActionNames");
  }

  async sidebarAccountButtonCount(): Promise<number> {
    return todo("sidebarAccountButtonCount");
  }

  async dragSidebarGripBy(): Promise<void> {
    return todo("dragSidebarGripBy");
  }

  async doubleClickSidebarGrip(): Promise<void> {
    return todo("doubleClickSidebarGrip");
  }

  async hoverCollapsedEdge(): Promise<void> {
    return todo("hoverCollapsedEdge");
  }

  async clickOutsideSidebar(): Promise<void> {
    return todo("clickOutsideSidebar");
  }

  async sidebarEntryVisible(): Promise<boolean> {
    return todo("sidebarEntryVisible");
  }

  async projectCapTypeText(): Promise<void> {
    return todo("projectCapTypeText");
  }

  async projectCapFill(): Promise<void> {
    return todo("projectCapFill");
  }

  async projectCapMinErrorVisible(): Promise<boolean> {
    return todo("projectCapMinErrorVisible");
  }

  async railSettingsEntryPresent(): Promise<boolean> {
    return todo("railSettingsEntryPresent");
  }

  async railContextMenuText(): Promise<string> {
    return todo("railContextMenuText");
  }

  async inboxDotPresent(): Promise<boolean> {
    return todo("inboxDotPresent");
  }

  async hoverProjectHeader(): Promise<void> {
    return todo("hoverProjectHeader");
  }

  async projectNameVisibleCount(): Promise<number> {
    return todo("projectNameVisibleCount");
  }

  async projectActionDialogHeading(): Promise<string | null> {
    return todo("projectActionDialogHeading");
  }

  async activeCyclesHeaderVisible(): Promise<boolean> {
    return todo("activeCyclesHeaderVisible");
  }

  async errorNoticeVisible(): Promise<boolean> {
    return todo("errorNoticeVisible");
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

  async signOutViaAccountMenu(): Promise<void> {
    return todo("signOutViaAccountMenu");
  }

  async signOutViaCommandPalette(): Promise<void> {
    return todo("signOutViaCommandPalette");
  }

  async isSignedOut(): Promise<boolean> {
    return todo("isSignedOut");
  }

  async openSwitchAccount(): Promise<void> {
    return todo("openSwitchAccount");
  }

  async switchAccountEmail(): Promise<string> {
    return todo("switchAccountEmail");
  }

  async confirmSwitchAccount(): Promise<void> {
    return todo("confirmSwitchAccount");
  }

  async openDeactivateAccount(): Promise<void> {
    return todo("openDeactivateAccount");
  }

  async confirmDeactivation(): Promise<void> {
    return todo("confirmDeactivation");
  }

  async dropSession(): Promise<void> {
    return todo("dropSession");
  }

  async visit(_path: string): Promise<void> {
    return todo("visit");
  }

  async showsText(_text: string): Promise<boolean> {
    return todo("showsText");
  }

  async typeDeviceCode(_code: string): Promise<void> {
    return todo("typeDeviceCode");
  }

  async deviceCodeFieldValue(): Promise<string> {
    return todo("deviceCodeFieldValue");
  }

  async submitDeviceApproval(): Promise<void> {
    return todo("submitDeviceApproval");
  }

  async typeText(_text: string): Promise<void> {
    return todo("typeText");
  }

  async focusedControlName(): Promise<null> {
    return todo("focusedControlName");
  }

  // Sidebar + workspace navigation (NEWFRONT-125, SHELL-046..062).
  // Throwing stubs per the shared driver contract; the sidebar area fills
  // these in when it lands. (Restored: the base merge dropped them.)
  async resetSession(): Promise<void> {
    return todo("resetSession");
  }

  async openWorkspacePath(_path: string): Promise<void> {
    return todo("openWorkspacePath");
  }

  async isCreateProjectVisible(): Promise<boolean> {
    return todo("isCreateProjectVisible");
  }

  async isSidebarOnScreen(): Promise<boolean> {
    return todo("isSidebarOnScreen");
  }

  async sidebarLinkTexts(): Promise<string[]> {
    return todo("sidebarLinkTexts");
  }

  async sidebarRowTone(_linkText: string): Promise<{ background: string; color: string }> {
    return todo("sidebarRowTone");
  }

  async isProjectsGroupOpen(): Promise<boolean> {
    return todo("isProjectsGroupOpen");
  }

  async setProjectsGroupOpen(_open: boolean): Promise<void> {
    return todo("setProjectsGroupOpen");
  }

  async projectRowHref(_projectName: string): Promise<string | null> {
    return todo("projectRowHref");
  }

  async isProjectRowInViewport(_projectName: string): Promise<boolean> {
    return todo("isProjectRowInViewport");
  }

  async toggleProjectRow(_projectName: string): Promise<void> {
    return todo("toggleProjectRow");
  }

  async isProjectRowOpen(_projectName: string): Promise<boolean> {
    return todo("isProjectRowOpen");
  }

  async setProjectRowOpen(_projectName: string, _open: boolean): Promise<void> {
    return todo("setProjectRowOpen");
  }

  async projectSubnavLinks(): Promise<{ text: string; href: string | null }[]> {
    return todo("projectSubnavLinks");
  }

  async isQuickCreateEnabled(): Promise<boolean> {
    return todo("isQuickCreateEnabled");
  }

  async openQuickCreate(): Promise<void> {
    return todo("openQuickCreate");
  }

  async isQuickCreateDialogOpen(): Promise<boolean> {
    return todo("isQuickCreateDialogOpen");
  }

  async openProjectQuickMenu(_projectName: string): Promise<void> {
    return todo("openProjectQuickMenu");
  }

  async projectQuickMenuTexts(): Promise<string[]> {
    return todo("projectQuickMenuTexts");
  }

  async isProjectsOverflowVisible(): Promise<boolean> {
    return todo("isProjectsOverflowVisible");
  }

  async isProjectsOverflowOpen(): Promise<boolean> {
    return todo("isProjectsOverflowOpen");
  }

  async setProjectsOverflowOpen(_open: boolean): Promise<void> {
    return todo("setProjectsOverflowOpen");
  }

  async searchOverflowProjects(_query: string): Promise<void> {
    return todo("searchOverflowProjects");
  }

  async overflowProjectNames(): Promise<string[]> {
    return todo("overflowProjectNames");
  }

  async isOverflowEmptyStateVisible(): Promise<boolean> {
    return todo("isOverflowEmptyStateVisible");
  }

  async isMoreSectionOpen(): Promise<boolean> {
    return todo("isMoreSectionOpen");
  }

  async setMoreSectionOpen(_open: boolean): Promise<void> {
    return todo("setMoreSectionOpen");
  }

  async moreSectionLinks(): Promise<{ text: string; href: string | null }[]> {
    return todo("moreSectionLinks");
  }

  async sidebarSectionNames(): Promise<string[]> {
    return todo("sidebarSectionNames");
  }

  async isFavoritesOpen(): Promise<boolean> {
    return todo("isFavoritesOpen");
  }

  async setFavoritesOpen(_open: boolean): Promise<void> {
    return todo("setFavoritesOpen");
  }

  async isOverflowCreateVisible(): Promise<boolean> {
    return todo("isOverflowCreateVisible");
  }

  async activateHelpEntry(_name: string): Promise<string | null> {
    return todo("activateHelpEntry");
  }

  async isToastVisible(_text: string): Promise<boolean> {
    return todo("isToastVisible");
  }

  async isDialogWithTextVisible(_text: string): Promise<boolean> {
    return todo("isDialogWithTextVisible");
  }

  async dismissTopmost(): Promise<void> {
    return todo("dismissTopmost");
  }

  async clickMainContent(): Promise<void> {
    return todo("clickMainContent");
  }

  async openSidebarLink(_text: string): Promise<void> {
    return todo("openSidebarLink");
  }

  async switchWorkspace(_name: string): Promise<void> {
    return todo("switchWorkspace");
  }

  async activateUserMenuItem(_name: string): Promise<void> {
    return todo("activateUserMenuItem");
  }

  async dragSidebarProjectBefore(_sourceName: string, _targetName: string): Promise<void> {
    return todo("dragSidebarProjectBefore");
  }

  async openFavoritesFolderDialog(): Promise<void> {
    return todo("openFavoritesFolderDialog");
  }

  async submitFavoritesFolderName(_name: string): Promise<void> {
    return todo("submitFavoritesFolderName");
  }

  async isFavoritesFolderDialogOpen(): Promise<boolean> {
    return todo("isFavoritesFolderDialogOpen");
  }

  async favoriteEntryNames(): Promise<string[]> {
    return todo("favoriteEntryNames");
  }

  async openFavoriteEntry(_name: string): Promise<void> {
    return todo("openFavoriteEntry");
  }

  async openFavoritesFolder(_name: string): Promise<void> {
    return todo("openFavoritesFolder");
  }

  async openWorkspaceSwitcher(): Promise<void> {
    return todo("openWorkspaceSwitcher");
  }

  async workspaceSwitcherTexts(): Promise<string[]> {
    return todo("workspaceSwitcherTexts");
  }

  async openUserMenu(): Promise<void> {
    return todo("openUserMenu");
  }

  async userMenuTexts(): Promise<string[]> {
    return todo("userMenuTexts");
  }

  async openHelpMenu(): Promise<void> {
    return todo("openHelpMenu");
  }

  async helpMenuTexts(): Promise<string[]> {
    return todo("helpMenuTexts");
  }

  async workspaceLogoState(): Promise<{ hasImage: boolean; label: string | null; initial: string | null }> {
    return todo("workspaceLogoState");
  }

  // --- NEWFRONT-125 review-fix stubs. Mirror of the interface additions.

  async activateProjectQuickMenuItem(_name: string): Promise<void> {
    return todo("activateProjectQuickMenuItem");
  }

  async dragFavoriteBefore(_sourceName: string, _targetName: string): Promise<void> {
    return todo("dragFavoriteBefore");
  }

  async openFavoriteQuickMenu(_name: string): Promise<void> {
    return todo("openFavoriteQuickMenu");
  }

  async favoriteQuickMenuTexts(): Promise<string[]> {
    return todo("favoriteQuickMenuTexts");
  }

  async activateFavoriteQuickMenuItem(_name: string): Promise<void> {
    return todo("activateFavoriteQuickMenuItem");
  }

  async setSidebarCollapsed(_collapsed: boolean): Promise<void> {
    return todo("setSidebarCollapsed");
  }

  async isSidebarCollapsed(): Promise<boolean> {
    return todo("isSidebarCollapsed");
  }

  async openCompactUserMenu(): Promise<void> {
    return todo("openCompactUserMenu");
  }

  async profileSettingsActiveTab(): Promise<string | null> {
    return todo("profileSettingsActiveTab");
  }

  async isSidebarPeekVisible(): Promise<boolean> {
    return todo("isSidebarPeekVisible");
  }

  async dismissDialogByOverlayClick(): Promise<void> {
    return todo("dismissDialogByOverlayClick");
  }

  async sidebarProjectPlaceholderCount(): Promise<number> {
    return todo("sidebarProjectPlaceholderCount");
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
    return todo("currentUrlPath");
  }

  async goToPath(_path: string): Promise<void> {
    return todo("goToPath");
  }

  async pressPaletteOpenChord(): Promise<void> {
    return todo("pressPaletteOpenChord");
  }

  async isCommandPaletteOpen(): Promise<boolean> {
    return todo("isCommandPaletteOpen");
  }

  async commandPalettePlaceholder(): Promise<string | null> {
    return todo("commandPalettePlaceholder");
  }

  async focusAndTypeTopBarSearch(_text: string): Promise<void> {
    return todo("focusAndTypeTopBarSearch");
  }

  async closeCommandPaletteViaBackdrop(): Promise<void> {
    return todo("closeCommandPaletteViaBackdrop");
  }

  async typeInCommandPalette(_text: string): Promise<void> {
    return todo("typeInCommandPalette");
  }

  async commandPaletteQueryValue(): Promise<string> {
    return todo("commandPaletteQueryValue");
  }

  async pressInCommandPalette(_key: string): Promise<void> {
    return todo("pressInCommandPalette");
  }

  async paletteGroupHeadings(): Promise<string[]> {
    return todo("paletteGroupHeadings");
  }

  async paletteCommandTitles(): Promise<string[]> {
    return todo("paletteCommandTitles");
  }

  async paletteHasCommand(_title: string): Promise<boolean> {
    return todo("paletteHasCommand");
  }

  async activatePaletteCommand(_title: string): Promise<void> {
    return todo("activatePaletteCommand");
  }

  async paletteSelectedItemText(): Promise<string | null> {
    return todo("paletteSelectedItemText");
  }

  async paletteSearchResultsHeading(): Promise<string | null> {
    return todo("paletteSearchResultsHeading");
  }

  async isPaletteSearchHeadingPulsing(): Promise<boolean> {
    return todo("isPaletteSearchHeadingPulsing");
  }

  async paletteHasWorkspaceLevelToggle(): Promise<boolean> {
    return todo("paletteHasWorkspaceLevelToggle");
  }

  async isWorkspaceLevelToggleEnabled(): Promise<boolean> {
    return todo("isWorkspaceLevelToggleEnabled");
  }

  async toggleWorkspaceLevel(): Promise<void> {
    return todo("toggleWorkspaceLevel");
  }

  async countSearchRequests(_action: () => Promise<void>): Promise<number> {
    return todo("countSearchRequests");
  }

  async lastSearchRequestParams(): Promise<Record<string, string> | null> {
    return todo("lastSearchRequestParams");
  }

  async isShortcutsDialogOpen(): Promise<boolean> {
    return todo("isShortcutsDialogOpen");
  }

  async pressShortcutsDialogChord(): Promise<void> {
    return todo("pressShortcutsDialogChord");
  }

  async typeShortcutsFilter(_text: string): Promise<void> {
    return todo("typeShortcutsFilter");
  }

  async shortcutsDialogCommandTitles(): Promise<string[]> {
    return todo("shortcutsDialogCommandTitles");
  }

  async repoStarLinkAttributes(): Promise<{ href: string; target: string; rel: string } | null> {
    return todo("repoStarLinkAttributes");
  }

  async repoStarIconSrc(): Promise<string | null> {
    return todo("repoStarIconSrc");
  }

  async documentTheme(): Promise<string> {
    return todo("documentTheme");
  }

  async documentLang(): Promise<string> {
    return todo("documentLang");
  }

  async paletteHasText(_text: string): Promise<boolean> {
    return todo("paletteHasText");
  }

  async openBrowseWorkItem(_workspaceSlug: string, _identifier: string): Promise<void> {
    return todo("openBrowseWorkItem");
  }

  async browseShowsWorkItemDetail(): Promise<boolean> {
    return todo("browseShowsWorkItemDetail");
  }

  async browseShowsWorkspaceWideList(): Promise<boolean> {
    return todo("browseShowsWorkspaceWideList");
  }

  // Top-bar search box (NEWFRONT-127, SHELL-081). Throwing stubs per the
  // shared driver contract.
  async topBarSearchPlaceholder(): Promise<string | null> {
    return todo("topBarSearchPlaceholder");
  }

  async focusTopBarSearch(): Promise<void> {
    return todo("focusTopBarSearch");
  }

  async isTopBarResultsOpen(): Promise<boolean> {
    return todo("isTopBarResultsOpen");
  }

  async typeInTopBarSearch(_text: string): Promise<void> {
    return todo("typeInTopBarSearch");
  }

  async topBarSearchValue(): Promise<string> {
    return todo("topBarSearchValue");
  }

  async topBarResultsCommandTitles(): Promise<string[]> {
    return todo("topBarResultsCommandTitles");
  }

  async pressInTopBarSearch(_key: string): Promise<void> {
    return todo("pressInTopBarSearch");
  }

  async closeTopBarViaOutsideClick(): Promise<void> {
    return todo("closeTopBarViaOutsideClick");
  }

  // Shared empty-state kit tiers (NEWFRONT-127, SHELL-104). Throwing stubs
  // per the shared driver contract.
  async titledEmptyState(_title: string): Promise<{
    description: string | null;
    imageSrc: string | null;
    buttons: string[];
  } | null> {
    return todo("titledEmptyState");
  }

  async clickEmptyStateAction(_title: string, _label: string): Promise<void> {
    return todo("clickEmptyStateAction");
  }

  async typeInIssueSearchModal(_text: string): Promise<void> {
    return todo("typeInIssueSearchModal");
  }

  // Cover-image primitive (NEWFRONT-127, SHELL-105). Throwing stubs per the
  // shared driver contract.
  async projectCardCoverSrcs(): Promise<(string | null)[]> {
    return todo("projectCardCoverSrcs");
  }

  async projectCardCoverShimmerVisible(): Promise<boolean> {
    return todo("projectCardCoverShimmerVisible");
  }

  // --- Projects list + lifecycle (NEWFRONT-124, SHELL-024..045) stubs. ---
  async openArchivedProjects(_workspaceSlug: string): Promise<void> {
    return todo("openArchivedProjects");
  }
  async visibleProjectCardNames(): Promise<string[]> {
    return todo("visibleProjectCardNames");
  }
  async awaitProjectCard(_name: string): Promise<void> {
    return todo("awaitProjectCard");
  }
  async gridColumnCount(): Promise<number> {
    return todo("gridColumnCount");
  }
  async setViewportWidth(_width: number): Promise<void> {
    return todo("setViewportWidth");
  }
  async isProjectsSkeletonVisible(): Promise<boolean> {
    return todo("isProjectsSkeletonVisible");
  }
  async emptyStateHeading(): Promise<string | null> {
    return todo("emptyStateHeading");
  }
  async isEmptyStateCreateVisible(): Promise<boolean> {
    return todo("isEmptyStateCreateVisible");
  }
  async isEmptyStateCreateEnabled(): Promise<boolean> {
    return todo("isEmptyStateCreateEnabled");
  }
  async clickEmptyStateCreate(): Promise<void> {
    return todo("clickEmptyStateCreate");
  }
  async emptyStateArtworkSignature(): Promise<string | null> {
    return todo("emptyStateArtworkSignature");
  }
  async isHeaderCreateButtonVisible(): Promise<boolean> {
    return todo("isHeaderCreateButtonVisible");
  }
  async headerCreateButtonLabel(): Promise<string | null> {
    return todo("headerCreateButtonLabel");
  }
  async clickHeaderCreateButton(): Promise<void> {
    return todo("clickHeaderCreateButton");
  }
  async breadcrumbLabels(): Promise<string[]> {
    return todo("breadcrumbLabels");
  }
  async isMobileListHeaderVisible(): Promise<boolean> {
    return todo("isMobileListHeaderVisible");
  }
  async isDesktopFilterRowVisible(): Promise<boolean> {
    return todo("isDesktopFilterRowVisible");
  }
  async breadcrumbTerminalIsLink(): Promise<boolean> {
    return todo("breadcrumbTerminalIsLink");
  }
  async openSortMenu(): Promise<void> {
    return todo("openSortMenu");
  }
  async selectSortOption(_label: string): Promise<void> {
    return todo("selectSortOption");
  }
  async currentSortLabel(): Promise<string> {
    return todo("currentSortLabel");
  }
  async isSortDirectionDisabled(): Promise<boolean> {
    return todo("isSortDirectionDisabled");
  }
  async closeMenu(): Promise<void> {
    return todo("closeMenu");
  }
  async openFilterMenu(): Promise<void> {
    return todo("openFilterMenu");
  }
  async typeFilterSearch(_text: string): Promise<void> {
    return todo("typeFilterSearch");
  }
  async filterMenuHasOption(_text: string): Promise<boolean> {
    return todo("filterMenuHasOption");
  }
  async selectFilterOption(_label: string): Promise<void> {
    return todo("selectFilterOption");
  }
  async isFilterBadgeVisible(): Promise<boolean> {
    return todo("isFilterBadgeVisible");
  }
  async appliedFilterChipTexts(): Promise<string[]> {
    return todo("appliedFilterChipTexts");
  }
  async removeAppliedFilterChip(_text: string): Promise<void> {
    return todo("removeAppliedFilterChip");
  }
  async clickClearAllFilters(): Promise<void> {
    return todo("clickClearAllFilters");
  }
  async filterMatchCountText(): Promise<string | null> {
    return todo("filterMatchCountText");
  }
  async openListSearch(): Promise<void> {
    return todo("openListSearch");
  }
  async typeListSearch(_text: string): Promise<void> {
    return todo("typeListSearch");
  }
  async listSearchValue(): Promise<string> {
    return todo("listSearchValue");
  }
  async isListSearchExpanded(): Promise<boolean> {
    return todo("isListSearchExpanded");
  }
  async pressEscapeInListSearch(): Promise<void> {
    return todo("pressEscapeInListSearch");
  }
  async clickListSearchClear(): Promise<void> {
    return todo("clickListSearchClear");
  }
  async clickOutsideListSearch(): Promise<void> {
    return todo("clickOutsideListSearch");
  }
  async cardShortCode(_name: string): Promise<string | null> {
    return todo("cardShortCode");
  }
  async cardHasPrivateMark(_name: string): Promise<boolean> {
    return todo("cardHasPrivateMark");
  }
  async cardSubText(_name: string): Promise<string | null> {
    return todo("cardSubText");
  }
  async cardHasFavoriteStar(_name: string): Promise<boolean> {
    return todo("cardHasFavoriteStar");
  }
  async clickFavoriteStar(_name: string): Promise<void> {
    return todo("clickFavoriteStar");
  }
  async cardHasCoverImage(_name: string): Promise<boolean> {
    return todo("cardHasCoverImage");
  }
  async cardHasLogo(_name: string): Promise<boolean> {
    return todo("cardHasLogo");
  }
  async cardAvatarStack(_name: string): Promise<string[]> {
    return todo("cardAvatarStack");
  }
  async clickProjectCard(_name: string): Promise<void> {
    return todo("clickProjectCard");
  }
  async openCardContextMenu(_name: string): Promise<void> {
    return todo("openCardContextMenu");
  }
  async contextMenuItemLabels(): Promise<string[]> {
    return todo("contextMenuItemLabels");
  }
  async clickCardContextMenuItem(_label: string): Promise<void> {
    return todo("clickCardContextMenuItem");
  }
  async cardFooterLabels(_name: string): Promise<string[]> {
    return todo("cardFooterLabels");
  }
  async clickCardJoin(_name: string): Promise<void> {
    return todo("clickCardJoin");
  }
  async isJoinDialogVisible(): Promise<boolean> {
    return todo("isJoinDialogVisible");
  }
  async joinDialogHeading(): Promise<string | null> {
    return todo("joinDialogHeading");
  }
  async confirmJoin(): Promise<void> {
    return todo("confirmJoin");
  }
  async openLeaveProjectDialog(_projectName: string): Promise<void> {
    return todo("openLeaveProjectDialog");
  }
  async fillLeaveProjectName(_text: string): Promise<void> {
    return todo("fillLeaveProjectName");
  }
  async fillLeaveConfirmPhrase(_text: string): Promise<void> {
    return todo("fillLeaveConfirmPhrase");
  }
  async submitLeave(): Promise<void> {
    return todo("submitLeave");
  }
  async leaveErrorText(): Promise<string | null> {
    return todo("leaveErrorText");
  }
  async isLeaveDialogVisible(): Promise<boolean> {
    return todo("isLeaveDialogVisible");
  }
  async openArchiveProjectDialog(_workspaceSlug: string, _projectId: string): Promise<void> {
    return todo("openArchiveProjectDialog");
  }
  async archiveDialogBodyText(): Promise<string | null> {
    return todo("archiveDialogBodyText");
  }
  async clickCardRestore(_name: string): Promise<void> {
    return todo("clickCardRestore");
  }
  async confirmRestore(): Promise<void> {
    return todo("confirmRestore");
  }
  async archivedCardHasAdminActions(_name: string): Promise<boolean> {
    return todo("archivedCardHasAdminActions");
  }
  async cardShowsArchivedMarker(_name: string): Promise<boolean> {
    return todo("cardShowsArchivedMarker");
  }
  async isRestoreDialogVisible(): Promise<boolean> {
    return todo("isRestoreDialogVisible");
  }
  async openDeleteProjectDialog(_name: string): Promise<void> {
    return todo("openDeleteProjectDialog");
  }
  async fillDeleteProjectName(_text: string): Promise<void> {
    return todo("fillDeleteProjectName");
  }
  async fillDeleteConfirmPhrase(_text: string): Promise<void> {
    return todo("fillDeleteConfirmPhrase");
  }
  async isDeleteSubmitDisabled(): Promise<boolean> {
    return todo("isDeleteSubmitDisabled");
  }
  async submitDelete(): Promise<void> {
    return todo("submitDelete");
  }
  async isCreateProjectDialogVisible(): Promise<boolean> {
    return todo("isCreateProjectDialogVisible");
  }
  async fillCreateProjectName(_text: string): Promise<void> {
    return todo("fillCreateProjectName");
  }
  async createProjectShortCodeValue(): Promise<string> {
    return todo("createProjectShortCodeValue");
  }
  async fillCreateProjectShortCode(_text: string): Promise<void> {
    return todo("fillCreateProjectShortCode");
  }
  async submitCreateProject(): Promise<void> {
    return todo("submitCreateProject");
  }
  async createProjectErrorText(): Promise<string | null> {
    return todo("createProjectErrorText");
  }
  async createFormCoverVisible(): Promise<boolean> {
    return todo("createFormCoverVisible");
  }
  async createFormIconVisible(): Promise<boolean> {
    return todo("createFormIconVisible");
  }

  // --- Invitation inbox + onboarding start (NEWFRONT-110, AUTH-026/033).
  // --- Throwing stubs per the shared driver contract; the invitations and
  // --- onboarding areas fill these in when they land.
  async openInvitations(): Promise<void> {
    return todo("openInvitations");
  }

  async invitationWorkspaceNames(): Promise<string[]> {
    return todo("invitationWorkspaceNames");
  }

  async toggleInvitation(_workspaceName: string): Promise<void> {
    return todo("toggleInvitation");
  }

  async acceptSelectedInvitations(): Promise<void> {
    return todo("acceptSelectedInvitations");
  }

  async invitationsEmptyStateVisible(): Promise<boolean> {
    return todo("invitationsEmptyStateVisible");
  }

  async openInvitationLink(_workspaceSlug: string, _invitationId: string, _token: string): Promise<void> {
    return todo("openInvitationLink");
  }

  async pageText(): Promise<string> {
    return todo("pageText");
  }

  async acceptSingleInvitation(): Promise<void> {
    return todo("acceptSingleInvitation");
  }

  async declineSingleInvitation(): Promise<void> {
    return todo("declineSingleInvitation");
  }

  async openOnboarding(): Promise<void> {
    return todo("openOnboarding");
  }

  async advanceCliInstall(): Promise<void> {
    return todo("advanceCliInstall");
  }

  async skipCliInstall(): Promise<void> {
    return todo("skipCliInstall");
  }

  async submitProfileStep(_displayName: string): Promise<void> {
    return todo("submitProfileStep");
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

  async ganttOpenTimeline(): Promise<void> {
    return todo("ganttOpenTimeline");
  }

  async ganttTimelineVisible(): Promise<boolean> {
    return todo("ganttTimelineVisible");
  }

  async boardActiveLayout(): Promise<BoardLayoutKey> {
    return todo("boardActiveLayout");
  }

  async boardReloadIssues(): Promise<void> {
    return todo("boardReloadIssues");
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

  async issuePeekVisible(): Promise<boolean> {
    return todo("issuePeekVisible");
  }

  async issuePeekTitle(): Promise<string | null> {
    return todo("issuePeekTitle");
  }

  async issuePeekClose(): Promise<void> {
    return todo("issuePeekClose");
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
    return todo("ganttHeader");
  }

  async ganttActiveZoom(): Promise<GanttZoom | "unknown"> {
    return todo("ganttActiveZoom");
  }

  async ganttSetZoom(_view: GanttZoom): Promise<void> {
    return todo("ganttSetZoom");
  }

  async ganttDayWidth(): Promise<number> {
    return todo("ganttDayWidth");
  }

  async ganttWeekendTinted(): Promise<boolean> {
    return todo("ganttWeekendTinted");
  }

  async ganttWeekRowStarts(): Promise<string[]> {
    return todo("ganttWeekRowStarts");
  }

  async ganttClickToday(): Promise<void> {
    return todo("ganttClickToday");
  }

  async ganttTodayVisible(): Promise<boolean> {
    return todo("ganttTodayVisible");
  }

  async ganttTodayHighlighted(): Promise<boolean> {
    return todo("ganttTodayHighlighted");
  }

  async ganttToggleFullscreen(): Promise<void> {
    return todo("ganttToggleFullscreen");
  }

  async ganttFullscreenActive(): Promise<boolean> {
    return todo("ganttFullscreenActive");
  }

  async ganttTimelineWidth(): Promise<number> {
    return todo("ganttTimelineWidth");
  }

  async ganttScrollLeft(): Promise<number> {
    return todo("ganttScrollLeft");
  }

  async ganttScrollTo(_x: number): Promise<void> {
    return todo("ganttScrollTo");
  }

  async ganttSidebarRows(): Promise<GanttSidebarRow[]> {
    return todo("ganttSidebarRows");
  }

  async ganttOpenRowPeek(_issueName: string): Promise<void> {
    return todo("ganttOpenRowPeek");
  }

  async ganttSidebarOrder(): Promise<string[]> {
    return todo("ganttSidebarOrder");
  }

  async ganttDragRowBefore(_sourceName: string, _targetName: string): Promise<void> {
    return todo("ganttDragRowBefore");
  }

  async ganttAttemptRowBefore(_sourceName: string, _targetName: string): Promise<void> {
    return todo("ganttAttemptRowBefore");
  }

  async ganttBarExists(_issueName: string): Promise<boolean> {
    return todo("ganttBarExists");
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
    return todo("ganttSidebarLoading");
  }

  async ganttLoadMoreVisible(): Promise<boolean> {
    return todo("ganttLoadMoreVisible");
  }

  async ganttLoadingObservedOnReload(): Promise<boolean> {
    return todo("ganttLoadingObservedOnReload");
  }

  async ganttEmptyVisible(): Promise<boolean> {
    return todo("ganttEmptyVisible");
  }

  async ganttLoadMoreObservedOnScroll(): Promise<boolean> {
    return todo("ganttLoadMoreObservedOnScroll");
  }

  // --- Root document shell + not-found (NEWFRONT-173). Throwing stubs
  // --- per the shared driver contract; the document area fills these in.

  async documentShellFacts(): Promise<DocumentShellFacts> {
    return todo("documentShellFacts");
  }
  async overlayPortalsPresent(): Promise<{ contextMenu: boolean; editor: boolean }> {
    return todo("overlayPortalsPresent");
  }
  async sessionRecorderPresent(): Promise<boolean> {
    return todo("sessionRecorderPresent");
  }
  async installAssetStatuses(): Promise<{ href: string; status: number }[]> {
    return todo("installAssetStatuses");
  }
  async notFoundFacts(): Promise<NotFoundFacts | null> {
    return todo("notFoundFacts");
  }
  async notFoundGoHome(): Promise<void> {
    return todo("notFoundGoHome");
  }
  async servedShellMarkers(_path: string): Promise<ServedShellMarkers> {
    return todo("servedShellMarkers");
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

  // --- Runner chat on cloud/web (NEWFRONT-181, RUN-025–032). Throwing
  // --- stubs per the shared driver contract; the chat area fills these
  // --- in when it lands in apps/web_new.

  async runnerChatOpen(_workspaceSlug: string, _runnerId: string, _sessionId?: string): Promise<void> {
    return todo("runnerChatOpen");
  }

  async runnerChatContactNames(): Promise<string[]> {
    return todo("runnerChatContactNames");
  }

  async runnerChatContactDotClass(_runnerName: string): Promise<string> {
    return todo("runnerChatContactDotClass");
  }

  async runnerChatOpenContact(_runnerName: string): Promise<void> {
    return todo("runnerChatOpenContact");
  }

  async runnerChatHeader(): Promise<{ name: string; secondary: string; badge: string }> {
    return todo("runnerChatHeader");
  }

  async runnerChatHistoryEmptyVisible(): Promise<boolean> {
    return todo("runnerChatHistoryEmptyVisible");
  }

  async runnerChatHistoryItems(): Promise<{ title: string; subtitle: string; active: boolean }[]> {
    return todo("runnerChatHistoryItems");
  }

  async runnerChatClickHistoryItem(_index: number): Promise<void> {
    return todo("runnerChatClickHistoryItem");
  }

  async runnerChatNewChat(): Promise<void> {
    return todo("runnerChatNewChat");
  }

  async runnerChatNewChatDisabled(): Promise<boolean> {
    return todo("runnerChatNewChatDisabled");
  }

  async runnerChatFailSessionCreate(): Promise<void> {
    return todo("runnerChatFailSessionCreate");
  }

  async runnerChatDelaySessionCreate(_ms: number): Promise<void> {
    return todo("runnerChatDelaySessionCreate");
  }

  async runnerChatClearSessionCreateStubs(): Promise<void> {
    return todo("runnerChatClearSessionCreateStubs");
  }

  async runnerChatFillDraft(_text: string): Promise<void> {
    return todo("runnerChatFillDraft");
  }

  async runnerChatDraftValue(): Promise<string> {
    return todo("runnerChatDraftValue");
  }

  async runnerChatPressEnter(): Promise<void> {
    return todo("runnerChatPressEnter");
  }

  async runnerChatPressShiftEnter(): Promise<void> {
    return todo("runnerChatPressShiftEnter");
  }

  async runnerChatSendEnabled(): Promise<boolean> {
    return todo("runnerChatSendEnabled");
  }

  async runnerChatClickSend(): Promise<void> {
    return todo("runnerChatClickSend");
  }

  async runnerChatComposerReason(): Promise<string | null> {
    return todo("runnerChatComposerReason");
  }

  async runnerChatTextareaDisabled(): Promise<boolean> {
    return todo("runnerChatTextareaDisabled");
  }

  async runnerChatAlertText(): Promise<string | null> {
    return todo("runnerChatAlertText");
  }

  async runnerChatDismissAlert(): Promise<void> {
    return todo("runnerChatDismissAlert");
  }

  async runnerChatLastToast(): Promise<{ title: string; message: string } | null> {
    return todo("runnerChatLastToast");
  }

  async runnerChatFailNextSend(): Promise<void> {
    return todo("runnerChatFailNextSend");
  }

  async runnerChatClearSendFailure(): Promise<void> {
    return todo("runnerChatClearSendFailure");
  }

  async runnerChatVoiceButtonLabel(): Promise<string | null> {
    return todo("runnerChatVoiceButtonLabel");
  }

  async runnerChatClickVoiceButton(): Promise<void> {
    return todo("runnerChatClickVoiceButton");
  }

  async runnerChatStartApiSpy(): Promise<void> {
    return todo("runnerChatStartApiSpy");
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
    return todo("runnerChatApiCounts");
  }

  async runnerChatStopApiSpy(): Promise<void> {
    return todo("runnerChatStopApiSpy");
  }

  async runnerChatDelayRunnerDetail(_runnerId: string, _ms: number): Promise<void> {
    return todo("runnerChatDelayRunnerDetail");
  }

  async runnerChatClearRunnerDetailDelay(): Promise<void> {
    return todo("runnerChatClearRunnerDetailDelay");
  }

  async runnerChatStubStream(_sessionId: string, _frames: RunnerChatStreamFrame[]): Promise<void> {
    return todo("runnerChatStubStream");
  }

  async runnerChatClearStreamStub(_sessionId: string): Promise<void> {
    return todo("runnerChatClearStreamStub");
  }

  async runnerChatStreamRequestUrls(_sessionId: string): Promise<string[]> {
    return todo("runnerChatStreamRequestUrls");
  }

  async runnerChatMessageBubbles(): Promise<{ role: string; text: string }[]> {
    return todo("runnerChatMessageBubbles");
  }

  async runnerChatAssistantBubbleHtml(_index: number): Promise<string> {
    return todo("runnerChatAssistantBubbleHtml");
  }

  async runnerChatActivityStrip(): Promise<string[]> {
    return todo("runnerChatActivityStrip");
  }

  async runnerChatStopVisible(): Promise<boolean> {
    return todo("runnerChatStopVisible");
  }

  async runnerChatClickStop(): Promise<void> {
    return todo("runnerChatClickStop");
  }

  async runnerChatClickClose(): Promise<void> {
    return todo("runnerChatClickClose");
  }

  async runnerChatApprovalPromptVisible(): Promise<boolean> {
    return todo("runnerChatApprovalPromptVisible");
  }

  async runnerChatHoldMessageList(_ms: number): Promise<void> {
    return todo("runnerChatHoldMessageList");
  }

  async runnerChatReleaseMessageList(): Promise<void> {
    return todo("runnerChatReleaseMessageList");
  }

  // --- Prompts + project automations (NEWFRONT-186, AGT-023–037) ---
  async promptsOpen(_workspaceSlug: string): Promise<void> {
    return todo("promptsOpen");
  }
  async promptsActiveTab(): Promise<"Sections" | "Receipt"> {
    return todo("promptsActiveTab");
  }
  async promptsOpenTab(_tab: "Sections" | "Receipt"): Promise<void> {
    return todo("promptsOpenTab");
  }
  async promptsSectionCards(): Promise<PromptSectionCard[]> {
    return todo("promptsSectionCards");
  }
  async promptsSectionCard(_key: string): Promise<PromptSectionCard | null> {
    return todo("promptsSectionCard");
  }
  async promptsSectionNav(): Promise<{ title: string; key: string }[]> {
    return todo("promptsSectionNav");
  }
  async promptsSectionNavJump(_key: string): Promise<string> {
    return todo("promptsSectionNavJump");
  }
  async promptsLoadingVisible(): Promise<boolean> {
    return todo("promptsLoadingVisible");
  }
  async promptsSectionsErrorVisible(): Promise<boolean> {
    return todo("promptsSectionsErrorVisible");
  }
  async promptsWorkspaceWarningVisible(): Promise<boolean> {
    return todo("promptsWorkspaceWarningVisible");
  }
  async promptsFailSectionsStart(_scope: "user" | "workspace"): Promise<void> {
    return todo("promptsFailSectionsStart");
  }
  async promptsFailSectionsStop(): Promise<void> {
    return todo("promptsFailSectionsStop");
  }
  async promptsDelaySectionsOnce(_ms: number): Promise<void> {
    return todo("promptsDelaySectionsOnce");
  }
  async promptsFailUpsertOnce(): Promise<void> {
    return todo("promptsFailUpsertOnce");
  }
  async promptsOpenSectionEditor(_key: string, _scope: "workspace" | "user"): Promise<void> {
    return todo("promptsOpenSectionEditor");
  }
  async promptsEditorState(): Promise<PromptEditorState | null> {
    return todo("promptsEditorState");
  }
  async promptsEditorFill(_text: string): Promise<void> {
    return todo("promptsEditorFill");
  }
  async promptsEditorSave(): Promise<void> {
    return todo("promptsEditorSave");
  }
  async promptsEditorCancel(): Promise<void> {
    return todo("promptsEditorCancel");
  }
  async promptsEditorToggleCompare(): Promise<void> {
    return todo("promptsEditorToggleCompare");
  }
  async promptsEditorRevertOpen(): Promise<void> {
    return todo("promptsEditorRevertOpen");
  }
  async promptsRevertDialog(): Promise<PromptRevertDialog | null> {
    return todo("promptsRevertDialog");
  }
  async promptsRevertConfirm(): Promise<void> {
    return todo("promptsRevertConfirm");
  }
  async promptsRevertCancel(): Promise<void> {
    return todo("promptsRevertCancel");
  }
  async promptsReceiptCards(): Promise<PromptReceiptCard[]> {
    return todo("promptsReceiptCards");
  }
  async promptsReceiptNav(): Promise<{ kind: string; count: string }[]> {
    return todo("promptsReceiptNav");
  }
  async promptsReceiptNavJump(_kind: string): Promise<string> {
    return todo("promptsReceiptNavJump");
  }
  async promptsReceiptToggle(_kind: string): Promise<void> {
    return todo("promptsReceiptToggle");
  }
  async promptsReceiptExpanded(_kind: string): Promise<boolean> {
    return todo("promptsReceiptExpanded");
  }
  async promptsReceiptTemplate(_kind: string): Promise<string | null> {
    return todo("promptsReceiptTemplate");
  }
  async promptsReceiptAutomatic(_kind: string): Promise<string | null> {
    return todo("promptsReceiptAutomatic");
  }
  async promptsSavedPreviewVisible(_kind: string): Promise<boolean> {
    return todo("promptsSavedPreviewVisible");
  }
  async promptsSavedPreviewSubmitEnabled(_kind: string): Promise<boolean> {
    return todo("promptsSavedPreviewSubmitEnabled");
  }
  async promptsSavedPreviewSubmit(_kind: string, _target: string): Promise<void> {
    return todo("promptsSavedPreviewSubmit");
  }
  async promptsSavedPreviewResult(_kind: string): Promise<{ prompt: string | null; error: string | null }> {
    return todo("promptsSavedPreviewResult");
  }
  async promptsDraftPreviewKinds(): Promise<string[]> {
    return todo("promptsDraftPreviewKinds");
  }
  async promptsDraftPreviewSelectKind(_kind: string): Promise<void> {
    return todo("promptsDraftPreviewSelectKind");
  }
  async promptsDraftPreviewSubmitEnabled(): Promise<boolean> {
    return todo("promptsDraftPreviewSubmitEnabled");
  }
  async promptsDraftPreviewSubmit(_target: string): Promise<void> {
    return todo("promptsDraftPreviewSubmit");
  }
  async promptsDraftPreviewResult(): Promise<{ prompt: string | null; error: string | null }> {
    return todo("promptsDraftPreviewResult");
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

  // --- Assistant chat core (NEWFRONT-187). Throwing stubs per the shared
  // --- driver contract; the assistant area fills these in when it lands.

  async assistantOpenLanding(_workspaceSlug: string): Promise<void> {
    return todo("assistantOpenLanding");
  }
  async assistantOpenThread(_workspaceSlug: string, _threadId: string): Promise<void> {
    return todo("assistantOpenThread");
  }
  async assistantOpenHome(_workspaceSlug: string): Promise<void> {
    return todo("assistantOpenHome");
  }
  async assistantCurrentPath(): Promise<string> {
    return todo("assistantCurrentPath");
  }
  async assistantGoBack(): Promise<void> {
    return todo("assistantGoBack");
  }
  async assistantLandingGreeting(): Promise<AssistantLandingGreeting | null> {
    return todo("assistantLandingGreeting");
  }
  async assistantLandingComposerVisible(): Promise<boolean> {
    return todo("assistantLandingComposerVisible");
  }
  async assistantSetupCard(): Promise<{ title: string; body: string; button: string } | null> {
    return todo("assistantSetupCard");
  }
  async assistantSetupCardClick(): Promise<void> {
    return todo("assistantSetupCardClick");
  }
  async assistantFillDraft(_text: string): Promise<void> {
    return todo("assistantFillDraft");
  }
  async assistantDraftValue(): Promise<string> {
    return todo("assistantDraftValue");
  }
  async assistantPressEnter(): Promise<void> {
    return todo("assistantPressEnter");
  }
  async assistantPressShiftEnter(): Promise<void> {
    return todo("assistantPressShiftEnter");
  }
  async assistantPressControlEnter(): Promise<void> {
    return todo("assistantPressControlEnter");
  }
  async assistantSendVisible(): Promise<boolean> {
    return todo("assistantSendVisible");
  }
  async assistantSendEnabled(): Promise<boolean> {
    return todo("assistantSendEnabled");
  }
  async assistantClickSend(): Promise<void> {
    return todo("assistantClickSend");
  }
  async assistantStopVisible(): Promise<boolean> {
    return todo("assistantStopVisible");
  }
  async assistantClickStop(): Promise<void> {
    return todo("assistantClickStop");
  }
  async assistantComposerReason(): Promise<string | null> {
    return todo("assistantComposerReason");
  }
  async assistantTextareaDisabled(): Promise<boolean> {
    return todo("assistantTextareaDisabled");
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
    return todo("assistantSidebarThreads");
  }
  async assistantSidebarEmptyVisible(): Promise<boolean> {
    return todo("assistantSidebarEmptyVisible");
  }
  async assistantClickNewChat(): Promise<void> {
    return todo("assistantClickNewChat");
  }
  async assistantClickSidebarThread(_index: number): Promise<void> {
    return todo("assistantClickSidebarThread");
  }
  async assistantCardVisible(): Promise<boolean> {
    return todo("assistantCardVisible");
  }
  async assistantCardFillDraft(_text: string): Promise<void> {
    return todo("assistantCardFillDraft");
  }
  async assistantCardDraftValue(): Promise<string> {
    return todo("assistantCardDraftValue");
  }
  async assistantCardPressEnter(): Promise<void> {
    return todo("assistantCardPressEnter");
  }
  async assistantCardAskDisabled(): Promise<boolean> {
    return todo("assistantCardAskDisabled");
  }
  async assistantCardClickAsk(): Promise<void> {
    return todo("assistantCardClickAsk");
  }
  async assistantCardClickSuggestion(_text: string): Promise<void> {
    return todo("assistantCardClickSuggestion");
  }
  async assistantCardSuggestions(): Promise<string[]> {
    return todo("assistantCardSuggestions");
  }
  async assistantCardRecents(): Promise<{ title: string; href: string }[]> {
    return todo("assistantCardRecents");
  }
  async assistantCardClickRecent(_index: number): Promise<void> {
    return todo("assistantCardClickRecent");
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
    return todo("assistantStartApiSpy");
  }
  async assistantApiCounts(): Promise<AssistantApiCounts> {
    return todo("assistantApiCounts");
  }
  async assistantStopApiSpy(): Promise<void> {
    return todo("assistantStopApiSpy");
  }
  async assistantFailThreadCreateOnce(): Promise<void> {
    return todo("assistantFailThreadCreateOnce");
  }
  async assistantDelayThreadCreate(_ms: number): Promise<void> {
    return todo("assistantDelayThreadCreate");
  }
  async assistantClearThreadCreateStubs(): Promise<void> {
    return todo("assistantClearThreadCreateStubs");
  }
  async assistantDelaySend(_ms: number): Promise<void> {
    return todo("assistantDelaySend");
  }
  async assistantClearSendDelay(): Promise<void> {
    return todo("assistantClearSendDelay");
  }
  async assistantLastToast(): Promise<{ title: string; message: string } | null> {
    return todo("assistantLastToast");
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
  async notificationsOpenEmailPreferences(): Promise<void> {
    return todo("notificationsOpenEmailPreferences");
  }
  async notificationsEmailPreferencesLoaderShown(): Promise<boolean> {
    return todo("notificationsEmailPreferencesLoaderShown");
  }
  async notificationsEmailPreferences(): Promise<Record<NotificationsEmailPref, boolean>> {
    return todo("notificationsEmailPreferences");
  }
  async notificationsEmailPreferencesToggle(_pref: NotificationsEmailPref): Promise<void> {
    return todo("notificationsEmailPreferencesToggle");
  }
  async notificationsEmailPreferencesCompletedNested(): Promise<boolean> {
    return todo("notificationsEmailPreferencesCompletedNested");
  }
  async notificationsFailEmailPreferenceSaves(_status: number): Promise<void> {
    return todo("notificationsFailEmailPreferenceSaves");
  }
  async notificationsClearEmailPreferenceSaveFailure(): Promise<void> {
    return todo("notificationsClearEmailPreferenceSaveFailure");
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
}
