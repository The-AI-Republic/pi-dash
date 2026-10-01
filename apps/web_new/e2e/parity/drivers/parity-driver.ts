// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Parity driver interface (NEWFRONT-19). Scenarios are written against this
// interface only: user-level actions plus user-visible reads. Two drivers
// implement it (drivers/web against apps/web, drivers/web_new against
// apps/web_new) so the same scenario runs on both apps. Extend the
// interface when a new area needs an action; never fork a driver.
import type { Page } from "@playwright/test";

/** Which frontend a scenario run targets. */
export type ParityTarget = "web" | "web_new";

/** Facts every parity scenario may assume about the seeded stack. */
export interface ParitySeedFacts {
  email: string;
  password: string;
  workspaceSlug: string;
  workspaceName: string;
  /** UUID string of the seeded project. */
  projectId: string;
  projectName: string;
  /** Issue names in the order the seed created them. */
  issueNames: string[];
  /**
   * Second workspace member (optional so older seed files still parse).
   * Mention scenarios @-mention this user instead of the author.
   */
  mentionMember?: {
    email: string;
    password: string;
    id: string;
    displayName: string;
  };
  /** Second seeded identity, a guest on the seeded project (NEWFRONT-113). */
  guestEmail?: string;
  guestPassword?: string;
  /** Pending triage row for the intake-screen variant (NEWFRONT-113). */
  inboxIssueId?: string;
}

/** A cookie shaped for a browser context, used to enter the app pre-authenticated. */
export interface ParityBrowserCookie {
  name: string;
  value: string;
  domain: string;
  path: string;
  httpOnly: boolean;
  sameSite: "Lax";
}

/** Which create-or-join sub-view of the workspace onboarding step is showing. */
export type WorkspaceOnboardingView = "create" | "invites" | "join_by_email" | "pending" | "none";

/** Which step of the email-first auth card is currently showing. */
export type AuthStep = "email" | "password" | "code";

/** User-level actions plus reads shared by both frontend drivers. */
export interface ParityDriver {
  readonly target: ParityTarget;
  readonly page: Page;
  /** Open the app entry point as a signed-out visitor. */
  openEntry(): Promise<void>;
  /** Complete the email-plus-password sign-in flow; ends authenticated. */
  signInWithPassword(email: string, password: string): Promise<void>;
  /** Open the issues list of a project; requires an authenticated session. */
  openProjectIssues(workspaceSlug: string, projectId: string): Promise<void>;
  /** Names of the issues currently rendered in the list, in display order. */
  visibleIssueNames(): Promise<string[]>;

  // --- Mention flows (NEWFRONT-115, CMT-019/020/021). Appended; existing
  // --- methods above are untouched per the shared driver contract.

  /** Open one work-item detail page; requires an authenticated session. */
  mentionsOpenIssueDetail(workspaceSlug: string, projectId: string, issueId: string): Promise<void>;
  /**
   * Type the mention trigger plus `query` in the comment composer and read
   * the suggestion names in display order. Resolves once the list appears.
   */
  mentionsSuggestionsFor(query: string): Promise<string[]>;
  /** Whether the currently open suggestion list renders member imagery. */
  mentionsSuggestionsHaveAvatars(): Promise<boolean>;
  /** Section headers grouping the open suggestion list (e.g. users). */
  mentionsSuggestionSections(): Promise<string[]>;
  /**
   * Post a comment that @-mentions `displayName`: types the trigger, picks
   * the matching suggestion, adds `bodyText`, and submits. Resolves once the
   * comment appears in the feed.
   */
  mentionsPostComment(displayName: string, bodyText: string): Promise<void>;
  /**
   * Member references rendered inside saved comments: the chip text plus the
   * member profile link target (null when the chip links nowhere).
   */
  mentionsVisibleReferences(): Promise<{ text: string; href: string | null }[]>;
  /**
   * Edit the comment showing `oldBodyText` so its mention becomes plain
   * `plainText`, and save. Resolves once the feed shows the edited comment.
   */
  mentionsEditRemovingMention(oldBodyText: string, plainText: string): Promise<void>;
  /**
   * Sign in through the UI and verify the session landed. Retries the
   * shared flow because the scratch stack rejects valid credentials
   * intermittently under parallel parity runs.
   */
  mentionsEnsureSignedIn(email: string, password: string): Promise<void>;

  // -------------------------------------------------------------------------
  // Workspace onboarding + creation (NEWFRONT-111, rows AUTH-034..043).
  // Sign-in itself is another area's row, so onboarding scenarios enter the
  // app pre-authenticated by injecting a minted user's session cookies, then
  // drive the onboarding/creation UI through these actions and reads.
  // -------------------------------------------------------------------------

  /** Enter the app at `path` already signed in as the owner of `cookies`. */
  openAuthenticated(path: string, cookies: ParityBrowserCookie[]): Promise<void>;
  /** The current location's path (what the address bar shows). */
  currentPath(): Promise<string>;
  /** Whether any element with this visible text is present. */
  hasVisibleText(text: string): Promise<boolean>;

  // --- create-or-join step (AUTH-034/035/038/043) ---
  /** Wait until the create-or-join step has settled on a sub-view. */
  awaitWorkspaceStep(): Promise<void>;
  /** Which create-or-join sub-view is currently visible. */
  visibleWorkspaceView(): Promise<WorkspaceOnboardingView>;
  fillWorkspaceName(name: string): Promise<void>;
  fillWorkspaceSlug(slug: string): Promise<void>;
  /** The normalized value currently shown in the workspace URL/slug field. */
  workspaceSlugValue(): Promise<string>;
  /** Pick a team-size pill by its visible label (onboarding create view). */
  selectTeamSizePill(label: string): Promise<void>;
  /** Pick a team-size option by label from the standalone dropdown. */
  selectTeamSizeDropdown(label: string): Promise<void>;
  submitCreateWorkspace(): Promise<void>;
  isCreateWorkspaceSubmitDisabled(): Promise<boolean>;
  /** The visible inline slug error, or null when none is shown. */
  workspaceSlugErrorText(): Promise<string | null>;
  /** From the create view, open the join-by-admin-email sub-view. */
  gotoJoinByEmailFromCreate(): Promise<void>;
  /** From the create view, open the invites sub-view (only when invites exist). */
  gotoInvitesFromCreate(): Promise<void>;

  // --- join-by-email + pending (AUTH-036) ---
  fillWorkspaceAdminEmail(email: string): Promise<void>;
  submitJoinRequest(): Promise<void>;
  /** Whether the pending/holding view names the given admin email. */
  pendingApprovalNamesEmail(email: string): Promise<boolean>;
  /** From the pending or join-by-email view, switch to creating a workspace. */
  createInsteadFromPending(): Promise<void>;

  // --- invites list (AUTH-034) ---
  selectInviteByWorkspace(workspaceName: string): Promise<void>;
  continueWithSelectedInvites(): Promise<void>;

  // --- invite-members step (AUTH-037/039) ---
  awaitInviteMembersStep(): Promise<void>;
  isInviteMembersStepVisible(): Promise<boolean>;
  inviteRowCount(): Promise<number>;
  fillInviteRow(index: number, email: string): Promise<void>;
  clickAddAnotherInvite(): Promise<void>;
  isSendInvitesDisabled(): Promise<boolean>;
  sendInvites(): Promise<void>;
  deferInvites(): Promise<void>;

  // --- back navigation (AUTH-043) ---
  isOnboardingBackVisible(): Promise<boolean>;
  clickOnboardingBack(): Promise<void>;

  // --- first-run product tour (AUTH-042) ---
  isTourWelcomeVisible(): Promise<boolean>;
  declineTour(): Promise<void>;

  // --- creation-disabled states (AUTH-041) ---
  isStandaloneCreationDisabledVisible(): Promise<boolean>;
  isRequestInstanceAdminLinkVisible(): Promise<boolean>;
  isInOnboardingCreationDisabledNoticeVisible(): Promise<boolean>;

  // --- Auth sign-in core (NEWFRONT-107, AUTH-001/008). Appended; existing
  // --- methods above are untouched per the shared driver contract.

  /** Open the sign-in card with query params (error_code, invitation, email). */
  openSignInWithParams(params: Record<string, string>): Promise<void>;
  /** Submit the email step and wait until the card leaves it. */
  submitEmail(email: string): Promise<void>;
  /** Submit the password step; ends in a full page load (success or error). */
  submitPassword(password: string): Promise<void>;
  /** Which sign-in card step is currently visible. */
  authStep(): Promise<"email" | "password" | "code" | "unavailable" | "unknown">;
  /** Text of the dismissible error banner, or null when none shows. */
  bannerText(): Promise<string | null>;
  /** Dismiss the error banner. */
  dismissBanner(): Promise<void>;
  /** True when the card header names the given workspace (invitation match). */
  seesWorkspaceInviteHeader(workspaceName: string): Promise<boolean>;
  /** True when the generic sign-in header shows. */
  seesGenericSignInHeader(): Promise<boolean>;
  /** True when the generic sign-up header shows. */
  seesGenericSignUpHeader(): Promise<boolean>;
  /** True when the password step shows the confirm-password field (sign-up mode). */
  seesConfirmPassword(): Promise<boolean>;
  /** Label of the primary button on the password step, or null when absent. */
  passwordPrimaryButtonLabel(): Promise<string | null>;
  /** How the forgot-password entry presents: reset link vs explanation popover. */
  forgotPasswordEntry(): Promise<"link" | "popover" | "absent">;
  /** Open the forgot-password explanation and return its text (mail-less mode). */
  forgotPasswordPopoverText(): Promise<string | null>;
  /** True when the "sign in with unique code" secondary button shows. */
  seesUniqueCodeButton(): Promise<boolean>;
  /** Move from the password step to the code step. */
  requestUniqueCode(): Promise<void>;
  /** Label of the resend control on the code step, or null when absent. */
  resendCodeLabel(): Promise<string | null>;
  /** Click the resend control on the code step. */
  clickResendCode(): Promise<void>;
  /** Submit the code step; ends in a full page load (success or error). */
  submitCode(code: string): Promise<void>;
  /** Names of the visible third-party provider buttons (empty when none). */
  providerSignInButtons(): Promise<string[]>;
  /** Click a third-party provider button (starts a full-page provider flow). */
  clickProviderButton(name: string): Promise<void>;
  /** Clear the locked email on the password/code step, resetting the card. */
  clearEmail(): Promise<void>;
  /** True when the no-authentication-methods card shows. */
  seesNoAuthMethods(): Promise<boolean>;

  // --- Auth sign-up, recovery, guards, landing (NEWFRONT-108, AUTH-009/016).
  // --- Appended; existing methods above are untouched per the shared driver
  // --- contract. currentPath keeps the pathname-plus-search shape the guard
  // --- scenarios assert on (?next_path=); authStep keeps the wider union
  // --- (the sign-in core also asserts "email"/"unavailable"/"unknown").

  /** Open the sign-up card as a signed-out visitor, with optional query params. */
  openSignUp(params?: { email?: string; nextPath?: string }): Promise<void>;
  /** Submit the email-first step; resolves once the next step renders. */
  submitAuthEmail(email: string): Promise<void>;
  /** Value currently in the auth card's email field. */
  authEmailValue(): Promise<string>;
  /** Value of the card's carried return path, when one is present. */
  authNextPathValue(): Promise<string | null>;
  /** Fill the sign-up password fields and submit; ends on the post-auth redirect. */
  signUpWithPassword(password: string, confirmPassword: string): Promise<void>;
  /** Fill the unique-code field and submit; ends on the post-auth redirect. */
  submitUniqueCode(code: string): Promise<void>;
  /** Resend control on the unique-code step: enabled state plus label. */
  codeResendState(): Promise<{ disabled: boolean; label: string }>;
  /** Click the resend control on the unique-code step. */
  requestNewCode(): Promise<void>;
  /** Whether the password submit is currently enabled. */
  passwordSubmitEnabled(): Promise<boolean>;
  /** Fill the password fields and click submit without requiring a navigation. */
  fillPasswordFields(password: string, confirmPassword: string): Promise<void>;
  /** Click the password form submit and settle (navigation optional). */
  clickPasswordSubmit(): Promise<void>;
  /** Inline password-confirmation error text, or null when none shows. */
  passwordMismatchError(): Promise<string | null>;
  /** Dismissible auth banner text, or null when no banner shows. */
  authBanner(): Promise<string | null>;
  /** Wait for the auth banner to appear; resolves with its text. */
  waitForAuthBanner(): Promise<string>;
  /** Open the forgot-password page, optionally with a prefilled address. */
  openForgotPassword(email?: string): Promise<void>;
  /** Submit the forgot-password form; resolves with the toast text shown. */
  submitForgotPassword(email: string): Promise<string>;
  /** Forgot-password resend control: enabled state plus label. */
  forgotResendState(): Promise<{ disabled: boolean; label: string }>;
  /** Open the reset-password page with the emailed-link params. */
  openResetPassword(params: { uid: string; token: string; email: string }): Promise<void>;
  /** Fill the new-password fields on the reset/set form and submit. */
  submitNewPassword(password: string, confirmPassword: string): Promise<void>;
  /** Open the set-password page; requires a signed-in session. */
  openSetPassword(): Promise<void>;
  /** Open an arbitrary app path (guards and legacy redirects). */
  openPath(path: string): Promise<void>;

  // --- NEWFRONT-113 (rules): comment permissions, visibility, deep links. ---
  // Appended additively per the NEWFRONT-30 shared driver contract; every
  // method is mirrored as a throwing stub in drivers/web_new.
  /** Open one work item's detail screen; waits for the activity section. */
  rulesOpenIssueDetail(workspaceSlug: string, projectId: string, issueId: string): Promise<void>;
  /** Open the intake-screen variant for one triage row; waits for its feed. */
  rulesOpenIntakeIssue(workspaceSlug: string, projectId: string, inboxIssueId: string): Promise<void>;
  /** Rendered body text of one comment card; null while collapsed/hidden. */
  rulesCommentBodyText(commentId: string): Promise<string | null>;
  /** Overflow-menu option keys currently offered on one comment card. */
  rulesCommentMenuOptions(commentId: string): Promise<string[]>;
  /** Pick one overflow-menu option on one comment card. */
  rulesChooseCommentMenuOption(commentId: string, option: RulesCommentMenuOption): Promise<void>;
  /** Current clipboard text (grants clipboard permission first). */
  rulesReadClipboard(): Promise<string>;
  /** Open an absolute-or-relative deep link; waits for page load. */
  rulesOpenDeepLink(url: string): Promise<void>;
  /** Whether the comment card currently carries the anchor highlight. */
  rulesCommentHighlighted(commentId: string): Promise<boolean>;
  /** Corner visibility marker on one comment card. */
  rulesCommentAccessBadge(commentId: string): Promise<"internal" | "public" | "hidden">;
  /** Most recently shown toast, if any is still visible. */
  rulesLastToast(): Promise<{ title: string; message: string } | null>;
  /** Reload the current page and wait for it to settle. */
  rulesReload(): Promise<void>;
  /**
   * Sign in through the base flow and confirm the workspace landing,
   * retrying the whole pass when a loaded dev server swallows the submit.
   * The base sign-in wait resolves on the entry URL itself, so the landing
   * check lives here instead of touching the shared method.
   */
  rulesEnsureSignedIn(email: string, password: string, workspaceSlug: string): Promise<void>;
  /**
   * Navigate to one work item without waiting for its activity section.
   * Refused viewers (a guest on someone else's item) never render the
   * section, so the raw open is their only entry point.
   */
  rulesOpenIssueDetailRaw(workspaceSlug: string, projectId: string, issueId: string): Promise<void>;
  /** Whether the work-item missing empty state is currently shown. */
  rulesIssueMissingVisible(): Promise<boolean>;
  /** Whether the comment composer is currently offered on the open item. */
  rulesCommentComposerVisible(): Promise<boolean>;
  /** Whether one comment card is present in the DOM (no waiting: callers poll). */
  rulesCommentCardVisible(commentId: string): Promise<boolean>;
  /** Full rendered text of one comment card (body plus reactions row). */
  rulesCommentCardText(commentId: string): Promise<string>;
  /** Whether the intake triage chrome (Accept/Decline) is currently offered. */
  rulesIntakeTriageVisible(): Promise<boolean>;

  // --- NEWFRONT-117 (layouts A): shared layout switching, list, spreadsheet,
  // --- calendar, row actions, empty states. Appended; existing methods above
  // --- are untouched per the shared driver contract.

  /** Layout keys the header switcher offers, in display order. */
  layoutsOfferedLayouts(): Promise<LayoutsLayoutKey[]>;
  /** Which layout is currently active. */
  layoutsActiveLayout(): Promise<LayoutsLayoutKey>;
  /** Switch to a layout through the header switcher; resolves once it renders. */
  layoutsSwitchTo(layout: LayoutsLayoutKey): Promise<void>;
  /** Reload the page and wait for the issues area to settle. */
  layoutsReloadIssues(): Promise<void>;
  /** Whether the list layout is rendered. */
  layoutsListVisible(): Promise<boolean>;
  /** Whether the calendar layout is rendered. */
  layoutsCalendarVisible(): Promise<boolean>;
  /** Whether the spreadsheet layout is rendered. */
  layoutsSpreadsheetVisible(): Promise<boolean>;
  /** Whether the board layout is rendered. */
  layoutsKanbanVisible(): Promise<boolean>;
  /** Whether the timeline layout is rendered. */
  layoutsGanttVisible(): Promise<boolean>;
  /** Group section titles in display order. */
  layoutsListGroups(): Promise<string[]>;
  /** Whether a group section is currently expanded. */
  layoutsListGroupExpanded(title: string): Promise<boolean>;
  /** Toggle a group section collapsed/expanded; resolves once it settles. */
  layoutsListToggleGroup(title: string): Promise<void>;
  /** Issue names rendered inside one group section. */
  layoutsListGroupIssueNames(title: string): Promise<string[]>;
  /** Whether a group shows its explicit "load more" row. */
  layoutsListGroupHasLoadMore(title: string): Promise<boolean>;
  /** Activate a group's "load more" row; resolves once it settles. */
  layoutsListGroupLoadMore(title: string): Promise<void>;
  /** Scroll the flat list to its end so the next page auto-loads. */
  layoutsListScrollEnd(): Promise<void>;
  /**
   * Create an issue through a list quick-add form (first group unless
   * `groupTitle` names one); resolves once the row shows.
   */
  layoutsListQuickAdd(title: string, groupTitle?: string): Promise<void>;
  /** Whether a row's state control opens its dropdown (edit gating). */
  layoutsRowCanEditState(issueName: string): Promise<boolean>;
  /** Href of a row link (null when the row is not a link). */
  layoutsRowHref(issueName: string): Promise<string | null>;
  /** Open peek by clicking a row; resolves once the peek panel shows the issue. */
  layoutsRowOpenPeek(issueName: string): Promise<void>;
  /** Whether the peek panel currently shows an issue. */
  layoutsPeekVisible(): Promise<boolean>;
  /** Title shown in the peek panel, or null when hidden. */
  layoutsPeekTitle(): Promise<string | null>;
  /** Close the peek panel. */
  layoutsPeekClose(): Promise<void>;
  /** Whether a row offers a sub-issue expander. */
  layoutsRowHasSubIssueToggle(issueName: string): Promise<boolean>;
  /** Expand a row's sub-issues; resolves once children render. */
  layoutsRowExpandSubIssues(issueName: string): Promise<void>;
  /** Names of the expanded sub-issues under a row. */
  layoutsRowSubIssueNames(issueName: string): Promise<string[]>;
  /** State label rendered on a row. */
  layoutsRowState(issueName: string): Promise<string>;
  /** Change a row's state through its inline dropdown. */
  layoutsRowSetState(issueName: string, stateName: string): Promise<void>;
  /** Priority label rendered on a row. */
  layoutsRowPriority(issueName: string): Promise<string>;
  /** Change a row's priority through its inline dropdown. */
  layoutsRowSetPriority(issueName: string, priorityName: string): Promise<void>;
  /** Menu entries offered by a row's quick-actions menu. */
  layoutsRowMenuItems(issueName: string): Promise<string[]>;
  /** Pick one entry of a row's quick-actions menu. */
  layoutsRowMenuChoose(issueName: string, item: string): Promise<void>;
  /** Menu entries offered by right-clicking a row. */
  layoutsRowContextMenuItems(issueName: string): Promise<string[]>;
  /** Open-in-new-tab from a row menu; returns the new tab's URL. */
  layoutsRowMenuOpenNewTabUrl(issueName: string): Promise<string>;

  // --- Spreadsheet (ISS-015..020). ---

  /** Column header titles in display order (first is the work-item column). */
  layoutsSheetHeaders(): Promise<string[]>;
  /** Issue names rendered as sheet rows, top to bottom. */
  layoutsSheetRowNames(): Promise<string[]>;
  /** Whether the first column sticks to the left edge. */
  layoutsSheetFirstColumnSticky(): Promise<boolean>;
  /** Whether the first column currently carries the scroll shadow. */
  layoutsSheetFirstColumnShadowed(): Promise<boolean>;
  /** Scroll the sheet horizontally to its right end. */
  layoutsSheetScrollRight(): Promise<void>;
  /** Whether the header row sticks to the top. */
  layoutsSheetHeaderSticky(): Promise<boolean>;
  /** Text rendered in one property cell. */
  layoutsSheetCellText(issueName: string, column: string): Promise<string>;
  /** Whether a cell opens an editor (edit gating per cell). */
  layoutsSheetCellEditable(issueName: string, column: string): Promise<boolean>;
  /** Change a row's state through its sheet cell. */
  layoutsSheetCellSetState(issueName: string, stateName: string): Promise<void>;
  /** Change a row's priority through its sheet cell. */
  layoutsSheetCellSetPriority(issueName: string, priorityName: string): Promise<void>;
  /** Change a row's due date through its sheet cell (ISO date, YYYY-MM-DD). */
  layoutsSheetCellSetDueDate(issueName: string, isoDate: string): Promise<void>;
  /** Assign a member through the sheet assignee cell (by display name). */
  layoutsSheetCellSetAssignee(issueName: string, memberName: string): Promise<void>;
  /** Focus one sheet cell through the keyboard path. */
  layoutsSheetFocusCell(issueName: string, column: string): Promise<void>;
  /** Press an arrow key while a sheet cell holds focus. */
  layoutsSheetPressArrow(arrow: "up" | "down" | "left" | "right"): Promise<void>;
  /** Which sheet cell currently holds keyboard focus (null when none). */
  layoutsSheetFocusedCell(): Promise<{ issueName: string; column: string } | null>;
  /** Sort menu entries offered by a column header. */
  layoutsSheetSortMenu(column: string): Promise<string[]>;
  /** Pick ascending/descending in a column's sort menu. */
  layoutsSheetSort(column: string, direction: "ascending" | "descending"): Promise<void>;
  /** Clear a column's sort through its header menu. */
  layoutsSheetClearSort(column: string): Promise<void>;
  /** Active sort marker on a header: ascending, descending, or none. */
  layoutsSheetSortMarker(column: string): Promise<"ascending" | "descending" | "none">;
  /** Create an issue through the sheet's sticky add row. */
  layoutsSheetQuickAdd(title: string): Promise<void>;
  /** Scroll the sheet to its end so the next page auto-loads. */
  layoutsSheetScrollEnd(): Promise<void>;
  /** Whether a sheet row offers a sub-issue expander. */
  layoutsSheetHasSubIssueToggle(issueName: string): Promise<boolean>;
  /** Expand a sheet row's sub-issues; resolves once children render. */
  layoutsSheetExpandSubIssues(issueName: string): Promise<void>;
  /** Names of the expanded sub-issues under a sheet row. */
  layoutsSheetSubIssueNames(issueName: string): Promise<string[]>;
  /** Activate the sub-issue-count cell (navigates to the sub-issues view). */
  layoutsSheetOpenSubIssueCount(issueName: string): Promise<void>;

  // --- Calendar (ISS-021..027). Day tiles are addressed by day number. ---

  /** Active calendar mode (month/week) as the Options menu reports it. */
  layoutsCalMode(): Promise<"month" | "week">;
  /** Title text in the calendar header (month name or week range). */
  layoutsCalTitle(): Promise<string>;
  /** Step the calendar back one month/week; resolves once it settles. */
  layoutsCalPrev(): Promise<void>;
  /** Step the calendar forward one month/week; resolves once it settles. */
  layoutsCalNext(): Promise<void>;
  /** Jump the calendar back to today; resolves once it settles. */
  layoutsCalToday(): Promise<void>;
  /** Month names offered by the header title picker. */
  layoutsCalMonthPickerMonths(): Promise<string[]>;
  /** Year currently shown in the open month picker. */
  layoutsCalMonthPickerYear(): Promise<number>;
  /** Step the open month picker's year. */
  layoutsCalMonthPickerYearStep(direction: "prev" | "next"): Promise<void>;
  /** Pick a month in the open month picker; resolves once the grid settles. */
  layoutsCalMonthPickerChoose(month: string): Promise<void>;
  /** Whether the month picker can be opened (disabled in week view). */
  layoutsCalMonthPickerEnabled(): Promise<boolean>;
  /** Switch month/week through the Options menu. */
  layoutsCalSetMode(mode: "month" | "week"): Promise<void>;
  /** Whether weekend columns are currently shown. */
  layoutsCalWeekendsVisible(): Promise<boolean>;
  /** Toggle weekend columns through the Options menu. */
  layoutsCalSetWeekends(show: boolean): Promise<void>;
  /** Number of day columns in the current grid (7 or 5). */
  layoutsCalColumnCount(): Promise<number>;
  /** Issue titles on one day tile. */
  layoutsCalDayIssueNames(dayNumber: number): Promise<string[]>;
  /** Whether a day tile carries the today badge. */
  layoutsCalDayIsToday(dayNumber: number): Promise<boolean>;
  /** Whether a day tile offers "load more". */
  layoutsCalDayHasLoadMore(dayNumber: number): Promise<boolean>;
  /** Activate a day's "load more"; resolves once it settles. */
  layoutsCalDayLoadMore(dayNumber: number): Promise<void>;
  /** Drag a dated block onto another day; resolves once it settles. */
  layoutsCalDragBlock(issueName: string, toDayNumber: number): Promise<void>;
  /** Text rendered on one calendar block (identifier + title). */
  layoutsCalBlockText(issueName: string): Promise<string>;
  /** Whether hovering a block reveals its preview popover. */
  layoutsCalBlockHoverPreview(issueName: string): Promise<boolean>;
  /** Open peek by clicking a calendar block. */
  layoutsCalBlockOpenPeek(issueName: string): Promise<void>;
  /** Quick-action entries offered on a calendar block. */
  layoutsCalBlockQuickActions(issueName: string): Promise<string[]>;
  /** Create an issue through a day tile's add control (due that day). */
  layoutsCalDayQuickAdd(dayNumber: number, title: string): Promise<void>;
  /** Add entries offered by a day tile's menu. */
  layoutsCalDayAddMenu(dayNumber: number): Promise<string[]>;
  /** Open a day tile's add-existing flow; resolves once the modal shows. */
  layoutsCalDayAddExisting(dayNumber: number): Promise<void>;
  /** Tap a day tile (mobile day-detail); resolves once the list settles. */
  layoutsCalTapDay(dayNumber: number): Promise<void>;
  /** Issue names in the mobile day-detail list under the grid. */
  layoutsCalDayDetailNames(): Promise<string[]>;

  // --- Row-action depth (ISS-060..067). ---

  /** Whether a row-menu entry is disabled. */
  layoutsRowMenuItemDisabled(issueName: string, item: string): Promise<boolean>;
  /** Explanatory note under a row-menu entry (null when none). */
  layoutsRowMenuItemNote(issueName: string, item: string): Promise<string | null>;
  /** Whether the create/edit work-item modal is currently open. */
  layoutsWorkItemModalVisible(): Promise<boolean>;
  /** Title text of the open work-item modal (null when closed). */
  layoutsWorkItemModalTitle(): Promise<string | null>;
  /** Whether the open work-item modal shows a text (prefill assertion). */
  layoutsWorkItemModalHasText(text: string): Promise<boolean>;
  /** Close the open work-item modal. */
  layoutsWorkItemModalClose(): Promise<void>;
  /** Whether the delete-confirm modal is currently open. */
  layoutsDeleteModalVisible(): Promise<boolean>;
  /** Confirm the open delete modal; resolves once it closes. */
  layoutsDeleteModalConfirm(): Promise<void>;
  /** Whether the archive-confirm modal is currently open. */
  layoutsArchiveModalVisible(): Promise<boolean>;
  /** Confirm the open archive modal; resolves once it closes. */
  layoutsArchiveModalConfirm(): Promise<void>;
  /** Whether the move-to-project modal is currently open. */
  layoutsMoveModalVisible(): Promise<boolean>;
  /** Pick a project in the open move modal; resolves once it closes. */
  layoutsMoveModalChoose(projectName: string): Promise<void>;
  /** Whether the add-existing-issues modal is currently open. */
  layoutsAddExistingModalVisible(): Promise<boolean>;
  /** Issue names currently listed in the add-existing modal. */
  layoutsAddExistingModalIssueNames(): Promise<string[]>;
  /** Pick an issue in the add-existing modal; resolves once it closes. */
  layoutsAddExistingModalChoose(issueName: string): Promise<void>;
  /** Menu entries in the issue detail/peek header menu. */
  layoutsDetailMenuItems(): Promise<string[]>;
  /** Pick one entry of the detail/peek header menu. */
  layoutsDetailMenuChoose(item: string): Promise<void>;
  /** Whether the peek header carries its own copy-link button. */
  layoutsPeekCopyLinkVisible(): Promise<boolean>;
  /** Menu entries in the whole-list ellipsis menu. */
  layoutsListPageMenuItems(): Promise<string[]>;
  /** Pick one entry of the whole-list ellipsis menu. */
  layoutsListPageMenuChoose(item: string): Promise<void>;
  /** Group-header add entries in cycle/module context (null opens the modal directly). */
  layoutsGroupHeaderAddMenu(groupTitle: string): Promise<string[] | null>;
  /** Activate a group-header add entry (null clicks the bare plus). */
  layoutsGroupHeaderAddChoose(groupTitle: string, item: string | null): Promise<void>;

  // --- Empty states (ISS-068..074). ---

  /** Title of the rendered empty state (null when the layout renders). */
  layoutsEmptyTitle(): Promise<string | null>;
  /** Action buttons of the rendered empty state: label + disabled. */
  layoutsEmptyActions(): Promise<Array<{ label: string; disabled: boolean }>>;
  /** Click one empty-state action; resolves once it settles. */
  layoutsEmptyChoose(label: string): Promise<void>;

  // --- Shared leftovers (ISS-003..006): mobile header, loaders, highlight. ---

  /** Layout keys the compact (mobile) layout selector offers. */
  layoutsMobileOfferedLayouts(): Promise<LayoutsLayoutKey[]>;
  /** Switch layout through the compact (mobile) selector. */
  layoutsMobileSwitchTo(layout: LayoutsLayoutKey): Promise<void>;
  /** Whether the mobile header's Display control is present. */
  layoutsMobileDisplayVisible(): Promise<boolean>;
  /** Whether the mobile header's Analytics button is present. */
  layoutsMobileAnalyticsVisible(): Promise<boolean>;
  /** Whether the Display dropdown's cycle/module options are disabled. */
  layoutsMobileDisplayCycleModuleDisabled(): Promise<{ cycleDisabled: boolean; moduleDisabled: boolean }>;
  /** Whether a layout skeleton is currently rendered. */
  layoutsSkeletonVisible(): Promise<boolean>;
  /** Whether the floating mutation spinner is currently shown. */
  layoutsMutationSpinnerVisible(): Promise<boolean>;
  /** Whether a row carries the fresh/drop highlight marker. */
  layoutsRowHighlighted(issueName: string): Promise<boolean>;
  /** Whether an unconfirmed (pulsing temp) row is currently rendered. */
  layoutsTempRowVisible(): Promise<boolean>;
  /** Stall issue-list GETs by ms (loader scenarios; release with layoutsReleaseStalls). */
  layoutsStallIssuesGet(delayMs: number): Promise<void>;
  /** Stall issue POST/PATCH by ms (mutation/temp-row scenarios). */
  layoutsStallIssueMutation(delayMs: number): Promise<void>;
  /** Release any stalls installed by the two methods above. */
  layoutsReleaseStalls(): Promise<void>;
  /** Current page URL (navigation assertions). */
  layoutsCurrentUrl(): Promise<string>;
}

/** Overflow-menu option keys the rules specs exercise (stable keys, not labels). */
export type RulesCommentMenuOption = "edit" | "copy_link" | "access_switch" | "fold" | "unfold" | "delete";

/** Canonical issue-layout keys shared by both frontend drivers. */
export type LayoutsLayoutKey = "list" | "kanban" | "calendar" | "spreadsheet" | "gantt";
