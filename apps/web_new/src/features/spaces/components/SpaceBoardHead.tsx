// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Board document head (SPC-007, SPC-008). Reads the board's display
// metadata through the public runtime — the same resolved API base every
// other space call uses, so the old NEWFRONT-453 fallback can never
// recur — and claims the live document's title, description, social tags
// and crawler directive. Renders nothing; the board mounts it in every
// branch (loading, resolved, unknown, failed) so the tags settle before
// the board content is announced. Anonymous loads get the same tags:
// the meta endpoint is public.

import { useQuery } from "@tanstack/react-query";
import * as React from "react";

import { useDocumentHead } from "../../../shared/shell/document.js";
import { spaceRuntime } from "../api/client.js";
import { spaceMetaQueryOptions } from "../api/queries.js";
import { buildSpaceBoardHead } from "../lib/boardHead.js";

export function SpaceBoardHead({ anchor }: { anchor: string }): React.ReactElement | null {
  const meta = useQuery(spaceMetaQueryOptions(spaceRuntime.client, anchor));
  const claim = buildSpaceBoardHead(meta.data);
  useDocumentHead({ title: claim.title, robots: claim.robots, tags: claim.tags });
  return null;
}
