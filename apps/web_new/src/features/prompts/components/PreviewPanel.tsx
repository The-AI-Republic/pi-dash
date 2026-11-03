// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Render-against-real-data form (parity/specs/agents.md §7, AGT-030,
// AGT-031). A single target field takes an issue id for the coding and
// review kinds and a scheduler install id for the scheduler kind; the
// rendered prompt lands below it, or the failure lands inline. The same
// form serves the saved receipt preview and the editor's draft preview,
// where the draft rides along so the server renders the unsaved edit.

import { Button } from "@pidash/kit";
import * as React from "react";

import {
  MISSING_INSTALL_TARGET,
  MISSING_ISSUE_TARGET,
  previewFailureMessage,
  usePreviewPrompt,
} from "../api/mutations.js";
import {
  previewTargetLabel,
  previewTargetMode,
  previewTargetPayload,
  previewTargetPlaceholder,
} from "../lib/receipt.js";
import type { PromptEditorScope, PromptKind } from "../lib/sections.js";

/** Draft under preview: the in-progress edit, scoped to its section. */
export interface PreviewDraft {
  scope: PromptEditorScope;
  sectionKey: string;
  body: string;
}

export interface PreviewPanelProps {
  workspaceSlug: string;
  kind: PromptKind;
  /** "Preview" on receipts, "Preview draft" inside the editor. */
  submitLabel: string;
  /** Rendered as the panel heading when set. */
  heading?: string;
  draft?: PreviewDraft;
  /** Test id prefix so each surface's result and error stay addressable. */
  testIdPrefix: string;
}

const TARGET_CLASS =
  "text-body w-full rounded-(--radius-control) border border-(--border) bg-(--bg) px-(--space-3) py-(--space-2) font-mono text-(--text) placeholder:text-(--text-muted) focus:border-(--accent) focus:outline-none";

export function PreviewPanel({
  workspaceSlug,
  kind,
  submitLabel,
  heading,
  draft,
  testIdPrefix,
}: PreviewPanelProps): React.ReactElement {
  const [target, setTarget] = React.useState("");
  const [rendered, setRendered] = React.useState<string | null>(null);
  const [error, setError] = React.useState<string | null>(null);
  const preview = usePreviewPrompt(workspaceSlug);
  const trimmed = target.trim();
  const submitEnabled = trimmed !== "" && !preview.isPending;

  const submit = (): void => {
    if (preview.isPending) return;
    if (trimmed === "") {
      setError(previewTargetMode(kind) === "install" ? MISSING_INSTALL_TARGET : MISSING_ISSUE_TARGET);
      return;
    }
    setError(null);
    preview.mutate(
      { kind, input: previewTargetPayload(kind, trimmed, draft) },
      {
        onSuccess: (result) => {
          setRendered(result.prompt);
          setError(null);
        },
        onError: (failure) => {
          setRendered(null);
          setError(previewFailureMessage(failure));
        },
      }
    );
  };

  return (
    <div className="flex flex-col gap-(--space-3)">
      {heading === undefined ? null : <h3 className="text-h3 text-(--text)">{heading}</h3>}
      <form
        className="flex flex-wrap items-end gap-(--space-2)"
        onSubmit={(event) => {
          event.preventDefault();
          submit();
        }}
      >
        <label className="flex min-w-0 flex-1 flex-col gap-(--space-1)">
          <span className="text-meta text-(--text-muted)">{previewTargetLabel(kind)}</span>
          <input
            type="text"
            value={target}
            spellCheck={false}
            autoComplete="off"
            placeholder={previewTargetPlaceholder(kind)}
            className={TARGET_CLASS}
            onChange={(event) => setTarget(event.target.value)}
          />
        </label>
        <Button type="submit" variant="secondary" loading={preview.isPending} disabled={!submitEnabled}>
          {submitLabel}
        </Button>
      </form>
      {error === null ? null : (
        <p role="alert" data-testid={`${testIdPrefix}-error`} className="text-body text-(--danger)">
          {error}
        </p>
      )}
      {rendered === null ? null : (
        <pre
          data-testid={`${testIdPrefix}-result`}
          className="text-body max-h-96 overflow-auto rounded-(--radius-control) border border-(--border) bg-(--bg) p-(--space-4) whitespace-pre-wrap text-(--text)"
        >
          {rendered}
        </pre>
      )}
    </div>
  );
}
