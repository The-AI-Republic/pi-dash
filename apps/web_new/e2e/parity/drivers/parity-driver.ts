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

  // --- NEWFRONT-118 (layouts B): kanban board + gantt timeline
  // --- (ISS-028/059). Appended; existing methods above are untouched per
  // --- the shared driver contract.

  /** Switch to the board through the header switcher; resolves once it renders. */
  kanbanOpenBoard(): Promise<void>;
  /** Whether the board layout is rendered. */
  kanbanBoardVisible(): Promise<boolean>;
  /** Switch to the timeline through the header switcher; resolves once it renders. */
  ganttOpenTimeline(): Promise<void>;
  /** Whether the timeline layout is rendered. */
  ganttTimelineVisible(): Promise<boolean>;
  /** Which layout the header switcher marks active. */
  boardActiveLayout(): Promise<BoardLayoutKey>;
  /** Reload the issues page and wait for the switcher to settle. */
  boardReloadIssues(): Promise<void>;
  /** Group columns in display order: value id, header name, live count. */
  kanbanColumns(): Promise<KanbanColumn[]>;
  /** Swimlanes in display order; empty when the board is not sub-grouped. */
  kanbanSwimlanes(): Promise<KanbanColumn[]>;
  /** Every rendered card: issue id, title, and group/sub-group value ids. */
  kanbanCards(): Promise<KanbanCard[]>;
  /** Card titles in one flat column, top to bottom. */
  kanbanColumnCards(columnName: string): Promise<string[]>;
  /** Toggle one flat column collapsed/expanded; resolves once it settles. */
  kanbanToggleColumn(columnName: string): Promise<void>;
  /** Whether one flat column is currently collapsed. */
  kanbanColumnCollapsed(columnName: string): Promise<boolean>;
  /** Toggle one swimlane's cards; resolves once it settles. */
  kanbanToggleSwimlane(laneName: string): Promise<void>;
  /** Whether one swimlane's cards are currently hidden. */
  kanbanSwimlaneCollapsed(laneName: string): Promise<boolean>;
  /** Identifier rendered on a card (e.g. PAR-12), or null when hidden. */
  kanbanCardIdentifier(issueName: string): Promise<string | null>;
  /** Whether a card renders its wrapped display-property chips. */
  kanbanCardShowsProperties(issueName: string): Promise<boolean>;
  /** Hover a card so its hover-only controls reveal. */
  kanbanCardHover(issueName: string): Promise<void>;
  /** Whether a card currently offers its quick-actions entry. */
  kanbanCardQuickActionsVisible(issueName: string): Promise<boolean>;
  /** Href of a card link (null when the card is not a link). */
  kanbanCardHref(issueName: string): Promise<string | null>;
  /** Open peek by clicking a card; resolves once the peek panel shows the issue. */
  kanbanOpenCardPeek(issueName: string): Promise<void>;
  /** Whether the peek panel currently shows an issue. */
  issuePeekVisible(): Promise<boolean>;
  /** Title shown in the peek panel, or null when hidden. */
  issuePeekTitle(): Promise<string | null>;
  /** Close the peek panel. */
  issuePeekClose(): Promise<void>;
  /** Whether one column ends with a quick-add entry. */
  kanbanColumnHasQuickAdd(columnName: string): Promise<boolean>;
  /** Create an issue through one column's quick-add; resolves once its card shows. */
  kanbanQuickAdd(columnName: string, title: string): Promise<void>;
  /** Whether one group header offers the create (+) entry. */
  kanbanHeaderCreateVisible(columnName: string): Promise<boolean>;
  /** Activate one group header's create entry; resolves once the modal or menu shows. */
  kanbanHeaderCreate(columnName: string): Promise<void>;
  /** Whether the create modal is currently open. */
  kanbanCreateModalVisible(): Promise<boolean>;
  /** Entries of one group header's create menu (cycle/module context). */
  kanbanHeaderMenuItems(columnName: string): Promise<string[]>;
  /** Pick one entry of one group header's create menu. */
  kanbanHeaderMenuChoose(columnName: string, item: string): Promise<void>;
  /** Reorder a card directly above another card; resolves once it settles. */
  kanbanDragCardBefore(sourceName: string, targetName: string): Promise<void>;
  /** Drop a card at the end of a column; resolves once it settles. */
  kanbanDragCardToColumnEnd(sourceName: string, columnName: string): Promise<void>;
  /** Drop a card on the delete zone; resolves once the confirm modal shows. */
  kanbanDragCardToDelete(sourceName: string): Promise<void>;
  /** Whether the delete-confirm modal is currently open. */
  kanbanDeleteModalVisible(): Promise<boolean>;
  /** Confirm the open delete modal; resolves once the card is gone. */
  kanbanConfirmDelete(): Promise<void>;
  /**
   * Hold a card over a column, read the drop overlay text (null when no
   * overlay shows), then release. The release drops the card: refused
   * drops move nothing, accepted drops apply — callers assert the card's
   * final place via kanbanCards.
   */
  kanbanDragHoldOverColumn(sourceName: string, columnName: string): Promise<{ overlay: string | null }>;
  /** Most recently shown toast still visible, or null when none shows. */
  boardLastToast(): Promise<{ title: string; message: string } | null>;
  /** Scroll one flat column to its bottom so the next page auto-loads. */
  kanbanColumnScrollEnd(columnName: string): Promise<void>;
  /** Whether one sub-grouped column shows its explicit load-more entry. */
  kanbanColumnHasLoadMore(columnName: string): Promise<boolean>;
  /** Activate one sub-grouped column's load-more entry; resolves once it settles. */
  kanbanColumnLoadMore(columnName: string): Promise<void>;
  /** Whether one column currently shows skeleton loaders. */
  kanbanColumnLoading(columnName: string): Promise<boolean>;
  /** Current scroll offsets of the board container. */
  kanbanBoardScroll(): Promise<{ x: number; y: number }>;
  /**
   * Press on a card and hold it near one board edge for holdMs, then
   * release. Callers compare kanbanBoardScroll before/after to observe
   * auto-scroll; the release may move the card.
   */
  kanbanDragHoldNearEdge(sourceName: string, edge: "left" | "right" | "top" | "bottom", holdMs: number): Promise<void>;
  /** Timeline header: live count, zoom entries, Today and fullscreen presence. */
  ganttHeader(): Promise<{ count: number | null; views: string[]; hasToday: boolean; hasFullscreen: boolean }>;
  /** Which zoom the timeline switcher marks active. */
  ganttActiveZoom(): Promise<GanttZoom | "unknown">;
  /** Switch the timeline zoom; resolves once the chart re-renders. */
  ganttSetZoom(view: GanttZoom): Promise<void>;
  /** Measured pixel width of one timeline day column. */
  ganttDayWidth(): Promise<number>;
  /** Whether weekend day columns render distinctly from weekdays. */
  ganttWeekendTinted(): Promise<boolean>;
  /** Weekday names starting each rendered week row, in order. */
  ganttWeekRowStarts(): Promise<string[]>;
  /** Activate Today; resolves once the chart re-centers. */
  ganttClickToday(): Promise<void>;
  /** Whether today's column is inside the viewport. */
  ganttTodayVisible(): Promise<boolean>;
  /** Whether today's column carries the highlight marker. */
  ganttTodayHighlighted(): Promise<boolean>;
  /** Toggle fullscreen; resolves once the mode settles. */
  ganttToggleFullscreen(): Promise<void>;
  /** Whether the chart currently renders in the fullscreen portal. */
  ganttFullscreenActive(): Promise<boolean>;
  /** Pixel width of the timeline items container. */
  ganttTimelineWidth(): Promise<number>;
  /** Horizontal scroll offset of the timeline container. */
  ganttScrollLeft(): Promise<number>;
  /** Scroll the timeline horizontally to an offset; resolves once it settles. */
  ganttScrollTo(x: number): Promise<void>;
  /** Sidebar rows top to bottom: identifier, name, duration label. */
  ganttSidebarRows(): Promise<GanttSidebarRow[]>;
  /** Open peek by clicking a sidebar row; resolves once the peek panel shows. */
  ganttOpenRowPeek(issueName: string): Promise<void>;
  /** Sidebar row titles top to bottom. */
  ganttSidebarOrder(): Promise<string[]>;
  /** Reorder a sidebar row directly above another; resolves once it settles. */
  ganttDragRowBefore(sourceName: string, targetName: string): Promise<void>;
  /** Whether an issue renders a dated bar (vs an empty row). */
  ganttBarExists(issueName: string): Promise<boolean>;
  /** Drag a bar body horizontally by whole days; resolves once it settles. */
  ganttDragBar(issueName: string, dayDelta: number): Promise<void>;
  /** Drag a bar edge handle by whole days; resolves once it settles. */
  ganttResizeBar(issueName: string, side: "left" | "right", dayDelta: number): Promise<void>;
  /** Hover a bar edge handle and read its floating date label (null when absent). */
  ganttResizePreview(issueName: string, side: "left" | "right"): Promise<string | null>;
  /** Whether a bar offers its resize affordances. */
  ganttHandlesVisible(issueName: string): Promise<boolean>;
  /** Whether hovering an issue's empty timeline row reveals the add entry. */
  ganttRowAddVisible(issueName: string): Promise<boolean>;
  /** Plant a block on an undated issue's row at a visible day offset; resolves once the bar shows. */
  ganttAddBlock(issueName: string, dayOffset: number): Promise<void>;
  /** Create an issue through the timeline quick-add; resolves once its bar shows. */
  ganttQuickAdd(title: string): Promise<void>;
  /** Whether the timeline offers its quick-add entry. */
  ganttHasQuickAdd(): Promise<boolean>;
  /** Bar presentation: state tint, half-date mask, pinned name (null when no bar). */
  ganttBarInfo(issueName: string): Promise<{ tinted: boolean; masked: boolean; namePinned: boolean } | null>;
  /** Hover a bar so its hover-only preview opens. */
  ganttHoverBar(issueName: string): Promise<void>;
  /** Whether the bar hover preview is currently open. */
  ganttPreviewVisible(): Promise<boolean>;
  /** Open peek by clicking a bar; resolves once the peek panel shows. */
  ganttOpenBarPeek(issueName: string): Promise<void>;
  /** Whether an issue's row shows the scroll-to-block arrow. */
  ganttScrollArrowVisible(issueName: string): Promise<boolean>;
  /** Activate an issue's scroll-to-block arrow; resolves once the bar is in view. */
  ganttClickScrollArrow(issueName: string): Promise<void>;
  /** Whether an issue's bar is inside the viewport. */
  ganttBarInView(issueName: string): Promise<boolean>;
  /** Whether the sidebar currently shows skeleton rows. */
  ganttSidebarLoading(): Promise<boolean>;
  /** Whether the load-more sentinel currently shows. */
  ganttLoadMoreVisible(): Promise<boolean>;
}

/** Overflow-menu option keys the rules specs exercise (stable keys, not labels). */
export type RulesCommentMenuOption = "edit" | "copy_link" | "access_switch" | "fold" | "unfold" | "delete";

/** Canonical issue-layout keys for the board/timeline driver area. */
export type BoardLayoutKey = "list" | "kanban" | "calendar" | "spreadsheet" | "gantt";

/**
 * One kanban group column or swimlane: value id, header name, live count.
 * `rendered` is false while the column body is not mounted (collapsed or
 * still virtualized away); `id` is empty then because only the body
 * carries the value id.
 */
export interface KanbanColumn {
  id: string;
  name: string;
  count: number;
  rendered: boolean;
}

/** One rendered kanban card: issue id, title, group/sub-group value ids. */
export interface KanbanCard {
  issueId: string;
  name: string;
  groupId: string;
  subGroupId: string;
}

/** Timeline zoom entries as the switcher labels them. */
export type GanttZoom = "Week" | "Month" | "Quarter";

/** One gantt sidebar row: identifier, name, duration label. */
export interface GanttSidebarRow {
  identifier: string | null;
  name: string;
  duration: string | null;
}
