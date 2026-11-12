// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The running app's version (SHELL-060, from parity/specs/shell.md
// §4): the help footer and the updates dialog read this single
// constant, which tracks the app package's own declared version.
import appPackage from "../../../package.json";

/** The app package's declared version (0.0.0 while unreleased). */
export const APP_VERSION: string = appPackage.version;
