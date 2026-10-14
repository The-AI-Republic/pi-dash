/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

/**
 * The overridable sign-out seam for downstream editions (e.g. a hosted OIDC
 * edition whose logout is a JSON request that returns an upstream logout URL).
 * Editions override sign-out by replacing this file wholesale — keep the
 * exported ``performSignOut`` name and signature stable so ``AuthService``
 * keeps resolving it. The default (self-hosted) strategy lives in
 * ``./auth-signout-default``.
 */
export { performSignOut } from "./auth-signout-default";
export type { SignOutClient } from "./auth-signout-default";
