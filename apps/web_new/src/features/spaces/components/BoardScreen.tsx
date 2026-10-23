// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One published board (SPC-001, SPC-003, SPC-004, SPC-016). Settings are
// the gate: while they load the navbar placeholder and an empty body show;
// a 404 swaps to the anchor illustration; any other failure swaps to the
// board-error illustration. Once settings resolve, issues load under the
// real navbar and their own spinner. Every branch mounts the board's
// document head (SPC-007) so the tags settle before content is announced.

import { useQuery } from "@tanstack/react-query";
import type { SpaceBoardSettings } from "@pidash/api-client";
import * as React from "react";

import { spaceRuntime } from "../api/client.js";
import { spaceIssuesQueryOptions, spaceSettingsQueryOptions, spaceStatesQueryOptions } from "../api/queries.js";
import { classifySpaceFailure } from "../lib/errors.js";
import { isValidAnchor, type SpaceBoardSearch } from "../lib/address.js";
import { BoardIssues, BoardLoading, BoardNavbar } from "./BoardShell.js";
import { SpaceAnchorNotFound, SpaceBoardError } from "./FailureSurfaces.js";
import { SpaceBoardHead } from "./SpaceBoardHead.js";

export interface BoardScreenProps {
  anchor: string;
  search: SpaceBoardSearch;
}

export function BoardScreen({ anchor, search }: BoardScreenProps): React.ReactElement {
  if (!isValidAnchor(anchor)) {
    return (
      <>
        <SpaceBoardHead anchor={anchor} />
        <SpaceAnchorNotFound />
      </>
    );
  }
  return <ValidAnchorBoard anchor={anchor} search={search} />;
}

function ValidAnchorBoard({ anchor, search }: BoardScreenProps): React.ReactElement {
  const settings = useQuery(spaceSettingsQueryOptions(spaceRuntime.client, anchor));
  if (settings.isError) {
    return (
      <>
        <SpaceBoardHead anchor={anchor} />
        {classifySpaceFailure(settings.error) === "not-found" ? <SpaceAnchorNotFound /> : <SpaceBoardError />}
      </>
    );
  }
  if (settings.data === undefined) {
    return (
      <>
        <SpaceBoardHead anchor={anchor} />
        <div data-testid="space-board">
          <BoardNavbar settings={undefined} anchor={anchor} search={search} />
          <div data-testid="space-board-placeholder" className="min-h-[60vh]" />
        </div>
      </>
    );
  }
  return (
    <>
      <SpaceBoardHead anchor={anchor} />
      <ResolvedBoard anchor={anchor} search={search} settings={settings.data} />
    </>
  );
}

function ResolvedBoard({
  anchor,
  search,
  settings,
}: {
  anchor: string;
  search: SpaceBoardSearch;
  settings: SpaceBoardSettings;
}): React.ReactElement {
  const issues = useQuery(spaceIssuesQueryOptions(spaceRuntime.client, anchor));
  const states = useQuery(spaceStatesQueryOptions(spaceRuntime.client, anchor));
  const identifier = settings.project_details?.identifier ?? "";
  return (
    <div data-testid="space-board">
      <BoardNavbar settings={settings} anchor={anchor} search={search} />
      {issues.isError || states.isError ? (
        <SpaceBoardError />
      ) : issues.data === undefined || states.data === undefined ? (
        <BoardLoading />
      ) : (
        <BoardIssues page={issues.data} states={states.data} projectIdentifier={identifier} />
      )}
    </div>
  );
}
