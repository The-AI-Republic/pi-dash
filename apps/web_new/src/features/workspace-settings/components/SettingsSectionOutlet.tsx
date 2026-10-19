// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Section outlet (parity/specs/workspace-settings.md §1, WSET-001/036).
// The shell owns the frame, gate and addresses; a section route only
// asks the page registry for its body. Until that slice registers, the
// outlet itself shows the shell's quiet placeholder, so the route stays
// honest and the slice changes nothing in the shell.

import * as React from "react";

import { useWorkspaceSettingsPage } from "../lib/registry.js";
import type { WorkspaceSettingsSectionKey } from "../lib/sections.js";
import { SettingsSectionPlaceholder } from "./SettingsSectionPlaceholder.js";

export interface SettingsSectionOutletProps {
  workspaceSlug: string;
  section: WorkspaceSettingsSectionKey;
}

export function SettingsSectionOutlet({ workspaceSlug, section }: SettingsSectionOutletProps): React.ReactElement {
  const Page = useWorkspaceSettingsPage(section);
  return Page === null ? <SettingsSectionPlaceholder /> : <Page workspaceSlug={workspaceSlug} />;
}
