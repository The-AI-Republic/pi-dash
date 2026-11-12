// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public API of the desktop About feature (DESK-028). The workspace shell
// mounts only `DesktopAboutControl` through its sidebar slot; the rest is
// the feature's own surface and its tests' seam.

export {
  ABOUT_CONTROL_LABEL,
  ABOUT_LICENSING_SUMMARY,
  ABOUT_RETRY_LABEL,
  AboutDialogView,
  aboutLoadFailureText,
  aboutVersionText,
  DesktopAboutControl,
  DesktopAboutControlView,
  noticeHeading,
  noticeMissingText,
  useAboutLoader,
} from "./DesktopAboutControl.js";
export type { AboutDialogViewProps, AboutLoadState, DesktopAboutControlViewProps } from "./DesktopAboutControl.js";
