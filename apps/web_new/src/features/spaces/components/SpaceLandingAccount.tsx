// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Landing guidance account control (parity/specs/space.md SPC-005,
// SPC-014). The signed-in guidance view carries the same account menu
// as the board navbar — avatar plus one name line, opening the
// sign-out-only form — so a visitor who lands here can still sign out.
// Its return position is the landing root echoing its own query. The
// control renders nothing inside an embedding frame and nothing for an
// anonymous or still-loading visitor; the gate only mounts it on the
// guidance view, where the visitor is signed in.
import { useQuery } from "@tanstack/react-query";
import * as React from "react";

import { spaceRuntime } from "../api/client.js";
import { spaceViewerQueryOptions } from "../api/queries.js";
import type { SpaceAuthServices } from "../auth/index.js";
import { spaceLandingReturnPath, type SpaceLandingSearch } from "../lib/returnPath.js";
import { useSpaceEmbedded } from "../lib/spaceEmbed.js";
import { SpaceAccountMenu } from "./SpaceAccountMenu.js";

export interface SpaceLandingAccountProps {
  search: SpaceLandingSearch;
  /** CSRF reader for the menu; a public-safe default client when omitted. */
  services?: SpaceAuthServices | undefined;
}

export function SpaceLandingAccount({ search, services }: SpaceLandingAccountProps): React.ReactElement | null {
  const embedded = useSpaceEmbedded();
  const viewer = useQuery(spaceViewerQueryOptions(spaceRuntime.client));
  if (embedded) return null;
  if (viewer.data === undefined || viewer.data === null) return null;
  return <SpaceAccountMenu viewer={viewer.data} returnPath={spaceLandingReturnPath(search)} services={services} />;
}
