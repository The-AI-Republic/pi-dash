// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
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
  // Workspace drafts oracle (NEWFRONT-32, DRAFT-001–027). The old app
  // nests the drafts content inside the shell's outer main landmark, so
  // the screen-scoped reads below target the inner main (the one holding
  // the drafts header and list), never the sidebar chrome. Row pickers
  // sit in fixed order (state, priority, labels, start, due, assignees,
  // then at most one conditional: modules, cycle, or estimate); the
  // conditional methods assume the scenario enabled exactly one.
  /** Tab title on the drafts screen. */
  draftsPageTitle(): Promise<string>;
  /** Header-area text (breadcrumb, count, actions) on the drafts screen. */
  draftsHeaderText(): Promise<string>;
  /** Count-chip text next to the drafts title, or null when no chip shows. */
  draftsCountChip(): Promise<string | null>;
  /** Header create control: enabled, disabled, or absent for this viewer. */
  draftsHeaderCreateState(): Promise<"enabled" | "disabled" | "absent">;
  /** Inner text of the drafts content area (header plus list, no shell chrome). */
  draftsMainText(): Promise<string>;
  /** Draft titles in render order (top row first). */
  draftRowNames(): Promise<string[]>;
  /** Project identifier rendered in the named draft's row. */
  draftRowProjectMarker(name: string): Promise<string>;
  /** Whether the named draft's row carries a work-type mark beside the project marker. */
  draftRowTypeMarkPresent(name: string): Promise<boolean>;
  /** Entry texts of the named draft's quick-action row menu, in display order. */
  draftRowMenuEntries(name: string): Promise<string[]>;
  /** Entry texts of the named draft's right-click menu, in display order. */
  draftContextMenuEntries(name: string): Promise<string[]>;
  /** Click an entry of the named draft's quick-action row menu ("edit", "make_a_copy", "move_to_project", "delete" on the oracle; the new app maps these keys to its translated labels). */
  draftRowMenuClick(name: string, entry: string): Promise<void>;
  /** Whether the named draft's row exposes a quick-action (ellipsis) trigger. */
  draftRowQuickActionVisible(name: string): Promise<boolean>;
  /** Open the named draft for editing through its row-menu edit entry. */
  editDraftByName(name: string): Promise<void>;
  /** Open the named draft's delete confirmation, then cancel it. */
  cancelDraftDelete(name: string): Promise<void>;
  /** Confirm the open delete-draft dialog. */
  confirmDraftDelete(): Promise<void>;
  /** Workflow-state text rendered in the named draft's row. */
  draftStateText(name: string): Promise<string>;
  /** Open the state picker of the named draft's row. */
  draftOpenStatePicker(name: string): Promise<void>;
  /** Priority name rendered in the named draft's row (mapped from its icon: the trigger is icon-only). */
  draftPriorityText(name: string): Promise<string>;
  /** Open the priority picker of the named draft's row. */
  draftOpenPriorityPicker(name: string): Promise<void>;
  /** Labels text rendered in the named draft's row. */
  draftLabelsText(name: string): Promise<string>;
  /** Open the labels picker of the named draft's row. */
  draftOpenLabelsPicker(name: string): Promise<void>;
  /** Assignees text rendered in the named draft's row. */
  draftAssigneesText(name: string): Promise<string>;
  /** Open the assignees picker of the named draft's row. */
  draftOpenAssigneesPicker(name: string): Promise<void>;
  /** Dates text rendered in the named draft's row. */
  draftDatesText(name: string): Promise<string>;
  /** Open the start-date picker of the named draft's row. */
  draftOpenStartDatePicker(name: string): Promise<void>;
  /** Open the due-date picker of the named draft's row. */
  draftOpenDueDatePicker(name: string): Promise<void>;
  /** Whether the open row picker marks the named option as selected. */
  draftPickerOptionSelected(text: string): Promise<boolean>;
  /** Estimate text rendered in the named draft's row ("" when unset). */
  draftEstimateText(name: string): Promise<string>;
  /** Open the estimate picker of the named draft's row. */
  draftOpenEstimatePicker(name: string): Promise<void>;
  /** Whether the named draft's row renders an estimate control. */
  draftEstimatePickerVisible(name: string): Promise<boolean>;
  /** Cycle text rendered in the named draft's row ("" when unset). */
  draftCycleText(name: string): Promise<string>;
  /** Open the cycle picker of the named draft's row. */
  draftOpenCyclePicker(name: string): Promise<void>;
  /** Whether the named draft's row renders a cycle control. */
  draftCyclePickerVisible(name: string): Promise<boolean>;
  /** Modules text rendered in the named draft's row ("" when unset). */
  draftModuleText(name: string): Promise<string>;
  /** Open the modules picker of the named draft's row. */
  draftOpenModulePicker(name: string): Promise<void>;
  /** Whether the named draft's row renders a modules control. */
  draftModulePickerVisible(name: string): Promise<boolean>;
  /** Fail the next drafts PATCH with `status` after `delayMs`, once (rollback probe); the route removes itself. */
  draftFailNextPatch(status: number, delayMs: number): Promise<void>;
  /** Remove a pending drafts-PATCH failure route (rollback-probe cleanup). */
  draftClearPatchFailure(): Promise<void>;
  /** "METHOD path" entries for draft-scoped API calls fired by entering the drafts screen (assumes a signed-in session). */
  draftsEntryRequests(workspaceSlug: string): Promise<string[]>;
  /** Count of draft rows marked draggable. */
  draftRowsDraggableCount(): Promise<number>;
  /** Press a key on the drafts screen (shortcut-absence probe). */
  draftsPressKey(key: string): Promise<void>;
  /** Failure notice text after a publish attempt, or null when none shows. */
  draftPublishError(): Promise<string | null>;
  /** Drafts empty-state create action: enabled, disabled, or absent. */
  draftsEmptyCreateState(): Promise<"enabled" | "disabled" | "absent">;
  /** No-project fallback create action: enabled, disabled, or absent. */
  draftsNoProjectCreateState(): Promise<"enabled" | "disabled" | "absent">;
  /** Hold the drafts-list answers for `ms` so the skeleton stays observable. */
  draftsDelayList(ms: number): Promise<void>;
  /** Whether loading placeholder rows are currently rendered. */
  draftsSkeletonVisible(): Promise<boolean>;
  /** Whether the list footer offers loading more drafts. */
  draftsLoadMoreVisible(): Promise<boolean>;
  /** Load the next drafts page through the list footer control. */
  draftsLoadMore(): Promise<void>;
  /** Drafts-list API calls fired by a blur/focus cycle (refetch-on-focus probe). */
  draftsListFetchCountOnRefocus(): Promise<number>;
  /** Checkbox count inside the main landmark (bulk-select absence probe). */
  draftsMainCheckboxCount(): Promise<number>;
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
  /** Notices currently visible, newest first, with their success/error kind. */
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
  /** Click the named row and capture the file it opens as a download; requires the new tab. */
  downloadWidgetRowFile(widget: string, rowName: string): Promise<{ fileName: string; bytes: Buffer }>;
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
  // --- Project settings workflow states (NEWFRONT-392, PSET-028–035).
  // --- Observed on the running old app: the States page renders seven
  // --- fixed group cards (backlog, unstarted, started, review, test,
  // --- completed, cancelled) with collapsible headers, an inline
  // --- name/description/color form docked per group, pragmatic
  // --- drag-and-drop rows with top/bottom placement, a delete
  // --- confirmation with default/last-in-group guards, an exclusive
  // --- default flag, and an admin-only member-editing switch.
  /** Open a project's settings States page; resolves once it settles. */
  settingsStatesOpen(workspaceSlug: string, projectId: string): Promise<void>;
  /** Group-card keys in render order (lowercase, e.g. "backlog"). */
  settingsStatesGroupNames(): Promise<string[]>;
  /** Whether the group card `group` is currently expanded. */
  settingsStatesGroupExpanded(group: string): Promise<boolean>;
  /** Toggle the group card `group` between collapsed and expanded. */
  settingsStatesToggleGroup(group: string): Promise<void>;
  /** State names inside the group card `group`, in display order. */
  settingsStatesStateNames(group: string): Promise<string[]>;
  /** Whether the group card's add button renders enabled. */
  settingsStatesAddEnabled(group: string): Promise<boolean>;
  /** Open the inline create form in the group card `group`. */
  settingsStatesOpenCreate(group: string): Promise<void>;
  /** Whether the inline state form is currently open. */
  settingsStatesFormVisible(): Promise<boolean>;
  /** Fill the inline form's name field. */
  settingsStatesFillName(name: string): Promise<void>;
  /** Fill the inline form's description field. */
  settingsStatesFillDescription(text: string): Promise<void>;
  /** Current value of the inline form's name field. */
  settingsStatesNameValue(): Promise<string>;
  /** Current value of the inline form's description field. */
  settingsStatesDescriptionValue(): Promise<string>;
  /** Inline form error text, or "" when the form shows none. */
  settingsStatesFormError(): Promise<string>;
  /** Submit the inline create form; resolves once it closes. */
  settingsStatesSubmitCreate(): Promise<void>;
  /** Submit the inline form in update mode; resolves once it closes. */
  settingsStatesSubmitUpdate(): Promise<void>;
  /** Close the inline form without saving. */
  settingsStatesCancelForm(): Promise<void>;
  /** Click the inline form's submit button without waiting for close. */
  settingsStatesAttemptSubmit(): Promise<void>;
  /** Background color of the inline form's swatch, as rendered. */
  settingsStatesDotColor(): Promise<string>;
  /** Open the inline form's color picker popup. */
  settingsStatesOpenColorPicker(): Promise<void>;
  /** Pick the swatch for `hex` in the open color picker. */
  settingsStatesPickColor(hex: string): Promise<void>;
  /** Swap the row `name` for the prefilled inline edit form. */
  settingsStatesOpenEdit(name: string): Promise<void>;
  /** Whether the row's drag grip is visible after hovering it. */
  settingsStatesGripVisible(name: string): Promise<boolean>;
  /** Drag the state `source` above `target` (same or another group). */
  settingsStatesDragAbove(source: string, target: string): Promise<void>;
  /** Drag the state `source` below `target` (same or another group). */
  settingsStatesDragBelow(source: string, target: string): Promise<void>;
  /** Whether the row's delete control is currently disabled. */
  settingsStatesDeleteDisabled(name: string): Promise<boolean>;
  /** Open the delete confirmation for the state `name`. */
  settingsStatesDelete(name: string): Promise<void>;
  /** Body text of the open delete-state confirmation, or "" when shut. */
  settingsStatesDeleteModalText(): Promise<string>;
  /** Confirm the delete-state modal; resolves once it closes. */
  settingsStatesDeleteConfirm(): Promise<void>;
  /** Dismiss the delete-state modal without deleting. */
  settingsStatesDeleteCancel(): Promise<void>;
  /** Click the delete modal's submit without waiting for close. */
  settingsStatesAttemptDeleteConfirm(): Promise<void>;
  /** Promote the state `name` to the project default. */
  settingsStatesMarkDefault(name: string): Promise<void>;
  /** Whether the row `name` currently carries the default marker. */
  settingsStatesIsDefault(name: string): Promise<boolean>;
  /** Whether the admin-only member-editing switch row renders. */
  settingsStatesMemberToggleVisible(): Promise<boolean>;
  /** Whether the member-editing switch is currently on. */
  settingsStatesMemberToggleOn(): Promise<boolean>;
  /** Flip the member-editing switch to `on`. */
  settingsStatesSetMemberToggle(on: boolean): Promise<void>;
  /** Whether the states page shows the not-authorized view. */
  settingsStatesAccessDenied(): Promise<boolean>;
  /** Whether the states loading skeleton currently shows. */
  settingsStatesSkeletonVisible(): Promise<boolean>;
  /** Delay states-list API answers by `ms` so the skeleton is observable. */
  settingsStatesDelayLoad(ms: number): Promise<void>;
  /** Per-group empty note text, or "" when the group lists states. */
  settingsStatesEmptyNote(group: string): Promise<string>;
  // --- Project settings labels gaps (NEWFRONT-392, PSET-036–042). The
  // --- NEWFRONT-122 label methods above already cover create/edit/menus/
  // --- delete/drag/empty basics; these fill the hierarchy reads the
  // --- project-settings rows need.
  /** Child label names nested under the group `parent`, in order. */
  settingsLabelsChildrenOf(parent: string): Promise<string[]>;
  /** Toggle the group card `name` between expanded and collapsed. */
  settingsLabelsToggleGroup(name: string): Promise<void>;
  /** Whether the group card `name` is currently collapsed. */
  settingsLabelsGroupCollapsed(name: string): Promise<boolean>;
  /** Whether the row's drag grip is visible after hovering it. */
  settingsLabelsGripVisible(name: string): Promise<boolean>;
  /**
   * Drag the label `source` onto/above `target` with real mouse events,
   * reporting the positional feedback seen mid-gesture ("line" for the
   * reorder insertion line, "ring" for the nesting highlight ring). The
   * drop completes, so callers also assert the persisted outcome.
   */
  settingsLabelsDragFeedback(source: string, target: string, edge: "nest" | "above"): Promise<"line" | "ring" | "none">;
  /** Whether the labels list container currently scrolls vertically. */
  settingsLabelsListScrollable(): Promise<boolean>;
  /**
   * Click the Delete Label modal's Delete button without waiting for
   * the modal to close (for refused deletes, where it stays open).
   */
  settingsLabelsDeleteAttemptConfirm(): Promise<void>;

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
  /** Current scroll offsets of the board container (the horizontal scroller). */
  kanbanBoardScroll(): Promise<{ x: number; y: number }>;
  /** Current scroll offsets of one column's own vertical scroller. */
  kanbanColumnScroll(columnName: string): Promise<{ x: number; y: number }>;
  /**
   * Press on a card and hold it near one board edge for holdMs, then
   * release. Callers compare kanbanBoardScroll (horizontal holds) or
   * kanbanColumnScroll (vertical holds) before/after to observe
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
  /** Drag a bar body horizontally by whole days; resolves once the drag engages. */
  ganttDragBar(issueName: string, dayDelta: number): Promise<void>;
  /**
   * Attempt a bar move the timeline may refuse; resolves after a fixed
   * wait instead of engaging. Refused drags move nothing: callers assert
   * the persisted dates via the API.
   */
  ganttAttemptBarMove(issueName: string, dayDelta: number): Promise<void>;
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
   * and report whether the layout loader showed before the timeline did.
   * The delay is test-only network shaping; the loader itself is the
   * behavior under test.
   */
  ganttLoadingObservedOnReload(): Promise<boolean>;
  /** Whether the timeline shows its first-run empty state (no chart). */
  ganttEmptyVisible(): Promise<boolean>;
  /**
   * Reload with the issues API delayed, scroll a 100+ issue timeline to
   * its end, and report whether the load-more sentinel pulsed while the
   * next page fetched. The delay is test-only network shaping.
   */
  ganttLoadMoreObservedOnScroll(): Promise<boolean>;

  // --- Root document shell + not-found (NEWFRONT-173, SHELL-107/108) ---
  /** Structured read of the document head: metadata, icons, installability markers. */
  documentShellFacts(): Promise<DocumentShellFacts>;
  /** Whether the global overlay portal roots for menus and editors exist. */
  overlayPortalsPresent(): Promise<{ contextMenu: boolean; editor: boolean }>;
  /** Whether a session-recorder snippet is present in the document. */
  sessionRecorderPresent(): Promise<boolean>;
  /** HTTP statuses of the installability assets (manifests, icons) the head links to. */
  installAssetStatuses(): Promise<{ href: string; status: number }[]>;
  /** Facts about the rendered not-found surface, or null when none shows. */
  notFoundFacts(): Promise<NotFoundFacts | null>;
  /** Activate the not-found surface's way home; resolves once navigation lands. */
  notFoundGoHome(): Promise<void>;
  /** Presence markers in the served (pre-hydration) document for a path. */
  servedShellMarkers(path: string): Promise<ServedShellMarkers>;

  // --- Desktop-only chat + agent runtime (NEWFRONT-182, RUN-033–036,
  // --- RUN-044–045, RUN-047). Appended; existing methods above are
  // --- untouched per the shared driver contract. The oracle proves the
  // --- web-observable side of desktop-only rows: absent contacts,
  // --- prompts, controls and banners, plus the absence of local-IPC
  // --- markers and desktop-endpoint traffic.
  /** Open the workspace runners area; requires an authenticated session. */
  desktopRuntimeOpenRunners(workspaceSlug: string): Promise<void>;
  /** Section headers rendered in the runners side nav, in display order. */
  desktopRuntimeRailSectionHeaders(): Promise<string[]>;
  /** Chat contacts rendered in the runners side nav (name plus link target). */
  desktopRuntimeRailChatLinks(): Promise<{ name: string; href: string }[]>;
  /** Open one runner's chat page; requires an authenticated session. */
  desktopRuntimeOpenChat(workspaceSlug: string, runnerId: string, sessionId?: string): Promise<void>;
  /** Whether an inline approval prompt shows above the chat composer. */
  desktopRuntimeApprovalPromptVisible(): Promise<boolean>;
  /** Whether the approval-mode control shows in the chat header. */
  desktopRuntimeApprovalModeVisible(): Promise<boolean>;
  /** Whether the background runtime's status banner is rendered. */
  desktopRuntimeRuntimeBannerVisible(): Promise<boolean>;
  /** Whether the page exposes the desktop native bridge. */
  desktopRuntimeIsTauriPresent(): Promise<boolean>;
  /** Chat transcript bubbles (role plus text), in display order. */
  desktopRuntimeChatBubbles(): Promise<{ role: string; text: string }[]>;
  /** Web-storage keys the app currently holds (residue checks). */
  desktopRuntimeStorageKeys(): Promise<{ local: string[]; session: string[] }>;
  /** Start logging every request URL the page issues. */
  desktopRuntimeStartRequestSpy(): Promise<void>;
  /** Request URLs logged since the spy started. */
  desktopRuntimeSpyUrls(): Promise<string[]>;
  /** Stop the request spy and forget the logged URLs. */
  desktopRuntimeStopRequestSpy(): Promise<void>;

  // --- Scheduler catalog + definitions (NEWFRONT-184, AGT-001..006, AGT-022).
  // --- Verbs stay generic (scheduler*) so sibling agents-area children reuse
  // --- them; later children extend, never rename.
  /** Open the workspace scheduler catalog; settles on the table, the gate panel, or the workspace not-found surface. */
  schedulerOpenCatalog(workspaceSlug: string): Promise<void>;
  /** Catalog table rows in display order (data rows only, never the empty guidance row). */
  schedulerCatalogRows(): Promise<SchedulerCatalogRow[]>;
  /** Whether the catalog shows its empty-guidance row. */
  schedulerCatalogEmptyVisible(): Promise<boolean>;
  /** The rendered tab title while the catalog shows. */
  schedulerPageTitle(): Promise<string>;
  /** Whether the catalog header offers the create control. */
  schedulerCreateVisible(): Promise<boolean>;
  /** Per-row action labels offered on one definition's row, in display order. */
  schedulerRowActions(handle: string): Promise<string[]>;
  /** Open the create dialog from the catalog header; waits for the form. */
  schedulerOpenCreate(): Promise<void>;
  /** Fill definition dialog fields; omitted fields are left untouched. */
  schedulerFillDefinition(input: {
    name?: string;
    handle?: string;
    description?: string;
    prompt?: string;
    color?: string;
  }): Promise<void>;
  /** Set the definition dialog's enabled switch to the desired state. */
  schedulerSetDefinitionEnabled(enabled: boolean): Promise<void>;
  /** Current values of every definition dialog field (for pre-fill assertions). */
  schedulerDefinitionValues(): Promise<SchedulerDefinitionValues>;
  /** Whether the definition dialog's handle field is locked. */
  schedulerDefinitionHandleLocked(): Promise<boolean>;
  /** Submit the definition dialog; resolves on click (a rejected submit keeps the dialog open). */
  schedulerSubmitDefinition(): Promise<void>;
  /** Whether the definition dialog is currently open. */
  schedulerDefinitionOpen(): Promise<boolean>;
  /** Inline validation texts currently shown in the definition dialog. */
  schedulerDefinitionErrors(): Promise<string[]>;
  /** Close the definition dialog via its cancel control. */
  schedulerCloseDefinition(): Promise<void>;
  /** Open the edit dialog for one definition; waits for the pre-filled form. */
  schedulerOpenEdit(handle: string): Promise<void>;
  /** Open the delete confirmation for one definition; waits for the dialog. */
  schedulerOpenDelete(handle: string): Promise<void>;
  /** Full text of the open delete confirmation (consequence copy plus definition name). */
  schedulerDeleteDialogText(): Promise<string>;
  /**
   * Confirm the open delete; resolves once the dialog closes, or once a
   * failure notice shows when the delete is rejected (the dialog stays
   * open for the scenario to assert).
   */
  schedulerConfirmDelete(): Promise<void>;
  /** Whether the delete confirmation is currently open. */
  schedulerDeleteOpen(): Promise<boolean>;
  /** Dismiss the open delete via its cancel control. */
  schedulerCancelDelete(): Promise<void>;
  /** Open one project's scheduler list; waits for the panel. */
  schedulerOpenProjectSchedulers(workspaceSlug: string, projectId: string): Promise<void>;
  /** Open the project-side create form on its "create new" tab; waits for the fields. */
  schedulerOpenProjectCreate(): Promise<void>;
  /** Submit the project-side create form; resolves on click (a rejected submit keeps it open). */
  schedulerProjectCreateSubmit(): Promise<void>;
  /** Fill the project-side create form's name field. */
  schedulerProjectCreateFillName(name: string): Promise<void>;
  /** Current value of the project-side create form's handle field. */
  schedulerProjectCreateHandleValue(): Promise<string>;
  /** Fill the project-side create form's handle field by hand. */
  schedulerProjectCreateFillHandle(handle: string): Promise<void>;
  /** Inline validation texts currently shown in the project-side create form. */
  schedulerProjectCreateErrors(): Promise<string[]>;
  /** Close the project-side create form via its cancel control. */
  schedulerCloseProjectCreate(): Promise<void>;
  /** Open the bulk-install dialog for one definition; waits for the picker. */
  schedulerOpenInstall(handle: string): Promise<void>;
  /** Bulk-install picker options in display order (opens the dropdown when needed). */
  schedulerInstallPickerOptions(): Promise<SchedulerInstallOption[]>;
  /** Filter the bulk-install picker by a search query. */
  schedulerInstallSearch(query: string): Promise<void>;
  /** Toggle the bulk-install picker's select-all control. */
  schedulerInstallToggleSelectAll(): Promise<void>;
  /** Toggle one project in the bulk-install picker by name. */
  schedulerInstallToggleProject(name: string): Promise<void>;
  /** The bulk-install picker's current selection summary text. */
  schedulerInstallSelectedSummary(): Promise<string>;
  /** Submit the bulk-install dialog; resolves on click (failures keep it open). */
  schedulerInstallSubmit(): Promise<void>;
  /** Whether the bulk-install dialog is currently open. */
  schedulerInstallOpen(): Promise<boolean>;
  /** Close the bulk-install dialog via its cancel control. */
  schedulerCloseInstall(): Promise<void>;
  /** Every toast currently visible, newest first (for partitioned outcomes). */
  schedulerVisibleToasts(): Promise<{ title: string; message: string }[]>;
  /** Open the workspace prompts route; settles on the page, the gate panel, or the workspace not-found surface. */
  schedulerOpenPrompts(workspaceSlug: string): Promise<void>;
  /** Whether the not-authorized panel is currently shown. */
  schedulerNotAuthorizedVisible(): Promise<boolean>;
  /** Whether the workspace not-found surface is currently shown. */
  schedulerWorkspaceNotFoundVisible(): Promise<boolean>;
  /** How many workspace sidebars are mounted (a second shell would double it). */
  schedulerShellCount(): Promise<number>;

  // --- Dev machines, runner detail, agent activity (NEWFRONT-183, RUN-037–043) ---
  /** Open the workspace AI-dev-machines page; resolves once the machine table settles. */
  devMachinesOpen(workspaceSlug: string): Promise<void>;
  /** Machine rows in display order: name, subline, status badge, counts, times, row actions. */
  devMachinesRows(): Promise<DevMachineRow[]>;
  /** One machine row by display name, or null when no row shows it. */
  devMachinesRowByName(name: string): Promise<DevMachineRow | null>;
  /** Whether the table shows its empty state. */
  devMachinesEmptyVisible(): Promise<boolean>;
  /** Whether the table shows its loading state. */
  devMachinesLoadingVisible(): Promise<boolean>;
  /** Whether the table shows its error state. */
  devMachinesErrorVisible(): Promise<boolean>;
  /** Serve the next machines-list GET with `rows` (empty-state shaping). */
  devMachinesStubListOnce(rows: unknown[]): Promise<void>;
  /** Fail the next machines-list GET once (error-state shaping). */
  devMachinesFailListOnce(): Promise<void>;
  /** Delay the next machines-list GET by `ms` (loading-state shaping). */
  devMachinesDelayListOnce(ms: number): Promise<void>;
  /** Count machines-list GETs across `windowMs` (refresh-cadence proof). */
  devMachinesListPollCount(windowMs: number): Promise<number>;
  /** Open the rotate confirm for the named machine's row. */
  devMachinesOpenRotate(name: string): Promise<void>;
  /** Open the revoke confirm for the named machine's row. */
  devMachinesOpenRevoke(name: string): Promise<void>;
  /** Open the delete confirm for the named machine's row. */
  devMachinesOpenDelete(name: string): Promise<void>;
  /** The open machine confirm modal, or null when none shows. */
  devMachinesModal(): Promise<DevMachineModal | null>;
  /** Whether a machine confirm modal is open. */
  devMachinesModalVisible(): Promise<boolean>;
  /** Activate the modal's confirm control (specs poll for the outcome). */
  devMachinesModalConfirm(): Promise<void>;
  /** Activate the modal's cancel/close control. */
  devMachinesModalCancel(): Promise<void>;
  /** Press Escape while the modal is open. */
  devMachinesModalPressEscape(): Promise<void>;
  /** Delay the next rotate/revoke/delete request by `ms` (in-flight shaping). */
  devMachinesDelayActionOnce(ms: number): Promise<void>;
  /** Fail the next rotate/revoke/delete request once (failure-toast shaping). */
  devMachinesFailActionOnce(): Promise<void>;
  /** Newest toast text, or null when no toast shows. */
  devMachinesLastToast(): Promise<string | null>;
  /** Record machine-delete request URLs until stopped (purge-flag proof). */
  devMachinesDeleteSpyStart(): Promise<void>;
  devMachinesDeleteSpyUrls(): Promise<string[]>;
  devMachinesDeleteSpyStop(): Promise<void>;
  /** Install cards in display order: platform label, command text, download link. */
  devMachinesInstallCards(): Promise<DevMachineInstallCard[]>;
  /** Activate the copy control of the named install card (grants clipboard first). */
  devMachinesInstallCopy(label: string): Promise<void>;
  /** The copy control's current label for the named card (transient confirm). */
  devMachinesInstallCopyState(label: string): Promise<string | null>;
  /** The install section's prerequisite note, or null when absent. */
  devMachinesInstallPrereq(): Promise<string | null>;
  /** Make the next clipboard write reject (copy-failure shaping). */
  devMachinesInstallBreakClipboard(): Promise<void>;
  /** Current clipboard text (grants clipboard permission first). */
  devMachinesReadClipboard(): Promise<string>;
  /** Open a runner's detail page (project scope when `projectId` is set); resolves once header, loader or error settles. */
  runnerDetailOpen(workspaceSlug: string, runnerId: string, projectId?: string): Promise<void>;
  /** Which detail surface shows: content, loader, or error. */
  runnerDetailState(): Promise<"loaded" | "loading" | "error">;
  /** Loaded detail header: runner name plus status badge, or null unless loaded. */
  runnerDetailHeader(): Promise<{ name: string; status: string } | null>;
  /** Metadata grid label/value pairs in display order. */
  runnerDetailMeta(): Promise<{ label: string; value: string }[]>;
  /** The back-to-list link target, or null when absent. */
  runnerDetailBackHref(): Promise<string | null>;
  /** Activate the into-chat control; resolves once navigation lands. */
  runnerDetailOpenChat(): Promise<void>;
  /** Count detail GETs for `runnerId` across `windowMs` (refresh-cadence proof). */
  runnerDetailPollCount(runnerId: string, windowMs: number): Promise<number>;
  /** Delay the next detail GET by `ms` (loading-state shaping). */
  runnerDetailDelayOnce(ms: number): Promise<void>;
  /** Fail the next detail GET once (error-state shaping). */
  runnerDetailFailOnce(): Promise<void>;
  /** The activity badge text, or null when the panel shows none. */
  runnerActivityBadge(): Promise<string | null>;
  /** Telemetry grid label/value pairs in display order. */
  runnerActivityTelemetry(): Promise<{ label: string; value: string }[]>;
  /** Whether the last-activity label visibly advances with server refetches blocked (local-tick proof). */
  runnerActivityAgingObserved(): Promise<boolean>;

  // --- Project scheduler installs + detail + calendar (NEWFRONT-185, AGT-007..021, AGT-062).
  /** Open one project's install list; settles on the table for any project role. */
  schedulerOpenProjectList(workspaceSlug: string, projectId: string): Promise<void>;
  /** Install list rows in display order (data rows only, never the empty row). */
  schedulerProjectRows(): Promise<SchedulerProjectRow[]>;
  /** Whether the install list shows its empty-state row. */
  schedulerProjectEmptyVisible(): Promise<boolean>;
  /** Whether the list header offers the New Scheduler control. */
  schedulerProjectNewVisible(): Promise<boolean>;
  /** Open an install's detail by clicking its list row; waits for the detail. */
  schedulerProjectRowOpen(handle: string): Promise<void>;
  /** Current enable-switch state of one list row. */
  schedulerProjectToggleState(handle: string): Promise<{ checked: boolean; disabled: boolean }>;
  /** Flip one list row's enable switch; resolves on click (the scenario polls for the outcome). */
  schedulerProjectToggle(handle: string): Promise<void>;
  /**
   * Stall the next toggle PATCH and report whether the switch gates input
   * mid-flight (the double-submit block). The stall releases before return;
   * the scenario polls the server row for the flip itself.
   */
  schedulerProjectToggleFlightGated(handle: string): Promise<boolean>;
  /** Open the edit dialog for one install from its list row; waits for the form. */
  schedulerProjectOpenEdit(handle: string): Promise<void>;
  /** Open the uninstall confirmation for one install from its list row; waits for the dialog. */
  schedulerProjectOpenUninstall(handle: string): Promise<void>;
  /** Full text of the open uninstall confirmation. */
  schedulerUninstallDialogText(): Promise<string>;
  /** Confirm the open uninstall; resolves once the dialog closes or a failure notice shows. */
  schedulerConfirmUninstall(): Promise<void>;
  /** Whether the uninstall confirmation is currently open. */
  schedulerUninstallOpen(): Promise<boolean>;
  /** Dismiss the open uninstall via its cancel control. */
  schedulerCancelUninstall(): Promise<void>;
  /** Open the New Scheduler modal; settles once its title shows. */
  schedulerOpenProjectInstall(): Promise<void>;
  /** Which path the open New Scheduler modal is on (or the dead-end variant). */
  schedulerProjectInstallMode(): Promise<"install" | "create" | "dead-end">;
  /** Full text of the dead-end variant (nothing installable, cannot author). */
  schedulerProjectInstallDeadEndText(): Promise<string>;
  /** Mode tabs offered by the open modal (empty when the viewer cannot author). */
  schedulerProjectInstallTabs(): Promise<string[]>;
  /** Switch the open modal to one mode path; waits for its fields. */
  schedulerProjectInstallSelectTab(tab: "Install existing" | "Create new"): Promise<void>;
  /** Definitions offered by the open modal's install-path picker, in display order. */
  schedulerProjectInstallOptions(): Promise<SchedulerProjectInstallOption[]>;
  /** Pick one definition in the install-path picker by handle. */
  schedulerProjectInstallSelect(handle: string): Promise<void>;
  /** Fill install-path schedule fields; omitted fields are left untouched. */
  schedulerProjectInstallFillSchedule(input: {
    dtstart?: string;
    tzid?: string;
    rrule?: string;
    extraContext?: string;
  }): Promise<void>;
  /** Current values of the install-path schedule fields. */
  schedulerProjectInstallScheduleValues(): Promise<SchedulerScheduleValues>;
  /** The live recurrence sentence under the install-path RRULE field. */
  schedulerProjectInstallHumanizer(): Promise<string>;
  /** Set the install-path enabled switch to the desired state. */
  schedulerProjectInstallSetEnabled(enabled: boolean): Promise<void>;
  /** Inline validation texts currently shown in the open modal. */
  schedulerProjectInstallErrors(): Promise<string[]>;
  /** Submit the open modal; resolves on click (a rejected submit keeps it open). */
  schedulerProjectInstallSubmit(): Promise<void>;
  /** Whether the open modal's submit control is currently disabled. */
  schedulerProjectInstallSubmitDisabled(): Promise<boolean>;
  /** Whether the New Scheduler modal is currently open. */
  schedulerProjectInstallOpen(): Promise<boolean>;
  /** Close the open modal via its cancel control. */
  schedulerCloseProjectInstall(): Promise<void>;
  /** Fill the project-side create form's description field. */
  schedulerProjectCreateFillDescription(description: string): Promise<void>;
  /** Fill the project-side create form's prompt field. */
  schedulerProjectCreateFillPrompt(prompt: string): Promise<void>;
  /** Current values of every edit-dialog field (for pre-fill assertions). */
  schedulerProjectEditValues(): Promise<SchedulerBindingValues>;
  /** Fill edit-dialog schedule fields; omitted fields are left untouched. */
  schedulerProjectEditFill(input: {
    dtstart?: string;
    tzid?: string;
    rrule?: string;
    extraContext?: string;
  }): Promise<void>;
  /** Set the edit dialog's enabled switch to the desired state. */
  schedulerProjectEditSetEnabled(enabled: boolean): Promise<void>;
  /** The live recurrence sentence under the edit dialog's RRULE field. */
  schedulerProjectEditHumanizer(): Promise<string>;
  /** Inline validation texts currently shown in the edit dialog. */
  schedulerProjectEditErrors(): Promise<string[]>;
  /** Submit the edit dialog; resolves on click (a rejected submit keeps it open). */
  schedulerProjectEditSubmit(): Promise<void>;
  /** Whether the edit dialog is currently open. */
  schedulerProjectEditOpen(): Promise<boolean>;
  /** Close the edit dialog via its cancel control. */
  schedulerCloseProjectEdit(): Promise<void>;
  /** Outcome-mode options in the open modal, with the help line for the current choice. */
  schedulerOutcomeState(): Promise<{ options: { label: string; checked: boolean }[]; help: string }>;
  /** Pick one outcome-mode option in the open modal by label. */
  schedulerOutcomeSelect(label: string): Promise<void>;
  /** Pod options in the open modal (the empty value is the project default). */
  schedulerPodOptions(): Promise<{ value: string; label: string; selected: boolean }[]>;
  /** Pick one pod in the open modal by option value ("" for the project default). */
  schedulerPodSelect(value: string): Promise<void>;
  /** Whether the open modal's pod selector is currently disabled (pods loading). */
  schedulerPodDisabled(): Promise<boolean>;
  /**
   * Stall the pod listing, open the New Scheduler modal, and report whether
   * the pod selector disables while pods load (then closes the modal).
   * Assumes the install list with a visible New control.
   */
  schedulerPodLoadingObserved(): Promise<boolean>;
  /** Open one install's detail page; settles on the config, or the removed-install panel. */
  schedulerOpenProjectBinding(workspaceSlug: string, projectId: string, bindingId: string): Promise<void>;
  /** Detail header facts (name, handle, badges, which management controls show). */
  schedulerBindingHeader(): Promise<SchedulerBindingHeader>;
  /** Detail configuration grid as label/value pairs in display order. */
  schedulerBindingConfig(): Promise<{ label: string; value: string }[]>;
  /** The stored-rule tooltip behind the detail's Schedule row. */
  schedulerBindingScheduleTitle(): Promise<string>;
  /** The last-error panel text, or null when no error is recorded. */
  schedulerBindingLastError(): Promise<string | null>;
  /** Project-context block text, or null when the install carries none. */
  schedulerBindingExtraContext(): Promise<string | null>;
  /** Whether the resolved-prompt toggle shows, and whether the prompt is revealed. */
  schedulerBindingPromptState(): Promise<{ toggleVisible: boolean; revealed: boolean }>;
  /** Toggle the resolved-prompt reveal. */
  schedulerBindingPromptToggle(): Promise<void>;
  /** The revealed resolved-prompt text (toggle it open first). */
  schedulerBindingPromptText(): Promise<string>;
  /** Run-history rows in display order. */
  schedulerBindingRuns(): Promise<SchedulerBindingRunRow[]>;
  /** The run-history empty-state text, or null when rows show. */
  schedulerBindingRunsEmpty(): Promise<string | null>;
  /** The run-history header count text ("N runs"). */
  schedulerBindingRunsCount(): Promise<string>;
  /** Run-history pager state, or null when no pager renders. */
  schedulerBindingRunsPager(): Promise<{ text: string; prevDisabled: boolean; nextDisabled: boolean } | null>;
  /** Step the run-history pager; waits for the page to settle. */
  schedulerBindingRunsPage(direction: "next" | "prev"): Promise<void>;
  /** Wait for the next run-history refetch; resolves on the response. */
  schedulerBindingWaitRunsRefetch(): Promise<void>;
  /** Whether the removed-install panel shows (with its way back). */
  schedulerBindingRemovedVisible(): Promise<boolean>;
  /** Follow the removed-install panel's way back; waits for the list. */
  schedulerBindingBackToList(): Promise<void>;
  /** Current enable-switch state in the detail configuration. */
  schedulerBindingToggleState(): Promise<{ checked: boolean; disabled: boolean }>;
  /** Flip the detail enable switch; resolves on click. */
  schedulerBindingToggle(): Promise<void>;
  /** Open the edit dialog from the detail page; waits for the form. */
  schedulerBindingOpenEdit(): Promise<void>;
  /** Open the uninstall confirmation from the detail page; waits for the dialog. */
  schedulerBindingOpenUninstall(): Promise<void>;
  /** Open one project's firing calendar; settles on the grid or the empty state. */
  schedulerOpenProjectCalendar(workspaceSlug: string, projectId: string): Promise<void>;
  /** Which calendar view is active. */
  schedulerCalendarView(): Promise<"week" | "month">;
  /** Switch the calendar to one view; waits for the grid to settle. */
  schedulerCalendarSetView(view: "week" | "month"): Promise<void>;
  /** The calendar header's month-year label. */
  schedulerCalendarTitle(): Promise<string>;
  /** Step the calendar one period; waits for the grid to settle. */
  schedulerCalendarStep(direction: "prev" | "next"): Promise<void>;
  /** Jump the calendar back to today; waits for the grid to settle. */
  schedulerCalendarToday(): Promise<void>;
  /** Whether the calendar shows its no-installs empty state. */
  schedulerCalendarEmptyVisible(): Promise<boolean>;
  /** Whether the calendar shows its too-many-occurrences hint. */
  schedulerCalendarTruncatedVisible(): Promise<boolean>;
  /** Month-grid occurrence blocks in display order. */
  schedulerCalendarMonthBlocks(): Promise<SchedulerCalendarBlock[]>;
  /** Month-grid overflow controls ("+ N more" / density rollups) in display order. */
  schedulerCalendarMonthOverflow(): Promise<string[]>;
  /** Whether the month grid marks today. */
  schedulerCalendarMonthTodayMarked(): Promise<boolean>;
  /** Week-grid occurrence blocks in display order. */
  schedulerCalendarWeekBlocks(): Promise<SchedulerCalendarBlock[]>;
  /** Whether the week grid's day headers mark today. */
  schedulerCalendarWeekTodayMarked(): Promise<boolean>;
  /** The week grid's current-time line offset in px, or null when absent. */
  schedulerCalendarTimeLineTop(): Promise<number | null>;
  /** Click one calendar block by scheduler name; waits for the occurrence panel. */
  schedulerCalendarClickBlock(name: string): Promise<void>;
  /** Whether any calendar block is draggable (the row says none are). */
  schedulerCalendarAnyDraggable(): Promise<boolean>;
  /** Export/download/print/share controls on the calendar, if any (the row says none). */
  schedulerCalendarExportControls(): Promise<string[]>;
  /** Whether the visibility rail is currently rendered. */
  schedulerRailVisible(): Promise<boolean>;
  /** Visibility-rail rows in display order. */
  schedulerRailRows(): Promise<{ name: string; checked: boolean }[]>;
  /** Toggle one rail row by scheduler name. */
  schedulerRailToggle(name: string): Promise<void>;
  /** Show every scheduler on the rail. */
  schedulerRailShowAll(): Promise<void>;
  /** Hide every scheduler on the rail. */
  schedulerRailHideAll(): Promise<void>;
  /**
   * Toggle `name` off in this tab, open the same calendar in a second tab,
   * and report whether the second tab loads with the choice applied (then
   * closes it). Assumes the calendar is already open in this tab.
   */
  schedulerRailCrossTabPersists(workspaceSlug: string, projectId: string, name: string): Promise<boolean>;
  /**
   * Narrow the viewport below the desktop breakpoint and report whether the
   * rail hides (restores the default viewport before returning).
   */
  schedulerRailNarrowHidden(): Promise<boolean>;
  /** Whether the occurrence panel is currently open. */
  schedulerDrawerOpen(): Promise<boolean>;
  /** Occurrence panel rows as label/value pairs in display order. */
  schedulerDrawerRows(): Promise<{ label: string; value: string }[]>;
  /** Occurrence panel heading: the state line plus the scheduler name. */
  schedulerDrawerHeading(): Promise<{ state: string; name: string }>;
  /** Links offered by the occurrence panel, in display order. */
  schedulerDrawerLinks(): Promise<string[]>;
  /** Close the occurrence panel via its dismiss control. */
  schedulerDrawerClose(): Promise<void>;
  /** Whether the occurrence panel offers edit-binding (future firing + project admin). */
  schedulerDrawerEditVisible(): Promise<boolean>;
  /** Open the edit dialog from the occurrence panel; waits for the form. */
  schedulerDrawerEdit(): Promise<void>;
  /** Follow the occurrence panel's parent-install link; waits for the detail. */
  schedulerDrawerViewScheduler(): Promise<void>;
  /** Open the bare project schedulers section; the app lands on the calendar. */
  schedulerOpenProjectSection(workspaceSlug: string, projectId: string): Promise<void>;
  /** Section tabs with their highlight state, in display order. */
  schedulerSectionTabs(): Promise<{ label: string; active: boolean }[]>;
  /** Follow one section tab; waits for its content to settle. */
  schedulerSectionOpenTab(tab: "List" | "Calendar"): Promise<void>;
  /** Open the project-settings schedulers variant. */
  schedulerOpenSettingsSchedulers(workspaceSlug: string, projectId: string): Promise<void>;
  /** Whether the settings variant shows the installs panel (admins) or the refusal (others). */
  schedulerSettingsPanelVisible(): Promise<boolean>;
  /** Export/download/print/share controls on the run-history table, if any (the row says none). */
  schedulerRunsExportControls(): Promise<string[]>;

  // --- Runner chat on cloud/web (NEWFRONT-181, RUN-025–032). Appended;
  // --- existing methods above are untouched per the shared driver
  // --- contract. Covers the chat page (contacts, history, composer,
  // --- warm-up, streaming, stop, close, header gating) plus the API
  // --- spy and SSE stub the oracle needs without a live runner daemon.

  /** Open a runner's chat page, optionally deep-linking one session. */
  runnerChatOpen(workspaceSlug: string, runnerId: string, sessionId?: string): Promise<void>;
  /** Runner contact names in the side-nav rail, in display order. */
  runnerChatContactNames(): Promise<string[]>;
  /** Class list of a contact's status dot (proves live per-status dots). */
  runnerChatContactDotClass(runnerName: string): Promise<string>;
  /** Open a runner's chat through its side-nav contact. */
  runnerChatOpenContact(runnerName: string): Promise<void>;
  /** Chat header: runner name, secondary line, and status badge text. */
  runnerChatHeader(): Promise<{ name: string; secondary: string; badge: string }>;
  /** Whether the history panel shows its empty state. */
  runnerChatHistoryEmptyVisible(): Promise<boolean>;
  /** History entries in display order with their active flag. */
  runnerChatHistoryItems(): Promise<{ title: string; subtitle: string; active: boolean }[]>;
  /** Click the history entry at `index` (display order). */
  runnerChatClickHistoryItem(index: number): Promise<void>;
  /** Start a fresh session through the New-chat control. */
  runnerChatNewChat(): Promise<void>;
  /** Whether the New-chat control is currently disabled (working state). */
  runnerChatNewChatDisabled(): Promise<boolean>;
  /** Fail the next session-create POST once (new-chat error state). */
  runnerChatFailSessionCreate(): Promise<void>;
  /** Delay the next session-create POST by `ms` (new-chat working state). */
  runnerChatDelaySessionCreate(ms: number): Promise<void>;
  /** Remove the session-create failure/delay stubs. */
  runnerChatClearSessionCreateStubs(): Promise<void>;
  /** Replace the composer draft. */
  runnerChatFillDraft(text: string): Promise<void>;
  /** Current composer draft text. */
  runnerChatDraftValue(): Promise<string>;
  /** Press Enter in the composer (send). */
  runnerChatPressEnter(): Promise<void>;
  /** Press Shift+Enter in the composer (newline, no send). */
  runnerChatPressShiftEnter(): Promise<void>;
  /** Whether the send control is currently enabled. */
  runnerChatSendEnabled(): Promise<boolean>;
  /** Click the send control. */
  runnerChatClickSend(): Promise<void>;
  /** Composer block reason text, or null when chat can proceed. */
  runnerChatComposerReason(): Promise<string | null>;
  /** Whether the composer textarea is disabled. */
  runnerChatTextareaDisabled(): Promise<boolean>;
  /** Inline stream-error banner text, or null when none shows. */
  runnerChatAlertText(): Promise<string | null>;
  /** Dismiss the inline stream-error banner. */
  runnerChatDismissAlert(): Promise<void>;
  /** Most recently shown toast, if any is still visible. */
  runnerChatLastToast(): Promise<{ title: string; message: string } | null>;
  /** Fail the next message-send POST once (send error state). */
  runnerChatFailNextSend(): Promise<void>;
  /** Remove the message-send failure stub. */
  runnerChatClearSendFailure(): Promise<void>;
  /** Voice-dictation control label, or null when the control is absent. */
  runnerChatVoiceButtonLabel(): Promise<string | null>;
  /** Activate the voice-dictation control (push-to-talk press). */
  runnerChatClickVoiceButton(): Promise<void>;
  /** Start counting chat API calls (warm/create/send/cancel/close/lists). */
  runnerChatStartApiSpy(): Promise<void>;
  /** Cumulative chat API call counts since the spy started. */
  runnerChatApiCounts(): Promise<{
    warm: number;
    sessionCreate: number;
    send: number;
    cancel: number;
    close: number;
    sessionList: number;
    messageList: number;
  }>;
  /** Stop counting chat API calls. */
  runnerChatStopApiSpy(): Promise<void>;
  /** Delay the next runner-detail GET by `ms` (loading state). */
  runnerChatDelayRunnerDetail(runnerId: string, ms: number): Promise<void>;
  /** Remove the runner-detail delay stub. */
  runnerChatClearRunnerDetailDelay(): Promise<void>;
  /** Serve canned SSE frames for one session's event stream (re-stubbable). */
  runnerChatStubStream(sessionId: string, frames: RunnerChatStreamFrame[]): Promise<void>;
  /** Remove the SSE stub for one session. */
  runnerChatClearStreamStub(sessionId: string): Promise<void>;
  /** Event-stream request URLs observed since the stub was installed. */
  runnerChatStreamRequestUrls(sessionId: string): Promise<string[]>;
  /** Message bubbles in display order (user + assistant + status rows). */
  runnerChatMessageBubbles(): Promise<{ role: string; text: string }[]>;
  /** Rendered HTML of the assistant bubble at `index` (markdown proof). */
  runnerChatAssistantBubbleHtml(index: number): Promise<string>;
  /** Activity-strip labels above the composer, in display order. */
  runnerChatActivityStrip(): Promise<string[]>;
  /** Whether the stop/interrupt control currently shows. */
  runnerChatStopVisible(): Promise<boolean>;
  /** Click the stop/interrupt control. */
  runnerChatClickStop(): Promise<void>;
  /** Click the header close-session control. */
  runnerChatClickClose(): Promise<void>;
  /** Whether an inline approval prompt shows above the composer. */
  runnerChatApprovalPromptVisible(): Promise<boolean>;
  /** Hold chat message-list GETs by `ms` so stream bubbles stay assertable. */
  runnerChatHoldMessageList(ms: number): Promise<void>;
  /** Release the message-list hold. */
  runnerChatReleaseMessageList(): Promise<void>;

  // --- Prompts + project automations (NEWFRONT-186, AGT-023–037) ---
  /** Open the workspace prompts page; settles on the sections tab, the loader, or a banner. */
  promptsOpen(workspaceSlug: string): Promise<void>;
  /** Which prompts tab is active. */
  promptsActiveTab(): Promise<"Sections" | "Receipt">;
  /** Follow one prompts tab; waits for its content to settle. */
  promptsOpenTab(tab: "Sections" | "Receipt"): Promise<void>;
  /** Section cards in display order: identity, provenance, kinds, edit affordances, effective body. */
  promptsSectionCards(): Promise<PromptSectionCard[]>;
  /** One section card by key, or null when no card shows it. */
  promptsSectionCard(key: string): Promise<PromptSectionCard | null>;
  /** Side-nav entries in display order. */
  promptsSectionNav(): Promise<{ title: string; key: string }[]>;
  /** Follow the side-nav entry for `key`; resolves with the location hash afterwards. */
  promptsSectionNavJump(key: string): Promise<string>;
  /** Whether the prompt-sections loading line is currently shown. */
  promptsLoadingVisible(): Promise<boolean>;
  /** Whether the member-list failure banner is currently shown. */
  promptsSectionsErrorVisible(): Promise<boolean>;
  /** Whether the admin-only baseline warning is currently shown. */
  promptsWorkspaceWarningVisible(): Promise<boolean>;
  /** Fail every prompt-sections list read at `scope` with 500 until stopped (failure-state shaping). */
  promptsFailSectionsStart(scope: "user" | "workspace"): Promise<void>;
  /** Stop failing prompt-sections reads. */
  promptsFailSectionsStop(): Promise<void>;
  /** Delay the next prompt-sections list burst by `ms` (loading-state shaping). */
  promptsDelaySectionsOnce(ms: number): Promise<void>;
  /** Fail the next section-upsert PUT once with 500 (save-failure shaping). */
  promptsFailUpsertOnce(): Promise<void>;
  /** Open the section editor for `key` at `scope`; waits for the editor. */
  promptsOpenSectionEditor(key: string, scope: "workspace" | "user"): Promise<void>;
  /** Current editor state, or null when no editor is open. */
  promptsEditorState(): Promise<PromptEditorState | null>;
  /** Replace the editor draft. */
  promptsEditorFill(text: string): Promise<void>;
  /** Submit the editor; resolves on click (specs poll for the outcome). */
  promptsEditorSave(): Promise<void>;
  /** Cancel the editor; resolves once it closes. */
  promptsEditorCancel(): Promise<void>;
  /** Toggle the compare-with-default pane. */
  promptsEditorToggleCompare(): Promise<void>;
  /** Open the revert confirmation; waits for the dialog. */
  promptsEditorRevertOpen(): Promise<void>;
  /** The open revert dialog, or null when none shows. */
  promptsRevertDialog(): Promise<PromptRevertDialog | null>;
  /** Confirm the open revert dialog (specs poll for the outcome). */
  promptsRevertConfirm(): Promise<void>;
  /** Dismiss the open revert dialog. */
  promptsRevertCancel(): Promise<void>;
  /** Receipt cards in display order with their composed-section lists. */
  promptsReceiptCards(): Promise<PromptReceiptCard[]>;
  /** Side receipt-nav entries in display order. */
  promptsReceiptNav(): Promise<{ kind: string; count: string }[]>;
  /** Follow the receipt-nav entry for `kind`; resolves with the location hash afterwards. */
  promptsReceiptNavJump(kind: string): Promise<string>;
  /** Toggle one receipt card's expanded body. */
  promptsReceiptToggle(kind: string): Promise<void>;
  /** Whether the receipt card for `kind` is expanded. */
  promptsReceiptExpanded(kind: string): Promise<boolean>;
  /** The expanded assembled template for `kind`, or null when collapsed. */
  promptsReceiptTemplate(kind: string): Promise<string | null>;
  /** The automatic-run variant block for `kind`, or null when absent. */
  promptsReceiptAutomatic(kind: string): Promise<string | null>;
  /** Whether the saved-preview form shows under the expanded `kind` receipt (admin-only). */
  promptsSavedPreviewVisible(kind: string): Promise<boolean>;
  /** Whether the saved-preview submit for `kind` is enabled. */
  promptsSavedPreviewSubmitEnabled(kind: string): Promise<boolean>;
  /** Fill the saved-preview target for `kind` and submit. */
  promptsSavedPreviewSubmit(kind: string, target: string): Promise<void>;
  /** Saved-preview outcome for `kind`: rendered prompt, inline error, or neither yet. */
  promptsSavedPreviewResult(kind: string): Promise<{ prompt: string | null; error: string | null }>;
  /** Draft-preview kind switcher labels (empty when the switcher hides for a single kind). */
  promptsDraftPreviewKinds(): Promise<string[]>;
  /** Select one draft-preview kind. */
  promptsDraftPreviewSelectKind(kind: string): Promise<void>;
  /** Whether the draft-preview submit is enabled. */
  promptsDraftPreviewSubmitEnabled(): Promise<boolean>;
  /** Fill the draft-preview target and submit. */
  promptsDraftPreviewSubmit(target: string): Promise<void>;
  /** Draft-preview outcome: rendered prompt, inline error, or neither yet. */
  promptsDraftPreviewResult(): Promise<{ prompt: string | null; error: string | null }>;
  /** Open the project-settings automations page; settles on the rows, the refusal, or the loader. */
  automationsOpen(workspaceSlug: string, projectId: string): Promise<void>;
  /** Whether the not-authorized view is currently shown. */
  automationsNotAuthorizedVisible(): Promise<boolean>;
  /** Auto-archive row state. */
  automationsArchiveRow(): Promise<AutomationRow>;
  /** Flip the auto-archive toggle (specs poll for the outcome). */
  automationsArchiveToggle(): Promise<void>;
  /** Choose a preset month delay on the auto-archive picker. */
  automationsArchiveSetPreset(months: number): Promise<void>;
  /** Open the custom-month dialog from the auto-archive picker. */
  automationsArchiveOpenCustom(): Promise<void>;
  /** Auto-close row state. */
  automationsCloseRow(): Promise<AutomationCloseRow>;
  /** Flip the auto-close toggle (specs poll for the outcome). */
  automationsCloseToggle(): Promise<void>;
  /** Choose a preset month delay on the auto-close picker. */
  automationsCloseSetPreset(months: number): Promise<void>;
  /** Choose a close-state target by name (enabled only with several cancelled states). */
  automationsCloseSetState(name: string): Promise<void>;
  /** Close-state option names in display order; ends with the picker closed. */
  automationsCloseStateOptions(): Promise<string[]>;
  /** Open the custom-month dialog from the auto-close picker. */
  automationsCloseOpenCustom(): Promise<void>;
  /** The open custom-month dialog, or null when none shows. */
  automationsMonthModal(): Promise<AutomationMonthModal | null>;
  /** Fill the custom-month input. */
  automationsMonthFill(value: string): Promise<void>;
  /** Submit the custom-month dialog (specs poll for the outcome). */
  automationsMonthSubmit(): Promise<void>;
  /** Cancel the custom-month dialog; resolves once it closes. */
  automationsMonthCancel(): Promise<void>;
  /** Fail the next project PATCH once (failure-notice shaping). */
  automationsFailUpdateOnce(): Promise<void>;
  /** Built-in automation row titles in display order. */
  automationsBuiltInRows(): Promise<string[]>;
  /** Whether any extension contributed rows below the built-ins. */
  automationsHasExtensionRows(): Promise<boolean>;

  // --- Add-runner modal + creation (NEWFRONT-179, RUN-006–009) ---
  /** Open the runners page (workspace scope, or project scope when `projectId` is set) and open the add-runner modal; resolves once the form shows. */
  addRunnerOpenFromRunners(workspaceSlug: string, projectId?: string): Promise<void>;
  /** Open the AI-dev-machines page and open the add-runner modal; resolves once the form shows. */
  addRunnerOpenFromMachines(workspaceSlug: string): Promise<void>;
  /** Whether the add-runner modal is currently open. */
  addRunnerVisible(): Promise<boolean>;
  /** Which modal layout shows: the entry form, the remote-create panel, or the command panel. */
  addRunnerLayout(): Promise<"form" | "remote" | "command">;
  /** Current form values as the controls render them. */
  addRunnerForm(): Promise<AddRunnerFormState>;
  /** Machine picker option labels in display order. */
  addRunnerMachineOptions(): Promise<string[]>;
  /** Project picker option labels in display order. */
  addRunnerProjectOptions(): Promise<string[]>;
  /** Pod picker option labels in display order. */
  addRunnerPodOptions(): Promise<string[]>;
  /** Agent picker option labels in display order. */
  addRunnerAgentOptions(): Promise<string[]>;
  /** Model picker option labels in display order. */
  addRunnerModelOptions(): Promise<string[]>;
  /** Project-field validation message, or null when none shows. */
  addRunnerProjectError(): Promise<string | null>;
  /** Name-field validation message, or null when none shows. */
  addRunnerNameError(): Promise<string | null>;
  /** Pick the machine option labelled `label`. */
  addRunnerPickMachine(label: string): Promise<void>;
  /** Pick the manual-command sentinel in the machine picker. */
  addRunnerPickManual(): Promise<void>;
  /** Pick the project option labelled `name`. */
  addRunnerPickProject(name: string): Promise<void>;
  /** Pick the pod option for `name` (`""` for the default-pod sentinel). */
  addRunnerPickPod(name: string): Promise<void>;
  /** Fill the runner name field. */
  addRunnerSetName(name: string): Promise<void>;
  /** Fill the working-directory field. */
  addRunnerSetWorkingDir(dir: string): Promise<void>;
  /** Pick the agent option labelled `label`. */
  addRunnerPickAgent(label: string): Promise<void>;
  /** Pick the model option labelled `label`. */
  addRunnerPickModel(label: string): Promise<void>;
  /** Submit the form (specs poll for the outcome: the form stays on validation errors). */
  addRunnerSubmit(): Promise<void>;
  /** Dismiss the modal via its Cancel/Close/Done control. */
  addRunnerClose(): Promise<void>;
  /** Remote-create panel phase, or null when the panel is not showing. */
  addRunnerRemotePhase(): Promise<AddRunnerRemotePhase | null>;
  /** Remote-create panel body text, or null when the panel is not showing. */
  addRunnerRemoteText(): Promise<string | null>;
  /** Runner name the success panel reports, or null when absent. */
  addRunnerRemoteRunnerName(): Promise<string | null>;
  /** Activate the remote panel's Back control (returns to the form). */
  addRunnerRemoteBack(): Promise<void>;
  /** Activate the remote panel's manual-command control (carries values over). */
  addRunnerRemoteManual(): Promise<void>;
  /** Record create-runner POST bodies until stopped. */
  addRunnerCreateSpyStart(): Promise<void>;
  addRunnerCreateSpyBodies(): Promise<string[]>;
  addRunnerCreateSpyStop(): Promise<void>;
  /** Record create-runner status GETs until stopped. */
  addRunnerStatusSpyStart(): Promise<void>;
  addRunnerStatusSpyUrls(): Promise<string[]>;
  addRunnerStatusSpyStop(): Promise<void>;
  /** Generated command text, or null when the command panel is not showing. */
  addRunnerCommandText(): Promise<string | null>;
  /** Command panel header line (project echo), or null when absent. */
  addRunnerCommandHeader(): Promise<string | null>;
  /** Shell tab labels in display order. */
  addRunnerShellOptions(): Promise<string[]>;
  /** Currently pressed shell tab label, or null when absent. */
  addRunnerActiveShell(): Promise<string | null>;
  /** Activate the shell tab labelled `label`. */
  addRunnerPickShell(label: string): Promise<void>;
  /** Activate the copy control (grants clipboard first). */
  addRunnerCopy(): Promise<void>;
  /** The copy control's current label (transient confirm). */
  addRunnerCopyState(): Promise<string | null>;
  /** Current clipboard text (grants clipboard permission first). */
  addRunnerReadClipboard(): Promise<string>;
  /** Make the next clipboard write reject (copy-failure shaping). */
  addRunnerBreakClipboard(): Promise<void>;
  /** Origin-fallback note, or null when absent. */
  addRunnerOriginNote(): Promise<string | null>;
  /** Activate the command panel's Back control (returns to the form). */
  addRunnerCommandBack(): Promise<void>;
  /** Newest toast text, or null when no toast shows. */
  addRunnerLastToast(): Promise<string | null>;

  // -- Assistant chat core (NEWFRONT-187, AGT-038-049, AGT-053-057) --------

  /** Open the assistant landing for a workspace and settle. */
  assistantOpenLanding(workspaceSlug: string): Promise<void>;
  /** Open one thread view and settle. */
  assistantOpenThread(workspaceSlug: string, threadId: string): Promise<void>;
  /** Open the workspace dashboard and settle. */
  assistantOpenHome(workspaceSlug: string): Promise<void>;
  /** Current pathname (navigation proofs). */
  assistantCurrentPath(): Promise<string>;
  /** Browser back plus settle (handoff-state proofs). */
  assistantGoBack(): Promise<void>;
  /** Landing greeting headline plus caption; null when the setup card shows. */
  assistantLandingGreeting(): Promise<AssistantLandingGreeting | null>;
  /** Whether the landing composer is present. */
  assistantLandingComposerVisible(): Promise<boolean>;
  /** Setup card title, body and button label; null when chat entry shows. */
  assistantSetupCard(): Promise<{ title: string; body: string; button: string } | null>;
  /** Follow the setup card's provider-settings button. */
  assistantSetupCardClick(): Promise<void>;
  /** Type into the assistant composer (landing or thread). */
  assistantFillDraft(text: string): Promise<void>;
  /** Current composer draft. */
  assistantDraftValue(): Promise<string>;
  /** Press Enter in the composer. */
  assistantPressEnter(): Promise<void>;
  /** Press Shift+Enter in the composer. */
  assistantPressShiftEnter(): Promise<void>;
  /** Press Control+Enter in the composer (modified-Enter halves). */
  assistantPressControlEnter(): Promise<void>;
  /** Whether the send button is present (absent while a turn runs). */
  assistantSendVisible(): Promise<boolean>;
  /** Whether the send button is enabled. */
  assistantSendEnabled(): Promise<boolean>;
  /** Click the send button. */
  assistantClickSend(): Promise<void>;
  /** Whether the stop button is present (a turn is running). */
  assistantStopVisible(): Promise<boolean>;
  /** Click the stop button. */
  assistantClickStop(): Promise<void>;
  /** Composer lockdown line; null when the composer is not locked down. */
  assistantComposerReason(): Promise<string | null>;
  /** Whether the composer textarea is disabled (lockdown or posting). */
  assistantTextareaDisabled(): Promise<boolean>;
  /** Inline send/turn error line; null when none shows. */
  assistantErrorLine(): Promise<string | null>;
  /** Dictation button label; null when the control hides. */
  assistantMicLabel(): Promise<string | null>;
  /** Press the dictation control (push-to-talk down; c5 owns the hold). */
  assistantClickMic(): Promise<void>;
  /** Dictation status hint above the composer; null when idle. */
  assistantDictationHint(): Promise<string | null>;
  /** Transcript bubbles top to bottom: user, assistant, tool, error, notice. */
  assistantBubbles(): Promise<AssistantBubble[]>;
  /** Rendered HTML of the transcript bubble at `index` (markdown proof). */
  assistantBubbleHtml(index: number): Promise<string>;
  /** Tool-activity rows with their deep links. */
  assistantToolActivities(): Promise<AssistantToolActivity[]>;
  /** Skipped-server notice lines in display order. */
  assistantNoticeLines(): Promise<string[]>;
  /** Empty-transcript placeholder text; null once rows render. */
  assistantEmptyState(): Promise<string | null>;
  /** Whether the transcript follows the tail (newest row fully visible). */
  assistantIsScrolledToBottom(): Promise<boolean>;
  /** Follow a tool-activity deep link. */
  assistantClickToolLink(activityIndex: number, linkIndex: number): Promise<void>;
  /** Sidebar threads top to bottom with hrefs and the active mark. */
  assistantSidebarThreads(): Promise<AssistantSidebarThread[]>;
  /** Whether the sidebar shows the no-conversations placeholder. */
  assistantSidebarEmptyVisible(): Promise<boolean>;
  /** Click the sidebar's New chat entry. */
  assistantClickNewChat(): Promise<void>;
  /** Click a sidebar thread by index. */
  assistantClickSidebarThread(index: number): Promise<void>;
  /** Whether the dashboard assistant card renders. */
  assistantCardVisible(): Promise<boolean>;
  /** Type into the dashboard card input. */
  assistantCardFillDraft(text: string): Promise<void>;
  /** Current dashboard card draft. */
  assistantCardDraftValue(): Promise<string>;
  /** Press Enter in the dashboard card input. */
  assistantCardPressEnter(): Promise<void>;
  /** Whether the card's Ask button is disabled. */
  assistantCardAskDisabled(): Promise<boolean>;
  /** Click the card's Ask button. */
  assistantCardClickAsk(): Promise<void>;
  /** Click a suggestion chip by its text (fills the draft). */
  assistantCardClickSuggestion(text: string): Promise<void>;
  /** Suggestion chip texts in display order (empty when the card hides). */
  assistantCardSuggestions(): Promise<string[]>;
  /** Card recents top to bottom with deep-link hrefs. */
  assistantCardRecents(): Promise<{ title: string; href: string }[]>;
  /** Follow a card recent by index. */
  assistantCardClickRecent(index: number): Promise<void>;
  /** Serve canned SSE frames for a thread's event stream (transport proofs). */
  assistantStubStream(threadId: string, frames: AssistantStreamFrame[]): Promise<void>;
  /** Remove a thread's SSE stub. */
  assistantClearStreamStub(threadId: string): Promise<void>;
  /** Event-stream request URLs seen per thread (replay cursor proofs). */
  assistantStreamRequestUrls(threadId: string): Promise<string[]>;
  /** Abort a thread's event stream (poll-path proofs). */
  assistantBlockStream(threadId: string): Promise<void>;
  /** Remove a thread's stream block. */
  assistantClearStreamBlock(threadId: string): Promise<void>;
  /** Count assistant API calls from here on. */
  assistantStartApiSpy(): Promise<void>;
  /** Calls seen since the spy started. */
  assistantApiCounts(): Promise<AssistantApiCounts>;
  /** Remove the API spy. */
  assistantStopApiSpy(): Promise<void>;
  /** Fail the next thread-create POST once with a 500. */
  assistantFailThreadCreateOnce(): Promise<void>;
  /** Delay the next thread-create POST by `ms`. */
  assistantDelayThreadCreate(ms: number): Promise<void>;
  /** Remove thread-create stubs. */
  assistantClearThreadCreateStubs(): Promise<void>;
  /** Delay the next message-send POST by `ms`. */
  assistantDelaySend(ms: number): Promise<void>;
  /** Remove the send delay. */
  assistantClearSendDelay(): Promise<void>;
  /** Newest visible toast; null when none shows. */
  assistantLastToast(): Promise<{ title: string; message: string } | null>;

  // -- Assistant voice/keys/tools/negatives/editor (NEWFRONT-188, AGT-050-052, AGT-058-061, AGT-063) --
  /** URL hash on the current page (dictation-settings anchor proofs). */
  assistantCurrentHash(): Promise<string>;
  /** Whether the composer mic button is disabled. */
  assistantMicDisabled(): Promise<boolean>;
  /** Press-and-hold the mic: pointer down, held for `ms`, pointer up. */
  assistantMicHold(ms: number): Promise<void>;
  /** Push-to-talk down without release (mid-hold assertions). */
  assistantMicDown(): Promise<void>;
  /** Push-to-talk up after a down. */
  assistantMicUp(): Promise<void>;
  /** Hold `ms` more, then push-to-talk up (clears the tap floor). */
  assistantMicUpAfter(ms: number): Promise<void>;
  /** Grant or deny microphone capture for the page. */
  assistantSetMicrophonePermission(state: "granted" | "denied"): Promise<void>;
  /** Hide capture APIs from the page (unsupported-browser proofs). */
  assistantSimulateUnsupportedCapture(): Promise<void>;
  /** Reject mic capture as denied (denied-mapping proofs). */
  assistantSimulateMicDenial(): Promise<void>;
  /** Serve a canned transcription for transcribe POSTs (transport proofs). */
  assistantStubTranscribeText(text: string): Promise<void>;
  /** Fail transcribe POSTs with `status` + body (error-path proofs). */
  assistantFailTranscribe(status: number, body: Record<string, string>): Promise<void>;
  /** Remove transcribe stubs. */
  assistantClearTranscribeStubs(): Promise<void>;
  /** Transcribe uploads seen (multipart audio posts). */
  assistantTranscribeRequests(): Promise<{ contentType: string; hasFilePart: boolean; byteLength: number }[]>;
  /** Visible texts of buttons inside the assistant sidebar (negative proofs). */
  assistantSidebarButtons(): Promise<string[]>;
  /** Rename/archive/delete-ish controls in the assistant layout (negative proofs). */
  assistantThreadManagementControls(): Promise<string[]>;
  /** Header text of the assistant sidebar. */
  assistantSidebarHeader(): Promise<string | null>;
  /** Row element kinds of the sidebar New-chat entry + thread rows. */
  assistantSidebarRowKinds(): Promise<{ newChat: string | null; rows: string[] }>;
  /** Links and buttons inside skipped-server notices (negative proofs). */
  assistantSkippedNoticeActions(): Promise<{ kind: string; text: string; href: string | null }[]>;
  /** Settings-bound link targets inside the assistant layout (registry-chrome proofs). */
  assistantChatSettingsLinks(): Promise<string[]>;
  /** Count desktop-gated endpoint calls from here on. */
  assistantStartDesktopCallWatch(): Promise<void>;
  /** Desktop-gated calls seen since the watch started. */
  assistantDesktopCallsObserved(): Promise<{ method: string; url: string }[]>;
  /** Remove the desktop-call watch. */
  assistantStopDesktopCallWatch(): Promise<void>;
  /** Report the instance as LLM-configured or not (editor-AI gate proofs). */
  assistantStubInstanceLlm(configured: boolean): Promise<void>;
  /** Remove the instance stub. */
  assistantClearInstanceStub(): Promise<void>;
  /** Serve a canned GPT-editor answer (transport proofs). */
  assistantStubGptAnswer(response: { response: string; response_html: string }): Promise<void>;
  /** Fail GPT-editor POSTs with `status` + body (error-path proofs). */
  assistantFailGptAnswer(status: number, body: Record<string, string>): Promise<void>;
  /** Remove GPT-editor stubs. */
  assistantClearGptStubs(): Promise<void>;
  /** GPT-editor request bodies seen. */
  assistantGptRequests(): Promise<{ prompt: string; task: string }[]>;
  /** Whether the issue-modal description editor offers the AI helper entry. */
  issueModalAiEntryVisible(): Promise<boolean>;
  /** Open the AI helper popover from the description editor. */
  issueModalAiOpen(): Promise<void>;
  /** Fill the helper's request box. */
  issueModalAiFillTask(text: string): Promise<void>;
  /** Submit the helper request. */
  issueModalAiGenerate(): Promise<void>;
  /** Review text the helper shows, or null while none shows. */
  issueModalAiResponse(): Promise<string | null>;
  /** Whether the helper marks its answer invalid. */
  issueModalAiInvalidVisible(): Promise<boolean>;
  /** Insert the reviewed answer into the description. */
  issueModalAiUseResponse(): Promise<void>;
  /** Close the helper popover. */
  issueModalAiClose(): Promise<void>;
  /** Plain text currently in the modal description editor. */
  issueModalDescriptionText(): Promise<string | null>;
  /** Open a project page in the document editor. */
  pageEditorOpen(workspaceSlug: string, projectId: string, pageId: string): Promise<void>;
  /** AI handles revealed after hovering the page blocks (absence proofs). */
  pageEditorAiHandleCount(): Promise<number>;
  /** Whether the AI popup shows. */
  pageEditorAiMenuVisible(): Promise<boolean>;
  /** Rephrase/grammar request URLs seen (missing-backend proofs). */
  pageEditorRephraseRequests(): Promise<string[]>;

  // ---------------------------------------------------------------------------
  // Notifications inbox foundation (NEWFRONT-198, NTF-001..006). Observed on
  // apps/web: the two-pane inbox shell, the stream tabs with their badges,
  // the navigation badge, the newest-first card list, and the unread marks.
  // Appended; existing entries above are untouched per the shared contract.
  // ---------------------------------------------------------------------------

  /** Open the workspace inbox and wait until its tab strip has mounted. */
  notificationsOpenInbox(workspaceSlug: string): Promise<void>;
  /** Whether the inbox list pane currently renders. */
  notificationsListPaneVisible(): Promise<boolean>;
  /** Whether the inbox detail pane currently renders. */
  notificationsDetailPaneVisible(): Promise<boolean>;
  /** Rendered widths of the list and detail panes in CSS pixels. */
  notificationsPaneWidths(): Promise<{ list: number; detail: number }>;
  /** Select a card by index and wait until it becomes selected. */
  notificationsSelectCard(index: number): Promise<void>;
  /** Visible stream-tab labels in display order. */
  notificationsTabNames(): Promise<string[]>;
  /** Key of the currently active stream tab. */
  notificationsActiveTab(): Promise<NotificationsTab>;
  /** Activate a stream tab and wait until it becomes active. */
  notificationsSelectTab(tab: NotificationsTab): Promise<void>;
  /** Badge text on a stream tab, or null when the tab carries no badge. */
  notificationsTabBadge(tab: NotificationsTab): Promise<string | null>;
  /** Badge text on the sidebar notifications entry, or null when hidden. */
  notificationsNavBadge(): Promise<string | null>;
  /**
   * Enter a project's issues page as the owner of `cookies` and read the
   * sidebar notifications badge once its unread fetch lands (or null when
   * the entry carries no badge). The fetch wait starts before navigation,
   * so zero-state reads are post-load rather than mid-flight.
   */
  notificationsProjectNavBadge(
    workspaceSlug: string,
    projectId: string,
    cookies: ParityBrowserCookie[]
  ): Promise<string | null>;
  /** Inbox cards in display order, each as the user reads it. */
  notificationsCards(): Promise<NotificationsCard[]>;
  /** Computed background of each card in display order (tint comparison). */
  notificationsCardBackgrounds(): Promise<string[]>;
  /**
   * Reload the inbox and report whether the list fetch and the unread-count
   * fetch each fired during entry.
   */
  notificationsEntryFetches(workspaceSlug: string): Promise<{ list: boolean; unread: boolean }>;

  // --- Desktop agent-runtime web-observable sides (NEWFRONT-207, DESK-001–010,
  // --- DESK-026). Appended; existing methods above are untouched per the
  // --- shared driver contract. Targeted absence probes the desktopRuntime*
  // --- verbs (NEWFRONT-182) do not cover: rendered page text, window-focus
  // --- refresh traffic, and client-side database residue.
  /** Rendered text of the current page (absence scans for desktop-only copy). */
  deskRuntimePageText(): Promise<string>;
  /** Dispatch a window focus event, as if the user returned to the app. */
  deskRuntimeDispatchWindowFocus(): Promise<void>;
  /** Client-side database names the page currently holds (residue checks). */
  deskRuntimeIndexedDatabaseNames(): Promise<string[]>;

  // ---------------------------------------------------------------------------
  // Notifications snooze + email preferences (NEWFRONT-201, NTF-020..022,
  // NTF-024..025). Observed on apps/web: the card snooze picker with preset
  // delays and a remove entry, the custom resume dialog with half-hour time
  // slots, the resume label on snoozed cards, and the profile-settings email
  // preference page with instant-save toggles. Appended; existing entries
  // above are untouched per the shared contract.
  // ---------------------------------------------------------------------------

  /** Preset labels the snooze picker offers for a card (opens and closes it). */
  notificationsSnoozePresets(index: number): Promise<string[]>;
  /**
   * Snooze a card via a preset label; resolves once the per-item PATCH
   * settles so the stream membership read after it is post-write.
   */
  notificationsSnoozeWithPreset(index: number, preset: string): Promise<void>;
  /** Whether the snooze picker for a card offers the remove-snooze entry. */
  notificationsSnoozeRemovalOffered(index: number): Promise<boolean>;
  /**
   * Remove a card's snooze via the picker; resolves once the per-item
   * PATCH settles.
   */
  notificationsUnsnooze(index: number): Promise<void>;
  /** Fail per-item PATCH writes with `status` from here on (snooze failure path). */
  notificationsFailItemWrites(status: number): Promise<void>;
  /** Remove the per-item write failure. */
  notificationsClearItemWriteFailure(): Promise<void>;
  /** Open the custom-resume dialog for a card. */
  notificationsOpenCustomSnooze(index: number): Promise<void>;
  /** Whether the custom-resume dialog currently shows. */
  notificationsCustomSnoozeVisible(): Promise<boolean>;
  /** Pick a resume day `offsetDays` out from today in the custom dialog. */
  notificationsCustomSnoozePickDay(offsetDays: number): Promise<void>;
  /** Time-slot labels the custom dialog offers for a period (AM/PM). */
  notificationsCustomSnoozeTimeSlots(period: "AM" | "PM"): Promise<string[]>;
  /** Choose a period (AM/PM) then a time slot in the custom dialog. */
  notificationsCustomSnoozePickTime(period: "AM" | "PM", slot: string): Promise<void>;
  /** Submit the custom dialog (caller asserts close vs stay-open). */
  notificationsCustomSnoozeSubmit(): Promise<void>;
  /** Enable/disable the snoozed-only stream; waits until the list reloads. */
  notificationsSetSnoozedMode(on: boolean): Promise<void>;
  /** Open the profile-settings email-preferences page; waits until toggles mount. */
  notificationsOpenEmailPreferences(): Promise<void>;
  /** Whether the email-preferences loader showed during a delayed entry. */
  notificationsEmailPreferencesLoaderShown(): Promise<boolean>;
  /** Current toggle states keyed by preference key. */
  notificationsEmailPreferences(): Promise<Record<NotificationsEmailPref, boolean>>;
  /** Flip one email-preference toggle; waits until its save request settles. */
  notificationsEmailPreferencesToggle(pref: NotificationsEmailPref): Promise<void>;
  /** Whether the completed-only toggle nests under the state toggle. */
  notificationsEmailPreferencesCompletedNested(): Promise<boolean>;
  /** Fail email-preference saves with `status` from here on. */
  notificationsFailEmailPreferenceSaves(status: number): Promise<void>;
  /** Remove the email-preference save failure. */
  notificationsClearEmailPreferenceSaveFailure(): Promise<void>;

  // ---------------------------------------------------------------------------
  // Notifications filters, modes, read/archive (NEWFRONT-200, NTF-015..019,
  // NTF-023). Observed on apps/web: the origin filter menu with its applied
  // chips, the overflow menu's unread/archived/snoozed modes, and the
  // hover-revealed per-card read/archive actions with their toasts.
  // Appended; existing entries above are untouched per the shared contract.
  // ---------------------------------------------------------------------------

  /** Reload the inbox and report the entry list fetch's query params. */
  notificationsEntryListQuery(workspaceSlug: string): Promise<NotificationsListQuery>;
  /** Open the origin filter menu and wait until its options show. */
  notificationsOpenFilterMenu(): Promise<void>;
  /** Origin-filter options in display order with their checked state. */
  notificationsFilterOptions(): Promise<NotificationsFilterOption[]>;
  /** Toggle one origin filter; resolves with the refetch's query params. */
  notificationsToggleFilterOrigin(origin: NotificationsOrigin): Promise<NotificationsListQuery>;
  /** Applied-filter chips in display order (origin key plus label). */
  notificationsAppliedChips(): Promise<NotificationsAppliedChip[]>;
  /** Remove one origin via its chip; resolves with the refetch's query params. */
  notificationsRemoveFilterChip(origin: NotificationsOrigin): Promise<NotificationsListQuery>;
  /** Clear all origins; resolves with the refetch's query params. */
  notificationsClearFilters(): Promise<NotificationsListQuery>;
  /** Dismiss any open inbox menus. */
  notificationsCloseMenus(): Promise<void>;
  /** Open the overflow menu and wait until its mode options show. */
  notificationsOpenOverflowMenu(): Promise<void>;
  /** Overflow-mode option labels in display order. */
  notificationsOverflowOptions(): Promise<string[]>;
  /** Toggle one overflow mode; resolves with the refetch's query params. */
  notificationsToggleMode(mode: NotificationsMode): Promise<NotificationsListQuery>;
  /** Whether the card's hover actions currently show. */
  notificationsCardActionsVisible(index: number): Promise<boolean>;
  /** Hover a card and wait until its actions show. */
  notificationsHoverCard(index: number): Promise<void>;
  /** Toggle one card's read state through its hover action. */
  notificationsToggleCardRead(index: number): Promise<void>;
  /** Toggle one card's archived state through its hover action. */
  notificationsToggleCardArchive(index: number): Promise<void>;
  /** Fail the next per-card read/archive write once (failure shaping). */
  notificationsFailNextCardWrite(): Promise<void>;
  // --- Archived cycles (NEWFRONT-224, ARCH-014–019). Appended; existing
  // --- methods above are untouched per the shared driver contract. Covers
  // --- the archives cycles tab (plain list, address-driven peek, search,
  // --- filters, empty states) plus the live-cycle archive dialog and the
  // --- archived-row restore entry. Reads are user-visible; the suite never
  // --- asserts store internals.
  /**
   * Open the tab through the live screen: fresh archived loads skeleton
   * forever (bug NEWFRONT-231), so this visits live cycles first — which
   * sets the cycle fetched flag — then navigates client-side via the
   * sidebar project menu and the Cycles tab. Ends with the list settled.
   */
  archivesCyclesOpenTabViaLive(workspaceSlug: string, projectId: string): Promise<void>;
  /** Open the tab without settling (fresh loads never settle: NEWFRONT-231). */
  archivesCyclesOpenTabRaw(workspaceSlug: string, projectId: string): Promise<void>;
  /** Whether the list skeleton loader currently shows. */
  archivesCyclesSkeletonVisible(): Promise<boolean>;
  /** Open the project's live cycles screen; ends with the list settled. */
  archivesCyclesOpenLive(workspaceSlug: string, projectId: string): Promise<void>;
  /** Reload the current page without settling (fresh archived loads never settle). */
  archivesCyclesReload(): Promise<void>;
  /** Cycle names currently rendered as rows, in display order. */
  archivesCyclesVisibleNames(): Promise<string[]>;
  /** Expand one live-screen group disclosure (Upcoming/Completed); waits for it to open. */
  archivesCyclesLiveGroupOpen(section: string): Promise<void>;
  /** List group headings currently rendered (empty on the archived tab). */
  archivesCyclesGroupHeadings(): Promise<string[]>;
  /** Open the side peek for the named row; waits for the peek to show it. */
  archivesCyclesOpenPeek(name: string): Promise<void>;
  /** Open the tab at a shared peek link without settling (fresh loads never settle). */
  archivesCyclesOpenPeekLink(workspaceSlug: string, projectId: string, cycleId: string): Promise<void>;
  /** Peek panel heading, or null when no peek is open. */
  archivesCyclesPeekName(): Promise<string | null>;
  /** The peekCycle address param, or null when absent. */
  archivesCyclesPeekParam(): Promise<string | null>;
  /** Close the open peek via its dismiss control; waits for it to clear. */
  archivesCyclesClosePeek(): Promise<void>;
  /** Whether the header search box is currently expanded. */
  archivesCyclesSearchExpanded(): Promise<boolean>;
  /** Expand the search box via the magnifier; waits for focus. */
  archivesCyclesSearchOpen(): Promise<void>;
  /** Whether the search input currently holds focus. */
  archivesCyclesSearchFocused(): Promise<boolean>;
  /** Replace the search box text (the list filters live). */
  archivesCyclesSearchFill(text: string): Promise<void>;
  /** Current search box text. */
  archivesCyclesSearchText(): Promise<string>;
  /** Press Escape while the search box is focused. */
  archivesCyclesSearchEscape(): Promise<void>;
  /** Click the search box clear control; waits for it to collapse. */
  archivesCyclesSearchClear(): Promise<void>;
  /** Click a neutral header area (outside-click behavior). */
  archivesCyclesClickAway(): Promise<void>;
  /** Open the filters menu; waits for its panel. */
  archivesCyclesFiltersOpen(): Promise<void>;
  /** Dismiss the filters menu. */
  archivesCyclesFiltersClose(): Promise<void>;
  /** Filter dimension headings in the open panel, in display order. */
  archivesCyclesFilterSections(): Promise<string[]>;
  /** Filter option labels in the open panel, in display order. */
  archivesCyclesFilterOptionNames(): Promise<string[]>;
  /** Pick one filter option within its dimension; waits for the chip row. */
  archivesCyclesFilterPick(section: string, optionName: string): Promise<void>;
  /** Applied-filter chip texts in display order (empty when no row shows). */
  archivesCyclesFilterChipTexts(): Promise<string[]>;
  /** Remove the chip carrying `chipText`; waits for the row to update. */
  archivesCyclesFilterRemoveChip(chipText: string): Promise<void>;
  /** Click the clear-all control; waits for the chip row to clear. */
  archivesCyclesFiltersClearAll(): Promise<void>;
  /** Whether the filters menu button carries its active-filters marker (mounted while any filter applies). */
  archivesCyclesFiltersActive(): Promise<boolean>;
  /** Zero-archived empty-state heading, or null when not shown. */
  archivesCyclesEmptyHeading(): Promise<string | null>;
  /** No-match hint paragraph, or null when the list is not in no-match. */
  archivesCyclesNoMatchHint(): Promise<string | null>;
  /**
   * Navigate via the live screen with the archived list fetch held back,
   * reporting whether the skeleton loader showed while waiting. Ends
   * settled (fresh loads never settle: NEWFRONT-231).
   */
  archivesCyclesSkeletonShownOnSlowFetchViaLive(workspaceSlug: string, projectId: string): Promise<boolean>;
  /** Open the named row's quick-action menu; waits for its entries. */
  archivesCyclesOpenRowMenu(name: string): Promise<void>;
  /** Entries of the open row menu with their disabled state and hint. */
  archivesCyclesMenuEntries(): Promise<{ title: string; disabled: boolean; description: string | null }[]>;
  /** Pick one entry of the open row menu by title. */
  archivesCyclesMenuPick(title: string): Promise<void>;
  /** Open archive dialog heading plus body, or null when no dialog shows. */
  archivesCyclesArchiveDialogText(): Promise<{ heading: string; body: string } | null>;
  /** Confirm the archive dialog; waits for the archive request to settle. */
  archivesCyclesArchiveDialogConfirm(): Promise<void>;
  /** Cancel the archive dialog; waits for it to close. */
  archivesCyclesArchiveDialogCancel(): Promise<void>;

  // ---------------------------------------------------------------------------
  // Notifications detail, pagination, refresh, mark-all-read (NEWFRONT-199,
  // NTF-007..014). Observed on apps/web: selecting a card opens its detail
  // and marks it read on first open; triage items embed the triage view and
  // ordinary items the peek overview; the placeholder, the next-page
  // control, skeletons, per-tab empty states, the refresh control and the
  // mark-all-read control. Appended; existing entries above are untouched
  // per the shared contract.
  // ---------------------------------------------------------------------------

  /** What the detail pane currently shows (selection state). */
  notificationsDetailVariant(): Promise<NotificationsDetailVariant>;
  /** Visible text of the detail pane, whitespace-collapsed. */
  notificationsDetailText(): Promise<string>;
  /** Close the detail preview and wait until the selection clears. */
  notificationsCloseDetail(): Promise<void>;
  /**
   * Select a card by index and report whether the selection posted a
   * mark-read write. Settles on the detail pane like notificationsSelectCard
   * when the selection changes, and on a short grace window when it does
   * not (reselecting the current card).
   */
  notificationsSelectCardPostedRead(index: number): Promise<boolean>;
  /**
   * Select a card whose detail waits on the project-access lookup while
   * that lookup is held back `holdMs`, reporting whether the loading
   * indicator showed. Releases the hold and waits for the triage view.
   */
  notificationsSelectCardHeldAccess(index: number, holdMs: number): Promise<{ spinnerShown: boolean }>;
  /** Label of the next-page control, or null when no further page exists. */
  notificationsNextPageLabel(): Promise<string | null>;
  /** Activate the next-page control and wait until older cards append. */
  notificationsLoadNextPage(): Promise<void>;
  /**
   * Activate the next-page control while the list fetch is held back
   * `holdMs`, reporting whether the loading label showed and how the card
   * count moved. Releases the hold and waits for the appended cards.
   */
  notificationsLoadNextPageHeld(holdMs: number): Promise<{ loadingShown: boolean; before: number; after: number }>;
  /**
   * Enter the inbox while the initial list fetch is held back `holdMs`,
   * reporting whether skeleton rows showed mid-flight and how many cards
   * settled once the fetch landed.
   */
  notificationsSkeletonOnDelayedEntry(
    workspaceSlug: string,
    holdMs: number
  ): Promise<{ skeletonShown: boolean; settledCards: number }>;
  /** Visible text of the empty state, or null when the list is non-empty. */
  notificationsEmptyText(): Promise<string | null>;
  /** Refresh the current stream and wait until the list fetch lands. */
  notificationsRefresh(): Promise<void>;
  /**
   * Press refresh twice while the list fetch is held back `holdMs`,
   * reporting whether the control showed progress and which list-fetch
   * URLs fired. Releases the hold and waits for the list to settle.
   */
  notificationsRefreshHeld(holdMs: number): Promise<{ spinning: boolean; requests: string[] }>;
  /** Mark the current scope read and wait until the request completes. */
  notificationsMarkAllRead(): Promise<void>;
  /**
   * Press mark-all-read twice while its request is held back `holdMs`,
   * reporting whether the control showed progress, how many requests
   * fired, and the first request's raw body. Releases the hold and waits
   * for the cards to settle read.
   */
  notificationsMarkAllReadHeld(
    holdMs: number
  ): Promise<{ progress: boolean; requests: number; scopeBody: string | null }>;

  // ---------------------------------------------------------------------------
  // Archived modules (NEWFRONT-225, ARCH-020..025). Observed on apps/web: the
  // archives modules tab (rows plus address-driven peek, sort control,
  // expandable search, filters menu with chip row), the live-module archive
  // dialog, the archived-row restore menu, and the read-only archived peek
  // panels for modules and cycles. Row menus are read through the
  // right-click context menu, which carries the same entries as the
  // hover-revealed ellipsis. Appended; existing entries above are untouched
  // per the shared contract.
  // ---------------------------------------------------------------------------

  /**
   * Open the archived-modules tab; resolves once rows/empty/loader settle.
   * Enters through the live screen plus client-side navigation because a
   * direct load hangs on the loader (NEWFRONT-228).
   */
  archivesOpenModulesTab(workspaceSlug: string, projectId: string): Promise<void>;
  /**
   * Open the archived-cycles tab; resolves once rows/empty/loader settle.
   * Same live-first client-side entry as the modules tab (NEWFRONT-228).
   */
  archivesOpenCyclesTab(workspaceSlug: string, projectId: string): Promise<void>;
  /** From a live screen, follow the sidebar project menu's Archives entry. */
  archivesGoToProjectArchives(projectId: string): Promise<void>;
  /** Click an archives tab-strip tab and wait for its address to land. */
  archivesSelectArchivesTab(tab: "Modules" | "Cycles"): Promise<void>;
  /**
   * Client-side navigate to a path without reloading (guests have no
   * sidebar Archives entry, and a full load would drop the store).
   */
  archivesClientNavigate(path: string): Promise<void>;
  /**
   * Open a live module's peek panel through its address (live rows link
   * to the detail page instead of peeking); waits for the detail read.
   */
  archivesOpenLiveModulePeek(name: string): Promise<void>;
  /** Open the live modules screen; resolves once its rows settle. */
  archivesOpenLiveModules(workspaceSlug: string, projectId: string): Promise<void>;
  /** Open the live cycles screen; resolves once its rows settle. */
  archivesOpenLiveCycles(workspaceSlug: string, projectId: string): Promise<void>;
  /**
   * Direct-load the archived-modules tab in the current session and report
   * whether rows or an empty state render without a prior live visit.
   * False pins the fetchedMap hang (NEWFRONT-228).
   */
  archivesDirectLoadRenders(workspaceSlug: string, projectId: string): Promise<boolean>;
  /** Wait until a module row with this exact name renders. */
  archivesAwaitModuleRow(name: string): Promise<void>;
  /** Module row names in display order on the current modules tab. */
  archivesModuleRowNames(): Promise<string[]>;
  /** Cycle row names in display order on the archived-cycles tab. */
  archivesCycleRowNames(): Promise<string[]>;
  /** Label the sort control currently shows (e.g. Name, Created date). */
  archivesModuleSortLabel(): Promise<string>;
  /** Pick a sort option (or Ascending/Descending) and wait for the reorder. */
  archivesSetModuleSort(label: string): Promise<void>;
  /** Open the archived-modules search box via its magnifier. */
  archivesOpenModuleSearch(): Promise<void>;
  /** Whether the search input is currently expanded and visible. */
  archivesModuleSearchVisible(): Promise<boolean>;
  /** Type into the search box (the list filters live). */
  archivesTypeModuleSearch(text: string): Promise<void>;
  /** Current search box text. */
  archivesModuleSearchText(): Promise<string>;
  /** Press Escape while the search box holds focus. */
  archivesEscapeModuleSearch(): Promise<void>;
  /** Clear the search through its in-box clear button. */
  archivesClearModuleSearch(): Promise<void>;
  /** Click away from the search box (outside-click collapse). */
  archivesCollapseSearchOutside(): Promise<void>;
  /** Open the filters menu and wait until its groups show. */
  archivesOpenModuleFilters(): Promise<void>;
  /** Filter group titles in display order (Lead, Members, dates). */
  archivesModuleFilterGroups(): Promise<string[]>;
  /** Toggle one lead-filter option by its label (current user reads "You"). */
  archivesToggleLeadFilter(label: string): Promise<void>;
  /** Applied-filter chips in display order (key plus rendered text). */
  archivesModuleChips(): Promise<ArchivesModuleChip[]>;
  /** Remove one applied filter through its chip's remove control. */
  archivesRemoveModuleChip(key: string): Promise<void>;
  /** Clear every applied filter through the clear-all control. */
  archivesClearModuleFilters(): Promise<void>;
  /** Whether the filters menu button indicates active filters. */
  archivesModuleFiltersActive(): Promise<boolean>;
  /** Dismiss the filters menu. */
  archivesCloseModuleFilters(): Promise<void>;
  /** Which empty state the modules tab shows (or rows when it lists). */
  archivesModulesEmptyKind(): Promise<ArchivesModulesEmptyKind>;
  /**
   * Reload the modules tab with the list fetch delayed; resolves true when
   * a skeleton loader showed before content. Restores the route after.
   */
  archivesModulesShowsSkeleton(workspaceSlug: string, projectId: string): Promise<boolean>;
  /** Open a module's peek panel by clicking its row; waits for the detail read. */
  archivesOpenModulePeek(name: string): Promise<void>;
  /** Name the open module peek panel shows, or null when none is open. */
  archivesModulePeekName(): Promise<string | null>;
  /** Close the module peek panel. */
  archivesCloseModulePeek(): Promise<void>;
  /** Open a cycle's peek panel by clicking its row; waits for the detail read. */
  archivesOpenCyclePeek(name: string): Promise<void>;
  /** Name the open cycle peek panel shows, or null when none is open. */
  archivesCyclePeekName(): Promise<string | null>;
  /** Close the cycle peek panel. */
  archivesCloseCyclePeek(): Promise<void>;
  /** Read-only facts about the open archived-module peek panel. */
  archivesModulePeekReadOnly(): Promise<ArchivesPeekReadOnly>;
  /** Read-only facts about the open archived-cycle peek panel. */
  archivesCyclePeekReadOnly(): Promise<ArchivesPeekReadOnly>;
  /** Row-menu entries of a live module (right-click menu). */
  archivesLiveModuleMenuEntries(name: string): Promise<ArchivesMenuEntry[]>;
  /** Pick one entry of a live module's row menu. */
  archivesChooseLiveModuleMenuEntry(name: string, title: string): Promise<void>;
  /** Row-menu entries of an archived module (right-click menu). */
  archivesArchivedModuleMenuEntries(name: string): Promise<ArchivesMenuEntry[]>;
  /** Pick one entry of an archived module's row menu. */
  archivesChooseArchivedModuleMenuEntry(name: string, title: string): Promise<void>;
  /** The open archive dialog's copy, or null when none is open. */
  archivesArchiveDialog(): Promise<ArchivesArchiveDialog | null>;
  /** Confirm the open archive dialog; waits until its write settles. */
  archivesConfirmArchiveDialog(): Promise<void>;
  /** Cancel the open archive dialog. */
  archivesCancelArchiveDialog(): Promise<void>;
  /** Fail the next module archive/restore write once (failure shaping). */
  archivesFailNextModuleWrite(): Promise<void>;

  // Archived work-items list, filters, display, peek (NEWFRONT-222,
  // ARCH-001..007). Observed on apps/web: the archives tab strip with
  // cycle/module gating, the breadcrumb header with the archived-count
  // badge, per-tab browser titles, the read-only archived list with row
  // menus, the shared filter expression with its chip row, the Display
  // control (grouping incl. none, ordering incl. manual, columns), and
  // the side peek panel with locked fields and address sync. Appended;
  // existing entries above are untouched per the shared contract.
  // ---------------------------------------------------------------------------

  /** Open the archived work-items tab of a project; waits until the list settles. */
  archivesOpenIssuesList(workspaceSlug: string, projectId: string): Promise<void>;
  /** Switch to an archives tab through the tab strip; waits for navigation. */
  archivesOpenTab(tab: "issues" | "cycles" | "modules"): Promise<void>;
  /** Visible archives tab labels in display order. */
  archivesTabNames(): Promise<string[]>;
  /** Label of the currently active archives tab. */
  archivesActiveTab(): Promise<string>;
  /** Whether the breadcrumb back control renders (narrow screens only). */
  archivesBackPresent(): Promise<boolean>;
  /** Activate the breadcrumb back control (narrow screens only). */
  archivesClickBack(): Promise<void>;
  /** Archived-count badge text, or null when the badge hides (zero). */
  archivesCountBadge(): Promise<string | null>;
  /** Hover tip of the count badge, or null when the badge hides. */
  archivesCountBadgeTooltip(): Promise<string | null>;
  /** Current browser tab title. */
  archivesPageTitle(): Promise<string>;
  /** Names of the archived issues rendered, in display order. */
  archivesVisibleIssueNames(): Promise<string[]>;
  /** Group header labels in display order (empty when grouping is off). */
  archivesGroupHeadings(): Promise<string[]>;
  /** Open the row menu of the named archived issue. */
  archivesOpenRowMenu(name: string): Promise<void>;
  /** Entries of the open row menu in display order. */
  archivesRowMenuEntries(): Promise<string[]>;
  /** Dismiss any open row menus. */
  archivesCloseMenus(): Promise<void>;
  /**
   * Click a property cell of the named row and report whether any inline
   * editor opens (the archived list is read-only, so never).
   */
  archivesInlineEditorOpens(name: string): Promise<boolean>;
  /**
   * Wait for the next archived-issues list fetch and return its query.
   * Callers start this before the UI action that triggers the refetch.
   */
  archivesWaitForListQuery(): Promise<ArchivesListQuery>;
  /** Full visible text of the named archived row (name plus property cells). */
  archivesRowText(name: string): Promise<string>;
  /** Title text shown in the archived peek, or null while it loads. */
  archivesPeekTitle(): Promise<string | null>;
  /** Whether the peek title is locked for editing (renders as plain text, no input). */
  archivesPeekTitleLocked(): Promise<boolean>;
  /** Visible text of the peek description, whitespace-collapsed. */
  archivesPeekDescriptionText(): Promise<string>;
  /** Whether the peek description can be edited. */
  archivesPeekDescriptionEditable(): Promise<boolean>;
  /** Whether the peek activity composer accepts input. */
  archivesPeekActivityEditable(): Promise<boolean>;
  /** Peek selection carried in the current address (nulls when absent). */
  archivesPeekQueryParams(): Promise<{ issue: string | null; project: string | null; nesting: string | null }>;
  /**
   * Seed the project's stored archived filter expression and reload onto
   * the list; resolves once the list settles with the expression applied.
   * Stands in for the UI filter entry, which is inert on apps/web from
   * the empty state (bug NEWFRONT-242).
   */
  archivesSeedStoredExpression(
    workspaceSlug: string,
    projectId: string,
    expression: ArchivesFilterExpression
  ): Promise<void>;

  // --- Project views list (NEWFRONT-42, VIEW-001-012). Observed on the
  // --- running old app: the list page titles its tab "<project> - Views"
  // --- and trails project crumbs with a views crumb; the header carries
  // --- an expandable search, a sort menu, a filters menu and an Add view
  // --- action; rows link to the detail page with an access badge, the
  // --- owner's avatar, a role-gated favorite star and an overflow menu.
  // --- With the feature off the list is replaced by the gate empty state
  // --- whose Manage-features shortcut only project admins can use.
  /** Saved-view names in display order. */
  viewsListNames(): Promise<string[]>;
  /** Breadcrumb trail texts above the list. */
  viewsListBreadcrumb(): Promise<string[]>;
  /** Browser tab title on the views list. */
  viewsListTabTitle(): Promise<string>;
  /** Whether the header Add-view action renders. */
  viewsHeaderAddVisible(): Promise<boolean>;
  /** Open the create dialog from the header action; resolves once it opens. */
  viewsOpenCreateFromHeader(): Promise<void>;
  /** Whether the list loading skeleton currently shows. */
  viewsListSkeletonVisible(): Promise<boolean>;
  /** Delay views-list API answers by `ms` so the skeleton is observable. */
  viewsDelayListLoad(ms: number): Promise<void>;
  /** Zero-views empty-state title, or "" when the list renders. */
  viewsEmptyTitle(): Promise<string>;
  /** Whether the empty-state create action renders. */
  viewsEmptyCreateVisible(): Promise<boolean>;
  /** Whether the empty-state create action is enabled. */
  viewsEmptyCreateEnabled(): Promise<boolean>;
  /** Open the create dialog from the empty-state action; resolves once it opens. */
  viewsEmptyCreateOpen(): Promise<void>;
  /** No-match empty-state title, or "" when rows or the zero state render. */
  viewsNoMatchTitle(): Promise<string>;
  /** Deep-link href of the row `name`, or null when the row is absent. */
  viewsRowHref(name: string): Promise<string | null>;
  /** Access badge of the row `name` ("Public"/"Private"), "" when none shows. */
  viewsRowAccess(name: string): Promise<string>;
  /** Whether the row `name` shows its owner's avatar. */
  viewsRowOwnerAvatar(name: string): Promise<boolean>;
  /** Whether the row `name` carries the published Live marker. */
  viewsRowLiveVisible(name: string): Promise<boolean>;
  /** Whether the row `name` renders a favorite star (role-gated). */
  viewsRowStarVisible(name: string): Promise<boolean>;
  /** Whether the row `name`'s star shows selected. */
  viewsRowStarSelected(name: string): Promise<boolean>;
  /** Toggle the row `name`'s favorite star, after network quiescence. */
  viewsToggleStar(name: string): Promise<void>;
  /** Visible text of the row `name` (icon glyphs, name, badges). */
  viewsRowText(name: string): Promise<string>;
  // --- Project views list controls (NEWFRONT-42, VIEW-004-007). Observed
  // --- on the running old app: the search field sits collapsed as an
  // --- icon until opened; Escape clears the query first and collapses
  // --- the empty field on a second press; the sort menu offers name and
  // --- timestamps with a direction half; the filters menu offers
  // --- favorites, creation date and creator (the access dimension is a
  // --- cloud-only stub on this build); applied filters render as chips.
  /** Whether the collapsed search trigger renders. */
  viewsSearchTriggerVisible(): Promise<boolean>;
  /** Open the collapsed search field; resolves once it focuses. */
  viewsSearchOpen(): Promise<void>;
  /** Whether the search field currently shows expanded. */
  viewsSearchExpanded(): Promise<boolean>;
  /** Type into the list search field. */
  viewsSearchType(text: string): Promise<void>;
  /** Current list-search query. */
  viewsSearchValue(): Promise<string>;
  /** Whether the list-search field currently holds focus. */
  viewsSearchFocused(): Promise<boolean>;
  /** Press Escape while the search field focuses. */
  viewsSearchEscape(): Promise<void>;
  /** Click the search field's clear control. */
  viewsSearchClear(): Promise<void>;
  /** Click outside the search field (breadcrumb area). */
  viewsSearchClickOutside(): Promise<void>;
  /** Sort trigger label (the active sort key). */
  viewsSortTriggerText(): Promise<string>;
  /** Open the sort menu. */
  viewsSortOpen(): Promise<void>;
  /** Sort menu item texts in display order. */
  viewsSortMenuTexts(): Promise<string[]>;
  /** Whether the sort menu item `text` carries the selected checkmark. */
  viewsSortMenuSelected(text: string): Promise<boolean>;
  /** Pick the sort menu item `text`; resolves once the menu closes. */
  viewsSortPick(text: string): Promise<void>;
  /** Open the list filters menu. */
  viewsFiltersOpen(): Promise<void>;
  /** Visible text of the open filters panel. */
  viewsFiltersPanelText(): Promise<string>;
  /** Toggle the favorites row in the open filters menu. */
  viewsFiltersToggleFavorites(): Promise<void>;
  /** Option texts of the created-date section in the open filters menu. */
  viewsFiltersDateOptions(): Promise<string[]>;
  /** Pick the created-date option `text` in the open filters menu. */
  viewsFiltersPickDate(text: string): Promise<void>;
  /** Member names offered in the created-by section of the open menu. */
  viewsFiltersCreatorOptions(): Promise<string[]>;
  /** Pick the creator `name` in the open filters menu. */
  viewsFiltersPickCreator(name: string): Promise<void>;
  /** Whether the open filters menu offers an access-type section (cloud-only). */
  viewsFiltersAccessPresent(): Promise<boolean>;
  /** Type into the open filters menu's own search box. */
  viewsFiltersSearchType(text: string): Promise<void>;
  /** Close the filters menu; resolves once the panel hides. */
  viewsFiltersClose(): Promise<void>;
  /** Whether the applied-filters chip strip renders. */
  viewsChipsVisible(): Promise<boolean>;
  /** Applied chip texts in display order. */
  viewsChipTexts(): Promise<string[]>;
  /** Remove one value from the `dimension` chip. */
  viewsChipRemoveValue(dimension: string, value: string): Promise<void>;
  /** Remove the whole `dimension` chip. */
  viewsChipRemoveDimension(dimension: string): Promise<void>;
  /** Click the clear-all chip; resolves once the strip hides. */
  viewsChipsClearAll(): Promise<void>;
  // --- Project views flag gate (NEWFRONT-42, VIEW-002). Observed on the
  // --- running old app: with the project's views feature off, the list
  // --- route renders an explanatory empty state instead of the list, with
  // --- a Manage-features shortcut into project settings that stays
  // --- disabled for non-admins.
  /** Gate empty-state title, or "" when the list renders. */
  viewsGateTitle(): Promise<string>;
  /** Whether the gate's Manage-features shortcut renders. */
  viewsGateManageVisible(): Promise<boolean>;
  /** Whether the gate's Manage-features shortcut is enabled. */
  viewsGateManageEnabled(): Promise<boolean>;
  /** Follow the Manage-features shortcut; resolves on the settings page. */
  viewsGateManageOpen(): Promise<void>;
  // --- Project view dialog (NEWFRONT-42, VIEW-003, VIEW-013, VIEW-014,
  // --- VIEW-015, VIEW-019). Observed on the running old app: the create
  // --- and update forms share one dialog with an icon picker, a required
  // --- title, an optional description, a layout picker, a Display
  // --- dropdown and an expanded work-item filter builder; the access
  // --- selector is a cloud-only stub on this build. Create success
  // --- navigates to the new detail page; failure toasts and keeps the
  // --- dialog open with its input intact.
  /** Dialog heading ("Create View"/"Update View"), or null when closed. */
  viewsDialogHeading(): Promise<string | null>;
  /** Fill the dialog title field. */
  viewsDialogFillTitle(text: string): Promise<void>;
  /** Current dialog title value. */
  viewsDialogTitleValue(): Promise<string>;
  /** Inline title validation message, or "" when none shows. */
  viewsDialogTitleError(): Promise<string>;
  /** Fill the dialog description field. */
  viewsDialogFillDescription(text: string): Promise<void>;
  /** Current dialog description value. */
  viewsDialogDescriptionValue(): Promise<string>;
  /** Whether the dialog offers an access selector (cloud-only). */
  viewsDialogAccessPresent(): Promise<boolean>;
  /** Open the dialog icon picker. */
  viewsDialogIconOpen(): Promise<void>;
  /** Icon picker tab names in display order. */
  viewsDialogIconTabs(): Promise<string[]>;
  /** Pick the first glyph icon in the open picker. */
  viewsDialogPickFirstIcon(): Promise<void>;
  /** Emoji preview currently shown in the dialog, or "" when none shows. */
  viewsDialogIconPreview(): Promise<string>;
  /** Open the dialog Display dropdown. */
  viewsDialogDisplayOpen(): Promise<void>;
  /** Display option texts in the open dropdown. */
  viewsDialogDisplayTexts(): Promise<string[]>;
  /** Whether the work-item filter builder renders expanded in the dialog. */
  viewsDialogFiltersExpanded(): Promise<boolean>;
  /** Current dialog layout choice label. */
  viewsDialogLayoutValue(): Promise<string>;
  /** Pick a layout in the dialog layout dropdown. */
  viewsDialogPickLayout(label: string): Promise<void>;
  /** Dismiss the dialog with Escape; resolves once it hides. */
  viewsDialogEscape(): Promise<void>;
  /** Cancel the dialog; resolves once it closes. */
  viewsDialogCancel(): Promise<void>;
  /** Submit the dialog's primary action; resolves once it closes. */
  viewsDialogSubmit(): Promise<void>;
  /** Click the dialog's submit without waiting (failure-path proofs). */
  viewsDialogSubmitAttempt(): Promise<void>;
  /** Whether the view dialog is currently open. */
  viewsDialogOpen(): Promise<boolean>;
  /** Fail the next view write with HTTP `status` (failure-path proofs). */
  viewsFailNextWrite(status: number): Promise<void>;
  // --- Project view row menu (NEWFRONT-42, VIEW-016, VIEW-017, VIEW-018,
  // --- VIEW-020). Observed on the running old app: each row's overflow
  // --- menu offers edit for the owner, open-in-new-tab and copy-link for
  // --- every role, and delete for the owner or a project admin; the
  // --- publish entry is a cloud-only stub on this build.
  /** Open the row `name`'s overflow menu; resolves once items show. */
  viewsRowMenuOpen(name: string): Promise<void>;
  /** Visible overflow-menu item texts in display order. */
  viewsRowMenuItems(): Promise<string[]>;
  /** Pick the overflow-menu item `item`; resolves once the menu closes. */
  viewsRowMenuPick(item: string): Promise<void>;
  /** Copy the row's deep link; resolves with the clipboard text. */
  viewsRowCopyLink(name: string): Promise<string>;
  /** Whether the open menu offers a publish entry (cloud-only). */
  viewsRowMenuPublishPresent(): Promise<boolean>;
  /** Open the row `name`'s menu and follow its new-tab entry; resolves with the popup URL. */
  viewsRowOpenNewTabHref(name: string): Promise<string>;
  // --- Project view delete (NEWFRONT-42, VIEW-016). Observed on the
  // --- running old app: deletion confirms through a modal that names
  // --- the loss (sort, filter, display and layout), then toasts and
  // --- returns to the list.
  /** Delete-confirmation title, or "" when closed. */
  viewsDeleteTitle(): Promise<string>;
  /** Delete-confirmation body text, or "" when closed. */
  viewsDeleteBody(): Promise<string>;
  /** Confirm deletion; resolves once the modal closes. */
  viewsDeleteConfirm(): Promise<void>;
  /** Click delete confirm without waiting for the modal to close. */
  viewsDeleteConfirmAttempt(): Promise<void>;
  /** Cancel deletion; resolves once the modal closes. */
  viewsDeleteCancel(): Promise<void>;
  /** Detail breadcrumb trail texts (project, list, current view). */
  viewsDetailBreadcrumb(): Promise<string[]>;
  /** Missing-view error title, or "" when the detail renders. */
  viewsDetailErrorTitle(): Promise<string>;
  /** Click the error state's back-to-list button. */
  viewsDetailErrorBack(): Promise<void>;
  /** Open the detail view switcher from the current view `name`. */
  viewsDetailSwitcherOpen(name: string): Promise<void>;
  /** Switcher option names in display order. */
  viewsDetailSwitcherOptions(): Promise<string[]>;
  /** Whether the switcher offers a search box. */
  viewsDetailSwitcherSearchVisible(): Promise<boolean>;
  /** Type into the switcher search box. */
  viewsDetailSwitcherSearch(text: string): Promise<void>;
  /** Pick a switcher option by name; resolves once navigation lands. */
  viewsDetailSwitcherPick(name: string): Promise<void>;
  /** Whether the private-view lock shows in the detail header. */
  viewsDetailLockVisible(): Promise<boolean>;
  /** Active layout index (0 list, 1 board, 2 calendar, 3 table, 4 timeline). */
  viewsDetailLayoutActive(): Promise<number>;
  /** Pick a layout by index; resolves once the choice applies. */
  viewsDetailLayoutPick(index: number): Promise<void>;
  /** Whether the layout switcher renders in the detail header. */
  viewsDetailLayoutVisible(): Promise<boolean>;
  /** Whether the display dropdown renders in the detail header. */
  viewsDetailDisplayVisible(): Promise<boolean>;
  /** Whether the work-item filter toggle renders in the detail header. */
  viewsDetailFiltersToggleVisible(): Promise<boolean>;
  /** Whether the add-work-item button renders in the detail header. */
  viewsDetailAddVisible(): Promise<boolean>;
  /** Empty-issues title on a fresh/empty detail, or "" when rows render. */
  viewsDetailEmptyTitle(): Promise<string>;
  /** Browser tab title on the detail page. */
  viewsDetailTabTitle(): Promise<string>;
  /** Display dropdown option texts for the active layout. */
  viewsDetailDisplayOptions(): Promise<string[]>;
  /** Add a saved-query condition; resolves once Update view offers. */
  viewsDetailFilterAdd(property: string, value: string): Promise<void>;
  /** Click Update view; resolves once the button hides. */
  viewsDetailUpdateView(): Promise<void>;
  /** Whether the Save as button offers in the filter row. */
  viewsDetailSaveAsVisible(): Promise<boolean>;
  /** Click Save as in the filter row. */
  viewsDetailSaveAsClick(): Promise<void>;
  /** Click the Add work item button. */
  viewsDetailAddClick(): Promise<void>;
  /** Whether the detail body text currently contains `name`. */
  viewsDetailShowsIssue(name: string): Promise<boolean>;
  /** Custom workspace view names in list order. */
  wsViewsListNames(): Promise<string[]>;
  /** Static default view names in list order. */
  wsViewsDefaultNames(): Promise<string[]>;
  /** Browser tab title on the workspace views list. */
  wsViewsListTabTitle(): Promise<string>;
  /** Fill the workspace views list search box. */
  wsViewsSearchFill(text: string): Promise<void>;
  /** Description under a custom row, or "" when the row shows none. */
  wsViewsRowDescription(name: string): Promise<string>;
  /** Open a custom row's overflow menu. */
  wsViewsRowMenuOpen(name: string): Promise<void>;
  /** Visible entries of the open row menu. */
  wsViewsRowMenuEntries(): Promise<string[]>;
  /** Pick an entry of the open row menu. */
  wsViewsRowMenuPick(entry: string): Promise<void>;
  /** Whether the customs-loading skeleton shows. */
  wsViewsSkeletonVisible(): Promise<boolean>;
  /** Whether the skeleton appears at any point within `windowMs`. */
  wsViewsSkeletonFlashed(windowMs: number): Promise<boolean>;
  /** Delay every customs-list GET by `ms` for the rest of the test. */
  wsViewsDelayNextList(ms: number): Promise<void>;
  /** Fail the next workspace-views write with `status` (once). */
  wsViewsFailNextWrite(status: number): Promise<void>;
  /** Whether the workspace tab strip renders anywhere. */
  wsViewsStripVisible(): Promise<boolean>;
  /** Whether any star control renders on the workspace views list. */
  wsViewsListStarVisible(): Promise<boolean>;
  /** Whether a default row carries an overflow menu button. */
  wsViewsDefaultRowHasMenu(name: string): Promise<boolean>;
  /** Click a custom row through to its detail page. */
  wsViewsRowOpen(name: string): Promise<void>;
  /** Display dropdown option texts inside the open create/edit dialog. */
  wsViewsDialogDisplayOptions(): Promise<string[]>;
  /** Browser tab title on a workspace view detail. */
  wsViewsDetailTabTitle(): Promise<string>;
  /** Detail breadcrumb texts (views list, current view). */
  wsViewsDetailCrumbs(): Promise<string[]>;
  /** Open the detail switcher from the current view `name`. */
  wsViewsDetailSwitcherOpen(name: string): Promise<void>;
  /** Switcher option names in display order. */
  wsViewsDetailSwitcherOptions(): Promise<string[]>;
  /** Whether the switcher offers a search box. */
  wsViewsDetailSwitcherSearchVisible(): Promise<boolean>;
  /** Type into the switcher search box. */
  wsViewsDetailSwitcherSearch(text: string): Promise<void>;
  /** Pick a switcher option by name. */
  wsViewsDetailSwitcherPick(name: string): Promise<void>;
  /** Whether a layout selector renders in the detail header. */
  wsViewsDetailLayoutVisible(): Promise<boolean>;
  /** Whether the display dropdown renders in the detail header. */
  wsViewsDetailDisplayVisible(): Promise<boolean>;
  /** Whether the work-item filter toggle renders in the detail header. */
  wsViewsDetailFiltersToggleVisible(): Promise<boolean>;
  /** Whether the Add view button renders in the detail header. */
  wsViewsDetailAddVisible(): Promise<boolean>;
  /** Click the Add view button. */
  wsViewsDetailAddClick(): Promise<void>;
  /** Open the detail header quick-actions menu. */
  wsViewsDetailMenuOpen(): Promise<void>;
  /** Visible entries of the open detail menu. */
  wsViewsDetailMenuEntries(): Promise<string[]>;
  /** Copy-link from the detail menu; returns the clipboard text. */
  wsViewsDetailCopyLink(): Promise<string>;
  /** Open-in-new-tab from the detail menu; returns the popup URL. */
  wsViewsDetailOpenNewTabHref(): Promise<string>;
  /** Missing-view error title, or "" when the detail renders. */
  wsViewsDetailErrorTitle(): Promise<string>;
  /** Click the error state's way-back button. */
  wsViewsDetailErrorBack(): Promise<void>;
  /** Whether the settings page shows the not-authorized notice. */
  viewsSettingsNotAuthorized(): Promise<boolean>;
  /** The Enable-views switch state on the features settings page. */
  viewsSettingsViewsToggleValue(): Promise<boolean>;
  /** Flip the Enable-views switch. */
  viewsSettingsViewsToggleFlip(): Promise<void>;
  /** Focused control labels while tabbing through the open view dialog. */
  viewsDialogTabOrder(): Promise<string[]>;
  /** Drag one project-list row onto another by name. */
  viewsListDragRow(from: string, to: string): Promise<void>;
  /** Whether any export/import/print control shows on a views page. */
  viewsPageExportImportVisible(): Promise<boolean>;
  // --- Notifications cross-cutting: roles, links, realtime, absent,
  // --- errors (NEWFRONT-202, NTF-026..031). Appended; existing methods
  // --- above are untouched per the shared driver contract.

  /** Full inbox address (path plus any query/hash), as the address bar shows it. */
  notificationsCurrentUrl(): Promise<string>;
  /** Fail inbox list GETs with `status` (item writes and unread counts pass through). */
  notificationsFailListFetches(status: number): Promise<void>;
  /** Remove the list-fetch failure route. */
  notificationsClearListFetchFailure(): Promise<void>;
  /** Whether any retry control shows inside the inbox list pane. */
  notificationsRetryControlVisible(): Promise<boolean>;
  /** Press one keyboard key with the inbox focused. */
  notificationsPressKey(key: string): Promise<void>;
  /** Drag the card at `fromIndex` onto the card at `toIndex`. */
  notificationsDragCard(fromIndex: number, toIndex: number): Promise<void>;
  /** Whether any export/import/download control shows inside the inbox list pane. */
  notificationsExportImportVisible(): Promise<boolean>;
  /**
   * Enter the inbox fresh, recording every notification-API request path
   * (list, unread, per-item, preferences) the entry fires. Paths are URL
   * paths without query strings, deduplicated.
   */
  notificationsEntryRequestPaths(workspaceSlug: string): Promise<string[]>;
  /** Arm a counter for Notification.requestPermission calls from this point on. */
  notificationsArmNotificationRequestSpy(): Promise<void>;
  /** Number of Notification.requestPermission calls observed since arming. */
  notificationsNotificationRequestCount(): Promise<number>;

  // --- Archives cross-cutting (NEWFRONT-226, ARCH-026–032). Appended;
  // --- the methods above are untouched per the shared driver contract.
  /** Confirm-button label in the open project archive/restore dialog. */
  archivesProjectConfirmLabel(): Promise<string>;
  /** Whether the dialog confirm currently shows progress (busy). */
  archivesProjectConfirmBusy(): Promise<boolean>;
  /** Delay the next project archive/restore write by ms (progress shaping). */
  archivesProjectDelayNextWrite(ms: number): Promise<void>;
  /** Fail the next project archive/restore write once (failure shaping). */
  archivesProjectFailNextWrite(): Promise<void>;
  /** Fail the next issue/cycle/module archive/restore write once. */
  archivesItemFailNextWrite(): Promise<void>;
  /** Open one archives tab; settles when content, empty, or loader shows. */
  archivesTabOpen(workspaceSlug: string, projectId: string, tab: ArchivesTab): Promise<void>;
  /** Active tab key, read from the address (labels live on archivesActiveTab). */
  archivesActiveTabKey(): Promise<ArchivesTab>;
  /** Open the action menu of the row showing `name` on the current tab. */
  archivesRowMenuOpenFirst(name: string): Promise<void>;
  /** First-line titles of the open row menu (see web.ts for the sibling split). */
  archivesRowMenuEntryTitles(): Promise<string[]>;
  /** Click the open row menu entry whose label contains `entry`. */
  archivesRowMenuClick(entry: string): Promise<void>;
  /** Select the row showing `name` to open its peek panel. */
  archivesPeekOpenFirst(name: string): Promise<void>;
  /** Whether a peek panel currently shows. */
  archivesPeekVisible(): Promise<boolean>;
  /** Close the open peek panel. */
  archivesPeekClose(): Promise<void>;
  /** Archived-detail banner text, or null when no banner shows. */
  archivesDetailBannerText(): Promise<string | null>;
  /** Whether a loading skeleton shows in the archives content. */
  archivesSkeletonVisible(): Promise<boolean>;
  /** Delay the next archived-list/detail reads by ms (skeleton shaping). */
  archivesDelayNextListReads(ms: number): Promise<void>;
  /** Start counting archived list reads and archive writes. */
  archivesBeginTrafficSpy(): Promise<void>;
  /** Traffic counters since the spy began. */
  archivesTrafficCounts(): Promise<ArchivesTrafficCounts>;
  /** Whether a filter or search control renders on the current tab. */
  archivesFilterControlVisible(): Promise<boolean>;
  /** Type into the tab's search box (cycles/modules tabs). */
  archivesSearchType(text: string): Promise<void>;
  /** Current search-box text. */
  archivesSearchText(): Promise<string>;
  /** Whether the first row is draggable. */
  archivesRowDraggable(name: string): Promise<boolean>;
  /** Drag the `source` row onto the `target` row; whether their vertical order flips. */
  archivesDragReorders(source: string, target: string): Promise<boolean>;
  /** Start recording whether any skeleton mounts from now on. */
  archivesArmSkeletonObserver(): Promise<void>;
  /** Whether a skeleton mounted since the observer armed. */
  archivesSkeletonWasSeen(): Promise<boolean>;
  /** Whether an export/download control renders on the current tab. */
  archivesExportControlVisible(): Promise<boolean>;
  /** Press a key with the archives content focused (absence probe). */
  archivesPressKey(key: string): Promise<void>;
  /** Whether a row showing `name` renders in the archives content. */
  archivesRowPresent(name: string): Promise<boolean>;
  /** Whether `text` shows anywhere on the page (live-page settle reads). */
  archivesPageTextPresent(text: string): Promise<boolean>;
  /**
   * Prime the session on the live cycles page, then enter the archives
   * through the project header menu without reloading (cold entry to the
   * cycles/modules tabs never settles — NEWFRONT-239).
   */
  archivesPrimeAndEnter(workspaceSlug: string, projectId: string, liveCycleName: string): Promise<void>;
  /** Click the tab-strip link for `tab` (client-side) and settle on it. */
  archivesTabClick(tab: ArchivesTab): Promise<void>;

  // --- Cycles edit/delete/archive oracles (NEWFRONT-251, CYC-017–024). ---
  // --- Appended; existing methods above are untouched per the shared
  // --- driver contract. Covers the live cycles list's create/update
  // --- dialog (edit reuse, Escape, keyboard order), the stored list tab,
  // --- the active-cycle hero refresh, creation gating, the delete
  // --- confirm dialog, the finished-cycle read-only surface, and the
  // --- archive confirm dialog. Reads are user-visible; the suite never
  // --- asserts store internals.
  /** Open the live row's Edit dialog; waits for the Update heading. */
  cyclesEditOpenUpdateDialog(name: string): Promise<void>;
  /** Open the live cycles list without settling (empty views never settle the shared wait). */
  cyclesEditOpenListRaw(workspaceSlug: string, projectId: string): Promise<void>;
  /** Create/update dialog heading, or null when no dialog is open. */
  cyclesEditDialogHeading(): Promise<string | null>;
  /** Fill the open dialog's title field. */
  cyclesEditFillName(text: string): Promise<void>;
  /** Fill the open dialog's description field. */
  cyclesEditFillDescription(text: string): Promise<void>;
  /** Open the range calendar from the update dialog's dates trigger. */
  cyclesEditRangeOpen(): Promise<void>;
  /** Submit the update dialog; resolves once it closes. */
  cyclesEditSubmitUpdate(): Promise<void>;
  /**
   * Submit the update dialog while counting overlap-check POSTs; resolves
   * once the dialog closes. Proves the unchanged-dates skip (CYC-017).
   */
  cyclesEditSubmitCountingDateChecks(): Promise<{ dateChecks: number }>;
  /** Parsed `cycle_tab` list-tab value from storage, or null when unset. */
  cyclesEditStoredCycleTab(): Promise<string | null>;
  /** Clear the stored `cycle_tab` value. */
  cyclesEditClearStoredCycleTab(): Promise<void>;
  /** Cycle name shown in the Active-cycle hero panel, or null when the empty view shows. */
  cyclesEditHeroCycleName(): Promise<string | null>;
  /** Press Escape with the create/update dialog open. */
  cyclesEditPressEscape(): Promise<void>;
  /** Whether the dialog's title field currently holds keyboard focus. */
  cyclesEditTitleFocused(): Promise<boolean>;
  /**
   * Focus trail through the open dialog: the focused control's label now,
   * then after each of `steps` Tab presses.
   */
  cyclesEditFocusTrail(steps: number): Promise<string[]>;
  /** Whether the list header's create button is present. */
  cyclesEditCreateButtonVisible(): Promise<boolean>;
  /** First-run empty view's creation shortcut: presence plus disabled state. */
  cyclesEditEmptyCreateState(): Promise<{ visible: boolean; disabled: boolean }>;
  /** Open the live row's quick-look panel through its row control. */
  cyclesEditOpenPeek(name: string): Promise<void>;
  /** Open the quick-actions menu on the cycle detail page. */
  cyclesEditOpenDetailMenu(): Promise<void>;
  /** Delete-confirm dialog heading plus body, or null when none shows. */
  cyclesEditDeleteDialogText(): Promise<{ heading: string; body: string } | null>;
  /** Confirm the delete dialog; resolves once the write lands (callers read the toast immediately). */
  cyclesEditDeleteConfirm(): Promise<void>;
  /** Cancel the delete dialog; resolves once it closes. */
  cyclesEditDeleteCancel(): Promise<void>;
  /** Fail the next cycle DELETE once with the server's permission refusal. */
  cyclesEditFailNextDeleteWrite(): Promise<void>;
  /** Read-only notice on a finished cycle's detail, or null when absent. */
  cyclesEditDetailReadOnlyNotice(): Promise<string | null>;

  // Archived work-item mutations (NEWFRONT-223, ARCH-008..013). Row-menu
  // restore/delete/copy/open reuse the layouts row-menu reads where they
  // match; the methods below cover only gaps: archived-list navigation,
  // mutation failure shaping, the archive dialog's text, and the archived
  // detail screen (breadcrumb, banner, loader, not-found, locked editing).
  // Appended; existing entries above are untouched per the shared contract.
  // ---------------------------------------------------------------------------

  /** Open the archived work-items list of a project; ends settled (rows or empty state). */
  archivesOpenList(workspaceSlug: string, projectId: string): Promise<void>;
  /** Names of the archived work items currently rendered, in display order. */
  archivesMutationVisibleIssueNames(): Promise<string[]>;
  /**
   * Fail the next request matching `method` whose URL contains `urlPart`
   * once with a 500 (failure shaping for restore/delete/archive).
   */
  archivesFailNextMutation(method: "POST" | "DELETE", urlPart: string): Promise<void>;
  /** Remove any armed archives mutation failure. */
  archivesClearMutationFailure(): Promise<void>;
  /** Heading text of the open archive dialog, or null when no dialog shows. */
  archivesArchiveModalTitle(): Promise<string | null>;
  /** Body text of the open archive dialog, or null when no dialog shows. */
  archivesArchiveModalBody(): Promise<string | null>;
  /** Dismiss the open archive dialog through its cancel control. */
  archivesArchiveModalCancel(): Promise<void>;
  /** Pick one entry of the archived detail header menu (reads reuse layoutsDetailMenuItems). */
  archivesDetailMenuChoose(item: string): Promise<void>;
  /** Open an archived detail address without waiting for the body (raw navigation for loader/not-found). */
  archivesOpenDetailRaw(workspaceSlug: string, projectId: string, issueId: string): Promise<void>;
  /** Breadcrumb trail text on the archived detail screen. */
  archivesDetailBreadcrumbText(): Promise<string>;
  /** Archive banner text on the archived detail screen, or null when absent. */
  archivesMutationDetailBannerText(): Promise<string | null>;
  /** Follow the archive banner's back control to the archived list. */
  archivesDetailBannerBack(): Promise<void>;
  /** Whether the detail address shows the not-found state. */
  archivesDetailNotFoundVisible(): Promise<boolean>;
  /**
   * Enter the archived detail while gating its record read, reporting what
   * the screen shows with the fetch provably outstanding (plus the
   * intercepted read count). Releases the hold and settles on the detail.
   */
  archivesDetailPendingStateOnHeldFetch(
    workspaceSlug: string,
    projectId: string,
    issueId: string
  ): Promise<{
    heldRequests: number;
    bannerDuringHold: string | null;
    breadcrumbDuringHold: string;
    activityDuringHold: boolean;
    settled: boolean;
  }>;
  /** Whether the archived detail renders the activity composer. */
  archivesDetailComposerVisible(): Promise<boolean>;
  /** Whether the archived detail's issue-level reaction control is enabled (false when absent or disabled). */
  archivesDetailReactionControlEnabled(): Promise<boolean>;

  // --- NEWFRONT-178 (runners overview + pods, RUN-001/005, RUN-010/015).
  // --- Appended; existing methods above are untouched per the shared
  // --- driver contract. One page serves the workspace and project
  // --- scopes; projectId selects the scoped route tree.

  /** Open the runners overview; projectId selects the project-scoped twin. */
  runnersOpenOverview(workspaceSlug: string, projectId?: string, subpath?: "runs" | "approvals"): Promise<void>;
  /** Current page URL (deep-link and navigation assertions). */
  runnersCurrentUrl(): Promise<string>;
  /** Tab strip entries: label, link target, and which tab reads active. */
  runnersTabs(): Promise<RunnersTab[]>;
  /** Click a tab; resolves once the URL carries the tab route. */
  runnersOpenTab(label: string): Promise<void>;
  /** Runner table rows in display order (excludes the empty state). */
  runnersTableRows(): Promise<RunnersTableRow[]>;
  /** Whether the table shows its empty state. */
  runnersEmptyVisible(): Promise<boolean>;
  /** Actions cell of one runner row: which actions are offered. */
  runnersRowActions(runnerName: string): Promise<RunnersRowActions>;
  /** Follow one row's Details action; resolves on the detail route. */
  runnersOpenDetails(runnerName: string): Promise<void>;
  /** Delete one runner through its row action plus confirmation. */
  runnersDeleteRunner(runnerName: string, options?: { expectFailure?: boolean }): Promise<void>;
  /** Revoke one runner through its row action plus confirmation. */
  runnersRevokeRunner(runnerName: string, options?: { expectFailure?: boolean }): Promise<void>;
  /** Title and body of the open confirmation modal, or null when none. */
  runnersModalCopy(): Promise<{ title: string; body: string } | null>;
  /** Dismiss the open confirmation modal via its cancel control. */
  runnersCancelModal(): Promise<void>;
  /** Hold runner/pod mutation responses by ms (in-flight assertions). */
  runnersStallMutations(delayMs: number): Promise<void>;
  /** Fail the next runner/pod mutation (error-toast assertions). */
  runnersFailNextMutation(): Promise<void>;
  /** Release any stalls/failures installed by the two methods above. */
  runnersReleaseMutationShaping(): Promise<void>;
  /** Side-nav rail: header, overview link, contacts, empty state. */
  runnersRail(): Promise<RunnersRail>;
  /** Pod tiles in display order with default marker and runner count. */
  runnersPodTiles(): Promise<RunnersPodTile[]>;
  /** Toggle one pod tile's table filter. */
  runnersSelectPod(podName: string): Promise<void>;
  /** Active pod-filter label, or null when no filter applies. */
  runnersFilterText(): Promise<string | null>;
  /** Clear the active pod filter through its affordance. */
  runnersClearPodFilter(): Promise<void>;
  /** Open the create-pod modal through the create tile. */
  runnersOpenCreatePod(): Promise<void>;
  /** Validation strings currently shown in the open pod modal. */
  runnersPodFormErrors(): Promise<string[]>;
  /** Fill and submit the create-pod form; resolves once the tile lands. */
  runnersCreatePod(
    input: { projectName: string; name: string; description?: string },
    options?: { expectFailure?: boolean }
  ): Promise<void>;
  /** Toggle one pod filter twice; true when no list fetch fired. */
  runnersPodFilterIsClientSide(podName: string): Promise<boolean>;
  /** Submit the open pod form without filling (validation assertions). */
  runnersSubmitPodForm(): Promise<void>;
  /** Whether the open pod form's cancel control is enabled. */
  runnersPodCancelEnabled(): Promise<boolean>;
  /** Create-form values (project trigger label, name, description). */
  runnersCreatePodForm(): Promise<{ project: string; name: string; description: string }>;
  /** Whether any pod modal (create/edit/confirm) is currently open. */
  runnersPodModalOpen(): Promise<boolean>;
  /** Pods section load-failure text, or null when pods render. */
  runnersPodsError(): Promise<string | null>;
  /** Whether the create submit currently shows its working state. */
  runnersCreatePodSubmitting(): Promise<boolean>;
  /** Open the edit modal for one pod through its tile menu. */
  runnersOpenEditPod(podName: string): Promise<void>;
  /** Edit-form values plus whether the default toggle is disabled. */
  runnersEditPodForm(): Promise<RunnersEditPodForm>;
  /** Apply an edit (rename/describe/promote); resolves once it lands. */
  runnersSavePodEdit(input: { name?: string; description?: string; makeDefault?: boolean }): Promise<void>;
  /** Save the edit form unchanged; true when no update call fired. */
  runnersUnchangedEditSkipsSave(): Promise<boolean>;
  /** Apply an edit and return the update payloads the app sent. */
  runnersSavePodEditCapturing(input: {
    name?: string;
    description?: string;
    makeDefault?: boolean;
  }): Promise<Record<string, unknown>[]>;
  /** Delete one pod through its tile menu plus confirmation. */
  runnersDeletePod(podName: string, options?: { expectFailure?: boolean }): Promise<void>;
  /** Most recent toast text, or null when no toast is showing. */
  runnersLastToast(): Promise<string | null>;
  /** Document title of the current page. */
  runnersPageTitle(): Promise<string>;
  /** In-page section headings of the overview (pods/runners/add). */
  runnersSectionHeadings(): Promise<string[]>;
  /** Current leaf of the project breadcrumb trail. */
  runnersBreadcrumbLeaf(): Promise<string | null>;
  /** Whether the not-authorized view is rendered. */
  runnersDeniedVisible(): Promise<boolean>;
  /** Whether the surrounding workspace/project chrome is rendered. */
  runnersChromePresent(): Promise<boolean>;
  /** Fail pod-list responses (load-failure assertions). */
  runnersFailPodsLoad(): Promise<void>;
  /** Release the pod-list failure above. */
  runnersReleasePodsFailure(): Promise<void>;
  /** Wait until the delete-pod confirm shows its in-flight state. */
  runnersWaitPodDeleteWorking(): Promise<void>;
  /** Wait until the delete-runner confirm shows its in-flight state. */
  runnersWaitRunnerDeleteWorking(): Promise<void>;
  /** Wait until the revoke-runner confirm shows its in-flight state. */
  runnersWaitRunnerRevokeWorking(): Promise<void>;

  // --- Intake detail (NEWFRONT-258, INT-019/024/026-029). Observed on the
  // --- running old app: the detail pane shows an inline title field, an
  // --- autosaving rich-text description, a reaction row, an attachments
  // --- section, and a properties panel whose state row is always disabled;
  // --- the header carries prev/next chevrons, arrow-key navigation with
  // --- wrap, and copy/open-work-item actions that move between a button
  // --- pair (resolved items) and an overflow menu (open items).

  /** Title text currently shown in the detail title field. */
  intakeDetailTitle(): Promise<string>;
  /** Whether the detail title field accepts edits. */
  intakeDetailTitleEditable(): Promise<boolean>;
  /** Replace the detail title; resolves once the save round-trip settles. */
  intakeDetailSetTitle(title: string): Promise<void>;
  /** Focus the detail title field without changing its text. */
  intakeDetailFocusTitle(): Promise<void>;
  /** Plain text currently shown in the detail description editor. */
  intakeDetailDescriptionText(): Promise<string>;
  /** Whether the detail description editor accepts edits. */
  intakeDetailDescriptionEditable(): Promise<boolean>;
  /** Replace the detail description; resolves once the autosave settles. */
  intakeDetailSetDescription(text: string): Promise<void>;
  /** Focus the detail description editor without changing its text. */
  intakeDetailFocusDescription(): Promise<void>;
  /** Reaction chips currently shown on the detail, each as the user reads it. */
  intakeDetailReactions(): Promise<string[]>;
  /** Add `emoji` to the detail through the reaction picker. */
  intakeDetailAddReaction(emoji: string): Promise<void>;
  /** Attachment file names currently listed on the detail. */
  intakeDetailAttachmentNames(): Promise<string[]>;
  /** Whether the named properties-panel row is currently disabled. */
  intakeDetailPropertyDisabled(label: IntakeDetailProperty): Promise<boolean>;
  /** Priority label currently shown in the properties panel. */
  intakeDetailPriorityText(): Promise<string>;
  /** Set the priority through the properties panel; resolves on save. */
  intakeDetailSetPriority(priority: string): Promise<void>;
  /** Assignee names currently shown in the properties panel. */
  intakeDetailAssigneeTexts(): Promise<string[]>;
  /** Add the named member as an assignee through the properties panel. */
  intakeDetailAddAssignee(displayName: string): Promise<void>;
  /** Due-date text currently shown in the properties panel. */
  intakeDetailDueDateText(): Promise<string>;
  /** Pick a due date `daysFromNow` days out through the properties panel. */
  intakeDetailSetDueDate(daysFromNow: number): Promise<void>;
  /** Label names currently shown in the properties panel. */
  intakeDetailLabelTexts(): Promise<string[]>;
  /** Attach the named project label through the properties panel. */
  intakeDetailAddLabel(labelName: string): Promise<void>;
  /** Copy the work-item link via whichever control the state offers. */
  intakeDetailCopyLink(): Promise<void>;
  /** Whether the header offers an open-work-item action. */
  intakeDetailOpenWorkItemVisible(): Promise<boolean>;
  /** Follow the header open-work-item action; resolves on navigation. */
  intakeDetailOpenWorkItem(): Promise<void>;
  /** The `inboxIssueId` URL param, or null when the URL carries none. */
  intakeDetailCurrentInboxIssueId(): Promise<string | null>;
  /** Full current URL including query params, as the address bar shows it. */
  intakeDetailCurrentUrl(): Promise<string>;
  /** Press the up/down arrow key with the detail focused. */
  intakeDetailPressArrow(direction: "up" | "down"): Promise<void>;
  /** Click the header prev/next chevron; resolves once the detail swaps. */
  intakeDetailClickChevron(direction: "prev" | "next"): Promise<void>;
  /** Open the intake URL `path` raw (no feed wait; stale ids redirect). */
  intakeDetailOpenRaw(path: string): Promise<void>;
  /** Whether the request list pane currently renders. */
  intakeDetailListVisible(): Promise<boolean>;
  /** Select the list row showing `name`; resolves once the detail swaps. */
  intakeDetailSelectListItem(name: string): Promise<void>;
  /** Whether the description-versions control currently renders. */
  intakeDetailVersionsVisible(): Promise<boolean>;
  /** Open the description-versions list. */
  intakeDetailOpenVersions(): Promise<void>;
  /** Version entries currently listed, each as the user reads it. */
  intakeDetailVersionTexts(): Promise<string[]>;
  /** Open version `index` (0 = latest) in the version viewer. */
  intakeDetailOpenVersion(index: number): Promise<void>;
  /** Plain text of the open version viewer, or null when closed. */
  intakeDetailVersionModalText(): Promise<string | null>;
  /** Restore the open version into the editor; resolves once applied. */
  intakeDetailRestoreVersion(): Promise<void>;
  /** Close the version viewer without restoring. */
  intakeDetailCloseVersionModal(): Promise<void>;
  // --- Cycles empty states, gone-away, create, validation (NEWFRONT-250,
  // --- CYC-009..016). Observed on apps/web: the live cycles list with its
  // --- cause-specific no-match view, the feature-off and first-run empty
  // --- views, the list skeleton, the detail gone-away view, and the
  // --- create dialog with client-side validation plus the date-overlap
  // --- check. Appended; existing entries above are untouched per the
  // --- shared contract.
  // ---------------------------------------------------------------------------

  /** Open the project's live cycles list; ends with the list settled. */
  cyclesEmptyOpenList(workspaceSlug: string, projectId: string): Promise<void>;
  /** Names of the currently visible live cycle rows, in render order. */
  cyclesEmptyVisibleNames(): Promise<string[]>;
  /** Whether the list skeleton loader currently shows. */
  cyclesEmptySkeletonVisible(): Promise<boolean>;
  /** No-match empty-view heading, or null when the list is not in no-match. */
  cyclesEmptyNoMatchHeading(): Promise<string | null>;
  /** No-match hint paragraph, or null when the list is not in no-match. */
  cyclesEmptyNoMatchHint(): Promise<string | null>;
  /** Open the live-list search box; ends with its input focused. */
  cyclesEmptySearchOpen(): Promise<void>;
  /** Fill the live-list search box with text (filters live). */
  cyclesEmptySearchFill(text: string): Promise<void>;
  /** Clear the live-list search box; ends with rows or an empty view. */
  cyclesEmptySearchClear(): Promise<void>;
  /** Open the live-list filters menu; ends with its sections mounted. */
  cyclesEmptyFiltersOpen(): Promise<void>;
  /** Close the live-list filters menu. */
  cyclesEmptyFiltersClose(): Promise<void>;
  /** Section names of the open filters menu. */
  cyclesEmptyFilterSections(): Promise<string[]>;
  /** Option names across the open filters menu. */
  cyclesEmptyFilterOptionNames(): Promise<string[]>;
  /** Pick one option of one filter section (applies live). */
  cyclesEmptyFilterPick(section: string, optionName: string): Promise<void>;
  /** Clear every applied filter via the clear-all action. */
  cyclesEmptyFiltersClearAll(): Promise<void>;
  /**
   * Enter the live list while its fetch is held back, reporting whether
   * the skeleton showed mid-flight. Releases the hold and ends settled
   * (rows or an empty view).
   */
  cyclesEmptySkeletonShownOnSlowFetch(workspaceSlug: string, projectId: string): Promise<boolean>;
  /** Whether the cycles-feature-off empty view renders. */
  cyclesEmptyFeatureOffVisible(): Promise<boolean>;
  /** Whether the feature-off view's management shortcut is disabled. */
  cyclesEmptyFeatureOffActionDisabled(): Promise<boolean>;
  /** Activate the feature-off view's management shortcut. */
  cyclesEmptyFeatureOffActionOpen(): Promise<void>;
  /** Whether the first-run (zero cycles) empty view renders. */
  cyclesEmptyZeroStateVisible(): Promise<boolean>;
  /** Whether the zero-state creation shortcut is disabled. */
  cyclesEmptyZeroStateCreateDisabled(): Promise<boolean>;
  /** Activate the zero-state creation shortcut. */
  cyclesEmptyZeroStateCreateOpen(): Promise<void>;
  /** Open one cycle's detail route directly; ends with content or gone-away. */
  cyclesEmptyOpenDetail(workspaceSlug: string, projectId: string, cycleId: string): Promise<void>;
  /** Whether the detail gone-away view renders. */
  cyclesEmptyGoneAwayVisible(): Promise<boolean>;
  /** Activate the gone-away view's back-to-list action. */
  cyclesEmptyGoneAwayBack(): Promise<void>;
  /** Open the create dialog from the list header add action. */
  cyclesCreateOpenFromHeader(): Promise<void>;
  /** Whether the cycle create dialog is open. */
  cyclesCreateDialogOpen(): Promise<boolean>;
  /** Close the create dialog via its cancel action. */
  cyclesCreateDialogCancel(): Promise<void>;
  /** Fill the dialog title field (replaces any text). */
  cyclesCreateFillTitle(title: string): Promise<void>;
  /** Fill the dialog description field (replaces any text). */
  cyclesCreateFillDescription(text: string): Promise<void>;
  /** Inline title error text, or null when no title error shows. */
  cyclesCreateTitleError(): Promise<string | null>;
  /** Pick a start/end date range in the dialog date picker. */
  cyclesCreatePickDateRange(startISO: string, endISO: string): Promise<void>;
  /** Whether yesterday's day cell is disabled in the dialog start picker. */
  cyclesCreatePastDayDisabled(): Promise<boolean>;
  /** Target another project via the dialog project picker. */
  cyclesCreatePickProject(projectName: string): Promise<void>;
  /** Submit the dialog; ends with it closed or showing an error. */
  cyclesCreateSubmit(): Promise<void>;

  // --- NEWFRONT-253 (cycles detail/sidebar, CYC-032-039). Appended;
  // --- existing methods above are untouched per the shared driver
  // --- contract. Layout switching itself reuses the layouts* methods
  // --- (Issues area); these cover the cycle-detail host: header,
  // --- crumbs, sidebar, dates, and progress.

  /** Cycle name shown in the detail sidebar, or null when the panel is absent. */
  cyclesDetailSidebarName(): Promise<string | null>;
  /** Click the header sidebar toggle; resolves once the click lands. */
  cyclesToggleSidebarViaHeader(): Promise<void>;
  /** Click the sidebar's own close control; resolves once the click lands. */
  cyclesCloseSidebarViaPanel(): Promise<void>;
  /** Stored sidebar-collapsed flag for this browser (persistence proof). */
  cyclesSidebarCollapsedStored(): Promise<boolean>;
  /** Header count-badge text, or null when no badge renders. */
  cyclesHeaderBadgeText(): Promise<string | null>;
  /** Whether the header work-item creation button is visible. */
  cyclesHeaderAddVisible(): Promise<boolean>;
  /** Whether the header Analytics shortcut is visible. */
  cyclesHeaderAnalyticsVisible(): Promise<boolean>;
  /** Open the Analytics dialog; resolves once it renders. */
  cyclesOpenAnalytics(): Promise<void>;
  /** Whether the Analytics dialog is currently rendered. */
  cyclesAnalyticsDialogVisible(): Promise<boolean>;
  /** Dismiss the Analytics dialog; resolves once it detaches. */
  cyclesDismissAnalytics(): Promise<void>;
  /** Open the header cycle switcher; resolves once its options render. */
  cyclesSwitcherOpen(currentName: string): Promise<void>;
  /** Cycle names the open switcher offers, in display order. */
  cyclesSwitcherOptions(): Promise<string[]>;
  /** Pick a cycle in the open switcher; resolves once the header shows it. */
  cyclesSwitcherPick(name: string): Promise<void>;
  /** Follow the Cycles crumb; the caller polls the address. */
  cyclesClickCyclesCrumb(): Promise<void>;
  /** Follow the project crumb; the caller polls the address. */
  cyclesClickProjectCrumb(projectName: string): Promise<void>;
  /** Whether the small-screen trail back control is visible. */
  cyclesMobileBackVisible(): Promise<boolean>;
  /** Activate the small-screen trail back control. */
  cyclesMobileBack(): Promise<void>;
  /** Sidebar date control text (the rendered range). */
  cyclesSidebarDateText(): Promise<string>;
  /** Whether the sidebar date control refuses edits. */
  cyclesSidebarDateDisabled(): Promise<boolean>;
  /** Open the sidebar date picker; resolves once the calendar renders. */
  cyclesSidebarOpenDatePicker(): Promise<void>;
  /** Pick one calendar day by its accessible date label. */
  cyclesPickDateDay(label: string): Promise<void>;
  /** Dismiss the sidebar date picker; resolves once it detaches. */
  cyclesDismissDatePicker(): Promise<void>;
  /** Sidebar description text, or null when none renders. */
  cyclesSidebarDescription(): Promise<string | null>;
  /** Sidebar lead facts (avatar is the image source, else the fallback initial). */
  cyclesSidebarLead(): Promise<{ name: string; avatar: string }>;
  /** Sidebar Work-items count text (e.g. "0/3"). */
  cyclesProgressWorkItemsText(): Promise<string>;
  /** Sidebar Points count text, or null when the row is absent. */
  cyclesProgressPointsText(): Promise<string | null>;
  /** Whether the sidebar progress chart renders. */
  cyclesProgressChartVisible(): Promise<boolean>;
  /** Current measure label, or null when the dropdown is absent. */
  cyclesProgressMeasureValue(): Promise<string | null>;
  /** Measure options in display order (absent dropdown reads empty). */
  cyclesProgressMeasureOptions(): Promise<string[]>;
  /** Pick a measure; resolves once the dropdown shows it. */
  cyclesProgressPickMeasure(label: string): Promise<void>;
  /** Open a progress breakdown tab; resolves once it selects. */
  cyclesProgressStatsTab(tab: "States" | "Assignees" | "Labels"): Promise<void>;
  /** Rows of the selected breakdown tab (title, percent, total). */
  cyclesProgressStatsRows(): Promise<CyclesProgressStatRow[]>;
  /** Empty-progress notice text, or null when breakdowns render. */
  cyclesProgressEmptyText(): Promise<string | null>;
  /** Click a breakdown entry; the caller polls the narrowed list. */
  cyclesProgressEntryClick(title: string): Promise<void>;
  /** Open Analytics from the small-screen bar; resolves once the dialog renders. */
  cyclesMobileOpenAnalytics(): Promise<void>;
  /**
   * Switch layouts through the desktop header switcher; resolves once the
   * header marker shows the layout. Marker wait, not a container wait:
   * the shared container heuristics misread this page (the sidebar's
   * own "Work items" label trips the board/spreadsheet detection).
   */
  cyclesSwitchLayout(layout: LayoutsLayoutKey): Promise<void>;
  /** Layout keys the small-screen layout menu offers, in display order. */
  cyclesMobileOfferedLayouts(): Promise<LayoutsLayoutKey[]>;
  /** Switch layout through the small-screen menu; resolves once applied. */
  cyclesMobileSwitchTo(layout: LayoutsLayoutKey): Promise<void>;

  // --- Cycles archive/menu/transfer acceptance (NEWFRONT-252, CYC-025-031
  // --- plus CYC-046). The shared archived/transfer/palette surfaces reuse
  // --- the settled archivesCycles*, cycleTransfer* and palette* methods;
  // --- only genuinely new reads live here under cycles* names.

  /** Favorite-marker state of the live-list row `name`: starred, unstarred, or absent (no marker renders). */
  cyclesRowFavoriteState(name: string): Promise<"starred" | "unstarred" | "absent">;
  /** Click the favorite marker on the live-list row `name`. */
  cyclesRowFavoriteToggle(name: string): Promise<void>;
  /** Hold favorite POST/DELETE writes for `delayMs` so progress toasts stay observable. */
  cyclesDelayFavoriteWrites(delayMs: number): Promise<void>;
  /** Remove favorite-write delays installed by cyclesDelayFavoriteWrites. */
  cyclesClearFavoriteDelays(): Promise<void>;
  /** Whether the row `name` currently shows its action-menu trigger. */
  cyclesRowMenuTriggerVisible(name: string): Promise<boolean>;
  /** Hover the live-list row `name` (reveals hover-only controls). */
  cyclesRowHover(name: string): Promise<void>;
  /** Open the row menu for `name`, pick the new-tab entry, and resolve with the opened page's URL. */
  cyclesRowMenuOpenNewTab(name: string): Promise<string>;
  /** Fill the open transfer dialog's cycle search box. */
  cyclesTransferSearchFill(text: string): Promise<void>;
  /** The open transfer dialog's zero-state text, or null when target options render. */
  cyclesTransferEmptyText(): Promise<string | null>;
  /** The archived-cycles zero-state supporting text, or null when rows render. */
  cyclesArchivedEmptyDetail(): Promise<string | null>;
  /** Grant clipboard read/write so a later copy action lands (call before copying). */
  cyclesGrantClipboardAccess(): Promise<void>;

  // --- Modules row actions (NEWFRONT-308, MOD-013-019 + MOD-021).
  // --- Appended; existing methods above are untouched per the shared
  // --- driver contract.
  /** Open the modules list; resolves once rows or the empty state settle. */
  modulesOpenList(workspaceSlug: string, projectId: string, cookies: ParityBrowserCookie[]): Promise<void>;
  /** Wait until the named module's list row renders. */
  modulesAwaitRow(name: string): Promise<void>;
  /** Current page URL (peek-param and deep-link assertions). */
  modulesCurrentUrl(): Promise<string>;
  /** Switch list/board/timeline through the layout control; resolves once applied. */
  modulesSetLayout(layout: "list" | "board" | "timeline"): Promise<void>;
  /** Pick an ordering through the order control; resolves once applied. */
  modulesSetSort(label: string): Promise<void>;
  /** Ordering the control currently shows. */
  modulesSortLabel(): Promise<string>;
  /** Merged date label (or start/end placeholders) on the named row. */
  modulesRowDateText(name: string): Promise<string>;
  /** Whether the named row's date control is disabled. */
  modulesRowDateDisabled(name: string): Promise<boolean>;
  /** Open the range calendar from the named row's date control. */
  modulesRowOpenDates(name: string): Promise<void>;
  /** Whether the range calendar popup is currently open. */
  modulesCalendarVisible(): Promise<boolean>;
  /** Click day-of-month `day` in the open range calendar. */
  modulesCalendarPickDay(day: number): Promise<void>;
  /** Show `monthLabel`/`yearLabel` in the open range calendar; verified before return. */
  modulesCalendarGotoMonth(monthLabel: string, yearLabel: string): Promise<void>;
  /** Split from/to date text on the named board card. */
  modulesCardDateText(name: string): Promise<string>;
  /** Whether the named card's date control is disabled. */
  modulesCardDateDisabled(name: string): Promise<boolean>;
  /** Open the range calendar from the named card's date control. */
  modulesCardOpenDates(name: string): Promise<void>;
  /** Most recent toast still visible, or null when none shows. */
  modulesLastToast(): Promise<ModulesToast | null>;
  /** Wait until a toast carrying `text` shows; fails on timeout. */
  modulesAwaitToast(text: string): Promise<void>;
  /** Status pill label on the named row. */
  modulesPillText(name: string): Promise<string>;
  /** Status pill text color on the named row (hex). */
  modulesPillColor(name: string): Promise<string>;
  /** Whether the named row's status control is disabled. */
  modulesStatusDisabled(name: string): Promise<boolean>;
  /** Open the named row's status options. */
  modulesOpenStatus(name: string): Promise<void>;
  /** Status option labels in display order. */
  modulesStatusOptions(): Promise<string[]>;
  /** Pick a status option; resolves once the pill shows it. */
  modulesPickStatus(label: string): Promise<void>;
  /** Lead slot on the named row: avatar shown or no-lead marker. */
  modulesLeadState(name: string): Promise<"avatar" | "none">;
  /** No-lead explanation tooltip text, or null when a lead shows. */
  modulesLeadTooltip(name: string): Promise<string | null>;
  /** Whether the named row offers the favorite star. */
  modulesStarVisible(name: string): Promise<boolean>;
  /** Whether the named row's star reads selected. */
  modulesStarSelected(name: string): Promise<boolean>;
  /** Toggle the named row's star; resolves once the toast shows. */
  modulesToggleStar(name: string): Promise<void>;
  /** Whether the sidebar favorites section reads expanded. */
  modulesFavoritesExpanded(): Promise<boolean>;
  /** Open the named row's overflow menu. */
  modulesOpenRowMenu(name: string): Promise<void>;
  /** Overflow menu entries in display order with gating state. */
  modulesMenuItems(): Promise<ModulesMenuEntry[]>;
  /** Choose an overflow menu entry; resolves once the menu closes. */
  modulesMenuChoose(title: string): Promise<void>;
  /** Choose "Open in new tab"; resolves with the popup URL and title once the title shows `titlePart`. */
  modulesMenuChooseOpenTab(titlePart: string): Promise<{ url: string; title: string }>;
  /** Whether a module dialog (edit/archive/delete) is currently open. */
  modulesDialogVisible(kind: "edit" | "archive" | "delete"): Promise<boolean>;
  /** Heading of the open module dialog, or null when none shows. */
  modulesDialogHeading(): Promise<string | null>;
  /** Read the clipboard text (copy-link assertions). */
  modulesReadClipboard(): Promise<string>;
  /** Open the module detail page; resolves once it renders. */
  modulesOpenDetail(
    workspaceSlug: string,
    projectId: string,
    moduleId: string,
    cookies: ParityBrowserCookie[]
  ): Promise<void>;
  /** Open the detail header's overflow menu. */
  modulesDetailOpenMenu(): Promise<void>;
  /** Click the info control on the named row (peek toggle). */
  modulesTogglePeek(name: string): Promise<void>;
  /** Whether the peek panel is currently open. */
  modulesPeekVisible(): Promise<boolean>;
  /** Module name the peek panel shows, or null when closed. */
  modulesPeekName(): Promise<string | null>;
  /** Close the peek panel; resolves once the param clears. */
  modulesClosePeek(): Promise<void>;
  /** Board grid column count at the current viewport. */
  modulesBoardColumnCount(): Promise<number>;
  /** Click the named row itself (archived rows open peek instead). */
  modulesClickRow(name: string): Promise<void>;
  /** Timeline sidebar rows in order: name, detail href, status glyph present. */
  modulesTimelineRows(): Promise<ModulesTimelineRow[]>;
  /** Timeline bar background color for the named module (hex). */
  modulesTimelineBarColor(name: string): Promise<string>;
  /** Hover the named module's timeline bar so its tooltip shows. */
  modulesTimelineHoverBar(name: string): Promise<void>;
  /** Timeline bar tooltip text (name + date summary), or null when hidden. */
  modulesTimelineTooltip(): Promise<string | null>;
  /** Whether the named module renders a dated timeline bar. */
  modulesTimelineBarExists(name: string): Promise<boolean>;
  /** Drag the named bar by dayDelta days; resolves once it settles. */
  modulesTimelineDragBar(name: string, dayDelta: number): Promise<void>;
  /** Resize the named bar from one end; resolves once it settles. */
  modulesTimelineResizeBar(name: string, side: "left" | "right", dayDelta: number): Promise<void>;
  /** Sidebar row names in timeline order. */
  modulesTimelineOrder(): Promise<string[]>;
  /** Drag one timeline sidebar row above another; resolves once it settles. */
  modulesTimelineDragRowBefore(sourceName: string, targetName: string): Promise<void>;
  /** Attempt a row reorder the timeline may refuse; fixed wait, no settle. */
  modulesTimelineAttemptRowBefore(sourceName: string, targetName: string): Promise<void>;
  /** Attempt a bar move the timeline may refuse; fixed wait, no settle. */
  modulesTimelineAttemptBarMove(name: string, dayDelta: number): Promise<void>;

  // -------------------------------------------------------------------------
  // Pages realtime collaboration + live server (NEWFRONT-319, PAGE-065-071).
  // Socket captures are two-phase: the first call installs the listener, the
  // later call reads what arrived — so scenarios call the reader once before
  // the triggering step and again to assert. Close codes come from the
  // browser's own socket-close frames (1006 when the drop carried no frame).
  // -------------------------------------------------------------------------

  /** Open a project page detail; resolves once the body editor mounts. */
  pagesLiveOpenPageDetail(workspaceSlug: string, projectId: string, pageId: string): Promise<void>;
  /** Visible sync badge label, or null when the badge is hidden (synced). */
  pagesLiveSyncBadge(): Promise<string | null>;
  /** Wait until the sync badge hides (the server confirmed synced). */
  pagesLiveWaitSynced(): Promise<void>;
  /** Body editor plain text. */
  pagesLiveBodyText(): Promise<string>;
  /** Title editor plain text. */
  pagesLiveTitleText(): Promise<string>;
  /** Click the body editor and type `text`. */
  pagesLiveTypeInBody(text: string): Promise<void>;
  /** Replace the title editor contents with `text`. */
  pagesLiveSetTitle(text: string): Promise<void>;
  /** Collaboration socket URLs this page opened (query carries doc context). */
  pagesLiveSocketUrls(): Promise<string[]>;
  /** Handshake identity payloads (user id, name, cursor color) the page sent. */
  pagesLiveSocketAuthTokens(): Promise<string[]>;
  /** Close codes observed on collaboration sockets, oldest first. */
  pagesLiveSocketCloseEvents(): Promise<PagesLiveSocketClose[]>;
  /** Received socket frames carrying realtime event payloads (error/force-close), oldest first. */
  pagesLiveSocketReceivedFrames(): Promise<string[]>;
  /** Toggle browser offline emulation for this page's context. */
  pagesLiveSetOffline(offline: boolean): Promise<void>;
  /** Abort the collaboration socket upgrade (socket down, REST up). */
  pagesLiveBlockCollaborationSocket(): Promise<void>;
  /** Undo pagesLiveBlockCollaborationSocket. */
  pagesLiveUnblockCollaborationSocket(): Promise<void>;
  /** Binary description fetches this page issued (fallback proof). */
  pagesLiveDescriptionFetchCount(): Promise<number>;
  /** Whether the sync loader skeleton is currently showing. */
  pagesLiveSyncLoaderVisible(): Promise<boolean>;

  // --- Pages prefs, key map, assistant, cloud stubs, desktop, negatives
  // --- (NEWFRONT-320, PAGE-072-073, PAGE-075-078, PAGE-081-083).
  // --- Distinctive pages* names; siblings own pagesPane* (317) and the
  // --- other PAGE ranges. Nav/editor primitives live under pagesPrefs*
  // --- and are reused by every scenario in prefs-cloud-misc.spec.ts.

  /** Open the project pages list; resolves once rows or an empty state render. */
  pagesPrefsOpenList(workspaceSlug: string, projectId: string): Promise<void>;
  /** Open one page's detail; resolves once the live editor renders. */
  pagesPrefsOpenDetail(workspaceSlug: string, projectId: string, pageId: string): Promise<void>;
  /** Reload the current page route; resolves once list or detail renders again. */
  pagesPrefsReload(): Promise<void>;
  /** Inner HTML of the live editor body. */
  pagesPrefsEditorHtml(): Promise<string>;
  /** Visible text of the live editor body. */
  pagesPrefsEditorText(): Promise<string>;
  /** Click at the end of the live editor body. */
  pagesPrefsEditorClickEnd(): Promise<void>;
  /** Type text into the live editor (caller focuses first). */
  pagesPrefsEditorType(text: string): Promise<void>;
  /** Select-all in the live editor and delete, leaving an empty document. */
  pagesPrefsEditorClear(): Promise<void>;
  /** Select all text in the live editor (keeps the selection). */
  pagesPrefsEditorSelectAll(): Promise<void>;
  /** Current value of the detail title control. */
  pagesPrefsTitleValue(): Promise<string>;
  /** Fill the detail title control. */
  pagesPrefsTitleFill(text: string): Promise<void>;
  /** Focus the detail title control. */
  pagesPrefsTitleFocus(): Promise<void>;
  /** Press Enter while the title control is focused. */
  pagesPrefsTitlePressEnter(): Promise<void>;
  /** Class attribute of the title control (carries the type-size class). */
  pagesPrefsTitleClass(): Promise<string>;
  /** Which region holds DOM focus: title, editor body, or elsewhere. */
  pagesPrefsFocusedRegion(): Promise<"title" | "body" | "other">;
  /** Open the detail header options menu; resolves once its items render. */
  pagesPrefsOptionsOpen(): Promise<void>;
  /** Visible item labels of the open options menu, in display order. */
  pagesPrefsOptionsItems(): Promise<string[]>;
  /** Click the options-menu item `label`; resolves once the menu closes. */
  pagesPrefsOptionsToggle(label: string): Promise<void>;
  /** Display prefs as the app persisted them locally, or null when absent. */
  pagesPrefsLocalConfig(): Promise<PagesPrefsLocalConfig | null>;
  /** Overwrite the locally persisted display prefs (caller reloads after). */
  pagesPrefsSeedLocalConfig(config: PagesPrefsLocalConfig): Promise<void>;
  /** Whether the sticky toolbar container is currently expanded. */
  pagesPrefsToolbarVisible(): Promise<boolean>;
  /** Whether the toolbar content carries the full-width layout marker. */
  pagesPrefsToolbarWide(): Promise<boolean>;
  /** Computed max-width of the detail header block (full-width reflow read). */
  pagesPrefsCanvasMaxWidth(): Promise<string>;
  /** Class attribute of the live editor container (carries font markers). */
  pagesPrefsEditorClass(): Promise<string>;
  /** Start counting page-endpoint writes (POST/PATCH/PUT/DELETE). */
  pagesPrefsSpyPageWrites(): Promise<void>;
  /** Page-endpoint writes observed since pagesPrefsSpyPageWrites. */
  pagesPrefsPageWriteCount(): Promise<number>;
  /** Request bodies of the writes observed since pagesPrefsSpyPageWrites. */
  pagesPrefsPageWritePayloads(): Promise<string[]>;
  /** Toolbar buttons with the tooltip each advertises, in display order. */
  pagesKeysToolbarTooltips(): Promise<PagesKeysToolbarTooltip[]>;
  /** Press one Playwright key chord (e.g. "Control+b") into the focused editor. */
  pagesKeysPress(chord: string): Promise<void>;
  /** Open the collapsed list search control. */
  pagesKeysSearchOpen(): Promise<void>;
  /** Fill the list search box. */
  pagesKeysSearchFill(text: string): Promise<void>;
  /** Current value of the list search box. */
  pagesKeysSearchValue(): Promise<string>;
  /** Press Escape while the list search box is focused. */
  pagesKeysSearchPressEscape(): Promise<void>;
  /** Whether the list search box is currently expanded. */
  pagesKeysSearchExpanded(): Promise<boolean>;
  /** List row titles currently rendered, in display order. */
  pagesKeysListRowTitles(): Promise<string[]>;
  /** Whether any global shortcut reference or cheat sheet renders. */
  pagesKeysShortcutReferenceVisible(): Promise<boolean>;
  /** Whether the editor side menu shows its AI handle. */
  pagesAssistAiHandleVisible(): Promise<boolean>;
  /** Whether any AI assistant menu or panel renders on the detail view. */
  pagesAssistAiMenuVisible(): Promise<boolean>;
  /** Accessible names of the detail header action controls, in order. */
  pagesCloudHeaderActionNames(): Promise<string[]>;
  /** Accessible names of the toolbar-end cluster buttons, in order. */
  pagesCloudToolbarEndNames(): Promise<string[]>;
  /** Open the navigation pane; resolves once its tabs render. */
  pagesCloudPaneOpen(): Promise<void>;
  /** Navigation pane tab labels in display order (empty when closed). */
  pagesCloudPaneTabNames(): Promise<string[]>;
  /** Top-level slash-menu entries, in display order (closes the menu after). */
  pagesCloudSlashEntries(): Promise<string[]>;
  /** Whether the current UA reads as mobile under the app's own rule. */
  pagesDeskIsMobile(): Promise<boolean>;
  /** Platform label under the app's own UA rule (Windows/MacOS/Linux/Unknown). */
  pagesDeskPlatform(): Promise<string>;
  /** List visibility tab labels in display order. */
  pagesNegListTabNames(): Promise<string[]>;
  /** Row menu item labels for the row `title`, in display order. */
  pagesNegRowMenuItems(title: string): Promise<string[]>;
  /** Whether any cover affordance renders in list or detail scope. */
  pagesNegCoverPresent(): Promise<boolean>;
  /** Whether any merge-conflict dialog renders. */
  pagesNegConflictDialogVisible(): Promise<boolean>;
  /** Whether the detail header shows the page mark control. */
  pagesNegMarkPresent(): Promise<boolean>;

  // --- Desktop bare sign-in, deep links, onboarding seam (NEWFRONT-208,
  // --- DESK-011–015, DESK-027). Appended; existing methods above are
  // --- untouched per the shared driver contract.
  /** True when the entry shows the marketing frame around the sign-in card. */
  deskSigninMarketingFrameVisible(): Promise<boolean>;
  /** Progress reading ("current/total") of the onboarding header, or null. */
  deskSigninOnboardingProgress(): Promise<string | null>;
  /** External footer links of the open product-updates dialog. */
  deskSigninUpdateDialogLinks(): Promise<DeskSigninUpdateLink[]>;
  /** Click an updates-dialog footer link; resolves with the popup URL or null. */
  deskSigninOpenUpdateDialogLink(label: string): Promise<string | null>;

  // --- Intake cross-cutting (NEWFRONT-260, INT-018/030/031/032/034).
  // --- Appended; existing methods above are untouched per the shared driver
  // --- contract. Minimal intake navigation of our own (sibling list blocks
  // --- are unmerged, so small overlap is expected): open the screen and one
  // --- request's detail, read list titles, switch tabs, open the create
  // --- dialog; plus narrow-viewport, no-live-push, and edition reads.
  /** Open the intake screen of a project; settles once the tab bar or the feature gate shows. */
  intakeXOpen(workspaceSlug: string, projectId: string): Promise<void>;
  /** Open one request's detail through its deep link; settles once the detail renders or the list redirect lands. */
  intakeXOpenDetail(workspaceSlug: string, projectId: string, issueId: string): Promise<void>;
  /** Titles of the rendered list rows, in display order. */
  intakeXListTitles(): Promise<string[]>;
  /** Which tab is currently highlighted. */
  intakeXActiveTab(): Promise<IntakeXTab>;
  /** Click `tab` and settle once the URL and the list reflect it. */
  intakeXClickTab(tab: IntakeXTab): Promise<void>;
  /** Title the open detail shows, or null when no detail is open. */
  intakeXDetailTitle(): Promise<string | null>;
  /** Open the create dialog from the intake screen; settles once the dialog shows. */
  intakeXOpenCreate(): Promise<void>;
  /** Fill the open create dialog's title field. */
  intakeXCreateFillTitle(title: string): Promise<void>;
  /** Whether duplicate-detection UI shows inside the open create dialog. */
  intakeXCreateDuplicateDetectionVisible(): Promise<boolean>;
  /** Whether duplicate-detection UI shows inside the open detail. */
  intakeXDetailDuplicateDetectionVisible(): Promise<boolean>;
  /** Resize the viewport (narrow widths render the mobile intake chrome). */
  intakeXSetViewport(width: number, height: number): Promise<void>;
  /** Whether the mobile action header currently shows. */
  intakeXMobileHeaderVisible(): Promise<boolean>;
  /** Whether the request list pane is currently on screen. */
  intakeXListPaneVisible(): Promise<boolean>;
  /** Toggle the list pane through the mobile sidebar control. */
  intakeXToggleMobileSidebar(): Promise<void>;
  /** Labels of the mobile action header's overflow menu entries (opens the menu, reads it, closes it). */
  intakeXMobileMenuItems(): Promise<string[]>;
  /** Start recording intake API traffic (list/detail reads). */
  intakeXBeginApiSpy(): Promise<void>;
  /** Intake API requests captured since the spy began, in order. */
  intakeXApiRequests(): Promise<IntakeXApiRequest[]>;
  /** Move window focus away and back (the detail fetch must not revalidate on focus). */
  intakeXRefocusPage(): Promise<void>;
  /** Whether any request-source indicator shows in the list rows. */
  intakeXSourceIndicatorVisible(): Promise<boolean>;

  // --- Intake triage actions (NEWFRONT-259, INT-020/021/022/023/025).
  // --- Appended; existing methods above are untouched per the shared
  // --- driver contract. The intake detail header offers Accept/Decline
  // --- buttons plus an overflow menu (snooze, duplicate, copy link,
  // --- delete) while the request is open; each action confirms through
  // --- its own dialog and then moves focus to a neighbour request.

  /** Whether the header's Accept control is currently offered. */
  intakeTriageAcceptVisible(): Promise<boolean>;
  /** Whether the header's Decline control is currently offered. */
  intakeTriageDeclineVisible(): Promise<boolean>;
  /** Click the header's Accept control; opens the add-to-project dialog for admins. */
  intakeTriageClickAccept(): Promise<void>;
  /** Whether the add-to-project dialog is currently open. */
  intakeTriageAcceptDialogVisible(): Promise<boolean>;
  /** Add-to-project dialog heading plus primary-button label. */
  intakeTriageAcceptDialogText(): Promise<{ heading: string; confirm: string }>;
  /** Confirm the add-to-project dialog; resolves once the write lands. */
  intakeTriageAcceptDialogConfirm(): Promise<void>;
  /** Cancel the add-to-project dialog; resolves once it closes. */
  intakeTriageAcceptDialogCancel(): Promise<void>;
  /** Click the header's Decline control; opens the decline dialog for admins. */
  intakeTriageClickDecline(): Promise<void>;
  /** Whether the decline confirmation dialog is currently open. */
  intakeTriageDeclineDialogVisible(): Promise<boolean>;
  /** Decline dialog heading plus body text (warns the action cannot be undone). */
  intakeTriageDeclineDialogText(): Promise<{ heading: string; body: string }>;
  /** Confirm the decline dialog; resolves once the write lands. */
  intakeTriageDeclineConfirm(): Promise<void>;
  /** Cancel the decline dialog; resolves once it closes. */
  intakeTriageDeclineCancel(): Promise<void>;
  /** Whether the header's overflow (more-actions) trigger is currently offered. */
  intakeTriageOverflowVisible(): Promise<boolean>;
  /** Open the header's overflow menu; resolves once its items render. */
  intakeTriageOpenOverflow(): Promise<void>;
  /** Overflow menu item labels in display order. */
  intakeTriageOverflowOptions(): Promise<string[]>;
  /** Choose the overflow menu's Snooze entry; opens the snooze date dialog. */
  intakeTriageClickSnooze(): Promise<void>;
  /** Whether the snooze date dialog is currently open. */
  intakeTriageSnoozeDialogVisible(): Promise<boolean>;
  /** Whether the snooze calendar disables every past day. */
  intakeTriageSnoozePastDisabled(): Promise<boolean>;
  /** Pick the day `daysAhead` days in the future in the snooze calendar. */
  intakeTriageSnoozePickFuture(daysAhead: number): Promise<void>;
  /** Confirm the snooze dialog; resolves once the write lands. */
  intakeTriageSnoozeConfirm(): Promise<void>;
  /** Cancel the snooze dialog; resolves once it closes. */
  intakeTriageSnoozeCancel(): Promise<void>;
  /** Header status chip text, or null when no chip renders. */
  intakeTriageStatusChipText(): Promise<string | null>;
  /** Choose the overflow menu's Un-snooze entry; resolves once the write lands. */
  intakeTriageClickUnsnooze(): Promise<void>;
  /** Choose the overflow menu's Mark-as-duplicate entry; opens the target picker. */
  intakeTriageClickMarkDuplicate(): Promise<void>;
  /** Whether the duplicate-target picker is currently open. */
  intakeTriageDuplicateDialogVisible(): Promise<boolean>;
  /** Search the duplicate-target picker; resolves once results settle. */
  intakeTriageDuplicateSearch(query: string): Promise<void>;
  /** Duplicate-picker candidate labels (identifier plus title) in display order. */
  intakeTriageDuplicateOptions(): Promise<string[]>;
  /** Pick the candidate whose label contains `name`; resolves once the write lands. */
  intakeTriageDuplicatePick(name: string): Promise<void>;
  /** Properties panel's duplicate-of reference text, or null when absent. */
  intakeTriageDuplicateOfText(): Promise<string | null>;
  /** Choose the overflow menu's Delete entry; opens the delete dialog. */
  intakeTriageClickDelete(): Promise<void>;
  /** Whether the delete confirmation dialog is currently open. */
  intakeTriageDeleteDialogVisible(): Promise<boolean>;
  /** Delete dialog heading plus body text. */
  intakeTriageDeleteDialogText(): Promise<{ heading: string; body: string }>;
  /** Confirm the delete dialog; resolves once the write lands. */
  intakeTriageDeleteConfirm(): Promise<void>;
  /** Cancel the delete dialog; resolves once it closes. */
  intakeTriageDeleteCancel(): Promise<void>;
  /** Current `inboxIssueId` URL param, or null when the URL carries none. */
  intakeTriageCurrentIssueId(): Promise<string | null>;
  /** Current `currentTab` URL param, or null when the URL carries none. */
  intakeTriageCurrentTab(): Promise<string | null>;
  /** Open one intake request on an explicit tab; waits for its detail. */
  intakeTriageOpenIssueOnTab(workspaceSlug: string, projectId: string, issueId: string, tab: string): Promise<void>;
  /**
   * Open the list's filter panel (minimal navigation aid: snoozed rows
   * leave the default open list, so revisiting one needs the status
   * filter; full filter behavior belongs to NEWFRONT-256).
   */
  intakeTriageFilterOpen(): Promise<void>;
  /** Toggle one work-item-status option; resolves once the list refetches. */
  intakeTriageFilterToggleStatus(name: string): Promise<void>;
  /** Close the list's filter panel. */
  intakeTriageFilterClose(): Promise<void>;
  /** Open a listed request through its sidebar link; resolves once its detail shows. */
  intakeTriageListOpenIssue(name: string): Promise<void>;

  // --- Estimates screen shell (NEWFRONT-321, EST-001-003, EST-026-028).
  // --- Appended; existing methods above are untouched per the shared
  // --- driver contract. This child owns estimatesOpenSettings-style
  // --- navigation; sibling EST children reuse it.
  // --- Estimates permissions matrix + edition split (NEWFRONT-326,
  // --- EST-024 + EST-029). Appended; existing methods above are untouched
  // --- per the shared driver contract. The shell child's navigation
  // --- (NEWFRONT-321, merged) is reused as-is; estimatesEnableSwitchVisible,
  // --- estimatesActiveSystemName and estimatesActiveRowButtonCount mirror
  // --- the enable sibling's (NEWFRONT-322, unmerged); the creation-dialog
  // --- readers mirror the create sibling's (NEWFRONT-323, unmerged); the
  // --- delete readers mirror the edit sibling's (NEWFRONT-324, unmerged).
  // --- Dedup at rebase, keeping one.
  /** Open the project estimates settings screen; settles on the heading or the denial view. */
  estimatesOpenSettings(workspaceSlug: string, projectId: string): Promise<void>;
  /** Whether the settings page shows the not-authorized notice. */
  estimatesNotAuthorized(): Promise<boolean>;
  /** Whether the main Estimates heading renders. */
  estimatesHeadingVisible(): Promise<boolean>;
  /** Breadcrumb trail text on the estimates settings header. */
  estimatesBreadcrumbText(): Promise<string>;
  /** Whether the estimates loading skeleton shows. */
  estimatesSkeletonVisible(): Promise<boolean>;
  /** Delay every project-estimates list GET by `ms` for the rest of the test. */
  estimatesDelayListFetch(ms: number): Promise<void>;
  /** Whether the no-systems empty state (with its add action) shows. */
  estimatesEmptyVisible(): Promise<boolean>;
  /** Title of the empty state, or "" when it shows nothing. */
  estimatesEmptyTitle(): Promise<string>;
  /** Click the empty state's add action. */
  estimatesEmptyAddClick(): Promise<void>;
  /** Whether a creation dialog is open (presence only; contents are the creation child's). */
  estimatesCreateDialogOpen(): Promise<boolean>;
  /** Visible text of the settings content area (excludes app chrome). */
  estimatesScreenText(): Promise<string>;
  /** Names of the estimate systems listed on the screen. */
  estimatesListedNames(): Promise<string[]>;
  /** Browser tab title on the estimates settings screen. */
  estimatesTabTitle(): Promise<string>;
  /** Whether any export/import/download control shows on the estimates screen. */
  estimatesExportImportVisible(): Promise<boolean>;
  /** Whether any filter/search control shows on the estimates screen. */
  estimatesFilterSearchVisible(): Promise<boolean>;
  /** Whether any bulk/multi-select control shows on the estimates screen. */
  estimatesBulkVisible(): Promise<boolean>;
  /** Open a guessed per-system address; settles on whatever the app renders for it. */
  estimatesOpenSystemGuess(workspaceSlug: string, projectId: string, estimateId: string): Promise<void>;
  /** Whether the app-level not-found surface shows. */
  estimatesNotFoundVisible(): Promise<boolean>;

  // --- Estimates consumers: analytics axes (NEWFRONT-325, EST-020).
  // --- Appended; existing methods above are untouched per the shared
  // --- driver contract. The project analytics dialog (openAnalytics)
  // --- hosts a Customized Insights section with y-axis, x-axis and
  // --- group-by selects plus a bar chart backed by a data table. The
  // --- analytics area (NEWFRONT-56, still Backlog) reuses these
  // --- estimates*-named readers when it lands.
  /** Open the project analytics dialog from the issues header; ends with it visible. */
  estimatesOpenAnalytics(): Promise<void>;
  /** Current y-axis metric label in the dialog's Customized Insights section. */
  estimatesAnalyticsYAxisValue(): Promise<string>;
  /** Metric labels the open y-axis select offers, in display order. */
  estimatesAnalyticsYAxisOptions(): Promise<string[]>;
  /** Pick a y-axis metric; resolves once the select shows it. */
  estimatesAnalyticsPickYAxis(label: string): Promise<void>;
  /** Current x-axis property label in the dialog's Customized Insights section. */
  estimatesAnalyticsXAxisValue(): Promise<string>;
  /** Property labels the open x-axis select offers, in display order. */
  estimatesAnalyticsXAxisOptions(): Promise<string[]>;
  /** Pick an x-axis property; resolves once the select shows it. */
  estimatesAnalyticsPickXAxis(label: string): Promise<void>;
  /** Cell texts of the Customized Insights data table, row by row (header first). */
  estimatesAnalyticsTableRows(): Promise<string[][]>;

  // -------------------------------------------------------------------------
  // Modules cross-cutting (NEWFRONT-311, rows MOD-036..043).
  // Settings toggle + flag gate, palette creation and module menus, deep
  // links, and the no-live-update / keyboard / no-drag-export /
  // desktop / edition negatives. Appended additively; sibling modules*
  // methods are untouched. The `modulesX` prefix marks the cross-cutting
  // child's additions so parallel children never collide on a name.
  // -------------------------------------------------------------------------

  // --- settings toggle + flag gate (MOD-036) ---
  /** Open the project features settings page for modules; resolves once settled. */
  modulesXOpenSettingsModules(workspaceSlug: string, projectId: string): Promise<void>;
  /** Whether the settings page shows the not-authorized notice instead of the toggle. */
  modulesXSettingsShowsNotAuthorized(): Promise<boolean>;
  /** The modules toggle state, or null when no toggle renders. */
  modulesXModulesToggleState(): Promise<boolean | null>;
  /** Flip the modules toggle; resolves once the flag PATCH answers. */
  modulesXFlipModulesToggle(): Promise<void>;
  /** Open the project's modules list; resolves once rows/empty/gate settle. */
  modulesXOpenModulesList(workspaceSlug: string, projectId: string): Promise<void>;
  /** Whether the list shows the flag-off gate instead of rows. */
  modulesXListGateVisible(): Promise<boolean>;
  /** The gate's Manage-features button visibility + disabled state. */
  modulesXGateManageFeaturesState(): Promise<{ visible: boolean; disabled: boolean }>;
  /** Module row names in display order on the list layout. */
  modulesXListNames(): Promise<string[]>;

  // --- create/edit dialog (MOD-037/039/040) ---
  /** Whether the module create/edit dialog is open. */
  modulesXModuleDialogOpen(): Promise<boolean>;
  /** Fill the open create dialog's name field. */
  modulesXFillCreateName(name: string): Promise<void>;
  /** Submit the open create dialog; resolves once the create answers. */
  modulesXSubmitCreate(): Promise<void>;
  /** Focus descriptors walking Tab from the dialog's initial focus. */
  modulesXDialogTabSequence(): Promise<string[]>;
  /** Descriptor of the currently focused element (dialog focus settle). */
  modulesXDialogFocused(): Promise<string>;
  /** Open the row menu for `name` and pick Edit; resolves once the dialog opens. */
  modulesXOpenEditDialogFromRow(name: string): Promise<void>;

  // --- palette module menus (MOD-037) ---
  /** Module entries in the open palette module menu, with icon + check marks. */
  modulesXPaletteModuleMenuItems(): Promise<ModulesXPaletteMenuItem[]>;
  /** Activate the palette module-menu entry `name` (jump navigation). */
  modulesXActivatePaletteModuleMenuItem(name: string): Promise<void>;

  // --- deep links (MOD-038) ---
  /** Open a module's detail page directly; resolves once content/empty settle. */
  modulesXOpenModuleDetail(workspaceSlug: string, projectId: string, moduleId: string): Promise<void>;
  /** The module name the detail page shows, or null when it shows none. */
  modulesXDetailName(): Promise<string | null>;
  /** Whether detail shows the missing-module empty state. */
  modulesXDetailMissingVisible(): Promise<boolean>;
  /** Click the missing-module empty state's back button; resolves once the list loads. */
  modulesXMissingModuleBackToList(): Promise<void>;
  /** Whether the project access wall shows instead of the module content. */
  modulesXProjectAccessWallVisible(): Promise<boolean>;
  /** Open the modules list carrying the peek parameter for `moduleId`. */
  modulesXOpenModulesListWithPeek(workspaceSlug: string, projectId: string, moduleId: string): Promise<void>;
  /** The module name the peek panel shows, or null when no panel shows. */
  modulesXPeekName(): Promise<string | null>;

  // --- keyboard + list search (MOD-040) ---
  /** Press a key with neutral page focus (Escape/Tab/single letters). */
  modulesXPressKey(key: string): Promise<void>;
  /** Press a two-key palette sequence back-to-back (e.g. "nm" for creation). */
  modulesXPressKeySequence(sequence: string): Promise<void>;
  /** Expand the list search field via its icon. */
  modulesXOpenListSearch(): Promise<void>;
  /** Fill the expanded list search field. */
  modulesXFillListSearch(text: string): Promise<void>;
  /** Whether the search field is expanded, plus its current query. */
  modulesXListSearchState(): Promise<{ expanded: boolean; query: string }>;

  // --- drag + layout + export absence (MOD-041) ---
  /** Drag the list row `sourceName` onto the row `targetName`. */
  modulesXAttemptRowDrag(sourceName: string, targetName: string): Promise<void>;
  /** Switch the modules layout; resolves once the new layout settles. */
  modulesXSwitchModulesLayout(layout: "list" | "board" | "timeline"): Promise<void>;
  /** Module card names in display order on the board layout. */
  modulesXBoardNames(): Promise<string[]>;
  /** Drag the board card `sourceName` onto the card `targetName`. */
  modulesXAttemptCardDrag(sourceName: string, targetName: string): Promise<void>;
  /** Whether any export/import/print action shows on the named surface. */
  modulesXExportActionsPresent(scope: "list" | "detail" | "dialog"): Promise<boolean>;
  /** Entry labels of the opened row menu for `name` (menu closes after). */
  modulesXRowMenuEntries(name: string): Promise<string[]>;

  // --- desktop + edition negatives (MOD-042/043) ---
  /** API paths the modules list entry fetches (traffic-allowlist spy). */
  modulesXEntryRequestPaths(workspaceSlug: string, projectId: string): Promise<string[]>;
  /** Whether the first list row shows a favorite toggle. */
  modulesXRowFavoriteToggleVisible(): Promise<boolean>;
  /** Open the list filters menu; resolves true when its panel shows. */
  modulesXFiltersMenuOpens(): Promise<boolean>;
  /** Whether the timeline layout renders module bars. */
  modulesXTimelineBarsPresent(): Promise<boolean>;
  /** Whether the detail page shows the analytics sidebar. */
  modulesXAnalyticsSidebarVisible(): Promise<boolean>;
  /** Whether the detail sidebar shows the links section. */
  modulesXLinksSectionVisible(): Promise<boolean>;
  /** Whether edition/desktop gate markers show on the named surface. */
  modulesXEditionGateMarkersPresent(scope: "list" | "detail"): Promise<boolean>;

  // ---------------------------------------------------------------------------
  // Modules dialogs, archive and archived screen (NEWFRONT-309, MOD-022..028).
  // Observed on apps/web: the create/edit form (project picker on create
  // only, required 255-capped title, free-text description, date range,
  // required status, single lead, multiple members), the delete confirm
  // (names the module, permanence warning, list navigation from detail),
  // and the archived screen under the archives tab strip. Row-menu,
  // archive-dialog and archived-chrome reads reuse the merged archives*
  // drivers (NEWFRONT-225); only dialog reads/writes are new here.
  // Appended; existing entries above are untouched per the shared contract.
  // ---------------------------------------------------------------------------

  /** Open the create dialog through the list header Add control. */
  modulesOpenCreateFromHeader(): Promise<void>;
  /** Open the create dialog through the zero-modules empty-state action. */
  modulesOpenCreateFromEmpty(): Promise<void>;
  /** Whether the create/edit dialog currently holds the title field. */
  modulesDialogOpen(): Promise<boolean>;
  /** Whether the dialog shows the project picker (creation only). */
  modulesDialogHasProjectPicker(): Promise<boolean>;
  /** Project picker option names in display order (opened read-only). */
  modulesDialogProjectOptions(): Promise<string[]>;
  /** Pick a project in the creation picker; resolves once it shows. */
  modulesDialogPickProject(name: string): Promise<void>;
  /** Current title field value. */
  modulesDialogTitle(): Promise<string>;
  /** Fill the title field. */
  modulesDialogFillTitle(text: string): Promise<void>;
  /** Inline title error text, or null when none shows. */
  modulesDialogTitleError(): Promise<string | null>;
  /** Current description field value. */
  modulesDialogDescription(): Promise<string>;
  /** Fill the description field. */
  modulesDialogFillDescription(text: string): Promise<void>;
  /** Status the dialog currently shows. */
  modulesDialogStatus(): Promise<string>;
  /** Pick a status; resolves once the dialog shows it. */
  modulesDialogPickStatus(label: string): Promise<void>;
  /** Lead control text (placeholder while unset). */
  modulesDialogLead(): Promise<string>;
  /** Lead picker option names in display order (opened read-only). */
  modulesDialogLeadOptions(): Promise<string[]>;
  /** Pick a lead; resolves once the dialog shows them. */
  modulesDialogPickLead(label: string): Promise<void>;
  /** Members control text (placeholder while empty). */
  modulesDialogMembers(): Promise<string>;
  /** Toggle members by option label; resolves once each applies. */
  modulesDialogPickMembers(labels: string[]): Promise<void>;
  /** Date-range control text (placeholders while unset). */
  modulesDialogDateRange(): Promise<string>;
  /** Pick a start/end range through the calendar; resolves once applied. */
  modulesDialogPickDateRange(startISO: string, endISO: string): Promise<void>;
  /** Tab indices of title, description, dates, status, lead, members, cancel, submit (null when unset). */
  modulesDialogTabOrder(): Promise<(number | null)[]>;
  /** Tab index of the project picker, or null when absent/unset. */
  modulesDialogProjectPickerTabIndex(): Promise<number | null>;
  /** Cancel the dialog; resolves once it unmounts. */
  modulesDialogCancel(): Promise<void>;
  /** Press Escape; resolves once the dialog unmounts. */
  modulesDialogEscape(): Promise<void>;
  /** Submit creation; settles on close, toast, or inline title error. */
  modulesDialogSubmitCreate(): Promise<void>;
  /** Submit the edit; settles on close, toast, or inline title error. */
  modulesDialogSubmitUpdate(): Promise<void>;
  /** Abort the next module-create write once (failure shaping). */
  modulesFailNextCreate(): Promise<void>;
  /** Abort the next module-update write once (failure shaping). */
  modulesFailNextUpdate(): Promise<void>;
  /** Open the detail header quick-actions menu (Delete lives there). */
  modulesOpenDetailMenu(): Promise<void>;
  /** Pick a detail-menu entry by title; resolves once it fires. */
  modulesChooseDetailMenuEntry(title: string): Promise<void>;
  /** The open delete dialog's copy, or null when none is open. */
  modulesDeleteDialog(): Promise<ModulesDeleteDialog | null>;
  /** Confirm deletion; resolves once the DELETE write lands. */
  modulesDeleteConfirm(): Promise<void>;
  /** Cancel the delete dialog; resolves once it unmounts. */
  modulesDeleteCancel(): Promise<void>;
  /** Refuse the next module DELETE with the server's permission shape. */
  modulesFailDeletePermission(): Promise<void>;
  /** Fail the next module DELETE with a generic server error. */
  modulesFailNextDelete(): Promise<void>;
  /** Row facts on the showing modules list (name link plus peek trigger). */
  modulesRowFacts(name: string): Promise<ModulesRowFacts>;

  // --- Intake list content (NEWFRONT-256, INT-006/007/009/010/011). Row
  // --- reads, cursor paging, the filter panel + chips, custom date ranges,
  // --- and the order menu. Sections/options/chips speak canonical keys and
  // --- data identities (names, not copy) so the same scenario runs on the
  // --- redesigned UI; only domain vocabulary (status/priority/preset names)
  // --- is asserted literally.

  /** Open the intake list on `tab`; resolves once rows or the empty state settle. */
  intakeListOpen(workspaceSlug: string, projectId: string, tab: "open" | "closed"): Promise<void>;
  /** Nested issue ids of the rendered rows, in display order. */
  intakeListRowIds(): Promise<string[]>;
  /** Project-scoped id label of one row (identifier + sequence). */
  intakeListRowIdLabel(issueId: string): Promise<string>;
  /** Status chip text of one row, or null when no chip renders. */
  intakeListRowStatusChip(issueId: string): Promise<string | null>;
  /** Title text of one row. */
  intakeListRowTitle(issueId: string): Promise<string>;
  /** Created-date text of one row. */
  intakeListRowCreatedText(issueId: string): Promise<string>;
  /** Created-date tooltip (hover): heading plus content. */
  intakeListRowCreatedTooltip(issueId: string): Promise<IntakeListTooltip>;
  /** Priority key one row's marker presents (urgent/high/medium/low/none). */
  intakeListRowPriority(issueId: string): Promise<string>;
  /** Label pill texts of one row (collapsed to a count pill at four or more). */
  intakeListRowLabels(issueId: string): Promise<string[]>;
  /** Which avatar one row shows: a member, the intake identity, or none. */
  intakeListRowAvatarKind(issueId: string): Promise<"member" | "intake" | "none">;
  /** Scroll the list to the bottom to trigger the next page fetch. */
  intakeListScrollToBottom(): Promise<void>;
  /** Whether placeholder rows render (a page is loading). */
  intakeListSkeletonsVisible(): Promise<boolean>;
  /** Whether the header syncing hint renders. */
  intakeListSyncingVisible(): Promise<boolean>;
  /** Resolve once exactly `count` rows render (bounded wait). */
  intakeListWaitForRowCount(count: number): Promise<void>;
  /** Delay the next list reads by `ms` so loading states can be observed. */
  intakeListDelayNextListReads(ms: number): Promise<void>;
  /** Open the filter panel; idempotent when already open. */
  intakeListOpenFilters(): Promise<void>;
  /** Close the filter panel (Escape); idempotent when already closed. */
  intakeListCloseFilters(): Promise<void>;
  /** Type into the panel's search box (replaces any previous query). */
  intakeListFilterSearch(query: string): Promise<void>;
  /** Canonical keys of the sections the panel currently shows. */
  intakeListFilterSections(): Promise<IntakeListFilterKey[]>;
  /** Option identities of one section, in display order. */
  intakeListFilterOptions(key: IntakeListFilterKey): Promise<string[]>;
  /** Pick (toggle) one option in one section; resolves once the list refetches. */
  intakeListFilterPick(key: IntakeListFilterKey, option: string): Promise<void>;
  /** Whether one option renders checked. */
  intakeListFilterChecked(key: IntakeListFilterKey, option: string): Promise<boolean>;
  /** Applied count one section header carries (0 when it shows none). */
  intakeListFilterSectionCount(key: IntakeListFilterKey): Promise<number>;
  /** Applied-filter chips as canonical groups with data-identity values. */
  intakeListChips(): Promise<IntakeListChip[]>;
  /** Remove one chip value; resolves once the list refetches. */
  intakeListChipRemove(key: IntakeListFilterKey, value: string): Promise<void>;
  /** Clear a whole chip group; resolves once the list refetches. */
  intakeListChipClearGroup(key: IntakeListFilterKey): Promise<void>;
  /** Start capturing intake-list request URLs (idempotent). */
  intakeListBeginRequestSpy(): Promise<void>;
  /** Query params of the most recent captured list request, or null when none. */
  intakeListLastRequestParams(): Promise<Record<string, string> | null>;
  /** Open the order menu; idempotent when already open. */
  intakeListOpenOrderMenu(): Promise<void>;
  /** Current order state: the checked field plus the checked direction. */
  intakeListOrderState(): Promise<IntakeListOrderState>;
  /** Pick an order field; resolves once the list refetches. */
  intakeListPickOrderField(field: IntakeListOrderField): Promise<void>;
  /** Pick a sort direction; resolves once the list refetches. */
  intakeListPickOrderDirection(direction: "asc" | "desc"): Promise<void>;
  /** Open the custom-date modal for one date section. */
  intakeListOpenCustomDate(key: "createdAt" | "updatedAt"): Promise<void>;
  /** Apply an explicit from/to range in the open custom-date modal. */
  intakeListCustomDateApply(fromISO: string, toISO: string): Promise<void>;

  // Active-cycle hero and cycles cross-cutting (NEWFRONT-254,
  // CYC-040..045, CYC-047..048). Observed on apps/web: the cycles list
  // leads with an active-cycle disclosure (three cards over the cycle
  // covering today, or an explanatory empty view), the sidebar and the
  // quick-look panel share one skeleton-then-content sidebar, the
  // workspace actives page is an OSS upsell, date controls carry the
  // project UTC offset with user-timezone hover equivalents, creation
  // and transfers refresh counts without reload while the archived list
  // never refetches on focus, guests see no mutation affordance, and
  // narrow screens move row menus inline with a layout switcher.
  // Appended; existing entries above are untouched per the shared
  // contract.
  // ---------------------------------------------------------------------------

  /** Open the project's live cycles list; waits until hero/rows/empty settle. */
  cyclesHeroOpenList(workspaceSlug: string, projectId: string): Promise<void>;
  /** Open a cycle's detail page; returns after first paint (no settle wait). */
  cyclesHeroOpenDetail(workspaceSlug: string, projectId: string, cycleId: string): Promise<void>;
  /** Open the list with `?peekCycle=` set; returns after first paint. */
  cyclesHeroOpenPeekRaw(workspaceSlug: string, projectId: string, cycleId: string): Promise<void>;
  /** Reload the current page; returns after first paint. */
  cyclesHeroReload(): Promise<void>;
  /** Delay the next cycle list/detail/progress reads by ms (skeleton shaping). */
  cyclesHeroDelayDetailReads(ms: number): Promise<void>;
  /** Strip `cycleId` from list reads so the peek mounts on a store miss. */
  cyclesHeroHideCycleFromListReads(cycleId: string): Promise<void>;
  /** Whether the detail sidebar currently shows its loading skeleton. */
  cyclesHeroSidebarSkeletonVisible(): Promise<boolean>;
  /** Detail sidebar cycle name, or null while the skeleton shows. */
  cyclesHeroSidebarName(): Promise<string | null>;
  /** Whether the quick-look panel currently shows its loading skeleton. */
  cyclesHeroPeekSkeletonVisible(): Promise<boolean>;
  /** Quick-look panel cycle name, or null while the skeleton shows. */
  cyclesHeroPeekName(): Promise<string | null>;
  /** Active-cycle group heading text, or null when the group is absent. */
  cyclesHeroGroupHeading(): Promise<string | null>;
  /** Name of the cycle row inside the hero, or null when none renders. */
  cyclesHeroActiveName(): Promise<string | null>;
  /** Hero empty-view copy, or null when the hero shows cards. */
  cyclesHeroEmptyCopy(): Promise<{ title: string; description: string } | null>;
  /** Progress card: closed-counts line plus per-state rows. */
  cyclesHeroProgressGroups(): Promise<{ closed: string | null; groups: CyclesHeroProgressGroup[] }>;
  /** Whether the progress card shows its per-card empty view. */
  cyclesHeroProgressEmptyVisible(): Promise<boolean>;
  /** Burn-down card heading, pending line, and whether the chart renders. */
  cyclesHeroBurndown(): Promise<{ heading: string | null; pending: string | null; chart: boolean }>;
  /** Whether the burn-down card shows its per-card empty view. */
  cyclesHeroBurndownEmptyVisible(): Promise<boolean>;
  /** Breakdown card tab labels in display order. */
  cyclesHeroBreakdownTabs(): Promise<string[]>;
  /** Select a breakdown card tab by label. */
  cyclesHeroBreakdownSelectTab(label: string): Promise<void>;
  /** Entry texts of the active breakdown tab panel. */
  cyclesHeroBreakdownEntries(): Promise<string[]>;
  /** Whether the active breakdown panel shows its per-card empty view. */
  cyclesHeroBreakdownEmptyVisible(): Promise<boolean>;
  /** Click the named progress group; waits for the detail navigation. */
  cyclesHeroClickProgressGroup(group: string): Promise<void>;
  /** Click the named progress group without waiting for navigation. */
  cyclesHeroTapProgressGroup(group: string): Promise<void>;
  /** Click the indexed breakdown entry; waits for the detail navigation. */
  cyclesHeroClickBreakdownEntry(index: number): Promise<void>;
  /** Open the workspace-level active-cycles page; waits until it settles. */
  cyclesHeroOpenWorkspaceActives(workspaceSlug: string): Promise<void>;
  /** Upsell banner heading, benefit-card count, and upgrade-link facts. */
  cyclesHeroUpsell(): Promise<CyclesHeroUpsell>;
  /** Whether the page header carries the upgrade marker. */
  cyclesHeroUpsellBadgeVisible(): Promise<boolean>;
  /** Offset chip text on the active row, or null when no chip shows. */
  cyclesHeroActiveRowOffset(): Promise<string | null>;
  /** Hover the active row's date span (reveals the timezone tip). */
  cyclesHeroHoverActiveRowDates(): Promise<void>;
  /** Offset chip text in the detail sidebar, or null when no chip shows. */
  cyclesHeroSidebarOffset(): Promise<string | null>;
  /** Hover the sidebar's date control (reveals the timezone tip). */
  cyclesHeroHoverSidebarDates(): Promise<void>;
  /** Visible tooltip text after a hover, or null when none shows. */
  cyclesHeroTooltipText(): Promise<string | null>;
  /** Start counting cycle reads, archived reads, and cycle writes. */
  cyclesHeroBeginTrafficSpy(): Promise<void>;
  /** Traffic counters since the spy began. */
  cyclesHeroTrafficCounts(): Promise<CyclesHeroTrafficCounts>;
  /** Open the cycle-create dialog from the list header. */
  cyclesHeroOpenCreate(): Promise<void>;
  /** Fill the create dialog's title field. */
  cyclesHeroFillCreateName(name: string): Promise<void>;
  /** Submit the create dialog; waits until it closes. */
  cyclesHeroSubmitCreate(): Promise<void>;
  /** Whether the create dialog is currently open. */
  cyclesHeroCreateOpen(): Promise<boolean>;
  /** Cycle row names in display order on the current list. */
  cyclesHeroVisibleNames(): Promise<string[]>;
  /** Transfer banner text on the named row, or null when none shows. */
  cyclesHeroTransferBanner(name: string): Promise<string | null>;
  /** Open the transfer dialog from the named row's banner. */
  cyclesHeroOpenTransfer(name: string): Promise<void>;
  /** Type into the transfer dialog's search box. */
  cyclesHeroTransferSearch(text: string): Promise<void>;
  /** Transfer dialog option names in display order. */
  cyclesHeroTransferOptions(): Promise<string[]>;
  /** Pick the named transfer target; waits until the dialog closes. */
  cyclesHeroTransferPick(name: string): Promise<void>;
  /** Dispatch a window focus event (focus-refetch probe). */
  cyclesHeroFocusWindow(): Promise<void>;
  /** Header create control state for the signed-in role. */
  cyclesHeroCreateButtonState(): Promise<"absent" | "disabled" | "enabled">;
  /** Open the row menu of the named cycle. */
  cyclesHeroOpenRowMenu(name: string): Promise<void>;
  /** Entries of the open row menu in display order. */
  cyclesHeroRowMenuEntries(): Promise<string[]>;
  /** Dismiss any open row menus. */
  cyclesHeroCloseMenus(): Promise<void>;
  /** Whether the sidebar date control refuses edits (read-only roles). */
  cyclesHeroSidebarDateDisabled(): Promise<boolean>;
  /** Whether the named row offers the favorites marker. */
  cyclesHeroFavoriteVisible(name: string): Promise<boolean>;
  /** Whether the list search control renders. */
  cyclesHeroSearchVisible(): Promise<boolean>;
  /** Whether the list filter control renders. */
  cyclesHeroFilterVisible(): Promise<boolean>;
  /** Whether the named row's inline (touch) quick actions show. */
  cyclesHeroInlineActionsVisible(name: string): Promise<boolean>;
  /** Whether the named row's hover (desktop) menu control shows. */
  cyclesHeroHoverActionsVisible(name: string): Promise<boolean>;
  /** Whether the mobile layout switcher renders on the list. */
  cyclesHeroLayoutMenuVisible(): Promise<boolean>;
  /** Layout options in the open mobile switcher. */
  cyclesHeroLayoutOptions(): Promise<string[]>;
  /** Pick a layout in the open mobile switcher. */
  cyclesHeroPickLayout(option: string): Promise<void>;
  /** Whether the mobile layout switcher renders on the detail page. */
  cyclesHeroDetailLayoutMenuVisible(): Promise<boolean>;
  /** Open the list search box. */
  cyclesHeroSearchOpen(): Promise<void>;
  /** Type into the open list search box. */
  cyclesHeroSearchType(text: string): Promise<void>;
  /** Current list search-box text. */
  cyclesHeroSearchText(): Promise<string>;
  /** Open the list filter menu and read its visible option texts. */
  cyclesHeroFilterMenuTexts(): Promise<string[]>;
  /** Open the named row's quick-look panel through its details action. */
  cyclesHeroOpenPeek(name: string): Promise<void>;
  /** Click the open row menu entry whose label contains `entry`. */
  cyclesHeroRowMenuClick(entry: string): Promise<void>;
  /** Grant clipboard read/write so copy-link confirmations fire headless. */
  cyclesHeroGrantClipboard(): Promise<void>;
  /** Current clipboard text (copy-link proof). */
  cyclesHeroReadClipboard(): Promise<string>;
  /** Collapse the mobile sidebar drawer when it covers the content. */
  cyclesHeroDismissSidebar(): Promise<void>;
  /** Layout options in the open detail mobile switcher. */
  cyclesHeroDetailLayoutOptions(): Promise<string[]>;
  /** Pick a layout in the open detail mobile switcher. */
  cyclesHeroDetailPickLayout(option: string): Promise<void>;

  /** Whether the enable-estimates switch renders on the screen. */
  estimatesEnableSwitchVisible(): Promise<boolean>;
  /** Name of the active estimate system, or "" when no active row shows. */
  estimatesActiveSystemName(): Promise<string>;
  /** Buttons rendered on the active row (OSS ships delete-only). */
  estimatesActiveRowButtonCount(): Promise<number>;
  /** Open the creation dialog from the empty state; resolves once step one shows. */
  estimatesOpenCreateDialog(): Promise<void>;
  /** Whether the creation dialog is currently open. */
  estimatesDialogOpen(): Promise<boolean>;
  /** Measurement-family radio options in display order. */
  estimatesFamilyOptions(): Promise<EstimatesFamilyOption[]>;
  /** Select a measurement family; resolves once its starters show. */
  estimatesSelectFamily(family: "points" | "categories"): Promise<void>;
  /** Pick a starter card by title; resolves once step two shows. */
  estimatesPickStarter(title: string): Promise<void>;
  /** Submit the dialog; resolves once the write settles (close or error notice). */
  estimatesSubmit(): Promise<void>;
  /** Click the active row's delete control. */
  estimatesActiveRowDeleteClick(): Promise<void>;
  /** Whether any app dialog is currently open. */
  estimatesConfirmDialogOpen(): Promise<boolean>;
  /** Dismiss the open confirmation dialog through its cancel action. */
  estimatesConfirmDialogCancel(): Promise<void>;
  /** Heading text of the open confirmation dialog, or "" when none is open. */
  estimatesEditDeleteConfirmDialogTitle(): Promise<string>;
  /** Body text of the open confirmation dialog, or "" when none is open. */
  estimatesEditDeleteConfirmDialogBody(): Promise<string>;
  /** Confirm the open whole-system delete dialog; resolves once it closes. */
  estimatesEditDeleteConfirmDialogConfirm(): Promise<void>;
  /** Whether the active row exposes any edit control (cloud-only; false in OSS). */
  estimatesEditDeleteActiveRowEditControlVisible(): Promise<boolean>;

  // -------------------------------------------------------------------------
  // Estimates creation dialog (NEWFRONT-323, EST-008..013). Appended;
  // existing methods above are untouched per the shared driver contract.
  // The shell child's navigation (NEWFRONT-321, merged) and the dialog
  // verbs shared with the permissions child (NEWFRONT-326, merged) are
  // reused as-is; only the dialog-specific readers below are new.
  // -------------------------------------------------------------------------

  /** Current step counter ("1" or "2"), or null when the dialog is closed. */
  estimatesDialogStep(): Promise<"1" | "2" | null>;
  /** Click the dialog back control; resolves once step one shows. */
  estimatesDialogBack(): Promise<void>;
  /** Whether the dialog back control is offered. */
  estimatesBackVisible(): Promise<boolean>;
  /** Click Cancel; resolves once the dialog closes. */
  estimatesDialogCancel(): Promise<void>;
  /** Step-one starter cards (scratch plus templates) with their previews. */
  estimatesStarterCards(): Promise<EstimatesStarterCard[]>;
  /** Step-two value rows in display order. */
  estimatesStepTwoValues(): Promise<string[]>;
  /** Step-two family label as rendered (lowercase family key). */
  estimatesStepTwoFamily(): Promise<string>;
  /** Drag the value at sourceIndex above the value at targetIndex (real mouse drag). */
  estimatesDragValueBefore(sourceIndex: number, targetIndex: number): Promise<void>;
  /** Open the inline edit form for the value at index; resolves once its pre-populated value reads back. */
  estimatesEditValueStart(index: number, via: "double-click" | "control"): Promise<void>;
  /** Current value of the open value form, or null when no form is open. */
  estimatesOpenFormValue(): Promise<string | null>;
  /** Fill the open value form and submit it; resolves once the value lands in the rows with no form left open. */
  estimatesOpenFormSave(value: string): Promise<void>;
  /** Fill the open value form without submitting it. */
  estimatesOpenFormFill(value: string): Promise<void>;
  /** Submit the open value form; returns without waiting for it to close. */
  estimatesOpenFormSubmit(): Promise<void>;
  /** Close the open value form without saving. */
  estimatesOpenFormCancel(): Promise<void>;
  /** How many value forms are currently open. */
  estimatesOpenFormCount(): Promise<number>;
  /** Remove the value at index through its row control. */
  estimatesRemoveValue(index: number): Promise<void>;
  /** Whether the value at index offers a remove control. */
  estimatesRemoveVisible(index: number): Promise<boolean>;
  /** Open the add-value form. */
  estimatesAddOpen(): Promise<void>;
  /** Whether the add-value control is offered. */
  estimatesAddVisible(): Promise<boolean>;
  /** Type into the open add form one keystroke at a time (length-cap path). */
  estimatesAddType(value: string): Promise<void>;
  /** Validation messages currently flagged on value rows, in marker order. */
  estimatesValueErrors(): Promise<string[]>;
  /** How many value rows currently show a validation marker. */
  estimatesValueErrorCount(): Promise<number>;
  /** Active system row as the settings screen shows it, or null when absent. */
  estimatesActiveRow(): Promise<EstimatesActiveRow | null>;

  // --- Intake-create dialog oracles (NEWFRONT-257, INT-013–017/033). ---
  // --- Appended; existing methods above are untouched per the shared
  // --- driver contract. Continues the merged intakeCreate family from
  // --- NEWFRONT-122 (open, title fill, submit) with the rest of the
  // --- dialog surface: description editing, priority picking, the
  // --- over-long-title guard, the create-more toggle, discard/escape,
  // --- and deterministic editor-upload control for the busy-editor
  // --- guard plus the creation-attachment path.
  /** Whether the intake-create dialog currently shows. */
  intakeCreateDialogOpen(): Promise<boolean>;
  /** Current value of the dialog's title field. */
  intakeCreateTitleValue(): Promise<string>;
  /** Over-long-title warning text, or "" when no warning shows. */
  intakeCreateTitleHint(): Promise<string>;
  /** Whether the dialog's Create submit control is disabled. */
  intakeCreateSubmitDisabled(): Promise<boolean>;
  /** Type `text` into the dialog's description editor. */
  intakeCreateFillDescription(text: string): Promise<void>;
  /** Plain text currently in the dialog's description editor. */
  intakeCreateDescriptionText(): Promise<string>;
  /** Priority name currently shown on the dialog's priority picker. */
  intakeCreatePriorityValue(): Promise<string>;
  /** Open the dialog's priority picker and choose `label`. */
  intakeCreateSetPriority(label: string): Promise<void>;
  /** Whether the dialog's create-more toggle is on. */
  intakeCreateMoreOn(): Promise<boolean>;
  /** Flip the dialog's create-more toggle. */
  intakeCreateToggleMore(): Promise<void>;
  /** Click the dialog's Discard control (no close wait; callers assert). */
  intakeCreateDiscard(): Promise<void>;
  /** Press Escape with the dialog open. */
  intakeCreatePressEscape(): Promise<void>;
  /** Submit with create-more on; resolves once the creation toast shows (the dialog stays open). */
  intakeCreateSubmitStayingOpen(): Promise<void>;
  /**
   * Hold the next editor-asset upload POST unanswered so the editor stays
   * busy deterministically; resolves once armed.
   */
  intakeCreateStallNextUpload(): Promise<void>;
  /** Let held editor-asset uploads through and disarm the stall. */
  intakeCreateReleaseUpload(): Promise<void>;
  /**
   * Insert an image into the dialog's description editor through its
   * insert menu, supplying `file` to the picker. When a stall is armed,
   * resolves once the upload request is observed (still held).
   */
  intakeCreateInsertImage(file: { name: string; mime: string; bytes: Buffer }): Promise<void>;
  /** Images currently rendered in the dialog's description editor. */
  intakeCreateEditorImageCount(): Promise<number>;
  // --- Pages gating, list states, URLs, detail shell (NEWFRONT-314,
  // --- PAGE-018-025,079-080). Appended; existing methods above are
  // --- untouched per the shared driver contract.
  /** Open a project's pages list; settles on a terminal variant unless told to mount only (stall proofs). */
  pagesGatingOpenList(workspaceSlug: string, projectId: string, query?: string, settle?: boolean): Promise<void>;
  /** Open one page's detail through its deep link; settles on domcontentloaded only (panel cases never mount an editor). */
  pagesGatingOpenDetail(workspaceSlug: string, projectId: string, pageId: string, query?: string): Promise<void>;
  /** Current page URL. */
  pagesGatingCurrentUrl(): Promise<string>;
  /** Current value of one URL query key, or null when absent. */
  pagesGatingQueryParam(key: string): Promise<string | null>;
  /** Document title. */
  pagesGatingDocTitle(): Promise<string>;
  /** Titles of the rendered list rows, in display order. */
  pagesGatingRowTitles(): Promise<string[]>;
  /** Href of one row's detail link, or null when the row is absent. */
  pagesGatingRowHref(title: string): Promise<string | null>;
  /** Active list tab from the `type` query value (unknown and missing read back public). */
  pagesGatingActiveTab(): Promise<PagesGatingTab>;
  /** Click one list tab; resolves once the row set settles. */
  pagesGatingClickTab(tab: PagesGatingTab): Promise<void>;
  /** True when the list header offers the quick-create action. */
  pagesGatingCreateOffered(): Promise<boolean>;
  /** True when one row offers its favorite star. */
  pagesGatingRowStarOffered(title: string): Promise<boolean>;
  /** Labels of one row's overflow menu, in display order. */
  pagesGatingRowMenuItems(title: string): Promise<string[]>;
  /** Presence of the detail header action cluster (star, copy-link, menu) plus the sync badge label, if any. */
  pagesGatingDetailCluster(): Promise<PagesGatingHeaderCluster>;
  /** Labels of the detail header options menu, in display order. */
  pagesGatingDetailMenuItems(): Promise<string[]>;
  /** Count of editable regions on the detail view (title plus body when editable, none when read-only or panelled). */
  pagesGatingEditableCount(): Promise<number>;
  /** True when the detail error panel shows instead of the editor. */
  pagesGatingErrorPanelVisible(): Promise<boolean>;
  /** Heading, body, and back-link href of the detail error panel. */
  pagesGatingErrorPanel(): Promise<PagesGatingErrorPanel>;
  /** Which variant the list content area currently shows. */
  pagesGatingListVariant(): Promise<PagesGatingListVariant>;
  /** Count of skeleton blocks in the list content area. */
  pagesGatingSkeletonCount(): Promise<number>;
  /** State of the empty-collection create action (absent, enabled, or disabled). */
  pagesGatingEmptyCreateState(): Promise<PagesGatingCreateState>;
  /** Hold collection GETs behind a gate until released (skeleton proof). */
  pagesGatingStallListFetch(): Promise<void>;
  /** Release held collection GETs. */
  pagesGatingReleaseListFetch(): Promise<void>;
  /** Fail collection GETs with 500 until lifted (error fall-through proof). */
  pagesGatingFailListFetch(): Promise<void>;
  /** Lift the collection failure. */
  pagesGatingUnfailListFetch(): Promise<void>;
  /** Fail page POSTs with 500 until lifted (failed-mutation notice proof). */
  pagesGatingFailListCreates(): Promise<void>;
  /** Lift the create failure. */
  pagesGatingUnfailListCreates(): Promise<void>;
  /** Visible toast texts (title plus message per toast). */
  pagesGatingToastTexts(): Promise<string[]>;
  /** Start logging page-API requests (collection, detail, favorites, shelved). */
  pagesGatingStartRequestSpy(): Promise<void>;
  /** Logged requests since the spy started or was last reset. */
  pagesGatingLoggedRequests(): Promise<PagesGatingRequest[]>;
  /** Clear the logged requests. */
  pagesGatingResetRequestSpy(): Promise<void>;
  /** Type into the list title search (expands the control first); resolves once rows settle. */
  pagesGatingSearchType(text: string): Promise<void>;
  /** Current search text, or null when the control is collapsed. */
  pagesGatingSearchValue(): Promise<string | null>;
  /** Current sort key label. */
  pagesGatingSortLabel(): Promise<string>;
  /** Switch the sort to a different key; resolves once rows settle. */
  pagesGatingSortCycle(): Promise<void>;
  /** Toggle the favorites-only structured filter; resolves once rows settle. */
  pagesGatingFilterToggleFavorites(): Promise<void>;
  /** Active filter chip texts (empty when no filter applies). */
  pagesGatingChips(): Promise<string[]>;
  /** Click quick-create; resolves with the new page id once its detail lands. */
  pagesGatingQuickCreate(): Promise<string>;
  /** Wait for the detail terminal state: the editor body or the error panel. */
  pagesGatingWaitDetailSettled(): Promise<"editor" | "panel">;
  /** True when the centered full-view spinner shows on the detail view. */
  pagesGatingDetailSpinnerVisible(): Promise<boolean>;
  /** Hold one page's detail GETs behind a gate until released (spinner proof). */
  pagesGatingStallDetailFetch(pageId: string): Promise<void>;
  /** Release held detail GETs. */
  pagesGatingReleaseDetailFetch(): Promise<void>;
  /** Project crumb text of the detail breadcrumb cluster. */
  pagesGatingBreadcrumbProject(): Promise<string | null>;
  /** Href of the breadcrumb back-to-list link. */
  pagesGatingBackLinkHref(): Promise<string | null>;
  /** Sibling-page switcher option names, in display order (proves the preloaded list). */
  pagesGatingSwitcherOptions(): Promise<string[]>;
  /** Switcher option names after filtering by text. */
  pagesGatingSwitcherSearch(text: string): Promise<string[]>;
  /** Select one switcher sibling; resolves once its detail lands. */
  pagesGatingSwitcherSelect(name: string): Promise<void>;
  /** Title field text, placeholder, and rendered height for one page. */
  pagesGatingTitleFacts(pageId: string): Promise<PagesGatingTitleFacts>;
  /** True when body skeleton blocks show in the detail canvas column. */
  pagesGatingBodySkeletonVisible(): Promise<boolean>;
  /** Name of the selected navigation-pane tab, or null when the pane is closed. */
  pagesGatingPaneActiveTab(): Promise<string | null>;
  /** Click one version-timeline link; resolves once the snapshot overlay opens. */
  pagesGatingClickVersionLink(versionId: string): Promise<void>;
  /** True when the snapshot overlay shows (loaded or errored). */
  pagesGatingOverlayOpen(): Promise<boolean>;
  /** Snapshot overlay header title. */
  pagesGatingOverlayTitle(): Promise<string>;
  /** Snapshot overlay body text. */
  pagesGatingOverlayText(): Promise<string>;
  /** True when the overlay shows skeleton blocks while its snapshot loads. */
  pagesGatingOverlaySkeletonVisible(): Promise<boolean>;
  /** Hold version-detail GETs behind a gate until released (overlay-skeleton proof). */
  pagesGatingStallVersionDetail(): Promise<void>;
  /** Release held version-detail GETs. */
  pagesGatingReleaseVersionDetail(): Promise<void>;
  /** Click one detail options-menu item by name. */
  pagesGatingClickOptionsItem(name: string): Promise<void>;
  /** Click the header copy-link control; resolves with the clipboard text. */
  pagesGatingCopyLinkText(): Promise<string>;
  /** Sign out deterministically by clearing cookies (role-switch aid; follow with openEntry plus sign-in). */
  pagesGatingSignOut(): Promise<void>;
  /** Click quick-create without waiting for navigation (failed-mutation proof clicks this, then reads toasts). */
  pagesGatingClickCreate(): Promise<void>;
  /** Open one row through its link (client-side navigation; the list cache survives for back-navigation proofs). */
  pagesGatingOpenRow(title: string): Promise<void>;
  /** Go back to the list (client-side when the row opened it that way); settles the list. */
  pagesGatingGoBack(): Promise<void>;

  // --- Pages list discovery (NEWFRONT-312, PAGE-001-006).
  /** Open the project pages list; resolves once list, gate, or empty state settles. */
  pagesListOpenProjectPages(workspaceSlug: string, projectId: string, query?: string): Promise<void>;
  /** Whether the feature-off guidance shows instead of the list. */
  pagesListGateVisible(): Promise<boolean>;
  /** Whether the gate's management shortcut is enabled (project admins). */
  pagesListGateManageEnabled(): Promise<boolean>;
  /** Follow the gate's management shortcut; resolves once project features show. */
  pagesListGateOpenManage(): Promise<void>;
  /** Whether the visibility tab strip renders. */
  pagesListTabsVisible(): Promise<boolean>;
  /** Visible tab labels in order, or [] when the strip is absent. */
  pagesListTabs(): Promise<string[]>;
  /** Active tab key from the URL (`public` when the value is missing or unknown). */
  pagesListActiveTab(): Promise<PagesListTab>;
  /** Tab whose link styling differs from its siblings, or null when none does. */
  pagesListMarkedTab(): Promise<PagesListTab | null>;
  /** Href of one tab link, or null when the strip is absent. */
  pagesListTabHref(tab: PagesListTab): Promise<string | null>;
  /** Click one tab; resolves once its rows settle. */
  pagesListClickTab(tab: PagesListTab): Promise<void>;
  /** Row titles in display order. */
  pagesListRowTitles(): Promise<string[]>;
  /** Open the collapsed search control; resolves once the input shows. */
  pagesListSearchOpen(): Promise<void>;
  /** Whether the search input is currently expanded. */
  pagesListSearchExpanded(): Promise<boolean>;
  /** Type into the expanded search input (replaces content). */
  pagesListSearchType(text: string): Promise<void>;
  /** Current search input value, or null when collapsed. */
  pagesListSearchValue(): Promise<string | null>;
  /** Press Escape in the search input. */
  pagesListSearchEscape(): Promise<void>;
  /** Click the expanded search's clear control. */
  pagesListSearchClear(): Promise<void>;
  /** Click the active tab (outside the search input) without navigating. */
  pagesListClickAway(): Promise<void>;
  /** Whether the no-results state shows. */
  pagesListNoResultsVisible(): Promise<boolean>;
  /** Current sort control state (key label plus direction). */
  pagesListSortState(): Promise<{ label: string; descending: boolean }>;
  /** Open the sort menu; resolves once its options render. */
  pagesListSortOpen(): Promise<void>;
  /** Visible sort menu option labels, in order. */
  pagesListSortOptions(): Promise<string[]>;
  /** Choose one sort key; resolves once rows re-settle. */
  pagesListSortChooseKey(label: string): Promise<void>;
  /** Choose one sort direction; resolves once rows re-settle (no-op when current). */
  pagesListSortChooseDirection(label: string): Promise<void>;
  /** Open the filters panel; resolves once its sections render. */
  pagesListFiltersOpen(): Promise<void>;
  /** Close the filters panel. */
  pagesListFiltersClose(): Promise<void>;
  /** Toggle the favorites-only option. */
  pagesListFilterToggleFavorites(): Promise<void>;
  /** Toggle one created-date option by label. */
  pagesListFilterChooseDate(label: string): Promise<void>;
  /** Search the filters panel (narrows creator options). */
  pagesListFilterSearch(text: string): Promise<void>;
  /** Creator option labels currently offered, in order. */
  pagesListFilterCreatorOptions(): Promise<string[]>;
  /** Pick one creator option by label. */
  pagesListFilterPickCreator(label: string): Promise<void>;
  /** Applied-filter chip texts in order, or [] when none render. */
  pagesListChips(): Promise<string[]>;
  /** Remove one applied chip by its key label; resolves once rows re-settle. */
  pagesListChipRemove(keyLabel: string): Promise<void>;
  /** Clear all applied filters; resolves once rows re-settle. */
  pagesListClearAll(): Promise<void>;
  /** Row anatomy snapshot for one title (owner, visibility, date, star, menu). */
  pagesListRowDetail(title: string): Promise<PagesListRowDetail>;
  /** Click one row; resolves once its detail shows. */
  pagesListClickRow(title: string): Promise<void>;
  /** Dismiss any open menu. */
  pagesListDismissMenus(): Promise<void>;
  /** Overflow menu item labels for one row, in order. */
  pagesListRowMenuItems(title: string): Promise<string[]>;
  /** Right-click one row; resolves once the context menu shows. */
  pagesListRowRightClick(title: string): Promise<void>;
  /** Visible context-menu item labels, in order. */
  pagesListContextMenuItems(): Promise<string[]>;

  // --- Pages create, star, row actions, move (NEWFRONT-313, PAGE-007-017).
  // Minimal list navigation lives here under pagesRow* names; the full
  // list surface (tabs, search, sort, filters, row rendering) belongs to
  // NEWFRONT-312 and the detail mirrors to NEWFRONT-318 — duplicated
  // minimal readers converge at rebase.

  /** Open the project pages list, optionally on one tab; resolves once rows settle. */
  pagesRowOpenList(workspaceSlug: string, projectId: string, tab?: PagesRowTab): Promise<void>;
  /** Open one page's detail; resolves once the canvas or the error panel shows. */
  pagesRowOpenDetail(workspaceSlug: string, projectId: string, pageId: string): Promise<void>;
  /** Visible row titles in display order. */
  pagesRowTitles(): Promise<string[]>;
  /** Active list tab from the URL query, or null when the strip is absent. */
  pagesRowActiveTab(): Promise<PagesRowTab | null>;
  /** Switch the list tab by clicking the strip; resolves once rows settle. */
  pagesRowSelectTab(tab: PagesRowTab): Promise<void>;
  /** Whether the header quick-create control renders for the current role. */
  pagesCreateQuickVisible(): Promise<boolean>;
  /** Click quick-create; resolves with the new page id once its detail lands. */
  pagesCreateQuick(): Promise<string>;
  /** Fail the next page-creation POST with `status` (once); error-path aid. */
  pagesCreateFailNextCreate(status?: number): Promise<void>;
  /** Open the form-based creation dialog through the palette; resolves once it shows. */
  pagesCreateOpenDialog(): Promise<void>;
  /** Whether the creation dialog is showing. */
  pagesCreateDialogVisible(): Promise<boolean>;
  /** Fill the dialog title field. */
  pagesCreateDialogSetTitle(text: string): Promise<void>;
  /** Current dialog title value. */
  pagesCreateDialogTitleValue(): Promise<string>;
  /** Whether the dialog shows its over-length message. */
  pagesCreateDialogOverLengthVisible(): Promise<boolean>;
  /** Whether the dialog submit control is enabled. */
  pagesCreateDialogSubmitEnabled(): Promise<boolean>;
  /** Visibility the dialog currently offers. */
  pagesCreateDialogAccess(): Promise<"open" | "restricted">;
  /** Choose the dialog visibility. */
  pagesCreateDialogSetAccess(which: "open" | "restricted"): Promise<void>;
  /** Pick the dialog mark by choosing the first icon entry; resolves once applied. */
  pagesCreateDialogPickIconMark(): Promise<void>;
  /** Whether the dialog currently carries a mark. */
  pagesCreateDialogHasMark(): Promise<boolean>;
  /** Cancel the dialog; resolves once it closes. */
  pagesCreateDialogCancel(): Promise<void>;
  /** Submit the dialog; resolves with the new page id once its detail lands. */
  pagesCreateDialogSubmit(): Promise<string>;
  /** Whether the row's star renders for the current role. */
  pagesRowStarVisible(title: string): Promise<boolean>;
  /** Whether the row's star shows the favorite state. */
  pagesRowStarSelected(title: string): Promise<boolean>;
  /** Toggle the row's star; resolves once the star settles. */
  pagesRowToggleStar(title: string): Promise<void>;
  /** Apply the favorites-only list filter (minimal aid; full filters are NEWFRONT-312). */
  pagesRowFilterFavoritesOnly(): Promise<void>;
  /** Clear the list filters back to the unfiltered rows (minimal aid). */
  pagesRowClearFilters(): Promise<void>;
  /** Whether the favorites navigation section currently stands expanded. */
  pagesRowFavoritesNavOpen(): Promise<boolean>;
  /** Open one row's overflow menu; resolves once its items show. */
  pagesRowOpenMenu(title: string): Promise<void>;
  /** Open one row's context menu; resolves once its items show. */
  pagesRowOpenContextMenu(title: string): Promise<void>;
  /** Visible titles of the currently open row/detail menu, in order. */
  pagesRowMenuItems(): Promise<string[]>;
  /** Click the open menu's item by title; resolves once the menu closes. */
  pagesRowMenuClick(item: string): Promise<void>;
  /** Run the row's new-tab action; resolves with the opened URL (popup closed). */
  pagesRowOpenInNewTab(title: string): Promise<string>;
  /** Run the row's copy-link action; resolves once the notice shows. */
  pagesRowCopyLink(title: string): Promise<void>;
  /** Run the row's duplicate action; resolves once the notice shows. */
  pagesRowDuplicate(title: string): Promise<void>;
  /** Run the row's visibility toggle; resolves once the notice shows. */
  pagesRowToggleAccess(title: string): Promise<void>;
  /** Run the row's lock toggle; resolves once the notice shows. */
  pagesRowToggleLock(title: string): Promise<void>;
  /** Run the row's shelve/restore toggle; resolves once the notice shows. */
  pagesRowToggleArchive(title: string): Promise<void>;
  /** Whether the delete confirmation dialog is showing. */
  pagesRowDeleteModalVisible(): Promise<boolean>;
  /** Full text of the delete confirmation dialog. */
  pagesRowDeleteModalText(): Promise<string>;
  /** Whether the delete confirm control currently shows its busy state. */
  pagesRowDeleteConfirmBusy(): Promise<boolean>;
  /** Cancel the delete dialog; resolves once it closes. */
  pagesRowDeleteModalCancel(): Promise<void>;
  /** Confirm the delete dialog; resolves once it closes. */
  pagesRowDeleteModalConfirm(): Promise<void>;
  /** Delay the next page DELETE by `ms` (once); busy-state aid. */
  pagesRowSlowNextDelete(ms?: number): Promise<void>;
  /** Open the detail view's options menu; resolves once its items show. */
  pagesRowDetailOpenOptions(): Promise<void>;
  /** Whether the detail view currently renders in its read-only state. */
  pagesRowDetailReadOnly(): Promise<boolean>;
  /** Whether a move affordance renders in the detail header for this page. */
  pagesRowDetailMoveVisible(): Promise<boolean>;
  /** Whether the detail view shows its not-found state for a missing page. */
  pagesRowDetailNotFoundVisible(): Promise<boolean>;
  /** Follow the not-found view's recovery link back to the pages list. */
  pagesRowDetailNotFoundBackToList(): Promise<void>;

  // --- Estimates enable switch, active/archived display, row gating
  // --- (NEWFRONT-322, EST-004-007). Appended; existing methods above are
  // --- untouched per the shared driver contract. Navigation reuses the
  // --- shell child's estimatesOpenSettings/estimatesNotAuthorized
  // --- (NEWFRONT-321, merged); the switch-visibility, active-name,
  // --- active-row and confirm-dialog readers reuse the permissions
  // --- child's mirrors (NEWFRONT-326, merged).
  /** Whether the enable switch currently shows the on position. */
  estimatesEnableSwitchOn(): Promise<boolean>;
  /** Whether the enable switch is disabled for the signed-in viewer. */
  estimatesEnableSwitchDisabled(): Promise<boolean>;
  /** Flip the enable switch; resolves once the toggle request settles. */
  estimatesEnableSwitchToggle(): Promise<void>;
  /** Abort project PATCH/PUT writes for the rest of the test (the EST-004 failure path). */
  estimatesFailProjectUpdate(): Promise<void>;
  /** Joined value preview of the active row, or "" when no active row shows. */
  estimatesActiveSystemValues(): Promise<string>;
  /** Whether the archived-estimates section renders. */
  estimatesArchivedSectionVisible(): Promise<boolean>;
  /** Names of the systems listed in the archived section, in display order. */
  estimatesArchivedNames(): Promise<string[]>;
  /** Explainer text of the archived section, or "" when the section is absent. */
  estimatesArchivedExplainerText(): Promise<string>;
  /** Href of the archived section's outbound reference link, or "" when absent. */
  estimatesArchivedLinkHref(): Promise<string>;
  /** Target of the archived section's outbound reference link, or "" when absent. */
  estimatesArchivedLinkTarget(): Promise<string>;
  /** Click the archived outbound link; resolves with the newly opened page's URL. */
  estimatesArchivedOpenLinkNewPage(): Promise<string>;
  /** Buttons rendered on the archived row `name` (read-only rows carry none). */
  estimatesArchivedRowButtonCount(name: string): Promise<number>;

  // --- Modules list states, layouts, rows and cards (NEWFRONT-307,
  // --- MOD-008..012, MOD-020). Appended; existing methods above are
  // --- untouched per the shared driver contract.

  /** Open the live modules list; ends settled (rows, zero state, or no-match). */
  modulesListOpenList(workspaceSlug: string, projectId: string): Promise<void>;
  /** Row names in the list layout (empty in other layouts). */
  modulesListVisibleNames(): Promise<string[]>;
  /** Card names in the board layout (empty in other layouts). */
  modulesListBoardCardNames(): Promise<string[]>;
  /** Module names in the timeline layout (empty in other layouts). */
  modulesListTimelineNames(): Promise<string[]>;
  /** Which list-area state renders once settled. */
  modulesListEmptyKind(): Promise<ModulesListEmptyKind>;
  /** No-match heading text, or null when the no-match view is absent. */
  modulesListNoMatchHeading(): Promise<string | null>;
  /** No-match hint text, or null when the no-match view is absent. */
  modulesListNoMatchHint(): Promise<string | null>;
  /** Whether a list loading skeleton currently shows. */
  modulesListSkeletonVisible(): Promise<boolean>;
  /**
   * Hold the list fetch on `layout`, reporting whether the skeleton
   * showed, matched that layout, and showed exclusively (no rows and no
   * empty state alongside). Releases the hold and ends settled.
   */
  modulesListSkeletonShownOnSlowFetch(
    workspaceSlug: string,
    projectId: string,
    layout: ModulesListLayout
  ): Promise<ModulesListSkeletonResult>;
  /** Whether the zero-modules onboarding view renders. */
  modulesListZeroStateVisible(): Promise<boolean>;
  /** Whether the zero-state artwork renders. */
  modulesListZeroStateArtVisible(): Promise<boolean>;
  /** Zero-state title and explanation text. */
  modulesListZeroStateCopy(): Promise<ModulesListZeroStateCopy>;
  /** Whether the zero-state creation action is disabled. */
  modulesListZeroStateCreateDisabled(): Promise<boolean>;
  /** Activate the zero-state creation action; ends with the create dialog open. */
  modulesListZeroStateCreateOpen(): Promise<void>;
  /** Whether the module create dialog is open. */
  modulesListCreateDialogVisible(): Promise<boolean>;
  /** Fill the list search with text that matches nothing; ends with no-match shown. */
  modulesListNoMatchShowViaSearch(text: string): Promise<void>;
  /** Clear the list search; ends settled with rows. */
  modulesListNoMatchClearSearch(): Promise<void>;
  /** Number of options the desktop layout switcher offers. */
  modulesListLayoutOptionCount(): Promise<number>;
  /** Which layout the switcher marks active. */
  modulesListActiveLayout(): Promise<ModulesListLayout>;
  /** Whether the desktop layout switcher renders (hidden on small screens). */
  modulesListSwitcherVisible(): Promise<boolean>;
  /** Switch layout through the desktop switcher; ends settled in that layout. */
  modulesListSwitchLayout(layout: ModulesListLayout): Promise<void>;
  /** Reload the list; ends settled. */
  modulesListReload(): Promise<void>;
  /** Whether the small-screen layout menu trigger renders. */
  modulesListCompactMenuVisible(): Promise<boolean>;
  /** Option labels of the small-screen layout menu. */
  modulesListCompactMenuOptions(): Promise<string[]>;
  /** Switch layout through the small-screen menu; ends settled in that layout. */
  modulesListCompactSwitchLayout(layout: "list" | "board"): Promise<void>;
  /** Row progress marker: full-completion check, finished-but-short alert, or percent. */
  modulesListRowProgress(name: string): Promise<ModulesListRowProgress>;
  /** Detail href of a list row. */
  modulesListRowDetailHref(name: string): Promise<string>;
  /** Open the peek panel through a row's info control; ends with the peek open. */
  modulesListRowOpenPeek(name: string): Promise<void>;
  /** Peek panel module name, or null when no peek panel is open. */
  modulesListPeekName(): Promise<string | null>;
  /** Close the peek panel; ends with it closed. */
  modulesListPeekClose(): Promise<void>;
  /** Whether a row's quick menu trigger renders (small screens only). */
  modulesListRowMenuVisible(name: string): Promise<boolean>;
  /**
   * Click an archived row on the archived-modules tab; ends with its
   * peek panel open on the same tab (no navigation to detail).
   */
  modulesListArchivedRowClickOpensPeek(name: string): Promise<void>;
  /** Whether a board card's name truncates. */
  modulesListCardNameTruncated(name: string): Promise<boolean>;
  /** Tooltip text of a board card's name, or null when no tooltip shows. */
  modulesListCardNameTooltip(name: string): Promise<string | null>;
  /** Status pill text of a board card. */
  modulesListCardStatusText(name: string): Promise<string>;
  /** Open the peek panel through a card's info control; ends with the peek open. */
  modulesListCardOpenPeek(name: string): Promise<void>;
  /** Work-item count line of a board card. */
  modulesListCardIssueCount(name: string): Promise<string>;
  /** Whether a board card shows a lead avatar or the no-lead marker. */
  modulesListCardLeadKind(name: string): Promise<ModulesListCardLead>;
  /** Non-zero progress segments of a board card (widths and colors). */
  modulesListCardProgressSegments(name: string): Promise<ModulesListCardSegments>;
  /** Whether a board card's date-range control renders. */
  modulesListCardDatesVisible(name: string): Promise<boolean>;
  /** Whether a board card's favorite control renders. */
  modulesListCardFavoriteVisible(name: string): Promise<boolean>;
  /** Whether a board card's quick menu trigger renders. */
  modulesListCardQuickMenuVisible(name: string): Promise<boolean>;
  /** Click a card's body; ends on the module detail page. */
  modulesListCardClickBody(name: string): Promise<void>;
  /**
   * Click one card control; ends with the list path unchanged and any
   * opened popover dismissed.
   */
  modulesListCardClickControl(name: string, control: ModulesListCardControl): Promise<void>;

  /** Refuse whole-system deletes behind the dialog's back (failure-path spy). */
  estimatesEditDeleteFailEstimateDelete(): Promise<void>;

  // --- Pages blocks/embeds/mentions/links (NEWFRONT-316, PAGE-037-044).
  /** Open a page detail and wait for the live body editor. */
  pagesBlocksOpenDetail(workspaceSlug: string, projectId: string, pageId: string): Promise<void>;
  /** Open a page detail with a query string and wait for the live body editor. */
  pagesBlocksOpenDetailQuery(workspaceSlug: string, projectId: string, pageId: string, query: string): Promise<void>;
  /** Reload the current page detail and wait for the live body editor. */
  pagesBlocksReload(): Promise<void>;
  /** Visible text of the live body editor (title excluded). */
  pagesBlocksEditorText(): Promise<string>;
  /** Inner HTML of the live body editor (title excluded). */
  pagesBlocksEditorHtml(): Promise<string>;
  /** Click the last body block and move the caret to its end. */
  pagesBlocksEditorClickEnd(): Promise<void>;
  /** Type text into the focused body editor. */
  pagesBlocksEditorType(text: string): Promise<void>;
  /** Press a key or chord in the body editor (Escape, End, ControlOrMeta+e, ...). */
  pagesBlocksEditorPressKey(chord: string): Promise<void>;
  /** Press Enter in the body editor. */
  pagesBlocksEditorPressEnter(): Promise<void>;
  /** Press Backspace in the body editor. */
  pagesBlocksEditorPressBackspace(): Promise<void>;
  /** Dispatch a synthetic text/html paste into the body editor. */
  pagesBlocksEditorPasteHtml(html: string): Promise<void>;
  /** Paste plain text through the real clipboard into the focused editor. */
  pagesBlocksEditorPasteText(text: string): Promise<void>;
  /** Select all document content (ControlOrMeta+a). */
  pagesBlocksEditorSelectAll(): Promise<void>;
  /** Copy the current selection (ControlOrMeta+c). */
  pagesBlocksEditorCopy(): Promise<void>;
  /** Paste the clipboard at the caret (ControlOrMeta+v). */
  pagesBlocksEditorPaste(): Promise<void>;
  /** Select the text of the first body paragraph containing `text`. */
  pagesBlocksParagraphSelectText(text: string): Promise<void>;
  /** Whether the slash-command palette is currently open. */
  pagesBlocksSlashOpen(): Promise<boolean>;
  /** Open the palette on a fresh line and read its entry titles, in order. */
  pagesBlocksSlashItems(): Promise<string[]>;
  /** Open the palette below the caret and choose `entry`; `occurrence` picks among duplicate titles. */
  pagesBlocksSlashChoose(entry: string, occurrence?: number): Promise<void>;
  /** Dismiss the slash palette if open. */
  pagesBlocksSlashDismiss(): Promise<void>;
  /** Open the emoji picker through the slash Emoji entry. */
  pagesBlocksEmojiOpen(): Promise<void>;
  /** Visible emoji picker row texts. */
  pagesBlocksEmojiItems(): Promise<string[]>;
  /** Choose the emoji picker row at `index`; resolves once inserted. */
  pagesBlocksEmojiChoose(index: number): Promise<void>;
  /** Hover the last content block; whether its drag handle shows. */
  pagesBlocksDragHandleVisible(): Promise<boolean>;
  /** Open the block menu for the last content block; resolves once open. */
  pagesBlocksDragOpenMenu(): Promise<void>;
  /** Visible block-menu item labels, in order. */
  pagesBlocksDragMenuItems(): Promise<string[]>;
  /** Choose a block-menu item; resolves once applied. */
  pagesBlocksDragMenuChoose(label: string): Promise<void>;
  /** Upload an image through the editor's image slash entry (file bytes). */
  pagesBlocksUploadImage(file: { name: string; mime: string; bytes: Buffer }): Promise<void>;
  /** Inline images in the body editor (src + alt). */
  pagesBlocksInlineImages(): Promise<PagesBlocksInlineImage[]>;
  /** Navigate to the navigation pane's assets tab. */
  pagesBlocksAssetsTabOpen(): Promise<void>;
  /** Asset rows in the assets tab (name + kind). */
  pagesBlocksAssetsTabRows(): Promise<PagesBlocksAssetRow[]>;
  /** Whether the assets tab shows its empty state. */
  pagesBlocksAssetsTabEmptyVisible(): Promise<boolean>;
  /** Whether a work-item embed node or upgrade card renders in the body. */
  pagesBlocksEmbedUpgradeVisible(): Promise<boolean>;
  /** Outbound link of the work-item upgrade card, or null when absent. */
  pagesBlocksEmbedUpgradeHref(): Promise<string | null>;
  /** Type a mention query and read the suggestion row texts. */
  pagesBlocksMentionSuggest(query: string): Promise<string[]>;
  /** Suggestion section titles above the mention rows. */
  pagesBlocksMentionSections(): Promise<string[]>;
  /** Choose the suggestion containing `displayName`; resolves once the chip inserts. */
  pagesBlocksMentionChoose(displayName: string): Promise<void>;
  /** Mention chips in the body editor (rendered text). */
  pagesBlocksMentionChips(): Promise<string[]>;
  /** Mention chip profile links, in chip order. */
  pagesBlocksMentionChipHrefs(): Promise<string[]>;
  /** Select the first body link's text (opens the formatting bubble). */
  pagesBlocksLinkSelect(): Promise<void>;
  /** Whether the formatting bubble's Link entry is visible. */
  pagesBlocksLinkBubbleVisible(): Promise<boolean>;
  /** Prefilled address of the bubble link input (opens the input). */
  pagesBlocksLinkPrefill(): Promise<string>;
  /** Rewrite the selected link's address in place; resolves once applied. */
  pagesBlocksLinkEdit(href: string): Promise<void>;
  /** Remove the selected link, keeping its text; resolves once applied. */
  pagesBlocksLinkRemove(): Promise<void>;
  /** Href of the first body link, or null when the body has none. */
  pagesBlocksLinkHref(): Promise<string | null>;
  /** Click the formatting bubble's icon button at `index` (0-based). */
  pagesBlocksBubbleFormat(index: number): Promise<void>;
  /** Open the detail header options menu. */
  pagesBlocksOptionsOpen(): Promise<void>;
  /** Options menu item titles, in order (opens and dismisses the menu). */
  pagesBlocksOptionsItems(): Promise<string[]>;
  /** Click the Copy-markdown entry; resolves once the toast fires. */
  pagesBlocksOptionsCopyMarkdown(): Promise<void>;
  /** Most recent toast text, or null when none is visible. */
  pagesBlocksToastText(): Promise<string | null>;
  /** Current clipboard text via a granted clipboard read. */
  pagesBlocksClipboardText(): Promise<string>;
  /** Lock then unlock the open page via the header control; leaves it unlocked. */
  pagesBlocksHeaderLockCycle(): Promise<void>;
  /** Info-tab document stats (navigates to the tab first unless told to skip). When given, waits for the Characters card to reach the count. */
  pagesBlocksInfoStats(expectedCharacters?: number, skipNavigate?: boolean): Promise<PagesBlocksInfoStats>;

  // --- NEWFRONT-310 (module detail/analytics/links, MOD-029-035).
  // --- Appended; existing methods above are untouched per the shared
  // --- driver contract. Layout switching itself reuses the layouts*
  // --- methods (Issues area), empty states reuse layoutsEmpty* plus
  // --- the add-existing picker methods, and the peek entry reuses
  // --- archivesOpenLiveModulePeek; these cover the module-detail host:
  // --- breadcrumb plus switcher, badge, header actions, the analytics
  // --- sidebar (fields, progress, links) wherever it renders.

  /** Module name shown in the analytics sidebar, or null when absent. */
  modulesDetailSidebarName(): Promise<string | null>;
  /** Header work-item count badge text, or null when no badge renders. */
  modulesDetailBadgeText(): Promise<string | null>;
  /** Explanatory tooltip of the count badge, or null when none shows. */
  modulesDetailBadgeTooltip(): Promise<string | null>;
  /** Browser tab title of the module detail page. */
  modulesDetailPageTitle(): Promise<string>;
  /** Breadcrumb labels in display order (project, Modules, switcher). */
  modulesDetailCrumbs(): Promise<string[]>;
  /** Follow the Modules crumb; the caller polls the address. */
  modulesDetailClickModulesCrumb(): Promise<void>;
  /** Follow the project crumb; the caller polls the address. */
  modulesDetailClickProjectCrumb(projectName: string): Promise<void>;
  /** Open the header module switcher; resolves once options render. */
  modulesDetailSwitcherOpen(): Promise<void>;
  /** Module names the open switcher offers, in display order. */
  modulesDetailSwitcherOptions(): Promise<string[]>;
  /** Search the open switcher; resolves once options settle. */
  modulesDetailSwitcherSearch(text: string): Promise<void>;
  /** Pick a module in the open switcher; resolves once the header shows it. */
  modulesDetailSwitcherPick(name: string): Promise<void>;
  /** Unknown-module empty-state title, or null when the page renders. */
  modulesDetailUnknownTitle(): Promise<string | null>;
  /** Follow the unknown-module empty state back to the modules list. */
  modulesDetailUnknownBack(): Promise<void>;
  /**
   * Switch layouts through the desktop header switcher; resolves once
   * the header marker shows the layout (marker wait: the sidebar's own
   * labels trip the shared container heuristics, same as cycles).
   */
  modulesDetailSwitchLayout(layout: LayoutsLayoutKey): Promise<void>;
  /** Whether the work-item filter toggle renders in the header. */
  modulesDetailFilterToggleVisible(): Promise<boolean>;
  /** Group-by entries of the open Display panel with disabled states. */
  modulesDetailDisplayGroupedOptions(): Promise<ModulesDetailGroupedOption[]>;
  /** Layout keys the small-screen layout menu offers, in display order. */
  modulesDetailMobileOfferedLayouts(): Promise<LayoutsLayoutKey[]>;
  /** Switch layout through the small-screen menu; resolves once applied. */
  modulesDetailMobileSwitchTo(layout: LayoutsLayoutKey): Promise<void>;
  /** Whether the small-screen bar offers Display. */
  modulesDetailMobileDisplayVisible(): Promise<boolean>;
  /** Whether the small-screen bar offers a filter toggle. */
  modulesDetailMobileFilterToggleVisible(): Promise<boolean>;
  /** Whether the header work-item creation button is visible. */
  modulesDetailHeaderAddVisible(): Promise<boolean>;
  /** Click the header work-item creation button. */
  modulesDetailHeaderAddClick(): Promise<void>;
  /** Fail module-issues add writes on this page (failure-notice proof). */
  modulesDetailFailNextIssuesAdd(): Promise<void>;
  /** Search the open add-existing picker; resolves once options settle. */
  modulesDetailPickerSearch(text: string): Promise<void>;
  /** Whether the header Analytics shortcut is visible. */
  modulesDetailAnalyticsVisible(): Promise<boolean>;
  /** Open the Analytics dialog; resolves once it renders. */
  modulesDetailOpenAnalytics(): Promise<void>;
  /** Whether the Analytics dialog is currently rendered. */
  modulesDetailAnalyticsDialogVisible(): Promise<boolean>;
  /** Dismiss the Analytics dialog; resolves once it detaches. */
  modulesDetailDismissAnalytics(): Promise<void>;
  /** Click the header sidebar toggle; resolves once the click lands. */
  modulesDetailToggleSidebar(): Promise<void>;
  /** Whether the analytics sidebar currently renders. */
  modulesDetailSidebarVisible(): Promise<boolean>;
  /** Stored sidebar-collapsed flag for this browser (persistence proof). */
  modulesDetailSidebarCollapsedStored(): Promise<boolean>;
  /** Click the sidebar's own close control; resolves once the click lands. */
  modulesDetailCloseSidebarViaPanel(): Promise<void>;
  /** Open Analytics from the small-screen bar; resolves once it renders. */
  modulesDetailMobileOpenAnalytics(): Promise<void>;
  /** Sidebar status pill text, or null when the sidebar is absent. */
  modulesDetailSidebarStatus(): Promise<string | null>;
  /** Pick a sidebar status; resolves once the pill shows it. */
  modulesDetailSidebarSetStatus(status: string): Promise<void>;
  /** Sidebar description text, or null when none renders. */
  modulesDetailSidebarDescription(): Promise<string | null>;
  /** Sidebar date control text (the rendered range). */
  modulesDetailSidebarDateText(): Promise<string>;
  /** Whether the sidebar date control refuses edits. */
  modulesDetailSidebarDateDisabled(): Promise<boolean>;
  /** Open the sidebar date picker; resolves once the calendar renders. */
  modulesDetailSidebarOpenDatePicker(): Promise<void>;
  /** Pick one calendar day by its accessible date label. */
  modulesDetailPickDateDay(label: string): Promise<void>;
  /** Dismiss the sidebar date picker; resolves once it detaches. */
  modulesDetailDismissDatePicker(): Promise<void>;
  /** Sidebar lead row value (live: avatar initial plus placeholder), or null when none renders. */
  modulesDetailSidebarLead(): Promise<string | null>;
  /** Pick the sidebar lead; resolves once the row shows them. */
  modulesDetailSidebarSetLead(name: string): Promise<void>;
  /** Sidebar member avatar initials in display order (live shows no names). */
  modulesDetailSidebarMembers(): Promise<string[]>;
  /** Add one sidebar member; resolves once the row shows them. */
  modulesDetailSidebarAddMember(name: string): Promise<void>;
  /** Whether the sidebar status control refuses edits. */
  modulesDetailSidebarStatusDisabled(): Promise<boolean>;
  /** Whether the sidebar lead control refuses edits. */
  modulesDetailSidebarLeadDisabled(): Promise<boolean>;
  /** Whether the sidebar members control refuses edits. */
  modulesDetailSidebarMembersDisabled(): Promise<boolean>;
  /** Sidebar Work-items count text (e.g. "0/3", "0 work items"). */
  modulesDetailSidebarWorkItemsText(): Promise<string>;
  /** Sidebar Points count text, or null when the row is absent. */
  modulesDetailSidebarPointsText(): Promise<string | null>;
  /** Reload with the detail fetch delayed; true when the loader shows. */
  modulesDetailSlowFetchShowsLoader(): Promise<boolean>;
  /** Whether the progress section renders expanded. */
  modulesDetailProgressOpen(): Promise<boolean>;
  /** Progress header percent, or null when no badge renders. */
  modulesDetailProgressHeaderPercent(): Promise<number | null>;
  /** Progress inactive/invalid notice, or null when the chart branch renders. */
  modulesDetailProgressNotice(): Promise<string | null>;
  /** Whether the sidebar progress chart renders. */
  modulesDetailProgressChartVisible(): Promise<boolean>;
  /** Current plot-measure label, or null when the dropdown is absent. */
  modulesDetailProgressMeasureValue(): Promise<string | null>;
  /** Plot-measure options in display order (absent dropdown reads empty). */
  modulesDetailProgressMeasureOptions(): Promise<string[]>;
  /** Pick a plot measure; resolves with the module detail GETs it fired. */
  modulesDetailMeasureSwitchGetCount(label: string): Promise<number>;
  /** Open a progress breakdown tab; resolves once it selects. */
  modulesDetailProgressStatsTab(tab: "States" | "Assignees" | "Labels"): Promise<void>;
  /** Title of the currently selected breakdown tab. */
  modulesDetailActiveStatsTab(): Promise<string>;
  /** Rows of the selected breakdown tab (title, percent, total). */
  modulesDetailProgressStatsRows(): Promise<ModulesDetailStatRow[]>;
  /** Click a breakdown entry; the caller polls the narrowed list. */
  modulesDetailProgressEntryClick(title: string): Promise<void>;
  /** Whether a breakdown entry is clickable (detail once the filter instance loads; never in peek). */
  modulesDetailProgressEntryClickable(title: string): Promise<boolean>;
  /** Whether a breakdown entry shows selected (its filter condition is active). */
  modulesDetailProgressEntrySelected(title: string): Promise<boolean>;
  /** Whether the links section renders expanded. */
  modulesDetailLinksOpen(): Promise<boolean>;
  /** Toggle the links section; resolves once the click lands. */
  modulesDetailLinksToggle(): Promise<void>;
  /** Links no-links notice, or null when link rows render. */
  modulesDetailLinksEmptyNotice(): Promise<string | null>;
  /** Whether the links add action is visible. */
  modulesDetailLinksAddVisible(): Promise<boolean>;
  /** Click the links add action; resolves once the dialog renders. */
  modulesDetailLinksAddClick(): Promise<void>;
  /** Link rows in display order (title, url, target). */
  modulesDetailLinksRows(): Promise<ModulesDetailLinkRow[]>;
  /** Whether the add/update link dialog is currently rendered. */
  modulesDetailLinkDialogOpen(): Promise<boolean>;
  /** Add/update link dialog heading, or null when no dialog renders. */
  modulesDetailLinkDialogTitle(): Promise<string | null>;
  /** Fill the link dialog URL field. */
  modulesDetailLinkFillUrl(url: string): Promise<void>;
  /** Fill the link dialog display-title field. */
  modulesDetailLinkFillTitle(title: string): Promise<void>;
  /** Submit the link dialog; resolves once the click lands. */
  modulesDetailLinkSubmit(): Promise<void>;
  /** Whether the link-dialog URL field shows invalid (failed required check). */
  modulesDetailLinkUrlInvalid(): Promise<boolean>;
  /** Cancel the link dialog; resolves once it detaches. */
  modulesDetailLinkCancel(): Promise<void>;
  /** Copy the named link's URL; resolves once the click lands. */
  modulesDetailLinkCopy(title: string): Promise<void>;
  /** Open the named link for editing; resolves once the dialog renders. */
  modulesDetailLinkEdit(title: string): Promise<void>;
  /** Delete the named link; resolves once the click lands. */
  modulesDetailLinkDelete(title: string): Promise<void>;
  /** Relative-age line of the named link, or null when it is absent. */
  modulesDetailLinkMeta(title: string): Promise<string | null>;
  /** Fail module-links writes on this page (failure-notice proof). */
  modulesDetailFailNextLinkWrite(): Promise<void>;
  // --- Runs, approvals, run dispatch, re-tick (NEWFRONT-180, RUN-016-024,
  // --- RUN-046, RUN-048). Appended; existing methods above are untouched
  // --- per the shared driver contract.

  /** Open the runs tab; project-scoped when `projectId` is given. Ends with the list settled. */
  openRunsList(workspaceSlug: string, projectId?: string): Promise<void>;
  /** Open the runs tab deep-linked to `page` via `?page=`; project-scoped when `projectId` is given. */
  openRunsListAtPage(workspaceSlug: string, page: number, projectId?: string): Promise<void>;
  /** Current session-history length, for proving pager turns rewrite rather than push. */
  runsHistoryLength(): Promise<number>;
  /** Open one run's detail; project-scoped when `projectId` is given. Ends with the detail settled. */
  openRunDetail(workspaceSlug: string, runId: string, projectId?: string): Promise<void>;
  /** Open the approvals tab; project-scoped when `projectId` is given. Ends with the queue settled. */
  openApprovals(workspaceSlug: string, projectId?: string): Promise<void>;
  /** Runs-list rows top to bottom: badge label plus prompt text (rows render no id; specs join prompts via the API). */
  runsRows(): Promise<RunsListRow[]>;
  /** Footer pager state; null when the list is empty and the footer hides. */
  runsPager(): Promise<{ label: string; prevDisabled: boolean; nextDisabled: boolean } | null>;
  /** Whether the runs list shows its empty state. */
  runsEmptyVisible(): Promise<boolean>;
  /** Activate the pager Next control; resolves once the new page settles. */
  runsNextPage(): Promise<void>;
  /** Activate the pager Previous control; resolves once the new page settles. */
  runsPrevPage(): Promise<void>;
  /** Click the row whose prompt contains `promptMarker`; resolves once the detail settles. */
  runsSelectRun(promptMarker: string): Promise<void>;
  /** Which detail pane state is showing. */
  runDetailState(): Promise<"none" | "loading" | "loaded" | "unavailable">;
  /** Loaded detail header: run id, badge label, executor badge. Null unless loaded. */
  runDetailHeader(): Promise<{ id: string; statusLabel: string; executor: string } | null>;
  /** Full prompt text of the loaded detail; null unless loaded. */
  runDetailPrompt(): Promise<string | null>;
  /** Scheduler link of the loaded detail; null when the run has no binding. */
  runDetailScheduler(): Promise<{ name: string; href: string } | null>;
  /** Error block of the loaded detail; null when the run has no error. Diagnostic fields are null when no structured diagnostic is present. */
  runDetailError(): Promise<{
    raw: string;
    source: string | null;
    kind: string | null;
    summary: string | null;
    action: string | null;
  } | null>;
  /** Result block of the loaded detail; null when the run produced no result payload. */
  runDetailResult(): Promise<{ summary: string | null; raw: string } | null>;
  /** Cloud-execution panel of the loaded detail; null when the panel is absent (non-cloud runs). */
  runCloudPanel(): Promise<{ tools: string[]; calls: Array<{ tool: string; risk: string; status: string }> } | null>;
  /** Event rows of the loaded detail in order: seq, kind, inline narrative (null for metadata rows). */
  runEvents(): Promise<Array<{ seq: string; kind: string; narrative: string | null }>>;
  /** Whether the detail header offers the cancel control. */
  runCancelAvailable(): Promise<boolean>;
  /** Open the cancel confirmation; resolves once the dialog shows. */
  runCancelOpen(): Promise<void>;
  /** Cancel dialog title and body; null when no dialog shows. */
  runCancelDialog(): Promise<{ title: string; body: string } | null>;
  /** Confirm the open cancel dialog; resolves once the detail settles again. */
  runCancelConfirm(): Promise<void>;
  /** Count runs-list API hits observed over `windowMs` while the list shows. */
  runsListPollCount(windowMs: number): Promise<number>;
  /** Count run-detail API hits for `runId` observed over `windowMs` while the detail shows. */
  runDetailPollCount(runId: string, windowMs: number): Promise<number>;
  /** Approval cards top to bottom. */
  approvalsCards(): Promise<ApprovalCard[]>;
  /** Whether the approvals queue shows its empty state. */
  approvalsEmptyVisible(): Promise<boolean>;
  /** Click one decision button on the card whose payload contains `payloadMarker`; resolves once the queue settles again. */
  approvalsDecide(payloadMarker: string, decision: "accept" | "accept_for_session" | "decline"): Promise<void>;
  /** Click the issue's Run-AI control and report the toast plus API request URLs seen during dispatch. */
  issueRunAiDispatch(): Promise<{ toast: string | null; requestUrls: string[] }>;
  /** Whether the issue detail offers the re-tick control. */
  issueReTickVisible(): Promise<boolean>;
  /** Click the issue's re-tick control; resolves once the toast shows. Returns the toast text. */
  issueReTickClick(): Promise<string | null>;
  /**
   * Reload the run detail with its API delayed and report whether the
   * loading state showed before the detail did. The delay is test-only
   * network shaping; the loader itself is the behavior under test.
   */
  runDetailLoadingObserved(workspaceSlug: string, runId: string): Promise<boolean>;

  // -------------------------------------------------------------------------
  // Pages detail header actions + sync/save/export (NEWFRONT-318,
  // PAGE-055-064 + PAGE-074). Sync-badge label/wait, offline emulation,
  // socket blocks, and frame/close captures come from the pagesLive* range
  // above; this range owns the header cluster, the options menu, the
  // offline/oversized indicators, the persistence spies, and the export
  // dialog. Failure-injection spies are route-based and permanent once
  // installed; the Unfail call flips the flag back off.
  // -------------------------------------------------------------------------

  /** Copy-link control state ("Copy link"/"Copied link"), or null when absent. */
  pagesHeaderCopyLinkLabel(): Promise<string | null>;
  /** Click the copy-link control. */
  pagesHeaderClickCopyLink(): Promise<void>;
  /** Current clipboard text (grants clipboard permissions first). */
  pagesHeaderReadClipboard(): Promise<string>;
  /** Star toggle state ("Add to favorites"/"Remove favorite"), or null when hidden. */
  pagesHeaderFavoriteLabel(): Promise<string | null>;
  /** Whether the star renders filled (favorited). */
  pagesHeaderFavoriteFilled(): Promise<boolean>;
  /** Click the star toggle. */
  pagesHeaderToggleFavorite(): Promise<void>;
  /** Whether the sidebar Favorites section is expanded. */
  pagesHeaderFavoritesExpanded(): Promise<boolean>;
  /** Page names listed under the sidebar Favorites section. */
  pagesHeaderFavoritesNames(): Promise<string[]>;
  /** Refuse favorite writes (500) so the optimistic toggle must roll back. */
  pagesHeaderFailFavoriteWrites(): Promise<void>;
  /** Undo pagesHeaderFailFavoriteWrites. */
  pagesHeaderUnfailFavoriteWrites(): Promise<void>;
  /** Open the detail options menu; resolves once its items render. */
  pagesHeaderOpenOptions(): Promise<void>;
  /** Labels of the open options menu, top to bottom. */
  pagesHeaderMenuItems(): Promise<string[]>;
  /** Click the options-menu item whose label contains `label`. */
  pagesHeaderChooseMenuItem(label: string): Promise<void>;
  /** Lock control state ("Lock"/"Locked"/"Unlocked"), or null when hidden. */
  pagesHeaderLockLabel(): Promise<string | null>;
  /** Click the lock control. */
  pagesHeaderToggleLock(): Promise<void>;
  /** Whether the body editor is currently editable. */
  pagesHeaderEditorEditable(): Promise<boolean>;
  /** Refuse lock writes (500) so the optimistic toggle must roll back. */
  pagesHeaderFailLockWrites(): Promise<void>;
  /** Undo pagesHeaderFailLockWrites. */
  pagesHeaderUnfailLockWrites(): Promise<void>;
  /** Shelved marker text ("Archived at ..."), or null when the page is live. */
  pagesHeaderArchivedMarker(): Promise<string | null>;
  /** Whether the delete confirmation dialog is open. */
  pagesHeaderDeleteDialogVisible(): Promise<boolean>;
  /** Delete confirmation dialog body text. */
  pagesHeaderDeleteDialogText(): Promise<string>;
  /** Confirm the delete dialog (busy state, then it closes). */
  pagesHeaderConfirmDelete(): Promise<void>;
  /** Cancel the delete dialog. */
  pagesHeaderCancelDelete(): Promise<void>;
  /** Canvas max-width the full-width toggle drives ("720px"/"1152px"). */
  pagesHeaderCanvasMaxWidth(): Promise<string>;
  /** Whether the editor toolbar is currently visible. */
  pagesHeaderToolbarVisible(): Promise<boolean>;
  /** Value of the navigation-pane tab query key, if present. */
  pagesHeaderPaneTabParam(): Promise<string | null>;
  /** Whether the navigation pane is currently open. */
  pagesHeaderPaneOpen(): Promise<boolean>;
  /** Sync badge tooltip text after hovering it, or null when unhoverable. */
  pagesHeaderSyncBadgeTooltip(): Promise<string | null>;
  /** Offline pill text ("Offline"), or null when the pill is hidden. */
  pagesHeaderOfflinePill(): Promise<string | null>;
  /** Oversized-content strip text, or null when the strip is hidden. */
  pagesHeaderOversizedStrip(): Promise<string | null>;
  /** Sync badge text via a bounded walk (safe on the oversized doc), or null when hidden. */
  pagesHeaderSyncBadgeText(): Promise<string | null>;
  /** Visible toast text via a bounded walk (safe on the oversized doc), or null when none shows. */
  pagesHeaderToastText(): Promise<string | null>;
  /** Millisecond timestamps of the spied description PATCH writes, in order. */
  pagesHeaderDescriptionWriteTimes(): Promise<number[]>;
  /** HTTP statuses of the spied description PATCH writes, in request order. */
  pagesHeaderDescriptionWriteStatuses(): Promise<number[]>;
  /** Start counting description PATCH writes (no-op when already spying). */
  pagesHeaderSpyDescriptionWrites(): Promise<void>;
  /** Description PATCH writes observed since pagesHeaderSpyDescriptionWrites. */
  pagesHeaderDescriptionWriteCount(): Promise<number>;
  /** Top-level keys of the most recent description PATCH body. */
  pagesHeaderDescriptionWriteKeys(): Promise<string[]>;
  /** Start counting page-record PATCH writes (title/mark/properties path). */
  pagesHeaderSpyPageWrites(): Promise<void>;
  /** Page-record PATCH writes observed since pagesHeaderSpyPageWrites. */
  pagesHeaderPageWriteCount(): Promise<number>;
  /** Press the standard save gesture (Mod+S). */
  pagesHeaderPressSave(): Promise<void>;
  /** Whether the export dialog is open. */
  pagesHeaderExportDialogVisible(): Promise<boolean>;
  /** Current export form state. */
  pagesHeaderExportForm(): Promise<PagesHeaderExportForm>;
  /** Set the export format ("PDF"/"Markdown"). */
  pagesHeaderExportSetFormat(label: string): Promise<void>;
  /** Set the export content scope ("Everything"/"No images"). */
  pagesHeaderExportSetScope(label: string): Promise<void>;
  /** Set the printable page format ("A4"/...; PDF only). */
  pagesHeaderExportSetPageFormat(label: string): Promise<void>;
  /** Confirm the export and capture the downloaded file. */
  pagesHeaderExportConfirm(): Promise<PagesHeaderDownload>;
  /** Cancel the export dialog. */
  pagesHeaderExportCancel(): Promise<void>;

  // -------------------------------------------------------------------------
  // Pages title, mark + formatting toolbar (NEWFRONT-315, PAGE-026-036).
  // Icon buttons are positional (0-13: four text marks, three alignments,
  // three list kinds, quote, code, table, image); tooltips name each action.
  // Title/mark reads go through the detail header; body reads through the
  // live editor. Scenarios reuse the 319 open/sync/title/body basics.
  // -------------------------------------------------------------------------

  /** Focus the title editor and press Enter. */
  pagesTitlePressEnter(): Promise<void>;
  /** Whether keyboard focus currently sits inside the body editor. */
  pagesTitleFocusInBody(): Promise<boolean>;
  /** Whether the body caret sits at the very top (start of the first block). */
  pagesTitleBodyCaretAtTop(): Promise<boolean>;
  /** Whether the title editor is editable (contenteditable true). */
  pagesTitleEditable(): Promise<boolean>;
  /** Placeholder text the empty title shows, or null when it shows none. */
  pagesTitleEmptyFallback(): Promise<string | null>;
  /** "N/255"-style count badge near the title, or null when absent. */
  pagesTitleCountBadge(): Promise<string | null>;
  /** Click into the title editor (focus without changing text). */
  pagesTitleFocus(): Promise<void>;
  /**
   * Replace the title editor contents, typing one char per `delayMs`.
   * Slow typing keeps per-keystroke title PATCHes from racing (see
   * NEWFRONT-358): the delay should exceed a PATCH round-trip.
   */
  pagesTitleSetTitle(text: string, delayMs: number): Promise<void>;
  /** Whether the title editor currently holds keyboard focus. */
  pagesTitleIsFocused(): Promise<boolean>;
  /** Open the mark picker (hover-revealed affordance or current mark). */
  pagesMarkOpenPicker(): Promise<void>;
  /** Picker tab labels in order. */
  pagesMarkTabs(): Promise<string[]>;
  /** Select a picker tab by label. */
  pagesMarkSelectTab(name: string): Promise<void>;
  /** Search the emoji tab for `query` and pick the `emoji` result. */
  pagesMarkSearchPick(emoji: string, query: string): Promise<void>;
  /** Pick the `index`-th vector icon on the icon tab. */
  pagesMarkPickIcon(index: number): Promise<void>;
  /** Currently selected mark as the header renders it, or null when markless. */
  pagesMarkCurrent(): Promise<PagesMarkCurrent | null>;
  /** Whether the picker trigger is disabled (read-only). */
  pagesMarkPickerDisabled(): Promise<boolean>;
  /** Computed opacity of the unhovered mark affordance ("0", "1", "selected"). */
  pagesMarkAffordanceOpacity(): Promise<string>;
  /** Whether the open picker offers any remove/clear control. */
  pagesMarkClearControl(): Promise<boolean>;
  /** Hover the detail header (reveals the mark affordance without opening it). */
  pagesMarkHoverHeader(): Promise<void>;
  /** Mark glyph the list row for `pageName` shows (emoji text or null). */
  pagesMarkListGlyph(workspaceSlug: string, projectId: string, pageName: string): Promise<string | null>;
  /** Breadcrumb page-switcher trigger text (mark glyph plus page name). */
  pagesMarkBreadcrumbGlyph(pageName: string): Promise<string | null>;
  /** Switcher dropdown entry text for `pageName` (mark glyph plus name). */
  pagesMarkSwitcherGlyph(pageName: string): Promise<string | null>;
  /**
   * Settle the detail view for toolbar driving: wait for socket steady
   * state, then verify a command round-trips (bold on/off), reloading up
   * to three times when the load bound the toolbar to a dead editor.
   */
  pagesToolbarSettle(workspaceSlug: string, projectId: string, pageId: string): Promise<void>;
  /** Count of icon buttons on the formatting bar (typography/color excluded). */
  pagesToolbarIconCount(): Promise<number>;
  /** Click the `index`-th formatting icon button. */
  pagesToolbarClick(index: number): Promise<void>;
  /** Whether the `index`-th icon button shows its active highlight. */
  pagesToolbarActive(index: number): Promise<boolean>;
  /** Tooltip (action name plus key hint) of the `index`-th icon button. */
  pagesToolbarTooltip(index: number): Promise<PagesToolbarTooltip>;
  /** Current typography selector label. */
  pagesToolbarTypography(): Promise<string>;
  /** Typography menu entries in order. */
  pagesToolbarTypographyItems(): Promise<string[]>;
  /** Apply the typography style named `name`. */
  pagesToolbarTypographyPick(name: string): Promise<void>;
  /** Open the color popover. */
  pagesToolbarColorOpen(): Promise<void>;
  /** Swatch counts per color row plus clear-control presence. */
  pagesToolbarColorCounts(): Promise<PagesToolbarColorCounts>;
  /** Pick the `index`-th swatch of the `kind` color row. */
  pagesToolbarColorPick(kind: "text" | "background", index: number): Promise<void>;
  /** Click the clear control of the `kind` color row. */
  pagesToolbarColorClear(kind: "text" | "background"): Promise<void>;
  /** Color preview state on the toolbar button (style strings or null). */
  pagesToolbarColorPreview(): Promise<PagesToolbarColorPreview>;
  /** Body editor inner HTML. */
  pagesToolbarBodyHtml(): Promise<string>;
  /** Focus the body editor and select all its content. */
  pagesToolbarSelectAll(): Promise<void>;
  /** Type `text` at the current body caret (no click first). */
  pagesToolbarTypeText(text: string): Promise<void>;
  /** Fire the manual save gesture (Control/Command+S). */
  pagesToolbarSave(): Promise<void>;
  /** Press one key into the focused editor (Tab, arrows, Enter). */
  pagesToolbarPressKey(key: string): Promise<void>;
  /** First table dimensions as "RxC", or null when the body has no table. */
  pagesToolbarTableDims(): Promise<string | null>;
  /** Caret cell as "rXcY", or "outside" when the caret is not in a table. */
  pagesToolbarCaretCell(): Promise<string>;
  /** Insert an image through the toolbar entry and upload `filePath`. */
  pagesToolbarImageUpload(filePath: string): Promise<void>;
  /** Whether the body holds an uploaded (asset-URL) image. */
  pagesToolbarEditorHasImage(): Promise<boolean>;
  /** Src of the first uploaded body image, or null when there is none. */
  pagesToolbarImageSrc(): Promise<string | null>;
  /** Whether the formatting bar renders with a non-zero height. */
  pagesToolbarVisible(): Promise<boolean>;
  /** Whether the floating pane toggle shows (bar-hidden state). */
  pagesToolbarFloatingToggle(): Promise<boolean>;
  /** Seed the sticky-toolbar preference before the next navigation. */
  pagesToolbarSeedStickyPref(value: boolean): Promise<void>;

  // --- NEWFRONT-317 (pages navigation pane + versions, PAGE-045-054).
  // --- Appended; existing methods above are untouched per the shared
  // --- driver contract. Covers the page-detail canvas host: the slide-over
  // --- pane shell, the canvas-edge heading preview, the outline/assets/info
  // --- tabs, and the version timeline with its snapshot overlay, gated
  // --- restore, and error paths. Editor reads/writes here stay primitive
  // --- (open, text, type, paste); toolbar and block surfaces belong to the
  // --- sibling children (PAGE-026-044), which own their own method names.

  /** Open a page detail and wait for the live editor to be ready. */
  pagesPaneOpenDetail(workspaceSlug: string, projectId: string, pageId: string): Promise<void>;
  /** Open a page detail with a raw query string (shareable pane/version links). */
  pagesPaneOpenDetailQuery(workspaceSlug: string, projectId: string, pageId: string, query: string): Promise<void>;
  /** Open a page detail that renders no editable region (locked pages). */
  pagesPaneOpenDetailReadOnly(workspaceSlug: string, projectId: string, pageId: string): Promise<void>;
  /** Visible text of the live editor. */
  pagesPaneEditorText(): Promise<string>;
  /** Focus the live editor at its end, ready for typing. */
  pagesPaneEditorClickEnd(): Promise<void>;
  /** Type text into the focused live editor. */
  pagesPaneEditorType(text: string): Promise<void>;
  /** Press Enter in the live editor. */
  pagesPaneEditorPressEnter(): Promise<void>;
  /** Vertical scroll offset of the editor scroll container. */
  pagesPaneEditorScrollTop(): Promise<number>;
  /** Paste HTML into the live editor (image-bearing fixtures for the assets tab). */
  pagesPaneEditorPasteHtml(html: string): Promise<void>;
  /** Current value of a page query key (pane tab, selected version), or null. */
  pagesPaneQueryParam(key: string): Promise<string | null>;
  /** Whether the navigation pane is open (its close control shows). */
  pagesPaneIsOpen(): Promise<boolean>;
  /** Open the pane through the toolbar toggle; resolves once it opens. */
  pagesPaneOpenViaToolbar(): Promise<void>;
  /** Close the pane through its close control; resolves once it closes. */
  pagesPaneClose(): Promise<void>;
  /** Rendered width of the pane shell in CSS pixels. */
  pagesPaneWidth(): Promise<number>;
  /** Pane tab labels in display order. */
  pagesPaneTabNames(): Promise<string[]>;
  /** Active pane tab key from the URL, or null when the pane is closed. */
  pagesPaneActiveTab(): Promise<string | null>;
  /** Select a pane tab; resolves once its panel renders. */
  pagesPaneSelectTab(name: string): Promise<void>;
  /** Rendered width of the canvas content column in CSS pixels. */
  pagesPaneCanvasWidth(): Promise<number>;
  /** Whether the pane shell carries a slide transition (animated open/close). */
  pagesPaneSlideTransition(): Promise<boolean>;
  /** Whether the canvas-edge heading preview shows (pane closed). */
  pagesPanePreviewVisible(): Promise<boolean>;
  /** Number of heading bars the collapsed preview renders. */
  pagesPanePreviewBarCount(): Promise<number>;
  /** Hover the collapsed preview to expand its heading card. */
  pagesPanePreviewHover(): Promise<void>;
  /** Whether the expanded preview heading card shows. */
  pagesPanePreviewCardVisible(): Promise<boolean>;
  /** Heading texts in the expanded preview card. */
  pagesPanePreviewCardHeadings(): Promise<string[]>;
  /** Click the collapsed preview; resolves once the pane opens. */
  pagesPanePreviewOpen(): Promise<void>;
  /** Outline tab headings in display order (level from indentation). */
  pagesPaneOutlineHeadings(): Promise<PagesPaneOutlineHeading[]>;
  /** Click an outline heading; the caller polls the editor scroll offset. */
  pagesPaneOutlineClick(text: string): Promise<void>;
  /** Whether the outline empty illustration shows. */
  pagesPaneOutlineEmptyVisible(): Promise<boolean>;
  /** Asset rows in display order (thumbnail, view/download links). */
  pagesPaneAssetsRows(): Promise<PagesPaneAssetRow[]>;
  /** Hover an asset row; whether its download action reveals. */
  pagesPaneAssetsDownloadVisible(name: string): Promise<boolean>;
  /** Whether the assets empty illustration shows. */
  pagesPaneAssetsEmptyVisible(): Promise<boolean>;
  /** Live document statistics cards (words, characters, paragraphs, read time). */
  pagesPaneStats(): Promise<PagesPaneStats>;
  /** Info tab actor rows (last editor + creator with profile links). */
  pagesPaneActors(): Promise<PagesPaneActor[]>;
  /** Version timeline entries: the current-version row plus snapshots. */
  pagesPaneVersionEntries(): Promise<PagesPaneVersionEntry[]>;
  /** Select a timeline snapshot; resolves once the overlay opens. */
  pagesPaneSelectVersion(versionId: string): Promise<void>;
  /** Select the current-version timeline row; resolves once the overlay closes. */
  pagesPaneSelectCurrentVersion(): Promise<void>;
  /** Whether the version snapshot overlay is open. */
  pagesPaneOverlayOpen(): Promise<boolean>;
  /** Overlay header: snapshot title plus the read-only marker. */
  pagesPaneOverlayHeader(): Promise<PagesPaneOverlayHeader>;
  /** Visible text of the overlay snapshot body. */
  pagesPaneOverlayText(): Promise<string>;
  /** Whether the overlay shows its loading skeleton. */
  pagesPaneOverlaySkeletonVisible(): Promise<boolean>;
  /** Stall the version-detail fetch so the skeleton stays up (test hook). */
  pagesPaneOverlayStallDetail(): Promise<void>;
  /** Release a stalled version-detail fetch (test hook). */
  pagesPaneOverlayReleaseDetail(): Promise<void>;
  /** Whether the overlay shows its restore control. */
  pagesPaneRestoreVisible(): Promise<boolean>;
  /** Click restore; resolves once the overlay closes or a notice lands. */
  pagesPaneRestore(): Promise<void>;
  /** Whether the overlay shows its load-error panel. */
  pagesPaneOverlayErrorVisible(): Promise<boolean>;
  /** Load-error panel texts (heading, body, retry label). */
  pagesPaneOverlayErrorTexts(): Promise<{ heading: string | null; body: string | null; retry: string | null }>;
  /** Click the error panel retry action. */
  pagesPaneOverlayRetry(): Promise<void>;
  /** Fail version-detail fetches with HTTP 500 (test hook). */
  pagesPaneFailVersionDetail(): Promise<void>;
  /** Stop failing version-detail fetches (test hook). */
  pagesPaneUnfailVersionDetail(): Promise<void>;
  /** Version-detail fetches observed since the last fail/stall hook armed. */
  pagesPaneVersionDetailRequests(): Promise<number>;
  /** Click the first live-editor block containing the text (for mid-doc edits). */
  pagesPaneEditorClickText(text: string): Promise<void>;
  /** Select all live-editor content and delete it. */
  pagesPaneEditorClear(): Promise<void>;
  /** Click the live-editor image block, then delete it. */
  pagesPaneEditorDeleteImage(): Promise<void>;
  /** Omit a workspace member from member-list answers (deactivated lookalike). */
  pagesPaneOmitWorkspaceMember(userId: string): Promise<void>;
  /** Stop omitting workspace members from member-list answers. */
  pagesPaneRestoreWorkspaceMembers(): Promise<void>;
  /**
   * Leave the detail view through the creator profile link and come back,
   * so the pane's shared editor handle remounts on warm client state.
   */
  pagesPaneProfileRoundtrip(): Promise<void>;

  // -------------------------------------------------------------------------
  // Workspace issue exports settings (NEWFRONT-381, rows EXP-001..013).
  // Appended; existing methods above are untouched per the shared driver
  // contract. Sibling children (NEWFRONT-382/383) add their own blocks.
  // -------------------------------------------------------------------------

  /** Open the workspace exports settings page as the `cookies` identity; resolves once the page settles. */
  exportsSettingsOpen(workspaceSlug: string, cookies: ParityBrowserCookie[]): Promise<void>;
  /** Which access shape the page renders: working form, not-authorized view, or the no-workspace view. */
  exportsSettingsAccess(): Promise<"form" | "not-authorized" | "no-workspace">;
  /** Page heading title plus the explanatory paragraph. */
  exportsSettingsHeading(): Promise<{ title: string; description: string }>;
  /** Breadcrumb trail texts in the settings header. */
  exportsSettingsBreadcrumb(): Promise<string[]>;
  /** Browser tab title. */
  exportsSettingsPageTitle(): Promise<string>;
  /** Current location path (what the address bar shows). */
  exportsSettingsCurrentPath(): Promise<string>;
  /** Open the project picker dropdown. */
  exportsSettingsProjectPickerOpen(): Promise<void>;
  /** Visible project options as key/name pairs in display order. */
  exportsSettingsProjectPickerOptions(): Promise<Array<{ key: string; name: string }>>;
  /** Type into the picker search box. */
  exportsSettingsProjectPickerSearch(query: string): Promise<void>;
  /** Toggle the option showing `name`. */
  exportsSettingsProjectPickerToggle(name: string): Promise<void>;
  /** Current picker button label (compact key list, or the all-projects label). */
  exportsSettingsProjectPickerLabel(): Promise<string>;
  /** Whether the project picker refuses interaction. */
  exportsSettingsProjectPickerDisabled(): Promise<boolean>;
  /** Open the format picker and read its option labels. */
  exportsSettingsFormatOptions(): Promise<string[]>;
  /** Choose a format by its visible label. */
  exportsSettingsFormatSelect(label: string): Promise<void>;
  /** Current format picker label. */
  exportsSettingsFormatLabel(): Promise<string>;
  /** Click the Export action. */
  exportsSettingsSubmit(): Promise<void>;
  /** Current Export button text (working state while the request runs). */
  exportsSettingsSubmitLabel(): Promise<string>;
  /** Whether the Export button refuses interaction right now. */
  exportsSettingsSubmitDisabled(): Promise<boolean>;
  /** Visible toast, if any (tone, title, message). */
  exportsSettingsToast(): Promise<{ kind: "success" | "error"; title: string; message: string } | null>;
  /** History rows in display order (who/when/count/format/state/download cell). */
  exportsSettingsHistoryRows(): Promise<ExportsSettingsHistoryRow[]>;
  /** Number of history rows currently rendered. */
  exportsSettingsHistoryCount(): Promise<number>;
  /** Click the manual refresh control. */
  exportsSettingsRefresh(): Promise<void>;
  /** Current refresh control text (working state while re-reading), or null when no control renders. */
  exportsSettingsRefreshLabel(): Promise<string | null>;
  /** Pagination shape: control visibility plus end states. */
  exportsSettingsPagination(): Promise<{ visible: boolean; prevDisabled: boolean; nextDisabled: boolean }>;
  /** Click the history back control. */
  exportsSettingsGoPrev(): Promise<void>;
  /** Click the history forward control. */
  exportsSettingsGoNext(): Promise<void>;
  /** Whether the pre-first-response skeleton placeholder shows. */
  exportsSettingsSkeletonVisible(): Promise<boolean>;
  /** Illustrated empty view facts, or null when history renders. */
  exportsSettingsEmptyFacts(): Promise<ExportsSettingsEmptyFacts | null>;
  /** Start counting export-history list requests (auto-poll observation). */
  exportsSettingsWatchHistoryRequests(): Promise<void>;
  /** Export-history list requests observed since the watch started. */
  exportsSettingsHistoryRequestCount(): Promise<number>;
  /** Capture export submit request bodies for later reading (test hook). */
  exportsSettingsCaptureSubmit(): Promise<void>;
  /** Bodies of captured export submit requests, in order. */
  exportsSettingsCapturedSubmits(): Promise<Array<{ provider: string; project: string[]; multiple: boolean }>>;
  /** Stall the export submit request so the working button state stays up (test hook). */
  exportsSettingsStallSubmit(): Promise<void>;
  /** Release a stalled export submit request (test hook). */
  exportsSettingsReleaseSubmit(): Promise<void>;
  /** Fail export submit requests with HTTP 500 (test hook). */
  exportsSettingsFailSubmit(): Promise<void>;
  /** Stop failing export submit requests (test hook). */
  exportsSettingsUnfailSubmit(): Promise<void>;
  /** Stall the history list read so the skeleton stays up (test hook). */
  exportsSettingsStallHistory(): Promise<void>;
  /** Release a stalled history list read (test hook). */
  exportsSettingsReleaseHistory(): Promise<void>;
  /** Click the download control of `rowIndex`; resolves with the new-tab URL (new-tab proof). */
  exportsSettingsDownloadPopupUrl(rowIndex: number): Promise<string>;
  // ---------------------------------------------------------------------------
  // Analytics and profile local-file downloads (NEWFRONT-382, EXP-014/015/022).
  // The work-items analytics tab carries two data tables (the chart
  // drill-down table first, the insight table second), each with its own
  // search box and "Export as csv" action that saves a locally built file
  // through the browser download event; the profile activity page carries a
  // permission-gated download control with a working state while it fetches.
  // Appended; existing entries above are untouched per the shared contract.
  // ---------------------------------------------------------------------------

  /** Open the analytics tab and wait until both table export actions render. */
  analyticsExportOpen(workspaceSlug: string, cookies: ParityBrowserCookie[]): Promise<void>;
  /** Number of "Export as csv" actions on the analytics tab. */
  analyticsExportButtonCount(): Promise<number>;
  /** Body-row cell texts of one analytics table in display order. */
  analyticsExportRowCells(kind: AnalyticsExportTable): Promise<string[][]>;
  /** Filter one analytics table through its search box. */
  analyticsExportSearch(kind: AnalyticsExportTable, query: string): Promise<void>;
  /** Clear one analytics table's search box. */
  analyticsExportClearSearch(kind: AnalyticsExportTable): Promise<void>;
  /** Click one analytics table's export action; resolves with the downloaded file (download-event proof). */
  analyticsExportDownload(kind: AnalyticsExportTable): Promise<LocalFileDownload>;
  /** Click one analytics table's export action; resolves true when no download lands (empty-export proof). */
  analyticsExportYieldsNoDownload(kind: AnalyticsExportTable): Promise<boolean>;
  /** Open one user's profile activity page (settles on the activity view or the below-member guard view). */
  profileActivityOpen(workspaceSlug: string, userId: string, cookies: ParityBrowserCookie[]): Promise<void>;
  /** Whether the profile activity download control renders. */
  profileActivityDownloadVisible(): Promise<boolean>;
  /** Current download control text (working state while fetching), or null when no control renders. */
  profileActivityDownloadLabel(): Promise<string | null>;
  /** Stall the profile activity export request so the working state stays up (test hook). */
  profileActivityStallExport(): Promise<void>;
  /** Release a stalled profile activity export request (test hook). */
  profileActivityReleaseExport(): Promise<void>;
  /** Click the download control; resolves with the saved file plus the export request body. */
  profileActivityDownload(): Promise<{ file: LocalFileDownload; requestBody: { date: string } }>;
  // ---------------------------------------------------------------------------
  // Modules list frame and header controls (NEWFRONT-306, MOD-001..007).
  // The live modules list: breadcrumb trail, flag gate, Add action,
  // expandable search, ordering, filters menu, applied chips, and the
  // timeline manual-sort reorder gate. Appended; existing entries above are
  // untouched per the shared contract.
  // ---------------------------------------------------------------------------

  /** Live module row names in display order. */
  modulesRowNames(): Promise<string[]>;
  /** Project crumb plus trailing crumb link texts of the header trail. */
  modulesBreadcrumb(): Promise<ModulesBreadcrumb>;
  /** Document title of the current page. */
  modulesPageTitle(): Promise<string>;
  /** Add-button label (Add on small screens, Add Module on larger ones), or null when no button renders. */
  modulesAddLabel(): Promise<string | null>;
  /** Click the header Add button. */
  modulesClickAdd(): Promise<void>;
  /** Whether the create-module dialog is open. */
  modulesCreateDialogOpen(): Promise<boolean>;
  /** Whether the header search box is expanded. */
  modulesSearchVisible(): Promise<boolean>;
  /** Expand the header search box through its icon button. */
  modulesOpenSearch(): Promise<void>;
  /** Type into the header search box (the list filters live). */
  modulesTypeSearch(text: string): Promise<void>;
  /** Current header search text. */
  modulesSearchText(): Promise<string>;
  /** Whether the header search box holds focus. */
  modulesSearchFocused(): Promise<boolean>;
  /** Press Escape while the header search box holds focus. */
  modulesEscapeSearch(): Promise<void>;
  /** Click the trailing Modules crumb link (outside the search box). */
  modulesClickOutsideSearch(): Promise<void>;
  /** Visible label of the sort trigger control. */
  modulesSortTriggerLabel(): Promise<string>;
  /** Pick a sort or direction entry through the header sort menu and wait for the reorder. */
  modulesSetSortViaMenu(label: string): Promise<void>;
  /** Sort menu entries in display order (opens and closes the menu). */
  modulesSortMenuItems(): Promise<string[]>;
  /** Open the filters menu and wait until its groups show. */
  modulesOpenFilters(): Promise<void>;
  /** Dismiss the filters menu. */
  modulesCloseFilters(): Promise<void>;
  /** Filter group titles in display order. */
  modulesFilterGroups(): Promise<string[]>;
  /** Option labels of one filter group in display order (View all/less excluded). */
  modulesFilterGroupOptions(group: string): Promise<string[]>;
  /** Toggle one filter option by group plus label. */
  modulesToggleFilterOption(group: string, label: string): Promise<void>;
  /** Expand a filter group past its first five options through View all. */
  modulesExpandFilterGroup(group: string): Promise<void>;
  /** Whether a filter group offers the View all expander. */
  modulesFilterGroupExpandable(group: string): Promise<boolean>;
  /** Type into the filters menu's own search box. */
  modulesMenuSearch(text: string): Promise<void>;
  /** Whether the filters button shows its applied dot. */
  modulesFiltersActive(): Promise<boolean>;
  /** Open the custom date dialog of a date group. */
  modulesOpenCustomDate(group: "Start date" | "Due date"): Promise<void>;
  /** Whether the custom date dialog is open. */
  modulesCustomDateOpen(): Promise<boolean>;
  /** Apply the custom date dialog as configured. */
  modulesApplyCustomDate(): Promise<void>;
  /** Applied-filter chips in display order (key plus rendered text). */
  modulesChips(): Promise<ModulesChip[]>;
  /** Remove one chip value through its pill control. */
  modulesRemoveChipValue(key: string, value: string): Promise<void>;
  /** Remove one whole filter dimension through its chip control. */
  modulesRemoveChipDimension(key: string): Promise<void>;
  /** Clear every applied filter through the clear-all control. */
  modulesClearFilters(): Promise<void>;
  /** Whether the applied-filters strip renders. */
  modulesChipsVisible(): Promise<boolean>;
  /** Flag-gate facts when modules are disabled for the project. */
  modulesGateFacts(): Promise<ModulesGateFacts>;
  /** Follow the gate's Manage-features button to project settings. */
  modulesGateToFeatures(): Promise<void>;
  /** Timeline sidebar row names in display order. */
  modulesTimelineRowNames(): Promise<string[]>;
  /** Drag a timeline sidebar row back onto itself (a no-op gesture). */
  modulesDragTimelineRowToSelf(name: string): Promise<void>;
  /** Whether the manual-sort warning toast shows. */
  modulesManualSortWarningVisible(): Promise<boolean>;

  // --- Reactions oracle (NEWFRONT-116, CMT-022/023/024). Observed on the
  // --- running old app: the issue cluster and each comment card carry a
  // --- picker trigger plus chip buttons (emoji, count, viewer highlight);
  // --- hovering a chip names who reacted; archived detail disables the
  // --- issue control and hides comment clusters entirely. Appended; existing
  // --- methods above are untouched per the shared driver contract.
  /** Hover an issue-level reaction chip; resolves with the reactor tooltip text. */
  reactionsIssueChipTooltipText(emoji: string, expectedName: string): Promise<string>;
  /** Add a specific emoji to the issue through the picker (re-adds allowed). */
  reactionsIssueAddEmoji(emoji: string): Promise<void>;
  /** Whether the issue-level reaction picker trigger is currently enabled. */
  reactionsIssuePickerReady(): Promise<boolean>;
  /** Add a specific emoji to a comment card through its picker (re-adds allowed). */
  reactionsCommentAddEmoji(cardText: string, emoji: string): Promise<void>;
  /** Whether a comment card on the archived detail shows any reaction UI. */
  reactionsArchivedCommentClusterVisible(cardText: string): Promise<boolean>;

  // --- Project lifecycle oracles (NEWFRONT-390, PSET-013..017). Appended;
  // --- existing methods above are untouched per the shared driver contract.
  // --- PSET-013 (create): the post-create feature-selection step was
  // --- intentionally hidden upstream (bd4f157e), so creation closes the
  // --- modal directly; this read proves that absence.
  /** Whether the post-create feature-selection step (Close + Open project exits) is showing. */
  isCreateFeatureStepVisible(): Promise<boolean>;
  // --- PSET-014 (restriction-screen join): the single-action join screen
  // --- a non-member meets on a direct project URL.
  /** Whether the project restriction screen with its join action is showing. */
  isProjectRestrictionVisible(): Promise<boolean>;
  /** Label on the restriction screen's join action ("Join project" vs "Joining project"), or null. */
  restrictionJoinLabel(): Promise<string | null>;
  /** Click the restriction screen's single join action. */
  clickRestrictionJoin(): Promise<void>;
  // --- PSET-015 (leave ordering): the leave dialog navigates to the list
  // --- first and only then calls the leave endpoint (the navigation does
  // --- not wait for the write).
  /**
   * Submit the open leave dialog while the leave-API write is held for
   * `holdMs`. Resolves once the view has navigated back to the project
   * list — while the write is still in flight — proving the navigation
   * does not await the endpoint.
   */
  submitLeaveHoldingWrite(holdMs: number): Promise<void>;
  // --- PSET-016 (publish modal): the public-board publish flow. Layout
  // --- names are the visible option labels ("List", "Kanban"); toggle
  // --- names are "comments" | "reactions" | "voting".
  /**
   * Open the publish modal for a project through its sidebar quick-actions
   * entry. Takes the project name to hover its sidebar row, like the leave
   * flow does.
   */
  openPublishProjectModal(workspaceSlug: string, projectId: string, projectName: string): Promise<void>;
  /** Whether the publish modal is currently open. */
  isPublishModalVisible(): Promise<boolean>;
  /** Whether the publish modal is still loading its settings. */
  isPublishModalLoading(): Promise<boolean>;
  /** Layout option labels offered in the publish modal's Views control. */
  publishLayoutOptions(): Promise<string[]>;
  /** Layout labels currently selected in the publish modal. */
  publishSelectedLayouts(): Promise<string[]>;
  /** Toggle one layout option in the publish modal's Views control. */
  publishToggleLayout(label: string): Promise<void>;
  /** Whether one publish toggle ("comments" | "reactions" | "voting") is on. */
  publishToggleState(name: "comments" | "reactions" | "voting"): Promise<boolean>;
  /** Set one publish toggle ("comments" | "reactions" | "voting"). */
  setPublishToggle(name: "comments" | "reactions" | "voting", on: boolean): Promise<void>;
  /** Submit the publish form (initial publish). */
  submitPublish(): Promise<void>;
  /** The live board link shown once published, or null when absent. */
  publishLiveLink(): Promise<string | null>;
  /** Anchor target of the live-link open action ("_blank" proof), or null. */
  publishLiveLinkTarget(): Promise<string | null>;
  /** Click the publish modal's "Copy link" action (grants clipboard permission first). */
  clickPublishCopyLink(): Promise<void>;
  /** Whether the update-settings control shows (published + dirty only). */
  isPublishUpdateVisible(): Promise<boolean>;
  /** Submit the dirty-gated settings update. */
  submitPublishUpdate(): Promise<void>;
  /** Whether the unpublish action shows (published only). */
  isPublishUnpublishVisible(): Promise<boolean>;
  /** Start unpublishing (resolves once the write lands). */
  clickPublishUnpublish(): Promise<void>;
  /** Whether unpublishing currently shows progress. */
  isPublishUnpublishing(): Promise<boolean>;
  // --- PSET-017 (list search locality): the expanding search box filters
  // --- locally with no data requests of its own.
  /** Arm the list-search request spy (call before opening/typing). */
  armListSearchRequestSpy(): Promise<void>;
  /** Data-API request paths observed since arming, deduplicated. */
  listSearchRequestPaths(): Promise<string[]>;
  // --- PSET-016 loading states: one-shot delays so the settings loader
  // --- and the write progress states stay observable against a fast local API.
  /** Delay the next publish-settings read (GET) by ms. */
  delayNextPublishRead(ms: number): Promise<void>;
  /** Delay the next publish write (POST/PATCH/DELETE) by ms. */
  delayNextPublishWrite(ms: number): Promise<void>;

  // --- Project-settings shell oracle (NEWFRONT-388, PSET-001/002/003/004/062/063).
  // --- Observed on the running old app: the settings shell renders a grouped
  // --- sidebar filtered by project role with the current tab highlighted and a
  // --- drawer trigger on narrow screens; the bare projects path forwards to the
  // --- first joined project or shows a no-projects guidance view; unknown or
  // --- forbidden ids render a restriction screen with a join action; tab pages
  // --- gate unauthorized roles behind a denial view; the server refuses
  // --- out-of-role writes. Appended; existing methods above are untouched per
  // --- the shared driver contract.
  /** Open a settings tab by href suffix ("" is general, "/" its slash form); resolves once the shell settles. */
  settingsShellOpen(workspaceSlug: string, projectId: string, tabHref: string): Promise<void>;
  /** Visible desktop-sidebar group headings, in display order. */
  settingsShellGroups(): Promise<string[]>;
  /** Visible desktop-sidebar item labels, in display order. */
  settingsShellItems(): Promise<string[]>;
  /** Label of the highlighted sidebar item, or null. */
  settingsShellActiveItem(): Promise<string | null>;
  /** Current page URL path, without a trailing slash (the slash is incidental; both forms serve). */
  settingsShellPath(): Promise<string>;
  /** Whether the narrow-screen drawer trigger is showing. */
  settingsShellDrawerTriggerVisible(): Promise<boolean>;
  /** Open the narrow-screen drawer; resolves once its items render. */
  settingsShellOpenDrawer(): Promise<void>;
  /** Item labels inside the open drawer. */
  settingsShellDrawerItems(): Promise<string[]>;
  /** Restore a wide viewport after narrow-screen reads. */
  settingsShellWideViewport(): Promise<void>;
  /** Whether the not-authorized denial view is showing. */
  settingsShellDeniedVisible(): Promise<boolean>;
  /** Whether the project restriction (join) screen is showing. */
  settingsShellRestrictionVisible(): Promise<boolean>;
  /** Visible text of the restriction screen, or null. */
  settingsShellRestrictionText(): Promise<string | null>;
  /** Whether the restriction screen offers a join action. */
  settingsShellJoinVisible(): Promise<boolean>;
  /** Confirm the join action; resolves once the shell settles authorized. */
  settingsShellJoin(): Promise<void>;
  /** Whether the bare-path no-projects guidance view is showing. */
  settingsShellNoProjectsVisible(): Promise<boolean>;
  /** Whether the general-tab form is showing. */
  settingsShellGeneralFormVisible(): Promise<boolean>;
  /** Fill the general-tab name field without saving. */
  settingsShellFillGeneralName(name: string): Promise<void>;
  /** Follow a sidebar item by label; resolves once navigation settles. */
  settingsShellFollowItem(label: string): Promise<void>;
  /** Press keys on the open settings tab; reports dialog sightings plus the resulting path. */
  settingsShellPressKeys(keys: string[]): Promise<{ dialogSeen: boolean; path: string }>;
  /** Whether any bulk/export/import control shows in the settings content. */
  settingsShellBulkExportImportVisible(): Promise<boolean>;
  /** Whether any edition-gated (upgrade/cloud-only) UI shows on settings screens. */
  settingsShellEditionGateVisible(): Promise<boolean>;
  /** Count websocket connections opened while visiting each tab href. */
  settingsShellCountSockets(tabHrefs: string[], workspaceSlug: string, projectId: string): Promise<number>;
  /** Open the bare settings projects path; resolves once it settles. */
  settingsProjectsOpen(workspaceSlug: string): Promise<void>;
  /** Open project creation from the bare-path guidance view; resolves once the create form shows. */
  settingsShellOpenGuidanceCreate(): Promise<void>;

  // --- Project-settings feature flags oracle (NEWFRONT-393, PSET-043..047,
  // --- PSET-061 slice). Observed on the running old app: one admin-only
  // --- tab per feature with a single boxed switch PATCHing its project
  // --- flag under promise toasts; the project sidebar hides or restores
  // --- each area's nav link with its flag. Appended; existing methods
  // --- above are untouched per the shared driver contract.
  /** Open a dedicated feature tab; settles on its switch or the denial view. */
  projectFeaturesOpenTab(
    workspaceSlug: string,
    projectId: string,
    tab: "views" | "cycles" | "intake" | "modules" | "pages"
  ): Promise<void>;
  /** Whether the dedicated tab's switch is currently on. */
  projectFeaturesSwitchOn(): Promise<boolean>;
  /** Whether the dedicated tab's switch is currently disabled. */
  projectFeaturesSwitchDisabled(): Promise<boolean>;
  /** Flip the dedicated tab's switch; resolves once the project write settles. */
  projectFeaturesToggle(): Promise<void>;
  /** Whether the tab shows the not-authorized view (non-admin callers). */
  projectFeaturesNotAuthorized(): Promise<boolean>;
  /** Whether the project sidebar still links an area (hides with its flag). */
  projectFeaturesNavLinkVisible(
    workspaceSlug: string,
    projectId: string,
    area: "cycles" | "modules" | "views" | "pages" | "intake"
  ): Promise<boolean>;

  // --- Project-settings automations oracle (NEWFRONT-393, PSET-053/054,
  // --- PSET-061 slice). Observed on the running old app: an admin-only
  // --- tab with auto-archive and auto-close rows, each a switch applying
  // --- a one-month default when enabled and expanding to a delay picker
  // --- (presets plus a whole-months custom dialog); auto-close adds a
  // --- state picker limited to cancelled-group states. The tab shows a
  // --- loader until the project loads. Appended; existing methods above
  // --- are untouched per the shared driver contract.
  /** Open the automations tab; settles on the archive row or the denial view. */
  projectAutomationsOpenTab(workspaceSlug: string, projectId: string): Promise<void>;
  /** Whether the tab shows the not-authorized view (non-admin callers). */
  projectAutomationsNotAuthorized(): Promise<boolean>;
  /** Whether the auto-archive switch is currently on (first switch on the tab). */
  projectAutomationsArchiveOn(): Promise<boolean>;
  /** Whether the auto-close switch is currently on (second switch on the tab). */
  projectAutomationsCloseOn(): Promise<boolean>;
  /** Flip the auto-archive switch; resolves once the project write settles. */
  projectAutomationsToggleArchive(): Promise<void>;
  /** Flip the auto-close switch; resolves once the project write settles. */
  projectAutomationsToggleClose(): Promise<void>;
  /** Whether any loader is currently visible on the tab. */
  projectAutomationsLoaderVisible(): Promise<boolean>;
  /** Hold the single-project retrieve GETs (bug NEWFRONT-397 probe: stretches the loading window). */
  projectAutomationsDelayDetailsFetch(ms: number): Promise<void>;
  /** Text of the single visible delay trigger (null unless exactly one shows). */
  projectAutomationsDelayTriggerText(): Promise<string | null>;
  /** Open the delay picker; resolves once its options render. */
  projectAutomationsOpenDelayOptions(): Promise<void>;
  /** Labels of the open delay picker options. */
  projectAutomationsDelayOptionLabels(): Promise<string[]>;
  /** Pick a delay preset; resolves once the project write settles. */
  projectAutomationsPickDelayOption(label: string): Promise<void>;
  /** Open the custom-months dialog; resolves once its input renders. */
  projectAutomationsOpenCustomize(): Promise<void>;
  /** Whether the custom-months dialog input is currently visible. */
  projectAutomationsMonthDialogOpen(): Promise<boolean>;
  /** Fill the custom-months input and submit; resolves once the write or the error settles. */
  projectAutomationsMonthDialogSubmit(months: string): Promise<void>;
  /** The custom-months range error, or null when absent. */
  projectAutomationsMonthDialogError(): Promise<string | null>;
  /** Text of the auto-close state trigger, or null when its panel is hidden. */
  projectAutomationsCloseStateText(): Promise<string | null>;
  /** Whether the auto-close state picker is currently disabled. */
  projectAutomationsCloseStateDisabled(): Promise<boolean>;

  // --- Project-settings github tab oracle (NEWFRONT-393, PSET-050..052,
  // --- PSET-061 slice). Observed on the running old app: a member-visible
  // --- tab showing a skeleton while the binding fetch is unresolved, then
  // --- a bound card (provider family, repository link, mirroring contract,
  // --- four-hour cadence with last sync, failure line) with an admin-only
  // --- sync switch and confirm-free Unbind, or an unbound guidance card
  // --- pointing at General settings with no branch or direction pickers.
  // --- Appended; existing methods above are untouched per the shared
  // --- driver contract.
  /** Open the github tab; settles on a card or the denial view. */
  projectGithubOpenTab(workspaceSlug: string, projectId: string): Promise<void>;
  /** Whether the tab shows the not-authorized view (below-member callers). */
  projectGithubNotAuthorized(): Promise<boolean>;
  /** Whether any loader is currently visible on the tab. */
  projectGithubLoaderVisible(): Promise<boolean>;
  /** Hold binding GETs so the loader stays up long enough to observe. */
  projectGithubDelayBindingFetch(ms: number): Promise<void>;
  /** Whether the tab shows the bound provider card. */
  projectGithubBoundVisible(): Promise<boolean>;
  /** The provider family label on the bound card (GitHub, GitLab, Git provider). */
  projectGithubProviderLabel(): Promise<string | null>;
  /** The repository full-name link text on the bound card, or null. */
  projectGithubRepoLinkText(): Promise<string | null>;
  /** Whether the mirroring-contract explainer is visible on the bound card. */
  projectGithubContractVisible(): Promise<boolean>;
  /** The sync-cadence line on the bound card, or null. */
  projectGithubCadenceText(): Promise<string | null>;
  /** The failure line on the bound card, or null when no failure is recorded. */
  projectGithubLastError(): Promise<string | null>;
  /** Whether the sync switch is currently on. */
  projectGithubSyncOn(): Promise<boolean>;
  /** Whether the sync switch is currently disabled. */
  projectGithubSyncDisabled(): Promise<boolean>;
  /** Flip the sync switch; resolves once the repository write settles. */
  projectGithubToggleSync(): Promise<void>;
  /** Whether the Unbind control is currently visible. */
  projectGithubUnbindVisible(): Promise<boolean>;
  /** Whether the Unbind control is currently disabled. */
  projectGithubUnbindDisabled(): Promise<boolean>;
  /** Click Unbind; resolves once the binding delete settles. */
  projectGithubUnbind(): Promise<void>;
  /** Whether the unbound guidance card is currently visible. */
  projectGithubGuidanceVisible(): Promise<boolean>;
  /** Full visible text of the tab (for pinned non-behavior assertions). */
  projectGithubTabText(): Promise<string>;

  // --- Project members tab (NEWFRONT-391, PSET-018–027). Observed on the
  // --- running old app: the members settings page stacks a defaults
  // --- section (lead, default assignee, guest access), a teamspace strip
  // --- and the member list (search, role filters, admin add control,
  // --- five-column table with per-column sort, hover remove/leave menu,
  // --- invite dialog, remove/leave confirmations, skeleton and no-match
  // --- states). Appended; existing methods above are untouched per the
  // --- shared driver contract.
  /** Open a project's settings Members page; settles on the list or the denial view. */
  settingsMembersOpen(workspaceSlug: string, projectId: string): Promise<void>;
  /** Whether the not-authorized view shows instead of the member table. */
  settingsMembersDenied(): Promise<boolean>;
  /** Member rows in display order: names, email, role word, joining text, profile link. */
  settingsMembersRows(): Promise<
    {
      fullName: string;
      displayName: string;
      email: string;
      roleLabel: string;
      joinedText: string;
      profileHref: string | null;
    }[]
  >;
  /** Fill the member search box (client-side filter, no requests). */
  settingsMembersSearchFill(query: string): Promise<void>;
  /** Whether the centered no-match notice currently shows. */
  settingsMembersNoMatchVisible(): Promise<boolean>;
  /** Open the role Filters dropdown. */
  settingsMembersFiltersOpen(): Promise<void>;
  /** Role checkbox labels offered in the open Filters dropdown. */
  settingsMembersFilterOptions(): Promise<string[]>;
  /** Whether the role checkbox `label` is currently checked. */
  settingsMembersFilterChecked(label: string): Promise<boolean>;
  /** Toggle the role checkbox `label` in the open Filters dropdown. */
  settingsMembersFilterToggle(label: string): Promise<void>;
  /** Close the Filters dropdown without losing the applied filters. */
  settingsMembersFiltersClose(): Promise<void>;
  /** Whether the Filters button shows its active-filters indicator. */
  settingsMembersFilterActive(): Promise<boolean>;
  /** Open the sort menu of the `column` header ("Full name", "Display name", "Email", "Account type", "Joining date"). */
  settingsMembersSortOpen(column: string): Promise<void>;
  /** Items offered in the open column sort menu. */
  settingsMembersSortItems(): Promise<string[]>;
  /** Pick the sort menu item `text`; resolves once the rows settle. */
  settingsMembersSortPick(text: string): Promise<void>;
  /** Move between settings tabs through the sidebar (in-app navigation). */
  settingsMembersGotoTab(tab: string): Promise<void>;
  /** Whether the admin-only add control renders. */
  settingsMembersAddVisible(): Promise<boolean>;
  /** Open the invite dialog via the add control. */
  settingsMembersOpenInvite(): Promise<void>;
  /** Whether the invite dialog is currently open. */
  settingsMembersInviteOpen(): Promise<boolean>;
  /** How many invite rows the open dialog holds. */
  settingsMembersInviteRowCount(): Promise<number>;
  /** Append another invite row ("Add more"). */
  settingsMembersInviteAddRow(): Promise<void>;
  /** Remove the invite row at `index` (0-based). */
  settingsMembersInviteRemoveRow(index: number): Promise<void>;
  /** Open the co-worker picker of invite row `index`. */
  settingsMembersInviteOpenMemberPicker(index: number): Promise<void>;
  /** Co-worker option texts in the open picker, in display order. */
  settingsMembersInviteMemberOptions(): Promise<string[]>;
  /** Pick the co-worker whose option contains `text` in the open picker. */
  settingsMembersInvitePickMember(index: number, text: string): Promise<void>;
  /** Role option labels offered for invite row `index`. */
  settingsMembersInviteRoleOptions(index: number): Promise<string[]>;
  /** Pick role `label` for invite row `index`. */
  settingsMembersInvitePickRole(index: number, label: string): Promise<void>;
  /** Inline error under invite row `index`, or "" when the row shows none. */
  settingsMembersInviteRowError(index: number): Promise<string>;
  /** Submit the invite dialog; resolves once it closes. */
  settingsMembersInviteSubmit(): Promise<void>;
  /** Click submit without waiting for close (validation-failure path). */
  settingsMembersInviteAttemptSubmit(): Promise<void>;
  /** Dismiss the invite dialog without adding ("Cancel"). */
  settingsMembersInviteCancel(): Promise<void>;
  /** Whether the account-type cell of `displayName` offers the inline role editor. */
  settingsMembersRoleEditable(displayName: string): Promise<boolean>;
  /** Role word currently shown in the account-type cell of `displayName`. */
  settingsMembersRoleText(displayName: string): Promise<string>;
  /** Open the inline role editor of `displayName`. */
  settingsMembersRoleEditorOpen(displayName: string): Promise<void>;
  /** Role option labels in the open inline editor. */
  settingsMembersRoleEditorOptions(): Promise<string[]>;
  /** Pick role `label` in the open inline editor; resolves once rows settle. */
  settingsMembersRoleEditorPick(label: string): Promise<void>;
  /** Hover the row of `displayName` and open its contextual menu. */
  settingsMembersOpenRowMenu(displayName: string): Promise<void>;
  /** Item texts in the open row menu. */
  settingsMembersRowMenuItems(): Promise<string[]>;
  /** Pick the row-menu item `text`; resolves once the menu closes. */
  settingsMembersRowMenuPick(text: string): Promise<void>;
  /** Body text of the open remove/leave confirmation, or "" when shut. */
  settingsMembersConfirmText(): Promise<string>;
  /** Confirm the remove/leave dialog; resolves once it closes. */
  settingsMembersConfirmSubmit(): Promise<void>;
  /** Dismiss the remove/leave dialog without acting. */
  settingsMembersConfirmCancel(): Promise<void>;
  /** Whether the members loading skeleton currently shows. */
  settingsMembersSkeletonVisible(): Promise<boolean>;
  /** Delay members-list API answers by `ms` so the skeleton is observable. */
  settingsMembersDelayLoad(ms: number): Promise<void>;
  /** Whether the project-defaults section renders. */
  settingsMembersDefaultsVisible(): Promise<boolean>;
  /** Current lead picker text ("None" when cleared). */
  settingsMembersLeadText(): Promise<string>;
  /** Pick `displayName` (or "None") as project lead; resolves once it saves. */
  settingsMembersLeadPick(displayName: string): Promise<void>;
  /** Current default-assignee picker text ("None" when cleared). */
  settingsMembersAssigneeText(): Promise<string>;
  /** Pick `displayName` (or "None") as default assignee; resolves once it saves. */
  settingsMembersAssigneePick(displayName: string): Promise<void>;
  /** Whether the lead picker is currently disabled (non-admin view). */
  settingsMembersLeadDisabled(): Promise<boolean>;
  /** Guest-access toggle state as rendered. */
  settingsMembersGuestToggleState(): Promise<boolean>;
  /** Flip the guest-access toggle; resolves once it saves. */
  settingsMembersGuestToggle(): Promise<void>;
  /** Whether the guest-access toggle is currently disabled (non-admin view). */
  settingsMembersGuestToggleDisabled(): Promise<boolean>;
  /** Shut the co-worker picker of invite row `index` without picking. */
  settingsMembersInviteCloseMemberPicker(index: number): Promise<void>;
  /** Active sort marker on a member-table column header: ascending, descending, or none. */
  settingsMembersSortMarker(column: string): Promise<"ascending" | "descending" | "none">;

  // --- Workspace-settings cross-cutting oracle (NEWFRONT-415, WSET-033..040).
  // --- Deep-link navigation plus content/denial reads per section, instance
  // --- full-screen states (maintenance, not-ready) with request-interception
  // --- hooks, settings-dialog open/close reads for the Escape matrix, tab
  // --- order, shortcut silence, list-request/socket watchers for the
  // --- no-live-updates row, and drag/import/download absence reads.
  // --- Appended; existing methods above are untouched per the shared
  // --- driver contract.
  /** Open a workspace settings section; settles on content or the denial view. */
  wsSettingsOpenSection(
    workspaceSlug: string,
    section: "general" | "members" | "billing" | "exports" | "webhooks" | "integrations",
    cookies: ParityBrowserCookie[]
  ): Promise<void>;
  /** Whether the not-authorized view is showing instead of section content. */
  wsSettingsNotAuthorized(): Promise<boolean>;
  /** Whether the settings sidebar is showing (authorized shell marker). */
  wsSettingsSidebarVisible(): Promise<boolean>;
  /** Normalized visible text of the settings content area (blank-check). */
  wsSettingsOutletText(): Promise<string>;
  /** Current pathname (deep links must not redirect). */
  wsSettingsCurrentPath(): Promise<string>;
  /** Open one webhook's detail page; settles on form, denial, or loading. */
  wsSettingsOpenWebhookDetail(workspaceSlug: string, webhookId: string, cookies: ParityBrowserCookie[]): Promise<void>;
  /** Webhook detail state: edit form, inline denial, or still loading. */
  wsSettingsWebhookDetailState(): Promise<"form" | "denied" | "loading">;
  /** Desktop-only hooks rendered inside settings (expected: none). */
  wsSettingsDesktopOnlyCount(): Promise<number>;
  /** Fail instance-info reads so the app renders the maintenance screen (test hook). */
  wsSettingsFailInstanceInfo(): Promise<void>;
  /** Serve instance-info as not-provisioned so the not-ready screen renders (test hook). */
  wsSettingsServeInstanceNotReady(): Promise<void>;
  /** Remove the instance-info hooks (test hook). */
  wsSettingsResetInstanceRoutes(): Promise<void>;
  /** Whether the maintenance full-screen message is showing. */
  wsSettingsMaintenanceVisible(): Promise<boolean>;
  /** Maintenance illustration image src, or null when absent. */
  wsSettingsMaintenanceIllustration(): Promise<string | null>;
  /** Maintenance support-link href, or null when absent. */
  wsSettingsMaintenanceSupportHref(): Promise<string | null>;
  /** Not-ready screen facts: heading, admin-entry href, workspace leak check. */
  wsSettingsNotReadyFacts(): Promise<{
    heading: string | null;
    getStartedHref: string | null;
    workspaceContent: boolean;
  }>;
  /** Render under a light or dark color scheme (themed illustration check). */
  wsSettingsSetColorScheme(scheme: "light" | "dark"): Promise<void>;
  /** Open the members invite dialog; resolves once it renders. */
  wsSettingsOpenInviteDialog(workspaceSlug: string, cookies: ParityBrowserCookie[]): Promise<void>;
  /** Open the webhook create dialog; resolves once it renders. */
  wsSettingsOpenWebhookCreateDialog(workspaceSlug: string, cookies: ParityBrowserCookie[]): Promise<void>;
  /** Open the token create dialog; resolves once it renders. */
  wsSettingsOpenTokenCreateDialog(cookies: ParityBrowserCookie[]): Promise<void>;
  /** Open the remove-member confirm for one member email; resolves once it renders. */
  wsSettingsOpenRemoveMemberDialog(
    workspaceSlug: string,
    memberEmail: string,
    cookies: ParityBrowserCookie[]
  ): Promise<void>;
  /** Open the delete-webhook confirm on a detail page; resolves once it renders. */
  wsSettingsOpenDeleteWebhookDialog(
    workspaceSlug: string,
    webhookId: string,
    cookies: ParityBrowserCookie[]
  ): Promise<void>;
  /** Whether any settings dialog is currently open. */
  wsSettingsDialogOpen(): Promise<boolean>;
  /** Button labels of the open dialog, in DOM order. */
  wsSettingsDialogButtons(): Promise<string[]>;
  /** Press Escape once. */
  wsSettingsPressEscape(): Promise<void>;
  /** Close the open dialog via its explicit Cancel/Close button. */
  wsSettingsCloseDialogExplicit(): Promise<void>;
  /** Focus labels across `presses` Tabs plus whether focus left the dialog. */
  wsSettingsDialogTabOrder(presses: number): Promise<{ order: string[]; escaped: boolean }>;
  /** Press keys with the page body focused (shortcut-silence check). */
  wsSettingsPressKeys(keys: string[]): Promise<void>;
  /** Fill the members search, press Escape, and read back value plus visibility. */
  wsSettingsMembersSearchEscapeProbe(query: string): Promise<{ value: string; visible: boolean }>;
  /** Start counting settings list reads (members, webhooks, tokens). */
  wsSettingsWatchListRequests(): Promise<void>;
  /** List reads observed since the watcher armed. */
  wsSettingsListRequestCount(): Promise<number>;
  /** Start counting websocket connections. */
  wsSettingsWatchSockets(): Promise<void>;
  /** Sockets observed since the watcher armed. */
  wsSettingsSocketCount(): Promise<number>;
  /** Member rows as rendered (display order snapshot). */
  wsSettingsMembersListText(): Promise<string[]>;
  /** Webhook rows as rendered (display order snapshot). */
  wsSettingsWebhooksListText(): Promise<string[]>;
  /** Draggable elements inside the settings content (expected: none). */
  wsSettingsDraggableCount(): Promise<number>;
  /** Drag the first member row onto the second (reorder attempt). */
  wsSettingsDragFirstMemberRowToSecond(): Promise<void>;
  /** File inputs inside the settings content. */
  wsSettingsFileInputCount(): Promise<number>;
  /** Visible "import" mentions inside the settings content (expected: none). */
  wsSettingsImportMentions(): Promise<string[]>;
  /** Start counting browser downloads. */
  wsSettingsWatchDownloads(): Promise<void>;
  /** Downloads observed since the watcher armed. */
  wsSettingsDownloadCount(): Promise<number>;

  // --- Workspace billing oracle (NEWFRONT-414, WSET-029..032). Observed on
  // --- the running old app: admin-only billing page with a community-plan
  // --- summary box, a plan comparison (pro/business/enterprise cards, the
  // --- pro card carrying a popularity marker, prices and a monthly/yearly
  // --- switch with a discount badge), an expandable full-feature
  // --- comparison, and plan buttons that open external pricing/sales pages
  // --- in a new tab; the paid-plan upgrade dialog still opens from the
  // --- sidebar user-menu community entry (bug: intended removed) while
  // --- the pro pills elsewhere are static labels. Appended; existing
  // --- methods above are untouched per the shared driver contract.
  /** Open the workspace billing page; settles on the plan content or the denial view. */
  wsSettingsBillingOpen(workspaceSlug: string): Promise<void>;
  /** Whether the billing page shows the not-authorized view. */
  wsSettingsBillingNotAuthorized(): Promise<boolean>;
  /** Community summary box title, or null when the box is absent. */
  wsSettingsBillingCommunityTitle(): Promise<string | null>;
  /** Community summary box supporting text, or null when the box is absent. */
  wsSettingsBillingCommunityDescription(): Promise<string | null>;
  /** Plan names in the comparison, in display order. */
  wsSettingsBillingPlanNames(): Promise<string[]>;
  /** Which plan carries the popularity marker, or null when none does. */
  wsSettingsBillingPopularPlan(): Promise<string | null>;
  /** Displayed price line for a plan, or null when the card shows none. */
  wsSettingsBillingPlanPrice(planName: string): Promise<string | null>;
  /** Whether a plan card shows the monthly/yearly switch. */
  wsSettingsBillingFrequencyVisible(planName: string): Promise<boolean>;
  /** The currently selected billing frequency on a plan card, or null. */
  wsSettingsBillingSelectedFrequency(planName: string): Promise<"month" | "year" | null>;
  /** Select a billing frequency on a plan card; resolves once prices settle. */
  wsSettingsBillingSelectFrequency(planName: string, frequency: "month" | "year"): Promise<void>;
  /** Discount badge text on a plan card (e.g. "-25%"), or null when absent. */
  wsSettingsBillingDiscountBadge(planName: string): Promise<string | null>;
  /** Label of the expand/collapse full-comparison control, or null. */
  wsSettingsBillingCompareToggleLabel(): Promise<string | null>;
  /** Flip the full-comparison section; resolves once the section settles. */
  wsSettingsBillingToggleCompare(): Promise<void>;
  /** Whether the full-feature comparison section is currently expanded. */
  wsSettingsBillingComparisonVisible(): Promise<boolean>;
  /** Full-comparison section titles in display order (empty when collapsed). */
  wsSettingsBillingComparisonSections(): Promise<string[]>;
  /** Plan card action-button label, or null when the card shows none. */
  wsSettingsBillingPlanButtonLabel(planName: string): Promise<string | null>;
  /** Click a plan card action button and return the new-tab URL it opens (closes the tab). */
  wsSettingsBillingPlanButtonPopupUrl(planName: string): Promise<string | null>;
  /** Full visible text of the billing page (for pinned non-behavior assertions). */
  wsSettingsBillingPageText(): Promise<string>;
  /** Whether the paid-plan upgrade dialog is currently open. */
  wsSettingsUpgradeModalVisible(): Promise<boolean>;
  /** Upgrade dialog heading text, or null when the dialog is closed. */
  wsSettingsUpgradeModalTitle(): Promise<string | null>;
  /** Open the workspace home user menu and activate its community entry. */
  wsSettingsUserMenuOpenCommunity(workspaceSlug: string): Promise<void>;
  /** Open the workspace active-cycles page (a pro-pill host); settles on its header. */
  wsSettingsActiveCyclesOpen(workspaceSlug: string): Promise<void>;
  /** Close the upgrade dialog; resolves once it is gone. */
  wsSettingsUpgradeModalClose(): Promise<void>;
  /** Number of pro pills (upgrade badges) on the current page. */
  wsSettingsProBadgeCount(): Promise<number>;
  /** Click the first pro pill on the current page, if any. */
  wsSettingsProBadgeClickFirst(): Promise<void>;

  // --- Workspace-settings shell + general oracle (NEWFRONT-410,
  // --- WSET-001..007). Observed on the running old app: the settings
  // --- layout gates each section by workspace role (general, members and
  // --- exports are admin-or-member; billing, webhooks and integrations
  // --- are admin-only; guests see the not-authorized view everywhere and
  // --- the tab title stays empty); the sidebar groups entries under
  // --- Administration and Developer (empty groups hidden), highlights
  // --- the current section, and collapses into a hamburger drawer with
  // --- an active-path crumb on narrow screens; the general page carries
  // --- the logo block (initial-letter fallback, upload dialog, remove),
  // --- the name/size/URL/timezone form (admin editable, members see it
  // --- dimmed and disabled), the copy-URL control and the admin-only
  // --- delete danger zone; every section titles its tab "<workspace> -
  // --- <section>". Appended; existing methods above are untouched per
  // --- the shared driver contract.
  /**
   * Open one workspace-settings section; settles on the section or the
   * denial view. `section` is "" (general) or one of "members",
   * "billing", "exports", "webhooks", "integrations".
   */
  wsSettingsOpen(workspaceSlug: string, section: string): Promise<void>;
  /** Sidebar group labels in display order (wide layout). */
  wsSettingsGroups(): Promise<string[]>;
  /** Sidebar entry labels in display order (wide layout). */
  wsSettingsItems(): Promise<string[]>;
  /** Label of the highlighted sidebar entry, or null when none is. */
  wsSettingsActiveItem(): Promise<string | null>;
  /** Follow the sidebar entry with this label; waits until settled. */
  wsSettingsFollowItem(label: string): Promise<void>;
  /** The current location's path (what the address bar shows). */
  wsSettingsPath(): Promise<string>;
  /** The current browser tab title. */
  wsSettingsTitle(): Promise<string>;
  /** Whether the not-authorized view is currently shown. */
  wsSettingsDeniedVisible(): Promise<boolean>;
  /** Whether the narrow-screen drawer trigger is currently visible. */
  wsSettingsNarrowTriggerVisible(): Promise<boolean>;
  /** Narrow the viewport and open the drawer; resolves with entries listed. */
  wsSettingsOpenDrawer(): Promise<void>;
  /** Drawer entry labels in display order. */
  wsSettingsDrawerItems(): Promise<string[]>;
  /** The active-path crumb beside the narrow-screen trigger. */
  wsSettingsMobileCrumb(): Promise<string>;
  /** Restore the wide viewport after narrow-screen reads. */
  wsSettingsWideViewport(): Promise<void>;
  /** Whether the general details form is currently rendered. */
  wsSettingsGeneralVisible(): Promise<boolean>;
  /** Current value of the workspace-name field. */
  wsSettingsNameValue(): Promise<string>;
  /** Fill the workspace-name field (replacing its content). */
  wsSettingsFillName(name: string): Promise<void>;
  /** Whether the workspace-name field is currently disabled. */
  wsSettingsNameDisabled(): Promise<boolean>;
  /** Inline name-validation error text, or null when none shows. */
  wsSettingsNameError(): Promise<string | null>;
  /** Label currently shown on the organization-size control. */
  wsSettingsSizeLabel(): Promise<string>;
  /** Organization-size preset labels offered by the control. */
  wsSettingsSizeOptions(): Promise<string[]>;
  /** Pick an organization-size preset by its label. */
  wsSettingsSelectSize(label: string): Promise<void>;
  /** Label currently shown on the timezone control. */
  wsSettingsTimezoneLabel(): Promise<string>;
  /** Search timezones for `query` and pick the `option` entry. */
  wsSettingsSelectTimezone(query: string, option: string): Promise<void>;
  /** Value of the read-only workspace-URL field. */
  wsSettingsUrlValue(): Promise<string>;
  /** Whether the save control is currently rendered (admin only). */
  wsSettingsSaveVisible(): Promise<boolean>;
  /** Label on the save control ("Update workspace" vs "Updating"), or null. */
  wsSettingsSaveLabel(): Promise<string | null>;
  /** Click the save control; resolves once the write settles. */
  wsSettingsSave(): Promise<void>;
  /** Hold workspace PATCH responses so the saving state stays observable. */
  wsSettingsDelaySave(ms: number): Promise<void>;
  /** Fail workspace PATCH responses with HTTP 500 until cleared. */
  wsSettingsFailSave(): Promise<void>;
  /** Remove the save interception installed above. */
  wsSettingsClearSaveRoutes(): Promise<void>;
  /** Whether the logo block shows the initial fallback or an image. */
  wsSettingsLogoState(): Promise<"fallback" | "image">;
  /** Resolved logo image src, or null while the fallback shows. */
  wsSettingsLogoSrc(): Promise<string | null>;
  /** Which logo action the page offers: upload, edit, or none (members). */
  wsSettingsLogoAction(): Promise<"upload" | "edit" | "none">;
  /** Open the logo upload dialog. */
  wsSettingsOpenLogoDialog(): Promise<void>;
  /** Whether the logo upload dialog is currently open. */
  wsSettingsLogoDialogVisible(): Promise<boolean>;
  /** Choose `filePath` in the dialog and save; resolves once it closes. */
  wsSettingsUploadLogo(filePath: string): Promise<void>;
  /** Click Remove in the open logo dialog; resolves once the write settles and the dialog closes. */
  wsSettingsRemoveLogo(): Promise<void>;
  /** Close the logo upload dialog via its Cancel control. */
  wsSettingsCloseLogoDialog(): Promise<void>;
  /** Activate the copy-URL control (clipboard permission granted first). */
  wsSettingsCopyUrl(): Promise<void>;
  /** Current clipboard text (grants clipboard permission first). */
  wsSettingsReadClipboard(): Promise<string>;
  /** Whether the delete danger section is currently rendered (admin only). */
  wsSettingsDeleteSectionVisible(): Promise<boolean>;
  /** Open the delete confirmation dialog. */
  wsSettingsOpenDeleteDialog(): Promise<void>;
  /** Whether the delete confirmation dialog is currently open. */
  wsSettingsDeleteDialogVisible(): Promise<boolean>;
  /** Visible text of the delete confirmation dialog. */
  wsSettingsDeleteDialogText(): Promise<string>;
  /** Fill the workspace-name confirmation field. */
  wsSettingsFillDeleteName(name: string): Promise<void>;
  /** Fill the final-phrase confirmation field. */
  wsSettingsFillDeletePhrase(text: string): Promise<void>;
  /** Whether the delete Confirm control is currently enabled. */
  wsSettingsConfirmDeleteEnabled(): Promise<boolean>;
  /** Click Confirm; resolves once the app navigates away from settings. */
  wsSettingsConfirmDelete(): Promise<void>;
  /** Dismiss the delete dialog via its Cancel control. */
  wsSettingsCancelDelete(): Promise<void>;
}

/** Inline image in the page body editor (NEWFRONT-316, PAGE-039). */
export interface PagesBlocksInlineImage {
  /** Resolved src attribute. */
  src: string;
  /** Alt text, or null when the image carries none. */
  alt: string | null;
}

/** Asset-tab row for a page attachment (NEWFRONT-316, PAGE-039). */
export interface PagesBlocksAssetRow {
  /** Display name of the asset. */
  name: string;
  /** Kind label the tab shows (image, file, ...), or null when absent. */
  kind: string | null;
}

/** Info-tab document stats (NEWFRONT-316, PAGE-037). */
export interface PagesBlocksInfoStats {
  /** Word count the tab reports, or null when it shows none. */
  words: number | null;
  /** Character count the tab reports, or null when it shows none. */
  characters: number | null;
}

/** Pages list tabs as the row-actions scenarios address them (NEWFRONT-313). */
export type PagesRowTab = "open" | "restricted" | "shelved";

/** Current page mark as the detail header renders it (NEWFRONT-315, PAGE-027). */
export interface PagesMarkCurrent {
  kind: "emoji" | "icon";
  /** Emoji glyph, or the vector-icon name. */
  label: string;
}

/** One toolbar tooltip: action name plus optional key hint (NEWFRONT-315, PAGE-029-031). */
export interface PagesToolbarTooltip {
  name: string;
  hint: string | null;
}

/** Color popover swatch counts per row (NEWFRONT-315, PAGE-036). */
export interface PagesToolbarColorCounts {
  text: number;
  textClear: boolean;
  background: number;
  backgroundClear: boolean;
}

/** Color preview state on the toolbar button (NEWFRONT-315, PAGE-036). */
export interface PagesToolbarColorPreview {
  text: string | null;
  background: string | null;
}

/** Most recent toast: title plus message (NEWFRONT-308, MOD-013/014/016). */
export interface ModulesToast {
  /** Toast title. */
  title: string;
  /** Toast message. */
  message: string;
}

/** One overflow menu entry with its gating state (NEWFRONT-308, MOD-017). */
export interface ModulesMenuEntry {
  /** Entry title. */
  title: string;
  /** Whether the entry renders disabled. */
  disabled: boolean;
  /** Explanation under the title, or null when none shows. */
  description: string | null;
}

/** One timeline sidebar row (NEWFRONT-308, MOD-021). */
export interface ModulesTimelineRow {
  /** Module name. */
  name: string;
  /** Detail href, or null when the row is not a link. */
  href: string | null;
  /** Whether a status glyph renders beside the name. */
  statusGlyph: boolean;
}

/** One outline heading as the pane lists it (NEWFRONT-317, PAGE-047). */
export interface PagesPaneOutlineHeading {
  /** Heading level from indentation, or null when unreadable. */
  level: 1 | 2 | 3 | null;
  /** Visible heading text. */
  text: string;
}

/** One assets-tab row as the pane lists it (NEWFRONT-317, PAGE-048). */
export interface PagesPaneAssetRow {
  /** Visible file name. */
  name: string;
  /** Whether the row renders a thumbnail. */
  thumbnail: boolean;
  /** Row link target (view variant). */
  viewHref: string;
  /** Download action target (download variant). */
  downloadHref: string;
}

/** Live document statistics cards (NEWFRONT-317, PAGE-049). */
export interface PagesPaneStats {
  /** Rendered words count. */
  words: number;
  /** Rendered characters count. */
  characters: number;
  /** Rendered paragraphs count. */
  paragraphs: number;
  /** Rendered estimated reading time. */
  readTime: string;
}

/** One info-tab actor row (NEWFRONT-317, PAGE-050). */
export interface PagesPaneActor {
  /** Which row this is: the last editor or the creator. */
  role: "edited" | "created";
  /** Visible display name (or the deactivated fallback). */
  name: string;
  /** Profile link target. */
  profileHref: string;
  /** Visible time text (relative for the editor, calendar for the creator). */
  timeText: string;
}

/** One version-timeline entry (NEWFRONT-317, PAGE-051). */
export interface PagesPaneVersionEntry {
  /** Snapshot id, or null for the current-version row. */
  id: string | null;
  /** Visible entry label (row title or snapshot timestamp). */
  label: string;
  /** Visible author name, or empty for the current-version row. */
  author: string;
  /** Whether the entry shows highlighted as active. */
  active: boolean;
}

/** Version-overlay header facts (NEWFRONT-317, PAGE-052). */
export interface PagesPaneOverlayHeader {
  /** Visible header title (snapshot timestamp or the loading text). */
  title: string;
  /** Whether the read-only marker shows. */
  viewOnly: boolean;
}

/** One collaboration socket close as the browser observed it. */
export interface PagesLiveSocketClose {
  /** WebSocket close code (1006 when the drop carried no close frame). */
  code: number;
  /** Close reason (empty when the drop carried no close frame). */
  reason: string;
}

/** One external footer link of the product-updates dialog (NEWFRONT-208). */
export interface DeskSigninUpdateLink {
  /** Visible label. */
  label: string;
  /** Lower-cased tag name of the rendered element. */
  tag: string;
  /** Resolved destination address. */
  href: string;
  /** Link target attribute, or null when absent. */
  target: string | null;

  // ---------------------------------------------------------------------------
  // Analytics shell, scoping, permissions (NEWFRONT-375, ANL-001..004 + ANL-018).
  // The workspace analytics page: tab addressing, header breadcrumb, document
  // title, no-project empty view, project picker, and guest gating.
  // Appended; existing entries above are untouched per the shared contract.
  // ---------------------------------------------------------------------------

  /** Open an analytics tab authenticated; resolves once tabs or the empty view settle. */
  analyticsOpenTab(workspaceSlug: string, tabId: string, cookies: ParityBrowserCookie[]): Promise<void>;
  /** Open the bare analytics address authenticated; resolves once the redirect settles. */
  analyticsOpenRoot(workspaceSlug: string, cookies: ParityBrowserCookie[]): Promise<void>;
  /** Current page path plus query (tab and redirect assertions). */
  analyticsCurrentPath(): Promise<string>;
  /** Tab trigger values in display order. */
  analyticsTabIds(): Promise<string[]>;
  /** Selected tab trigger value, or null when no trigger is selected. */
  analyticsSelectedTab(): Promise<string | null>;
  /** Click a tab trigger; resolves once the address carries it. */
  analyticsSelectTab(tabId: string): Promise<void>;
  /** Whether the named tab's panel content renders (false for unknown ids). */
  analyticsPanelVisible(tabId: string): Promise<boolean>;
  /** App-header breadcrumb texts in display order. */
  analyticsBreadcrumb(): Promise<string[]>;
  /** Document title of the current page. */
  analyticsPageTitle(): Promise<string>;
  /** No-project empty view facts, or null when the tabs view renders. */
  analyticsEmptyFacts(): Promise<AnalyticsEmptyFacts | null>;
  /** Click the empty view's project-creation action. */
  analyticsEmptyCreateProject(): Promise<void>;
  /** Labels of enabled project-creation controls on the current view (ANL-018 negative). */
  analyticsCreateAffordances(): Promise<string[]>;
  /** Project picker button label (all-projects, names, or the count summary). */
  analyticsPickerLabel(): Promise<string>;
  /** Open the project picker menu. */
  analyticsPickerOpen(): Promise<void>;
  /** Dismiss the project picker menu. */
  analyticsPickerClose(): Promise<void>;
  /** Visible picker option names in display order. */
  analyticsPickerOptions(): Promise<string[]>;
  /** Type into the picker search box. */
  analyticsPickerSearch(text: string): Promise<void>;
  /** Toggle one picker option by project name. */
  analyticsPickerToggle(name: string): Promise<void>;
  /** Names currently checked in the open picker. */
  analyticsPickerChecked(): Promise<string[]>;
  /** Start capturing analytics API reads (path plus query). */
  analyticsStartRequestSpy(): Promise<void>;
  /** Stop the analytics request spy. */
  analyticsStopRequestSpy(): Promise<void>;
  /** Analytics API reads captured since the spy started. */
  analyticsSpyRequests(): Promise<AnalyticsApiRequest[]>;
}

/** Canonical filter-section keys for the intake list panel. */
export type IntakeListFilterKey =
  | "status"
  | "priority"
  | "assignees"
  | "createdBy"
  | "labels"
  | "createdAt"
  | "updatedAt";

/** One applied-filter chip group: canonical key plus data-identity values. */
export interface IntakeListChip {
  /** Canonical group key. */
  key: IntakeListFilterKey;
  /** Chip values in display order. */
  values: string[];
}

/** Order-menu state: the checked field plus the checked direction. */
export interface IntakeListOrderState {
  /** Checked order field. */
  field: IntakeListOrderField;
  /** Checked sort direction. */
  direction: "asc" | "desc";
}

/** Canonical order fields for the intake list. */
export type IntakeListOrderField = "created" | "updated" | "id";

/** A hover tooltip: heading plus content. */
export interface IntakeListTooltip {
  /** Heading line (empty when the tooltip shows content only). */
  heading: string;
  /** Content text. */
  content: string;
}

/** One catalog table row: the user-visible definition facts. */
export interface SchedulerCatalogRow {
  /** Display name. */
  name: string;
  /** URL handle (the slug). */
  handle: string;
  /** Origin mark text (e.g. Built-in). */
  origin: string;
  /** Install-count cell text (e.g. "2 installs"). */
  installs: string;
  /** Status mark text (e.g. Enabled). */
  status: string;
  /** Last-update cell text (locale-rendered; scenarios assert presence, not format). */
  updated: string;
}

/** Current values of the definition dialog's fields. */
export interface SchedulerDefinitionValues {
  name: string;
  handle: string;
  description: string;
  prompt: string;
  /** Selected swatch hex (e.g. #3b82f6). */
  color: string;
  enabled: boolean;
}

/** One bulk-install picker option. */
export interface SchedulerInstallOption {
  name: string;
  identifier: string | null;
  /** Checked in the picker (selected, or locked as already installed). */
  checked: boolean;
  /** Already installed: checked and cannot be toggled. */
  locked: boolean;
}

/** One project install-list row: the user-visible install facts. */
export interface SchedulerProjectRow {
  /** Display name. */
  name: string;
  /** URL handle (the slug). */
  handle: string;
  /** Plain-language schedule sentence. */
  schedule: string;
  /** Stored-rule tooltip behind the schedule cell. */
  scheduleTitle: string;
  /** Next-run cell text (locale-rendered; scenarios assert presence, not format). */
  nextRun: string;
  /** Last-run cell text (locale-rendered, or the never-fired mark). */
  lastRun: string;
  /** Status mark text (e.g. Enabled). */
  status: string;
  /** Last-update cell text (locale-rendered; scenarios assert presence, not format). */
  updated: string;
  /** Whether the row offers Edit/Uninstall management controls. */
  manageVisible: boolean;
}

/** One definition offered by the project install-path picker. */
export interface SchedulerProjectInstallOption {
  name: string;
  handle: string;
  selected: boolean;
}

/** Current values of the shared install/edit schedule fields. */
export interface SchedulerScheduleValues {
  /** `datetime-local` input value ("YYYY-MM-DDTHH:mm", browser-local). */
  dtstart: string;
  tzid: string;
  rrule: string;
  extraContext: string;
  enabled: boolean;
}

/** Current values of every edit-dialog field. */
export interface SchedulerBindingValues extends SchedulerScheduleValues {
  /** Visible label of the checked outcome-mode option. */
  outcomeLabel: string;
  /** Selected pod option value ("" is the project default). */
  pod: string;
}

/** Install-detail header facts. */
export interface SchedulerBindingHeader {
  name: string;
  handle: string;
  /** Badge texts (origin mark, workspace-disabled mark). */
  badges: string[];
  workspaceLinkVisible: boolean;
  editVisible: boolean;
  uninstallVisible: boolean;
}

/** One install-detail run-history row. */
export interface SchedulerBindingRunRow {
  started: string;
  ended: string;
  status: string;
  duration: string;
  pod: string;
  result: string;
}

/** One calendar occurrence block. */
export interface SchedulerCalendarBlock {
  /** Containing day: the month-cell numeral ("9") or the week day-header text. */
  day: string;
  /** Block time text (locale-rendered). */
  time: string;
  /** Scheduler display name. */
  name: string;
  /** Block tooltip (name, time, status). */
  title: string;
  /** Computed block background color (past grey vs scheduler tint). */
  background: string;
}

/** One runners tab: label, link target, active marker. */
export interface RunnersTab {
  label: string;
  href: string | null;
  active: boolean;
}

/** One runner table row: the columns the overview renders. */
export interface RunnersTableRow {
  name: string;
  pod: string;
  status: string;
  osArch: string;
  version: string;
  heartbeat: string;
}

/** Actions cell of one runner row. */
export interface RunnersRowActions {
  hasDetails: boolean;
  hasRevoke: boolean;
  hasDelete: boolean;
}

/** Side-nav rail snapshot. */
export interface RunnersRail {
  header: string;
  overviewHref: string | null;
  contacts: RunnersRailContact[];
  emptyVisible: boolean;
}

/** One rail chat contact. */
export interface RunnersRailContact {
  name: string;
  href: string | null;
  hasDot: boolean;
}

/** One pod tile. */
export interface RunnersPodTile {
  name: string;
  isDefault: boolean;
  runnerCount: string;
}

/** Edit-pod form values. */
export interface RunnersEditPodForm {
  name: string;
  description: string;
  isDefault: boolean;
  defaultDisabled: boolean;
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

/** Observed document-shell head facts (NEWFRONT-173, SHELL-107). */
export interface DocumentShellFacts {
  /** The document language tag, or null when absent. */
  lang: string | null;
  /** The rendered tab title. */
  title: string;
  /** Meta description content, or null when absent. */
  description: string | null;
  /** Whether a keywords meta tag is present. */
  keywordsPresent: boolean;
  /** Meta viewport content, or null when absent. */
  viewport: string | null;
  /** Meta theme-color content, or null when absent. */
  themeColor: string | null;
  /** Meta robots content (crawl directives), or null when absent. */
  robots: string | null;
  /** Open-graph title, or null when absent. */
  ogTitle: string | null;
  /** Open-graph description, or null when absent. */
  ogDescription: string | null;
  /** Open-graph canonical URL, or null when absent. */
  ogUrl: string | null;
  /** Open-graph image reference, or null when absent. */
  ogImage: string | null;
  /** Open-graph image pixel size, or null when absent. */
  ogImageSize: { width: number; height: number } | null;
  /** Open-graph image alternative text, or null when absent. */
  ogImageAlt: string | null;
  /** Twitter card handle, or null when absent. */
  twitterSite: string | null;
  /** Twitter card layout token, or null when absent. */
  twitterCard: string | null;
  /** PWA capability markers (application name, mobile-web flags), or null when absent. */
  installability: {
    applicationName: string | null;
    appleMobileCapable: string | null;
    mobileWebCapable: string | null;
  };
  /** Hrefs of tab-icon links (rel icon / shortcut icon). */
  iconHrefs: string[];
  /** Hrefs of home-screen icon links (rel apple-touch-icon). */
  appleTouchIconHrefs: string[];
  /** Hrefs of install-manifest links (rel manifest). */
  manifestHrefs: string[];
  /** The color-scheme style the theme provider sets on the root element, or null. */
  rootColorScheme: string | null;
  /** Whether the shell mounts a main content landmark for the page. */
  mainMounted: boolean;
}

/** Observed not-found surface facts (NEWFRONT-173, SHELL-108). */
export interface NotFoundFacts {
  /** The rendered tab title while the surface shows. */
  title: string;
  /** The surface heading text, or null when absent. */
  heading: string | null;
  /** The explanatory body text, or null when absent. */
  body: string | null;
  /** The way-home link target, or null when absent. */
  homeHref: string | null;
  /** The way-home link label, or null when absent. */
  homeLabel: string | null;
  /** The explanatory illustration, or null when absent. */
  illustration: { src: string; alt: string; status: number } | null;
  /** Meta robots content while the surface shows, or null when absent. */
  robots: string | null;
}

/** Served-document shell markers (NEWFRONT-173, SHELL-107). */
export interface ServedShellMarkers {
  /** HTTP status of the served document. */
  status: number;
  /** Whether the served markup carries a non-empty tab title. */
  hasTitle: boolean;
  /** Whether the served markup carries a non-empty description meta tag. */
  hasDescription: boolean;
  /** Whether the served markup carries the social-card meta set. */
  hasSocial: boolean;
  /** Whether the served markup links tab icons. */
  hasIcons: boolean;
  /** Whether the served markup links install manifests. */
  hasManifests: boolean;
  /** Whether the served markup mounts both overlay portal roots. */
  hasPortals: boolean;
  /** Whether the served markup loads a session-recorder snippet. */
  hasRecorder: boolean;
}

/** One dev-machines table row (NEWFRONT-183, RUN-037). */
export interface DevMachineRow {
  /** Display name: label, else host label, else id prefix. */
  name: string;
  /** Secondary line under the name (host label or short id). */
  subline: string;
  /** Client-derived status badge text. */
  status: string;
  /** Active/total runner counts line. */
  runners: string;
  /** Rendered last-seen time. */
  lastSeen: string;
  /** Rendered last-heartbeat time. */
  lastHeartbeat: string;
  /** Row action labels in display order. */
  actions: string[];
}

/** An open dev-machine confirm modal (NEWFRONT-183, RUN-038–040). */
export interface DevMachineModal {
  /** Modal title. */
  title: string;
  /** Warning body. */
  body: string;
  /** Confirm button label. */
  confirmLabel: string;
}

/** One pidash-CLI install card (NEWFRONT-183, RUN-041). */
export interface DevMachineInstallCard {
  /** Platform label. */
  label: string;
  /** Displayed install command. */
  command: string;
  /** Direct download link target, or null when the card has none. */
  downloadHref: string | null;
}

/**
 * One canned chat-stream frame (NEWFRONT-181, RUN-029). The driver turns
 * these into SSE `chat.event` frames on the wire shape the backend
 * serializes (session id, sequence, kind, payload, message link, stamp).
 */
export interface RunnerChatStreamFrame {
  seq: number;
  kind: string;
  payload: Record<string, unknown>;
  /** Assistant-delta target bubble; null for session-level frames. */
  message?: string | null;
}

/** One prompt-section card with its provenance and edit affordances (NEWFRONT-186, AGT-023). */
export interface PromptSectionCard {
  /** Registry key. */
  key: string;
  /** Display title. */
  title: string;
  /** Provenance badge text (default / workspace override / personal override). */
  sourceBadge: string;
  /** Governance and kind badge texts in display order. */
  badges: string[];
  /** Prompt-kind labels the card belongs to. */
  kinds: string[];
  /** Whether the stale-override warning line shows. */
  staleWarning: boolean;
  /** Effective body text shown on the card (empty while the editor is open). */
  body: string;
  /** Workspace-edit button label, or null when the control hides. */
  workspaceEditLabel: string | null;
  /** Personal-edit button label, or null when the control hides. */
  personalEditLabel: string | null;
}

/** Open section-editor state (NEWFRONT-186, AGT-024–026). */
export interface PromptEditorState {
  /** Scope caption above the draft. */
  scopeLabel: string;
  /** Current draft text. */
  draft: string;
  /** Whether Save is enabled (dirty and idle). */
  saveEnabled: boolean;
  /** Whether the pristine-default pane shows. */
  defaultVisible: boolean;
  /** Pristine-default text, or null when the pane hides. */
  defaultBody: string | null;
  /** Whether the revert affordance shows (an override exists at this scope). */
  revertVisible: boolean;
  /** Inline error text, or null when none shows. */
  error: string | null;
}

/** Open revert-confirmation dialog (NEWFRONT-186, AGT-027). */
export interface PromptRevertDialog {
  /** Dialog title. */
  title: string;
  /** Explanatory body with the irreversibility warning. */
  body: string;
  /** Confirm button label. */
  confirmLabel: string;
}

/** One receipt card with its composed-section list (NEWFRONT-186, AGT-028). */
export interface PromptReceiptCard {
  /** Kind label. */
  kind: string;
  /** Rendered section-count badge text. */
  countBadge: string;
  /** Composed sections in order with their provenance. */
  sections: { num: string; title: string; key: string; sourceBadge: string }[];
}

/** One idle-automation row (NEWFRONT-186, AGT-034–035). */
export interface AutomationRow {
  /** Whether the row toggle reads on. */
  toggleOn: boolean;
  /** Whether the toggle is disabled for this viewer. */
  toggleDisabled: boolean;
  /** Whether the delay picker panel shows. */
  pickerVisible: boolean;
  /** Current delay label (empty when the picker hides). */
  pickerLabel: string;
}

/** Auto-close row: delay plus cancelled-state target (NEWFRONT-186, AGT-035). */
export interface AutomationCloseRow extends AutomationRow {
  /** Current close-state label. */
  stateLabel: string;
  /** Whether the state picker is disabled (fewer than two cancelled states). */
  statePickerDisabled: boolean;
}

/** Open custom-month dialog (NEWFRONT-186, AGT-036). */
export interface AutomationMonthModal {
  /** Dialog title. */
  title: string;
  /** Current input value. */
  inputValue: string;
  /** Inline range error, or null when none shows. */
  error: string | null;
}

/** Add-runner form values as the controls render them (NEWFRONT-179, RUN-006). */
export interface AddRunnerFormState {
  /** Machine picker button label. */
  machine: string;
  /** Project picker button label. */
  project: string;
  /** Whether the project picker is locked (project-scoped entry). */
  projectLocked: boolean;
  /** Pod picker button label. */
  pod: string;
  /** Runner name field value. */
  name: string;
  /** Working-directory field value. */
  workingDir: string;
  /** Agent picker button label. */
  agent: string;
  /** Model picker button label. */
  model: string;
}

/** Remote-create panel phase (NEWFRONT-179, RUN-007). */
export type AddRunnerRemotePhase = "creating" | "ok" | "error" | "timeout";

/** Landing greeting headline plus caption (NEWFRONT-187, AGT-038). */
export interface AssistantLandingGreeting {
  /** Headline text. */
  headline: string;
  /** Caption text. */
  caption: string;
}

/** One transcript bubble: role plus visible text (NEWFRONT-187, AGT-047). */
export interface AssistantBubble {
  /** user, assistant, tool, error or notice. */
  role: string;
  /** Visible text. */
  text: string;
}

/** One tool-activity row with its deep links (NEWFRONT-187, AGT-046). */
export interface AssistantToolActivity {
  /** Activity text without link labels. */
  text: string;
  /** Deep links in display order. */
  links: { label: string; href: string }[];
}

/** One sidebar thread row (NEWFRONT-187, AGT-044). */
export interface AssistantSidebarThread {
  /** Title or the untitled fallback. */
  title: string;
  /** Deep-link href. */
  href: string;
  /** Whether this is the open thread. */
  active: boolean;
}

/**
 * One canned assistant SSE frame (NEWFRONT-187, AGT-042). The driver renders
 * these into SSE `chat.event` frames on the wire shape the backend
 * serializes (thread id, sequence, kind, payload, message link, stamp).
 */
export interface AssistantStreamFrame {
  seq: number;
  kind: string;
  payload: Record<string, unknown>;
  /** Delta/message target row; null for turn-level frames. */
  message?: string | null;
}

/** Assistant API call counts between spy start and read (NEWFRONT-187). */
export interface AssistantApiCounts {
  threadCreate: number;
  send: number;
  cancel: number;
  threadList: number;
  messageList: number;
}

/** Inbox stream-tab key (NEWFRONT-198, NTF-002). */
export type NotificationsTab = "all" | "mentions";

/**
 * What the inbox detail pane shows (NEWFRONT-199, NTF-007..009): the
 * no-selection placeholder, the access-lookup loading indicator, the
 * triage-queue embed, or the ordinary work-item peek overview.
 */
export type NotificationsDetailVariant = "placeholder" | "loading" | "triage" | "peek";

/** One inbox card as the user reads it (NEWFRONT-198, NTF-005..006). */
export interface NotificationsCard {
  /** Actor display text (who acted). */
  actor: string;
  /** Human-readable change summary line. */
  summary: string;
  /** Work-item reference as shown (identifier plus sequence). */
  reference: string;
  /** Work-item title as shown. */
  title: string;
  /** Relative age label as shown. */
  age: string;
  /** Whether the unread marker currently shows on the card. */
  unread: boolean;
}

/** Email-preference toggle key (NEWFRONT-201, NTF-024). */
export type NotificationsEmailPref = "property_change" | "state_change" | "issue_completed" | "comment" | "mention";

/** Inbox origin-filter key (NEWFRONT-200, NTF-015). */
export type NotificationsOrigin = "assigned" | "created" | "subscribed";

/** Inbox overflow-mode key (NEWFRONT-200, NTF-016). */
export type NotificationsMode = "unread" | "archived" | "snoozed";

/** One origin-filter menu option as the user reads it (NEWFRONT-200, NTF-015). */
export interface NotificationsFilterOption {
  /** Origin key carried by the option's test hook. */
  value: NotificationsOrigin;
  /** Visible option label. */
  label: string;
  /** Whether the option currently shows its checkmark. */
  checked: boolean;
}

/** One applied-filter chip as the user reads it (NEWFRONT-200, NTF-015). */
export interface NotificationsAppliedChip {
  /** Origin key carried by the chip's test hook. */
  origin: NotificationsOrigin;
  /** Visible chip label. */
  label: string;
}

/** Query params of one inbox list fetch, as the UI requested them. */
export interface NotificationsListQuery {
  /** Selected origins the UI sent, or null when the param is absent. */
  type: string | null;
  /** Read flag the UI sent, or null when the param is absent. */
  read: string | null;
  /** Archived flag the UI sent, or null when the param is absent. */
  archived: string | null;
  /** Snoozed flag the UI sent, or null when the param is absent. */
  snoozed: string | null;
  /** Mentions flag the UI sent, or null when the param is absent. */
  mentioned: string | null;
  /** Page cursor the UI sent, or null when the param is absent. */
  cursor: string | null;
}

/** Which empty state the archived-modules tab shows (NEWFRONT-225, ARCH-022). */
export type ArchivesModulesEmptyKind = "zero" | "filters" | "search" | "rows";

/** One row-menu entry: title, disabled state, explanation line (NEWFRONT-225). */
export interface ArchivesMenuEntry {
  /** Visible entry title. */
  title: string;
  /** Whether the entry renders disabled. */
  disabled: boolean;
  /** Explanation line under the title, or null when none shows. */
  description: string | null;
}

/** One applied-filter chip: filter key plus rendered chip text (NEWFRONT-225). */
export interface ArchivesModuleChip {
  /** Filter key as rendered (e.g. lead, members, start date). */
  key: string;
  /** Full rendered chip text. */
  text: string;
}

/** Read-only facts about an open archived peek panel (NEWFRONT-225, ARCH-025). */
export interface ArchivesPeekReadOnly {
  /** Name the panel shows, or null when no panel is open. */
  name: string | null;
  /** Whether the status control refuses to open its options. */
  statusLocked: boolean;
  /** Whether an add-link action is offered. */
  addLinkOffered: boolean;
  /** Whether a progress summary section renders. */
  summaryShown: boolean;
}

/** The archive confirmation dialog's copy (NEWFRONT-225, ARCH-023). */
export interface ArchivesArchiveDialog {
  /** Dialog heading (names the module). */
  title: string;
  /** Dialog body text. */
  body: string;
  /** Confirm button label (progress state while the write runs). */
  confirmLabel: string;
}

/** One archived-issues list fetch as the UI sent it (NEWFRONT-222, ARCH-005/006). */
export interface ArchivesListQuery {
  /** Full request URL. */
  url: string;
  /** Decoded query params (repeated keys joined with commas). */
  params: Record<string, string>;
}

/**
 * A stored rich filter expression over archived work items (NEWFRONT-222,
 * ARCH-005): the same condition shape the shared chip row renders.
 */
export interface ArchivesFilterExpression {
  /** Conditions combined with AND; each holds one property operator to values. */
  and?: Array<Record<string, string | Array<string>>>;
  /** Conditions combined with OR; each holds one property operator to values. */
  or?: Array<Record<string, string | Array<string>>>;
}

/** Archives tab keys (NEWFRONT-226, ARCH-029). */
export type ArchivesTab = "issues" | "cycles" | "modules";

/** Intake detail properties-panel row keys (NEWFRONT-258, INT-019). */
export type IntakeDetailProperty = "State" | "Assignees" | "Priority" | "Due date" | "Labels";
/** One selected progress-breakdown row (NEWFRONT-253, CYC-038/039). */
export interface CyclesProgressStatRow {
  /** Row title (state group, assignee, or label). */
  title: string;
  /** Whole percent the row shows. */
  percent: number;
  /** Denominator the row shows ("of N"). */
  total: number;
}

/** Traffic counters since the archives spy began (NEWFRONT-226, ARCH-032). */
export interface ArchivesTrafficCounts {
  /** GETs to the archived-issues list. */
  issuesReads: number;
  /** GETs to the archived-cycles list. */
  cyclesReads: number;
  /** GETs to the archived-modules list. */
  modulesReads: number;
  /** GETs to an archived single-item/cycle/module read. */
  detailReads: number;
  /** POST/DELETE archive and restore writes. */
  writes: number;
}

/** Locally persisted pages display prefs (NEWFRONT-320, PAGE-072). */
export interface PagesPrefsLocalConfig {
  /** Full-width canvas on/off. */
  fullWidth: boolean;
  /** Editor type size key. */
  fontSize: string;
  /** Editor typeface key. */
  fontStyle: string;
  /** Sticky toolbar on/off. */
  stickyToolbar: boolean;
}

/** One toolbar button with its advertised tooltip (NEWFRONT-320, PAGE-073). */
export interface PagesKeysToolbarTooltip {
  /** Button label as the tooltip names it. */
  label: string;
  /** Advertised key combination text, or empty when the tooltip names none. */
  keys: string;
}

/** Intake tab keys (NEWFRONT-260, INT-031). */
export type IntakeXTab = "open" | "closed";

/** One intake API request as the UI sent it (NEWFRONT-260, INT-018/031). */
export interface IntakeXApiRequest {
  /** HTTP method. */
  method: string;
  /** Full request URL. */
  url: string;
}

/** One module entry in an open palette module menu (NEWFRONT-311, MOD-037). */
export interface ModulesXPaletteMenuItem {
  /** Module display name. */
  name: string;
  /** Whether the entry shows its status icon. */
  hasIcon: boolean;
  /** Whether the entry shows the selected check mark. */
  hasCheck: boolean;
}

/** The open delete-module confirmation (NEWFRONT-309, MOD-025). */
export interface ModulesDeleteDialog {
  /** Dialog heading ("Delete module"). */
  heading: string;
  /** Whole confirmation copy (names the module, warns of permanence). */
  body: string;
}

/** Identity facts of one modules-list row (NEWFRONT-309, MOD-027). */
export interface ModulesRowFacts {
  /** Href of the module-name link, or null when the row carries none. */
  nameHref: string | null;
  /** Whether the row offers the peek/info trigger. */
  hasPeek: boolean;
}

/** One per-state row of the hero progress card (NEWFRONT-254, CYC-041). */
export interface CyclesHeroProgressGroup {
  /** State-group label as rendered (e.g. completed, started). */
  name: string;
  /** Full row text (label plus its work-item count). */
  text: string;
}

/** The workspace active-cycles upsell (NEWFRONT-254, CYC-043). */
export interface CyclesHeroUpsell {
  /** Banner heading, or null when the banner is absent. */
  heading: string | null;
  /** Rendered benefit cards. */
  benefits: number;
  /** Upgrade link target, or null when no upgrade link renders. */
  upgradeHref: string | null;
  /** Upgrade link target attribute (blank for a new tab), or null. */
  upgradeTarget: string | null;
}

/** Traffic counters since the hero spy began (NEWFRONT-254, CYC-045). */
export interface CyclesHeroTrafficCounts {
  /** GETs to live cycle reads (list, detail, progress, analytics). */
  cycleReads: number;
  /** GETs to the archived-cycles list. */
  archivedReads: number;
  /** Cycle mutating writes (create, update, delete, transfer). */
  writes: number;
}

/** One measurement-family radio option (NEWFRONT-323, EST-009; NEWFRONT-326, EST-029). */
export interface EstimatesFamilyOption {
  /** Option value as submitted (points, categories, time on cloud). */
  value: string;
  /** Visible option label. */
  label: string;
  /** Whether the option renders disabled. */
  disabled: boolean;
  /** Whether the option is currently selected. */
  checked: boolean;
}

/** List tab keys shared through the `type` query value (NEWFRONT-314, PAGE-021). */
export type PagesGatingTab = "public" | "private" | "archived";

/** Variant the list content area shows (NEWFRONT-314, PAGE-019). */
export type PagesGatingListVariant =
  | "rows"
  | "docs-empty"
  | "archived-empty"
  | "no-results"
  | "gate"
  | "loading"
  | "unknown";

/** Empty-collection create action state (NEWFRONT-314, PAGE-018/019). */
export type PagesGatingCreateState = "absent" | "enabled" | "disabled";

/** Detail error panel facts (NEWFRONT-314, PAGE-023). */
export interface PagesGatingErrorPanel {
  /** Panel heading, if any. */
  heading: string | null;
  /** Panel body, if any. */
  body: string | null;
  /** Back-to-list link href, if any. */
  backHref: string | null;
}

/** One logged page-API request (NEWFRONT-314, PAGE-020/022). */
export interface PagesGatingRequest {
  /** HTTP method. */
  method: string;
  /** Request path without query. */
  path: string;
  /** Raw query string without the leading `?`. */
  query: string;
}

/** Detail header action cluster presence (NEWFRONT-314, PAGE-018/024). */
export interface PagesGatingHeaderCluster {
  /** Favorite star offered. */
  star: boolean;
  /** Copy-link control offered. */
  copyLink: boolean;
  /** Options menu offered. */
  menu: boolean;
  /** Sync badge label, or null when the badge hides (synced). */
  syncBadge: string | null;
}

/** Title field facts (NEWFRONT-314, PAGE-079). */
export interface PagesGatingTitleFacts {
  /** Rendered title text. */
  text: string;
  /** Placeholder the empty field shows, if any. */
  placeholder: string | null;
  /** Rendered field height in CSS pixels. */
  height: number;
}

/** Header breadcrumb trail of the live modules list (NEWFRONT-306, MOD-001). */
export interface ModulesBreadcrumb {
  /** Project crumb button text (carries the project name). */
  project: string;
  /** Trailing crumb link texts in display order. */
  trail: string[];
}

/** One applied-filter chip on the live modules list (NEWFRONT-306, MOD-007). */
export interface ModulesChip {
  /** Filter key as rendered (e.g. status, lead, target date, Modules). */
  key: string;
  /** Full rendered chip text. */
  text: string;
}

/** Flag-gate empty state facts (NEWFRONT-306, MOD-002). */
export interface ModulesGateFacts {
  /** Explanation text the gate shows beside its art. */
  explanation: string;
  /** Manage-features button label, or null when no button renders. */
  buttonLabel: string | null;
  /** Whether the button renders disabled. */
  buttonDisabled: boolean;
  /** Whether themed art renders. */
  hasArt: boolean;
}

/** Pages list tab keys (NEWFRONT-312, PAGE-002). */
export type PagesListTab = "public" | "private" | "archived";

/** One list row's anatomy snapshot (NEWFRONT-312, PAGE-006). */
export interface PagesListRowDetail {
  /** Displayed title (untitled fallback already applied). */
  title: string;
  /** Whether the leading mark renders. */
  markOffered: boolean;
  /** Owner tooltip content after hovering the avatar, or null when absent. */
  owner: string | null;
  /** Visibility tooltip content after hovering the glyph, or null when absent. */
  visibility: string | null;
  /** Date tooltip content after hovering the info mark, or null when absent. */
  createdOn: string | null;
  /** Whether the favorite star renders. */
  starOffered: boolean;
  /** Whether the star shows selected. */
  starred: boolean;
  /** Whether the overflow menu trigger renders. */
  menuOffered: boolean;
}

/** List-area states the live modules list settles into (NEWFRONT-307, MOD-008..010). */
export type ModulesListEmptyKind = "rows" | "zero" | "nomatch";

/** Modules list layouts (NEWFRONT-307, MOD-011). */
export type ModulesListLayout = "list" | "board" | "timeline";

/** Slow-fetch skeleton observation (NEWFRONT-307, MOD-008). */
export interface ModulesListSkeletonResult {
  /** The skeleton showed while the list fetch was held. */
  shown: boolean;
  /** The shown skeleton matched the requested layout. */
  layoutMatched: boolean;
  /** No rows and no empty state showed alongside the skeleton. */
  exclusive: boolean;
}

/** Row progress marker (NEWFRONT-307, MOD-012). */
export type ModulesListRowProgress = { kind: "check" } | { kind: "alert" } | { kind: "percent"; percent: number };

/** Board card lead slot (NEWFRONT-307, MOD-020). */
export type ModulesListCardLead = "avatar" | "none";

/** Zero-state title and explanation (NEWFRONT-307, MOD-009). */
export interface ModulesListZeroStateCopy {
  /** Onboarding title. */
  title: string;
  /** Onboarding explanation. */
  description: string;
}

/** Non-zero progress segments of a board card (NEWFRONT-307, MOD-020). */
export interface ModulesListCardSegments {
  /** Segment widths in percent, in render order. */
  widths: number[];
  /** Segment background colors, in render order. */
  colors: string[];
}

/** Clickable card controls that must not navigate (NEWFRONT-307, MOD-020). */
export type ModulesListCardControl = "status" | "dates" | "favorite" | "menu";

/** One step-one starter card (NEWFRONT-323, EST-010). */
export interface EstimatesStarterCard {
  /** Card title (scratch card or template name). */
  title: string;
  /** Preview line under the title (values the card loads). */
  detail: string;
}

/** Active system row on the settings screen (NEWFRONT-323, EST-013). */
export interface EstimatesActiveRow {
  /** System name as rendered. */
  name: string;
  /** Joined value preview as rendered. */
  values: string;
}

/** One selected progress-breakdown row (NEWFRONT-310, MOD-034). */
export interface ModulesDetailStatRow {
  /** Row title (state group, assignee, or label). */
  title: string;
  /** Whole percent the row shows. */
  percent: number;
  /** Denominator the row shows ("of N"). */
  total: number;
}

/** One module link row (NEWFRONT-310, MOD-035). */
export interface ModulesDetailLinkRow {
  /** Rendered title (display title, or the URL when untitled). */
  title: string;
  /** Link target URL. */
  url: string;
  /** Anchor target attribute (new-tab proof). */
  target: string;
}

/** One Display-panel group-by entry (NEWFRONT-310, MOD-030). */
export interface ModulesDetailGroupedOption {
  /** Entry label as rendered. */
  label: string;
  /** Whether the entry refuses selection. */
  disabled: boolean;
}

/** Export dialog form state (NEWFRONT-318, PAGE-074). */
export interface PagesHeaderExportForm {
  /** Selected export format label ("PDF"/"Markdown"). */
  format: string;
  /** Selected content scope label ("Everything"/"No images"). */
  scope: string;
  /** Selected printable page format label ("A4"/...). */
  pageFormat: string;
  /** Whether the page-format row renders (PDF only). */
  pageFormatVisible: boolean;
}

/** A captured export download (NEWFRONT-318, PAGE-074). */
export interface PagesHeaderDownload {
  /** Suggested file name the browser download carried. */
  fileName: string;
  /** Download byte length. */
  byteLength: number;
  /** First bytes as latin1 text (magic + early content probe). */
  headText: string;
}
/** One runs-list row: badge label plus prompt text. */
export interface RunsListRow {
  statusLabel: string;
  prompt: string;
}

/** One pending-approval card: owning-run header, kind label, reason, expiry, payload. */
export interface ApprovalCard {
  header: string;
  kindLabel: string;
  reason: string | null;
  expiry: string | null;
  payload: string;
}

/** No-project empty view facts (NEWFRONT-375, ANL-003). */
export interface AnalyticsEmptyFacts {
  /** Empty view heading. */
  title: string;
  /** Empty view supporting text. */
  description: string;
  /** Project-creation action label, or null when no action renders. */
  actionLabel: string | null;
  /** Whether the creation action renders disabled. */
  actionDisabled: boolean;
}

/** One captured analytics API read (NEWFRONT-375, ANL-004). */
export interface AnalyticsApiRequest {
  /** Request path without query. */
  path: string;
  /** Raw query string without the leading `?`. */
  query: string;
}

/** One workspace-exports history row (NEWFRONT-381, EXP-007/008/011/012). */
export interface ExportsSettingsHistoryRow {
  /** Display name of the user who started the export. */
  exportedBy: string;
  /** Avatar image URL, or null when the row shows an initial instead. */
  avatarUrl: string | null;
  /** Initial character shown when there is no avatar. */
  initial: string | null;
  /** Human-formatted start date as rendered. */
  exportedOn: string;
  /** Project count the row reports. */
  projectCount: number;
  /** Format label as rendered (CSV/Excel/JSON). */
  format: string;
  /** Processing state text as rendered (queued/processing/completed/failed). */
  status: string;
  /** Download cell shape: link (fresh + finished), dash, or expired marker. */
  download: { kind: "link"; href: string; target: string } | { kind: "dash" } | { kind: "expired" };
}

/** Illustrated history empty view facts (NEWFRONT-381, EXP-013). */
export interface ExportsSettingsEmptyFacts {
  /** Empty view heading. */
  title: string;
  /** Empty view supporting text. */
  description: string;
}

/**
 * Which analytics data table a download/read targets (NEWFRONT-382,
 * EXP-014/015). The chart drill-down table renders first on the work-items
 * tab, the insight table second.
 */
export type AnalyticsExportTable = "chart" | "insight";

/** A locally generated file captured through the Playwright download event (NEWFRONT-382). */
export interface LocalFileDownload {
  /** File name the app suggested for the save. */
  fileName: string;
  /** File bytes decoded as UTF-8 text. */
  content: string;
}
