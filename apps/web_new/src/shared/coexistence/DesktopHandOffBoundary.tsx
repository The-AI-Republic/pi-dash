// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// What the desktop build shows when it lands on an address it does not
// carry (M-01/NEWFRONT-92). This module is loaded only by the lazy split
// in the not-found page, so the classifier, the hand-off card and its kit
// UI stay out of the web build's first paint.
//
// The decision and the card stay together on purpose: an address the
// bundle owns gets the ordinary not-found page (an in-window answer), and
// one another frontend owns gets the card that opens it in the browser.

import * as React from "react";

import { platform } from "../../core/platform/index.js";
import { NotFoundNotice } from "../shell/NotFoundPage.js";
import { DesktopHostedScreen } from "./DesktopHostedScreen.js";
import { classifyDesktopRoute, currentAddress } from "./desktopHandOff.js";

/**
 * The surface for an unknown address on the desktop.
 *
 * The card mounts only for a hand-off verdict; every other address keeps
 * the in-window not-found page, the same answer the web build gives.
 */
export function DesktopHandOffBoundary(): React.ReactElement {
  if (platform.kind !== "desktop") return <NotFoundNotice />;
  if (classifyDesktopRoute(currentAddress()).kind !== "hand-off") return <NotFoundNotice />;
  return <DesktopHostedScreen />;
}
