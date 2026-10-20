// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Emblem picker (parity/specs/project-settings-general.md §PSET-007). A
// dialog of emoji choices in a searchable grid; a pick swaps the draft mark
// and dirties the form, and the normal save persists it. Artwork is only the
// mark or an uploaded image — there is no cover, gallery or provider search.

import { Button, Dialog, Input } from "@pidash/kit";
import * as React from "react";

import { markEmblem } from "../rules.js";
import { EMBLEM_CHOICES } from "./GeneralIdentity.js";

export interface EmblemPickerProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** Current draft mark, used to mark the chosen tile. */
  logoProps: Record<string, unknown>;
  onPick: (logoProps: Record<string, unknown>) => void;
}

/** Matching emoji for the query; an empty query shows the whole alphabet. */
export function matchEmoji(query: string): readonly string[] {
  const trimmed = query.trim();
  if (trimmed === "") return EMBLEM_CHOICES;
  return EMBLEM_CHOICES.filter((choice) => choice === trimmed);
}

export function EmblemPicker({ open, onOpenChange, logoProps, onPick }: EmblemPickerProps): React.ReactElement {
  const [query, setQuery] = React.useState("");
  const choices = matchEmoji(query);
  const stored = logoProps["in_use"] === "emoji" ? logoProps["emoji"] : undefined;
  const currentValue = typeof stored === "string" ? stored : (stored as Record<string, unknown> | undefined)?.["value"];
  return (
    <Dialog
      title="Change emblem"
      open={open}
      onOpenChange={(next) => {
        if (!next) setQuery("");
        onOpenChange(next);
      }}
      data-testid="general-emblem-picker"
    >
      <div className="flex flex-col gap-(--space-4)">
        <Input
          label="Search emoji"
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          placeholder="Type an emoji"
        />
        {choices.length === 0 ? (
          <p className="text-body text-(--text-muted)">No emoji match that search.</p>
        ) : (
          <ul aria-label="Emblems" className="grid grid-cols-6 gap-(--space-2)">
            {choices.map((emoji) => {
              const selected = currentValue === emoji;
              return (
                <li key={emoji}>
                  <button
                    type="button"
                    aria-label={emoji}
                    aria-pressed={selected}
                    data-testid="general-emblem-choice"
                    className={`text-h3 flex h-12 w-full items-center justify-center rounded-(--radius-control) border ${
                      selected ? "border-(--accent) bg-(--subtle)" : "border-(--border) hover:bg-(--subtle)"
                    }`}
                    onClick={() => {
                      onPick(markEmblem("emoji", emoji));
                      setQuery("");
                      onOpenChange(false);
                    }}
                  >
                    {emoji}
                  </button>
                </li>
              );
            })}
          </ul>
        )}
        <div className="flex justify-end">
          <Button variant="secondary" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
        </div>
      </div>
    </Dialog>
  );
}
