// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Role filter (parity/specs/project-settings-members.md §"Search, filter
// and sort"; PSET-020). A Filters button opens one checkbox per project
// role; checks narrow the table together, the panel stays open across
// picks, and a dot marks the button while any role is checked.

import { Button, Popover, cn } from "@pidash/kit";
import { Check, Filter } from "lucide-react";
import * as React from "react";

import { MEMBER_ROLE_OPTIONS, toggleMemberRole, type MemberRoleKey } from "./roster.js";

export interface MemberFiltersProps {
  roles: readonly MemberRoleKey[];
  onChange: (roles: MemberRoleKey[]) => void;
}

export function MemberFilters({ roles, onChange }: MemberFiltersProps): React.ReactElement {
  return (
    <span data-testid="member-filters" className="relative inline-flex">
      <Popover
        trigger={
          <Button variant="secondary" aria-label="Filters">
            <Filter size={14} strokeWidth={1.5} aria-hidden="true" />
            Filters
          </Button>
        }
      >
        <fieldset data-testid="member-filters-panel" className="flex flex-col gap-(--space-1)">
          <legend className="text-caption pb-(--space-2) font-medium text-(--text-muted)">Role</legend>
          {MEMBER_ROLE_OPTIONS.map((option) => {
            const checked = roles.includes(option.key);
            return (
              <label
                key={option.key}
                className={cn(
                  "text-body flex cursor-pointer items-center gap-(--space-3) rounded-(--radius-control) px-(--space-3) py-(--space-2) text-(--text) hover:bg-(--subtle)",
                  checked && "font-medium"
                )}
              >
                <input
                  type="checkbox"
                  checked={checked}
                  data-testid={`member-filter-${option.key}`}
                  onChange={() => onChange(toggleMemberRole(roles, option.key))}
                  className="size-4 accent-(--accent)"
                />
                <span className="flex-1">{option.label}</span>
                {checked ? <Check size={14} strokeWidth={2} aria-hidden="true" className="text-(--accent)" /> : null}
              </label>
            );
          })}
        </fieldset>
      </Popover>
      {roles.length > 0 ? (
        <span
          aria-label="Filters applied"
          data-testid="member-filter-active"
          className="pointer-events-none absolute -top-1 -right-1 size-2 rounded-full bg-(--accent)"
        />
      ) : null}
    </span>
  );
}
