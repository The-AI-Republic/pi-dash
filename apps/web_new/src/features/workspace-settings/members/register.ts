// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Members section registration (WSET-001/008, NEWFRONT-537). The shell's
// page registry owns which body answers `settings/members`; importing this
// chunk puts the roster body there. The exported helper lets tests and the
// route re-establish that registration after a registry reset.

import { registerWorkspaceSettingsPage } from "../lib/registry.js";
import { MembersPage } from "./MembersPage.js";

/** Registers the roster body under the members section. */
export function registerMembersSettingsPage(): () => void {
  return registerWorkspaceSettingsPage("members", MembersPage);
}

registerMembersSettingsPage();
