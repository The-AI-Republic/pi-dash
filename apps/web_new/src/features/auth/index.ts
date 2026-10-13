// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public API of the auth feature. Other features import only from here.

export {
  useEmailCheck,
  useMagicGenerate,
  useMagicSignIn,
  useMagicSignUp,
  usePasswordSignIn,
  usePasswordSignUp,
  useSignOut,
} from "./api.js";
export { describeAuthError, describeSubmitFailure } from "./authErrors.js";
export type { AuthError, AuthErrorAction, AuthMode, SignInStep, SubmitFailure } from "./authErrors.js";
export { resolveSignInProviders } from "./providers.js";
export type { ResolveProvidersOptions, SignInProvider } from "./providers.js";
export { authKeys, invitationQueryOptions, useWorkspaceInvitation } from "./queries.js";
export { isValidEmail, SignInCard } from "./components/SignInCard.js";
export type { SignInCardProps } from "./components/SignInCard.js";
