// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

export { CrossAppLink } from "./CrossAppLink.js";
export type { CrossAppLinkProps } from "./CrossAppLink.js";
export {
  isMigratedPath,
  MIGRATED_RESERVED_TOP_LEVEL,
  MIGRATED_ROUTE_ENTRIES,
  normalizePathPrefix,
  resolveLinkKind,
} from "./migrated-routes.js";
export { classifyDesktopRoute, currentAddress, hostedAddressFor } from "./desktopHandOff.js";
export type { DesktopRouteVerdict } from "./desktopHandOff.js";
export {
  DesktopHostedScreen,
  HOSTED_SCREEN_FAILURE_HEADING,
  HOSTED_SCREEN_HEADING,
  HOSTED_SCREEN_MOVING,
  HOSTED_SCREEN_NO_ADDRESS,
  HOSTED_SCREEN_OPENED,
  HOSTED_SCREEN_REFUSED,
  HOSTED_SCREEN_RETRY,
  HOSTED_SCREEN_TITLE,
  hostedScreenAddressFor,
} from "./DesktopHostedScreen.js";
