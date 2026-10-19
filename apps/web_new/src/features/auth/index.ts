// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public API of the auth feature. Other features import only from here.

export {
  DEACTIVATE_FAILED_TITLE,
  DEACTIVATE_SUCCESS_MESSAGE,
  DEACTIVATE_SUCCESS_TITLE,
  FINISH_FAILURE_TITLE,
  JOIN_PROJECT_FAILED_TITLE,
  SINGLE_ANSWER_FAILED_TITLE,
  TOUR_COMPLETE_FAILED_TITLE,
  useAcceptWorkspaceInvitations,
  useAnswerWorkspaceInvitation,
  useApproveDeviceCode,
  useCheckWorkspaceSlug,
  useCompleteTour,
  useCreateJoinRequest,
  useCreateWorkspace,
  useDeactivateAccount,
  useEmailCheck,
  useFinishOnboarding,
  useForgotPassword,
  useJoinProject,
  useMagicGenerate,
  useMagicSignIn,
  useMagicSignUp,
  usePasswordReset,
  usePasswordSignIn,
  usePasswordSignUp,
  useSendWorkspaceInvites,
  useSetPassword,
  useSwitchAccount,
  useUpdateMe,
  useUploadUserAvatar,
} from "./api.js";
export { useUpdateProfile } from "./profile.js";
export type { AnswerInvitationInput } from "./api.js";
export { runSignOutAction, SIGN_OUT_COMMAND_ID, signOutCommand } from "./commands.js";
export { describeAuthError, describeSubmitFailure } from "./authErrors.js";
export type { AuthError, AuthErrorAction, AuthMode, SignInStep, SubmitFailure } from "./authErrors.js";
export {
  decideSingleInvitationView,
  INVITATION_IGNORE_LANDING,
  resolveSingleAcceptLanding,
  selectBatchLandingTarget,
  selectInvitationRoleLabel,
  toggleInvitationSelection,
} from "./invitations.js";
export type { SingleInvitationView, SingleInvitationViewState } from "./invitations.js";
export { asString, safeNextPath } from "./entrySearch.js";
export { isStrongSecret, secretCriteria } from "./passwordStrength.js";
export type { SecretCriterion } from "./passwordStrength.js";
export { CLOUD_PROVIDER_ID, resolveSignInProviders } from "./providers.js";
export type { ResolveProvidersOptions, SignInProvider } from "./providers.js";
export { authKeys, useViewerEmail } from "./queries.js";
export {
  invitationQueryOptions,
  myInvitationsQueryOptions,
  myJoinRequestsQueryOptions,
  useMyWorkspaceInvitations,
  useMyWorkspaceJoinRequests,
  useWorkspaceInvitation,
} from "./invitations.js";
export {
  advanceOnboardingStep,
  AVATAR_MAX_BYTES,
  AVATAR_MIME_TYPES,
  avatarFileError,
  CLI_INSTALL_RELEASE_BASE,
  CLI_INSTALL_TABS,
  combineUseCases,
  copyInstallCommand,
  decideInitialOnboardingStep,
  detectCliInstallTab,
  displayNameForAccount,
  finishProfileStamp,
  isOnboardingFlagSet,
  mergeOnboardingFlags,
  ONBOARDING_ROLES,
  ONBOARDING_STEPS,
  ONBOARDING_USE_CASES,
  onboardingProgress,
  parseUseCases,
  previousOnboardingStep,
  PROFILE_NAME_MAX_LENGTH,
  shouldShowInviteStep,
  USE_CASE_SEPARATOR,
  validateProfileName,
  visibleOnboardingSteps,
} from "./onboarding.js";
export type {
  CliInstallCommand,
  CliInstallTab,
  OnboardingAdvance,
  OnboardingStamp,
  OnboardingStep,
} from "./onboarding.js";
export {
  ADMIN_REQUEST_SUBJECT,
  buildAdminRequestMailto,
  deriveWorkspaceSlug,
  INVITE_INITIAL_ROW_COUNT,
  isReservedWorkspaceSlug,
  isSoloTeamSize,
  RESERVED_WORKSPACE_SLUGS,
  resolvePendingEmail,
  resolveWorkspaceDefaultView,
  selectPendingJoinRequest,
  SLUG_FORMAT_MESSAGE,
  SLUG_TAKEN_MESSAGE,
  SOLO_TEAM_SIZE,
  validateWorkspaceName,
  validateWorkspaceSlug,
  WORKSPACE_INVITE_ROLES,
  WORKSPACE_NAME_MAX_LENGTH,
  WORKSPACE_SLUG_MAX_LENGTH,
  WORKSPACE_TEAM_SIZES,
  workspaceRoleLabel,
} from "./workspace.js";
export type { WorkspaceStepView } from "./workspace.js";

export {
  CLOUD_SIGN_OUT_LANDING,
  isHostedBackend,
  performSignOut,
  SIGN_OUT_FAILED_MESSAGE,
  SIGN_OUT_FAILED_TITLE,
  signOutLandingTarget,
} from "./signOut.js";
export type { PerformSignOutArgs } from "./signOut.js";
export { useSignOut } from "./signOut.js";
export {
  describeDeviceApprovalFailure,
  DEVICE_APPROVE_FAILED_MESSAGE,
  DEVICE_CODE_FIELD_LENGTH,
  DEVICE_CODE_LENGTH,
  formatDeviceCode,
  isCompleteDeviceCode,
  normalizeDeviceCode,
} from "./device.js";
export { decideProjectDenial, decideWorkspaceDenial, isProjectGateError } from "./denials.js";
export type { ProjectDenial, WorkspaceDenial } from "./denials.js";
export {
  bakedWebBaseUrl,
  describeDesktopSignInFailure,
  fallbackToPlainNavigation,
  parseBakedWebBaseUrl,
  parseDesktopEntrySearch,
  resolveBrowserAppUrl,
} from "./desktop.js";
export type { DesktopEntrySearch, DesktopSignInFailure } from "./desktop.js";
export { authStepInputProps, pointerOnlyControlProps, swallowEnterInTextField, useFocusOnMount } from "./focus.js";
export {
  membershipQueryOptions,
  projectMembershipQueryOptions,
  projectQueryOptions,
  projectRolesQueryOptions,
  useProjectRoles,
  useProjectRow,
  useWorkspaceMembership,
} from "./queries.js";
export {
  nextTourStep,
  previousTourStep,
  shouldShowProductTour,
  TOUR_CONTENT_STOPS,
  TOUR_VIDEO_EMBED_URL,
  tourContentStop,
  tourWelcomeGreeting,
} from "./tour.js";
export type { TourContentStop, TourStepKey } from "./tour.js";
export { DeactivateAccountDialog } from "./components/DeactivateAccountDialog.js";
export type { DeactivateAccountDialogProps } from "./components/DeactivateAccountDialog.js";
export {
  BATCH_ACCEPT_FAILED_TITLE,
  BATCH_ACCEPT_STALE_MESSAGE,
  INVITATIONS_TITLE,
  InvitationInbox,
  NO_INVITES_ART,
} from "./components/InvitationInbox.js";
export {
  COMMUNITY_URL,
  GITHUB_STAR_URL,
  SINGLE_INVITATION_TITLE,
  SingleInvitation,
} from "./components/SingleInvitation.js";
export type { SingleInvitationProps } from "./components/SingleInvitation.js";
export { CloudLoginCard } from "./components/CloudLoginCard.js";
export type { CloudLoginCardProps } from "./components/CloudLoginCard.js";
export {
  clearExplicitLogout,
  cloudHandoffTarget,
  cloudSignInUrl,
  CLOUD_SIGN_IN_HREF,
  currentPageTarget,
  DESKTOP_OPEN_FAILED,
  DESKTOP_ORIGIN_MISSING,
  DESKTOP_ORIGIN_UNUSABLE,
  EXPLICIT_LOGOUT_KEY,
  markExplicitLogout,
  parseCloudLoginSearch,
  readExplicitLogout,
  resolveCloudReturnTarget,
  shouldAttemptSilentSignIn,
  startCloudHandoff,
} from "./cloudLogin.js";
export type { CloudLoginSearch, StartCloudHandoffArgs } from "./cloudLogin.js";
export { AuthBanner, isValidEmail, SignInCard } from "./components/SignInCard.js";
export { EntryScreen } from "./components/EntryScreen.js";
export { parseLoginSearch } from "./components/LoginSearch.js";
export type { LoginSearch } from "./components/LoginSearch.js";
export type { EntryScreenProps } from "./components/EntryScreen.js";
export type { AuthBannerProps, SignInCardProps } from "./components/SignInCard.js";
export { SignUpCard } from "./components/SignUpCard.js";
export type { SignUpCardProps } from "./components/SignUpCard.js";
export { ForgotPasswordCard } from "./components/ForgotPasswordCard.js";
export type { ForgotPasswordCardProps } from "./components/ForgotPasswordCard.js";
export { NewPasswordForm } from "./components/NewPasswordForm.js";
export type { NewPasswordFormProps } from "./components/NewPasswordForm.js";
export { ResetPasswordCard } from "./components/ResetPasswordCard.js";
export type { ResetPasswordCardProps } from "./components/ResetPasswordCard.js";
export { SetPasswordCard } from "./components/SetPasswordCard.js";
export { SwitchAccountDialog } from "./components/SwitchAccountDialog.js";
export type { SwitchAccountDialogProps } from "./components/SwitchAccountDialog.js";
export { DeviceApprovalCard } from "./components/DeviceApprovalCard.js";
export type { DeviceApprovalCardProps } from "./components/DeviceApprovalCard.js";
export { DesktopSignInScreen } from "./components/DesktopSignInScreen.js";
export type { DesktopSignInScreenProps } from "./components/DesktopSignInScreen.js";
export { DesktopUnavailableCard } from "./components/DesktopUnavailableCard.js";
export { ProjectAccessGate } from "./components/ProjectAccessGate.js";
export type { ProjectAccessGateProps } from "./components/ProjectAccessGate.js";
export { WorkspaceNotFoundScreen } from "./components/WorkspaceNotFoundScreen.js";
export type { WorkspaceNotFoundScreenProps } from "./components/WorkspaceNotFoundScreen.js";
export { WorkspaceNotMemberScreen } from "./components/WorkspaceNotMemberScreen.js";
export { CLI_COPY_FAILED_TITLE, CliInstallStep } from "./components/CliInstallStep.js";
export type { CliInstallStepProps } from "./components/CliInstallStep.js";
export { OnboardingFlow, ONBOARDING_STAMP_FAILED_TITLE } from "./components/OnboardingFlow.js";
export type { OnboardingFlowProps, OnboardingStepComponent, OnboardingStepProps } from "./components/OnboardingFlow.js";
export { OnboardingShell } from "./components/OnboardingShell.js";
export type { OnboardingShellProps } from "./components/OnboardingShell.js";
export {
  AVATAR_UPLOAD_FAILED_MESSAGE,
  PROFILE_SAVE_FAILED_MESSAGE,
  PROFILE_SAVE_FAILED_TITLE,
  ProfileSetupStep,
} from "./components/ProfileSetupStep.js";
export type { ProfileSetupStepProps } from "./components/ProfileSetupStep.js";
export {
  ROLE_REQUIRED_MESSAGE,
  ROLE_SAVE_FAILED_TITLE,
  ROLE_SAVE_SUCCESS_TITLE,
  RoleSetupStep,
} from "./components/RoleSetupStep.js";
export type { RoleSetupStepProps } from "./components/RoleSetupStep.js";
export {
  USE_CASE_REQUIRED_MESSAGE,
  USE_CASE_SAVE_FAILED_TITLE,
  USE_CASE_SAVE_SUCCESS_TITLE,
  UseCaseSetupStep,
} from "./components/UseCaseSetupStep.js";
export type { UseCaseSetupStepProps } from "./components/UseCaseSetupStep.js";
export {
  CREATE_FAILED_TITLE,
  CREATE_SUCCESS_TITLE,
  TEAM_SIZE_REQUIRED_MESSAGE,
  WorkspaceCreateForm,
} from "./components/WorkspaceCreateForm.js";
export type { TeamSizeSelectorField, WorkspaceCreateFormProps } from "./components/WorkspaceCreateForm.js";
export {
  INVITE_FAILED_TITLE,
  INVITE_ROW_ERROR_MESSAGE,
  INVITE_SUCCESS_TITLE,
  InviteMembersStep,
} from "./components/InviteMembersStep.js";
export type { InviteMembersStepProps } from "./components/InviteMembersStep.js";
export {
  INVITES_EMPTY_MESSAGE,
  INVITES_FAILED_TITLE,
  WorkspaceInvitesView,
} from "./components/WorkspaceInvitesView.js";
export type { WorkspaceInvitesViewProps } from "./components/WorkspaceInvitesView.js";
export { JOIN_FAILED_TITLE, WorkspaceJoinByEmailView } from "./components/WorkspaceJoinByEmailView.js";
export type { WorkspaceJoinByEmailViewProps } from "./components/WorkspaceJoinByEmailView.js";
export { WorkspacePendingView } from "./components/WorkspacePendingView.js";
export type { WorkspacePendingViewProps } from "./components/WorkspacePendingView.js";
export { WorkspaceStep } from "./components/WorkspaceStep.js";
export type { WorkspaceStepProps } from "./components/WorkspaceStep.js";
export {
  CREATION_DISABLED_ASK_LABEL,
  CREATION_DISABLED_BACK_LABEL,
  CREATION_DISABLED_HEADING,
  CreationDisabledScreen,
} from "./components/CreationDisabledScreen.js";
export type { CreationDisabledScreenProps } from "./components/CreationDisabledScreen.js";
export { CREATION_NOTICE_JOIN_LABEL, InOnboardingCreationNotice } from "./components/InOnboardingCreationNotice.js";
export type { InOnboardingCreationNoticeProps } from "./components/InOnboardingCreationNotice.js";
export {
  ProductTour,
  TOUR_CLOSE_LABEL,
  TOUR_SKIP_LABEL,
  TOUR_START_PROJECT_LABEL,
  TOUR_TAKE_TOUR_LABEL,
} from "./components/ProductTour.js";
export type { ProductTourProps } from "./components/ProductTour.js";
export { TeamSizeDropdown, TEAM_SIZE_DROPDOWN_PLACEHOLDER } from "./components/TeamSizeDropdown.js";
export { WorkspaceHomeTour } from "./components/WorkspaceHomeTour.js";
export type { WorkspaceHomeTourProps } from "./components/WorkspaceHomeTour.js";
