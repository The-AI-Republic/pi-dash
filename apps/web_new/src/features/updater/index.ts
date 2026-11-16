// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public API of the desktop updater feature (DESK-016–018). The workspace
// shell mounts only `DesktopUpdateControl` through its sidebar slot; the
// rest is the feature's own surface and its tests' seam.

export {
  DesktopUpdateControl,
  DesktopUpdateControlView,
  UPDATE_CONTROL_LABEL,
  UPDATE_RETRY_LABEL,
  updateControlName,
  updateInstallingHint,
  updateReadyHint,
} from "./DesktopUpdateControl.js";
export type { DesktopUpdateControlViewProps } from "./DesktopUpdateControl.js";
export { installFailureText, UPDATE_FAILED_FALLBACK_TEXT, UPDATE_NO_RESTART_TEXT, useUpdaterState } from "./state.js";
export type { UpdaterState } from "./state.js";
