// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Role filter (parity/specs/workspace-settings.md §7, WSET-008). Four
// multi-select toggles — Admin, Member, Guest and Suspended — that narrow
// the roster together with the search. A toggle stays open across picks,
// and an applied filter paints a dot on the trigger.

import { Button, Popover, cn } from "@pidash/kit";
import { Check, Filter } from "lucide-react";
import * as React from "react";

import { ROSTER_ROLE_FILTERS, toggleRoleFilter, type RosterRoleFilter } from "./roster.js";

export interface MembersFiltersProps {
  roles: readonly RosterRoleFilter[];
  onChange: (roles: RosterRoleFilter[]) => void;
}

export function MembersFilters({ roles, onChange }: MembersFiltersProps): React.ReactElement {
  return (
    <span data-testid="members-filters" className="relative inline-flex">
      <Popover
        trigger={
          <Button variant="secondary" aria-label="Filters">
            <Filter size={14} strokeWidth={1.5} aria-hidden="true" />
            Filters
          </Button>
        }
      >
        <div data-testid="members-filters-panel" className="flex flex-col gap-(--space-1)">
          {ROSTER_ROLE_FILTERS.map((option) => {
            const checked = roles.includes(option.key);
            return (
              <button
                key={option.key}
                type="button"
                aria-pressed={checked}
                data-testid={`members-filter-${option.key}`}
                onClick={() => onChange(toggleRoleFilter(roles, option.key))}
                className={cn(
                  "text-body flex w-full cursor-pointer items-center justify-between gap-(--space-3) rounded-(--radius-control) px-(--space-3) py-(--space-2) text-left text-(--text) hover:bg-(--subtle)",
                  checked && "font-medium"
                )}
              >
                {option.label}
                {checked ? <Check size={14} strokeWidth={2} aria-hidden="true" className="text-(--accent)" /> : null}
              </button>
            );
          })}
        </div>
      </Popover>
      {roles.length > 0 ? (
        <span
          aria-label="Filters applied"
          data-testid="members-filter-active"
          className="pointer-events-none absolute -top-1 -right-1 size-2 rounded-full bg-(--accent)"
        />
      ) : null}
    </span>
  );
}
