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
}
