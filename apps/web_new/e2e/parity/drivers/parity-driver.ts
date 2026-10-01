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
}

/** Overflow-menu option keys the rules specs exercise (stable keys, not labels). */
export type RulesCommentMenuOption = "edit" | "copy_link" | "access_switch" | "fold" | "unfold" | "delete";
