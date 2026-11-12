// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Transfer-hop note under the cycle row (ISS-236). Cloud-only: the
// edition supplies the component that knows where the item came from and
// when it moved; without a provider (and without the flag) the note
// renders nothing at all, so the cycle row reads as it always has.

import * as React from "react";

import { getEditionIssuesProviders } from "./providers.js";
import { showsDetailAccents } from "./badges.js";

export interface TransferHopNoteProps {
  workspaceSlug: string;
  projectId: string;
  issueId: string;
  cycleId: string | null;
}

export function TransferHopNote({
  workspaceSlug,
  projectId,
  issueId,
  cycleId,
}: TransferHopNoteProps): React.ReactElement | null {
  if (!showsDetailAccents() || cycleId === null) return null;
  const Note = getEditionIssuesProviders().detailBadges?.transferNote;
  if (Note === undefined) return null;
  return (
    <span data-testid="detail-transfer-hop">
      <Note workspaceSlug={workspaceSlug} projectId={projectId} issueId={issueId} cycleId={cycleId} />
    </span>
  );
}
