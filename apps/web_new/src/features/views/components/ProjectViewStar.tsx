// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Favorite star on a project view row (parity/specs/views.md §6,
// VIEW-012). The button only flips; the container owns the standing
// check, the silent write and the pending state.

import { IconButton } from "@pidash/kit";
import { Star } from "lucide-react";
import * as React from "react";

export interface ProjectViewStarProps {
  viewName: string;
  favorited: boolean;
  /** True while this row's toggle is in flight. */
  pending: boolean;
  onToggle: () => void;
}

export function ProjectViewStar({ viewName, favorited, pending, onToggle }: ProjectViewStarProps): React.ReactElement {
  return (
    <IconButton
      label={favorited ? `Remove ${viewName} from favorites` : `Add ${viewName} to favorites`}
      size="small"
      data-testid="project-views-row-star"
      disabled={pending}
      aria-pressed={favorited}
      onClick={onToggle}
    >
      <Star size={16} strokeWidth={1.5} aria-hidden="true" fill={favorited ? "currentColor" : "none"} />
    </IconButton>
  );
}
