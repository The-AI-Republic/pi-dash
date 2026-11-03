// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Prompts page tabs (parity/specs/agents.md §6-7): the sections browser
// and the assembled receipts. Tab state stays local to the page, so a
// refresh opens on the sections.

import * as React from "react";

export const PROMPT_TABS = ["sections", "receipt"] as const;

export type PromptTab = (typeof PROMPT_TABS)[number];

const TAB_LABELS: Record<PromptTab, string> = { sections: "Sections", receipt: "Receipt" };

export interface PromptsTabsProps {
  active: PromptTab;
  onSelect: (tab: PromptTab) => void;
}

export function PromptsTabs({ active, onSelect }: PromptsTabsProps): React.ReactElement {
  return (
    <div role="tablist" aria-label="Prompts views" className="flex gap-(--space-1) border-b border-(--border)">
      {PROMPT_TABS.map((tab) => {
        const selected = tab === active;
        return (
          <button
            key={tab}
            type="button"
            role="tab"
            aria-selected={selected}
            onClick={() => onSelect(tab)}
            className={`text-body px-(--space-3) py-(--space-2) ${
              selected
                ? "border-b-2 border-(--accent) text-(--text)"
                : "border-b-2 border-transparent text-(--text-muted) hover:text-(--text)"
            }`}
          >
            {TAB_LABELS[tab]}
          </button>
        );
      })}
    </div>
  );
}
