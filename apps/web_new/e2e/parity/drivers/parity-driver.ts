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
  /** Public anchor of the seeded project's published board (NEWFRONT-97 space oracle). */
  spaceAnchor?: string;
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
  /**
   * Native tooltip of every priority marker on work-item recent rows.
   * Target-only (SHELL-017, main#894): the old app tooltips the raw
   * enum, so the oracle driver leaves this unimplemented.
   */
  homeRecentPriorityTooltips(): Promise<string[]>;
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
  /** Whether the named widget shows its own Load more control (ISS-238). */
  widgetLoadMorePresent(widget: string): Promise<boolean>;
  /** Click the named widget's Load more control; ends after the page settles. */
  widgetLoadMore(widget: string): Promise<void>;
  /** Whether the named widget's Load more control is mid-fetch (ISS-238). */
  widgetLoadMoreFetching(widget: string): Promise<boolean>;
  /** Expand a nested parent row in the named widget; ends with its children fetched (ISS-238). */
  expandWidgetRowChildren(widget: string, rowName: string): Promise<void>;
  /** True when a nested parent row in the named widget shows its own Load more control (ISS-238). */
  widgetRowLoadMorePresent(widget: string, rowName: string): Promise<boolean>;
  /** Click a nested parent row's own Load more control; ends after the page settles (ISS-238). */
  widgetRowLoadMore(widget: string, rowName: string): Promise<void>;
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
  /** Drop one sidebar Favorites entry onto a folder to nest it inside. */
  dragFavoriteIntoFolder(sourceName: string, folderName: string): Promise<void>;
  /** Drag a nested Favorites entry out onto a top-level row to unnest it. */
  dragFavoriteOutOfFolder(sourceName: string, targetName: string): Promise<void>;
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
  // --- Cross-edition comment/activity sameness (NEWFRONT-819, CMT-025).
  // --- Appended; existing methods above are untouched per the shared
  // --- driver contract. The sameness scenario compares the individual
  // --- capabilities' own assertions with what the app actually sent, so it
  // --- needs the requests the detail screen made and the presence of each
  // --- capability's entry point without driving it.
  /**
   * Start recording the detail screen's API requests. By default the open
   * detail is reloaded first, so the load's own traffic is captured; pass
   * `{ reload: false }` to reset the counters in place before the next
   * capability runs.
   */
  activityBeginTrafficSpy(options?: { reload?: boolean }): Promise<void>;
  /** What the detail screen asked the server for since the spy started. */
  activityTrafficCounts(): Promise<CommentActivityTraffic>;
  /** Whether a comment card renders its per-card reaction control (picker trigger). */
  activityCommentReactionControlVisible(cardText: string): Promise<boolean>;
  /** Whether the issue-level reaction control renders above the feed. */
  issueReactionControlVisible(): Promise<boolean>;
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
  /**
   * The reason text a picker option carries beside its label (a server
   * reason, a default badge), or null when the option is absent or bare.
   */
  pickerOptionReason(text: string): Promise<string | null>;
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
  // --- Waiting-run banner (ISS-239/240). New-app-first: the old UI gains
  // --- it with PDASHOSS01-299, so these probes are web_new-only.
  /** Whether the waiting-for-runner banner renders on the detail page. */
  waitingBannerPresent(): Promise<boolean>;
  /** Banner sentence naming the runner and the reason, or null when absent. */
  waitingBannerText(): Promise<string | null>;
  /** Names of the banner's action controls, in render order. */
  waitingBannerActions(): Promise<string[]>;
  /** Whether the banner action named `name` is disabled (or absent). */
  waitingBannerActionDisabled(name: string): Promise<boolean>;
  /** Click the banner action named `name`; ends after the request settles. */
  waitingBannerAction(name: string): Promise<void>;
  /** In-place API validation message under the property row `label`, or null. */
  propertyInlineError(label: string): Promise<string | null>;
  /** Advisory note a property row carries (a busy-runner wait note), or null. */
  propertyNote(label: string): Promise<string | null>;
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
  /**
   * True when the repo-star affordance renders a theme-driven vector icon
   * (currentColor SVG) instead of a swapped image asset. Only the web_new
   * driver implements this; the oracle leg asserts the png asset instead.
   */
  repoStarIconThemed(): Promise<boolean>;

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
  /** Current builder state in the edit dialog; `visible: false` on a plain-textarea target. */
  schedulerRecurrenceFacts(): Promise<SchedulerRecurrenceFacts>;
  /** One builder interaction in the open edit dialog (target-only; throws on the oracle). */
  schedulerRecurrenceAct(action: SchedulerRecurrenceAction): Promise<void>;
  /** Toggle the advanced raw-rule disclosure; returns its expanded state. */
  schedulerRecurrenceAdvanced(): Promise<boolean>;
  /** Type into the raw rule textarea (opening it first when needed). */
  schedulerRecurrenceRaw(rule: string): Promise<void>;
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
  /** Computed text colour of an error row (AGT-047's tint probe). */
  assistantErrorTint(index: number): Promise<string>;
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
  /** Whether the composer mic exists at all (a hidden cell reads false). */
  assistantMicVisible(): Promise<boolean>;
  /** Ask the page to notice a refocus after an operator-side flag change. */
  assistantRefocus(): Promise<void>;
  /** Number of speech-config reads the page has made so far. */
  assistantSpeechConfigReads(): Promise<number>;
  /** Statuses of the speech read/save/test/transcribe/remove endpoints, driven in the browser. */
  assistantDictationApiProbe(): Promise<{
    read: number;
    readEnabled: boolean | null;
    save: number;
    test: number;
    transcribe: number;
    remove: number;
  }>;
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
  /**
   * Click the row marker until it reads `want`, re-clicking at most once and only while the row
   * and `isServerFavorite` agree the toggle never happened; throws instead of double-driving.
   */
  cyclesRowFavoriteToggleConverge(
    name: string,
    want: "starred" | "unstarred",
    isServerFavorite: () => Promise<boolean>,
    rowPollMs: number
  ): Promise<void>;
  /**
   * Drive the row marker to `want`, then hold the row/server agreement until it sticks,
   * healing stale-read overwrites with reconciling toggles; throws instead of toggling blind.
   */
  cyclesRowFavoriteSettleAgreement(
    name: string,
    want: "starred" | "unstarred",
    isServerFavorite: () => Promise<boolean>,
    rowPollMs: number
  ): Promise<void>;
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

  // --- Intake list shell (NEWFRONT-255, INT-001-005/008/012). Appended;
  // --- existing methods above are untouched per the shared driver contract.
  // --- Selectors follow the intake screen observed on the running old app:
  // --- the Open/Closed tab bar, the pending-count badge on the active Open
  // --- tab, id-anchored list rows, and the per-situation empty states.

  /** Open the intake screen of a project; settles once the tab bar or the feature gate shows. */
  intakeShellOpen(workspaceSlug: string, projectId: string): Promise<void>;
  /** Open the retired project-inbox address; settles once it lands on the intake route. */
  intakeShellOpenRetiredInbox(workspaceSlug: string, projectId: string): Promise<void>;
  /** Tab labels in display order (Open, Closed). */
  intakeShellTabLabels(): Promise<string[]>;
  /** Which tab is currently highlighted. */
  intakeShellActiveTab(): Promise<IntakeShellTab>;
  /** Pending total on the Open tab, or null when the badge is absent. */
  intakeShellOpenCount(): Promise<string | null>;
  /** Click `tab` and settle once the URL and the list reflect it. */
  intakeShellClickTab(tab: IntakeShellTab): Promise<void>;
  /** Titles of the rendered list rows, in display order. */
  intakeShellListTitles(): Promise<string[]>;
  /** Whether the pick-an-item placeholder shows in the detail area. */
  intakeShellPlaceholderVisible(): Promise<boolean>;
  /** Breadcrumb trail texts (project, Intake area) as the header renders them. */
  intakeShellBreadcrumbTrail(): Promise<string[]>;
  /** Href of the Intake breadcrumb link, or null when it is absent. */
  intakeShellBreadcrumbIntakeHref(): Promise<string | null>;
  /** Current document title. */
  intakeShellPageTitle(): Promise<string>;
  /** Whether the intake-disabled gate shows instead of the list. */
  intakeShellFeatureGateVisible(): Promise<boolean>;
  /** Whether the gate's settings shortcut is enabled. */
  intakeShellFeatureGateActionEnabled(): Promise<boolean>;
  /** Click the gate's settings shortcut; resolves with the landed path. */
  intakeShellFeatureGateActionGo(): Promise<string>;
  /** Heading of the list empty state, or null when rows render. */
  intakeShellListEmptyHeading(): Promise<string | null>;
  /** Click the empty-Open create action; resolves with the landed path (unreached on the old app while NEWFRONT-262 stands; the new-app build exercises it). */
  intakeShellEmptyCreateClick(): Promise<string>;
  /** Toggle a status-filter option in the filter panel, then close it and settle. */
  intakeShellFilterToggleStatus(option: "Pending" | "Snoozed"): Promise<void>;
  /** Start recording intake list reads (list endpoint GETs). */
  intakeShellBeginListSpy(): Promise<void>;
  /** Intake list reads captured since the spy began, in order. */
  intakeShellListQueries(): Promise<IntakeShellListQuery[]>;

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
  /**
   * Every API path the modules list entry fetches, unfiltered (the scenario
   * applies its own endpoint and edition/desktop rules to the whole set).
   */
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

  // --- Workspace-settings members + invitations oracle (NEWFRONT-411,
  // --- WSET-008..015). Observed on the running old app: the members page
  // --- shows a roster with a count chip, a name search box, a role filter
  // --- (Admin/Member/Guest/Suspended) combining with the search, and an
  // --- active-first row order; admins invite through a bulk email-plus-role
  // --- dialog; pending invitations render in an admin-only collapsible
  // --- group with role editing and a copy-link/remove menu; member rows
  // --- carry identity columns plus a profile link, an admin-only role
  // --- control (never offered on self; the server refuses self role
  // --- changes) and a hover remove/leave action behind a confirmation
  // --- dialog (self-removal leaves the workspace and lands on a
  // --- not-authorized gate for it); no skeleton renders while the roster loads
  // --- (bug NEWFRONT-422) but an inline no-match message shows when
  // --- filters exclude everyone; the activity and billing entries beside
  // --- the Add control render nothing in this OSS checkout. Appended;
  // --- existing methods above are untouched per the shared driver
  // --- contract.
  /** Open the workspace members settings page; resolves once the roster settles. */
  wsSettingsMembersOpen(workspaceSlug: string): Promise<void>;
  /** Roster-size count chip next to the Members heading, or null when absent. */
  wsSettingsMembersCountChip(): Promise<number | null>;
  /** Fill the roster search box. */
  wsSettingsMembersSearch(query: string): Promise<void>;
  /** Current value of the roster search box. */
  wsSettingsMembersSearchValue(): Promise<string>;
  /** Member emails in row order (stable identity for order assertions). */
  wsSettingsMembersVisibleEmails(): Promise<string[]>;
  /** Role label per member row, in row order. */
  wsSettingsMembersVisibleRoles(): Promise<{ email: string; role: string }[]>;
  /** Emails whose rows currently show the Suspended marker. */
  wsSettingsMembersSuspendedEmails(): Promise<string[]>;
  /** Open the role-filters dropdown. */
  wsSettingsMembersOpenFilters(): Promise<void>;
  /** Role-filter option labels in display order. */
  wsSettingsMembersFilterOptions(): Promise<string[]>;
  /** Toggle one role filter by label; resolves once the roster settles. */
  wsSettingsMembersToggleFilter(label: string): Promise<void>;
  /** Close the role-filters dropdown. */
  wsSettingsMembersCloseFilters(): Promise<void>;
  /** Whether the filters control shows its applied-filters marker. */
  wsSettingsMembersFilterBadge(): Promise<boolean>;
  /** Whether the inline no-match message is showing. */
  wsSettingsMembersNoMatchVisible(): Promise<boolean>;
  /** The inline no-match message text, or null when absent. */
  wsSettingsMembersNoMatchText(): Promise<string | null>;
  /** Whether the roster skeleton loader is currently visible. */
  wsSettingsMembersLoaderVisible(): Promise<boolean>;
  /** Delay the next roster reads (members + invitations GETs) by ms. */
  wsSettingsMembersDelayNextList(ms: number): Promise<void>;
  /** Whether the Add-member control is showing (admin-only). */
  wsSettingsMembersAddVisible(): Promise<boolean>;
  /** Open the bulk-invite dialog; resolves once its first row renders. */
  wsSettingsInviteOpen(): Promise<void>;
  /** Close the bulk-invite dialog via Cancel. */
  wsSettingsInviteClose(): Promise<void>;
  /** Whether the bulk-invite dialog is currently open. */
  wsSettingsInviteVisible(): Promise<boolean>;
  /** Number of email-plus-role rows in the open dialog. */
  wsSettingsInviteRowCount(): Promise<number>;
  /** Fill one dialog row's email field. */
  wsSettingsInviteFillRow(index: number, email: string): Promise<void>;
  /** Current value of one dialog row's email field. */
  wsSettingsInviteRowValue(index: number): Promise<string>;
  /** Current role label of one dialog row. */
  wsSettingsInviteRowRole(index: number): Promise<string>;
  /** Set one dialog row's role by label. */
  wsSettingsInviteSetRowRole(index: number, roleLabel: string): Promise<void>;
  /** Role options one dialog row offers, in display order. */
  wsSettingsInviteRowRoles(index: number): Promise<string[]>;
  /** Append a row via the dialog's Add-more control. */
  wsSettingsInviteAddRow(): Promise<void>;
  /** Remove one dialog row by index. */
  wsSettingsInviteRemoveRow(index: number): Promise<void>;
  /** Inline validation error under one dialog row's email, or null. */
  wsSettingsInviteRowError(index: number): Promise<string | null>;
  /** Submit the dialog; resolves once it closes, toasts, or flags a row inline. */
  wsSettingsInviteSubmit(): Promise<void>;
  /** Whether the pending-invitations group is showing (admin-only). */
  wsSettingsInvitesGroupVisible(): Promise<boolean>;
  /** Pending-invitations count chip, or null when the group is hidden. */
  wsSettingsInvitesCount(): Promise<number | null>;
  /** Pending-invitation emails in display order. */
  wsSettingsInvitesVisibleEmails(): Promise<string[]>;
  /** Toggle the pending-invitations collapsible group; resolves once the rows finish hiding/showing. */
  wsSettingsInvitesToggleGroup(): Promise<void>;
  /** Whether the pending group currently shows its invitation rows. */
  wsSettingsInvitesExpanded(): Promise<boolean>;
  /** Current role label of one pending invitation. */
  wsSettingsInviteRole(email: string): Promise<string>;
  /** Role options one pending invitation offers, in display order. */
  wsSettingsInviteRoleOptions(email: string): Promise<string[]>;
  /** Set one pending invitation's role by label. */
  wsSettingsInviteSetRole(email: string, roleLabel: string): Promise<void>;
  /** Ellipsis-menu entries of one pending invitation, in display order. */
  wsSettingsInviteMenuEntries(email: string): Promise<string[]>;
  /** Copy one pending invitation's link; resolves with the clipboard text. */
  wsSettingsInviteCopyLink(email: string): Promise<string>;
  /** Open the remove confirmation for one pending invitation. */
  wsSettingsInviteOpenRemove(email: string): Promise<void>;
  /** Whether the member/invitation remove dialog is showing. */
  wsSettingsRemoveDialogVisible(): Promise<boolean>;
  /** Remove-dialog heading text, or null when absent. */
  wsSettingsRemoveDialogTitle(): Promise<string | null>;
  /** Remove-dialog body text, or null when absent. */
  wsSettingsRemoveDialogText(): Promise<string | null>;
  /** Confirm the open remove dialog; resolves once the write settles (success closes it; specs assert the outcome). */
  wsSettingsRemoveDialogConfirm(): Promise<void>;
  /** Cancel the open remove dialog; resolves once it closes. */
  wsSettingsRemoveDialogCancel(): Promise<void>;
  /** Identity facts of one member row: names, role, auth medium, joining date, avatar shape, profile link. */
  wsSettingsMemberRowFacts(email: string): Promise<WsSettingsMemberRowFacts>;
  /** Whether one member row offers an editable role control (never on self). */
  wsSettingsMemberRoleEditable(email: string): Promise<boolean>;
  /** Role options one member row offers, in display order. */
  wsSettingsMemberRoleOptions(email: string): Promise<string[]>;
  /** Set one member's role by label; resolves once the row settles. */
  wsSettingsMemberSetRole(email: string, roleLabel: string): Promise<void>;
  /** Hover-menu entry of one member row ("Remove" or "Leave"), or null when none. */
  wsSettingsMemberMenuEntry(email: string): Promise<string | null>;
  /** Open the remove/leave confirmation for one member row. */
  wsSettingsMemberOpenRemove(email: string): Promise<void>;
  /** Whether any member-activity entry shows on the members page. */
  wsSettingsMembersActivityVisible(): Promise<boolean>;
  /** Whether any billing/upsell entry shows in the members header actions. */
  wsSettingsMembersBillingVisible(): Promise<boolean>;

  // --- Desktop updater web-observable sides (NEWFRONT-209, DESK-016–018).
  // --- Appended; existing methods above are untouched per the shared driver
  // --- contract. The updater lives in the native shell (updates.rs) and the
  // --- desktop bundle (the sidebar install button); on web all three rows
  // --- assert absence: no launch prompt, no feed traffic, no sidebar button.
  /** True when a dialog carrying the native launch-prompt markers is visible. */
  deskUpdateNativePromptVisible(): Promise<boolean>;
  /** Start recording every request URL the page issues. */
  deskUpdateBeginTrafficWatch(): Promise<void>;
  /** Request URLs recorded since the traffic watch began, in order. */
  deskUpdateTrafficUrls(): Promise<string[]>;
  /** Stop the traffic watch and detach its listener. */
  deskUpdateEndTrafficWatch(): Promise<void>;
  /** True when the desktop update-install button is present. */
  deskUpdateSidebarButtonVisible(): Promise<boolean>;

  // === NEWFRONT-97 space navigation (shared; every space-oracle child
  // carries this block verbatim — keep the earliest-merged copy on dup). ===
  /** Open a published board at its anchor address; query is old-app raw (e.g. "?board=kanban"). */
  spaceOpenBoard(anchor: string, query?: string): Promise<void>;
  /** Open the space landing page (the auth gate). */
  spaceOpenLanding(): Promise<void>;
  /** Open a legacy workspace/project link (resolves to the anchor board or not-found). */
  spaceOpenLegacyLink(workspaceSlug: string, projectId: string): Promise<void>;
  // --- Space layouts & cards oracle (NEWFRONT-449, SPC-020..033).
  // --- Appended; existing methods above are untouched per the shared
  // --- driver contract.
  /** Layout keys the board switcher currently offers, in display order. */
  spaceBoardLayoutOptions(): Promise<string[]>;
  /** The rendered layout key, read from the board address. */
  spaceBoardActiveLayout(): Promise<string>;
  /** Switch to a layout via the switcher; resolves once the address carries it and the layout renders. */
  spaceBoardSwitchLayout(layout: string): Promise<void>;
  /** Switch layouts; resolves with the count of public-issues fetches observed during the flip. */
  spaceBoardSwitchLayoutRefetches(layout: string): Promise<number>;
  /** List-layout groups: state name, header count, rendered rows. */
  spaceBoardListGroups(): Promise<SpaceBoardGroupFacts[]>;
  /** Board-layout columns: state name, header count, rendered cards. */
  spaceBoardKanbanColumns(): Promise<SpaceBoardGroupFacts[]>;
  /** How board columns flow: side-by-side, stacked, or a lone column. */
  spaceBoardKanbanColumnsFlow(): Promise<"row" | "column" | "single">;
  /** Whether the zero-issues empty notice is showing. */
  spaceBoardEmptyVisible(): Promise<boolean>;
  /** Whether a list group currently shows its manual load-more control. */
  spaceBoardListGroupLoadMoreVisible(groupName: string): Promise<boolean>;
  /** Click a list group's manual load-more; resolves once its rows grow. */
  spaceBoardListGroupLoadMore(groupName: string): Promise<void>;
  /** Whether placeholder rows show while a list group pages (delays the next fetch to catch them). */
  spaceBoardListGroupShowsLoader(groupName: string): Promise<boolean>;
  /** Scroll a board column to its end; resolves once its cards grow past the first page. */
  spaceBoardKanbanColumnScrollLoad(columnName: string): Promise<void>;
  /** Whether placeholder cards show while a board column pages (delays the next fetch to catch them). */
  spaceBoardKanbanColumnShowsLoader(columnName: string): Promise<boolean>;
  /** Rapid scroll bursts on one column; resolves with the issues-fetch count observed (debounce). */
  spaceBoardKanbanScrollBurstFetches(columnName: string, scrolls: number): Promise<number>;
  /** List card facts for an issue: public key, title, and the full title hover reveals. */
  spaceBoardCardFacts(issueName: string): Promise<SpaceBoardCardFacts>;
  /** List card property markers for an issue: state, priority, labels, dates, plus raw card text. */
  spaceBoardCardMarkers(issueName: string): Promise<SpaceBoardCardMarkers>;
  /** Board card footer for an issue: presence plus vote/reaction halves. */
  spaceBoardKanbanCardFooter(issueName: string): Promise<SpaceBoardCardFooter>;
  /** Whether a list card carries any vote/reaction footer. */
  spaceBoardListCardFooterVisible(issueName: string): Promise<boolean>;
  /** Click a card; resolves once the issue is in the address and the overlay opens. */
  spaceBoardOpenCard(issueName: string): Promise<void>;
  /** Whether the issue overlay is currently open. */
  spaceBoardPeekVisible(): Promise<boolean>;
  /** Close the issue overlay; resolves once it shuts and the address drops the issue. */
  spaceBoardClosePeek(): Promise<void>;
  /** Name of the highlighted list row, or null when none is highlighted. */
  spaceBoardHighlightedRow(): Promise<string | null>;
  /** Read-only facts: draggable cards, drag handles, and inline edit controls on the board. */
  spaceBoardCardDragFacts(): Promise<SpaceBoardDragFacts>;
  /** Attempt to drag one card; resolves with the list order before and after. */
  spaceBoardDragCardOrder(issueName: string): Promise<{ before: string[]; after: string[] }>;
  /** Issue names in list-layout display order. */
  spaceBoardVisibleOrder(): Promise<string[]>;
  /** Blur/focus cycle; resolves with order before/after plus issues-fetch count. */
  spaceBoardRefocusStability(): Promise<SpaceBoardStability>;
  /** Offline/online cycle; resolves with the list order before and after. */
  spaceBoardReconnectStability(): Promise<{ before: string[]; after: string[] }>;

  // --- Instance-admin entry/session oracle (NEWFRONT-441, ADM-001..012).
  // --- Appended; existing methods above are untouched per the shared
  // --- driver contract.

  /** Open the admin console entry as a signed-out visitor. */
  adminOpenRoot(): Promise<void>;
  /** Open an admin console path under the /god-mode mount (leading slash, query allowed). */
  adminOpenPath(path: string): Promise<void>;
  /** Which branch of the admin bootstrap gate is currently showing. */
  adminRootBranch(): Promise<AdminRootBranch>;
  /** Poll until the bootstrap gate shows the expected branch (throws on timeout). */
  adminWaitForRootBranch(branch: AdminRootBranch): Promise<void>;
  /** Current admin pathname (the /god-mode mount included). */
  adminCurrentPath(): Promise<string>;
  /** Current admin query string, with the leading `?`, or empty. */
  adminCurrentSearch(): Promise<string>;
  /** Browser history length (proves replace-navigation adds no entry). */
  adminHistoryLength(): Promise<number>;
  /** Start counting GET /api/instances/ info reads (resets the count). */
  adminStartInstanceFetchSpy(): Promise<void>;
  /** Info reads observed since the spy started. */
  adminInstanceFetchCount(): Promise<number>;
  /** Fail subsequent info reads (stays armed until cleared). */
  adminFailInstanceFetch(): Promise<void>;
  /** Delay subsequent info reads by ms (stays armed until cleared). */
  adminDelayInstanceFetch(ms: number): Promise<void>;
  /** Lift any fail/delay fault; the spy keeps counting. */
  adminClearInstanceFetchFault(): Promise<void>;
  /** Failure-screen facts: illustration, retry control and its label. */
  adminFailureFacts(): Promise<AdminFailureFacts>;
  /** Click the failure-screen retry; resolves once the reload settles. */
  adminFailureRetry(): Promise<void>;
  /** Whether the brand loader is currently visible (and no form alongside it). */
  adminLoaderFacts(): Promise<{ visible: boolean; formVisible: boolean }>;
  /** Setup-form facts: values, gating, telemetry, meter, mismatch, errors. */
  adminSetupFacts(): Promise<AdminSetupFacts>;
  /** Fill setup-form fields (absent keys are left untouched). */
  adminSetupFill(fields: AdminSetupFields): Promise<void>;
  /** Set the telemetry opt-in to the wanted state. */
  adminSetupSetTelemetry(on: boolean): Promise<void>;
  /** Focus a setup password field (drives the meter/hint visibility). */
  adminSetupFocus(field: "password" | "confirm"): Promise<void>;
  /** Move focus off the password fields (onto the form heading). */
  adminSetupBlur(): Promise<void>;
  /** Toggle a setup password field between hidden and shown. */
  adminSetupTogglePassword(field: "password" | "confirm"): Promise<void>;
  /** Submit the setup form; resolves once the POST round-trip settles. */
  adminSetupSubmit(): Promise<void>;
  /** Sign-in-form facts: values, gating, busy state, banners. */
  adminSignInFacts(): Promise<AdminSignInFacts>;
  /** Fill the sign-in email and password fields. */
  adminSignInFill(email: string, password: string): Promise<void>;
  /** Toggle the sign-in password field between hidden and shown. */
  adminSignInTogglePassword(): Promise<void>;
  /** Submit the sign-in form; resolves once the POST round-trip settles. */
  adminSignInSubmit(): Promise<void>;
  /** Dismiss the mapped sign-in info banner; resolves once it closes. */
  adminSignInDismissMappedBanner(): Promise<void>;
  /** Complete the admin sign-in flow; ends on the dashboard. */
  adminEnsureSignedIn(email: string, password: string): Promise<void>;
  /** Whether the dashboard shell (sidebar plus header) is showing. */
  adminDashboardVisible(): Promise<boolean>;
  /** Open the sidebar account menu. */
  adminOpenAccountMenu(): Promise<void>;
  /** Account-menu facts: admin email, theme entry, sign-out entry. */
  adminAccountMenuFacts(): Promise<AdminAccountMenuFacts>;
  /** Sign out through the account menu; resolves once the round-trip settles. */
  adminSidebarSignOut(): Promise<void>;
  /** Theme persistence facts: stored choice plus the resolved mode. */
  adminThemeFacts(): Promise<{ stored: string | null; resolved: "light" | "dark" }>;
  /** Set the theme through the account menu (for the sign-out reset row). */
  adminSetTheme(mode: "light" | "dark"): Promise<void>;
  /** 404-screen facts: illustration, message, and the back action. */
  adminNotFoundFacts(): Promise<AdminNotFoundFacts>;
  /** Follow the 404 back action; resolves once navigation settles. */
  adminNotFoundFollowAction(): Promise<void>;
  /** First-workspace popup facts: visibility plus the create target. */
  adminFirstWorkspacePopupFacts(): Promise<{ visible: boolean; createHref: string | null }>;
  /** Dismiss the first-workspace popup; resolves once it closes. */
  adminFirstWorkspacePopupDismiss(): Promise<void>;
  /** Follow the popup's create action; resolves once navigation settles. */
  adminFirstWorkspacePopupFollowCreate(): Promise<void>;
  /** Tabindex of a setup password toggle (proves it stays out of tab order). */
  adminSetupToggleTabIndex(field: "password" | "confirm"): Promise<string | null>;
  /** Whether the sign-in email field holds focus (autofocus row). */
  adminSignInEmailFocused(): Promise<boolean>;
  /** Delay the admin session reads (admins/me) by ms. */
  adminDelaySessionFetch(ms: number): Promise<void>;
  /** Lift the session-read delay. */
  adminClearSessionFetchDelay(): Promise<void>;
  /** Answer info reads with no workspaces (drives the first-workspace popup branch). */
  adminStubNoWorkspaces(): Promise<void>;

  // --- Instance-admin general/email oracle (NEWFRONT-442, ADM-013..022). ---
  /** General-settings facts: name/email/id fields, telemetry, save control. */
  adminGeneralFacts(): Promise<AdminGeneralFacts>;
  /** Fill the instance-name field (empty string clears it). */
  adminGeneralFillName(name: string): Promise<void>;
  /** Flip the telemetry switch to `on`. */
  adminGeneralSetTelemetry(on: boolean): Promise<void>;
  /** Click the general-settings save control (no settle wait; the spec polls). */
  adminGeneralSave(): Promise<void>;
  /** Start recording instance PATCH writes (resets the bodies log). */
  adminStartInstancePatchSpy(): Promise<void>;
  /** Instance PATCH request bodies seen since the spy started. */
  adminInstancePatchBodies(): Promise<unknown[]>;
  /** Fail subsequent instance PATCH writes with a server error. */
  adminFailInstancePatch(): Promise<void>;
  /** Delay subsequent instance PATCH writes by ms. */
  adminDelayInstancePatch(ms: number): Promise<void>;
  /** Lift the instance-PATCH fault. */
  adminClearInstancePatchFault(): Promise<void>;
  /** Newest visible admin toast, or null when none shows. */
  adminLastToast(): Promise<{ title: string; message: string } | null>;
  /** Start capturing the page's console errors (resets the log). */
  adminStartConsoleSpy(): Promise<void>;
  /** Console error texts captured since the spy started. */
  adminConsoleErrors(): Promise<string[]>;
  /** Email-page facts: master toggle, form/skeleton visibility. */
  adminEmailFacts(): Promise<AdminEmailFacts>;
  /** Flip the master email toggle to `on` (no settle wait; the spec polls). */
  adminEmailSetEnabled(on: boolean): Promise<void>;
  /** Email config-form facts (fails fast while the form is hidden). */
  adminEmailFormFacts(): Promise<AdminEmailFormFacts>;
  /** Fill email config fields (partial; empty string clears). */
  adminEmailFill(fields: AdminEmailFields): Promise<void>;
  /** Toggle the config password field between hidden and shown. */
  adminEmailTogglePassword(): Promise<void>;
  /** Pick the transport-security choice. */
  adminEmailSetSecurity(which: AdminEmailSecurity): Promise<void>;
  /** Click the config-form save control (no settle wait; the spec polls). */
  adminEmailSave(): Promise<void>;
  /** Start recording configuration PATCH writes (resets the bodies log). */
  adminStartConfigPatchSpy(): Promise<void>;
  /** Configuration PATCH request bodies seen since the spy started. */
  adminConfigPatchBodies(): Promise<unknown[]>;
  /** Fail subsequent configuration PATCH writes with a server error. */
  adminFailConfigPatch(): Promise<void>;
  /** Delay subsequent configuration PATCH writes by ms. */
  adminDelayConfigPatch(ms: number): Promise<void>;
  /** Lift the configuration-PATCH fault. */
  adminClearConfigPatchFault(): Promise<void>;
  /** Delay configuration GET reads by ms. */
  adminDelayConfigFetch(ms: number): Promise<void>;
  /** Fail configuration GET reads (network error). */
  adminFailConfigFetch(): Promise<void>;
  /** Lift the configuration-GET fault. */
  adminClearConfigFetchFault(): Promise<void>;
  /** Start counting email-disable DELETE calls (resets the count). */
  adminStartEmailDisableSpy(): Promise<void>;
  /** Email-disable DELETE calls seen since the spy started. */
  adminEmailDisableCount(): Promise<number>;
  /** Fail subsequent email-disable DELETE calls with a server error. */
  adminFailEmailDisable(): Promise<void>;
  /** Lift the email-disable failure. */
  adminClearEmailDisableFault(): Promise<void>;
  /** Open the send-test-email modal (waits for it to show). */
  adminTestEmailOpen(): Promise<void>;
  /** Send-test-email modal facts (resolves open:false while the modal is shut). */
  adminTestEmailFacts(): Promise<AdminTestEmailFacts>;
  /** Fill the modal's receiver field. */
  adminTestEmailFillReceiver(email: string): Promise<void>;
  /** Click the modal's send control (no settle wait; the spec polls). */
  adminTestEmailSend(): Promise<void>;
  /** Click the modal's dismiss control and wait for it to shut. */
  adminTestEmailClose(): Promise<void>;
  /** Start recording test-email POST bodies (resets the log). */
  adminStartEmailCheckSpy(): Promise<void>;
  /** Test-email POST request bodies seen since the spy started. */
  adminEmailCheckBodies(): Promise<unknown[]>;

  // --- Admin authentication landing + provider sub-pages (NEWFRONT-443, ADM-023..036). ---
  /** Landing snapshot: sign-up toggle, skeleton, and method cards in DOM order. */
  adminAuthLandingFacts(): Promise<AdminAuthLandingFacts>;
  /** Set the open-sign-up toggle to `on`, clicking only on mismatch. */
  adminAuthSetSignup(on: boolean): Promise<void>;
  /** Set the named method card's enable toggle to `on`, clicking only on mismatch. */
  adminAuthSetMethodEnabled(name: string, on: boolean): Promise<void>;
  /** Method card logo image source, or null when the card shows no image. */
  adminAuthMethodIconSrc(name: string): Promise<string | null>;
  /** Provider sub-page shell snapshot: header card, master toggle, body state, tab title. */
  adminAuthProviderFacts(): Promise<AdminAuthProviderFacts>;
  /** Set the provider sub-page master toggle to `on`, clicking only on mismatch. */
  adminAuthProviderSetEnabled(on: boolean): Promise<void>;
  /** Provider credential-form snapshot: values, placeholders, sync switch, save gating. */
  adminAuthFormFacts(): Promise<AdminAuthFormFacts>;
  /** Fill provider-form fields by input id. */
  adminAuthFormFill(values: Record<string, string>): Promise<void>;
  /** Set the provider-form sync switch to `on`, clicking only on mismatch. */
  adminAuthFormSetSync(on: boolean): Promise<void>;
  /** Press the provider-form save control. */
  adminAuthFormSave(): Promise<void>;
  /** Press the provider-form "Go back" link. */
  adminAuthFormBack(): Promise<void>;
  /** Unsaved-changes discard modal snapshot. */
  adminAuthDiscardFacts(): Promise<AdminAuthDiscardFacts>;
  /** Dismiss the discard modal via Keep editing. */
  adminAuthDiscardKeep(): Promise<void>;
  /** Confirm the discard modal via its Go back link. */
  adminAuthDiscardLeave(): Promise<void>;
  /** Copy-to-clipboard fields on the page in DOM order. */
  adminAuthCopyFacts(): Promise<AdminAuthCopyFact[]>;
  /** Press the copy field labeled `label`. */
  adminAuthCopyField(label: string): Promise<void>;
  /** Help-link target inside the copy field labeled `label`, or null when absent. */
  adminAuthCopyHelpHref(label: string): Promise<string | null>;
  /** Current clipboard text (grants clipboard permissions first). */
  adminAuthReadClipboard(): Promise<string>;

  // --- Admin workspaces + AI + image oracle (NEWFRONT-444, ADM-037..050). ---
  /** Workspace-list page snapshot: toggle, count, skeletons, load-more, helper. */
  adminWorkspaceListFacts(): Promise<AdminWorkspaceListFacts>;
  /** Workspace rows in DOM order (external links into the main web app). */
  adminWorkspaceRows(): Promise<AdminWorkspaceRow[]>;
  /** Set the restrict-workspace-creation toggle to `on`, clicking only on mismatch. */
  adminWorkspaceSetCreationRestricted(on: boolean): Promise<void>;
  /** Press the load-more control (no-op guard lives in the caller). */
  adminWorkspaceLoadMore(): Promise<void>;
  /** Start spying on instance-workspace list reads. */
  adminStartWorkspacesFetchSpy(): Promise<void>;
  /** Workspace-list request URLs seen since the spy started. */
  adminWorkspacesFetchUrls(): Promise<string[]>;
  /** Delay instance-workspace list reads by ms. */
  adminDelayWorkspacesFetch(ms: number): Promise<void>;
  /** Fail instance-workspace list reads (HTTP 500). */
  adminFailWorkspacesFetch(): Promise<void>;
  /** Answer instance-workspace list reads with an empty page. */
  adminStubEmptyWorkspaces(): Promise<void>;
  /** Clear the workspaces-list fetch fault. */
  adminClearWorkspacesFetchFault(): Promise<void>;
  /** Workspace-create form snapshot: values, prefix, org size, gating, errors. */
  adminWorkspaceCreateFacts(): Promise<AdminWorkspaceCreateFacts>;
  /** Type into the create-form name field (drives slug derivation). */
  adminWorkspaceCreateFillName(name: string): Promise<void>;
  /** Type into the create-form slug field (independent edit). */
  adminWorkspaceCreateFillSlug(slug: string): Promise<void>;
  /** Pick an org-size option in the create form. */
  adminWorkspaceCreateSelectOrgSize(size: string): Promise<void>;
  /** Press the create-form submit control. */
  adminWorkspaceCreateSubmit(): Promise<void>;
  /** Start spying on slug-check reads. */
  adminStartSlugCheckSpy(): Promise<void>;
  /** Slug-check request slugs seen since the spy started. */
  adminSlugCheckSlugs(): Promise<string[]>;
  /** Fail slug-check reads (HTTP 500). */
  adminFailSlugCheck(): Promise<void>;
  /** Clear the slug-check fault. */
  adminClearSlugCheckFault(): Promise<void>;
  /** Start spying on workspace-create POSTs. */
  adminStartWorkspaceCreateSpy(): Promise<void>;
  /** Workspace-create POST bodies seen since the spy started. */
  adminWorkspaceCreateBodies(): Promise<unknown[]>;
  /** Fail workspace-create POSTs (HTTP 500). */
  adminFailWorkspaceCreate(): Promise<void>;
  /** Delay workspace-create POSTs by ms. */
  adminDelayWorkspaceCreate(ms: number): Promise<void>;
  /** Clear the workspace-create fault. */
  adminClearWorkspaceCreateFault(): Promise<void>;
  /** AI settings page snapshot: model, key, save, callout, skeleton. */
  adminAiFacts(): Promise<AdminAiFacts>;
  /** Fill the AI form model and key fields. */
  adminAiFill(model: string, key: string): Promise<void>;
  /** Toggle the AI key visibility control. */
  adminAiToggleKeyVisibility(): Promise<void>;
  /** Press the AI save control. */
  adminAiSave(): Promise<void>;
  /** Image-page presence snapshot (bug-pinned until main#523 lands). */
  adminImageFacts(): Promise<AdminImageFacts>;

  // --- Admin loop jobs + runners (NEWFRONT-445, ADM-051..061 + ADM-080). ---
  /** Loop-list page snapshot (header, skeleton, empty state, table chrome). */
  adminLoopListFacts(): Promise<AdminLoopListFacts>;
  /** Loop jobs as rendered in the list table, top to bottom. */
  adminLoopRows(): Promise<AdminLoopRow[]>;
  /** Set the enabled switch of the row with `slug` to `on`. */
  adminLoopSetEnabled(slug: string, on: boolean): Promise<void>;
  /** Press the loop-list New-job action. */
  adminLoopOpenCreate(): Promise<void>;
  /** Loop job create/edit modal snapshot. */
  adminLoopModalFacts(): Promise<AdminLoopModalFacts>;
  /** Fill the given modal fields (minRole picks the matching option). */
  adminLoopModalFill(fields: Partial<AdminLoopModalFields>): Promise<void>;
  /** Press the modal Save control. */
  adminLoopModalSave(): Promise<void>;
  /** Press the modal Cancel control. */
  adminLoopModalCancel(): Promise<void>;
  /** Loop job detail page snapshot (stats, skip filter, targets chrome). */
  adminLoopDetailFacts(): Promise<AdminLoopDetailFacts>;
  /** Loop targets as rendered in the detail table, top to bottom. */
  adminLoopDetailRows(): Promise<AdminLoopTargetRow[]>;
  /** Pick a skip-reason filter option on the detail page ("" resets to all). */
  adminLoopSetSkipFilter(reason: string): Promise<void>;
  /** Press the detail Edit action. */
  adminLoopOpenEdit(): Promise<void>;
  /** Press the detail Delete action. */
  adminLoopDelete(): Promise<void>;
  /** Start spying on loop-jobs list reads. */
  adminStartLoopJobsSpy(): Promise<void>;
  /** Loop-jobs list read URLs seen since the spy started. */
  adminLoopJobsUrls(): Promise<string[]>;
  /** Delay loop-jobs list reads by `ms`. */
  adminDelayLoopJobsFetch(ms: number): Promise<void>;
  /** Fail loop-jobs list reads with HTTP 500. */
  adminFailLoopJobsFetch(): Promise<void>;
  /** Answer loop-jobs list reads with an empty list. */
  adminStubEmptyLoopJobs(): Promise<void>;
  /** Clear the loop-jobs list read fault. */
  adminClearLoopJobsFetchFault(): Promise<void>;
  /** Start spying on loop-job detail reads. */
  adminStartLoopJobSpy(): Promise<void>;
  /** Loop-job detail read URLs seen since the spy started. */
  adminLoopJobUrls(): Promise<string[]>;
  /** Delay loop-job detail reads by `ms`. */
  adminDelayLoopJobFetch(ms: number): Promise<void>;
  /** Fail loop-job detail reads with HTTP 500. */
  adminFailLoopJobFetch(): Promise<void>;
  /** Clear the loop-job detail read fault. */
  adminClearLoopJobFetchFault(): Promise<void>;
  /** Start spying on loop-targets reads. */
  adminStartLoopTargetsSpy(): Promise<void>;
  /** Loop-targets read URLs seen since the spy started. */
  adminLoopTargetsUrls(): Promise<string[]>;
  /** Start spying on loop-job creates. */
  adminStartLoopJobCreateSpy(): Promise<void>;
  /** Loop-job create request bodies seen since the spy started. */
  adminLoopJobCreateBodies(): Promise<unknown[]>;
  /** Fail loop-job creates with HTTP 500. */
  adminFailLoopJobCreate(): Promise<void>;
  /** Clear the loop-job create fault. */
  adminClearLoopJobCreateFault(): Promise<void>;
  /** Start spying on loop-job updates. */
  adminStartLoopJobUpdateSpy(): Promise<void>;
  /** Loop-job update request bodies seen since the spy started. */
  adminLoopJobUpdateBodies(): Promise<unknown[]>;
  /** Fail loop-job updates with HTTP 500. */
  adminFailLoopJobUpdate(): Promise<void>;
  /** Delay loop-job updates by `ms`. */
  adminDelayLoopJobUpdate(ms: number): Promise<void>;
  /** Clear the loop-job update fault. */
  adminClearLoopJobUpdateFault(): Promise<void>;
  /** Start spying on loop-job deletes. */
  adminStartLoopJobDeleteSpy(): Promise<void>;
  /** Loop-job delete count seen since the spy started. */
  adminLoopJobDeleteCount(): Promise<number>;
  /** Fail loop-job deletes with HTTP 500. */
  adminFailLoopJobDelete(): Promise<void>;
  /** Clear the loop-job delete fault. */
  adminClearLoopJobDeleteFault(): Promise<void>;
  /** Runners presence snapshot (bug-pinned until the runners page lands). */
  adminRunnersFacts(): Promise<AdminRunnersFacts>;
  /** Runners page body facts (page root, heading, table, failure state). */
  adminRunnersPageFacts(): Promise<AdminRunnersPageFacts>;

  // --- Admin dashboard shell + negatives (NEWFRONT-446, ADM-062..073). ---
  /** Sidebar entries with active state, plus collapse facts. */
  adminSidebarFacts(): Promise<AdminSidebarFacts>;
  /** Follow the sidebar entry named `name`; resolves once the dashboard settles. */
  adminSidebarFollowEntry(name: string): Promise<void>;
  /** Hover the sidebar entry at `href`; resolve with its hover-label text (null when none shows). */
  adminSidebarEntryTooltip(href: string, name: string): Promise<string | null>;
  /** Hover the help-footer `control`; resolve with its hover-label text (null when none shows). */
  adminFooterTooltip(control: AdminFooterControl, label: string): Promise<string | null>;
  /** Toggle the sidebar through the help-section collapse control. */
  adminSidebarToggleCollapse(): Promise<void>;
  /** Click outside the sidebar (mobile auto-collapse probe, ADM-063). */
  adminSidebarOutsideClick(): Promise<void>;
  /** Account dropdown facts (identity row plus opened-menu contents). */
  adminAccountFacts(): Promise<AdminAccountFacts>;
  /** Open the account dropdown (avatar when expanded, icon button when collapsed). */
  adminAccountOpen(): Promise<void>;
  /** Header breadcrumb trail facts. */
  adminBreadcrumbFacts(): Promise<AdminBreadcrumbFacts>;
  /** Hover the crumb labeled `label`; resolve with its tooltip text (long labels). */
  adminBreadcrumbTooltip(label: string): Promise<string | null>;
  /** Help-section facts (redirect link, popover links, version). */
  adminHelpFacts(): Promise<AdminHelpFacts>;
  /** Open the help popover. */
  adminHelpOpen(): Promise<void>;
  /** Mobile hamburger facts. */
  adminHamburgerFacts(): Promise<AdminHamburgerFacts>;
  /** Toggle the sidebar through the header hamburger. */
  adminHamburgerToggle(): Promise<void>;
  /** Start counting instance-configuration fetches (shared-cache probe). */
  adminStartConfigFetchSpy(): Promise<void>;
  /** Instance-configuration fetch count since the spy started. */
  adminConfigFetchCount(): Promise<number>;
  /** Start counting realtime transports (websockets, event streams). */
  adminStartTransportSpy(): Promise<void>;
  /** Realtime transport counts since the spy started. */
  adminTransportFacts(): Promise<AdminTransportFacts>;
  /** Dashboard absence scan: drag sources, file inputs, transfer and desktop controls. */
  adminAbsenceFacts(): Promise<AdminAbsenceFacts>;
  /** Press `keys` with body focus; report navigation/dialog side effects. */
  adminShortcutProbe(keys: string[]): Promise<AdminShortcutProbeFacts>;
  /** Delete the open loop job while watching for confirmation dialogs. */
  adminLoopDeleteWatchDialogs(): Promise<AdminDeleteWatchFacts>;
  /** Workspace-page delete-control scan. */
  adminWorkspaceDeleteFacts(): Promise<AdminWorkspaceDeleteFacts>;

  // --- Admin cloud entry gate (NEWFRONT-446, ADM-074..079). ---
  /** Cloud home-entry branch snapshot. */
  adminCloudHomeFacts(): Promise<AdminCloudHomeFacts>;
  /** Wait until the cloud home entry settles on `branch`. */
  adminCloudWaitForHomeBranch(branch: AdminCloudHomeBranch): Promise<void>;
  /** Open the admin root with `cookies` planted (cloud sessions). */
  adminCloudOpenRootWithCookies(cookies: ParityBrowserCookie[]): Promise<void>;
  /** Fail the next `times` admin-check reads with `status`, then pass through. */
  adminCloudFailAdminCheck(times: number, status: number): Promise<void>;
  /** Stop failing admin-check reads (a held fault, released before a retry). */
  adminCloudReleaseAdminCheckFault(): Promise<void>;
  /** Fail the next `times` auth-probe reads with `status`, then pass through. */
  adminCloudFailAuthProbe(times: number, status: number): Promise<void>;
  /** Delay the next admin-check read by `ms` (one-shot; the spinner then shows deterministically). */
  adminCloudDelayAdminCheck(ms: number): Promise<void>;
  /** Admin-check + probe request counts since the last spy start. */
  adminCloudGateFetchCounts(): Promise<AdminCloudGateFetchCounts>;
  /** Start counting the gate's admin-check + probe reads. */
  adminCloudStartGateFetchSpy(): Promise<void>;
  /** Click the gate's retry control. */
  adminCloudRetry(): Promise<void>;

  // --- Space board access & routing (NEWFRONT-447, SPC-001..009). ---
  /** Open an arbitrary in-app path (unknown-path routing, SPC-009). */
  spaceAccessOpenPath(path: string): Promise<void>;
  /** Open the landing page already signed in as the owner of `cookies`; `query` is old-app raw (e.g. "?next_path=/issues/x"). */
  spaceAccessOpenLandingAuthed(cookies: ParityBrowserCookie[], query?: string): Promise<void>;
  /** Whether the board navbar shows the project named `name`. */
  spaceAccessBoardShowsProject(name: string): Promise<boolean>;
  /** Whether the board shows an issue row titled `name`. */
  spaceAccessBoardShowsIssue(name: string): Promise<boolean>;
  /** Anonymous navbar sign-in link target, or null when no sign-in renders. */
  spaceAccessSignInHref(): Promise<string | null>;
  /** Whether the anchor-not-found illustration page is showing (SPC-003). */
  spaceAccessAnchorNotFoundVisible(): Promise<boolean>;
  /** Whether the generic board-error illustration is showing (SPC-004). */
  spaceAccessBoardErrorVisible(): Promise<boolean>;
  /** Whether the generic unmatched-path page is showing (SPC-009). */
  spaceAccessGenericNotFoundVisible(): Promise<boolean>;
  /** Fail upcoming board-settings fetches with `status` until cleared (SPC-004). */
  spaceAccessFailBoardSettings(status?: number): Promise<void>;
  /** Fail upcoming public-issues fetches with `status` until cleared (SPC-004). */
  spaceAccessFailBoardIssues(status?: number): Promise<void>;
  /** Remove the failure routes installed by the spaceAccessFail* helpers. */
  spaceAccessClearFailures(): Promise<void>;
  /** Which landing view is showing: the auth forms, the signed-in guidance, or neither. */
  spaceAccessLandingView(): Promise<"auth" | "signed-in" | "other">;
  /** Document head metadata facts for the current page (SPC-007, SPC-008). */
  spaceAccessHeadFacts(): Promise<SpaceAccessHeadFacts>;
  // --- Space filters + peek shell (NEWFRONT-450, SPC-034–046). Appended;
  // --- existing methods above are untouched per the shared driver contract.

  // --- Space filters (NEWFRONT-450, SPC-034–039). ---
  /** Open the board's Filters panel; resolves once the panel is visible. */
  spaceFiltersOpenPanel(): Promise<void>;
  /** True while the Filters panel is open. */
  spaceFiltersPanelOpen(): Promise<boolean>;
  /** Facet section titles the open panel offers, in display order. */
  spaceFiltersFacetNames(): Promise<string[]>;
  /** Type into the panel's option search box. */
  spaceFiltersSearchPanel(query: string): Promise<void>;
  /** Priority option rows the panel currently shows (after any search narrowing). */
  spaceFiltersPriorityOptions(): Promise<Array<{ name: string; checked: boolean }>>;
  /** State option rows the panel currently shows (after any search narrowing). */
  spaceFiltersStateOptions(): Promise<Array<{ name: string; checked: boolean }>>;
  /** Pick a priority option by its displayed name. */
  spaceFiltersPickPriority(name: string): Promise<void>;
  /** Pick a state option by its displayed name. */
  spaceFiltersPickState(name: string): Promise<void>;
  /** True when the panel shows its no-matches empty text. */
  spaceFiltersNoMatchesVisible(): Promise<boolean>;
  /** Search boxes currently visible on the board (panel search included). */
  spaceFiltersSearchBoxCount(): Promise<number>;
  /** Switch the board to another layout through the layout switcher. */
  spaceFiltersSwitchLayout(layout: "list" | "kanban"): Promise<void>;
  /** Facets on the applied strip: each facet key plus its pill values (empty when the facet renders a bare shell). */
  spaceFiltersAppliedFacets(): Promise<Array<{ facet: string; values: string[] }>>;
  /** True while the applied-filters strip renders above the board. */
  spaceFiltersAppliedStripVisible(): Promise<boolean>;
  /** Remove one applied value pill. */
  spaceFiltersRemoveValue(facet: string, value: string): Promise<void>;
  /** Clear one facet through its facet-level remove control. */
  spaceFiltersClearFacet(facet: string): Promise<void>;
  /** Clear every applied filter through the strip's clear-all control. */
  spaceFiltersClearAll(): Promise<void>;
  /** Issue titles currently rendered on the board, in display order. */
  spaceFiltersVisibleIssueNames(): Promise<string[]>;
  /** Start recording public-issues fetch URLs the page issues. */
  spaceFiltersBeginFetchWatch(): Promise<void>;
  /** Query strings of the recorded public-issues fetches, in order. */
  spaceFiltersFetchQueries(): Promise<string[]>;
  /** Stop the fetch watch and detach its listener. */
  spaceFiltersEndFetchWatch(): Promise<void>;

  // --- Space peek shell (NEWFRONT-450, SPC-040–046). ---
  /** True while the issue overlay is open. */
  spacePeekOpen(): Promise<boolean>;
  /** Current overlay presentation; call only while the overlay is open. */
  spacePeekMode(): Promise<"side" | "modal" | "full">;
  /** Switch the open overlay to another presentation; resolves once it renders. */
  spacePeekSwitchMode(mode: "side" | "modal" | "full"): Promise<void>;
  /** Close the overlay; Escape works in every presentation, the header control only in side view, the backdrop only in modal/full. */
  spacePeekClose(via?: "header" | "backdrop" | "escape"): Promise<void>;
  /** Public key line the overlay shows (e.g. "PAR-12"), or null when closed. */
  spacePeekKey(): Promise<string | null>;
  /** Issue title the overlay shows, or null when closed. */
  spacePeekTitle(): Promise<string | null>;
  /** True when the overlay renders a description block. */
  spacePeekDescriptionVisible(): Promise<boolean>;
  /** Rendered description text, or null when no description block shows. */
  spacePeekDescriptionText(): Promise<string | null>;
  /** Raw markup of the rendered description block, or null when none shows. */
  spacePeekDescriptionHtml(): Promise<string | null>;
  /** True when the rendered description block is user-editable. */
  spacePeekDescriptionEditable(): Promise<boolean>;
  /** Property rows the overlay shows, or null when closed. */
  spacePeekProperties(): Promise<{
    state: string;
    priority: string;
    dueDate: string | null;
    dueDateEmphasised: boolean;
  } | null>;
  /** True when the overlay mounts its activity (comments) section. */
  spacePeekActivityVisible(): Promise<boolean>;
  /** True when the overlay header offers a copy-link action. */
  spacePeekCopyLinkVisible(): Promise<boolean>;
  /** Click the header copy-link action (arranges clipboard permission first). */
  spacePeekCopyLink(): Promise<void>;
  /** Current clipboard text (arranges read permission first). */
  spacePeekReadClipboard(): Promise<string>;
  /** Most recently shown notice, if any is still visible. */
  spacePeekLastNotice(): Promise<{ title: string; message: string } | null>;
  /** True while the open overlay shows skeleton placeholders instead of content. */
  spacePeekSkeletonsVisible(): Promise<boolean>;

  // --- Spaces auth (NEWFRONT-452, SPC-058..064). ---
  /** Open the landing auth card with query params (email, error_code, next_path). */
  spaceAuthOpenLanding(params?: { email?: string; errorCode?: string; nextPath?: string }): Promise<void>;
  /** Which auth step is currently visible (signed-in = the logged-in guidance view). */
  spaceAuthStep(): Promise<"email" | "password" | "code" | "signed-in" | "unknown">;
  /** Sign-in vs sign-up mode from the header, or null when the header is absent. */
  spaceAuthMode(): Promise<"sign-in" | "sign-up" | null>;
  /** Fill the email field without submitting (negative validation tests). */
  spaceAuthFillEmail(email: string): Promise<void>;
  /** Submit the email step; resolves once the email-check round-trip answers. */
  spaceAuthSubmitEmail(email: string): Promise<void>;
  /** Blur the email field (inline validation shows only when unfocused). */
  spaceAuthBlurEmail(): Promise<void>;
  /** Inline email validation error text, or null when none shows. */
  spaceAuthEmailError(): Promise<string | null>;
  /** Whether the email-step Continue submit is enabled. */
  spaceAuthEmailSubmitEnabled(): Promise<boolean>;
  /** Dismissible auth banner text, or null when no banner shows. */
  spaceAuthBanner(): Promise<string | null>;
  /** Dismiss the auth banner. */
  spaceAuthDismissBanner(): Promise<void>;
  /** Clear the locked email on the password/code step, resetting the card. */
  spaceAuthClearEmail(): Promise<void>;
  /** Fill the password fields without submitting (strength/meter tests). */
  spaceAuthFillPassword(password: string, confirmPassword?: string): Promise<void>;
  /** Submit the password step; ends in a full page load (success or error redirect). */
  spaceAuthSubmitPassword(password: string, confirmPassword?: string): Promise<void>;
  /** Whether the password-step submit is currently enabled. */
  spaceAuthPasswordSubmitEnabled(): Promise<boolean>;
  /** Toggle password visibility for one field. */
  spaceAuthTogglePasswordVisibility(field: "password" | "confirm"): Promise<void>;
  /** Input types of the password fields (null when the field is absent). */
  spaceAuthPasswordFieldTypes(): Promise<{ password: string | null; confirm: string | null }>;
  /** True when the sign-up strength meter shows. */
  spaceAuthStrengthMeterVisible(): Promise<boolean>;
  /** Strength meter message text, or null when the meter is absent. */
  spaceAuthStrengthMessage(): Promise<string | null>;
  /** Inline password-confirmation error text, or null when none shows. */
  spaceAuthPasswordMismatchError(): Promise<string | null>;
  /** Label of the primary button on the password step, or null when absent. */
  spaceAuthPasswordPrimaryLabel(): Promise<string | null>;
  /** True when the "sign in with unique code" switch shows (sign-in + SMTP). */
  spaceAuthSeesSwitchToCode(): Promise<boolean>;
  /** Click the switch-to-code control; resolves once the code step renders. */
  spaceAuthSwitchToCode(): Promise<void>;
  /** True when the password step shows the confirm-password field (sign-up mode). */
  spaceAuthSeesConfirmPassword(): Promise<boolean>;
  /** Submit the code step; ends in a full page load (success or error redirect). */
  spaceAuthSubmitCode(code: string): Promise<void>;
  /** Resend control on the code step: enabled state plus label. */
  spaceAuthCodeResendState(): Promise<{ disabled: boolean; label: string | null }>;
  /** Click the resend control on the code step. */
  spaceAuthClickResendCode(): Promise<void>;
  /** Names of the visible third-party provider buttons (empty when none). */
  spaceAuthProviderButtons(): Promise<string[]>;
  /** True when the provider buttons render compact (icon-only, password step). */
  spaceAuthProvidersCompact(): Promise<boolean>;
  /** True when the provider divider ("or") shows. */
  spaceAuthProvidersDividerVisible(): Promise<boolean>;
  /** Click a third-party provider button (starts a full-page provider flow). */
  spaceAuthClickProvider(name: string): Promise<void>;
  /** Auth header heading + subline (nulls when the header is absent). */
  spaceAuthHeader(): Promise<{ heading: string | null; subline: string | null }>;
  /** Consent line text plus terms/privacy link targets (nulls when absent). */
  spaceAuthConsent(): Promise<{ text: string | null; termsHref: string | null; privacyHref: string | null }>;

  // --- Desktop sign-in halves (NEWFRONT-486, AUTH-022/AUTH-023).
  // --- Appended; existing methods above are untouched per the shared driver
  // --- contract. These run against the desktop builds only: the community
  // --- desktop entry renders the unavailable card, and the failure landing
  // --- echoes provider errors above the card.
  /** True when the desktop unavailable-sign-in card shows. */
  authDesktopUnavailableCardVisible(): Promise<boolean>;
  /** Accessible label of the unavailable card's open-in-browser action, or null when absent. */
  authDesktopBrowserActionLabel(): Promise<string | null>;
  /** Click the unavailable card's open-in-browser action. */
  authDesktopClickBrowserAction(): Promise<void>;
  /** Alert text above the desktop card for a failure landing, or null when no alert shows. */
  authDesktopFailureAlertText(): Promise<string | null>;

  // --- Live cycles list, rows, detail nav, peek, search, filters
  // --- (NEWFRONT-249, CYC-001–008). Appended; existing methods above are
  // --- untouched per the shared driver contract. Covers the project
  // --- cycles list (hero plus grouped rows), row markers, detail
  // --- addresses, the peek query key, search, filters, and chips.
  /** Open the project's live cycles list; ends with rows or an empty view settled. */
  cyclesOpenList(workspaceSlug: string, projectId: string): Promise<void>;
  /** Cycle names currently rendered as rows, in display order (hero first, then groups). */
  cyclesVisibleNames(): Promise<string[]>;
  /** Group headings currently rendered (Active/Upcoming/Completed), in display order. */
  cyclesGroupHeadings(): Promise<string[]>;
  /** Per-group row counts parsed from the group headings. */
  cyclesGroupCounts(): Promise<{ upcoming: number; completed: number }>;
  /** Toggle one group disclosure by heading prefix (Upcoming/Completed/Active). */
  cyclesGroupToggle(section: string): Promise<void>;
  /** Whether one group disclosure is currently expanded. */
  cyclesGroupExpanded(section: string): Promise<boolean>;
  /** Active-cycle hero row name, or null when no hero row shows. */
  cyclesHeroName(): Promise<string | null>;
  /** Href of the named row's detail link, or null when absent. */
  cyclesRowHref(name: string): Promise<string | null>;
  /** Click the named row; ends on its detail page. */
  cyclesClickRow(name: string): Promise<void>;
  /** Open a cycle detail address directly; ends with detail settled. */
  cyclesOpenDetail(workspaceSlug: string, projectId: string, cycleId: string): Promise<void>;
  /** Cycle name shown on the detail page, or null when absent. */
  cyclesDetailName(): Promise<string | null>;
  /** Work-item names shown on the detail page, in display order. */
  cyclesDetailIssueNames(): Promise<string[]>;
  /** Per-row marker facts for the named row (ring, dates, extras, favorite). */
  cyclesRowFacts(name: string): Promise<CyclesRowFacts>;
  /** Open the side peek for the named row; waits for the peek to show it. */
  cyclesOpenPeek(name: string): Promise<void>;
  /** Open the list at a shared peek link; ends with the peek settled. */
  cyclesOpenPeekLink(workspaceSlug: string, projectId: string, cycleId: string): Promise<void>;
  /** Peek panel heading, or null when no peek is open. */
  cyclesPeekName(): Promise<string | null>;
  /** The peekCycle address param, or null when absent. */
  cyclesPeekParam(): Promise<string | null>;
  /** Close the open peek via its dismiss control; waits for it to clear. */
  cyclesClosePeek(): Promise<void>;
  /** Reload the current page; ends with the list settled. */
  cyclesReload(): Promise<void>;
  /** Navigate back client-side (detail to list); ends with the list settled. */
  cyclesGoBack(): Promise<void>;
  /** Whether the header search box is currently expanded. */
  cyclesSearchExpanded(): Promise<boolean>;
  /** Expand the search box via the magnifier; waits for expansion. */
  cyclesSearchOpen(): Promise<void>;
  /** Replace the search box text (the list filters live). */
  cyclesSearchFill(text: string): Promise<void>;
  /** Current search box text. */
  cyclesSearchText(): Promise<string>;
  /** Press Escape while the search box is focused. */
  cyclesSearchEscape(): Promise<void>;
  /** Click the search box clear control; waits for it to collapse. */
  cyclesSearchClear(): Promise<void>;
  /** Click a neutral header area (outside-click behavior). */
  cyclesClickAway(): Promise<void>;
  /** Open the filters menu; waits for its panel. */
  cyclesFiltersOpen(): Promise<void>;
  /** Dismiss the filters menu. */
  cyclesFiltersClose(): Promise<void>;
  /** Filter dimension headings in the open panel, in display order. */
  cyclesFilterSections(): Promise<string[]>;
  /** Filter option labels in the open panel, in display order. */
  cyclesFilterOptionNames(): Promise<string[]>;
  /** Pick one filter option within its dimension; waits for the list to update. */
  cyclesFilterPick(section: string, optionName: string): Promise<void>;
  /** Applied-filter chip texts in display order (empty when no row shows). */
  cyclesFilterChipTexts(): Promise<string[]>;
  /** Remove the chip carrying `chipText`; waits for the list to update. */
  cyclesFilterRemoveChip(chipText: string): Promise<void>;
  /** Click the clear-all control; waits for the chip row to clear. */
  cyclesFiltersClearAll(): Promise<void>;
  /** Whether the filters menu button carries its active-filters marker. */
  cyclesFiltersActive(): Promise<boolean>;
}

/** One list group / board column as rendered (NEWFRONT-449, SPC-023/024/026). */
export interface SpaceBoardGroupFacts {
  /** State name the header shows. */
  name: string;
  /** Issue count the header shows. */
  count: number;
  /** Cards/rows actually rendered beneath the header. */
  rows: number;
}

/** Public identity facts of one board card (NEWFRONT-449, SPC-027). */
export interface SpaceBoardCardFacts {
  /** Public key as rendered (project code plus sequence number). */
  key: string;
  /** Card title as rendered. */
  title: string;
  /** Full title the hover tooltip reveals. */
  hoverTitle: string;
}

/** Property markers of one board card (NEWFRONT-449, SPC-028). */
export interface SpaceBoardCardMarkers {
  /** State name the card shows, or null when no state marker renders. */
  state: string | null;
  /** Priority level the card shows (icon level), or null when absent. */
  priority: string | null;
  /** Label names the card shows. */
  labels: string[];
  /** Date marker texts the card shows. */
  dates: string[];
  /** Raw card text (cycle/module absence checks read this). */
  text: string;
}

/** Presence facts of one board card footer (NEWFRONT-449, SPC-029). */
export interface SpaceBoardCardFooter {
  /** Whether any footer region renders inside the card. */
  visible: boolean;
  /** Whether the vote half renders. */
  votes: boolean;
  /** Whether the reaction half renders. */
  reactions: boolean;
}

/** Read-only facts of the board (NEWFRONT-449, SPC-032). */
export interface SpaceBoardDragFacts {
  /** Cards marked draggable. */
  draggable: number;
  /** Drag handles on cards. */
  handles: number;
  /** Inline edit controls (inputs, textareas, editable regions) on cards. */
  editControls: number;
}

/** Order stability across a blur/focus cycle (NEWFRONT-449, SPC-033). */
export interface SpaceBoardStability {
  /** List order before the cycle. */
  before: string[];
  /** List order after the cycle. */
  after: string[];
  /** Public-issues fetches observed during the cycle. */
  refetches: number;

  // --- Space engagement: votes, reactions, comments (NEWFRONT-451, SPC-047–057).
  // --- Appended; existing methods above are untouched per the shared driver
  // --- contract. Reads target the peek overlay the card click opens; specs
  // --- flip the board engagement flags through the publish API per test.
  /** Open an issue's peek overlay through its board card; waits for the overlay. */
  spaceCommentOpenIssue(anchor: string, issueName: string, view?: string): Promise<void>;
  /** Where an anonymous action landed: the auth gate plus the preserved return address. */
  spaceCommentAuthLanding(): Promise<SpaceAuthLanding>;
  /** Whether the overlay renders vote controls (publisher flag on). */
  spaceVoteOverlayVisible(): Promise<boolean>;
  /** Overlay vote controls: counts, voter names, and the visitor's own votes. */
  spaceVoteOverlay(): Promise<SpaceVoteState>;
  /** Click the overlay up/down vote control. */
  spaceVoteClick(direction: "up" | "down"): Promise<void>;
  /** Whether a kanban card renders a vote footer. */
  spaceVoteCardVisible(anchor: string, issueName: string): Promise<boolean>;
  /** Kanban card vote footer: counts and the visitor's own votes. */
  spaceVoteCard(anchor: string, issueName: string): Promise<SpaceVoteState>;
  /** Click a kanban card's up/down vote control. */
  spaceVoteCardClick(anchor: string, issueName: string, direction: "up" | "down"): Promise<void>;
  /** Whether the overlay renders the emoji reaction control (flag on, not embedded). */
  spaceReactOverlayVisible(): Promise<boolean>;
  /** Overlay emoji reaction groups: emoji, counts, own state, reactors. */
  spaceReactGroups(): Promise<SpaceReactionGroup[]>;
  /** Add an emoji reaction through the overlay picker; resolves with the picked emoji. */
  spaceReactPick(): Promise<SpacePickedEmoji>;
  /** Click an overlay reaction pill (toggles the visitor's own reaction). */
  spaceReactClickGroup(emoji: string): Promise<void>;
  /** Whether the overlay reaction add control is present. */
  spaceReactAddVisible(): Promise<boolean>;
  /** Comments section state: visibility, thread, composer, sign-in invitation. */
  spaceCommentSection(): Promise<SpaceCommentSection>;
  /** Comment rows in display order. */
  spaceCommentRows(): Promise<SpaceCommentRow[]>;
  /** Toggle a folded comment's expanded state by row index. */
  spaceCommentToggleFold(index: number): Promise<void>;
  /** Type into the comment composer. */
  spaceCommentType(text: string): Promise<void>;
  /** Paste a formatted HTML fragment into the comment composer. */
  spaceCommentPasteHtml(html: string): Promise<void>;
  /** Current composer draft text. */
  spaceCommentDraftText(): Promise<string>;
  /** Submit the composer through the Enter key. */
  spaceCommentSubmit(): Promise<void>;
  /** Attach an image through the composer toolbar; resolves when the upload settles. */
  spaceCommentAttachImage(filePath: string): Promise<void>;
  /** Type a mention trigger and pick a member suggestion. */
  spaceCommentMention(memberQuery: string, memberDisplayName: string): Promise<SpaceMentionResult>;
  /** Whether the mention suggestion dropdown is currently open. */
  spaceCommentMentionOpen(): Promise<boolean>;
  /** Whether a comment row's owner menu is present (row index). */
  spaceCommentMenuVisible(index: number): Promise<boolean>;
  /** Open a comment row's owner menu (row index). */
  spaceCommentMenuOpen(index: number): Promise<void>;
  /** Names of the open owner menu's entries, in order. */
  spaceCommentMenuItems(index: number): Promise<string[]>;
  /** Return address the anonymous sign-in invitation preserves, when one renders. */
  spaceCommentInviteReturn(): Promise<{ anchor: string | null; view: string | null; issueId: string | null } | null>;
  /** Edit a comment through its owner menu: opens edit mode, replaces content, saves. */
  spaceCommentEdit(index: number, newText: string): Promise<void>;
  /** Delete a comment through its owner menu. */
  spaceCommentDelete(index: number): Promise<void>;
  /** Visible toast notices (for the failure paths). */
  spaceCommentNotices(): Promise<SpaceCommentNotice[]>;
  /** Reaction groups on one comment row. */
  spaceCommentReactions(index: number): Promise<SpaceReactionGroup[]>;
  /** Add an emoji reaction to one comment row; resolves with the picked emoji. */
  spaceCommentReactPick(index: number): Promise<SpacePickedEmoji>;
  /** Click a comment reaction pill (toggles the visitor's own reaction). */
  spaceCommentReactClick(index: number, emoji: string): Promise<void>;
  /** Whether a comment row's reaction add control is present. */
  spaceCommentReactAddVisible(index: number): Promise<boolean>;
  /** Switch the peek overlay presentation; resolves when the new view renders. */
  spaceCommentPeekMode(mode: "side" | "modal" | "full"): Promise<void>;
  /** Embed the board in a same-origin frame and open an issue inside it. */
  spaceCommentOpenEmbed(anchor: string, issueName: string): Promise<void>;
  /** Comments section state inside the embedded frame. */
  spaceCommentEmbedSection(): Promise<SpaceCommentSection>;
  /** Whether the embedded overlay renders the issue emoji add control. */
  spaceReactEmbedAddVisible(): Promise<boolean>;
  /** Whether a comment row's owner menu is present inside the embedded frame. */
  spaceCommentEmbedMenuVisible(index: number): Promise<boolean>;
  /** Comment reaction groups plus the add control inside the embedded frame. */
  spaceCommentEmbedReactions(index: number): Promise<{ groups: SpaceReactionGroup[]; addVisible: boolean }>;

  // --- Cloud marketing + public surface (NEWFRONT-437, CLOUD-003/004/006–010).
  // --- Appended; existing methods above are untouched per the shared driver
  // --- contract. Reads are structural (counts, link targets, metadata) so
  // --- scenarios prove the surface without quoting its marketing copy.

  /** Hero entry-link destinations on the marketing home, in display order. */
  cloudHomeHeroActionHrefs(): Promise<string[]>;
  /** Follow the hero entry link pointing at `href`; resolves once the landing renders. */
  cloudHomeClickHeroAction(href: string): Promise<void>;
  /** Structural facts of the marketing home below the hero. */
  cloudHomeSections(): Promise<CloudHomeSectionFacts>;
  /** Marketing header links: label, destination, external marker, current visibility. */
  cloudHeaderLinks(): Promise<CloudNavLinkFacts[]>;
  /** Marketing footer links: label, destination, external marker, current visibility. */
  cloudFooterLinks(): Promise<CloudNavLinkFacts[]>;
  /** Focus each header link in turn; false when any shows no focus ring. */
  cloudHeaderLinksFocusVisible(): Promise<boolean>;
  /** Focus each footer link in turn; false when any shows no focus ring. */
  cloudFooterLinksFocusVisible(): Promise<boolean>;
  /** Structural facts of the docs page (concepts, quick-start steps, code blocks, out-links). */
  cloudDocsFacts(): Promise<CloudDocsFacts>;
  /** Desktop installer cards: OS, architecture note, primary + secondary download targets. */
  cloudDownloadCards(): Promise<CloudDownloadCardFacts[]>;
  /** Whether the unsigned-installer notice renders on the downloads page. */
  cloudDownloadsSigningNoticeVisible(): Promise<boolean>;
  /** CLI card facts: installer target, docs link, web fallback link, sign-in hint. */
  cloudCliCardFacts(): Promise<CloudCliCardFacts>;
  /** Pricing tiers: name, price, call-to-action target, feature count. */
  cloudPricingTiers(): Promise<CloudPricingTierFacts[]>;
  /** Enterprise contact link target, or null when absent. */
  cloudPricingEnterpriseHref(): Promise<string | null>;
  /** FAQ entries on the pricing page (question/answer articles). */
  cloudPricingFaqCount(): Promise<number>;
  /** Whether the free tier names its creation cap numerically. */
  cloudPricingFreeCapMentioned(): Promise<boolean>;
  /** Arm a one-shot handler for the next quota confirm dialog (records its message). */
  cloudArmConfirm(action: "accept" | "dismiss"): Promise<void>;
  /** Confirm dialog messages seen since the arm, in arrival order. */
  cloudConfirmMessages(): Promise<string[]>;
  /** Wait for the upgrade popup the last accept opened; resolves with its URL and closes it. */
  cloudUpgradePopupUrl(): Promise<string>;

  // --- Cloud login, SSO + session (NEWFRONT-438, CLOUD-011..020).
  // --- Appended; existing methods above are untouched per the shared driver
  // --- contract. Reads are structural (targets, counts, markers) so the
  // --- scenarios prove the handoff without quoting the card's copy.

  /** Structural facts of the cloud sign-in card on the current page. */
  cloudLoginCardFacts(): Promise<CloudLoginCardFacts>;
  /** Intercept sign-in handoffs: record request URLs, answer a stub page. */
  cloudArmSignInCapture(): Promise<void>;
  /** Sign-in request URLs seen since the arm, in arrival order. */
  cloudSignInRequests(): Promise<string[]>;
  /** Wait for capture entry `previousCount + 1`; resolves with its URL. */
  cloudAwaitSignInRequest(previousCount: number): Promise<string>;
  /** Activate the login card's continue action. */
  cloudClickLoginContinue(): Promise<void>;
  /** Whether the explicit-logout marker is recorded for this tab. */
  cloudExplicitLogoutMarker(): Promise<boolean>;
  /** Record the explicit-logout marker before the next page load (fixture). */
  cloudSuppressSilentSso(): Promise<void>;
  /** Count refresh-endpoint posts from now on. */
  cloudArmRefreshCounter(): Promise<void>;
  /** Refresh-endpoint posts seen since the arm. */
  cloudRefreshHits(): Promise<number>;
  /** Sign out through the sidebar account menu; resolves on the sign-in card. */
  cloudSignOutViaAccountMenu(): Promise<void>;
  /** Fail the refresh endpoint at the network level from now on. */
  cloudFailRefresh(): Promise<void>;
  /** Record console errors from now on. */
  cloudArmConsoleErrors(): Promise<void>;
  /** Console error texts seen since the arm, in arrival order. */
  cloudConsoleErrors(): Promise<string[]>;
  /** Present a desktop shell global that rejects browser opens (fixture). */
  cloudStubTauri(): Promise<void>;
  /** Password inputs on the current page (onboarding profile step). */
  cloudProfilePasswordFieldCount(): Promise<number>;
  /** Drop the access cookie while keeping the refresh cookie (fixture). */
  cloudExpireAccessCookie(): Promise<void>;
  /** Cookie names the browser jar currently holds. */
  cloudCookieNames(): Promise<string[]>;
  /** Provider button facts on a generic auth card. */
  cloudProviderButtonFacts(): Promise<CloudProviderButtonFacts>;
  /** Activate the generic card's provider button. */
  cloudClickProviderButton(): Promise<void>;

  // --- Cloud Apps catalog (NEWFRONT-439, CLOUD-021–031). Appended;
  // --- existing methods above are untouched per the shared driver
  // --- contract. Reads are structural (entries, cards, badges, dialog
  // --- fields, toggle state) so scenarios prove the catalog without
  // --- quoting its copy; server payloads stay in the specs' stubs.

  /** Sidebar Apps entry destination, or null when the entry is absent. */
  cloudAppsSidebarHref(): Promise<string | null>;
  /** Whether the sidebar Apps entry carries its own icon node. */
  cloudAppsSidebarIconPresent(): Promise<boolean>;
  /** Follow the sidebar Apps entry; resolves once the Apps page renders. */
  cloudAppsOpenFromSidebar(): Promise<void>;
  /** Intro paragraph under the Apps heading (ownership statement). */
  cloudAppsIntroText(): Promise<string>;
  /** Replace the catalog search box contents. */
  cloudAppsSearchFill(value: string): Promise<void>;
  /** Type into the catalog search box with realistic keystroke pacing. */
  cloudAppsSearchTypeText(text: string): Promise<void>;
  /** Current catalog search box contents. */
  cloudAppsSearchValue(): Promise<string>;
  /** Whether the catalog search box renders. */
  cloudAppsSearchVisible(): Promise<boolean>;
  /** Card titles in display order. */
  cloudAppsCardNames(): Promise<string[]>;
  /** Per-card facts: badge, actions, icon source, letter tile. */
  cloudAppsCards(): Promise<CloudAppsCardFacts[]>;
  /** Click a card action (Install, Connect, Uninstall) by card name. */
  cloudAppsClickCardAction(name: string, action: "Install" | "Connect" | "Uninstall"): Promise<void>;
  /** Whether a card action is currently disabled (busy guard). */
  cloudAppsCardActionDisabled(name: string, action: "Install" | "Connect" | "Uninstall"): Promise<boolean>;
  /** Step the catalog pager forward; resolves once the window renders. */
  cloudAppsNextPage(): Promise<void>;
  /** Step the catalog pager back; resolves once the window renders. */
  cloudAppsPrevPage(): Promise<void>;
  /** Pager indicator text (page number), or null when the pager is absent. */
  cloudAppsPageIndicator(): Promise<string | null>;
  /** Whether the catalog pager renders. */
  cloudAppsPagerVisible(): Promise<boolean>;
  /** Whether the credential dialog is open. */
  cloudAppsDialogVisible(): Promise<boolean>;
  /** Credential dialog title, or null when closed. */
  cloudAppsDialogTitle(): Promise<string | null>;
  /** Credential dialog fields: label, input type, placeholder. */
  cloudAppsDialogFields(): Promise<CloudAppsDialogFieldFacts[]>;
  /** Credential dialog publisher instructions as rendered plain text, or null when absent. */
  cloudAppsDialogInstructions(): Promise<string | null>;
  /** Credential dialog issuance-link target, or null when no link renders. */
  cloudAppsDialogSetupHref(): Promise<string | null>;
  /** Fill a credential dialog field by its label. */
  cloudAppsDialogFill(label: string, value: string): Promise<void>;
  /** Submit the credential dialog. */
  cloudAppsDialogSubmit(): Promise<void>;
  /** Cancel the credential dialog. */
  cloudAppsDialogCancel(): Promise<void>;
  /** Inline credential dialog errors currently shown. */
  cloudAppsDialogErrors(): Promise<string[]>;
  /** Current values of the credential dialog inputs, in order. */
  cloudAppsDialogInputValues(): Promise<string[]>;
  /** Agent-tools toggle state, or null when the toggle is absent. */
  cloudAppsToggleState(): Promise<boolean | null>;
  /** Agent-tools toggle label plus description text. */
  cloudAppsToggleText(): Promise<string>;
  /** Flip the agent-tools toggle. */
  cloudAppsToggle(): Promise<void>;
  /** Inner text of the Apps content area (panels, empty states, catalog). */
  cloudAppsBodyText(): Promise<string>;
  /** Whether the catalog card grid renders (vs a panel or empty state). */
  cloudAppsCatalogVisible(): Promise<boolean>;
  /** Click the visible Retry button in the Apps content area. */
  cloudAppsClickRetry(): Promise<void>;
  /** Click the sidebar create control on the Apps page; true when the create dialog opens. */
  cloudAppsSidebarCreateOpensDialog(): Promise<boolean>;

  // --- Cloud profile tabs + misc (NEWFRONT-440, CLOUD-032..040).
  // --- Appended; existing methods above are untouched per the shared driver
  // --- contract. Reads are structural (titles, markers, control kinds) so
  // --- the scenarios prove the surface without quoting its copy.

  /** Connection cards on the AI Assistant tab, in display order. */
  cloudAssistantConnections(): Promise<CloudAssistantConnectionFacts[]>;
  /** Whether the empty-list guidance panel shows, and its add action label. */
  cloudAssistantEmptyState(): Promise<{ visible: boolean; addLabel: string | null }>;
  /** Which assistant-tab view is showing: the list or the add form. */
  cloudAssistantView(): Promise<"list" | "add">;
  /** Open the add-connection view from the list header. */
  cloudAssistantClickAdd(): Promise<void>;
  /** Back to the connections list from the add view. */
  cloudAssistantClickBackToList(): Promise<void>;
  /** Select the connection titled `title` via its radio. */
  cloudAssistantSelectConnection(title: string): Promise<void>;
  /** Set the model of the connection titled `title` (picker or text commit). */
  cloudAssistantSetModel(title: string, model: string): Promise<void>;
  /** Activate a row action (Test, Remove, Disconnect) on the titled connection. */
  cloudAssistantClickRowAction(title: string, action: "Test" | "Remove" | "Disconnect"): Promise<void>;
  /** Add-form structure: provider options, base-field visibility, save state. */
  cloudAssistantAddFormFacts(): Promise<CloudAssistantAddFormFacts>;
  /** Fill the add form; resolves once the fields hold the values. */
  cloudAssistantAddFill(fill: CloudAssistantAddFill): Promise<void>;
  /** Submit the add form; resolves once the list view returns. */
  cloudAssistantAddSave(): Promise<void>;
  /** Activate the provider sign-in action on the titled account row. */
  cloudAssistantClickProviderSignIn(title: string): Promise<void>;
  /** Wait for the provider sign-in popup; resolves with its URL and closes it. */
  cloudAssistantProviderPopupUrl(): Promise<string>;
  /** Count provider-login status polls from now on. */
  cloudAssistantArmStatusCounter(): Promise<void>;
  /** Provider-login status polls seen since the arm. */
  cloudAssistantStatusHits(): Promise<number>;
  /** Make window.open return null from now on (blocked-popup fixture). */
  cloudAssistantBlockPopups(): Promise<void>;
  /** Stale-session notice on the titled managed row, or null when absent. */
  cloudAssistantStaleNotice(title: string): Promise<string | null>;
  /** Visible profile-sidebar tab labels, in display order. */
  cloudProfileSidebarTabs(): Promise<string[]>;
  /** Active profile tab key, or null when none marks active. */
  cloudProfileActiveTab(): Promise<string | null>;
  /** Whether the shared tool-server section renders inside the assistant tab. */
  cloudAssistantMcpSectionVisible(): Promise<boolean>;
  /** Whether the Apps pointer renders inside the assistant tab. */
  cloudAssistantAppsPointerVisible(): Promise<boolean>;
  /** Whether the workspace sidebar shows the plan badge (absent on the oracle: NEWFRONT-474). */
  cloudPlanBadgeVisible(): Promise<boolean>;
  /** Plan badge label + hover hint, or null when no badge renders. */
  cloudPlanBadgeFacts(): Promise<{ label: string; hint: string } | null>;
  /** Focused control role + label, or null when the body holds focus. */
  cloudFocusedControl(): Promise<{ role: string; label: string } | null>;
  // --- Cloud worklog (NEWFRONT-672, ISS-206 cloud leg). Appended;
  // --- existing methods above are untouched per the shared driver
  // --- contract. The cloud spec runs on web_new only: the OSS oracle
  // --- provably lacks the surface and the private cloud oracle's UI is
  // --- intentionally not pinned here, so the old-app driver throws on
  // --- these while worklogCreateVisible stays the OSS absence reader.
  /** Log `minutes` on `loggedOn` with `note` through the header control; resolves once the dialog closes. */
  worklogLogEntry(minutes: number, loggedOn: string, note: string): Promise<void>;
  /** Sidebar worklog total text, or null when no total renders. */
  worklogSidebarTotal(): Promise<string | null>;
  /** Sidebar worklog entry texts in display order. */
  worklogSidebarEntryTexts(): Promise<string[]>;
}

/** One Apps catalog card (NEWFRONT-439, CLOUD-028). */
export interface CloudAppsCardFacts {
  /** Card title as rendered. */
  name: string;
  /** State marker: installed, unfinished (needs connection), or none. */
  badge: "installed" | "needs-connection" | null;
  /** Action buttons on the card, in display order. */
  actions: string[];
  /** Icon source: self-hosted graphic, proxied URL, or letter tile only. */
  icon: "inline" | "proxied" | "letter";
  /** Letter-tile character rendered under the icon. */
  letter: string;
}

/** One credential-dialog field (NEWFRONT-439, CLOUD-027). */
export interface CloudAppsDialogFieldFacts {
  /** Field label as rendered. */
  label: string;
  /** Input type (secret fields mask). */
  inputType: string;
  /** Placeholder hint, or empty when none. */
  placeholder: string;
}

/** One marketing header/footer link (NEWFRONT-437, CLOUD-004). */
export interface CloudNavLinkFacts {
  /** Accessible label as rendered. */
  label: string;
  /** Link destination (path or absolute URL). */
  href: string;
  /** Whether the link opens a new tab. */
  external: boolean;
  /** Whether the link is currently visible (responsive hiding). */
  visible: boolean;
}

/** Marketing home structure below the hero (NEWFRONT-437, CLOUD-003). */
export interface CloudHomeSectionFacts {
  /** Four-item value strip entries. */
  stripItems: number;
  /** Numbered workflow steps. */
  workflowSteps: number;
  /** Capability articles. */
  capabilityCards: number;
  /** Open-source panel repository link target, or null when absent. */
  repoHref: string | null;
  /** Whether the repository link opens a new tab. */
  repoExternal: boolean;
  /** Closing call-to-action destination, or null when absent. */
  closingCtaHref: string | null;
}

/** Docs page structure (NEWFRONT-437, CLOUD-006). */
export interface CloudDocsFacts {
  /** Three-part model concept cards. */
  concepts: number;
  /** Numbered quick-start steps. */
  steps: number;
  /** Command blocks across the page. */
  codeBlocks: number;
  /** Whether the install blocks reference the stable release-download URL. */
  installRefsReleaseDownload: boolean;
  /** Windows MSI installer link target, or null when absent. */
  msiHref: string | null;
  /** Sign-in entry link target, or null when absent. */
  signInHref: string | null;
  /** Public repository link target, or null when absent. */
  repoHref: string | null;
  /** Trust-note items in the closing band. */
  footerItems: number;
}

/** One desktop installer card (NEWFRONT-437, CLOUD-007). */
export interface CloudDownloadCardFacts {
  /** Operating system the card targets. */
  os: string;
  /** Architecture/format note as rendered. */
  arch: string;
  /** Primary download target. */
  primaryHref: string;
  /** Secondary format target, or null when the card offers one format. */
  secondaryHref: string | null;
}

/** Standalone CLI card facts (NEWFRONT-437, CLOUD-008). */
export interface CloudCliCardFacts {
  /** CLI installer download target. */
  installerHref: string;
  /** Setup-guide docs link target, or null when absent. */
  docsHref: string | null;
  /** Web fallback sign-in link target, or null when absent. */
  webHref: string | null;
  /** Whether the post-install sign-in hint renders. */
  signinHintVisible: boolean;
}

/** One pricing tier (NEWFRONT-437, CLOUD-009). */
export interface CloudPricingTierFacts {
  /** Tier name as rendered. */
  name: string;
  /** Price line as rendered. */
  price: string;
  /** Call-to-action destination. */
  ctaHref: string;
  /** Whether the call-to-action leaves the app. */
  ctaExternal: boolean;
  /** Feature bullets. */
  features: number;
}

/** Cloud sign-in card structure (NEWFRONT-438, CLOUD-011). */
export interface CloudLoginCardFacts {
  /** Card heading as rendered. */
  heading: string;
  /** Account-type line as rendered. */
  accountLine: string;
  /** Continue actions the card offers (exactly one on the oracle). */
  continueActions: number;
  /** Static destination of the continue action. */
  continueHref: string;
  /** Terms link target, or null when absent. */
  termsHref: string | null;
  /** Privacy link target, or null when absent. */
  privacyHref: string | null;
  /** Marketing-home link target, or null when absent. */
  homeHref: string | null;
  /** Resume-failure alert text, or null when no alert renders. */
  alertText: string | null;
}

/** Provider button on a generic auth card (NEWFRONT-438, CLOUD-017). */
export interface CloudProviderButtonFacts {
  /** Whether the provider button renders. */
  visible: boolean;
  /** Provider button label as rendered. */
  label: string;
  /** Whether the standard continue action renders alongside it. */
  standardContinueVisible: boolean;
}

/** One AI Assistant connection card (NEWFRONT-440, CLOUD-032). */
export interface CloudAssistantConnectionFacts {
  /** Card title as rendered (kind-appropriate: host-derived or product name). */
  title: string;
  /** Whether the card's select radio is checked. */
  selected: boolean;
  /** Whether the select radio is disabled. */
  selectDisabled: boolean;
  /** Status markers rendered on the card, as text. */
  markers: string[];
  /** Model control kind: fixed picker, free text, or hidden (disconnected). */
  modelControl: "select" | "input" | "hidden";
  /** Current model value. */
  modelValue: string;
  /** Picker options when the control is a select. */
  modelOptions: string[];
  /** Whether a last-verified timestamp renders. */
  verifiedVisible: boolean;
  /** Row action labels rendered (Test, Remove, Disconnect, provider sign-in). */
  actions: string[];
}

/** AI Assistant add-form structure (NEWFRONT-440, CLOUD-033). */
export interface CloudAssistantAddFormFacts {
  /** Provider option values in display order. */
  providers: string[];
  /** Currently chosen provider value. */
  provider: string;
  /** Whether the base-URL field renders. */
  baseVisible: boolean;
  /** Whether the save action is enabled. */
  saveEnabled: boolean;
}

/** Add-form fill (NEWFRONT-440, CLOUD-033); every field is optional. */
export interface CloudAssistantAddFill {
  provider?: string;
  baseUrl?: string;
  model?: string;
  key?: string;
}

/** Identity facts of one workspace-member roster row (NEWFRONT-411, WSET-011). */
export interface WsSettingsMemberRowFacts {
  /** Full name as rendered (first plus last). */
  fullName: string;
  /** Display name as rendered. */
  displayName: string;
  /** Member email as rendered. */
  email: string;
  /** Role label as rendered (Admin/Member/Guest), or "Suspended". */
  role: string;
  /** Authentication-medium label, or null when the row shows none. */
  auth: string | null;
  /** Joining-date text as rendered, or null when the row shows none. */
  joiningDate: string | null;
  /** Avatar shape: photo, initial letter, or the suspended marker. */
  avatarKind: "image" | "initial" | "suspended";
  /** Profile-link target of the avatar, or null when it links nowhere. */
  profileHref: string | null;

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
  /** Open one webhook's detail page (shared by NEWFRONT-415 and NEWFRONT-412). With cookies, navigates authenticated and settles on form, denial, or loading; without, uses the ambient session and requires the form or a denial view. */
  wsSettingsOpenWebhookDetail(workspaceSlug: string, webhookId: string, cookies?: ParityBrowserCookie[]): Promise<void>;
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

  // --- Workspace-settings developer oracle (NEWFRONT-412, WSET-016..024,
  // --- WSET-041). Observed on the running old app: an admin-only
  // --- integrations page (banner heading plus one card per git provider,
  // --- each with a token form that verifies against the live provider and
  // --- a disconnect action from the connected state) and an admin-only
  // --- webhooks area (list with loader and empty state, create dialog with
  // --- URL plus all-or-individual event selection, one-time masked secret
  // --- reveal with copy and an auto-downloaded CSV backup, detail page with
  // --- edit form plus secret block and danger-zone delete). Non-admins get
  // --- the shared not-authorized view on every page (the layout intercepts
  // --- members before the detail page's own paragraph can render).
  // --- Appended; existing methods above are untouched per the shared
  // --- driver contract.
  /** Open the integrations page; settles on the cards or the denial view. */
  wsSettingsOpenIntegrations(workspaceSlug: string): Promise<void>;
  /** Whether the integrations page shows the not-authorized view. */
  wsSettingsIntegrationsNotAuthorized(): Promise<boolean>;
  /** Whether the integrations banner heading is visible. */
  wsSettingsIntegrationsHeadingVisible(): Promise<boolean>;
  /** Provider card titles in render order (GitHub, GitLab). */
  wsSettingsIntegrationsProviderTitles(): Promise<string[]>;
  /** Current document title. */
  wsSettingsPageTitle(): Promise<string>;
  /** The GitHub card status line, or null when the card is missing. */
  wsSettingsGithubStatusText(): Promise<string | null>;
  /** Fill the GitHub token field. */
  wsSettingsGithubTokenFill(token: string): Promise<void>;
  /** Current GitHub token field value; empty when the field is missing. */
  wsSettingsGithubTokenValue(): Promise<string>;
  /** Whether the GitHub Connect control is currently enabled. */
  wsSettingsGithubConnectEnabled(): Promise<boolean>;
  /** Click GitHub Connect; resolves once the connect call settles. */
  wsSettingsGithubConnect(): Promise<void>;
  /** Whether the GitHub card shows the connected state. */
  wsSettingsGithubConnected(): Promise<boolean>;
  /** Whether the GitHub Disconnect control is currently visible. */
  wsSettingsGithubDisconnectVisible(): Promise<boolean>;
  /** Click GitHub Disconnect; resolves once the disconnect call settles. */
  wsSettingsGithubDisconnect(): Promise<void>;
  /** The GitLab card status line, or null when the card is missing. */
  wsSettingsGitlabStatusText(): Promise<string | null>;
  /** Fill the GitLab host field. */
  wsSettingsGitlabHostFill(host: string): Promise<void>;
  /** Fill the GitLab token field. */
  wsSettingsGitlabTokenFill(token: string): Promise<void>;
  /** Current GitLab token field value; empty when the field is missing. */
  wsSettingsGitlabTokenValue(): Promise<string>;
  /** Whether the GitLab Connect control is currently enabled. */
  wsSettingsGitlabConnectEnabled(): Promise<boolean>;
  /** Click GitLab Connect; resolves once the connect call settles. */
  wsSettingsGitlabConnect(): Promise<void>;
  /** Whether the GitLab card shows the connected state. */
  wsSettingsGitlabConnected(): Promise<boolean>;
  /** Whether the GitLab Disconnect control is currently visible. */
  wsSettingsGitlabDisconnectVisible(): Promise<boolean>;
  /** Click GitLab Disconnect; resolves once the disconnect call settles. */
  wsSettingsGitlabDisconnect(): Promise<void>;
  /** Open the webhooks list; settles on the list or the denial view. */
  wsSettingsOpenWebhooks(workspaceSlug: string): Promise<void>;
  /** Whether the webhooks list shows the not-authorized view. */
  wsSettingsWebhooksNotAuthorized(): Promise<boolean>;
  /** Whether the webhooks heading is visible. */
  wsSettingsWebhooksHeadingVisible(): Promise<boolean>;
  /** Whether the list-level Add webhook control is visible. */
  wsSettingsWebhooksAddVisible(): Promise<boolean>;
  /** Whether the no-webhooks empty state is visible. */
  wsSettingsWebhooksEmptyVisible(): Promise<boolean>;
  /** Whether the list skeleton is currently showing. */
  wsSettingsWebhooksLoaderVisible(): Promise<boolean>;
  /** Hold list GETs so the skeleton stays up long enough to observe. */
  wsSettingsWebhooksDelayListFetch(ms: number): Promise<void>;
  /** Row target URLs in render order. */
  wsSettingsWebhooksRowUrls(): Promise<string[]>;
  /** Open the row for a target URL; settles on the detail form. */
  wsSettingsWebhooksOpenRow(url: string): Promise<void>;
  /** Whether the row switch for a target URL is currently on. */
  wsSettingsWebhooksRowActiveOn(url: string): Promise<boolean>;
  /** Flip the row switch for a target URL; resolves once the write settles. */
  wsSettingsWebhooksRowToggleActive(url: string): Promise<void>;
  /** Click the list-level Add webhook control. */
  wsSettingsWebhooksAddClick(): Promise<void>;
  /** Click the empty-state creation action. */
  wsSettingsWebhooksEmptyAddClick(): Promise<void>;
  /** Whether the webhook dialog (form or reveal step) is open. */
  wsSettingsWebhookDialogOpen(): Promise<boolean>;
  /** The dialog form title (Create webhook), or null when not on the form step. */
  wsSettingsWebhookDialogTitle(): Promise<string | null>;
  /** Fill the dialog/detail target URL field. */
  wsSettingsWebhookUrlFill(url: string): Promise<void>;
  /** Current target URL field value. */
  wsSettingsWebhookUrlValue(): Promise<string>;
  /** The inline URL error, or null when none shows. */
  wsSettingsWebhookUrlError(): Promise<string | null>;
  /** Which event mode radio is checked, or null when the form is missing. */
  wsSettingsWebhookEventMode(): Promise<"all" | "individual" | null>;
  /** Pick an event mode radio. */
  wsSettingsWebhookPickEventMode(mode: "all" | "individual"): Promise<void>;
  /** Individual event options with their checked state (empty when hidden). */
  wsSettingsWebhookIndividualOptions(): Promise<{ label: string; checked: boolean }[]>;
  /** Toggle one individual event option by label. */
  wsSettingsWebhookToggleIndividual(label: string): Promise<void>;
  /** Click Create; resolves once the create call settles. */
  wsSettingsWebhookSubmit(): Promise<void>;
  /** Click Create; resolves with the auto-downloaded backup file. */
  wsSettingsWebhookSubmitAndCaptureDownload(): Promise<LocalFileDownload>;
  /** Click Cancel in the dialog. */
  wsSettingsWebhookCancel(): Promise<void>;
  /** Press Escape with the dialog open. */
  wsSettingsWebhookPressEscape(): Promise<void>;
  /** Whether the secret reveal step (Key created) is visible. */
  wsSettingsWebhookRevealVisible(): Promise<boolean>;
  /** The reveal explainer paragraph, or null when the step is missing. */
  wsSettingsWebhookRevealText(): Promise<string | null>;
  /** Whether the secret currently renders in the clear (vs masked dots). */
  wsSettingsWebhookSecretShown(): Promise<boolean>;
  /** The in-clear secret, or null when masked or absent. */
  wsSettingsWebhookSecretValue(): Promise<string | null>;
  /** Whether the view/copy secret actions are present. */
  wsSettingsWebhookSecretActionsVisible(): Promise<boolean>;
  /** Click the view-secret action. */
  wsSettingsWebhookSecretToggleView(): Promise<void>;
  /** Click the copy-secret action. */
  wsSettingsWebhookSecretCopy(): Promise<void>;
  /** Click Close on the reveal step. */
  wsSettingsWebhookRevealClose(): Promise<void>;
  /** Whether the detail page shows a not-authorized state (view or paragraph). */
  wsSettingsWebhookDetailNotAuthorized(): Promise<boolean>;
  /** Which denial the detail page shows: the shared view, the page paragraph, or none. */
  wsSettingsWebhookDetailDenialKind(): Promise<"view" | "paragraph" | null>;
  /** Whether the detail spinner is currently showing. */
  wsSettingsWebhookDetailLoaderVisible(): Promise<boolean>;
  /** Hold detail GETs so the spinner stays up long enough to observe. */
  wsSettingsWebhookDetailDelayFetch(ms: number): Promise<void>;
  /** Whether the detail active switch is currently on. */
  wsSettingsWebhookDetailActiveOn(): Promise<boolean>;
  /** Flip the detail active switch. */
  wsSettingsWebhookDetailToggleActive(): Promise<void>;
  /** Click Update; resolves once the update call settles. */
  wsSettingsWebhookDetailUpdate(): Promise<void>;
  /** Current Update button label, or null when the button is missing. */
  wsSettingsWebhookDetailUpdateLabel(): Promise<string | null>;
  /** Hold update PATCHes so the updating state stays up long enough to observe. */
  wsSettingsWebhookDetailDelayUpdate(ms: number): Promise<void>;
  /** Whether the detail secret-key block is visible. */
  wsSettingsWebhookSecretBlockVisible(): Promise<boolean>;
  /** Click Re-generate key; resolves once the regenerate call settles. */
  wsSettingsWebhookRegenerate(): Promise<void>;
  /** Current Re-generate button label, or null when the button is missing. */
  wsSettingsWebhookRegenerateLabel(): Promise<string | null>;
  /** Hold regenerate POSTs so the re-generating state stays up to observe. */
  wsSettingsWebhookDelayRegenerate(ms: number): Promise<void>;
  /**
   * Click Re-generate key and capture the backup file download; null when
   * no download fires.
   */
  wsSettingsWebhookRegenerateAndCaptureDownload(): Promise<LocalFileDownload | null>;
  /** Whether the Danger zone disclosure is visible. */
  wsSettingsWebhookDeleteSectionVisible(): Promise<boolean>;
  /** Expand the Danger zone disclosure. */
  wsSettingsWebhookDeleteSectionExpand(): Promise<void>;
  /** Whether the delete panel (with its Delete webhook button) is showing. */
  wsSettingsWebhookDeletePanelVisible(): Promise<boolean>;
  /** Click the Delete webhook button. */
  wsSettingsWebhookDeleteOpen(): Promise<void>;
  /** Whether the delete confirmation dialog is open. */
  wsSettingsWebhookDeleteDialogOpen(): Promise<boolean>;
  /** The delete confirmation body text, or null when the dialog is missing. */
  wsSettingsWebhookDeleteDialogText(): Promise<string | null>;
  /** Confirm deletion; resolves once the delete settles on the list. */
  wsSettingsWebhookDeleteConfirm(): Promise<void>;
  /** Dismiss the delete confirmation dialog. */
  wsSettingsWebhookDeleteCancel(): Promise<void>;
  // --- Profile-settings assistant/upkeep/integrations oracle (NEWFRONT-428,
  // --- PROF-021..025, PROF-027, PROF-028). Tab navigation plus reads and
  // --- actions for the AI-assistant tab (BYO LLM provider, BYO STT
  // --- dictation, MCP tool servers), the upkeep tab (master pause,
  // --- per-job toggles, empty-scheduled explanation), and the integrations
  // --- tab (per-workspace GitHub App panel, install-callback markers,
  // --- manual refresh); key-press/shortcut-silence, read/socket watchers,
  // --- and drag/import absence reads for the cross-cutting rows.
  // --- Appended; existing methods above are untouched per the shared
  // --- driver contract.
  /** Open a profile-settings tab; settles once its content renders. */
  settingsProfileOpenTab(
    tab: "ai-assistant" | "auto-project-management" | "integrations",
    cookies: ParityBrowserCookie[]
  ): Promise<void>;
  /** Open a profile-settings tab with a raw query string (callback markers). */
  settingsProfileOpenTabWithQuery(
    tab: "ai-assistant" | "auto-project-management" | "integrations",
    query: string,
    cookies: ParityBrowserCookie[]
  ): Promise<void>;
  /** Current pathname. */
  settingsProfileCurrentPath(): Promise<string>;
  /** Current query string without the leading `?`. */
  settingsProfileCurrentQuery(): Promise<string>;
  /** Newest settings toast (title + message), or null when none shows. */
  settingsProfileLastToast(): Promise<{ title: string; message: string } | null>;
  /** LLM provider form facts. */
  settingsProfileAssistantFacts(): Promise<{
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
  }>;
  /** Pick the LLM provider kind. */
  settingsProfileAssistantSetProvider(provider: "openai_compatible" | "anthropic"): Promise<void>;
  /** Fill one LLM form field. */
  settingsProfileAssistantFill(field: "baseUrl" | "model" | "key", value: string): Promise<void>;
  /** Click Save; resolves once the write settles. */
  settingsProfileAssistantSave(): Promise<void>;
  /** Click Test connection; resolves once the probe settles. */
  settingsProfileAssistantTest(): Promise<void>;
  /** Click Remove; resolves once the delete settles. */
  settingsProfileAssistantRemove(): Promise<void>;
  /** Fail LLM/STT config PUT writes with HTTP 500 until cleared (save-failure hook). */
  settingsProfileAssistantFailWrites(): Promise<void>;
  /** Remove the assistant interception installed above. */
  settingsProfileAssistantClearRoutes(): Promise<void>;
  /** Voice dictation section facts. */
  settingsProfileDictationFacts(): Promise<{
    sectionVisible: boolean;
    baseUrl: string;
    model: string;
    keyValue: string;
    keyPlaceholder: string;
    saveDisabled: boolean;
    testDisabled: boolean;
    removeVisible: boolean;
    lastVerified: string | null;
  }>;
  /** Fill one dictation form field. */
  settingsProfileDictationFill(field: "baseUrl" | "model" | "key", value: string): Promise<void>;
  /** Click Save; resolves once the write settles. */
  settingsProfileDictationSave(): Promise<void>;
  /** Click Test connection; resolves once the probe settles. */
  settingsProfileDictationTest(): Promise<void>;
  /** Click Remove; resolves once the delete settles. */
  settingsProfileDictationRemove(): Promise<void>;
  /** Make the next speech-config GET report the given kill-switch state. */
  settingsProfileStubDictationEnabled(enabled: boolean): Promise<void>;
  /** Make speech save/test/transcribe answer the kill-switch not-found. */
  settingsProfileStubDictationWritesDisabled(): Promise<void>;
  /** Remove the speech-config read/write stubs, leaving the real API. */
  settingsProfileClearDictationStubs(): Promise<void>;
  /** Tool-server rows in display order. */
  settingsProfileToolServerRows(): Promise<
    { name: string; url: string; disabled: boolean; locked: boolean; prefix: string | null }[]
  >;
  /** Whether the add-server control is currently disabled. */
  settingsProfileToolServerAddDisabled(): Promise<boolean>;
  /** Fill the add form and submit; resolves once the write settles. */
  settingsProfileToolServerAdd(name: string, url: string, authHeader?: string): Promise<void>;
  /** Flip one server's enabled switch; resolves once the write settles. */
  settingsProfileToolServerToggle(name: string): Promise<void>;
  /** Remove one server; resolves once the delete settles. */
  settingsProfileToolServerRemove(name: string): Promise<void>;
  /** Upkeep tab facts: empty explanation or master + job switches. */
  settingsProfileUpkeepFacts(): Promise<{
    emptyExplanation: string | null;
    masterPaused: boolean | null;
    jobs: { name: string; interval: string; description: string; enabled: boolean; toggleDisabled: boolean }[];
  }>;
  /** Set the master pause switch; resolves once the write settles. */
  settingsProfileUpkeepToggleMaster(paused: boolean): Promise<void>;
  /** Set one job's switch; resolves once the write settles. */
  settingsProfileUpkeepToggleJob(name: string, enabled: boolean): Promise<void>;
  /** Fail upkeep PATCH writes with HTTP 500 until cleared (rollback hook). */
  settingsProfileUpkeepFailWrites(): Promise<void>;
  /** Remove the upkeep interception installed above. */
  settingsProfileUpkeepClearRoutes(): Promise<void>;
  /** Hold upkeep PATCH writes `ms` before passing them through (optimistic hook). */
  settingsProfileUpkeepDelayWrites(ms: number): Promise<void>;
  /** Integrations tab facts: picker, panel, banners. */
  settingsProfileIntegrationsFacts(): Promise<{
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
  }>;
  /** Pick a workspace in the integrations picker; resolves once the panel swaps. */
  settingsProfileIntegrationsSelect(slug: string): Promise<void>;
  /** Click the manual Refresh control; resolves once the check settles. */
  settingsProfileIntegrationsRefresh(): Promise<void>;
  /** Fail refresh POSTs with the observed 502 until cleared (check-failure hook). */
  settingsProfileIntegrationsFailRefresh(): Promise<void>;
  /** Remove the integrations interception installed above. */
  settingsProfileIntegrationsClearRoutes(): Promise<void>;
  /** Press keys with the page body focused (shortcut-silence check). */
  settingsProfilePressKeys(keys: string[]): Promise<void>;
  /** Whether any dialog is currently open. */
  settingsProfileDialogOpen(): Promise<boolean>;
  /** Start counting profile-settings API reads. */
  settingsProfileWatchReads(): Promise<void>;
  /** Reads observed since the watcher armed. */
  settingsProfileReadCount(): Promise<number>;
  /** Start counting websocket connections. */
  settingsProfileWatchSockets(): Promise<void>;
  /** Sockets observed since the watcher armed. */
  settingsProfileSocketCount(): Promise<number>;
  /** Draggable elements inside the settings content. */
  settingsProfileDraggableCount(): Promise<number>;
  /** File inputs inside the settings content. */
  settingsProfileFileInputCount(): Promise<number>;
  /** Visible import/export mentions inside the settings content. */
  settingsProfileImportExportMentions(): Promise<string[]>;
  /** Member/user-picker controls inside the settings content (expected: none). */
  settingsProfileMemberControlCount(): Promise<number>;
  // ---------------------------------------------------------------------------
  // Member profile pages (NEWFRONT-425, PROF-001..008).
  // The profile overview (breadcrumb, tab strip, count cards, workload and
  // distribution charts, recent activity), the assigned/created/subscribed
  // issue tabs, the activity tab, and the identity sidebar. Guests see the
  // summary tab only and meet an inline denial on the issue-tab addresses
  // (the activity address renders its feed view without a denial); the
  // sidebar still carries cover art and the edit pencil still opens the
  // settings dialog in this checkout (removal/navigation are main#523,
  // main#574 and main#573 targets pinned as Gaps in the rows). Issue-tab
  // layout/display/peek behavior reuses the shared layouts, display and
  // peek methods; only the two-layout switcher contract is profile-side.
  // Appended; existing entries above are untouched per the shared contract.
  // ---------------------------------------------------------------------------

  /** Open one member's profile overview; settles on the breadcrumb plus the overview. */
  profileOpen(workspaceSlug: string, userId: string, cookies: ParityBrowserCookie[]): Promise<void>;
  /** Breadcrumb label ("Your work" on your own profile, "<name> Work" on another's). */
  profileBreadcrumbLabel(): Promise<string>;
  /** Desktop tab strip labels in display order. */
  profileVisibleTabs(): Promise<string[]>;
  /** Href of one desktop tab, or null when the tab is absent. */
  profileTabHref(tab: string): Promise<string | null>;
  /** Open one profile tab directly; settles on tab content or the inline denial. */
  profileOpenTab(workspaceSlug: string, userId: string, tab: string, cookies: ParityBrowserCookie[]): Promise<void>;
  /** True while the inline no-permission notice renders instead of tab content. */
  profileAccessDenied(): Promise<boolean>;
  /** Mobile tab picker label (empty on the summary tab), or null when the picker is absent. */
  profileMobileTabPickerLabel(): Promise<string | null>;
  /** Mobile tab picker option labels, opening and closing the picker. */
  profileMobileTabPickerOptions(): Promise<string[]>;
  /** Overview count cards: title, rendered value, and tab href, in display order. */
  profileOverviewCards(): Promise<Array<{ title: string; value: string; href: string | null }>>;
  /** Open one overview count card's tab; resolves once the tab settles. */
  profileOverviewOpenCard(route: string): Promise<void>;
  /** True while the overview shows stats placeholder blocks instead of cards. */
  profileOverviewStatsLoading(): Promise<boolean>;
  /** Stall the user-stats read so placeholders stay up (test hook). */
  profileOverviewStallStats(): Promise<void>;
  /** Release a stalled user-stats read (test hook). */
  profileOverviewReleaseStats(): Promise<void>;
  /** Workload group cards: label plus rendered count, in display order. */
  profileWorkloadGroups(): Promise<Array<{ label: string; count: string }>>;
  /** Priority chart category labels in display order. */
  profilePriorityChartCategories(): Promise<string[]>;
  /** State chart legend rows: label plus rendered count, in display order. */
  profileStateChartLegend(): Promise<Array<{ label: string; count: string }>>;
  /** Number of "no work item assigned yet" chart empty states on the overview. */
  profileChartsEmptyCount(): Promise<number>;
  /** Recent-activity entry texts on the overview, in display order. */
  profileOverviewActivityEntries(): Promise<string[]>;
  /** Work-item hrefs behind the overview recent-activity entries, in display order. */
  profileOverviewActivityEntryHrefs(): Promise<string[]>;
  /** True while the overview recent-activity section shows its empty state. */
  profileOverviewActivityEmpty(): Promise<boolean>;
  /** Layout keys the profile issue-tab switcher offers, in display order. */
  profileIssuesOfferedLayouts(): Promise<Array<"list" | "kanban">>;
  /** Layout keys the mobile Layout menu offers, in display order. */
  profileMobileOfferedLayouts(): Promise<Array<"list" | "kanban">>;
  /** Switch layouts through the mobile Layout menu; resolves once the layout renders. */
  profileMobileSwitchTo(layout: "list" | "kanban"): Promise<void>;
  /** True while the mobile kanban board renders (drop-target marker). */
  profileMobileKanbanVisible(): Promise<boolean>;
  /** Active profile layout on mobile, derived from the rendered container. */
  profileMobileActiveLayout(): Promise<"list" | "kanban">;
  /** Reload the current profile tab on mobile; resolves once a layout container renders. */
  profileMobileReloadIssues(): Promise<void>;
  /** True while a profile tab renders neither content nor denial (unknown tab id). */
  profileTabRendersNothing(): Promise<boolean>;
  /** Activity-tab entry texts in display order. */
  profileActivityEntries(): Promise<string[]>;
  /** Work-item hrefs behind the activity-tab entries, in display order. */
  profileActivityEntryHrefs(): Promise<string[]>;
  /** True while the activity tab shows its load-more control. */
  profileActivityLoadMoreVisible(): Promise<boolean>;
  /** Click load-more; resolves once further entries render. */
  profileActivityLoadMore(): Promise<void>;
  /** True while the identity sidebar panel is on-screen (expanded). */
  profileSidebarVisible(): Promise<boolean>;
  /** True while the sidebar shows cover art (current behavior; removal is the main#523/#574 target). */
  profileSidebarCoverVisible(): Promise<boolean>;
  /** Sidebar avatar: photo URL or initial fallback. */
  profileSidebarAvatar(): Promise<{ kind: "image"; src: string } | { kind: "initial"; initial: string }>;
  /** Sidebar full name plus parenthesized display name. */
  profileSidebarName(): Promise<{ full: string; display: string }>;
  /** Sidebar "Joined on" value. */
  profileSidebarJoinDate(): Promise<string>;
  /** Sidebar live-clock text. */
  profileSidebarClock(): Promise<string>;
  /** Sidebar per-project rows: name plus completion tint, in display order. */
  profileSidebarProjectRows(): Promise<Array<{ name: string; completion: string | null }>>;
  /** Expand one sidebar project row; resolves once its counts render. */
  profileSidebarExpandProject(name: string): Promise<void>;
  /** One expanded sidebar project row's created/assigned/due/completed counts. */
  profileSidebarProjectCounts(name: string): Promise<{
    created: string;
    assigned: string;
    due: string;
    completed: string;
  }>;
  /** True while the own-profile edit pencil renders in the sidebar. */
  profileSidebarEditPencilVisible(): Promise<boolean>;
  /** Click the own-profile edit pencil; resolves once the settings dialog opens. */
  profileSidebarClickEditPencil(): Promise<void>;
  /** Toggle the identity sidebar through the header control (mobile); resolves once it settles. */
  profileSidebarToggle(): Promise<void>;

  // --- Space navbar & chrome (NEWFRONT-448, SPC-010..019 + SPC-065). ---
  // Reads below were observed on the running old space app; icon-only
  // controls are reached structurally (neighbor-anchored), never by hooks.

  /** Navbar project identity as rendered: name text plus logo-mark kind (SPC-010). */
  spaceChromeNavbarIdentity(): Promise<SpaceChromeNavbarIdentity>;
  /** Navbar identity when the publish payload carries no project details (SPC-010 fallback). */
  spaceChromeNavbarWithoutProjectDetails(anchor: string): Promise<SpaceChromeNavbarIdentity>;
  /** Navbar identity with an injected icon/emoji logo mark (SPC-010 mark branches). */
  spaceChromeNavbarWithLogo(anchor: string, kind: "icon" | "emoji"): Promise<SpaceChromeNavbarIdentity>;
  /** Current board theme as the document declares it (SPC-011). */
  spaceChromeTheme(): Promise<"light" | "dark">;
  /** Click the navbar theme switch; resolves once the theme settles (SPC-011). */
  spaceChromeToggleTheme(): Promise<void>;
  /** Computed body background color, proving the whole board flipped (SPC-011). */
  spaceChromeBodyBackground(): Promise<string>;
  /** Forget the stored theme choice so the next load follows the OS again (SPC-011). */
  spaceChromeClearStoredTheme(): Promise<void>;
  /** Emulate an OS color scheme for the next load's default (SPC-011). */
  spaceChromeEmulateColorScheme(scheme: "light" | "dark"): Promise<void>;
  /** Fixed product badge facts, or null when no badge renders (SPC-012). */
  spaceChromeBadgeFacts(): Promise<SpaceChromeBadgeFacts | null>;
  /** Click the badge; resolves with the new tab's URL (SPC-012). */
  spaceChromeBadgeNewTabUrl(): Promise<string>;
  /** Anonymous sign-in link target carrying the board position (SPC-013). */
  spaceChromeAnonymousSignInHref(): Promise<string | null>;
  /** Signed-in account menu identity (opens the menu; SPC-014). */
  spaceChromeAccountMenuIdentity(): Promise<SpaceChromeAccountIdentity>;
  /** Sign-out form target: action plus preserved return address (SPC-014). */
  spaceChromeSignOutForm(): Promise<SpaceChromeSignOutForm>;
  /** Submit sign-out; resolves with the redirect target and landed path (SPC-014). */
  spaceChromeSignOut(): Promise<SpaceChromeSignOutResult>;
  /** Open a board inside an embedding frame harness (SPC-015). */
  spaceChromeOpenBoardFramed(anchor: string, query?: string): Promise<void>;
  /** Framed-board facts: readability plus hidden controls (SPC-015). */
  spaceChromeFramedFacts(): Promise<SpaceChromeFramedFacts>;
  /** Open a board with an issue peek overlay raised (SPC-015/017/065 vehicle). */
  spaceChromeOpenIssuePeek(anchor: string, issueId: string, query?: string): Promise<void>;
  /** Count of visible emoji-reaction add controls (SPC-015 surface). */
  spaceChromeEmojiControlCount(): Promise<number>;
  /** Loading-spinner probe for one boot stage (SPC-016). */
  spaceChromeSpinnerDuring(stage: "instance" | "settings" | "issues", anchor: string): Promise<SpaceChromeSpinnerProbe>;
  /** Navigation-progress probe across a slow legacy-link transition (SPC-017). */
  spaceChromeProgressDuringSlowTransition(workspaceSlug: string, projectId: string): Promise<SpaceChromeProgressProbe>;
  /** Copy the peek link; resolves with the notice plus clipboard text (SPC-017). */
  spaceChromeCopyIssueLinkToast(anchor: string, issueId: string): Promise<SpaceChromeToastFacts>;
  /** Attempt a comment post with the API blocked; resolves with the error notice (SPC-017). */
  spaceChromeCommentFailureToast(anchor: string, issueId: string, bodyText: string): Promise<SpaceChromeToastFacts>;
  /** Instance-failure screen facts with the backend unreachable (SPC-018). */
  spaceChromeInstanceFailureFacts(anchor: string): Promise<SpaceChromeFailureFacts>;
  /** Restore the backend and retry from the failure screen; resolves on the board (SPC-018). */
  spaceChromeRetryInstanceLoad(): Promise<void>;
  /** Crash-screen facts after a render fault (SPC-019). */
  spaceChromeCrashFacts(anchor: string): Promise<SpaceChromeCrashFacts>;
  /** Refresh from the crash screen; resolves once the board renders again (SPC-019). */
  spaceChromeRefreshAfterCrash(): Promise<void>;
  /** Locale-observable board facts: layered strings, keys, dates, html lang (SPC-065). */
  spaceLocaleBoardFacts(anchor: string, issueId: string): Promise<SpaceLocaleBoardFacts>;
  /** Landing-page chrome strings for locale comparison (SPC-065). */
  spaceLocaleLandingFacts(): Promise<SpaceLocaleLandingFacts>;
  /** Store a locale choice (or clear it); applies on the next load (SPC-065). */
  spaceLocaleSetStoredLocale(locale: string | null): Promise<void>;
  // ---------------------------------------------------------------------------
  // Page-export dialog oracle reads (NEWFRONT-383, EXP-016..021 + 023/024).
  // The dialog offers a format row (document vs plain text), a scope row
  // (full fidelity vs image-free), and a page-size row that renders only
  // for the document format; confirming saves a locally rendered file
  // through the browser download event. Existing pagesHeaderExport* methods
  // (NEWFRONT-318) drive the dialog; these reads cover what they cannot:
  // the offered options, the full download bytes, fault injection, focus
  // facts, and edition-gate markers. Appended; existing entries above are
  // untouched per the shared contract.
  // ---------------------------------------------------------------------------

  /** Dialog heading text, or null when the page-export dialog is closed. */
  pageExportHeading(): Promise<string | null>;
  /** Offered option labels of one dialog row in display order ([] when the row renders no button). */
  pageExportOfferedOptions(row: PageExportRow): Promise<string[]>;
  /** Confirm the export; resolves with the downloaded file's full bytes as latin1 text. */
  pageExportConfirmFull(): Promise<PageExportFile>;
  /** Sabotage the in-page save primitive so the next confirm fails (test hook; reload to clear). */
  pageExportFailSave(): Promise<void>;
  /** Focused control facts (label plus visible-indicator state), or null when nothing focuses. */
  pageExportFocusedFacts(): Promise<PageExportFocusFacts | null>;
  /** Whether edition/desktop gate markers render on the current export surface. */
  pageExportGateMarkersPresent(): Promise<boolean>;

  // --- Personal access tokens oracle (NEWFRONT-413, WSET-025..028).
  // --- Observed on the running old app: the token page lives under
  // --- profile settings (/settings/profile/api-tokens, Developer group);
  // --- it lists the signed-in user's tokens with an Add control, a
  // --- loader branch, and an illustrated empty state with its own
  // --- creation action. The create dialog validates the title inline,
  // --- offers expiry presets plus a custom date floored at tomorrow and
  // --- a never-expires toggle with a live preview, then reveals the raw
  // --- token once with copy support while a CSV backup downloads. Rows
  // --- carry an active/expired marker, an expiry line, and a hover-only
  // --- delete behind a confirmation dialog. Appended; existing methods
  // --- above are untouched per the shared driver contract.
  /** Open the token page as the cookie owner; settles on the page heading. */
  wsSettingsApiTokenOpenTokens(cookies: ParityBrowserCookie[]): Promise<void>;
  /** Open the general tab, then reach the token page via its sidebar entry. */
  wsSettingsApiTokenOpenViaSidebar(cookies: ParityBrowserCookie[]): Promise<void>;
  /** The token page heading, or null when the page never settled. */
  wsSettingsApiTokenHeading(): Promise<string | null>;
  /** Whether the header Add control is currently visible. */
  wsSettingsApiTokenAddVisible(): Promise<boolean>;
  /** Hold the token list GET so the loader stays up long enough to observe. */
  wsSettingsApiTokenLoaderSeen(): Promise<boolean>;
  /** Whether the illustrated empty state with its creation action shows. */
  wsSettingsApiTokenEmptyVisible(): Promise<boolean>;
  /** Click the empty state's creation action; resolves on the open dialog. */
  wsSettingsApiTokenEmptyCreate(): Promise<void>;
  /** Click the header Add control; resolves on the open dialog. */
  wsSettingsApiTokenOpenCreate(): Promise<void>;
  /** Token rows in render order. */
  wsSettingsApiTokenRows(): Promise<WsSettingsApiTokenRow[]>;
  /** Full visible text of the token page content (for absence assertions). */
  wsSettingsApiTokenPageText(): Promise<string>;
  /** Whether the create dialog is currently open. */
  wsSettingsApiTokenDialogOpen(): Promise<boolean>;
  /** Fill the create dialog's title field. */
  wsSettingsApiTokenFillTitle(title: string): Promise<void>;
  /** Fill the create dialog's description field. */
  wsSettingsApiTokenFillDescription(text: string): Promise<void>;
  /** The inline title error, or null when none shows. */
  wsSettingsApiTokenTitleError(): Promise<string | null>;
  /** Expiry preset option labels in render order (closes the menu after). */
  wsSettingsApiTokenPresetOptions(): Promise<string[]>;
  /** Pick an expiry preset (or Custom) from the dialog's expiry menu. */
  wsSettingsApiTokenPickPreset(label: string): Promise<void>;
  /** Current text of the dialog's expiry trigger. */
  wsSettingsApiTokenExpiryTriggerText(): Promise<string>;
  /** Whether the dialog's expiry trigger is currently disabled. */
  wsSettingsApiTokenExpiryTriggerDisabled(): Promise<boolean>;
  /** The dialog's live expiry preview, or null when none shows. */
  wsSettingsApiTokenExpiryPreview(): Promise<string | null>;
  /** Flip the dialog's never-expires toggle. */
  wsSettingsApiTokenToggleNeverExpires(): Promise<void>;
  /** Pick Custom, move one month forward, and pick that month's given day. */
  wsSettingsApiTokenPickCustomDayNextMonth(day: string): Promise<void>;
  /** Whether today reads disabled and tomorrow enabled in the custom picker. */
  wsSettingsApiTokenCustomFloor(): Promise<{ todayDisabled: boolean; tomorrowEnabled: boolean }>;
  /** Submit the create dialog; resolves once the click settles. */
  wsSettingsApiTokenSubmit(): Promise<void>;
  /** Submit and capture the auto-downloaded backup file. */
  wsSettingsApiTokenSubmitAndCaptureBackup(): Promise<WsSettingsApiTokenBackup>;
  /** Cancel the create dialog; resolves once it closes. */
  wsSettingsApiTokenCancelCreate(): Promise<void>;
  /** The reveal step's heading, or null when the reveal is not showing. */
  wsSettingsApiTokenRevealHeading(): Promise<string | null>;
  /** The raw token the reveal step shows once, or null when absent. */
  wsSettingsApiTokenRevealValue(): Promise<string | null>;
  /** The reveal step's expiry line, or null when absent. */
  wsSettingsApiTokenRevealExpiryLine(): Promise<string | null>;
  /** Click the reveal copy control; resolves with the clipboard text. */
  wsSettingsApiTokenRevealCopy(): Promise<string>;
  /** Close the reveal step; resolves once the dialog closes. */
  wsSettingsApiTokenRevealClose(): Promise<void>;
  /** Open the delete confirmation for a row; resolves with the dialog text. */
  wsSettingsApiTokenDeleteDialogText(label: string): Promise<string>;
  /** Cancel the open delete confirmation. */
  wsSettingsApiTokenDeleteCancel(): Promise<void>;
  /** Delete a row through its confirmation; resolves once the row is gone. */
  wsSettingsApiTokenDeleteConfirm(label: string): Promise<void>;
  // --- Project settings General tab (NEWFRONT-389, PSET-005–012). Observed
  // --- on the running old app: one long admin form (cover banner with
  // --- overlapping emblem, name, description, repository row with its own
  // --- bind button, base branch, execution-target picker, ticking switch
  // --- with budget plus no-cap and cadence controls, identifier, network
  // --- and timezone pickers, submit, creation stamp) with a skeleton while
  // --- the details load; non-admins get the same tab dimmed and inert with
  // --- no danger zone; archive and delete live in the danger zone below the
  // --- form, restore mode opens from the project card. Appended; existing
  // --- methods above are untouched per the shared driver contract.
  /** Open a project's settings General tab; resolves once it settles. */
  settingsGeneralOpen(workspaceSlug: string, projectId: string): Promise<void>;
  /** Whether the general form (as opposed to skeleton or restriction) shows. */
  settingsGeneralFormVisible(): Promise<boolean>;
  /** Whether the loading skeleton shows. */
  settingsGeneralSkeletonVisible(): Promise<boolean>;
  /** Whether the danger zone (archive/delete rows) renders below the form. */
  settingsGeneralDangerVisible(): Promise<boolean>;
  /** Whether the tab renders dimmed (the non-admin inert presentation). */
  settingsGeneralFormDimmed(): Promise<boolean>;
  /** Current value of a general-tab text field. */
  settingsGeneralFieldValue(field: SettingsGeneralField): Promise<string>;
  /** Whether a general-tab field is currently disabled. */
  settingsGeneralFieldDisabled(field: SettingsGeneralField): Promise<boolean>;
  /** Inline field error text, or "" when the field shows none. */
  settingsGeneralFieldError(field: SettingsGeneralField): Promise<string>;
  /** Fill a general-tab text field (replacing its content). */
  settingsGeneralFill(field: SettingsGeneralField, value: string): Promise<void>;
  /** Whether the cover banner renders on the tab. */
  settingsGeneralCoverVisible(): Promise<boolean>;
  /** Banner headline text (project name plus identifier line). */
  settingsGeneralHeaderText(): Promise<string>;
  /** Visibility network label as rendered. */
  settingsGeneralNetworkText(): Promise<string>;
  /** Timezone value as rendered. */
  settingsGeneralTimezoneText(): Promise<string>;
  /** Creation-timestamp line as rendered. */
  settingsGeneralCreatedText(): Promise<string>;
  /** Selected execution-target value. */
  settingsGeneralExecutorValue(): Promise<string>;
  /** Whether an execution-target option is disabled. */
  settingsGeneralExecutorOptionDisabled(value: string): Promise<boolean>;
  /** Pick an execution target. */
  settingsGeneralSetExecutor(value: string): Promise<void>;
  /** Whether the automated-run master switch is on. */
  settingsGeneralTickingOn(): Promise<boolean>;
  /** Flip the automated-run master switch. */
  settingsGeneralSetTicking(on: boolean): Promise<void>;
  /** Run-budget input value as rendered ("" while no-cap shows its marker). */
  settingsGeneralBudgetValue(): Promise<string>;
  /** Fill the run-budget input. */
  settingsGeneralSetBudget(value: string): Promise<void>;
  /** Tick or untick the no-cap toggle. */
  settingsGeneralSetNoCap(on: boolean): Promise<void>;
  /** Cadence picker text as rendered (preset label or custom marker). */
  settingsGeneralCadenceText(): Promise<string>;
  /** Pick a cadence preset by its visible label. */
  settingsGeneralSetCadence(label: string): Promise<void>;
  /** Submit the form; resolves once the save settles (toast shown). */
  settingsGeneralSave(): Promise<void>;
  /** Repository bind control state: disabled bound pill vs clickable bind. */
  settingsGeneralBindState(): Promise<"bound" | "bind">;
  /** Click the repository Bind button; resolves once it settles. */
  settingsGeneralBind(): Promise<void>;
  /** Open the emblem picker. */
  settingsGeneralEmblemOpen(): Promise<void>;
  /** Whether the emblem picker is currently open. */
  settingsGeneralEmblemPickerVisible(): Promise<boolean>;
  /** Pick the emoji `char` in the open emblem picker. */
  settingsGeneralEmblemPick(char: string): Promise<void>;
  /** Open the cover picker ("Change cover"). */
  settingsGeneralCoverOpen(): Promise<void>;
  /** Cover picker tab titles in display order. */
  settingsGeneralCoverTabNames(): Promise<string[]>;
  /** How many bundled gallery images the cover picker offers. */
  settingsGeneralCoverGalleryCount(): Promise<number>;
  /** Pick a bundled gallery image by index; resolves once the picker closes. */
  settingsGeneralCoverPickGallery(index: number): Promise<void>;
  /** Upload a cover file through the picker's upload tab. */
  settingsGeneralCoverUpload(file: { name: string; mime: string; bytes: string }): Promise<void>;
  /** Open the delete confirmation from the danger zone (phrase fill and submit reuse the shared delete-dialog methods). */
  settingsGeneralOpenDelete(): Promise<void>;
  /** Fill the project-name confirmation inside the delete dialog (scoped: the form behind shares the placeholder). */
  settingsGeneralFillDeleteName(text: string): Promise<void>;
  /** Archive/restore confirm dialog text (heading plus body), or "" while closed. */
  settingsGeneralConfirmModalText(): Promise<string>;
  /** Abort project-scoped fetches so the tab stays on its skeleton; pair with the release below. */
  settingsGeneralStallDetails(): Promise<void>;
  /** Release a stall installed above; the tab proceeds to the form. */
  settingsGeneralReleaseStall(): Promise<void>;
  /** Fail the save-time cover upload so the abort path can be observed; pair with the release below. */
  settingsGeneralStallCoverUpload(): Promise<void>;
  /** Release a cover-upload stall installed above. */
  settingsGeneralReleaseCoverUpload(): Promise<void>;
  // Archive/restore entries, card reads, toasts and the delete phrase
  // fill reuse the sibling areas' methods (openArchiveProjectDialog,
  // clickCardRestore, confirmArchive/confirmRestore,
  // visibleProjectCardNames, isToastVisible,
  // fillDeleteConfirmPhrase/isDeleteSubmitDisabled/submitDelete): same
  // controls, no fork. The confirm-modal text and the delete name fill
  // are this block's own: the shared body reader sees neither the
  // heading nor a closed modal, and the shared name fill collides with
  // the form behind the dialog.

  // ---------------------------------------------------------------------------
  // Profile preferences + own activity (NEWFRONT-427, PROF-016..019).
  // The preferences tab (theme switcher with custom-color designer, language /
  // timezone / week-start selectors), cross-screen reads proving those choices
  // reshape other screens (home greeting clock, profile sidebar clock, issue
  // calendar day order, cycle "in your timezone" dates), and the settings
  // activity tab (paged own history with a themed empty state). The
  // notifications tab needs no methods here: the
  // PROF-018 scenario reuses the notificationsEmail* methods (NEWFRONT-201).
  // Appended; existing entries above are untouched per the shared contract.
  // ---------------------------------------------------------------------------

  /** Open the profile-settings preferences tab; resolves once the theme control settles. */
  settingsProfileOpenPreferences(): Promise<void>;
  /** Open the profile-settings activity tab; resolves once entries or the empty state settle. */
  settingsProfileOpenActivity(): Promise<void>;
  /** Current theme choice as the switcher shows it, or null while unset. */
  settingsProfileThemeValue(): Promise<SettingsProfileTheme | null>;
  /** Pick a theme; resolves once the save settles and the app re-applies it. */
  settingsProfileSetTheme(theme: SettingsProfileTheme): Promise<void>;
  /** Theme marker the document currently applies, or null when absent. */
  settingsProfileAppliedTheme(): Promise<string | null>;
  /** Whether custom-theme CSS variables are currently applied to the document. */
  settingsProfileCustomVarsApplied(): Promise<boolean>;
  /** Whether the custom-color designer renders (custom theme only). */
  settingsProfileCustomDesignerVisible(): Promise<boolean>;
  /** Current custom-designer field values. */
  settingsProfileCustomDesignerValues(): Promise<SettingsProfileCustomTheme>;
  /** Type a hex color into one designer field without saving; resolves with the field's live swatch color. */
  settingsProfilePreviewCustomColor(field: "primary" | "background", hex: string): Promise<string | null>;
  /** Pick the custom designer's light/dark palette without saving. */
  settingsProfileSetCustomPalette(dark: boolean): Promise<void>;
  /** Save the custom designer; resolves once the save settles and the app re-applies it. */
  settingsProfileSaveCustomTheme(): Promise<void>;
  /** Fail subsequent profile-save PATCH calls with `status` (test hook). */
  settingsProfileFailProfileSaves(status: number): Promise<void>;
  /** Release failed profile-save PATCH calls (test hook). */
  settingsProfileClearProfileSaveFailure(): Promise<void>;
  /** Current language, timezone and week-start labels as the selectors show them. */
  settingsProfileLocaleValues(): Promise<SettingsProfileLocale>;
  /** Pick a timezone; resolves once the save toast settles. */
  settingsProfileSetTimezone(timezone: string): Promise<void>;
  /** Pick an interface language; resolves once the save toast settles. */
  settingsProfileSetLanguage(language: string): Promise<void>;
  /** Pick a week-start day; resolves once the save toast settles. */
  settingsProfileSetWeekStart(day: string): Promise<void>;
  /** Open the workspace home; resolves once the greeting settles. */
  settingsProfileOpenHome(workspaceSlug: string): Promise<void>;
  /** Greeting clock line on the workspace home, or null when absent. */
  settingsProfileGreetingClockText(): Promise<string | null>;
  /** Open one member's profile page; resolves once the sidebar settles. */
  settingsProfileOpenMemberProfile(workspaceSlug: string, userId: string): Promise<void>;
  /** Profile-sidebar clock line (time plus timezone), or null when absent. */
  settingsProfileSidebarClockText(): Promise<string | null>;
  /** Calendar week-header day labels in display order (current page must show a calendar layout). */
  settingsProfileCalendarDayOrder(): Promise<string[]>;
  /** Rendered own-activity entries on the settings activity tab. */
  settingsProfileOwnActivityEntryCount(): Promise<number>;
  /** Whether the activity tab's load-more control renders. */
  settingsProfileOwnActivityLoadMoreVisible(): Promise<boolean>;
  /** Click load-more; resolves once the next page renders. */
  settingsProfileOwnActivityLoadMore(): Promise<void>;
  /** Empty-state illustration src on the activity tab, or null when entries render. */
  settingsProfileOwnActivityEmptyIllustration(): Promise<string | null>;
  /**
   * Stored cycle dates as the "In your timezone" tooltip renders them.
   * The current page must be a cycle detail page; hovers the analytics
   * sidebar's date range and reads the tooltip (each date "MMM dd, yyyy",
   * or null when the tooltip shows none).
   */
  settingsProfileCycleUserTimezoneDates(): Promise<{ start: string | null; end: string | null }>;
}

/** General-tab text field addressed by the settingsGeneral field methods (NEWFRONT-389). */
export type SettingsGeneralField = "name" | "description" | "identifier" | "repoUrl" | "baseBranch" | "budget";

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
  // ---------------------------------------------------------------------------
  // Analytics overview and work-items headline charts (NEWFRONT-376, ANL-005..009).
  // Headline total cards, the per-project radar chart plus its written
  // summary, the active-projects list, and the created-vs-resolved chart.
  // Appended; existing entries above are untouched per the shared contract.
  // ---------------------------------------------------------------------------

  /** Overview-tab headline cards in display order, settled (placeholders gone). */
  analyticsOverviewCards(): Promise<AnalyticsHeadlineCard[]>;
  /** Work-items-tab headline cards in display order, settled (placeholders gone). */
  analyticsWorkItemsCards(): Promise<AnalyticsHeadlineCard[]>;
  /** Placeholder blocks currently in the visible tab's headline grid (stall probes read this). */
  analyticsHeadlinePlaceholders(): Promise<number>;
  /** Stall one tab's headline-totals read so placeholders stay up (test hook). */
  analyticsTotalsStall(tab: "overview" | "work-items"): Promise<void>;
  /** Release a stalled headline-totals read (test hook). */
  analyticsTotalsRelease(): Promise<void>;
  /** Stall one charts read so the skeleton/loader stays up (test hook). */
  analyticsChartsStall(chart: "projects" | "work-items"): Promise<void>;
  /** Release a stalled charts read (test hook). */
  analyticsChartsRelease(): Promise<void>;
  /** Stall the project-stats read so list placeholders stay up (test hook). */
  analyticsProjectStatsStall(): Promise<void>;
  /** Release a stalled project-stats read (test hook). */
  analyticsProjectStatsRelease(): Promise<void>;
  /** Written per-dimension summary rows beside the radar chart, settled. */
  analyticsRadarSummary(): Promise<AnalyticsRadarRow[]>;
  /** Radar chart facts: angle-axis dimension labels plus whether the graphic renders. */
  analyticsRadarFacts(): Promise<AnalyticsRadarFacts>;
  /** Whether the radar section currently shows its loading skeleton. */
  analyticsRadarLoading(): Promise<boolean>;
  /** Active-projects rows in display order, settled (placeholders gone). */
  analyticsActiveProjects(): Promise<AnalyticsActiveProject[]>;
  /** Whether the active-projects list currently shows placeholder rows. */
  analyticsActiveProjectsLoading(): Promise<boolean>;
  /** Hover one active-project row; resolves with the tooltip text, or null when none appears. */
  analyticsActiveProjectTooltip(name: string): Promise<string | null>;
  /** Created-vs-resolved facts: legend, x-axis labels, series count, or the guidance text. */
  analyticsCreatedResolvedFacts(): Promise<AnalyticsCreatedResolvedFacts>;
  /** Whether the created-vs-resolved section currently shows its loader. */
  analyticsCreatedResolvedLoading(): Promise<boolean>;
  /** Guidance empty-panel text within a named analytics section, or null when it shows data. */
  analyticsSectionGuidance(title: string): Promise<string | null>;
  /** Scoped-dialog headline cards in display order, settled (placeholders gone). */
  analyticsDialogHeadlineCards(): Promise<AnalyticsHeadlineCard[]>;
  // ---------------------------------------------------------------------------
  // Analytics custom insights, data tables, and scoped dialog (NEWFRONT-377,
  // ANL-010..017). The custom-insights section pairs three picker buttons
  // (metric, x property, optional second split) with a bar chart plus a data
  // table; the insight table lists per-project stage counts; the scoped
  // dialog retitles the area per surface. Appended; existing entries above
  // are untouched per the shared contract.
  // ---------------------------------------------------------------------------

  /** Current labels of the three custom-insights picker buttons, in order. */
  analyticsCustomPickerLabels(): Promise<{ metric: string; xAxis: string; split: string }>;
  /** Option labels offered by one custom-insights picker (0 metric, 1 x, 2 split); opens and closes it. */
  analyticsCustomPickerOptions(index: 0 | 1 | 2): Promise<string[]>;
  /** Choose one option in a custom-insights picker; resolves once the re-query lands. */
  analyticsCustomPickerSelect(index: 0 | 1 | 2, label: string): Promise<void>;
  /** Custom bar chart facts: per-segment fills plus rounding, x labels, legend presence. */
  analyticsCustomChartFacts(): Promise<AnalyticsCustomChartFacts>;
  /** Tooltip segment rows after hovering one custom-chart bar (label plus value per segment). */
  analyticsCustomTooltipSegments(barIndex: number): Promise<AnalyticsCustomTooltipRow[]>;
  /** Body-row count of one analytics table (atomic; race-free settle polls). */
  analyticsExportRowCount(kind: AnalyticsExportTable): Promise<number>;
  /** Header cell texts of one analytics table in display order. */
  analyticsExportHeaders(kind: AnalyticsExportTable): Promise<string[]>;
  /** Row-count label beside one analytics table's search box ("N Projects"). */
  analyticsExportCountLabel(kind: AnalyticsExportTable): Promise<string>;
  /** Whether one analytics table's search box currently renders expanded. */
  analyticsExportSearchExpanded(kind: AnalyticsExportTable): Promise<boolean>;
  /** Close one analytics table's open search box through its clear control. */
  analyticsExportSearchClose(kind: AnalyticsExportTable): Promise<void>;
  /** Empty-view title when one analytics table shows its guidance branch, else null. */
  analyticsExportEmptyText(kind: AnalyticsExportTable): Promise<string | null>;
  /** Stall the insight-table stats read so its skeletons stay up (test hook). */
  analyticsInsightTableStall(): Promise<void>;
  /** Release a stalled insight-table stats read (test hook). */
  analyticsInsightTableRelease(): Promise<void>;
  /** Skeleton placeholder cells currently rendered in the insight-table area. */
  analyticsInsightSkeletonCells(): Promise<number>;
  /** Open the scoped dialog from a project's issues list, authenticated as the given session. */
  analyticsDialogOpenFromIssues(
    workspaceSlug: string,
    projectId: string,
    cookies: ParityBrowserCookie[]
  ): Promise<void>;
  /** Open the scoped dialog from a cycle detail page, authenticated as the given session. */
  analyticsDialogOpenFromCycle(
    workspaceSlug: string,
    projectId: string,
    cycleId: string,
    cookies: ParityBrowserCookie[]
  ): Promise<void>;
  /** Open the scoped dialog from a module detail page, authenticated as the given session. */
  analyticsDialogOpenFromModule(
    workspaceSlug: string,
    projectId: string,
    moduleId: string,
    cookies: ParityBrowserCookie[]
  ): Promise<void>;
  /** Analytics open-triggers on the current surface (guest-negative proof). */
  analyticsSurfaceTriggerCount(): Promise<number>;
  /** Scoped-dialog heading text naming the current scope. */
  analyticsDialogTitle(): Promise<string>;
  /** Whether the dialog's pre-content spinner mounted during the last dialog open. */
  analyticsDialogSpinnerSeen(): Promise<boolean>;
  /** Whether the dialog's wide-screen toggle renders visibly at this viewport. */
  analyticsDialogWideToggleVisible(): Promise<boolean>;
  /** Flip the dialog's wide-screen toggle; resolves once the width settles. */
  analyticsDialogToggleWide(): Promise<void>;
  /** Current dialog content width in CSS pixels. */
  analyticsDialogWidth(): Promise<number>;
  /** Body-row count of the dialog's breakdown table (atomic; race-free settle polls). */
  analyticsDialogRowCount(): Promise<number>;
  /** Header cell texts of the dialog's breakdown table in display order. */
  analyticsDialogTableHeaders(): Promise<string[]>;
  /** Body-row cell texts of the dialog's breakdown table in display order. */
  analyticsDialogTableCells(): Promise<string[][]>;
  /** First-column identity facts per dialog breakdown row (avatar, initial, or unassigned). */
  analyticsDialogAssigneeFacts(): Promise<AnalyticsDialogAssignee[]>;
  /** Click one dialog table's export action; resolves with the downloaded file. */
  analyticsDialogExportDownload(tableIndex: 0 | 1): Promise<LocalFileDownload>;
  // ---------------------------------------------------------------------------
  // Profile-settings shell, general/account tabs and security tab
  // (NEWFRONT-426, rows PROF-009..015, PROF-026). The standalone settings
  // page at /settings/profile/<tab>: grouped sidebar navigation, the general
  // tab (names, locked email, avatar dialog, email-change flow, deactivation
  // danger zone) and the security tab (password change with strength,
  // confirmation and reveal controls). Appended; existing entries above are
  // untouched per the shared contract.
  // ---------------------------------------------------------------------------

  /** Open one profile-settings tab authenticated; settles on the shell or the loader-only view. */
  settingsProfileOpen(tabId: string, cookies: ParityBrowserCookie[]): Promise<void>;
  /** Whether the settings shell (sidebar with the settings title) rendered. */
  settingsProfileShellVisible(): Promise<boolean>;
  /** Whether the page shows only the loading indicator with no settings content. */
  settingsProfileLoaderOnly(): Promise<boolean>;
  /** Sidebar group headings in display order (personal, developer, workspace shortcuts). */
  settingsProfileGroupLabels(): Promise<string[]>;
  /** Tab labels inside one sidebar group, in display order. */
  settingsProfileGroupTabs(group: string): Promise<string[]>;
  /** Label of the currently active sidebar tab, or null when none reads active. */
  settingsProfileActiveTab(): Promise<string | null>;
  /** Accessible name of the sidebar back control, or null when it renders none. */
  settingsProfileBackLabel(): Promise<string | null>;
  /** Sidebar title text ("Profile settings"), or null when the shell is absent. */
  settingsProfileSidebarTitle(): Promise<string | null>;
  /** Workspace shortcut entries (workspaces plus create/invites actions) in display order. */
  settingsProfileWorkspaceShortcuts(): Promise<string[]>;
  /** Activate one sidebar tab; resolves once the tab address lands. */
  settingsProfileSelectTab(label: string): Promise<void>;
  /** Open an arbitrary path authenticated; resolves with the final path-plus-query after redirects. */
  settingsProfileOpenPath(path: string, cookies: ParityBrowserCookie[]): Promise<string>;
  /** Current value of one general-tab field. */
  settingsProfileGeneralFieldValue(field: SettingsProfileGeneralField): Promise<string>;
  /** Fill one general-tab field without submitting. */
  settingsProfileGeneralFillField(field: SettingsProfileGeneralField, value: string): Promise<void>;
  /** Inline error under one general-tab field, or null when the field shows none. */
  settingsProfileGeneralFieldError(field: SettingsProfileGeneralField): Promise<string | null>;
  /** Whether the general-tab email input is disabled. */
  settingsProfileGeneralEmailDisabled(): Promise<boolean>;
  /** Submit the general form; resolves once feedback (toast or inline) settles. */
  settingsProfileGeneralSave(): Promise<void>;
  /** General save button label ("Save changes" idle, "Saving" while working). */
  settingsProfileGeneralSaveLabel(): Promise<string>;
  /** General-tab header identity line (displayed name plus email). */
  settingsProfileGeneralIdentity(): Promise<{ name: string; email: string }>;
  /** Open the avatar upload dialog from the general tab. */
  settingsProfileAvatarDialogOpen(): Promise<void>;
  /** Whether the avatar upload dialog is currently visible. */
  settingsProfileAvatarDialogVisible(): Promise<boolean>;
  /** Pick one file in the avatar dialog without submitting (rejection probes). */
  settingsProfileAvatarPick(filePath: string): Promise<void>;
  /** Upload one image file through the avatar dialog and save; resolves once feedback settles. */
  settingsProfileAvatarUpload(filePath: string): Promise<void>;
  /** Remove the current avatar through the dialog; resolves once feedback settles. */
  settingsProfileAvatarRemove(): Promise<void>;
  /** Whether the dialog's remove action renders disabled. */
  settingsProfileAvatarRemoveDisabled(): Promise<boolean>;
  /** Whether the general tab shows an avatar photo (as opposed to the placeholder). */
  settingsProfileAvatarPhotoVisible(): Promise<boolean>;
  /** Dismiss the avatar dialog without saving. */
  settingsProfileAvatarCancel(): Promise<void>;
  /** Invalid-file rejection text in the avatar dialog, or null when none shows. */
  settingsProfileAvatarRejection(): Promise<string | null>;
  /** Whether the general tab shows a cover-image control (main#523/#574 remove it; still present). */
  settingsProfileCoverControlVisible(): Promise<boolean>;
  /** Whether the change-email entry renders below the locked email field. */
  settingsProfileChangeEmailVisible(): Promise<boolean>;
  /** Open the change-email dialog. */
  settingsProfileChangeEmailOpen(): Promise<void>;
  /** Which change-email step shows: address entry, code verification, or no dialog. */
  settingsProfileEmailModalStep(): Promise<"email" | "code" | "closed">;
  /** Fill the new-address field of the change-email dialog. */
  settingsProfileEmailModalFillEmail(email: string): Promise<void>;
  /** Submit the visible change-email step; resolves once the dialog settles. */
  settingsProfileEmailModalSubmit(): Promise<void>;
  /** Fill the verification-code field of the change-email dialog. */
  settingsProfileEmailModalFillCode(code: string): Promise<void>;
  /** Inline error on one change-email field, or null when it shows none. */
  settingsProfileEmailModalError(field: "email" | "code"): Promise<string | null>;
  /** Helper line under the code field, or null when none shows. */
  settingsProfileEmailModalHelper(): Promise<string | null>;
  /** Whether the address field is locked while the code step shows. */
  settingsProfileEmailModalEmailDisabled(): Promise<boolean>;
  /** Dismiss the change-email dialog. */
  settingsProfileEmailModalClose(): Promise<void>;
  /** Whether the deactivation danger zone renders on the general tab. */
  settingsProfileDeactivateVisible(): Promise<boolean>;
  /** Danger-zone explanatory text, or null when the zone is absent. */
  settingsProfileDeactivateDescription(): Promise<string | null>;
  /** Open the deactivation confirmation dialog. */
  settingsProfileDeactivateOpen(): Promise<void>;
  /** Deactivation dialog title, or null when no dialog shows. */
  settingsProfileDeactivateTitle(): Promise<string | null>;
  /** Confirm deactivation; resolves once the app settles after the write. */
  settingsProfileDeactivateConfirm(): Promise<void>;
  /** Dismiss the deactivation dialog without confirming. */
  settingsProfileDeactivateCancel(): Promise<void>;
  /** Whether the confirm action currently reads as working ("Deactivating"). */
  settingsProfileDeactivateWorking(): Promise<boolean>;
  /** Whether one security-tab password field renders (current is gated by account origin). */
  settingsProfileSecurityFieldVisible(field: SettingsProfileSecurityField): Promise<boolean>;
  /** Fill one security-tab password field. */
  settingsProfileSecurityFill(field: SettingsProfileSecurityField, value: string): Promise<void>;
  /** Input type of one password field ("password" masked, "text" revealed). */
  settingsProfileSecurityFieldType(field: SettingsProfileSecurityField): Promise<string>;
  /** Toggle the reveal control of one password field. */
  settingsProfileSecurityToggleReveal(field: SettingsProfileSecurityField): Promise<void>;
  /** Inline error on one password field, or null when it shows none. */
  settingsProfileSecurityFieldError(field: SettingsProfileSecurityField): Promise<string | null>;
  /** Whether the confirmation-mismatch note currently shows. */
  settingsProfileSecurityMismatchVisible(): Promise<boolean>;
  /** Whether the new-same-as-old note currently shows. */
  settingsProfileSecuritySameAsOldVisible(): Promise<boolean>;
  /** Whether the strength indicator currently shows under the new-password field. */
  settingsProfileSecurityStrengthVisible(): Promise<boolean>;
  /** Whether the change-password submit currently renders disabled. */
  settingsProfileSecuritySubmitDisabled(): Promise<boolean>;
  /** Submit the password form; resolves once feedback settles. */
  settingsProfileSecuritySubmit(): Promise<void>;
  /** Current value of one password field (proves the post-success reset). */
  settingsProfileSecurityFieldValue(field: SettingsProfileSecurityField): Promise<string>;
  /** Move focus away from one password field (proves focus-aware note timing). */
  settingsProfileSecurityBlur(field: SettingsProfileSecurityField): Promise<void>;

  // --- Desktop share links, transport, CLI, native shell (NEWFRONT-210, DESK-019–025).
  // --- Appended; existing methods above are untouched per the shared driver
  // --- contract. The native halves live in the desktop shell (main.rs) and
  // --- the desktop bundle; on web these rows assert the web-observable
  // --- side: same-origin copied links, browser fetch transport, the
  // --- human-typed device approval, normal close semantics, no zoom menu,
  // --- normal server-host navigation, and rendered entry routes.
  /** True when the desktop native-HTTP flag is defined on the window. */
  deskShellNativeHttpPresent(): Promise<boolean>;
  /** Start recording every request URL the page issues. */
  deskShellBeginTrafficWatch(): Promise<void>;
  /** Request URLs recorded since the traffic watch began, in order. */
  deskShellTrafficUrls(): Promise<string[]>;
  /** Stop the traffic watch and detach its listener. */
  deskShellEndTrafficWatch(): Promise<void>;
  /** Browser fetch failure names for aborted, timed-out, and unroutable requests. */
  deskShellAbortSemantics(): Promise<{ aborted: string; timedOut: string; unroutable: string }>;
  /** True when an app menu carries zoom in/out/reset entries. */
  deskShellZoomMenuVisible(): Promise<boolean>;
  /** Heading of the device-code approval page, or null when absent. */
  deskShellDeviceAuthHeading(): Promise<string | null>;
  /** Whether the device-code approval submit currently renders enabled. */
  deskShellDeviceApproveEnabled(): Promise<boolean>;
  /** Type the device code into the approval form and submit; resolves once the outcome settles. */
  deskShellDeviceSubmitCode(code: string): Promise<void>;
  /** Close the page, reopen the entry in a fresh page of the same context; resolves with the landed path. The driver's page stays closed, so only API checks may follow. */
  deskShellCloseReopenPath(): Promise<string>;
}

/** General-tab field keys for the NEWFRONT-426 settingsProfile readers. */
export type SettingsProfileGeneralField = "first_name" | "last_name" | "display_name" | "email";

/** Security-tab field keys for the NEWFRONT-426 settingsProfile readers. */
export type SettingsProfileSecurityField = "current" | "new" | "confirm";

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

/** One weekday chip in the recurrence builder, in Sunday-first display order. */
export interface SchedulerRecurrenceWeekday {
  name: string;
  pressed: boolean;
}

/** Current state of the calendar-style recurrence builder. */
export interface SchedulerRecurrenceFacts {
  /** Whether the builder renders (the oracle still has a plain textarea). */
  visible: boolean;
  /** Repeat-every count, or null when the single-shot form hides it. */
  interval: string | null;
  /** Selected repeat unit value. */
  unit: string | null;
  /** Every repeat unit the control offers, in display order. */
  unitOptions: string[];
  /** Weekly chips with their pressed state. */
  weekdays: SchedulerRecurrenceWeekday[];
  /** Selected monthly form ("day" or "weekday"), or null outside monthly. */
  monthlyPattern: string | null;
  /** Monthly day-of-month value, or null when the day form hides. */
  dayOfMonth: string | null;
  /** Monthly nth-weekday ordinal, or null when the weekday form hides. */
  ordinal: string | null;
  /** Monthly weekday code, or null when the weekday form hides. */
  weekdayOfMonth: string | null;
  /** Selected ends mode ("never", "on" or "after"), or null when hidden. */
  ends: string | null;
  /** End-date input value, or null when that mode hides. */
  endDate: string | null;
  /** Occurrence-count input value, or null when that mode hides. */
  occurrences: string | null;
  /** Whether the raw rule textarea is visible. */
  rawVisible: boolean;
  /** Current raw rule text (the stored string while untouched). */
  rawValue: string | null;
  /** Whether the advanced disclosure control is offered. */
  advancedVisible: boolean;
  /** The live plain-language sentence under the builder. */
  sentence: string;
}

/** One user-level interaction with the recurrence builder. */
export type SchedulerRecurrenceAction =
  | { kind: "interval"; value: number }
  | { kind: "unit"; value: "none" | "day" | "week" | "month" | "year" }
  | { kind: "weekday"; value: string }
  | { kind: "monthly"; value: "day" | "weekday" }
  | { kind: "day"; value: number }
  | { kind: "ordinal"; value: string }
  | { kind: "weekdayOfMonth"; value: string }
  | { kind: "ends"; value: "never" | "on" | "after" }
  | { kind: "endDate"; value: string }
  | { kind: "occurrences"; value: number };

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

/** Per-row marker facts on the live cycles list (NEWFRONT-249, CYC-002). */
export interface CyclesRowFacts {
  /** Completion-ring reading: percent text, "done" at 100, or null when no ring shows. */
  progress: string | null;
  /** Rendered date span text, or null when the row shows no dates. */
  dateText: string | null;
  /** Work-item count text on draft/upcoming rows, or null when absent. */
  workItemCount: string | null;
  /** Whether a creator avatar shows on the row. */
  hasCreatorAvatar: boolean;
  /** Whether a favorite control shows on the row. */
  hasFavorite: boolean;
  /** Whether the favorite control renders selected. */
  favoriteSelected: boolean;
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

/** One headline total card (NEWFRONT-376, ANL-005/008). */
export interface AnalyticsHeadlineCard {
  /** Card label as rendered. */
  label: string;
  /** Card count as rendered. */
  value: string;
}

/** One written radar-summary row (NEWFRONT-376, ANL-006). */
export interface AnalyticsRadarRow {
  /** Dimension name as rendered. */
  dimension: string;
  /** Dimension count as rendered. */
  count: string;
}

/** Radar graphic facts (NEWFRONT-376, ANL-006). */
export interface AnalyticsRadarFacts {
  /** Angle-axis dimension labels in chart order. */
  dimensions: string[];
  /** Whether the radar graphic (svg) renders. */
  plotted: boolean;
}

/** One active-projects row (NEWFRONT-376, ANL-007). */
export interface AnalyticsActiveProject {
  /** Project name as rendered (shortened when long). */
  name: string;
  /** Completion badge text as rendered (e.g. `75%`). */
  badge: string;
  /** Badge tone: success above halfway, danger at or below, unknown when unreadable. */
  tone: "success" | "danger" | "unknown";
  /** Whether the rendered name is visually shortened. */
  truncated: boolean;
}

/** Created-vs-resolved chart facts (NEWFRONT-376, ANL-009). */
export interface AnalyticsCreatedResolvedFacts {
  /** Legend entries in display order. */
  legend: string[];
  /** X-axis tick labels in display order. */
  xLabels: string[];
  /** Rendered area series count. */
  series: number;
  /** Guidance empty-panel text, or null when the chart renders. */
  guidance: string | null;
}

/** One custom-chart bar segment (NEWFRONT-377, ANL-011). */
export interface AnalyticsCustomChartSegment {
  /** Segment center x in SVG units (groups segments into bars). */
  cx: number;
  /** Segment top y in SVG units (orders segments within a bar). */
  cy: number;
  /** Segment fill color as rendered. */
  fill: string;
  /** Whether the segment's top edge rounds. */
  topRounded: boolean;
  /** Whether the segment's bottom edge rounds. */
  bottomRounded: boolean;
}

/** Custom bar chart facts (NEWFRONT-377, ANL-011). */
export interface AnalyticsCustomChartFacts {
  /** Rendered bar segments in document order. */
  segments: AnalyticsCustomChartSegment[];
  /** X-axis tick labels in display order. */
  xLabels: string[];
  /** Whether a legend renders beside the chart. */
  legend: boolean;
}

/** One custom-chart tooltip row (NEWFRONT-377, ANL-011). */
export interface AnalyticsCustomTooltipRow {
  /** Segment label as rendered. */
  label: string;
  /** Segment value as rendered. */
  value: string;
}

/** One dialog breakdown first-column identity (NEWFRONT-377, ANL-015). */
export interface AnalyticsDialogAssignee {
  /** First-column text as rendered (name, initial carrier, or the unassigned marker). */
  text: string;
  /** Whether the cell shows an avatar image. */
  avatar: boolean;
  /** Whether the cell shows a name initial with no avatar. */
  initial: boolean;
  /** Whether the cell shows the unassigned marker. */
  unassigned: boolean;
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

// --- Instance-admin entry/session oracle types (NEWFRONT-441, ADM-001..012).

/** Which branch of the admin bootstrap gate is showing. */
export type AdminRootBranch = "loader" | "failure" | "setup" | "sign-in" | "dashboard" | "unknown";

/** Failure-screen facts (NEWFRONT-441, ADM-003). */
export interface AdminFailureFacts {
  /** Whether the explanatory illustration is showing. */
  illustrationVisible: boolean;
  /** Whether the retry control is showing. */
  retryVisible: boolean;
  /** Retry control label, or null when absent. */
  retryLabel: string | null;
}

/** Setup-form field values for filling (NEWFRONT-441, ADM-004/005). */
export interface AdminSetupFields {
  firstName?: string;
  lastName?: string;
  email?: string;
  company?: string;
  password?: string;
  confirmPassword?: string;
}

/** Setup-form facts (NEWFRONT-441, ADM-004/005/006). */
export interface AdminSetupFacts {
  /** Current field values. */
  values: { firstName: string; lastName: string; email: string; company: string };
  /** Whether the password and confirm fields are currently shown as text. */
  shown: { password: boolean; confirm: boolean };
  /** Whether the submit control is disabled. */
  submitDisabled: boolean;
  /** Submit control label (busy state swaps it for a spinner). */
  submitBusy: boolean;
  /** Whether the telemetry opt-in is checked. */
  telemetryChecked: boolean;
  /** Whether the strength meter (fragments) is showing. */
  strengthVisible: boolean;
  /** Strength message text, or null when no message renders. */
  strengthMessage: string | null;
  /** Whether the confirm-mismatch hint is showing. */
  mismatchVisible: boolean;
  /** Top error banner text, or null when absent. */
  bannerText: string | null;
  /** Inline field error texts, or null when absent. */
  inlineErrors: { email: string | null; password: string | null };
}

/** Sign-in-form facts (NEWFRONT-441, ADM-007/008). */
export interface AdminSignInFacts {
  /** Current email field value. */
  email: string;
  /** Whether the password field holds any value. */
  passwordFilled: boolean;
  /** Whether the password field is currently shown as text. */
  passwordShown: boolean;
  /** Whether the submit control is disabled. */
  submitDisabled: boolean;
  /** Whether the submit control shows its busy spinner. */
  submitBusy: boolean;
  /** Generic error banner text, or null when absent. */
  genericBanner: string | null;
  /** Mapped info banner facts, or null when absent. */
  mappedBanner: { text: string; linkHref: string | null } | null;
}

/** Account-menu facts (NEWFRONT-441, ADM-010). */
export interface AdminAccountMenuFacts {
  /** Admin email as rendered, or null when absent. */
  email: string | null;
  /** Theme entry label, or null when absent. */
  themeLabel: string | null;
  /** Whether the sign-out entry is showing. */
  signOutVisible: boolean;
}

/** 404-screen facts (NEWFRONT-441, ADM-011). */
export interface AdminNotFoundFacts {
  /** Whether the not-found illustration is showing. */
  illustrationVisible: boolean;
  /** Whether the explanatory message is showing. */
  messageVisible: boolean;
  /** Back-action label, or null when absent. */
  actionLabel: string | null;
}

// --- Instance-admin general/email oracle types (NEWFRONT-442, ADM-013..022).

/** General-settings facts (NEWFRONT-442, ADM-013..017). */
export interface AdminGeneralFacts {
  /** Current instance-name field value. */
  name: string;
  /** Whether the name field is disabled. */
  nameDisabled: boolean;
  /** Admin-email field value (first admin's address). */
  email: string;
  /** Whether the email field is disabled. */
  emailDisabled: boolean;
  /** Instance-ID field value. */
  instanceId: string;
  /** Whether the instance-ID field is disabled. */
  instanceIdDisabled: boolean;
  /** Whether the telemetry switch is on. */
  telemetryOn: boolean;
  /** Whether the telemetry switch is disabled. */
  telemetryDisabled: boolean;
  /** Telemetry policy link target, or null when absent. */
  telemetryPolicyHref: string | null;
  /** Telemetry policy link target attribute, or null when absent. */
  telemetryPolicyTarget: string | null;
  /** Save control label (busy state swaps it). */
  saveLabel: string;
  /** Whether the save control is disabled. */
  saveDisabled: boolean;
  /** Inline required-name message, or null when none shows. */
  nameError: string | null;
}

/** Email-page shell facts (NEWFRONT-442, ADM-018/022). */
export interface AdminEmailFacts {
  /** Whether the master toggle is showing. */
  toggleVisible: boolean;
  /** Whether the master toggle is on. */
  toggleOn: boolean;
  /** Whether the master toggle is disabled. */
  toggleDisabled: boolean;
  /** Whether the config form is showing. */
  formVisible: boolean;
  /** Whether the header pill skeleton is showing. */
  headerSkeletonVisible: boolean;
  /** Whether the multi-row form skeleton is showing. */
  formSkeletonVisible: boolean;
  /** Form skeleton row count (zero when the skeleton hides). */
  formSkeletonRows: number;
}

/** Transport-security choice as rendered (NEWFRONT-442, ADM-020). */
export type AdminEmailSecurity = "TLS" | "SSL" | "none";

/** Email config-form field values for filling (NEWFRONT-442, ADM-019). */
export interface AdminEmailFields {
  host?: string;
  port?: string;
  from?: string;
  user?: string;
  password?: string;
}

/** Email config-form facts (NEWFRONT-442, ADM-019/020/021). */
export interface AdminEmailFormFacts {
  /** Current field values. */
  values: { host: string; port: string; from: string; user: string; passwordFilled: boolean };
  /** Whether the password field is currently shown as text. */
  passwordShown: boolean;
  /** Rendered transport-security choice. */
  security: AdminEmailSecurity;
  /** Whether the save control is disabled. */
  saveDisabled: boolean;
  /** Save control label (busy state swaps it). */
  saveLabel: string;
  /** Whether the send-test-email control is disabled. */
  testEmailDisabled: boolean;
  /** Save-first hint shown while the form carries unsaved edits, or null. */
  saveFirstHint: string | null;
}

/** Send-test-email modal step (NEWFRONT-442, ADM-021). */
export type AdminTestEmailStep = "compose" | "success" | "failed";

/** Send-test-email modal facts (NEWFRONT-442, ADM-021). */
export interface AdminTestEmailFacts {
  /** Whether the modal is open. */
  open: boolean;
  /** Which step is showing. */
  step: AdminTestEmailStep;
  /** Current receiver field value. */
  receiver: string;
  /** Modal heading text. */
  heading: string;
  /** Step body text (success copy or the server error). */
  bodyText: string;
  /** Send control label (busy state swaps it). */
  sendLabel: string | null;
  /** Dismiss control label. */
  dismissLabel: string;
}

/** Method-card control branch as rendered (NEWFRONT-443, ADM-026). */
export type AdminAuthMethodControl = "toggle" | "configure" | "edit-toggle";

/** One authentication-method card in DOM order (NEWFRONT-443, ADM-024/025/026). */
export interface AdminAuthMethodCard {
  /** Card name text. */
  name: string;
  /** Card description text. */
  description: string;
  /** Whether the card shows an icon slot. */
  hasIcon: boolean;
  /** Which control branch the card renders. */
  control: AdminAuthMethodControl;
  /** Toggle state; null on the configure branch (no toggle rendered). */
  toggleOn: boolean | null;
  /** Whether the toggle is disabled (false on the configure branch). */
  toggleDisabled: boolean;
  /** Configure/Edit link target; null on the toggle-only branch. */
  linkHref: string | null;
}

/** Authentication landing-page snapshot (NEWFRONT-443, ADM-023/024/025/026/036). */
export interface AdminAuthLandingFacts {
  /** Whether the loaded page (sign-up row) is showing rather than the skeleton. */
  loaded: boolean;
  /** Whether the loading skeleton is showing. */
  skeletonVisible: boolean;
  /** Skeleton bar count (zero when the skeleton hides). */
  skeletonBars: number;
  /** Whether the open-sign-up switch is on. */
  signupOn: boolean;
  /** Whether the open-sign-up switch is disabled. */
  signupDisabled: boolean;
  /** Method cards in DOM order (empty while unloaded). */
  cards: AdminAuthMethodCard[];
}

/** Provider sub-page shell snapshot (NEWFRONT-443, ADM-027). */
export interface AdminAuthProviderFacts {
  /** Header card provider name. */
  name: string;
  /** Header card provider description. */
  description: string;
  /** Whether the header logo image is showing. */
  hasIcon: boolean;
  /** Whether the master toggle is on. */
  toggleOn: boolean;
  /** Whether the master toggle is disabled. */
  toggleDisabled: boolean;
  /** Whether the credential form body is showing. */
  formVisible: boolean;
  /** Whether the body skeleton is showing. */
  skeletonVisible: boolean;
  /** Body skeleton bar count (zero when the skeleton hides). */
  skeletonBars: number;
  /** Document tab title. */
  title: string;
}

/** Provider credential-form snapshot (NEWFRONT-443, ADM-028/029/030/031/032/035). */
export interface AdminAuthFormFacts {
  /** Field values keyed by input id. */
  values: Record<string, string>;
  /** Field placeholders keyed by input id. */
  placeholders: Record<string, string>;
  /** Field input types keyed by input id. */
  types: Record<string, string>;
  /** Sync-switch caption text. */
  syncLabel: string;
  /** Whether the sync switch is on. */
  syncOn: boolean;
  /** Whether the save control is disabled. */
  saveDisabled: boolean;
  /** Save control label (busy state swaps it). */
  saveLabel: string;
  /** Go-back link target, or null when absent. */
  backHref: string | null;
}

/** Unsaved-changes discard modal snapshot (NEWFRONT-443, ADM-034). */
export interface AdminAuthDiscardFacts {
  /** Whether the modal is open. */
  open: boolean;
  /** Modal title text. */
  title: string;
  /** Modal body text. */
  body: string;
  /** Keep-editing control label. */
  keepLabel: string;
  /** Leave (go-back) control label. */
  backLabel: string;
  /** Leave control link target, or null when absent. */
  backHref: string | null;
}

/** One copy-to-clipboard field in DOM order (NEWFRONT-443, ADM-033). */
export interface AdminAuthCopyFact {
  /** Field label text. */
  label: string;
  /** Shown URL text. */
  url: string;
}

/** Navbar project identity as rendered (NEWFRONT-448, SPC-010). */
export interface SpaceChromeNavbarIdentity {
  /** Project name text, the placeholder, or null when the name renders nothing. */
  name: string | null;
  /** Logo-mark branch: configured icon/emoji, the generic fallback, or an empty slot. */
  logo: "icon" | "emoji" | "fallback" | "empty";
}

/** Fixed product badge facts (NEWFRONT-448, SPC-012). */
export interface SpaceChromeBadgeFacts {
  /** Link target (the product site address). */
  href: string;
  /** Link target attribute (new-tab marker). */
  target: string | null;
  /** Rel attribute. */
  rel: string | null;
  /** Visible badge text. */
  text: string;
  /** Badge bounding box in CSS pixels. */
  box: { x: number; y: number; width: number; height: number };
  /** Viewport size the box is measured against. */
  viewport: { width: number; height: number };
}

/** Signed-in account menu identity (NEWFRONT-448, SPC-014). */
export interface SpaceChromeAccountIdentity {
  /** Display name shown on the avatar button, or null when absent. */
  displayName: string | null;
  /** Whether the opened menu offers sign-out. */
  offersSignOut: boolean;
}

/** Sign-out form target (NEWFRONT-448, SPC-014). */
export interface SpaceChromeSignOutForm {
  /** Form action URL. */
  action: string;
  /** Preserved return address, or null when the form carries none. */
  nextPath: string | null;
}

/** Sign-out outcome (NEWFRONT-448, SPC-014). */
export interface SpaceChromeSignOutResult {
  /** Redirect target the sign-out POST answered with, or null when unobserved. */
  locationHeader: string | null;
  /** Path the browser landed on after sign-out. */
  landedPath: string;
}

/** Framed-board facts (NEWFRONT-448, SPC-015). */
export interface SpaceChromeFramedFacts {
  /** Whether the frame carries an authenticated session (guard against vacuous hides). */
  sessionActiveInFrame: boolean;
  /** Whether issue rows render inside the frame. */
  boardReadable: boolean;
  /** Whether any sign-in affordance renders inside the frame. */
  signInVisible: boolean;
  /** Whether the account avatar renders inside the frame. */
  avatarVisible: boolean;
  /** Visible emoji-reaction add controls inside the frame. */
  emojiControls: number;
}

/** Loading-spinner probe outcome (NEWFRONT-448, SPC-016). */
export interface SpaceChromeSpinnerProbe {
  /** Whether the spinner showed while its stage was held. */
  sawSpinner: boolean;
  /** Whether the navbar showed its placeholder with content absent while held. */
  placeholderWhileHeld: boolean;
  /** Whether board content stayed absent while the spinner showed. */
  contentAbsentWhileSpinning: boolean;
  /** Whether the board settled with content after the hold released. */
  settledWithContent: boolean;
}

/** Navigation-progress probe outcome (NEWFRONT-448, SPC-017). */
export interface SpaceChromeProgressProbe {
  /** Whether the progress indicator showed during the slow transition. */
  sawBar: boolean;
  /** Whether the indicator completed once the transition settled. */
  completed: boolean;
}

/** Toast notice facts (NEWFRONT-448, SPC-017). */
export interface SpaceChromeToastFacts {
  /** Notice heading, or null when no notice appeared. */
  title: string | null;
  /** Notice body, or null when no notice appeared. */
  message: string | null;
  /** Clipboard text after a copy action, or null when unreadable. */
  clipboard: string | null;
}

/** Instance-failure screen facts (NEWFRONT-448, SPC-018). */
export interface SpaceChromeFailureFacts {
  /** Failure heading text, or null when absent. */
  heading: string | null;
  /** Whether the retry control renders. */
  retryVisible: boolean;
  /** Whether a themed illustration renders. */
  illustrationVisible: boolean;
  /** Whether the logo bar renders above the failure view. */
  logoBarVisible: boolean;
}

/** Crash-screen facts (NEWFRONT-448, SPC-019). */
export interface SpaceChromeCrashFacts {
  /** Error heading text, or null when absent. */
  heading: string | null;
  /** Whether the refresh control renders. */
  refreshVisible: boolean;
  /** Support contact link target, or null when absent. */
  supportHref: string | null;
  /** Forum link target, or null when absent. */
  forumHref: string | null;
}

/** Locale-observable board facts (NEWFRONT-448, SPC-065). */
export interface SpaceLocaleBoardFacts {
  /** Peek property rows (state/priority/due-date labels and values). */
  peekProperties: string;
  /** Public issue keys in display order. */
  issueKeys: string[];
  /** Board chrome text (navbar plus badge). */
  chromeText: string;
  /** Document language tag. */
  htmlLang: string | null;
  /** Stored locale choice, or null when none is stored. */
  storedLocale: string | null;
}

/** Landing-page chrome strings for locale comparison (NEWFRONT-448, SPC-065). */
export interface SpaceLocaleLandingFacts {
  /** Auth gate welcome title line. */
  title: string;
  /** Auth gate subtitle line. */
  subtitle: string;
  /** Consent/legal line text. */
  consent: string;
}

/** Document head metadata facts for a space page (NEWFRONT-447, SPC-007/008). */
export interface SpaceAccessHeadFacts {
  /** Rendered document title. */
  title: string;
  /** Content of meta[name="description"], or null when absent. */
  description: string | null;
  /** Content of meta[name="robots"], or null when absent. */
  robots: string | null;
  /** Content of meta[property="og:title"], or null when absent. */
  ogTitle: string | null;
  /** Content of meta[property="og:description"], or null when absent. */
  ogDescription: string | null;
  /** Content of meta[property="og:image"], or null when absent. */
  ogImage: string | null;
  /** Content of meta[name="twitter:title"], or null when absent. */
  twitterTitle: string | null;
  /** Content of meta[name="twitter:description"], or null when absent. */
  twitterDescription: string | null;
  /** Content of meta[name="twitter:image"], or null when absent. */
  twitterImage: string | null;
}

// --- Space engagement facts (NEWFRONT-451, SPC-047–057). ---

/** Where an anonymous engagement action landed (NEWFRONT-451). */
export interface SpaceAuthLanding {
  /** True when the current page is the auth gate (landing). */
  onAuthGate: boolean;
  /** Board anchor the return address preserves, when one is preserved. */
  returnAnchor: string | null;
  /** Board view the return address preserves, when one is preserved. */
  returnView: string | null;
  /** Open issue the return address preserves, when one is preserved. */
  returnIssueId: string | null;
}

/** One side of the up/down vote control (NEWFRONT-451). */
export interface SpaceVoteSide {
  /** Vote count as rendered. */
  count: number;
  /** Voter display names from the hover card, in render order. */
  voters: string[];
  /** True when the control shows the visitor's own vote. */
  mine: boolean;
}

/** Overlay or card vote state (NEWFRONT-451). */
export interface SpaceVoteState {
  up: SpaceVoteSide;
  down: SpaceVoteSide;
}

/** One grouped emoji reaction row (NEWFRONT-451). */
export interface SpaceReactionGroup {
  /** Emoji character as rendered. */
  emoji: string;
  /** Reaction count as rendered. */
  count: number;
  /** True when the pill shows the visitor's own reaction. */
  mine: boolean;
  /** Reactor display names from the hover card, in render order. */
  users: string[];
}

/** An emoji chosen through a reaction picker (NEWFRONT-451). */
export interface SpacePickedEmoji {
  /** Emoji character as rendered. */
  emoji: string;
  /** Decimal code-point form the API stores ("128077" for 👍). */
  code: string;
}

/** Comments section visibility facts (NEWFRONT-451). */
export interface SpaceCommentSection {
  /** True when the section shell renders at all. */
  sectionVisible: boolean;
  /** Thread rows rendered. */
  threadCount: number;
  /** True when the composer renders. */
  composerVisible: boolean;
  /** True when the anonymous sign-in invitation renders. */
  signInInviteVisible: boolean;
}

/** One rendered comment row (NEWFRONT-451). */
export interface SpaceCommentRow {
  /** Author line as rendered (bots carry their suffix). */
  author: string;
  /** True when the row carries the bot marking. */
  botMark: boolean;
  /** Age line as rendered ("commented …"). */
  ageText: string;
  /** Rendered body text (empty while folded and unexpanded). */
  contentText: string;
  /** True when the row is folded (moderated). */
  folded: boolean;
  /** Machine-authored speaker label, when the row carries one. */
  speakerLabel: string | null;
  /** Whether the avatar renders an image or an initial fallback. */
  avatarKind: "image" | "initial";
  /** True when the rendered body embeds an image node. */
  hasImage: boolean;
}

/** Outcome of a mention attempt in the composer (NEWFRONT-451). */
export interface SpaceMentionResult {
  /** True when the dropdown offered the member. */
  offered: boolean;
  /** True when a mention chip landed in the draft. */
  inserted: boolean;
}

/** One visible toast notice (NEWFRONT-451). */
export interface SpaceCommentNotice {
  message: string;
  kind: "success" | "error" | "unknown";
}

// --- Page-export dialog oracle types (NEWFRONT-383, EXP-016..021 + 023/024). ---

/** One choice row of the page-export dialog. */
export type PageExportRow = "format" | "scope" | "pageFormat";

/** A captured page-export download with its full bytes (NEWFRONT-383). */
export interface PageExportFile {
  /** Suggested file name the browser download carried. */
  fileName: string;
  /** Download byte length. */
  byteLength: number;
  /** Full download bytes as latin1 text (byte-faithful for binary introspection). */
  content: string;
}

/** Focused-control facts for keyboard walkthroughs (NEWFRONT-383). */
export interface PageExportFocusFacts {
  /** Accessible label of the focused control (`role :: name`). */
  label: string;
  /** Visible focus indicator kind: a drawn outline, a drawn shadow, or none. */
  indicator: "outline" | "shadow" | "none";
  /** Whether the control matches :focus-visible. */
  focusVisible: boolean;
}

// --- Admin workspaces + AI + image oracle types (NEWFRONT-444, ADM-037..050). ---

/** Workspace-list page snapshot (NEWFRONT-444, ADM-037/038/040/041/042). */
export interface AdminWorkspaceListFacts {
  /** Whether the restrict-creation toggle is showing (config loaded). */
  toggleVisible: boolean;
  /** Whether the restrict-creation toggle is on. */
  toggleOn: boolean;
  /** Whether the restrict-creation toggle is disabled. */
  toggleDisabled: boolean;
  /** Whether the toggle-row skeleton is showing (config loading). */
  toggleSkeletonVisible: boolean;
  /** Rendered workspace count, or null when the section is not showing. */
  count: number | null;
  /** Whether the mutation/pagination spinner beside the count is showing. */
  countSpinnerVisible: boolean;
  /** Whether the list skeleton (first load) is showing. */
  listSkeletonVisible: boolean;
  /** List skeleton row count (zero when the skeleton hides). */
  listSkeletonRows: number;
  /** Whether the no-delete helper line is showing. */
  helperVisible: boolean;
  /** Create-workspace link target, or null when absent. */
  createHref: string | null;
  /** Whether the load-more control is showing. */
  loadMoreVisible: boolean;
  /** Whether the load-more control is disabled. */
  loadMoreDisabled: boolean;
  /** Load-more control label. */
  loadMoreLabel: string | null;
}

/** One workspace row as rendered (NEWFRONT-444, ADM-038/039/042). */
export interface AdminWorkspaceRow {
  /** Workspace name as rendered. */
  name: string;
  /** Workspace slug as rendered (without brackets). */
  slug: string;
  /** Row link target (main web app path). */
  href: string | null;
  /** Row link target attribute. */
  target: string | null;
  /** Row link rel attribute. */
  rel: string | null;
  /** Owner email as rendered, or null when the row shows none. */
  ownerEmail: string | null;
  /** Project count as rendered, or null when the row shows none. */
  totalProjects: number | null;
  /** Member count as rendered, or null when the row shows none. */
  totalMembers: number | null;
  /** Whether the row shows a logo image (vs a letter fallback). */
  logoImageVisible: boolean;
  /** Letter fallback as rendered, or null when a logo image shows. */
  logoLetter: string | null;
  /** Whether the row exposes any delete or edit control. */
  exposesMutation: boolean;
}

/** Workspace-create form snapshot (NEWFRONT-444, ADM-043/044). */
export interface AdminWorkspaceCreateFacts {
  /** Current name field value. */
  name: string;
  /** Current slug field value. */
  slug: string;
  /** Slug-prefix (web base URL) as rendered, or null when absent. */
  slugPrefix: string | null;
  /** Selected org-size label, or null when none is picked. */
  orgSize: string | null;
  /** Whether the submit control is disabled. */
  submitDisabled: boolean;
  /** Submit control label (busy state swaps it). */
  submitLabel: string;
  /** Name validation message, or null when none shows. */
  nameError: string | null;
  /** Slug validation message, or null when none shows. */
  slugError: string | null;
  /** Whether the inline URL-taken error is showing. */
  slugTakenVisible: boolean;
  /** Whether the invalid-slug charset hint is showing. */
  invalidSlugVisible: boolean;
  /** Org-size validation message, or null when none shows. */
  orgSizeError: string | null;
  /** Go-back link target, or null when absent. */
  backHref: string | null;
}

/** AI settings page snapshot (NEWFRONT-444, ADM-046/047/048). */
export interface AdminAiFacts {
  /** Whether the AI form is showing (config loaded). */
  formVisible: boolean;
  /** Current model field value. */
  model: string;
  /** Whether the key field holds any value. */
  keyFilled: boolean;
  /** Whether the key field is currently shown as text. */
  keyShown: boolean;
  /** Save control label (busy state swaps it). */
  saveLabel: string;
  /** Whether the save control is disabled. */
  saveDisabled: boolean;
  /** Model helper-link target, or null when absent. */
  modelHelpHref: string | null;
  /** Key helper-link target, or null when absent. */
  keyHelpHref: string | null;
  /** Whether the other-vendor callout is showing. */
  calloutVisible: boolean;
  /** Other-vendor callout contact-link target, or null when absent. */
  calloutHref: string | null;
  /** Whether the loading skeleton is showing. */
  skeletonVisible: boolean;
  /** Skeleton block count (zero when the skeleton hides). */
  skeletonBlocks: number;
}

/** Image-page presence snapshot (NEWFRONT-444, ADM-049/050; bug: NEWFRONT-493). */
export interface AdminImageFacts {
  /** Whether the sidebar Images entry is showing. */
  sidebarEntryVisible: boolean;
  /** Sidebar Images entry target, or null when absent. */
  sidebarHref: string | null;
  /** Image page title as rendered, or null when the page is not showing. */
  pageTitle: string | null;
  /** Whether the Unsplash key field is showing. */
  keyFieldVisible: boolean;
  /** Whether the image save control is showing. */
  saveVisible: boolean;
  /** Whether the image loading skeleton is showing. */
  skeletonVisible: boolean;
}

// --- Admin loop jobs + runners oracle types (NEWFRONT-445, ADM-051..061 + ADM-080). ---

/** One loop job row as rendered in the list table (NEWFRONT-445, ADM-051/052). */
export interface AdminLoopRow {
  /** Job name as rendered. */
  name: string;
  /** Name link target (job detail path), or null when absent. */
  href: string | null;
  /** Job slug as rendered. */
  slug: string;
  /** Recurrence rule as rendered. */
  rrule: string;
  /** Minimum-role label as rendered. */
  minRoleLabel: string;
  /** Whether the row shows the job as builtin. */
  builtin: boolean;
  /** Whether the row's enabled switch is on. */
  enabled: boolean;
}

/** Loop-list page snapshot (NEWFRONT-445, ADM-051/053/054/055). */
export interface AdminLoopListFacts {
  /** Whether the loop header title is showing. */
  titleVisible: boolean;
  /** Whether the loop header description is showing. */
  descriptionVisible: boolean;
  /** Whether the New-job action is showing. */
  createVisible: boolean;
  /** Whether the list loading skeleton is showing. */
  skeletonVisible: boolean;
  /** List skeleton bar count (zero when the skeleton hides). */
  skeletonRows: number;
  /** List skeleton bar heights as rendered, top to bottom. */
  skeletonHeights: string[];
  /** Whether the empty-state message is showing. */
  emptyVisible: boolean;
  /** Empty-state message text, or null when hidden. */
  emptyText: string | null;
  /** Whether the jobs table is showing. */
  tableVisible: boolean;
  /** Table column headers as rendered, left to right. */
  headers: string[];
}

/** Loop job modal field values (NEWFRONT-445, ADM-055/056/057). */
export interface AdminLoopModalFields {
  /** Slug field value. */
  slug: string;
  /** Admin-name field value. */
  name: string;
  /** User-facing-name field value. */
  publicName: string;
  /** User-facing-description field value. */
  publicDescription: string;
  /** Prompt field value. */
  prompt: string;
  /** Minimum-role option value, or null when none matches. */
  minRole: number | null;
  /** Timezone field value. */
  tzid: string;
  /** Recurrence-rule field value. */
  rrule: string;
}

/** Loop job create/edit modal snapshot (NEWFRONT-445, ADM-055/056/057). */
export interface AdminLoopModalFacts {
  /** Whether the modal is showing. */
  open: boolean;
  /** Modal title as rendered ("New job" / "Edit job"), or null when closed. */
  title: string | null;
  /** Current field values (blanks when closed). */
  fields: AdminLoopModalFields;
  /** Whether the slug field is disabled. */
  slugDisabled: boolean;
  /** Whether the Save control is disabled. */
  saveDisabled: boolean;
  /** Whether the Cancel control is showing. */
  cancelVisible: boolean;
  /** Whether the modal body scrolls within a capped height. */
  bodyScrollable: boolean;
  /** Modal body max-height as rendered, or null when uncapped/absent. */
  bodyMaxHeight: string | null;
}

/** Loop job stat tiles as rendered (NEWFRONT-445, ADM-058). */
export interface AdminLoopStats {
  /** Targets tile value. */
  targets: number;
  /** Completed (24h) tile value. */
  completed: number;
  /** Failed (24h) tile value. */
  failed: number;
  /** Skipped (24h) tile value. */
  skipped: number;
}

/** Loop job detail page snapshot (NEWFRONT-445, ADM-058/059/060/061). */
export interface AdminLoopDetailFacts {
  /** Detail header title as rendered, or null when the header hides. */
  title: string | null;
  /** Detail header description as rendered, or null when the header hides. */
  description: string | null;
  /** Whether the detail loading skeleton is showing. */
  skeletonVisible: boolean;
  /** Detail skeleton bar heights as rendered, top to bottom. */
  skeletonHeights: string[];
  /** Whether the Edit action is showing. */
  editVisible: boolean;
  /** Whether the Delete action is showing. */
  deleteVisible: boolean;
  /** Skip-reason filter option values as rendered ("" first for all). */
  skipOptions: string[];
  /** Selected skip-reason filter value ("" for all). */
  skipSelected: string;
  /** Stat tiles, or null when the page shows none. */
  stats: AdminLoopStats | null;
  /** Whether the targets table is showing. */
  targetsTableVisible: boolean;
  /** Targets table column headers as rendered, left to right. */
  targetsHeaders: string[];
  /** Whether the no-targets placeholder row is showing. */
  targetsEmptyVisible: boolean;
  /** No-targets placeholder text, or null when hidden. */
  targetsEmptyText: string | null;
}

/** One loop target row as rendered in the detail table (NEWFRONT-445, ADM-058). */
export interface AdminLoopTargetRow {
  /** Workspace slug as rendered. */
  workspace: string;
  /** User email as rendered. */
  user: string;
  /** Next-run cell text as rendered. */
  nextRun: string;
  /** Last-run status cell text as rendered. */
  lastRun: string;
  /** Last-run tokens cell text as rendered. */
  tokens: string;
  /** Last skip reason cell text as rendered. */
  lastSkip: string;
}

/** Runners page body facts (ADM-080, target behavior per NEWFRONT-518). */
export interface AdminRunnersPageFacts {
  /** Whether the page root is showing. */
  pageVisible: boolean;
  /** Page heading text. */
  title: string;
  /** Whether the runners table is showing. */
  tableVisible: boolean;
  /** Whether the failure state is showing. */
  errorVisible: boolean;
  /** Whether the failure state offers its retry. */
  retryVisible: boolean;
}

/** Runners presence snapshot (NEWFRONT-445, ADM-080; bug-pinned until the page lands). */
export interface AdminRunnersFacts {
  /** Whether the sidebar Runners entry is showing. */
  sidebarEntryVisible: boolean;
  /** Sidebar Runners entry target, or null when absent. */
  sidebarHref: string | null;
}

/** One dashboard sidebar entry (NEWFRONT-446, ADM-062). */
export interface AdminSidebarEntry {
  /** Entry name as rendered. */
  name: string;
  /** Entry description as rendered (empty when collapsed to icons). */
  description: string;
  /** Entry target href. */
  href: string;
  /** Whether the entry carries the active highlight. */
  active: boolean;
}

/** Sidebar navigation + collapse snapshot (NEWFRONT-446, ADM-062/063). */
export interface AdminSidebarFacts {
  /** Menu entries in render order. */
  entries: AdminSidebarEntry[];
  /** Whether the sidebar is collapsed to the icon rail. */
  collapsed: boolean;
  /** Persisted collapse flag, or null when nothing is stored yet. */
  persistedCollapsed: boolean | null;
  /** Sidebar left edge in CSS pixels (negative when off-canvas). */
  x: number | null;
}

/** Account dropdown snapshot (NEWFRONT-446, ADM-064). */
export interface AdminAccountFacts {
  /** Whether the admin label row is showing. */
  labelVisible: boolean;
  /** Whether the avatar control is showing. */
  avatarVisible: boolean;
  /** Whether the dropdown menu is open. */
  menuOpen: boolean;
  /** Account email shown in the open menu, or null when closed. */
  email: string | null;
  /** Theme menu item label, or null when closed. */
  themeItemLabel: string | null;
  /** Whether the sign-out control is showing in the open menu. */
  signOutVisible: boolean;
  /** Sign-out form action, or null when closed. */
  signOutAction: string | null;
  /** Whether the sign-out form carries a CSRF token. */
  csrfTokenPresent: boolean;
}

/** One header breadcrumb (NEWFRONT-446, ADM-066). */
export interface AdminBreadcrumbItem {
  /** Crumb label as rendered. */
  label: string;
  /** Crumb target href, or null when the crumb is not a link. */
  href: string | null;
}

/** Header breadcrumb trail snapshot (NEWFRONT-446, ADM-066). */
export interface AdminBreadcrumbFacts {
  /** Crumbs in render order, leading with the settings entry. */
  items: AdminBreadcrumbItem[];
}

/** One help-popover link (NEWFRONT-446, ADM-067). */
export interface AdminHelpLink {
  /** Link name as rendered. */
  name: string;
  /** Link target href. */
  href: string | null;
  /** Link target attribute, or null when unset. */
  target: string | null;
}

/** Sidebar help-section snapshot (NEWFRONT-446, ADM-067). */
export interface AdminHelpFacts {
  /** Main-app redirect link href. */
  redirectHref: string | null;
  /** Whether the help popover is open. */
  popoverOpen: boolean;
  /** Popover links in render order (empty when closed). */
  links: AdminHelpLink[];
  /** Popover version footer text, or null when closed. */
  versionText: string | null;
}

/** Help-footer control carrying a collapsed hover label (NEWFRONT-446, ADM-067). */
export type AdminFooterControl = "redirect" | "help" | "toggle";

/** Mobile hamburger snapshot (NEWFRONT-446, ADM-068). */
export interface AdminHamburgerFacts {
  /** Whether the hamburger control is visible at the current viewport. */
  visible: boolean;
}

/** Realtime transport counts (NEWFRONT-446, ADM-069). */
export interface AdminTransportFacts {
  /** WebSocket connections opened since the spy started. */
  websockets: number;
  /** Event-stream requests seen since the spy started. */
  eventStreams: number;
}

/** Dashboard absence scan (NEWFRONT-446, ADM-070/072). */
export interface AdminAbsenceFacts {
  /** Elements marked draggable. */
  draggable: number;
  /** File inputs. */
  fileInputs: number;
  /** Buttons/links offering import or export. */
  transferControls: number;
  /** Buttons/links mentioning the desktop app. */
  desktopMentions: number;
}

/** Keyboard-shortcut probe outcome (NEWFRONT-446, ADM-071). */
export interface AdminShortcutProbeFacts {
  /** Path before the key presses. */
  pathBefore: string;
  /** Path after the key presses. */
  pathAfter: string;
  /** Dialogs that opened during the key presses. */
  dialogsOpened: number;
}

/** Confirmation-dialog watch outcome (NEWFRONT-446, ADM-073). */
export interface AdminDeleteWatchFacts {
  /** Native dialogs that fired during the delete. */
  nativeDialogs: number;
  /** In-page dialogs that appeared during the delete. */
  modalDialogs: number;
}

/** Workspace-page delete-control scan (NEWFRONT-446, ADM-073). */
export interface AdminWorkspaceDeleteFacts {
  /** Buttons/links offering workspace deletion. */
  deleteControls: number;
}

/** Cloud home-entry branches (NEWFRONT-446, ADM-074..079). */
export type AdminCloudHomeBranch = "spinner" | "denied" | "error";

/** Cloud home-entry snapshot (NEWFRONT-446, ADM-074..079). */
export interface AdminCloudHomeFacts {
  /** Which entry branch is showing. */
  branch: AdminCloudHomeBranch;
  /** Branch heading text. */
  heading: string | null;
  /** Denied-screen way-back link href (denied branch only). */
  deniedHref: string | null;
  /** Whether the retry control is showing (error branch only). */
  retryVisible: boolean;
  /** Password fields on the page (always zero on the cloud gate). */
  passwordFields: number;
  /** Setup-form fields on the page (always zero on the cloud gate). */
  setupFields: number;
}

/** Cloud gate request counts (NEWFRONT-446, ADM-078/079). */
export interface AdminCloudGateFetchCounts {
  /** Admin-check reads since the spy started. */
  adminCheck: number;
  /** Auth-probe reads since the spy started. */
  probe: number;
}

/** One personal-access-token row as the token page renders it (NEWFRONT-413, WSET-028). */
export interface WsSettingsApiTokenRow {
  /** Token label. */
  label: string;
  /** True for the active marker, false for the expired marker. */
  active: boolean;
  /** Human expiry line as rendered (future date, never-expires, or expired-ago). */
  expiryLine: string;
  /** Description as rendered, or null when the row shows none. */
  description: string | null;
}

/** The auto-downloaded token backup captured at creation (NEWFRONT-413, WSET-027). */
export interface WsSettingsApiTokenBackup {
  /** File name the app suggested for the save. */
  fileName: string;
  /** File bytes decoded as UTF-8 text. */
  content: string;
}

/** Theme choices the preferences tab offers (NEWFRONT-427, PROF-016). */
export type SettingsProfileTheme = "system" | "light" | "dark" | "light-contrast" | "dark-contrast" | "custom";

/** Custom-color designer field values (NEWFRONT-427, PROF-016). */
export interface SettingsProfileCustomTheme {
  /** Brand hex color as typed (with leading `#`). */
  primary: string;
  /** Neutral hex color as typed (with leading `#`). */
  background: string;
  /** Whether the dark palette is selected. */
  darkPalette: boolean;
}

/** Locale selector labels as shown (NEWFRONT-427, PROF-017). */
export interface SettingsProfileLocale {
  /** Language label, or null when the selector shows none. */
  language: string | null;
  /** Timezone label, or null when the selector shows none. */
  timezone: string | null;
  /** Week-start label, or null when the selector shows none. */
  weekStart: string | null;
}

/** Intake tab keys (NEWFRONT-255, INT-004). */
export type IntakeShellTab = "open" | "closed";

/** One intake list fetch as the UI sent it (NEWFRONT-255, INT-004/012). */
export interface IntakeShellListQuery {
  /** Full request URL. */
  url: string;
  /** Decoded query params (repeated keys joined with commas). */
  params: Record<string, string>;
}

/** Comment/activity requests observed on a detail screen (NEWFRONT-819, CMT-025). */
export interface CommentActivityTraffic {
  /** GETs to the issue history feed. */
  historyReads: number;
  /** GETs to the issue's comment list. */
  commentReads: number;
  /** POSTs that created a comment. */
  commentCreates: number;
  /** GETs to mention suggestions (entity-search). */
  mentionSearches: number;
  /** POSTs that added a comment reaction. */
  commentReactionCreates: number;
  /** DELETE requests against a comment reaction. */
  commentReactionDeletes: number;
  /** POSTs that added an issue reaction. */
  issueReactionCreates: number;
  /** DELETE requests against an issue reaction. */
  issueReactionDeletes: number;
  /**
   * POST/PATCH/PUT/DELETE requests to any other endpoint. A plain detail load
   * creates nothing, so any non-zero value means the build wrote somewhere the
   * shared comment/activity capabilities do not.
   */
  unexpectedWrites: number;
}
