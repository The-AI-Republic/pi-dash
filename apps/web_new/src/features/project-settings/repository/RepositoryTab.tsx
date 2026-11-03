// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Repository tab body (PSET-050–052). A member-visible status surface:
// the shared card reads the binding, and the admin-only switch and
// unbind disable for everyone else without hiding the card.

import * as React from "react";

import { useProjectSettings } from "../components/ProjectSettingsLayout.js";
import { isAdminStanding } from "../tabs.js";
import { RepositorySyncCard } from "./RepositorySyncCard.js";

export function RepositoryTab(): React.ReactElement {
  const { workspaceSlug, projectId, role } = useProjectSettings();
  return <RepositorySyncCard workspaceSlug={workspaceSlug} projectId={projectId} canManage={isAdminStanding(role)} />;
}
