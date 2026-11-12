// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Navbar account slot (parity/specs/space.md SPC-013, SPC-014, SPC-015).
// One control per visitor state: nothing inside an embedding frame,
// a sign-in link carrying the full board position for an anonymous
// visitor, and the account menu for a signed-in one. While identity
// is still loading the slot offers sign-in — the anonymous shape —
// and swaps to the menu when a session lands; the board never waits
// on it.
import { useQuery } from "@tanstack/react-query";
import * as React from "react";

import { spaceRuntime } from "../api/client.js";
import { spaceViewerQueryOptions } from "../api/queries.js";
import type { SpaceAuthServices } from "../auth/index.js";
import { spaceReturnPath, spaceSignInHref, type SpaceBoardSearch } from "../lib/address.js";
import { useSpaceEmbedded } from "../lib/spaceEmbed.js";
import { spaceMessage } from "../lib/spaceMessages.js";
import { SpaceAccountMenu } from "./SpaceAccountMenu.js";

export interface SpaceAccountSlotProps {
  anchor: string;
  search: SpaceBoardSearch;
  /** CSRF reader for the menu; a public-safe default client when omitted. */
  services?: SpaceAuthServices | undefined;
}

export function SpaceAccountSlot({ anchor, search, services }: SpaceAccountSlotProps): React.ReactElement | null {
  const embedded = useSpaceEmbedded();
  const viewer = useQuery(spaceViewerQueryOptions(spaceRuntime.client));
  if (embedded) return null;
  if (viewer.data === undefined || viewer.data === null) {
    return (
      <a
        href={spaceSignInHref(anchor, search)}
        data-testid="space-navbar-signin"
        className="text-body rounded-(--radius-control) border border-(--border) px-(--space-3) py-(--space-1)"
      >
        {spaceMessage("account.signIn")}
      </a>
    );
  }
  return <SpaceAccountMenu viewer={viewer.data} returnPath={spaceReturnPath(anchor, search)} services={services} />;
}
