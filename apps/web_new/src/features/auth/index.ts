// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public API of the auth feature. Other features import only from here.

export {
  DEACTIVATE_FAILED_TITLE,
  DEACTIVATE_SUCCESS_MESSAGE,
  DEACTIVATE_SUCCESS_TITLE,
  FINISH_FAILURE_TITLE,
  useDeactivateAccount,
  useEmailCheck,
  useFinishOnboarding,
  useForgotPassword,
  useMagicGenerate,
  useMagicSignIn,
  useMagicSignUp,
  usePasswordReset,
  usePasswordSignIn,
  usePasswordSignUp,
  useSetPassword,
  useSignOut,
  useSwitchAccount,
  useUpdateMe,
  useUpdateProfile,
  useUploadUserAvatar,
} from "./api.js";
export { runSignOutAction, SIGN_OUT_COMMAND_ID, signOutCommand } from "./commands.js";
export { describeAuthError, describeSubmitFailure } from "./authErrors.js";
export type { AuthError, AuthErrorAction, AuthMode, SignInStep, SubmitFailure } from "./authErrors.js";
export { asString, safeNextPath } from "./entrySearch.js";
export { isStrongSecret, secretCriteria } from "./passwordStrength.js";
export type { SecretCriterion } from "./passwordStrength.js";
export { resolveSignInProviders } from "./providers.js";
export type { ResolveProvidersOptions, SignInProvider } from "./providers.js";
export {
  authKeys,
  invitationQueryOptions,
  myInvitationsQueryOptions,
  myJoinRequestsQueryOptions,
  useMyWorkspaceInvitations,
  useMyWorkspaceJoinRequests,
  useWorkspaceInvitation,
} from "./queries.js";
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
export { performSignOut, SIGN_OUT_FAILED_MESSAGE, SIGN_OUT_FAILED_TITLE, signOutLandingTarget } from "./signOut.js";
export { DeactivateAccountDialog } from "./components/DeactivateAccountDialog.js";
export type { DeactivateAccountDialogProps } from "./components/DeactivateAccountDialog.js";
export { AuthBanner, isValidEmail, SignInCard } from "./components/SignInCard.js";
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
