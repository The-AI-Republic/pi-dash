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
  LayoutsLayoutKey,
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
}
