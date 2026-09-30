// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Public API of the auth feature. Other features import only from here.

export { useEmailCheck, useMagicGenerate, useMagicSignIn, usePasswordSignIn, useSignOut } from "./api.js";
export { describeSignInError } from "./authErrors.js";
export type { SignInError, SignInStep } from "./authErrors.js";
export { SignInCard } from "./components/SignInCard.js";
export type { SignInCardProps } from "./components/SignInCard.js";
