// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Searchable timezone picker (shell spec, SHELL-097). Searches the full
// zone list by city, region and offset, marks the current selection, and
// reports picks to the caller (persistence lives with useUpdateTimezone).
// The profile timezone section mounts this; the palette's Change-timezone
// page renders the same options through its own page chrome.

import { Input, Spinner } from "@pidash/kit";
import * as React from "react";

import { filterTimezoneOptions, selectTimezoneOptions, useTimezones } from "./timezone.js";

export interface TimezonePickerProps {
  /** The currently selected zone value. */
  value: string;
  /** Fired when the user picks a zone. */
  onSelect: (value: string) => void;
}

export function TimezonePicker({ value, onSelect }: TimezonePickerProps): React.ReactElement {
  const zones = useTimezones();
  const [query, setQuery] = React.useState("");
  const options = React.useMemo(
    () => filterTimezoneOptions(selectTimezoneOptions(zones.data?.timezones ?? []), query),
    [zones.data, query]
  );

  if (zones.isPending) {
    return (
      <div role="status" aria-label="Loading timezones" className="flex justify-center p-(--space-4)">
        <Spinner />
      </div>
    );
  }
  if (zones.isError) {
    return (
      <p role="alert" className="text-body text-(--danger)">
        Couldn't load the timezone list.
      </p>
    );
  }
  return (
    <div className="flex min-h-0 flex-col gap-(--space-2)">
      <Input label="Search timezones" value={query} onChange={(event) => setQuery(event.target.value)} />
      {options.length === 0 ? (
        <p className="text-body text-(--text-muted)">No timezones match “{query}”.</p>
      ) : (
        <ul role="listbox" aria-label="Timezones" className="min-h-0 overflow-y-auto">
          {options.map((option) => {
            const selected = option.value === value;
            return (
              <li key={option.value}>
                <button
                  type="button"
                  role="option"
                  aria-selected={selected}
                  onClick={() => onSelect(option.value)}
                  className="text-body flex w-full items-center justify-between gap-(--space-3) rounded-(--radius-control) px-(--space-3) py-(--space-2) text-left text-(--text) hover:bg-(--subtle)"
                >
                  <span className="min-w-0">
                    <span className="block truncate font-medium">{option.label}</span>
                    <span className="block truncate text-(--text-muted)">{option.detail}</span>
                  </span>
                  {selected ? (
                    <span aria-hidden="true" className="shrink-0 text-(--accent)">
                      ✓
                    </span>
                  ) : null}
                </button>
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}
