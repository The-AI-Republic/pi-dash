// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public-route decisions shared by the root, the marketing routes and
// their tests (CLOUD-001/002/005). Pure, so a redirect target is decided
// without a router and the OSS behavior stays visible in the unit suite.
import {
  isEditionMarketingPath,
  isEditionRootEnabled,
  publicMarketingPathOf,
  type Edition,
} from "../../core/edition/index.js";

/** Marketing home every public forward lands on. */
export const MARKETING_HOME_PATH = "/home";
/** Cloud sign-in page, rendered in place by the cloud edition. */
export const CLOUD_LOGIN_PATH = "/login";

/**
 * The address a signed-out visitor on the edition's root is sent to
 * (CLOUD-001). A safe return target is carried as the login page's
 * `next_path`, so the visitor comes back to it after signing in; an absent
 * or unsafe target is not carried, and the visitor lands on the marketing
 * home (CLOUD-013 discards unsafe values rather than forwarding them).
 * `safeNext` is the parser's already-sanitized value.
 */
export function signedOutRootTarget(safeNext: string | undefined): string {
  if (safeNext === undefined) return MARKETING_HOME_PATH;
  return `${CLOUD_LOGIN_PATH}?next_path=${encodeURIComponent(safeNext)}`;
}

/**
 * Which public surface an edition claims for a marketing address. The
 * route renders its page only when the edition declares the address;
 * otherwise the route behaves as if it did not exist (not found), so the
 * OSS build exposes no marketing surface.
 */
export function isMarketingRouteEnabled(edition: Edition, pathname: string): boolean {
  return publicMarketingPathOf(pathname) !== null && isEditionMarketingPath(edition, pathname);
}

/** Whether the edition redirects its signed-out root (CLOUD-001). */
export function ownsPublicRoot(edition: Edition): boolean {
  return isEditionRootEnabled(edition);
}
