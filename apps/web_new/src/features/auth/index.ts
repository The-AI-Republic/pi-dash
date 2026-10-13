// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public API of the auth feature. Other features import only from here.

export {
  DEACTIVATE_FAILED_TITLE,
  DEACTIVATE_SUCCESS_MESSAGE,
  DEACTIVATE_SUCCESS_TITLE,
  useDeactivateAccount,
  useEmailCheck,
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
} from "./api.js";
export { runSignOutAction, SIGN_OUT_COMMAND_ID, signOutCommand } from "./commands.js";
export { describeAuthError, describeSubmitFailure } from "./authErrors.js";
export type { AuthError, AuthErrorAction, AuthMode, SignInStep, SubmitFailure } from "./authErrors.js";
export { asString, safeNextPath } from "./entrySearch.js";
export { isStrongSecret, secretCriteria } from "./passwordStrength.js";
export type { SecretCriterion } from "./passwordStrength.js";
export { resolveSignInProviders } from "./providers.js";
export type { ResolveProvidersOptions, SignInProvider } from "./providers.js";
export { authKeys, invitationQueryOptions, useWorkspaceInvitation } from "./queries.js";
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
