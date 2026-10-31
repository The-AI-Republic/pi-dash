// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One group's header: marker, state name and the server's count for that
// group (SPC-023/024). Empty groups keep the same header with a zero.

import * as React from "react";

import type { SpaceBoardState } from "@pidash/api-client";

import { StateMarker } from "./StateMarker.js";

export interface GroupHeaderProps {
  state: Pick<SpaceBoardState, "name" | "color" | "group">;
  count: number;
  /** Present on list headers: toggles the rows beneath. */
  onToggle?: (() => void) | undefined;
  expanded?: boolean | undefined;
}

export function GroupHeader({ state, count, onToggle, expanded }: GroupHeaderProps): React.ReactElement {
  const body = (
    <>
      <StateMarker state={state} />
      <span data-testid="space-group-name" className="text-emphasis min-w-0 flex-1 truncate text-(--text)">
        {state.name}
      </span>
      <span data-testid="space-group-count" className="text-meta text-(--text-muted) tabular-nums">
        {count}
      </span>
    </>
  );
  if (onToggle === undefined) {
    return <div className="flex items-center gap-(--space-2) px-(--space-2) py-(--space-1)">{body}</div>;
  }
  return (
    <button
      type="button"
      aria-expanded={expanded === true}
      data-testid="space-list-group-header"
      onClick={onToggle}
      className="flex w-full cursor-pointer items-center gap-(--space-2) rounded-(--radius-control) px-(--space-2) py-(--space-1) text-left hover:bg-(--subtle)"
    >
      <svg
        aria-hidden="true"
        viewBox="0 0 12 12"
        width={10}
        height={10}
        className={
          expanded === true
            ? "rotate-90 text-(--text-muted) transition-transform"
            : "text-(--text-muted) transition-transform"
        }
      >
        <path d="M4 2l4 4-4 4" fill="none" stroke="currentColor" strokeWidth="1.6" />
      </svg>
      {body}
    </button>
  );
}
