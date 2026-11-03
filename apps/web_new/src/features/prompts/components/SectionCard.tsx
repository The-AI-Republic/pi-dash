// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Section card (parity/specs/agents.md §6, AGT-023, AGT-033). One card
// per registry key: title, key, provenance badge, governance and kind
// marks, the stale-override warning, the effective body, and the edit
// controls the viewer's tier and role allow. Opening the editor swaps
// the body for the editor at that scope.

import { Badge, Button } from "@pidash/kit";
import * as React from "react";

import {
  editLabelsFor,
  PROMPT_KIND_LABELS,
  sourceBadgeFor,
  tierMarkFor,
  type MergedPromptSection,
  type PromptEditorScope,
  type PromptViewerGates,
} from "../lib/sections.js";
import { SectionEditor } from "./SectionEditor.js";

/** Stale-override warning, shown when the default moved past the override. */
export const STALE_OVERRIDE_WARNING = "The default changed after this override was saved, so it may be stale.";

export interface SectionCardProps {
  workspaceSlug: string;
  section: MergedPromptSection;
  viewer: PromptViewerGates;
  /** The scope under edit on this card, or null when the body shows. */
  editorScope: PromptEditorScope | null;
  onEdit: (scope: PromptEditorScope) => void;
  onCloseEditor: () => void;
}

export function SectionCard({
  workspaceSlug,
  section,
  viewer,
  editorScope,
  onEdit,
  onCloseEditor,
}: SectionCardProps): React.ReactElement {
  const labels = editLabelsFor(section, viewer);
  const tierMark = section.user === null ? null : tierMarkFor(section.user.customizable);
  const stale = section.user?.needs_attention ?? false;
  return (
    <article id={`prompt-section-${section.key}`} className="flex flex-col gap-(--space-4)">
      <div className="flex flex-wrap items-start justify-between gap-(--space-3)">
        <div className="flex flex-col gap-(--space-1)">
          <h2 className="text-h3 text-(--text)">{section.title}</h2>
          <p className="text-meta text-(--text-muted)">{section.key}</p>
        </div>
        {section.user === null ? null : (
          <div className="flex flex-wrap gap-(--space-2)">
            <Badge tone="accent">{sourceBadgeFor(section.user.source)}</Badge>
            {tierMark === null ? null : <Badge tone={tierMark === "Locked" ? "neutral" : "warning"}>{tierMark}</Badge>}
            {section.kinds.map((kind) => (
              <Badge key={kind}>{PROMPT_KIND_LABELS[kind]}</Badge>
            ))}
          </div>
        )}
      </div>
      {stale ? <p className="text-body text-(--warning)">{STALE_OVERRIDE_WARNING}</p> : null}
      {editorScope === null ? (
        <pre className="text-body overflow-auto rounded-(--radius-control) border border-(--border) bg-(--bg) p-(--space-4) whitespace-pre-wrap text-(--text)">
          {section.user?.body ?? ""}
        </pre>
      ) : (
        <SectionEditor
          key={`${section.key}:${editorScope}`}
          workspaceSlug={workspaceSlug}
          section={section}
          scope={editorScope}
          onClose={onCloseEditor}
        />
      )}
      {editorScope === null && (labels.workspace !== null || labels.personal !== null) ? (
        <div className="flex flex-wrap gap-(--space-2)">
          {labels.workspace === null ? null : (
            <Button variant="secondary" onClick={() => onEdit("workspace")}>
              {labels.workspace}
            </Button>
          )}
          {labels.personal === null ? null : (
            <Button variant="secondary" onClick={() => onEdit("user")}>
              {labels.personal}
            </Button>
          )}
        </div>
      ) : null}
    </article>
  );
}
