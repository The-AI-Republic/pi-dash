// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public API of the auth feature. Other features import only from here.

export { useEmailCheck, useMagicGenerate, useMagicSignIn, usePasswordSignIn, useSignOut } from "./api.js";
export { describeSignInError } from "./authErrors.js";
export type { SignInError, SignInStep } from "./authErrors.js";
export { SignInCard } from "./components/SignInCard.js";
export type { SignInCardProps } from "./components/SignInCard.js";
