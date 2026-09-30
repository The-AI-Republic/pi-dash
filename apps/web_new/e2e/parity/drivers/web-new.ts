// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// New-app driver skeleton (NEWFRONT-19). Implements the same interface as
// the oracle driver so scenarios compile against either target, but every
// action throws until the matching area lands in apps/web_new. Area issues
// fill these in method by method; the oracle driver stays untouched.
import type { Page } from "@playwright/test";
import type { ParityBrowserCookie, ParityDriver, ParityTarget, WorkspaceOnboardingView } from "./parity-driver";

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
}
