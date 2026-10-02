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
  /** Bot identity for the CMT-012 automated-author step (NEWFRONT-112). */
  botEmail?: string;
  botPassword?: string;
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

  // Issue multi-select and bulk operations (NEWFRONT-120, ISS-108–116).
  // In builds where selection is disabled these reads stay at their
  // empty values: no checkboxes, no bulk bar.
  /** Row and group-header selection checkboxes currently rendered. */
  selectionCheckboxCount(): Promise<number>;
  /** Whether any sticky bulk bar (functional toolbar or upgrade banner) shows. */
  bulkBarVisible(): Promise<boolean>;
  /** Press a key as the user would; Shift held when `shift` is set. */
  pressKey(key: string, shift?: boolean): Promise<void>;
  /** Reload and report whether a leave-confirmation dialog appeared. */
  reloadSawDialog(): Promise<boolean>;

  // Create/edit work-item modal (NEWFRONT-120, ISS-117–136).
  /** Dismiss the first-run welcome dialog when it overlays the app. */
  dismissWelcomeDialog(): Promise<void>;
  /** Open the create work-item modal from the issues header. */
  openCreateModal(): Promise<void>;
  /** Whether the create/edit modal is currently open. */
  createModalOpen(): Promise<boolean>;
  /** Heading text of the open modal (create, edit, or draft title). */
  createModalHeading(): Promise<string>;
  /** Fill the modal title field. */
  fillCreateTitle(title: string): Promise<void>;
  /** Current value of the modal title field. */
  createTitleValue(): Promise<string>;
  /** Inline validation error under the title field, if any. */
  createTitleError(): Promise<string>;
  /** Press the modal primary button (Save / Update / Save to Drafts). */
  submitCreateModal(): Promise<void>;
  /** Press the modal Discard button. */
  clickModalDiscard(): Promise<void>;
  /** Switch the "Create more" toggle on (create only). */
  enableCreateMore(): Promise<void>;
  /** Label of the modal primary button. */
  modalPrimaryButtonLabel(): Promise<string>;
  /** Current value of the git work-branch field (section must be expanded). */
  gitBranchValue(): Promise<string>;
  /** Whether the modal title field currently holds keyboard focus. */
  createTitleFocused(): Promise<boolean>;
  /** Whether the given text shows inside the open modal. */
  modalTextContains(text: string): Promise<boolean>;
  /** Press "Save to Drafts" in the discard-confirm dialog. */
  confirmSaveDraft(): Promise<void>;
  /** Press "Cancel" in the discard-confirm dialog. */
  cancelDiscardDialog(): Promise<void>;
  /** Press the dialog's own "Discard" button, dropping the draft. */
  discardDialogDiscard(): Promise<void>;
  /** Open the named draft for editing (double-clicks its block). */
  openDraftForEdit(name: string): Promise<void>;
  /** Press "Publish issue" in the open draft modal. */
  publishDraft(): Promise<void>;
  /** Click the row's More menu and then the named entry (Edit, Delete, …). */
  openRowMenuEntry(issueName: string, entry: string): Promise<void>;
  /** Whether the modal button with the given accessible name is disabled. */
  modalButtonDisabled(name: string): Promise<boolean>;
  /** Open the parent picker from the modal ("Add parent"). */
  openParentPicker(): Promise<void>;
  /** Type into the parent picker search. */
  searchParentInModal(query: string): Promise<void>;
  /** Choose the parent search result showing the given issue name. */
  selectParentResult(issueName: string): Promise<void>;
  /** New-tab links rendered next to the parent search results. */
  parentResultNewTabLinks(): Promise<number>;
  /** Remove the attached parent through the parent chip menu. */
  removeParentInModal(issueName: string): Promise<void>;
  /** Open the labels picker from the modal strip. */
  openLabelsPicker(): Promise<void>;
  /** Type a label name and confirm, creating it when permitted. */
  createLabelInModal(name: string): Promise<void>;
  /** Whether the modal shows the label as selected. */
  selectedLabelVisible(name: string): Promise<boolean>;
  /** Hover the issue row (reveals the hover preview card). */
  hoverIssueRow(issueName: string): Promise<void>;
  /** Click the toast "View work item" action; returns the opened page URL. */
  openToastViewAction(): Promise<string>;
  /** Confirm the archive modal. */
  confirmArchive(): Promise<void>;
  /** Confirm the delete modal. */
  confirmDeleteIssue(): Promise<void>;
  /** Whether a modal dialog currently shows an input with `placeholder`. */
  modalHasPlaceholder(placeholder: string): Promise<boolean>;
  /** Type `text` into the modal input with `placeholder` (replacing it). */
  fillModalPlaceholder(placeholder: string, text: string): Promise<void>;
  /**
   * Click every rendered "Load more" row once so paginated list rows
   * render. Returns whether any existed. Newly created rows sort last
   * and hide behind group pagination on a busy shared stack.
   */
  expandListRows(): Promise<boolean>;
  /** Switch the issues view to the layout whose tooltip reads `label`. */
  switchIssueLayout(label: string): Promise<void>;
  /**
   * Best-effort return to the list layout. The active layout persists
   * across navigation on the shared stack, so list oracles call this
   * before asserting list content. Never throws: a dead page or a header
   * without the switcher simply leaves the layout alone.
   */
  ensureListLayout(): Promise<void>;
  /** Expand or collapse the "Advanced — git" section of the modal. */
  toggleAdvancedGit(): Promise<void>;
  /** Fill the git work-branch field (section must be expanded). */
  fillGitBranch(branch: string): Promise<void>;
  /** Inline validation error under the git branch field, if any. */
  gitBranchError(): Promise<string>;
  /** Type into the modal description editor. */
  fillDescription(text: string): Promise<void>;
  /** How many times the exact text occurs on the page. */
  countText(text: string): Promise<number>;

  // Workspace drafts (NEWFRONT-120, ISS-137–141).
  /** Open the workspace drafts page; requires an authenticated session. */
  openDraftsPage(workspaceSlug: string): Promise<void>;
  /** Names of the drafts currently rendered, in display order. */
  visibleDraftNames(): Promise<string[]>;
  /** Open the create-draft modal from the drafts empty state. */
  openCreateDraftModal(): Promise<void>;
  /** Whether the given text is visible anywhere on the page. */
  pageTextContains(text: string): Promise<boolean>;
  /** Delete the named draft through its quick actions and confirm. */
  deleteDraftByName(name: string): Promise<void>;
  /** How many draft blocks are rendered on the drafts page. */
  draftBlockCount(): Promise<number>;
  /** Full text of the draft block holding the named draft. */
  draftBlockText(name: string): Promise<string>;
  /** Wait for the drafts page to settle; re-enters the route when stuck. */
  settleDraftsPage(workspaceSlug: string): Promise<"empty" | "list">;
  /** Retry wrapper around the shared sign-in flow for the loaded shared stack. */
  signInWithPasswordRetry(email: string, password: string): Promise<void>;
  /** Settled list entry: reloads past stalls, returns to list layout, waits for life. */
  openProjectIssuesSettled(workspaceSlug: string, projectId: string): Promise<void>;
  /** Copy the named draft through its quick-action menu (opens the duplicated payload). */
  copyDraftByName(name: string): Promise<void>;
  /** Move the named draft toward a project through its quick-action menu. */
  moveDraftToProject(name: string): Promise<void>;
  /** Confirm the move-to-project modal ("Add to project"). */
  confirmMoveToProject(): Promise<void>;
  /** Project currently selected in the open issue modal. */
  modalProjectName(): Promise<string>;
  /** Select the named project in the open issue modal's picker. */
  selectModalProject(name: string): Promise<void>;
  /** Hover the named issue and read its preview card (empty fields when no card shows). */
  hoverCardRead(issueName: string): Promise<{ text: string; priorityIcon: string; dateColor: string }>;
  /** Tabindex order of the modal's focusable fields, as tag#index:label triples. */
  modalTabOrder(): Promise<string[]>;
  /** Focus the modal's title field without changing its value. */
  focusCreateTitle(): Promise<void>;
  /** Open a cycle page and wait until its Add action shows. */
  openCyclePage(workspaceSlug: string, projectId: string, cycleId: string): Promise<void>;
  /** Open a module page and wait until its Add action shows. */
  openModulePage(workspaceSlug: string, projectId: string, moduleId: string): Promise<void>;

  // --- NEWFRONT-123 (home): home dashboard reads and actions. ---
  // Added additively per the NEWFRONT-26 shared driver contract; every
  // method is mirrored as a throwing stub in drivers/web_new.
  /** Open the workspace home dashboard; waits until greeting or tour settles. */
  homeOpen(workspaceSlug: string): Promise<void>;
  /** Greeting heading text (salutation plus user name), if rendered. */
  homeGreetingHeading(): Promise<string | null>;
  /** Date-and-clock sub-line text under the greeting, if rendered. */
  homeDateLine(): Promise<string | null>;
  /** Whether the first-run tour overlay currently covers the dashboard. */
  homeTourVisible(): Promise<boolean>;
  /** Advance the tour one step with its primary forward control. */
  homeTourAdvance(): Promise<void>;
  /** Leave the tour with its skip/close control when one is offered. */
  homeTourDismiss(): Promise<void>;
  /** Assistant card state: hidden for guests, setup reminder without a model key, ready otherwise. */
  homeAssistantState(): Promise<"hidden" | "setup" | "ready">;
  /** Suggested-prompt texts offered by the assistant card (empty when hidden). */
  homeAssistantSuggestions(): Promise<string[]>;
  /** Whether the quickstart onboarding panel is currently shown. */
  homeQuickstartVisible(): Promise<boolean>;
  /** Titles of the quickstart cards currently rendered. */
  homeQuickstartTitles(): Promise<string[]>;
  /** Whether the quickstart panel offers an enabled project-creation action. */
  homeQuickstartCreateEnabled(): Promise<boolean>;
  /** Dismiss the quickstart panel for this workspace. */
  homeQuickstartDismiss(): Promise<void>;
  /** Whether the named quickstart card shows its completed state. */
  homeQuickstartCardDone(title: string): Promise<boolean>;
  /** Action texts currently offered inside the quickstart panel. */
  homeQuickstartActionTexts(): Promise<string[]>;
  /** Titles of the widgets currently rendered in the dashboard stack. */
  homeWidgetTitles(): Promise<string[]>;
  /** Open the manage-widgets dialog from the header control. */
  homeOpenManageWidgets(): Promise<void>;
  /** Close the manage-widgets dialog. */
  homeCloseManageWidgets(): Promise<void>;
  /** Widget names listed in the manage dialog, in display order. */
  homeManageWidgetNames(): Promise<string[]>;
  /** Whether the named widget's toggle is on in the manage dialog. */
  homeManageWidgetEnabled(name: string): Promise<boolean>;
  /** Flip the named widget's toggle in the manage dialog. */
  homeToggleManageWidget(name: string): Promise<void>;
  /** Drag the source widget row onto the target row in the manage dialog. */
  homeDragWidget(sourceName: string, targetName: string): Promise<void>;
  /** Whether the all-widgets-off guidance illustration is shown. */
  homeAllOffVisible(): Promise<boolean>;
  /** Display names of the saved reference links, in render order. */
  homeQuickLinkNames(): Promise<string[]>;
  /** Expand the collapsed reference-link list. */
  homeExpandQuickLinks(): Promise<void>;
  /** Whether the reference-link list currently offers a collapse expander. */
  homeQuickLinksCollapsed(): Promise<boolean>;
  /** Create a reference link through the dashboard dialog. */
  homeAddQuickLink(title: string, url: string): Promise<void>;
  /** Rename/retarget a reference link through the dashboard dialog. */
  homeEditQuickLink(currentTitle: string, nextTitle: string, nextUrl: string): Promise<void>;
  /** Delete a reference link from its row control. */
  homeDeleteQuickLink(title: string): Promise<void>;
  /** Inline validation text currently shown in the link dialog, if any. */
  homeLinkDialogError(): Promise<string | null>;
  /** Copy a reference link's address through its row control. */
  homeCopyQuickLink(title: string): Promise<void>;
  /** Current clipboard text (grants clipboard permission first). */
  homeReadClipboard(): Promise<string>;
  /** Open a reference link in a new tab; resolves with the popup URL. */
  homeOpenQuickLinkPopup(title: string): Promise<string | null>;
  /** Whether the link add/edit dialog is currently open. */
  homeLinkDialogOpen(): Promise<boolean>;
  /** Discard the link dialog through its cancel control. */
  homeCancelLinkDialog(): Promise<void>;
  /** Switch the recents feed to the named activity filter. */
  homeSetRecentsFilter(name: "all" | "issue" | "page" | "project"): Promise<void>;
  /** Full text of every recent-activity row currently rendered. */
  homeRecentRowTexts(): Promise<string[]>;
  /** Click a recent row identified by contained text. */
  homeOpenRecentRow(text: string): Promise<void>;
  /** Whether a work-item preview overlay is shown (in-place side panel). */
  homeIssuePreviewVisible(): Promise<boolean>;
  /** Combined preview text, including editable field values. */
  homeIssuePreviewText(): Promise<string>;
  /** Breadcrumb label identifying the dashboard screen, if rendered. */
  homeBreadcrumb(): Promise<string | null>;
  /** Most recently shown toast, if any is still visible. */
  homeLastToast(): Promise<{ title: string; message: string } | null>;
  /** Reload the current page and wait for it to settle. */
  homeReload(): Promise<void>;
  /** Open one work item's detail screen (also records a recent visit). */
  homeOpenIssueDetail(workspaceSlug: string, projectId: string, issueId: string): Promise<void>;
  /** Whether any widget loading skeleton is currently shown. */
  homeSkeletonVisible(): Promise<boolean>;
  /**
   * Widget titles once the stack renders, reloading once when a transient
   * fetch failure leaves the loader stuck. Callers assert on the result.
   */
  homeWaitForWidgets(): Promise<string[]>;
  // --- Comment composer and CRUD (NEWFRONT-112, rows CMT-001..012) ---
  // Appended additively per the shared driver contract on NEWFRONT-30:
  // never modify the methods above or another area's methods.
  /** Open a work item detail page by browse ref (e.g. "PAR-1"); ends with the activity section visible. */
  composerOpenIssue(workspaceSlug: string, issueRef: string): Promise<void>;
  /** Type text into the comment composer. */
  composerType(text: string): Promise<void>;
  /** Paste an HTML fragment into the composer as formatted rich text. */
  composerPasteHtml(html: string): Promise<void>;
  /** Current draft text in the composer. */
  composerDraftText(): Promise<string>;
  /** Whether the composer submit control is currently disabled. */
  composerSubmitDisabled(): Promise<boolean>;
  /** Submit the composer through its submit button. */
  composerSubmit(): Promise<void>;
  /** Press Enter with no modifiers while the composer is focused. */
  composerPressEnter(): Promise<void>;
  /** Press Shift+Enter while the composer is focused (newline, never a submit). */
  composerPressShiftEnter(): Promise<void>;
  /** Attach a file to the composer draft. */
  composerAttachFile(path: string): Promise<void>;
  /** Bodies of the comments currently rendered in the feed, in display order. */
  composerVisibleCommentTexts(): Promise<string[]>;
  /** Open the overflow menu of the comment card showing the given text. */
  composerOpenCommentMenu(text: string): Promise<void>;
  /** Click a menu item (e.g. "Edit", "Delete") in the open comment menu. */
  composerMenuClick(item: string): Promise<void>;
  /** Replace the inline edit form content with the given text. */
  composerEditType(text: string): Promise<void>;
  /** Whether the inline edit form's save control is currently disabled. */
  composerEditSaveDisabled(): Promise<boolean>;
  /** Save the inline edit form through its save control. */
  composerEditSave(): Promise<void>;
  /** Discard the inline edit form, restoring the original body. */
  composerEditDiscard(): Promise<void>;
  /** Press Enter with no modifiers while the edit form is focused. */
  composerEditPressEnter(): Promise<void>;
  /** Header facts for the card showing the given text. */
  composerCommentMeta(text: string): Promise<{
    author: string;
    time: string;
    edited: boolean;
    tooltip: string | null;
  }>;
  /** Images rendered in the body of the card showing the given text. */
  composerCommentImageCount(text: string): Promise<number>;
  /** Notices currently visible, oldest first, with their success/error kind. */
  composerVisibleNotices(): Promise<{ message: string; kind: "success" | "error" | "unknown" }[]>;
  // ---- Issue detail (NEWFRONT-121). ----
  /** Open the full-page detail of `IDENT-seq` (e.g. `PAR-1`); ends hydrated. */
  openIssueDetail(workspaceSlug: string, issueSeq: string): Promise<void>;
  /** Open a read-only detail of `IDENT-seq` (archived, or viewed without edit rights); ends with the sidebar hydrated. */
  openReadOnlyIssueDetail(workspaceSlug: string, issueSeq: string): Promise<void>;
  /** Title heading text on the detail page, or null when absent. */
  issueDetailTitle(): Promise<string | null>;
  /** Identifier line (e.g. `PAR-1`) on the detail page, or null when absent. */
  issueDetailIdentifier(): Promise<string | null>;
  /** Edit the title inline; ends with the save settled. */
  editIssueTitle(name: string): Promise<void>;
  /** Current save indicator text (`Saving…`/`Saved`), or null when hidden. */
  saveIndicator(): Promise<string | null>;
  /** Plain text of the description body, or null when the editor is absent. */
  descriptionText(): Promise<string | null>;
  /** Replace the description through the UI; ends with the save settled. */
  setDescription(text: string): Promise<void>;
  /** Value text of a sidebar property row named by its label, or null. */
  sidebarProperty(label: string): Promise<string | null>;
  /** True when a sidebar property row with the label renders (even with an empty value). */
  sidebarRowPresent(label: string): Promise<boolean>;
  /** True when the sidebar property row carries an editing control (dropdown trigger, picker). */
  sidebarRowHasControl(label: string): Promise<boolean>;
  /** Pick a state from the sidebar State dropdown. */
  pickState(name: string): Promise<void>;
  /** Pick a priority from the sidebar Priority dropdown. */
  pickPriority(name: string): Promise<void>;
  /** Click the header copy-link button. */
  copyIssueLink(): Promise<void>;
  /** Text of the most recent toast, or null when none shows. */
  lastToast(): Promise<string | null>;
  /** Clipboard text (the copy-link scenarios grant clipboard permission). */
  readClipboard(): Promise<string>;
  /** Current subscribe toggle label, or null when the toggle is absent. */
  subscribeToggle(): Promise<string | null>;
  /** Click the subscribe toggle; ends with the toggle settled. */
  clickSubscribeToggle(): Promise<void>;
  /** Names of the detail header `…` quick-action menu items. */
  quickActionNames(): Promise<string[]>;
  /** Click the named detail header `…` quick-action menu item. */
  clickQuickAction(name: string): Promise<void>;
  /** True when the named `…` quick-action menu item is disabled; ends with the menu closed. */
  quickActionDisabled(name: string): Promise<boolean>;
  /** Open the legacy short-link route that redirects to the detail page. */
  openLegacyIssueRoute(workspaceSlug: string, projectId: string, issueId: string): Promise<void>;
  /** True when the detail page shows the does-not-exist empty state. */
  seesDetailMissing(): Promise<boolean>;
  /** True when the current page shows authenticated app chrome (not the sign-in card). */
  signedIn(): Promise<boolean>;
  /** Open the "Last edited by" description-history menu; ends with versions listed. */
  openDescriptionHistory(): Promise<void>;
  /** Names of the description versions in the open history menu. */
  historyVersionNames(): Promise<string[]>;
  /** Restore the named history version through the preview modal; ends with the save settled. */
  restoreHistoryVersion(name: string): Promise<void>;
  /** Pick an assignee from the sidebar Assignees picker. */
  pickAssignee(displayName: string): Promise<void>;
  /** Names of the Runs-on execution-target options; ends with the dropdown closed. */
  runsOnOptions(): Promise<string[]>;
  /** Pick the named execution target from the sidebar Runs-on dropdown. */
  pickRunsOn(name: string): Promise<void>;
  /** Pick a day of the month in the sidebar date picker for the named row. */
  pickDate(label: string, day: string): Promise<void>;
  /** True when the named day cell is disabled in the currently open date picker. */
  calendarDayDisabled(day: string): Promise<boolean>;
  /** Clear the sidebar date row through its hover remove control. */
  clearDate(label: string): Promise<void>;
  /** Pick a cycle from the sidebar Cycle dropdown. */
  pickCycle(name: string): Promise<void>;
  /** Clear the sidebar Cycle row through its "No cycle" option. */
  clearCycle(): Promise<void>;
  /** Toggle a module in the sidebar Modules multi-dropdown; ends with the menu closed. */
  toggleModule(name: string): Promise<void>;
  /** Set the parent through the picker modal by searching for the named issue. */
  setParentByName(name: string): Promise<void>;
  /** Text of the parent banner pill (excluding the child's own identifier), or null when absent. */
  parentBanner(childSeq: string): Promise<string | null>;
  /** Names of the banner ellipsis menu items (sibling work items plus remove). */
  bannerMenuNames(childSeq: string): Promise<string[]>;
  /** Remove the parent through the banner menu. */
  removeParent(): Promise<void>;
  /** Follow the banner link to the parent's detail page; ends hydrated. */
  openParentFromBanner(): Promise<void>;
  /** Add a label through the sidebar Labels combobox (selects or creates). */
  addLabel(name: string): Promise<void>;
  /** Remove a label chip from the sidebar Labels row. */
  removeLabel(name: string): Promise<void>;

  // ---- Peek panel (NEWFRONT-121). ----
  /** Open the issues list with the peek panel on `issueId`; ends with the peek hydrated. */
  openPeek(workspaceSlug: string, projectId: string, issueId: string): Promise<void>;
  /** True while the peek panel is rendered. */
  peekOpen(): Promise<boolean>;
  /** Issue name shown in the peek, or null when the peek is absent. */
  peekTitle(): Promise<string | null>;
  /** Identifier (e.g. `PAR-1`) shown in the peek, or null when absent. */
  peekIdentifier(): Promise<string | null>;
  /** Close the peek through its header close control; ends with the panel gone. */
  closePeek(): Promise<void>;
  /** True when the peek header close control is rendered (even while loading). */
  peekCloseVisible(): Promise<boolean>;
  /** Click the named issue row in the list; ends once the URL or peek settles. */
  clickListRow(name: string): Promise<void>;
  /** Switch the peek display mode (`Side Peek` | `Modal` | `Full Screen`); ends applied. */
  setPeekMode(mode: string): Promise<void>;
  /** Bounding box of the peek panel, or null when absent. */
  peekPanelBox(): Promise<{ x: number; y: number; width: number; height: number } | null>;
  /** Click the peek header copy-link control. */
  copyPeekLink(): Promise<void>;
  /** Href of the peek "open full screen" control, or null when disabled/absent. */
  peekFullScreenHref(): Promise<string | null>;
  /** Open the peek header `…` menu; returns the item names. */
  peekQuickActionNames(): Promise<string[]>;
  /** Title of the peek error empty state, or null when the peek loaded. */
  peekErrorTitle(): Promise<string | null>;

  // ---- Detail widgets (NEWFRONT-121). ----
  /** Titles of the widget collapsibles currently rendered. */
  widgetTitles(): Promise<string[]>;
  /** Names of the rows in the named widget section. */
  widgetRowNames(widget: string): Promise<string[]>;
  /** Progress text (`<done>/<total> Done`) of the named widget, or null. */
  widgetProgress(widget: string): Promise<string | null>;
  /** Group titles inside the named widget (e.g. relation types), or []. */
  widgetGroupNames(widget: string): Promise<string[]>;
  /** Ensure the named widget section is expanded. */
  openWidgetSection(widget: string): Promise<void>;
  /** True when the named widget section is currently expanded. */
  widgetExpanded(widget: string): Promise<boolean>;
  /** Number of control buttons nested in the named widget's header row. */
  widgetHeaderControlCount(widget: string): Promise<number>;
  /** Toggle the named widget section open/closed; ends settled. */
  toggleWidgetSection(widget: string): Promise<void>;
  /** Option names of the named widget's add (`+`) menu. */
  widgetAddMenuNames(widget: string): Promise<string[]>;
  /** Open the sub-issue create modal through the widget; ends with the modal open. */
  openSubIssueCreateModal(): Promise<void>;
  /** Parent name preset in the open create modal, or null. */
  createModalParentName(): Promise<string | null>;
  /** True when the open create modal locks the project field. */
  createModalProjectLocked(): Promise<boolean>;
  /** Submit the open create modal with the given issue name; ends with the modal closed. */
  createModalSubmit(name: string): Promise<void>;
  /** Add an existing issue as a sub-issue: search, pick, confirm; ends with the row rendered. */
  addExistingSubIssue(search: string, name: string): Promise<void>;
  /** Click the named row in the named widget (opens child peek / attachment). */
  clickWidgetRow(widget: string, rowName: string): Promise<void>;
  /** Names of the named row's ellipsis-menu items in the named widget. */
  widgetRowActionNames(widget: string, rowName: string): Promise<string[]>;
  /** Click the named ellipsis-menu action for the named row in the named widget. */
  clickWidgetRowAction(widget: string, rowName: string, action: string): Promise<void>;
  /** Title of the open confirm modal, or null when no dialog shows. */
  confirmModalTitle(): Promise<string | null>;
  /** Full text of the open confirm modal, or null when no dialog shows. */
  confirmModalText(): Promise<string | null>;
  /** Click the named button in the open dialog; ends with the dialog closed. */
  confirmModal(label: string): Promise<void>;
  /** Add a relation through the widget: pick the type, search, pick, confirm. */
  addRelationViaModal(type: string, search: string, name: string): Promise<void>;
  /** Add an external link through the Links widget modal. */
  addLinkModal(url: string, title?: string): Promise<void>;
  /** Click the copy control of the named link row; ends with the URL copied. */
  clickLinkCopy(rowName: string): Promise<void>;
  /** Change the named link's title through the Links widget. */
  editLinkTitle(rowName: string, title: string): Promise<void>;
  /** Href/target of the named link row, or null. */
  linkRowTarget(rowName: string): Promise<{ href: string; target: string | null } | null>;
  /** Upload a file through the Attachments widget; ends with the row rendered. */
  uploadAttachment(file: { name: string; mime: string; bytes: Buffer }): Promise<void>;
  /** Click the widget action-row button (e.g. `Manually Run AI`); ends settled. */
  clickWidgetAction(name: string): Promise<void>;
  /** Post a comment through the composer; ends with the comment rendered. */
  postComment(text: string): Promise<void>;
  /** Type into the composer without submitting; the draft stays editable. */
  typeComment(text: string): Promise<void>;
  /** Click the `Comment & Run` composer control; ends settled. */
  clickCommentAndRun(): Promise<void>;
  /** True while Comment & Run is disabled (the empty-composer short-circuit). */
  commentAndRunDisabled(): Promise<boolean>;
  // --- Sidebar + workspace navigation (NEWFRONT-125, SHELL-046..062). Appended;
  // --- existing entries above are untouched per the shared driver contract.
  /** Click an entry of the open help menu; resolves with the popup URL when one opens. */
  activateHelpEntry(name: string): Promise<string | null>;
  /** Click an entry of the open user menu. */
  activateUserMenuItem(name: string): Promise<void>;
  /** Click the main content area (outside click for slide-overs and menus). */
  clickMainContent(): Promise<void>;
  /** Dismiss the topmost dialog or menu. */
  dismissTopmost(): Promise<void>;
  /** Drag one sidebar project row above another. */
  dragSidebarProjectBefore(sourceName: string, targetName: string): Promise<void>;
  /** Entry labels of the sidebar Favorites section, in display order. */
  favoriteEntryNames(): Promise<string[]>;
  /** Item labels of the currently open help menu. */
  helpMenuTexts(): Promise<string[]>;
  /** Whether the sidebar project-creation button is rendered. */
  isCreateProjectVisible(): Promise<boolean>;
  /** Whether a dialog containing the given text is visible. */
  isDialogWithTextVisible(text: string): Promise<boolean>;
  /** Whether the favorites folder dialog is open. */
  isFavoritesFolderDialogOpen(): Promise<boolean>;
  /** Whether the sidebar Favorites section is expanded. */
  isFavoritesOpen(): Promise<boolean>;
  /** Whether the secondary-destinations disclosure is expanded. */
  isMoreSectionOpen(): Promise<boolean>;
  /** Whether a project-creation button is rendered in the overflow slide-over. */
  isOverflowCreateVisible(): Promise<boolean>;
  /** Whether the overflow slide-over shows its no-match empty state. */
  isOverflowEmptyStateVisible(): Promise<boolean>;
  /** Whether a sidebar project row's link is currently inside the viewport. */
  isProjectRowInViewport(projectName: string): Promise<boolean>;
  /** Whether a sidebar project row's sub-navigation is expanded. */
  isProjectRowOpen(projectName: string): Promise<boolean>;
  /** Whether the sidebar Projects group is expanded. */
  isProjectsGroupOpen(): Promise<boolean>;
  /** Whether the projects overflow slide-over is open. */
  isProjectsOverflowOpen(): Promise<boolean>;
  /** Whether the Projects-group overflow toggle (extra projects) is rendered. */
  isProjectsOverflowVisible(): Promise<boolean>;
  /** Whether the quick-create dialog is open. */
  isQuickCreateDialogOpen(): Promise<boolean>;
  /** Whether the sidebar quick-create control is enabled. */
  isQuickCreateEnabled(): Promise<boolean>;
  /** True while the app sidebar is rendered on screen (false on full-width routes). */
  isSidebarOnScreen(): Promise<boolean>;
  /** Whether a toast or notice with the given text is visible. */
  isToastVisible(text: string): Promise<boolean>;
  /** Links of the secondary-destinations disclosure when expanded. */
  moreSectionLinks(): Promise<{ text: string; href: string | null }[]>;
  /** Follow a Favorites entry to its destination. */
  openFavoriteEntry(name: string): Promise<void>;
  /** Expand a Favorites folder to reveal its entries. */
  openFavoritesFolder(name: string): Promise<void>;
  /** Open the favorites folder creation dialog. */
  openFavoritesFolderDialog(): Promise<void>;
  /** Open the help menu from the sidebar. */
  openHelpMenu(): Promise<void>;
  /** Open the quick-actions menu of the named sidebar project row. */
  openProjectQuickMenu(projectName: string): Promise<void>;
  /** Follow a sidebar link by its label. */
  openSidebarLink(text: string): Promise<void>;
  /** Open the user menu from the sidebar account area. */
  openUserMenu(): Promise<void>;
  /** Follow a workspace-relative path (deep link); requires an authenticated session. */
  openWorkspacePath(path: string): Promise<void>;
  /** Open the workspace switcher from the sidebar. */
  openWorkspaceSwitcher(): Promise<void>;
  /** Project names listed in the overflow slide-over. */
  overflowProjectNames(): Promise<string[]>;
  /** Item labels of the currently open project quick-actions menu. */
  projectQuickMenuTexts(): Promise<string[]>;
  /** Href of a sidebar project row, if the row is rendered. */
  projectRowHref(projectName: string): Promise<string | null>;
  /** Links of the currently expanded project sub-navigation. */
  projectSubnavLinks(): Promise<{ text: string; href: string | null }[]>;
  /** Drop the current session so a different user can sign in. */
  resetSession(): Promise<void>;
  /** Type into the overflow slide-over search box. */
  searchOverflowProjects(query: string): Promise<void>;
  /** Expand or collapse the sidebar Favorites section. */
  setFavoritesOpen(open: boolean): Promise<void>;
  /** Expand or collapse the secondary-destinations disclosure. */
  setMoreSectionOpen(open: boolean): Promise<void>;
  /**
   * Bring a sidebar project row to the wanted expansion state, toggling
   * again when a remount swallows the first click. Leaves the final state
   * to the caller's poll: it never asserts by itself.
   */
  setProjectRowOpen(projectName: string, open: boolean): Promise<void>;
  /** Expand or collapse the sidebar Projects group. */
  setProjectsGroupOpen(open: boolean): Promise<void>;
  /** Open or close the projects overflow slide-over. */
  setProjectsOverflowOpen(open: boolean): Promise<void>;
  /** Link labels rendered in the app sidebar, in display order. */
  sidebarLinkTexts(): Promise<string[]>;
  /**
   * Rendered tone of a sidebar row: computed background and foreground.
   * Scenarios compare rows against each other (active versus idle) instead
   * of asserting a fixed color, so a redesign keeps the scenario green.
   */
  sidebarRowTone(linkText: string): Promise<{ background: string; color: string }>;
  /** Section headings rendered in the app sidebar (Projects, Favorites, ...). */
  sidebarSectionNames(): Promise<string[]>;
  /** Submit the favorites folder dialog with a name. */
  submitFavoritesFolderName(name: string): Promise<void>;
  /** Pick a workspace in the open switcher. */
  switchWorkspace(name: string): Promise<void>;
  /** Expand or collapse a sidebar project row's inline sub-navigation. */
  toggleProjectRow(projectName: string): Promise<void>;
  /** Item labels of the currently open user menu. */
  userMenuTexts(): Promise<string[]>;
  /**
   * Workspace identity mark: whether a logo image renders, its accessible
   * label, and the fallback initial shown when no logo is uploaded.
   */
  workspaceLogoState(): Promise<{ hasImage: boolean; label: string | null; initial: string | null }>;
  /** Text content of the currently open workspace switcher. */
  workspaceSwitcherTexts(): Promise<string[]>;

  /** Open the sidebar quick-create dialog. */
  openQuickCreate(): Promise<void>;
  // --- NEWFRONT-125 review fixes. Appended; existing entries above are
  // --- untouched per the shared driver contract.
  /** Click an entry of the open project quick-actions menu. */
  activateProjectQuickMenuItem(name: string): Promise<void>;
  /** Drag one sidebar Favorites entry above another. */
  dragFavoriteBefore(sourceName: string, targetName: string): Promise<void>;
  /** Open the quick-actions menu of the named Favorites entry or folder. */
  openFavoriteQuickMenu(name: string): Promise<void>;
  /** Item labels of the currently open Favorites quick-actions menu. */
  favoriteQuickMenuTexts(): Promise<string[]>;
  /** Click an entry of the open Favorites quick-actions menu. */
  activateFavoriteQuickMenuItem(name: string): Promise<void>;
  /** Collapse or expand the app sidebar; ends with the state applied. */
  setSidebarCollapsed(collapsed: boolean): Promise<void>;
  /** Whether the app sidebar is currently collapsed. */
  isSidebarCollapsed(): Promise<boolean>;
  /** Open the compact (top-bar) user menu. */
  openCompactUserMenu(): Promise<void>;
  /** Label of the active tab in the open profile-settings dialog, if any. */
  profileSettingsActiveTab(): Promise<string | null>;
  /** Whether the sidebar peek overlay is currently visible. */
  isSidebarPeekVisible(): Promise<boolean>;
  /** Close the topmost dialog by clicking the overlay outside its panel. */
  dismissDialogByOverlayClick(): Promise<void>;
  // Activity feed (NEWFRONT-114, rows CMT-013..017). Oracle selectors read
  // the running old app's user-visible structure only: the "Activity"
  // heading scopes the section, header icon buttons are addressed from the
  // end (worklog, sort, filter), and entries are the feed container's
  // children. Appended additively; never modify an existing method.
  /** Defensive sign-in: repeat the entry-plus-password flow until the workspace URL lands. */
  activitySignIn(email: string, password: string, workspaceSlug: string): Promise<void>;
  /** Open a work item detail page; requires an authenticated session. */
  activityOpenIssueDetail(workspaceSlug: string, projectId: string, issueId: string): Promise<void>;
  /** Feed entry texts in display order (merged property-change entries and comments). */
  activityEntryTexts(): Promise<string[]>;
  /** Toggle oldest-first/newest-first ordering. */
  activityToggleSort(): Promise<void>;
  /** Open the updates/comments/state/assignee filter menu. */
  activityOpenFilterMenu(): Promise<void>;
  /** Visible filter option labels in the open menu. */
  activityFilterOptionLabels(): Promise<string[]>;
  /** Toggle one filter option by its visible label. */
  activityToggleFilterOption(label: string): Promise<void>;
  /** True while the filter control shows its narrowed marker. */
  activityFilterNarrowed(): Promise<boolean>;
  /** Composer position relative to the feed. */
  activityComposerPosition(): Promise<"above" | "below" | "hidden">;
  /** Type text into the feed composer editor. */
  activityComposerType(text: string): Promise<void>;
  /** Submit the feed composer. */
  activityComposerSubmit(): Promise<void>;
  /** Edit the work item title through the detail header (drives a property-change entry). */
  activityRenameTitle(title: string): Promise<void>;
  /** True while the feed skeleton loader is visible. */
  activityLoadingVisible(): Promise<boolean>;
  /** Stored sort preference from browser-local storage. */
  activityStoredSort(): Promise<string | null>;
  /** Stored filter selection from browser-local storage. */
  activityStoredFilters(): Promise<string | null>;
  /** Click the first link inside the feed; resolves with the resulting URL. */
  activityOpenFirstEntryLink(): Promise<string>;

  // --- Issue activity & comments (NEWFRONT-122, ISS-194–206). Appended;
  // --- existing methods above are untouched per the shared driver contract.

  /** Open one work-item detail page; ends on the canonical browse URL. */
  openIssueDetail(workspaceSlug: string, projectId: string, issueId: string): Promise<void>;
  /** Visible texts of the comment cards in feed order. */
  activityCommentTexts(): Promise<string[]>;
  /** Whether the feed shows the issue-created activity row. */
  activityHasCreationEntry(): Promise<boolean>;
  /** Feed filter options in display order with their selected state. */
  activityFilterOptions(): Promise<{ label: string; selected: boolean }[]>;
  /** Toggle one feed filter option by label. */
  activityToggleFilter(label: string): Promise<void>;
  /** Whether the filter button carries the partial-selection accent dot. */
  activityFilterDotVisible(): Promise<boolean>;
  /** Whether the comment composer renders above the feed (true in DESC). */
  activityComposerIsAboveFeed(): Promise<boolean>;
  /** Current text of the comment composer, trimmed. */
  activityComposerText(): Promise<string>;
  /** Post `bodyText` through the composer; resolves once its card appears. */
  activityPostComment(bodyText: string): Promise<void>;
  /** Hover a comment card and open its quick-actions menu. */
  activityOpenCommentMenu(cardText: string): Promise<void>;
  /** Titles of the open quick-actions menu items. */
  activityMenuItems(): Promise<string[]>;
  /** Click one open quick-actions menu item by title. */
  activityClickMenuItem(name: string): Promise<void>;
  /** Rename a comment through its inline editor; resolves on the edited marker. */
  activityEditComment(oldText: string, newText: string): Promise<void>;
  /** Whether a toast carrying `text` is currently visible. */
  sawToast(text: string): Promise<boolean>;
  /** Cancel the open inline editor; resolves once editing closes. */
  activityCancelEdit(cardText: string): Promise<void>;
  /** Whether a comment card carries the deep-link highlight class. */
  activityCommentHighlighted(cardText: string): Promise<boolean>;
  /** Hover a comment reaction chip; resolves with the reactor tooltip text. */
  activityChipTooltipText(cardText: string, emoji: string, expectedName: string): Promise<string>;
  /** Whether the comment body for `cardText` is currently rendered. */
  activityCommentBodyVisible(cardText: string): Promise<boolean>;
  /** Expand a folded comment through its in-card toggle. */
  activityExpandFoldedComment(cardText: string): Promise<void>;
  /** Copy a comment link; resolves with the clipboard text. */
  activityCopyCommentLink(cardText: string): Promise<string>;
  /**
   * Add a reaction to a comment through the first emoji-grid entry.
   * Resolves with the picked emoji character and its decimal code-point key.
   */
  activityAddCommentReaction(cardText: string): Promise<{ emoji: string; code: string }>;
  /** Reaction chips on a comment card: emoji, rendered count, current-user highlight. */
  activityCommentReactionChips(cardText: string): Promise<{ emoji: string; count: number; reacted: boolean }[]>;
  /** Toggle one reaction chip on a comment card. */
  activityClickCommentReactionChip(cardText: string, emoji: string): Promise<void>;
  /** Add a reaction to the issue itself through the first emoji-grid entry. */
  issueAddReaction(): Promise<{ emoji: string; code: string }>;
  /** Reaction chips on the issue: emoji, rendered count, current-user highlight. */
  issueReactionChips(): Promise<{ emoji: string; count: number; reacted: boolean }[]>;
  /** Toggle one reaction chip on the issue. */
  issueClickReactionChip(emoji: string): Promise<void>;
  /** Whether the "Code reviews" section renders above the activity feed. */
  codeReviewsVisible(): Promise<boolean>;
  /** Code-review links in display order: badge, title, href and target. */
  codeReviewLinks(): Promise<{ badge: string; title: string; href: string | null; target: string | null }[]>;
  /** Attach a review URL through the section form; resolves once its row appears. */
  codeReviewAttach(url: string): Promise<void>;
  /** Fill the attach form and submit without waiting for success (error path). */
  codeReviewAttemptAttach(url: string): Promise<void>;
  /** Current value of the attach-form URL input. */
  codeReviewInputValue(): Promise<string>;
  /** Detach the review row showing `title`; resolves once its row is gone. */
  codeReviewDetach(title: string): Promise<void>;
  /** Whether the worklog create control renders in the activity header. */
  worklogCreateVisible(): Promise<boolean>;
  // --- Shared property dropdowns (NEWFRONT-122, ISS-207–220). Observed on
  // --- the running old app: issue-detail sidebar rows pair a label span with
  // --- a value container holding the picker trigger; Base UI combobox
  // --- popups render in a portal with an optional search input and a
  // --- listbox of options.
  /** Value text of the sidebar property row `label` ("State", "Priority", ...). */
  propertyValueText(label: string): Promise<string>;
  /** Open the sidebar picker popup for row `label`. */
  propertyOpenPicker(label: string): Promise<void>;
  /** Focus the row trigger and open the popup with the Enter key. */
  propertyOpenPickerByKeyboard(label: string): Promise<void>;
  /** Whether the sidebar picker trigger for row `label` is disabled. */
  propertyPickerDisabled(label: string): Promise<boolean>;
  /** Whether the sidebar property row `label` renders a picker trigger at all. */
  propertyTriggerPresent(label: string): Promise<boolean>;
  /** Whether any picker popup listbox is currently open. */
  pickerOpen(): Promise<boolean>;
  /** Option texts in the open picker popup, in display order. */
  pickerOptionTexts(): Promise<string[]>;
  /** Whether the open picker popup carries a search box. */
  pickerHasSearch(): Promise<boolean>;
  /** Type into the open picker's search box. */
  pickerSearch(query: string): Promise<void>;
  /** Current value of the open picker's search box. */
  pickerSearchValue(): Promise<string>;
  /** Whether the open picker's search box currently has DOM focus. */
  pickerSearchFocused(): Promise<boolean>;
  /** Click the open-picker option whose text contains `text`. */
  pickerPick(text: string): Promise<void>;
  /** Whether the open-picker option containing `text` is disabled. */
  pickerOptionDisabled(text: string): Promise<boolean>;
  /** Empty-state message in the open picker, or "" when options render. */
  pickerEmptyText(): Promise<string>;
  /** Press Escape with the picker popup open. */
  pickerPressEscape(): Promise<void>;
  /** Click neutral sidebar chrome to dismiss the picker popup. */
  pickerClickOutside(): Promise<void>;
  /** Open an archived issue's detail page; ends on the archives URL. */
  openArchivedIssueDetail(workspaceSlug: string, projectId: string, issueId: string): Promise<void>;
  // --- Single-date dropdowns (NEWFRONT-122, ISS-214). Observed on the
  // --- running old app: the Start/Due sidebar rows open a portal calendar
  // --- popup (single-select month grid with month/year caption dropdowns)
  // --- instead of the combobox listbox the other pickers use.
  /** Open the calendar popup for the date row `label` ("Start date", ...). */
  datePickerOpen(label: string): Promise<void>;
  /** Whether the calendar popup is currently open. */
  datePickerVisible(): Promise<boolean>;
  /** Month and year the open calendar currently shows (caption dropdowns). */
  datePickerVisibleMonth(): Promise<{ month: string; year: string }>;
  /** Pick day-of-month `day` in the open calendar's current month. */
  datePickerPickDay(day: number): Promise<void>;
  /** Whether day-of-month `day` is disabled in the open calendar. */
  datePickerDayDisabled(day: number): Promise<boolean>;
  /** Whether the open calendar renders outside the sidebar row (portal). */
  datePickerPortalAttached(label: string): Promise<boolean>;
  /** Clear the date row `label` through its hover-revealed clear control. */
  datePickerClear(label: string): Promise<void>;
  /** Whether the sidebar property row `label` renders at all. */
  propertyRowPresent(label: string): Promise<boolean>;
  // --- Create-issue modal project picker (NEWFRONT-122, ISS-211).
  // --- Observed on the running old app: the modal header carries the
  // --- project picker (a combobox popup with a search box and a listbox
  // --- of joined projects the user may create in); the modal title field
  // --- is a named text input and "Save" submits.
  /** Open the create-issue modal from a project's issues list. */
  issueModalOpenCreate(workspaceSlug: string, projectId: string): Promise<void>;
  /** Project name currently shown on the modal's project picker button. */
  issueModalProjectValue(): Promise<string>;
  /** Open the modal's project picker popup. */
  issueModalProjectOpenPicker(): Promise<void>;
  /** Project option texts in the open picker, in display order. */
  issueModalProjectOptionTexts(): Promise<string[]>;
  /** Type into the open project picker's search box. */
  issueModalProjectSearch(query: string): Promise<void>;
  /** Empty-state message in the open project picker, or "" when options render. */
  issueModalProjectEmptyText(): Promise<string>;
  /** Click the open-picker option whose text contains `text`. */
  issueModalProjectPick(text: string): Promise<void>;
  /** Dismiss the modal's project picker popup with Escape. */
  issueModalProjectPressEscape(): Promise<void>;
  /** Fill the modal's title field. */
  issueModalFillTitle(title: string): Promise<void>;
  /** Submit the modal; resolves once it closes. */
  issueModalSubmit(): Promise<void>;
  // --- Date-range dropdowns (NEWFRONT-122, ISS-215). Observed on the
  // --- running old app: the issues-list merged-dates cell shows one smart
  // --- label for the issue's start+due pair with a clear control, while the
  // --- cycle-create form shows the split from/to pair; both open a range
  // --- calendar popup where out-of-range days render disabled.
  /** Smart-label text of the merged-dates cell in the list row `issueName`. */
  rangeMergedCellText(issueName: string): Promise<string>;
  /** Open the range calendar from the list row's merged-dates cell. */
  rangeMergedCellOpen(issueName: string): Promise<void>;
  /** Clear both dates through the merged cell's clear control. */
  rangeMergedCellClear(issueName: string): Promise<void>;
  /** Whether the range calendar popup is currently open. */
  rangeCalendarVisible(): Promise<boolean>;
  /** Click day-of-month `day` in the open range calendar (no close wait). */
  rangeCalendarPickDay(day: number): Promise<void>;
  /** Whether day-of-month `day` is disabled in the open range calendar. */
  rangeCalendarDayDisabled(day: number): Promise<boolean>;
  /** Switch the open range calendar to the captioned month `monthLabel`. */
  rangeCalendarSelectMonth(monthLabel: string): Promise<void>;
  /** Switch the open range calendar to the captioned year `yearLabel`. */
  rangeCalendarSelectYear(yearLabel: string): Promise<void>;
  /** Open the cycle-create form from a project's cycles list. */
  cycleCreateOpen(workspaceSlug: string, projectId: string): Promise<void>;
  /** Placeholder pair the cycle form's split range trigger shows. */
  cycleFormRangePlaceholders(): Promise<{ from: string; to: string }>;
  /** Open the range calendar from the cycle form's split trigger. */
  cycleFormRangeOpen(): Promise<void>;
  /** Fill the cycle form's name field. */
  cycleFormFillName(name: string): Promise<void>;
  /** Submit the cycle form; resolves once it closes. */
  cycleFormSubmit(): Promise<void>;
  // --- Intake-state dropdown (NEWFRONT-122, ISS-217). Observed on the
  // --- running old app: the intake page header offers an "Add work item"
  // --- button opening the intake-create modal, whose state picker is a
  // --- searchable single-select over the project's intake states; the
  // --- triage screen instead renders the state row disabled.
  /** Open the intake-create modal from a project's intake page. */
  intakeCreateOpen(workspaceSlug: string, projectId: string): Promise<void>;
  /** State name currently shown on the intake modal's state picker. */
  intakeStateValue(): Promise<string>;
  /** Open the intake modal's state picker popup. */
  intakeStateOpenPicker(): Promise<void>;
  /** State option texts in the open picker, in display order. */
  intakeStateOptionTexts(): Promise<string[]>;
  /** Type into the open state picker's search box. */
  intakeStateSearch(query: string): Promise<void>;
  /** Empty-state message in the open picker, or "" when options render. */
  intakeStateEmptyText(): Promise<string>;
  /** Click the open-picker option whose text contains `text`. */
  intakeStatePick(text: string): Promise<void>;
  /** Fill the intake modal's title field. */
  intakeCreateFillTitle(title: string): Promise<void>;
  /** Submit the intake modal; resolves once it closes. */
  intakeCreateSubmit(): Promise<void>;
  /** Whether the triage screen's State row picker is disabled. */
  intakeTriageStateDisabled(): Promise<boolean>;
  // --- Layout dropdown (NEWFRONT-122, ISS-219). Observed on the running old
  // --- app: a fresh project's views page shows a "Create view" empty-state
  // --- action opening the view form, whose layout picker is a
  // --- search-disabled dropdown over the five layouts with a checkmark on
  // --- the selected one; no caller ever excludes a layout.
  /** Open a project's views list page. */
  viewsOpenList(workspaceSlug: string, projectId: string): Promise<void>;
  /** Open the view-create modal from the empty-state action. */
  viewsOpenCreate(): Promise<void>;
  /** Layout name currently shown on the form's layout picker. */
  viewsLayoutValue(): Promise<string>;
  /** Open the form's layout picker popup. */
  viewsLayoutOpenPicker(): Promise<void>;
  /** Layout option texts in the open picker, in display order. */
  viewsLayoutOptionTexts(): Promise<string[]>;
  /** Whether the open layout picker carries a search box. */
  viewsLayoutHasSearch(): Promise<boolean>;
  /** Whether the option `text` carries the selected checkmark. */
  viewsLayoutSelectedMarked(text: string): Promise<boolean>;
  /** Click the open-picker option whose text contains `text`. */
  viewsLayoutPick(text: string): Promise<void>;
  /** Fill the view form's name field. */
  viewsFillName(name: string): Promise<void>;
  /** Submit the view form; resolves once it closes. */
  viewsSubmit(): Promise<void>;
  // --- Edition-only stubs (NEWFRONT-122, ISS-231–237). Observed on the
  // --- running old app: the OSS ce/ stubs render empty fragments where
  // --- cloud mounts de-dupe, epics, workflow, team/type filters, issue
  // --- types, sidebar accents and gantt dependencies. Specs assert the
  // --- absence on the live surfaces plus the surrounding OSS behavior.
  /** Open the sub-issues filter dropdown on an issue's detail page. */
  subIssueFiltersOpen(workspaceSlug: string, projectId: string, parentIssueId: string): Promise<void>;
  /** Text of the open sub-issues filter dropdown panel. */
  subIssueFiltersPanelText(): Promise<string>;
  /** Identifier text shown on the open issue detail (e.g. "PAR-12"). */
  detailIdentifierText(): Promise<string>;
  /** Click the detail identifier (copies it when copy is enabled). */
  detailIdentifierCopy(): Promise<void>;
  /** Open a saved view's detail page. */
  viewsOpenDetail(workspaceSlug: string, projectId: string, viewId: string): Promise<void>;
  /** Whether the open gantt view renders a block for the named issue. */
  ganttShowsIssue(issueName: string): Promise<boolean>;
  // --- Label management (NEWFRONT-122, ISS-226–230). Observed on the
  // --- running old app: project settings carry a Labels page (admin-only
  // --- "Add label" plus an inline name/color form, a two-level
  // --- drag-and-drop tree, per-row edit/delete menus, a delete
  // --- confirmation, and empty/loading states); the issue detail Labels
  // --- row offers the same labels through a multi-select picker.
  /** Open a project's settings Labels page; resolves once it settles. */
  settingsLabelsOpen(workspaceSlug: string, projectId: string): Promise<void>;
  /** Label names in tree order as the settings list shows them. */
  settingsLabelsNames(): Promise<string[]>;
  /** Whether the "Add label" control renders (admin-only). */
  settingsLabelsAddVisible(): Promise<boolean>;
  /** Open the inline create form via "Add label". */
  settingsLabelsOpenCreate(): Promise<void>;
  /** Whether the inline label form is currently open. */
  settingsLabelsFormVisible(): Promise<boolean>;
  /** Fill the inline form's name field. */
  settingsLabelsFillName(name: string): Promise<void>;
  /** Inline name-field error text, or "" when the form shows none. */
  settingsLabelsFormError(): Promise<string>;
  /** Submit the inline create form ("Add"); resolves once it closes. */
  settingsLabelsSubmitCreate(): Promise<void>;
  /** Submit the inline form in update mode ("Update"). */
  settingsLabelsSubmitUpdate(): Promise<void>;
  /** Close the inline form without saving ("Cancel"). */
  settingsLabelsCancelForm(): Promise<void>;
  /** Background color of the inline form's color dot, as rendered. */
  settingsLabelsDotColor(): Promise<string>;
  /** Open the inline form's color picker popup. */
  settingsLabelsOpenColorPicker(): Promise<void>;
  /** Pick the swatch for `hex` in the open color picker. */
  settingsLabelsPickColor(hex: string): Promise<void>;
  /** Open the row menu for the label `name`. */
  settingsLabelsOpenRowMenu(name: string): Promise<void>;
  /** Visible row-menu item texts. */
  settingsLabelsMenuItems(): Promise<string[]>;
  /** Click the row-menu item `text`; resolves once the menu closes. */
  settingsLabelsMenuPick(text: string): Promise<void>;
  /** Whether the label `name` renders as a group (has child rows). */
  settingsLabelsIsGroup(name: string): Promise<boolean>;
  /** Delete the label `name` through its row trash control. */
  settingsLabelsDeleteViaTrash(name: string): Promise<void>;
  /** Body text of the open delete-label confirmation, or "" when shut. */
  settingsLabelsDeleteModalText(): Promise<string>;
  /** Confirm the delete-label modal; resolves once it closes. */
  settingsLabelsDeleteConfirm(): Promise<void>;
  /** Dismiss the delete-label modal without deleting. */
  settingsLabelsDeleteCancel(): Promise<void>;
  /** Drag the label `source` onto `target` (nest as its child). */
  settingsLabelsDragOnto(source: string, target: string): Promise<void>;
  /** Drag the label `source` above `target` (reorder within the list). */
  settingsLabelsDragAbove(source: string, target: string): Promise<void>;
  /** Empty-state title on the labels page, or "" when the list renders. */
  settingsLabelsEmptyTitle(): Promise<string>;
  /** Click the empty-state "Create your first label" action. */
  settingsLabelsEmptyAction(): Promise<void>;
  /** Whether the labels loading skeleton currently shows. */
  settingsLabelsSkeletonVisible(): Promise<boolean>;
  /** Delay label-list API answers by `ms` so the skeleton is observable. */
  settingsLabelsDelayLoad(ms: number): Promise<void>;
  /** Open the issue detail Labels picker popup. */
  issueLabelsOpenPicker(): Promise<void>;
  /** Label option texts in the open issue picker, in display order. */
  issueLabelsOptionTexts(): Promise<string[]>;
  /** Text of the issue detail Labels row (attached label names). */
  issueLabelsRowText(): Promise<string>;
  /** Current value of the inline form's name field. */
  settingsLabelsNameValue(): Promise<string>;
  /** Click the inline form's Add/Update button without waiting for close. */
  settingsLabelsAttemptSubmit(): Promise<void>;
  /** Click the delete modal's Delete button without waiting for close. */
  settingsLabelsAttemptDeleteConfirm(): Promise<void>;

  // --- Cross-cutting: realtime-absence, permissions, URL params, cycle
  // --- transfer, optimistic rollback (NEWFRONT-122, ISS-221–225).

  /** Reload the current page and wait for the document to settle. */
  reloadPage(): Promise<void>;
  /** Whether the list quick-add ("New work item") trigger renders. */
  projectQuickAddVisible(): Promise<boolean>;
  /** Whether the detail title renders as an editable input. */
  issueTitleInputEnabled(): Promise<boolean>;
  /** Whether the labels settings page shows the not-authorized view. */
  settingsLabelsAccessDenied(): Promise<boolean>;
  /** Whether the main view currently shows an issue named `name`. */
  globalViewIssueVisible(name: string): Promise<boolean>;
  /** Open a cycle's issues page. */
  openCycleIssues(workspaceSlug: string, projectId: string, cycleId: string): Promise<void>;
  /** Whether the cycle page offers the transfer-issues button. */
  cycleTransferButtonVisible(): Promise<boolean>;
  /** Open the transfer-issues modal. */
  cycleTransferOpen(): Promise<void>;
  /** Target cycle names listed in the open transfer modal. */
  cycleTransferOptionNames(): Promise<string[]>;
  /** Pick the target cycle named `name` in the open transfer modal. */
  cycleTransferPick(name: string): Promise<void>;
  /** Fail the next issue PATCH with `status` after `delayMs` (once). */
  failNextIssuePatch(status: number, delayMs: number): Promise<void>;
  /** Remove any issue-PATCH failure route. */
  clearIssuePatchFailure(): Promise<void>;

  /** Sign out through the sidebar account menu; ends at the signed-out entry. */
  signOutViaAccountMenu(): Promise<void>;
  /** Sign out through the command palette; ends at the signed-out entry. */
  signOutViaCommandPalette(): Promise<void>;
  /** Whether the app currently shows the signed-out entry (sign-in card). */
  isSignedOut(): Promise<boolean>;
  /** Open the switch-account control on the onboarding header. */
  openSwitchAccount(): Promise<void>;
  /** Email named as the active account by the switch-account dialog. */
  switchAccountEmail(): Promise<string>;
  /** Confirm account switching; routes through sign-out into a fresh login. */
  confirmSwitchAccount(): Promise<void>;
  /** Open the deactivate-account dialog from profile settings. */
  openDeactivateAccount(): Promise<void>;
  /** Confirm deactivation; disables the account and signs out. */
  confirmDeactivation(): Promise<void>;
  /** Drop the local session cookies (simulates expiry) without using the UI. */
  dropSession(): Promise<void>;
  /** Follow a link verbatim (deep links, denial URLs, error landings). */
  visit(path: string): Promise<void>;
  /** Whether the given text is visible anywhere on screen. */
  showsText(text: string): Promise<boolean>;
  /** Type a device code into the approval form (auto-formats as typed). */
  typeDeviceCode(code: string): Promise<void>;
  /** Current value of the device-code field (proves auto-formatting). */
  deviceCodeFieldValue(): Promise<string>;
  /** Submit the device-approval form. */
  submitDeviceApproval(): Promise<void>;
  /** Type text into the currently focused control. */
  typeText(text: string): Promise<void>;
  /** Accessible name of the currently focused control, or null. */
  focusedControlName(): Promise<string | null>;

  /** Open the workspace home dashboard; requires an authenticated session. */
  openWorkspaceHome(workspaceSlug: string): Promise<void>;
  /** Open the workspace projects list; requires an authenticated session. */
  openProjectsList(workspaceSlug: string): Promise<void>;
  /** Open one project tab (for example "issues"); requires an authenticated session. */
  openProjectTab(workspaceSlug: string, projectId: string, tab: string): Promise<void>;
  /** True while the persistent app sidebar is mounted on the current page. */
  sidebarPresent(): Promise<boolean>;
  /** Sidebar width in CSS pixels, or null while the sidebar is unmounted. */
  sidebarWidth(): Promise<number | null>;
  /** True while the full-screen overlay portal slot exists on the page. */
  portalPresent(): Promise<boolean>;
  /** True while the far-left app rail strip is rendered. */
  railPresent(): Promise<boolean>;
  /** Computed left padding in pixels of the content holder, or null without the chrome. */
  contentPaddingLeft(): Promise<number | null>;
  /** Names plus hrefs of the project tabs rendered in the tab strip, in order. */
  projectTabs(): Promise<Array<{ name: string; href: string }>>;
  /** Name of the currently highlighted project tab, or null when none is. */
  activeTabName(): Promise<string | null>;
  /** True while the cloud edition badge is rendered. */
  editionBadgePresent(): Promise<boolean>;
  /** True while the desktop update control is rendered in the sidebar. */
  desktopUpdatePresent(): Promise<boolean>;
  /** How many upgrade pills ("Pro" markers) are visible right now. */
  upgradePillCount(): Promise<number>;
  /**
   * Which top-bar controls are currently visible: the workspace menu, the
   * sidebar toggle, the command search box, the inbox link, the help menu,
   * the repository star link, and the compact account fallback.
   */
  topBarControls(): Promise<{
    workspaceMenu: boolean;
    sidebarToggle: boolean;
    search: boolean;
    inbox: boolean;
    help: boolean;
    starLink: boolean;
    accountFallback: boolean;
  }>;
  /** Click the sidebar toggle in the top bar. */
  toggleSidebar(): Promise<void>;
  /** Click the sidebar personalization control to open its dialog. */
  openPersonalizeDialog(): Promise<void>;
  /** True while the sidebar personalization dialog is open. */
  personalizeDialogOpen(): Promise<boolean>;
  /** Checked state of a personal entry ("Your work", "Drafts") in the open dialog. */
  personalItemChecked(name: string): Promise<boolean | null>;
  /** Flip a personal entry checkbox in the open dialog. */
  setPersonalItemEnabled(name: string, enabled: boolean): Promise<void>;
  /** Drag one personal entry onto another to reorder them in the open dialog. */
  movePersonalItem(dragged: string, target: string): Promise<void>;
  /** Names of the personal entries in dialog order. */
  personalItemNames(): Promise<string[]>;
  /** Which project-list rendering mode ("ACCORDION" or "TABBED") is selected. */
  projectNavMode(): Promise<"ACCORDION" | "TABBED" | null>;
  /** Select a project-list rendering mode in the open dialog. */
  setProjectNavMode(mode: "ACCORDION" | "TABBED"): Promise<void>;
  /** Current value of the listed-projects count input, or null when hidden. */
  projectCapInput(): Promise<string | null>;
  /** Whether the listed-projects cap is currently enabled, or null when hidden. */
  projectCapEnabled(): Promise<boolean | null>;
  /** Toggle the listed-projects cap and optionally set its count. */
  setProjectCap(enabled: boolean, count?: number): Promise<void>;
  /** Text of the project header button in the tab strip. */
  projectHeaderText(): Promise<string | null>;
  /** True while the header name is visually truncated with an ellipsis. */
  projectHeaderTruncated(): Promise<boolean>;
  /** Open the project switcher dropdown from the tab strip header. */
  openProjectSwitcher(): Promise<void>;
  /** Names of the projects offered by the open switcher. */
  switcherOptionNames(): Promise<string[]>;
  /** Pick a project from the open switcher. */
  chooseSwitcherOption(name: string): Promise<void>;
  /** Open the quick-actions menu beside the project header. */
  openProjectActions(): Promise<void>;
  /** Entries offered by the open quick-actions menu. */
  projectActionNames(): Promise<string[]>;
  /** Click a quick-actions menu entry. */
  clickProjectAction(name: string): Promise<void>;
  /** Read back text the app wrote to the clipboard (copy-location flows). */
  readClipboardText(): Promise<string>;
  /** Most recent toast notice text, or null when none is showing. */
  toastText(): Promise<string | null>;
  /** Right-click a visible project tab to summon its context menu. */
  rightClickTab(name: string): Promise<void>;
  /** Entries offered by the open tab context menu. */
  contextMenuItems(): Promise<string[]>;
  /** Click a tab context menu entry. */
  clickContextMenuItem(name: string): Promise<void>;
  /** Open the tab strip overflow menu. */
  openOverflowMenu(): Promise<void>;
  /** True while the tab strip overflow trigger is laid out on the page. */
  overflowTriggerPresent(): Promise<boolean>;
  /** Rows listed in the open overflow menu. */
  overflowRowNames(): Promise<string[]>;
  /** Restore a user-hidden tab from the open overflow menu. */
  restoreOverflowTab(name: string): Promise<void>;
  /** Resize the browser viewport (responsive and overflow scenarios). */
  setViewportSize(width: number, height: number): Promise<void>;
  /** Open the workspace notifications page (the app sidebar stays unmounted there). */
  openNotifications(workspaceSlug: string): Promise<void>;
  /** True while the sidebar header shows the product brand. */
  sidebarBrandVisible(): Promise<boolean>;
  /** Names of the quick-action buttons carried by the sidebar header. */
  sidebarQuickActionNames(): Promise<string[]>;
  /** How many account controls the sidebar currently renders. */
  sidebarAccountButtonCount(): Promise<number>;
  /** Drag the sidebar resize grip horizontally by the given pixels. */
  dragSidebarGripBy(dx: number): Promise<void>;
  /** Double-click the sidebar resize grip (collapse gesture). */
  doubleClickSidebarGrip(): Promise<void>;
  /** Hover the collapsed left edge where a peek overlay would appear. */
  hoverCollapsedEdge(): Promise<void>;
  /** Click content outside the floating sidebar (outside-tap gesture). */
  clickOutsideSidebar(): Promise<void>;
  /** True while a personal entry with this name shows in the sidebar. */
  sidebarEntryVisible(name: string): Promise<boolean>;
  /** Type text at the end of the listed-projects count input. */
  projectCapTypeText(text: string): Promise<void>;
  /** Replace the listed-projects count input with the given value. */
  projectCapFill(value: string): Promise<void>;
  /** True while the count input flags a below-minimum value inline. */
  projectCapMinErrorVisible(): Promise<boolean>;
  /** True while a rail settings entry links anywhere on the page. */
  railSettingsEntryPresent(): Promise<boolean>;
  /** Context-menu text summoned from the left-edge rail zone. */
  railContextMenuText(): Promise<string>;
  /** True while the inbox link carries an unread dot. */
  inboxDotPresent(): Promise<boolean>;
  /** Hover the project header button (hover-reveal gesture). */
  hoverProjectHeader(): Promise<void>;
  /** How many visible matches of a project name the page shows. */
  projectNameVisibleCount(name: string): Promise<number>;
  /** Heading of the dialog the last project action opened, or null. */
  projectActionDialogHeading(): Promise<string | null>;
  /** True while the paywalled Cycles page header is visible. */
  activeCyclesHeaderVisible(): Promise<boolean>;
  /** True while the generic error notice shows (bare-address flows). */
  errorNoticeVisible(): Promise<boolean>;
  // --- NEWFRONT-125 re-review fix (049 placeholders). Appended; existing
  // --- entries above are untouched per the shared driver contract.
  /** Placeholder blocks in the on-screen sidebar's project list while it loads; zero once rows render. */
  sidebarProjectPlaceholderCount(): Promise<number>;

  /**
   * Display (arrangement) controls (NEWFRONT-119): the issues header
   * popover with shown-fields pills, grouping, ordering, and switches.
   */
  /** Open the Display popover; ends with its panel visible. */
  openDisplayOptions(): Promise<void>;
  /** Dismiss the Display popover. */
  closeDisplayOptions(): Promise<void>;
  /** Visible text of the open Display panel (section headings plus options). */
  displayPanelText(): Promise<string>;
  /** Pick a grouping dimension (for example "States" or "None"). */
  setDisplayGroupBy(option: string): Promise<void>;
  /** Pick a sort order (for example "Manual" or "Last created"). */
  setDisplayOrderBy(option: string): Promise<void>;
  /** Check or uncheck an Options switch (for example "Show empty groups"). */
  setDisplayExtraOption(option: string, enabled: boolean): Promise<void>;
  /** Whether a Display radio or checkbox option currently shows checked. */
  isDisplayOptionChecked(option: string): Promise<boolean>;
  /** Toggle a shown-fields pill (for example "Assignee"). */
  toggleDisplayProperty(option: string): Promise<void>;
  /** Whether a shown-fields pill currently shows active. */
  isDisplayPropertyActive(option: string): Promise<boolean>;
  /**
   * Condition-row builder (NEWFRONT-119): the header toggle plus the row
   * of field/operator/value conditions with its clear and view actions.
   */
  /** Toggle the builder row via the header control beside Display. */
  toggleRichFilterRow(): Promise<void>;
  /** Whether the builder row is currently visible. */
  isRichFilterRowVisible(): Promise<boolean>;
  /** Visible text of the builder row (conditions plus actions). */
  richFilterRowText(): Promise<string>;
  /** Open the field picker and add a condition for the named property. */
  addRichCondition(property: string): Promise<void>;
  /**
   * Pick values in the open value slot of the last condition (for example
   * ["Urgent"] or ["Backlog"]); closes the slot afterwards.
   */
  pickRichValues(values: string[]): Promise<void>;
  /**
   * Pick values by substring in the open value slot; closes the slot
   * afterwards. Member options prefix an avatar initial in their accessible
   * name ("P Parity Oracle"), so callers match on the name part.
   */
  pickRichValuesContaining(values: string[]): Promise<void>;
  /**
   * Names of the properties the field picker currently offers, without
   * adding anything (opens the picker and dismisses it).
   */
  listRichPickerOptions(): Promise<string[]>;
  /** Names of the options in the currently open value slot. */
  richValueOptions(): Promise<string[]>;
  /**
   * Operator option labels for the single condition in the row, without
   * changing anything (opens the operator menu and dismisses it).
   */
  richOperatorOptions(): Promise<string[]>;
  /** Pick an operator for the single condition in the row by label. */
  pickRichOperator(option: string): Promise<void>;
  /** Whether the single-condition operator control is locked (one operator). */
  isSingleRichOperatorLocked(): Promise<boolean>;
  /** Whether a date calendar is currently open on the row. */
  isRichCalendarOpen(): Promise<boolean>;
  /** Pick a calendar day by its number (first match; opens the slot first). */
  pickRichDay(day: string): Promise<void>;
  /** Number of conditions currently in the row. */
  richConditionCount(): Promise<number>;
  /** Remove the index-th condition in the row. */
  removeRichCondition(index: number): Promise<void>;
  /** Click "Clear all" when the row offers it. */
  clearRichFilters(): Promise<void>;
  /** Open a saved project view page; requires an authenticated session. */
  openProjectView(workspaceSlug: string, projectId: string, viewId: string): Promise<void>;
  /** Save the current expression as a view with the given title. */
  saveRichViewAs(name: string): Promise<void>;
  /** Apply pending view changes via "Update view". */
  updateRichView(): Promise<void>;
  /** Header analytics entry (NEWFRONT-119): project-scoped dialog. */
  /** Open the analytics dialog; ends with it visible. */
  openAnalytics(): Promise<void>;
  /** Dismiss the analytics dialog. */
  closeAnalytics(): Promise<void>;
  /** Visible text of the open analytics dialog. */
  analyticsDialogText(): Promise<string>;

  // -------------------------------------------------------------------------
  // Command palette / Power-K, search, help, browse, repo-star
  // (NEWFRONT-127, rows SHELL-080, 082, 083, 084, 085, 087, 089, 094, 103, 106).
  // Added additively to the interface (never forking a driver). The palette is
  // a cmdk surface inside a Headless-UI dialog: scenarios drive it with the
  // real keyboard chord and read what the user sees (placeholder, group
  // headings, command titles, the selected row, the server-search heading).
  // -------------------------------------------------------------------------

  /** The current location's path (what the address bar shows). */
  currentUrlPath(): Promise<string>;
  /** Navigate to an arbitrary path already authenticated (workspace pages). */
  goToPath(path: string): Promise<void>;

  // --- palette open / close / reset (SHELL-080, SHELL-082) ---
  /** Press the global open chord (Ctrl/Cmd+K); resolves settled at root, throws if it never settles. */
  pressPaletteOpenChord(): Promise<void>;
  /** Whether the centered modal palette is open (dialog + command input shown). */
  isCommandPaletteOpen(): Promise<boolean>;
  /** The palette search input's placeholder (identifies root vs a sub-page), or null. */
  commandPalettePlaceholder(): Promise<string | null>;
  /** Focus the top-bar search input (an always-present text field) and type into it. */
  focusAndTypeTopBarSearch(text: string): Promise<void>;
  /** Click the modal backdrop (outside the panel) to close the palette. */
  closeCommandPaletteViaBackdrop(): Promise<void>;

  // --- palette query + keyboard flow (SHELL-083, SHELL-085) ---
  /** Type into the open palette's command input. */
  typeInCommandPalette(text: string): Promise<void>;
  /** The current value of the palette command input. */
  commandPaletteQueryValue(): Promise<string>;
  /** Press a key while the palette input is focused (Escape/Backspace/ArrowDown/Enter/etc.). */
  pressInCommandPalette(key: string): Promise<void>;
  /** Visible group headings currently rendered in the palette, in display order. */
  paletteGroupHeadings(): Promise<string[]>;
  /** Visible command item titles currently rendered in the palette, in display order. */
  paletteCommandTitles(): Promise<string[]>;
  /** Whether a command with this exact title is currently listed. */
  paletteHasCommand(title: string): Promise<boolean>;
  /** Activate (click) a palette command by its exact visible title. */
  activatePaletteCommand(title: string): Promise<void>;
  /** The text of the currently highlighted (aria-selected) palette item, or null. */
  paletteSelectedItemText(): Promise<string | null>;

  // --- server search (SHELL-084) ---
  /** The "Search results for …" heading text shown for a server search, or null. */
  paletteSearchResultsHeading(): Promise<string | null>;
  /** Whether the search-results heading is showing its in-flight pulse. */
  isPaletteSearchHeadingPulsing(): Promise<boolean>;
  /** Whether the footer workspace-level scope toggle is present. */
  paletteHasWorkspaceLevelToggle(): Promise<boolean>;
  /** Whether that scope toggle is enabled (disabled when no project is in context). */
  isWorkspaceLevelToggleEnabled(): Promise<boolean>;
  /** Flip the footer workspace-level scope toggle. */
  toggleWorkspaceLevel(): Promise<void>;
  /**
   * Count palette search requests to GET /workspaces/{slug}/search/ while
   * running `action` (proves debounce coalescing and the no-network blank case).
   */
  countSearchRequests(action: () => Promise<void>): Promise<number>;
  /** The query params of the most recent palette search request, or null. */
  lastSearchRequestParams(): Promise<Record<string, string> | null>;

  // --- shortcuts reference dialog (SHELL-094) ---
  /** Whether the keyboard-shortcuts reference dialog is open. */
  isShortcutsDialogOpen(): Promise<boolean>;
  /** Press the global chord that opens the shortcuts dialog (Ctrl/Cmd+/). */
  pressShortcutsDialogChord(): Promise<void>;
  /** Type into the shortcuts dialog's filter box. */
  typeShortcutsFilter(text: string): Promise<void>;
  /** Visible command titles listed in the shortcuts dialog, in display order. */
  shortcutsDialogCommandTitles(): Promise<string[]>;

  // --- repo-star action (SHELL-103) ---
  /** The repo-star link's {href,target,rel} attributes, or null when absent. */
  repoStarLinkAttributes(): Promise<{ href: string; target: string; rel: string } | null>;
  /** The src of the repo-star icon image (theme-adaptive asset), or null. */
  repoStarIconSrc(): Promise<string | null>;

  // --- preferences: theme, language, timezone, first day of week
  //     (SHELL-088, 095, 096, 097). The four "Change …" preference commands
  //     open cmdk sub-pages whose options are ordinary [cmdk-item] nodes, so the
  //     existing palette readers/activators (paletteHasCommand, paletteCommandTitles,
  //     activatePaletteCommand, typeInCommandPalette) drive them additively.
  //     These two reads observe the applied result the user sees: the active
  //     theme is the <html> data-theme attribute, the interface language is
  //     the <html> lang attribute. The persisted server state is read back
  //     through helpers/api. ---
  /** The data-theme attribute of the document root (the applied theme), or "". */
  documentTheme(): Promise<string>;
  /** The lang attribute of the document root (set when the interface language changes). */
  documentLang(): Promise<string>;

  // --- palette creation entries (SHELL-086) ---
  //     The "Create" group commands open their own scoped creation surface:
  //     work-item/page/view/cycle/module/project open a modal dialog (separate
  //     from the palette's cmdk dialog), while workspace creation routes to a
  //     dedicated page. Reuses the existing palette readers/activators plus
  //     page-visible copy (the creation modal embeds closed cmdk pickers, so
  //     its title identifies it, not the absence of cmdk).

  // --- palette pickers: empty / no-results / no-recents (SHELL-093) ---
  //     A picker whose data is empty renders a plain empty line (e.g. "No
  //     labels found"), and a server search with no hits renders a no-results
  //     row ("No results found — Clear search"); no history/recents section
  //     ever appears. The empty line is plain text (not a cmdk item), so this
  //     scoped read finds any visible text inside the palette surface.
  /** Whether the open palette surface shows this visible text anywhere. */
  paletteHasText(text: string): Promise<boolean>;

  // --- browse route (SHELL-106, negative row) ---
  /** Open the workspace-level browse route for a work-item identifier (e.g. "PROJ-1"). */
  openBrowseWorkItem(workspaceSlug: string, identifier: string): Promise<void>;
  /** Whether the browse route rendered the project-scoped work-item detail view. */
  browseShowsWorkItemDetail(): Promise<boolean>;
  /** Whether any workspace-wide list/grid of work items exists on the browse route. */
  browseShowsWorkspaceWideList(): Promise<boolean>;

  // --- top-bar search box (SHELL-081) ---
  //     The top bar carries an always-visible plain text input that opens an
  //     inline (non-dialog) cmdk results panel on focus: the same command list
  //     plus grouped server hits as the modal palette. Escape clears the term
  //     and closes; an outside click closes; closing resets the term.
  /** The top-bar search input's placeholder, or null when absent. */
  topBarSearchPlaceholder(): Promise<string | null>;
  /** Focus (click) the top-bar search input. */
  focusTopBarSearch(): Promise<void>;
  /** Whether the top-bar inline results panel is currently open. */
  isTopBarResultsOpen(): Promise<boolean>;
  /** Type into the top-bar search input. */
  typeInTopBarSearch(text: string): Promise<void>;
  /** The current value of the top-bar search input. */
  topBarSearchValue(): Promise<string>;
  /** Visible command item titles in the top-bar results panel, in display order. */
  topBarResultsCommandTitles(): Promise<string[]>;
  /** Press a key while the top-bar search input is focused. */
  pressInTopBarSearch(key: string): Promise<void>;
  /** Click outside the top-bar panel to close it. */
  closeTopBarViaOutsideClick(): Promise<void>;

  // --- shared empty-state kit tiers (SHELL-104) ---
  //     The kit tiers are distinguished by rendered structure: Simple centers
  //     an optional illustration with a heading and never renders buttons;
  //     Detailed leads with text plus optional art and action buttons; Section
  //     is a compact box with an icon slot, title text and an optional action.
  //     Art resolves per the active theme. Empty states render full-page or
  //     embedded in a widget, so the reader scopes to the box carrying the
  //     given exact title text instead of to a page region.
  /**
   * The rendered structure of the empty-state box titled `title`: its
   * description (or null), illustration src (or null) and visible button
   * labels in display order — or null when no such box is visible.
   */
  titledEmptyState(title: string): Promise<{
    description: string | null;
    imageSrc: string | null;
    buttons: string[];
  } | null>;
  /** Click the `label` action button inside the empty-state box titled `title`. */
  clickEmptyStateAction(title: string, label: string): Promise<void>;
  /** Type into the issue-search modal's search box (placeholder "Type to search"). */
  typeInIssueSearchModal(text: string): Promise<void>;

  // --- cover-image primitive (SHELL-105) ---
  //     Project cards and detail headers render covers through one primitive:
  //     a missing source shows a shimmer placeholder unless the caller opts
  //     into default art; static and Unsplash URLs render as-is; anything else
  //     resolves through the file-URL helper; loaded art is a cover-fit image.
  /** Srcs of the cover images on the projects list, in card order (null when a card shows the shimmer). */
  projectCardCoverSrcs(): Promise<(string | null)[]>;
  /** Whether any project card currently shows the cover shimmer placeholder. */
  projectCardCoverShimmerVisible(): Promise<boolean>;

  // -------------------------------------------------------------------------
  // Projects list + lifecycle (NEWFRONT-124, rows SHELL-024..045).
  // Added additively to the interface (never forking a driver). Single-user
  // scenarios sign in through the UI; multi-user scenarios (join, leave,
  // non-member card state, guest gating) enter the app pre-authenticated by
  // injecting a minted user's session cookies, then drive the list UI.
  // (Generic entries this area also needs — pre-authenticated entry,
  // current path, visible-text read, projects-list entry, context-menu
  // click, archive confirm — are owned by sibling areas and reused here.)
  // -------------------------------------------------------------------------

  // --- list + responsive grid (SHELL-024, SHELL-025) ---
  /** Open the archived-projects view of a workspace. */
  openArchivedProjects(workspaceSlug: string): Promise<void>;
  /** Names shown on the project cards currently rendered, in display order. */
  visibleProjectCardNames(): Promise<string[]>;
  /** Wait until at least one project card (by name) is rendered. */
  awaitProjectCard(name: string): Promise<void>;
  /** Number of grid columns the card grid resolves to at the current viewport. */
  gridColumnCount(): Promise<number>;
  /** Set the browser viewport width (px), keeping a tall height. */
  setViewportWidth(width: number): Promise<void>;
  /** Whether the list is showing its loading skeleton (shimmer cards, no names). */
  isProjectsSkeletonVisible(): Promise<boolean>;

  // --- empty / no-match / archive-empty (SHELL-026, SHELL-027) ---
  /** The visible empty/no-match/archive-empty state heading, or null. */
  emptyStateHeading(): Promise<string | null>;
  /** Whether the empty-state primary action (create first project) is present. */
  isEmptyStateCreateVisible(): Promise<boolean>;
  /** Whether that empty-state action is enabled (guests get it disabled). */
  isEmptyStateCreateEnabled(): Promise<boolean>;
  /** Click the empty-state create action (opens the create modal). */
  clickEmptyStateCreate(): Promise<void>;
  /** A signature of the empty-state artwork (distinguishes the art variants), or null. */
  emptyStateArtworkSignature(): Promise<string | null>;

  // --- desktop + mobile header (SHELL-043, SHELL-044, SHELL-045) ---
  /** Whether the header create button is present (hidden from guests / archive view). */
  isHeaderCreateButtonVisible(): Promise<boolean>;
  /** The visible label on the header create button, or null when absent. */
  headerCreateButtonLabel(): Promise<string | null>;
  /** Click the header create button (opens the create modal). */
  clickHeaderCreateButton(): Promise<void>;
  /** The breadcrumb labels shown in the list header, in order. */
  breadcrumbLabels(): Promise<string[]>;
  /** Whether the mobile (touch) list header bar is the one shown at this width. */
  isMobileListHeaderVisible(): Promise<boolean>;
  /** Whether the desktop filter row is shown at this width. */
  isDesktopFilterRowVisible(): Promise<boolean>;
  /** Whether the terminal (last) breadcrumb crumb is a link rather than plain text. */
  breadcrumbTerminalIsLink(): Promise<boolean>;

  // --- sort (SHELL-028) ---
  openSortMenu(): Promise<void>;
  /** Choose a sort key by its visible label (e.g. "Name", "Manual"). */
  selectSortOption(label: string): Promise<void>;
  /** The current sort key label shown on the sort trigger. */
  currentSortLabel(): Promise<string>;
  /** Whether the direction items (Ascending/Descending) are disabled in the open menu. */
  isSortDirectionDisabled(): Promise<boolean>;
  closeMenu(): Promise<void>;

  // --- filter panel + applied strip (SHELL-029, SHELL-030) ---
  openFilterMenu(): Promise<void>;
  /** Type into the filter panel's value-search box. */
  typeFilterSearch(text: string): Promise<void>;
  /** Whether the filter panel currently lists an option with this visible text. */
  filterMenuHasOption(text: string): Promise<boolean>;
  /** Choose a filter option by its visible label in the open panel. */
  selectFilterOption(label: string): Promise<void>;
  /** Whether the filter trigger shows its active-filters badge/dot. */
  isFilterBadgeVisible(): Promise<boolean>;
  /** Texts of the applied-filter chips currently in the strip. */
  appliedFilterChipTexts(): Promise<string[]>;
  /** Remove one applied-filter value chip by its visible text. */
  removeAppliedFilterChip(text: string): Promise<void>;
  /** Click the strip's "Clear all". */
  clickClearAllFilters(): Promise<void>;
  /** The match-count text shown beside the strip (e.g. "1/3"), or null. */
  filterMatchCountText(): Promise<string | null>;

  // --- list search (SHELL-031) ---
  /** Expand the list search from its magnifier icon. */
  openListSearch(): Promise<void>;
  typeListSearch(text: string): Promise<void>;
  listSearchValue(): Promise<string>;
  /** Whether the search input is currently expanded/visible. */
  isListSearchExpanded(): Promise<boolean>;
  pressEscapeInListSearch(): Promise<void>;
  clickListSearchClear(): Promise<void>;
  /** Click a neutral area outside the search to test outside-click collapse. */
  clickOutsideListSearch(): Promise<void>;

  // --- card contents (SHELL-032) ---
  /** The short code (identifier) shown on a project card, or null. */
  cardShortCode(name: string): Promise<string | null>;
  /** Whether the card shows the private (lock) mark. */
  cardHasPrivateMark(name: string): Promise<boolean>;
  /** The card's description-or-generated sub-line text, or null. */
  cardSubText(name: string): Promise<string | null>;
  /** Whether the favorite star control is rendered on the card. */
  cardHasFavoriteStar(name: string): Promise<boolean>;
  /** Toggle the favorite star on a card. */
  clickFavoriteStar(name: string): Promise<void>;
  /** Whether the card renders its cover image (as opposed to a loading block). */
  cardHasCoverImage(name: string): Promise<boolean>;
  /** Whether the card renders the project logo/icon next to its name. */
  cardHasLogo(name: string): Promise<boolean>;
  /** Texts of the member-avatar stack circles (initials plus any "+N" overflow). */
  cardAvatarStack(name: string): Promise<string[]>;

  // --- routing + quick actions (SHELL-033, SHELL-034) ---
  /** Click a project card body (routing / intercept depends on membership). */
  clickProjectCard(name: string): Promise<void>;
  /** Open a card's right-click context menu. */
  openCardContextMenu(name: string): Promise<void>;
  /** Labels of the items in the currently open context menu. */
  contextMenuItemLabels(): Promise<string[]>;
  /** Click a card context-menu entry by its visible label. */
  clickCardContextMenuItem(label: string): Promise<void>;
  /** Labels of the inline footer actions on a card (Join / Settings / Restore / Archived …). */
  cardFooterLabels(name: string): Promise<string[]>;
  /** Click a card's inline footer "Join" control. */
  clickCardJoin(name: string): Promise<void>;

  // --- join flow (SHELL-035) ---
  isJoinDialogVisible(): Promise<boolean>;
  /** The join dialog heading text, or null. */
  joinDialogHeading(): Promise<string | null>;
  confirmJoin(): Promise<void>;

  // --- leave flow (SHELL-036) ---
  /** Open the leave-project dialog for a project (via its reachable entry point). */
  openLeaveProjectDialog(projectName: string): Promise<void>;
  fillLeaveProjectName(text: string): Promise<void>;
  fillLeaveConfirmPhrase(text: string): Promise<void>;
  submitLeave(): Promise<void>;
  /** The visible leave error/toast text after a failed submit, or null. */
  leaveErrorText(): Promise<string | null>;
  isLeaveDialogVisible(): Promise<boolean>;

  // --- archive / restore (SHELL-037, SHELL-038) ---
  /** Open the archive dialog from the project's settings control section. */
  openArchiveProjectDialog(workspaceSlug: string, projectId: string): Promise<void>;
  /** The archive/restore dialog body text, or null. */
  archiveDialogBodyText(): Promise<string | null>;
  /** Click the inline restore control on an archived card. */
  clickCardRestore(name: string): Promise<void>;
  confirmRestore(): Promise<void>;
  /** Whether an archived card exposes inline restore/delete admin actions. */
  archivedCardHasAdminActions(name: string): Promise<boolean>;
  /** Whether the restore confirmation dialog is currently open. */
  isRestoreDialogVisible(): Promise<boolean>;
  /** Whether an archived card shows its muted "Archived" marker. */
  cardShowsArchivedMarker(name: string): Promise<boolean>;

  // --- delete (SHELL-039) ---
  /** Open the delete dialog from an archived card (menu or footer). */
  openDeleteProjectDialog(name: string): Promise<void>;
  fillDeleteProjectName(text: string): Promise<void>;
  fillDeleteConfirmPhrase(text: string): Promise<void>;
  isDeleteSubmitDisabled(): Promise<boolean>;
  submitDelete(): Promise<void>;

  // --- create flow (SHELL-040, SHELL-041, SHELL-042) ---
  isCreateProjectDialogVisible(): Promise<boolean>;
  fillCreateProjectName(text: string): Promise<void>;
  /** The current value of the auto-derived short-code field. */
  createProjectShortCodeValue(): Promise<string>;
  fillCreateProjectShortCode(text: string): Promise<void>;
  submitCreateProject(): Promise<void>;
  /** The visible create error text (duplicate name/code, upload warning), or null. */
  createProjectErrorText(): Promise<string | null>;
  /** Whether the open create form shows its prefilled cover image. */
  createFormCoverVisible(): Promise<boolean>;
  /** Whether the open create form shows its prefilled project icon. */
  createFormIconVisible(): Promise<boolean>;

  // --- Invitation inbox + onboarding start (NEWFRONT-110, AUTH-026/033).
  // --- Appended; existing methods above are untouched per the shared driver
  // --- contract.
  /** Open the invitation inbox; requires an authenticated session. */
  openInvitations(): Promise<void>;
  /** Workspace names of the pending-invitation cards currently shown. */
  invitationWorkspaceNames(): Promise<string[]>;
  /** Toggle the invitation card for a workspace (select / deselect). */
  toggleInvitation(workspaceName: string): Promise<void>;
  /** Accept and join every selected invitation. */
  acceptSelectedInvitations(): Promise<void>;
  /** True once the inbox empty state (no pending invites) is visible. */
  invitationsEmptyStateVisible(): Promise<boolean>;
  /** Open an emailed single-invitation link; works signed in or out. */
  openInvitationLink(workspaceSlug: string, invitationId: string, token: string): Promise<void>;
  /** User-visible text of the current page (headings, card titles, states). */
  pageText(): Promise<string>;
  /** Accept the pending single invitation on screen. */
  acceptSingleInvitation(): Promise<void>;
  /** Decline (ignore) the pending single invitation on screen. */
  declineSingleInvitation(): Promise<void>;
  /** Open the onboarding flow; requires an authenticated session. */
  openOnboarding(): Promise<void>;
  /** Advance past the CLI-install step via the primary continue action. */
  advanceCliInstall(): Promise<void>;
  /** Advance past the CLI-install step via the skip action. */
  skipCliInstall(): Promise<void>;
  /** Fill the profile step name field and submit. */
  submitProfileStep(displayName: string): Promise<void>;
  /** Choose a role and submit the role step. */
  submitRoleStep(roleLabel: string): Promise<void>;
  /** Skip the role step without choosing. */
  skipRoleStep(): Promise<void>;
  /** Choose use cases and submit the use-case step. */
  submitUseCaseStep(useCaseLabels: string[]): Promise<void>;
  /** Skip the use-case step without choosing. */
  skipUseCaseStep(): Promise<void>;
  /** Move one step back via the onboarding header back control. */
  goBackOnboardingStep(): Promise<void>;

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
  /** Activate a sheet row's sub-issue toggle once (no child wait). */
  layoutsSheetToggleSubIssues(issueName: string): Promise<void>;
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
  /** Press-travel gesture from one day tile to another (mobile no-drag pin). */
  layoutsCalTileDrag(fromDayNumber: number, toDayNumber: number): Promise<void>;
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
  /** Replace the open work-item modal title field. */
  layoutsWorkItemModalSetTitle(title: string): Promise<void>;
  /** Submit the open work-item modal; resolves once it closes. */
  layoutsWorkItemModalSubmit(): Promise<void>;
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
  /**
   * Add a filter condition through the filter row's own add control
   * (property menu, then value menu). The row must already be visible
   * with at least one pill — callers seed one first — because the page
   * header control is unreliable before the row renders.
   */
  layoutsFilterAddConditionViaRow(propertyLabel: string, valueLabel: string): Promise<void>;
  /**
   * Seed the archived page's locally stored filter and reload so its row
   * renders with a live pill. The archived page never consults server
   * preferences, so API seeding cannot reach it.
   */
  layoutsSeedArchivedLocalFilter(workspaceSlug: string, projectId: string, expression: unknown): Promise<void>;
  /** Whether the profile activity tab's Recent-activity chrome is rendered. */
  layoutsProfileActivityVisible(): Promise<boolean>;

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
  /** Requests held by the mutation stall since it was armed. */
  layoutsStalledMutationCount(): Promise<number>;
  /** Release any stalls installed by the two methods above. */
  layoutsReleaseStalls(): Promise<void>;
  /** Current page URL (navigation assertions). */
  layoutsCurrentUrl(): Promise<string>;
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
  /**
   * Attempt a card move the board may refuse; resolves after a fixed wait
   * instead of settling. Refused drops move nothing: callers assert the
   * card's final place via kanbanCards.
   */
  kanbanAttemptCardBefore(sourceName: string, targetName: string): Promise<void>;
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
  /** Card titles in one swimlane cell, top to bottom. */
  kanbanCellCards(columnName: string, laneName: string): Promise<string[]>;
  /** Whether one swimlane cell shows its explicit load-more entry. */
  kanbanCellHasLoadMore(columnName: string, laneName: string): Promise<boolean>;
  /** Activate one swimlane cell's load-more entry; resolves once it settles. */
  kanbanCellLoadMore(columnName: string, laneName: string): Promise<void>;
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
  /**
   * Attempt a sidebar reorder the timeline may refuse; resolves after a
   * fixed wait instead of settling. Refused drags move nothing: callers
   * assert the order via ganttSidebarOrder.
   */
  ganttAttemptRowBefore(sourceName: string, targetName: string): Promise<void>;
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
  /**
   * Reload with the issues API delayed so the loading state is observable,
   * and report whether sidebar skeletons showed before the timeline loaded.
   * The delay is test-only network shaping; the skeleton itself is the
   * behavior under test.
   */
  ganttLoadingObservedOnReload(): Promise<boolean>;
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

/** Canonical issue-layout keys shared by both frontend drivers. */
export type LayoutsLayoutKey = "list" | "kanban" | "calendar" | "spreadsheet" | "gantt_chart";
