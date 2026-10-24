// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Detail gone-away gate (parity/specs/modules.md §8.1, MOD-029). Wraps
// the module detail content: a missing, deleted, or
// archived-via-live-address module renders the gone-away view instead
// of the page, with a working route back to the list.

import { useNavigate } from "@tanstack/react-router";
import { useQuery } from "@tanstack/react-query";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { isModuleGoneAway } from "../lib/moduleDetail.js";
import { modulesListHref } from "../lib/moduleList.js";
import { moduleDetailQueryOptions } from "./sidebar/moduleSidebarQueries.js";
import { ModuleGoneAway } from "./ModuleGoneAway.js";

export interface ModuleDetailGateProps {
  workspaceSlug: string;
  projectId: string;
  moduleId: string;
  children: React.ReactNode;
}

export function ModuleDetailGate({
  workspaceSlug,
  projectId,
  moduleId,
  children,
}: ModuleDetailGateProps): React.ReactElement {
  const navigate = useNavigate();
  const detail = useQuery(moduleDetailQueryOptions(getAppClient(), workspaceSlug, projectId, moduleId));
  if (isModuleGoneAway(detail.error, detail.data?.archived_at)) {
    return (
      <ModuleGoneAway
        onBack={() => {
          void navigate({ to: modulesListHref(workspaceSlug, projectId) });
        }}
      />
    );
  }
  return <>{children}</>;
}
