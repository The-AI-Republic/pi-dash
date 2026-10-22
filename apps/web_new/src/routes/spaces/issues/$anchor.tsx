// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Published board address (SPC-001). The search schema settles only the
// board parameters this slice consumes; unknown values fall back without
// throwing. Address rewriting and the full view/filter contract belong to
// slices E/F and mount onto this route.

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { BoardScreen } from "../../../features/spaces/components/BoardScreen.js";
import { parseSpaceBoardSearch, type SpaceBoardSearch } from "../../../features/spaces/lib/address.js";

export const Route = createFileRoute("/spaces/issues/$anchor")({
  validateSearch: parseSpaceBoardSearch,
  component: function SpaceBoardRoute() {
    const { anchor } = Route.useParams();
    const search = Route.useSearch();
    return <BoardScreen anchor={anchor} search={search} />;
  },
});

export type { SpaceBoardSearch };
