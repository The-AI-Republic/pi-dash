// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public API of the spaces feature. Other features import only from here;
// each slice adds its own exports and keeps the other slices' lines.

export * from "./auth/index.js";
export * from "./lib/boardHead.js";
export * from "./lib/returnPath.js";
export { LandingGate, LandingGuidance, spaceLandingCapabilities } from "./components/LandingGate.js";
export type { LandingGateProps, LandingGuidanceProps } from "./components/LandingGate.js";
export { SpaceBoardHead } from "./components/SpaceBoardHead.js";
export { SpaceProjectMark, resolveSpaceMarkKind } from "./components/SpaceProjectMark.js";
export type { SpaceMarkKind } from "./components/SpaceProjectMark.js";
export { SpaceThemeSwitch } from "./components/SpaceThemeSwitch.js";
export { DEFAULT_SPACE_BADGE_URL, resolveSpaceBadgeUrl, SpaceProductBadge } from "./components/SpaceProductBadge.js";
export type { SpaceProductBadgeProps } from "./components/SpaceProductBadge.js";
export { SPACE_SIGN_OUT_ACTION, SpaceAccountMenu } from "./components/SpaceAccountMenu.js";
export type { SpaceAccountMenuProps } from "./components/SpaceAccountMenu.js";
export { SpaceAccountSlot } from "./components/SpaceAccountSlot.js";
export type { SpaceAccountSlotProps } from "./components/SpaceAccountSlot.js";
export { SpaceIssuePeek } from "./peek/SpaceIssuePeek.js";
export type { SpaceIssuePeekProps } from "./peek/SpaceIssuePeek.js";
export {
  SPACE_PEEK_PRESENTATIONS,
  SpacePeekModeSwitch,
  spacePeekPresentationLabel,
} from "./peek/SpacePeekModeSwitch.js";
export type { SpacePeekModeSwitchProps, SpacePeekPresentation } from "./peek/SpacePeekModeSwitch.js";
export { SpaceOverlayProperties, spaceOverlayPriority, spaceOverlayState } from "./peek/SpaceOverlayProperties.js";
export type { SpaceOverlayPropertiesProps } from "./peek/SpaceOverlayProperties.js";
export { useClipboardWritePermission } from "./lib/clipboard.js";
export {
  formatSpacePropertyDate,
  isSpaceDueOverdue,
  isSpaceIssueId,
  spacePeekCloseHref,
  spacePeekOpenHref,
} from "./lib/peek.js";
export { SpaceLandingAccount } from "./components/SpaceLandingAccount.js";
export type { SpaceLandingAccountProps } from "./components/SpaceLandingAccount.js";
export {
  SPACE_PROGRESS_DELAY_MS,
  SpaceProgressProvider,
  SpaceRouteProgress,
  useSpaceProgress,
} from "./components/SpaceRouteProgress.js";
export { SpaceLocaleGate } from "./components/SpaceLocaleGate.js";
export { spaceAccountLabel } from "./lib/spaceAccount.js";
export type { SpaceAccountIdentity } from "./lib/spaceAccount.js";
export { isEmbeddedWindow, readSpaceEmbedded, useSpaceEmbedded } from "./lib/spaceEmbed.js";
export type { FramedWindow } from "./lib/spaceEmbed.js";
export { healSpaceLocale, storedSpaceLocaleNeedsHeal, useSpaceLocaleTag } from "./lib/spaceLocale.js";
export {
  registerSpaceMessages,
  resetSpaceMessagesForTests,
  SPACE_MESSAGE_KEYS,
  spaceMessage,
} from "./lib/spaceMessages.js";
export type { SpaceMessageKey, SpaceMessagePack } from "./lib/spaceMessages.js";
export {
  notifySpaceAttachmentFailure,
  notifySpaceCommentDeleteFailure,
  notifySpaceCommentDeleteSuccess,
  notifySpaceCommentEditFailure,
  notifySpaceCommentFailure,
  notifySpaceCopyFailure,
  notifySpaceCopySuccess,
  notifySpaceReactionFailure,
  notifySpaceVoteFailure,
} from "./lib/spaceNotices.js";
export {
  canWatchSystemTheme,
  readStoredSpaceTheme,
  resolveSpaceTheme,
  SPACE_THEME_STORAGE_KEY,
  useSpaceTheme,
  writeStoredSpaceTheme,
} from "./lib/spaceTheme.js";
export type { SpaceTheme } from "./lib/spaceTheme.js";
