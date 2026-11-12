// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Account trigger label (parity/specs/space.md SPC-014, NEWFRONT-823).
// Display name wins, then first plus last name, then the address, then
// a neutral translated label. The old trigger repeated the first name
// when no display name existed; that duplication is a bug the new app
// does not rebuild.
import type { Me } from "@pidash/api-client";

import { spaceMessage } from "./spaceMessages.js";

export type SpaceAccountIdentity = Pick<Me, "display_name" | "first_name" | "last_name" | "email">;

/** The single name line on the navbar account trigger. */
export function spaceAccountLabel(identity: SpaceAccountIdentity): string {
  const display = identity.display_name.trim();
  if (display !== "") return display;
  const names = [identity.first_name.trim(), identity.last_name.trim()].filter((part) => part !== "");
  if (names.length > 0) return names.join(" ");
  const email = identity.email.trim();
  if (email !== "") return email;
  return spaceMessage("account.fallbackName");
}
