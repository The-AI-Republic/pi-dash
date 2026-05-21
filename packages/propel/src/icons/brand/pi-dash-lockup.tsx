/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import type { ISvgIcons } from "../type";
import { PiSymbol } from "./pi-symbol";

/**
 * Header / lockup brand mark.
 *
 * Aliased to ``PiSymbol`` so the project ships a single canonical Pi
 * mark — the same one the root README points at. Existing callsites
 * keep working unchanged; they render the Pi symbol instead of the
 * older "Pi Dash" wordmark.
 */
export function PiDashLockup(props: ISvgIcons) {
  return <PiSymbol {...props} />;
}
