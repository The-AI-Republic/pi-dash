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

  async kanbanBoardScroll(): Promise<{ x: number; y: number }> {
    return todo("kanbanBoardScroll");
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

  async ganttBarExists(_issueName: string): Promise<boolean> {
    return todo("ganttBarExists");
  }

  async ganttDragBar(_issueName: string, _dayDelta: number): Promise<void> {
    return todo("ganttDragBar");
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
}
