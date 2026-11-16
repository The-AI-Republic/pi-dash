// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The workflow-state marker shared by list headers and board columns: a
// small disc in the state's colour with a glyph matching the state group,
// never the sole carrier of the header's meaning.

import { Ban, CheckCircle2, Circle, CircleDashed, CircleDot, Eye, FlaskConical, LoaderCircle } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import * as React from "react";

import type { SpaceBoardState } from "@pidash/api-client";

const GROUP_GLYPHS: Record<string, LucideIcon> = {
  backlog: CircleDashed,
  unstarted: Circle,
  started: LoaderCircle,
  review: Eye,
  test: FlaskConical,
  completed: CheckCircle2,
  cancelled: Ban,
  triage: CircleDot,
};

export interface StateMarkerProps {
  state: Pick<SpaceBoardState, "name" | "color" | "group">;
  size?: number;
}

export function StateMarker({ state, size = 12 }: StateMarkerProps): React.ReactElement {
  const Glyph = GROUP_GLYPHS[state.group] ?? CircleDashed;
  return (
    <span
      data-testid="space-state-marker"
      aria-hidden="true"
      title={state.name}
      className="inline-flex shrink-0 items-center justify-center"
      style={{ color: state.color }}
    >
      <Glyph size={size} strokeWidth={2} className="text-current" />
    </span>
  );
}
