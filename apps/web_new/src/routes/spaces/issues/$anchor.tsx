// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Published board address (SPC-001). The search schema settles only the
// board parameters this slice consumes; unknown values fall back without
// throwing. Pasted slash-less addresses settle on the canonical
// trailing-slash form on the first client navigation, keeping their search
// and fragment. The full view/filter contract belongs to slices E/F and
// mounts onto this route.

import { createFileRoute, redirect } from "@tanstack/react-router";
import * as React from "react";

import { BoardScreen } from "../../../features/spaces/components/BoardScreen.js";
import { parseSpaceBoardSearch, type SpaceBoardSearch } from "../../../features/spaces/lib/address.js";

export const Route = createFileRoute("/spaces/issues/$anchor")({
  validateSearch: parseSpaceBoardSearch,
  beforeLoad: ({ location }) => {
    if (!location.pathname.endsWith("/")) {
      const hash = location.hash === "" ? "" : `#${location.hash}`;
      throw redirect({ href: `${location.pathname}/${location.searchStr}${hash}` });
    }
  },
  component: function SpaceBoardRoute() {
    const { anchor } = Route.useParams();
    const search = Route.useSearch();
    return <BoardScreen anchor={anchor} search={search} />;
  },
});

export type { SpaceBoardSearch };
