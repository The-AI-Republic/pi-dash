// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Receipt view (parity/specs/agents.md §7, AGT-028-030). One card per
// agent kind with the ordered composition and the assembled template,
// a side nav listing the kinds with rendered counts and anchors, and a
// loading line or failure banner instead of a blank page (AGT-032).

import { Button } from "@pidash/kit";
import * as React from "react";

import { usePromptReceipts } from "../api/queries.js";
import { PROMPT_KIND_LABELS } from "../lib/sections.js";
import { receiptAnchorId, sectionCountLabel } from "../lib/receipt.js";
import { ReceiptCard } from "./ReceiptCard.js";

/** Loading line while the receipt reads resolve. */
export const RECEIPTS_LOADING_TEXT = "Loading…";
/** Banner when a receipt read fails. */
export const RECEIPTS_FAILED_TEXT = "Could not load prompt receipts for this workspace.";
const RECEIPTS_INTRO =
  "A receipt is the template an agent runs, assembled in order from the sections beneath it. Editing a section changes every receipt that includes it.";

export interface ReceiptViewProps {
  workspaceSlug: string;
  isAdmin: boolean;
}

export function ReceiptView({ workspaceSlug, isAdmin }: ReceiptViewProps): React.ReactElement {
  const receipts = usePromptReceipts(workspaceSlug);
  if (receipts.failed) {
    return (
      <div className="flex flex-col gap-(--space-4)">
        <p role="alert" className="text-body text-(--danger)">
          {RECEIPTS_FAILED_TEXT}
        </p>
        <div>
          <Button variant="secondary" onClick={receipts.refetch}>
            Try again
          </Button>
        </div>
      </div>
    );
  }
  if (receipts.pending) {
    return <p className="text-body text-(--text-muted)">{RECEIPTS_LOADING_TEXT}</p>;
  }
  return (
    <div className="flex flex-col gap-(--space-4)">
      <p className="text-body text-(--text-muted)">{RECEIPTS_INTRO}</p>
      <div className="grid grid-cols-1 gap-(--space-8) lg:grid-cols-[12rem_minmax(0,1fr)]">
        <nav
          aria-label="Prompt receipt"
          className="flex flex-col gap-(--space-1) lg:sticky lg:top-(--space-8) lg:max-h-[calc(100vh_-_var(--space-16))] lg:self-start lg:overflow-auto"
        >
          {receipts.entries.map((entry) => (
            <a
              key={entry.kind}
              href={`#${receiptAnchorId(entry.kind)}`}
              className="text-body flex flex-col rounded-(--radius-control) px-(--space-3) py-(--space-2) text-(--text) hover:bg-(--subtle)"
            >
              <span>{PROMPT_KIND_LABELS[entry.kind]}</span>
              <span className="text-meta text-(--text-muted)">{sectionCountLabel(entry.sections.sections.length)}</span>
            </a>
          ))}
        </nav>
        <div className="flex flex-col gap-(--space-4)">
          {receipts.entries.map((entry) => (
            <ReceiptCard key={entry.kind} workspaceSlug={workspaceSlug} receipt={entry} isAdmin={isAdmin} />
          ))}
        </div>
      </div>
    </div>
  );
}
