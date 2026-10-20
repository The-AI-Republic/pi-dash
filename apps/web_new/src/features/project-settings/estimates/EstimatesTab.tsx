// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Estimates tab body (PSET-049). The settings entry is a thin shell: the
// page gate admits project admins, and the shared estimates surface below
// owns its own loader, empty state, rows and enable switch.

import * as React from "react";

import { EstimatesScreen } from "../../estimates/index.js";
import { useProjectSettings } from "../components/ProjectSettingsLayout.js";
import { isAdminStanding } from "../tabs.js";

export function EstimatesTab(): React.ReactElement {
  const { workspaceSlug, projectId, role } = useProjectSettings();
  return <EstimatesScreen workspaceSlug={workspaceSlug} projectId={projectId} canManage={isAdminStanding(role)} />;
}
