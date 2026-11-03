// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Receipt card (parity/specs/agents.md §7, AGT-028-030). Collapsed it
// numbers the ordered composition with provenance marks and a rendered
// section count; expanded it adds the assembled template, the
// automatic-run variant when the viewer holds overrides, and, for
// admins, the render-against-real-data form.

import { Badge } from "@pidash/kit";
import * as React from "react";

import { sourceBadgeFor, PROMPT_KIND_LABELS } from "../lib/sections.js";
import {
  automaticTemplate,
  receiptAnchorId,
  receiptSlots,
  sectionCountLabel,
  type ReceiptSlot,
} from "../lib/receipt.js";
import type { PromptReceiptEntry } from "../api/queries.js";
import { PreviewPanel } from "./PreviewPanel.js";

/** Label on the contrast block automatic runs receive. */
export const AUTOMATIC_RUNS_LABEL = "Automatic runs ignore personal overrides and get this assembly instead.";

export interface ReceiptCardProps {
  workspaceSlug: string;
  receipt: PromptReceiptEntry;
  /** The viewer administers the workspace: saved previews show. */
  isAdmin: boolean;
}

function Composition({ slots }: { slots: ReceiptSlot[] }): React.ReactElement {
  return (
    <div className="flex flex-col gap-(--space-2)">
      <p className="text-meta text-(--text-muted)">Composed in order</p>
      <ol className="grid grid-cols-1 gap-(--space-2) md:grid-cols-2">
        {slots.map((slot) => (
          <li
            key={slot.key}
            className="flex items-start gap-(--space-2) rounded-(--radius-control) bg-(--subtle) px-(--space-2) py-(--space-1)"
          >
            <span className="text-meta font-mono text-(--text-muted)">{slot.num}</span>
            <span className="flex min-w-0 flex-1 flex-col">
              <span className="text-body truncate text-(--text)">{slot.title}</span>
              <span className="text-meta font-mono truncate text-(--text-muted)">{slot.key}</span>
            </span>
            <Badge tone="accent">{sourceBadgeFor(slot.source)}</Badge>
          </li>
        ))}
      </ol>
    </div>
  );
}

export function ReceiptCard({ workspaceSlug, receipt, isAdmin }: ReceiptCardProps): React.ReactElement {
  const { kind, sections, compiled } = receipt;
  const [open, setOpen] = React.useState(false);
  const slots = receiptSlots(sections.sections);
  const count = sectionCountLabel(slots.length);
  const automatic = automaticTemplate(compiled);
  return (
    <article id={receiptAnchorId(kind)} className="flex flex-col rounded-(--radius-control) border border-(--border)">
      <button
        type="button"
        aria-label={PROMPT_KIND_LABELS[kind]}
        aria-expanded={open}
        onClick={() => setOpen((current) => !current)}
        className="flex flex-wrap items-center gap-(--space-2) px-(--space-4) py-(--space-3) text-left hover:bg-(--subtle)"
      >
        <span className="flex min-w-0 flex-1 flex-col">
          <span className="text-h3 text-(--text)">{PROMPT_KIND_LABELS[kind]}</span>
          <span className="text-meta text-(--text-muted)">{count}</span>
        </span>
        <span data-testid="receipt-count">
          <Badge>{count}</Badge>
        </span>
        <span className="text-meta text-(--text-muted)">{open ? "Hide" : "Show"}</span>
      </button>
      <div className="flex flex-col gap-(--space-3) border-t border-(--border) px-(--space-4) py-(--space-3)">
        <Composition slots={slots} />
      </div>
      {open ? (
        <div className="flex flex-col gap-(--space-4) border-t border-(--border) px-(--space-4) py-(--space-3)">
          <div className="flex flex-col gap-(--space-2)">
            <p className="text-meta text-(--text-muted)">Assembled template</p>
            <pre
              data-testid={`receipt-template-${kind}`}
              className="text-body max-h-96 overflow-auto rounded-(--radius-control) border border-(--border) bg-(--bg) p-(--space-4) whitespace-pre-wrap text-(--text)"
            >
              {compiled.template_body.trimEnd()}
            </pre>
          </div>
          {automatic === null ? null : (
            <div className="flex flex-col gap-(--space-2)">
              <p className="text-meta text-(--text-muted)">{AUTOMATIC_RUNS_LABEL}</p>
              <pre
                data-testid={`receipt-automatic-${kind}`}
                className="text-body max-h-96 overflow-auto rounded-(--radius-control) border border-(--border) bg-(--subtle) p-(--space-4) whitespace-pre-wrap text-(--text)"
              >
                {automatic.trimEnd()}
              </pre>
            </div>
          )}
          {isAdmin ? (
            <PreviewPanel
              workspaceSlug={workspaceSlug}
              kind={kind}
              submitLabel="Preview"
              heading="Preview"
              testIdPrefix={`receipt-preview-${kind}`}
            />
          ) : null}
        </div>
      ) : null}
    </article>
  );
}
