// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public API of the spaces auth card. The landing gate (slice B) and the
// board's sign-in entries (slice D) mount SpaceAuthCard from here.

export { SpaceAuthCard, SPACE_AUTH_RESEND_COOLDOWN_SECONDS } from "./SpaceAuthCard.js";
export type { SpaceAuthCardProps, SpaceAuthInstanceCapabilities } from "./SpaceAuthCard.js";
export {
  initialSpaceAuthState,
  routeAfterSpaceEmailCheck,
  spaceAuthReducer,
  SPACE_AUTH_NO_METHOD_MESSAGE,
} from "./spaceAuthMachine.js";
export type {
  SpaceAuthBanner,
  SpaceAuthCapabilities,
  SpaceAuthCheckAnswer,
  SpaceAuthMachineEvent,
  SpaceAuthMachineState,
  SpaceAuthMode,
  SpaceAuthStep,
} from "./spaceAuthMachine.js";
export {
  bannerForSpaceAuthFailure,
  describeSpaceAuthCode,
  SPACE_AUTH_REQUEST_FAILED_MESSAGE,
} from "./spaceAuthErrors.js";
export type { SpaceAuthCodeMapping } from "./spaceAuthErrors.js";
export {
  isPlausibleSpaceEmail,
  isStrongSpacePassword,
  spacePasswordCriteria,
  SPACE_PASSWORD_MIN_LENGTH,
} from "./spaceAuthValidation.js";
export type { SpacePasswordCriterion, SpacePasswordCriterionId } from "./spaceAuthValidation.js";
export { resolveSpaceAuthProviders } from "./spaceAuthProviders.js";
export type { SpaceAuthProvider, SpaceAuthProviderFlags, SpaceAuthProviderId } from "./spaceAuthProviders.js";
export {
  createDefaultSpaceAuthServices,
  createSpaceAuthServices,
  spaceAuthFailureCode,
  useSpaceCsrfToken,
} from "./spaceAuthServices.js";
export type { SpaceAuthServices, SpaceCsrfToken } from "./spaceAuthServices.js";
