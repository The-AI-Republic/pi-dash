// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Router glue for the legacy forwards (PROF-026): the target as router
// redirect options, or null when the address belongs to no legacy family.

import { legacySettingsForward } from "./legacyForwards.js";

export interface LegacyRedirectOptions {
  to: string;
  search: true;
}

/** Redirect options for a legacy settings address, or null. */
export function legacyRedirectOptions(pathname: string, search: string): LegacyRedirectOptions | null {
  const forward = legacySettingsForward(pathname, search);
  if (forward === null) return null;
  return { to: forward.href, search: true };
}
