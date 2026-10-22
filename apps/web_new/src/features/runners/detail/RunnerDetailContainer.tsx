// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runner detail container (NEWFRONT-555, RUN-042). Reads the area scope from
// context where available and the single runner by id; renders the screen
// with the read's progress. Route-level tests can mount the screen alone.

import * as React from "react";

import { useRunnerDetail } from "../api/detail.js";
import { RunnerDetailScreen } from "./RunnerDetailScreen.js";

export interface RunnerDetailContainerProps {
  workspaceSlug: string;
  runnerId: string;
  projectId?: string | undefined;
}

export function RunnerDetailContainer({
  workspaceSlug,
  runnerId,
  projectId,
}: RunnerDetailContainerProps): React.ReactElement {
  const detail = useRunnerDetail(workspaceSlug, runnerId);
  return (
    <RunnerDetailScreen
      workspaceSlug={workspaceSlug}
      runnerId={runnerId}
      projectId={projectId}
      runner={detail.data}
      loading={detail.isPending}
      failed={detail.isError && detail.data === undefined}
    />
  );
}
