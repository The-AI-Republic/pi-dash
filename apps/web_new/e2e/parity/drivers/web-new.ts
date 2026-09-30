// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// New-app driver skeleton (NEWFRONT-19). Implements the same interface as
// the oracle driver so scenarios compile against either target, but every
// action throws until the matching area lands in apps/web_new. Area issues
// fill these in method by method; the oracle driver stays untouched.
import type { Page } from "@playwright/test";
import type {
  ParityBrowserCookie,
  ParityDriver,
  ParityTarget,
  RulesCommentMenuOption,
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
  async openIssueDetail(_workspaceSlug: string, _issueSeq: string): Promise<void> {
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
}
