// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Detail gone-away gate (parity/specs/cycles.md §5 and §13, CYC-013).
// Wraps the cycle detail content: a missing, deleted, foreign, or
// archived-via-live-address cycle renders the gone-away view instead of
// the page, with a working route back to the list. Slice F keeps this
// mount inside the full detail host.

import { useNavigate } from "@tanstack/react-router";
import * as React from "react";

import { useCycleDetail } from "../api/cycleDetail.js";
import { cyclesListHref, isCycleGoneAway } from "../lib/cycleScreen.js";
import { CycleGoneAway } from "./CycleGoneAway.js";

export interface CycleDetailGateProps {
  workspaceSlug: string;
  projectId: string;
  cycleId: string;
  children: React.ReactNode;
}

export function CycleDetailGate({
  workspaceSlug,
  projectId,
  cycleId,
  children,
}: CycleDetailGateProps): React.ReactElement {
  const navigate = useNavigate();
  const detail = useCycleDetail(workspaceSlug, projectId, cycleId);
  if (isCycleGoneAway(detail.error, detail.data?.archived_at)) {
    return (
      <CycleGoneAway
        onBack={() => {
          void navigate({ to: cyclesListHref(workspaceSlug, projectId) });
        }}
      />
    );
  }
  return <>{children}</>;
}
