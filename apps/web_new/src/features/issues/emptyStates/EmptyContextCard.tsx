// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One context empty-state card (parity/specs/issues.md §8, ISS-068–074).
// Every issues-bearing list renders through the kit's detailed empty-state
// tier, so the cards share one layout, one type scale and one button
// rhythm. A card names its context for tests and for the surroundings that
// pick between the first-run and filtered variants, and it offers at most
// the two entries the detailed tier holds: a primary first, a secondary
// next. Callers keep every action visible and pass `disabled` for guests.

import { EmptyState } from "@pidash/kit";
import * as React from "react";

export interface EmptyContextAction {
  label: string;
  onSelect: () => void;
  /** Guests and read-only standings see the same copy with a dead control. */
  disabled?: boolean | undefined;
}

export interface EmptyContextCardProps {
  /** Stable context name, e.g. "project" or "cycle-completed". */
  context: string;
  title: string;
  description: string;
  artwork: React.ReactNode;
  /** Primary entry first; the second entry renders as the secondary style. */
  actions?: readonly EmptyContextAction[] | undefined;
}

export function EmptyContextCard({
  context,
  title,
  description,
  artwork,
  actions = [],
}: EmptyContextCardProps): React.ReactElement {
  const [primary, secondary] = actions;
  return (
    <div data-testid="issues-empty-state" data-context={context} className="flex h-full w-full justify-center">
      <EmptyState
        variant="detailed"
        art={artwork}
        title={title}
        description={description}
        {...(primary === undefined
          ? null
          : {
              primaryAction: { label: primary.label, onSelect: primary.onSelect, disabled: primary.disabled === true },
            })}
        {...(secondary === undefined
          ? null
          : {
              secondaryAction: {
                label: secondary.label,
                onSelect: secondary.onSelect,
                disabled: secondary.disabled === true,
              },
            })}
      />
    </div>
  );
}
