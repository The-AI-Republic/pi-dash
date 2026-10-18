// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Section editor (parity/specs/agents.md §6, AGT-024-027). The draft
// seeds from the scope's current body; Save arms only on a dirty draft,
// compare shows the pristine default beside it, cancel discards, and the
// revert affordance arms only with an override at the edited scope. Save
// failures land inline with the server's detail; a refused revert lands
// the generic message. The text surface is a plain textarea rather than
// the kit's field component: the kit ships as one entry, so importing its
// Input pulls the field scaffold into the shell's initial chunk and the
// initial-JS gate overruns (see Notes on NEWFRONT-597).

import { Button } from "@pidash/kit";
import * as React from "react";

import {
  REVERT_FAILED_MESSAGE,
  saveFailureMessage,
  useRevertPromptSection,
  useUpsertPromptSection,
} from "../api/mutations.js";
import {
  editorScopeLabel,
  editorSeedFor,
  revertArmedFor,
  type MergedPromptSection,
  type PromptEditorScope,
} from "../lib/sections.js";
import { RevertDialog } from "./RevertDialog.js";

export interface SectionEditorProps {
  workspaceSlug: string;
  section: MergedPromptSection;
  scope: PromptEditorScope;
  onClose: () => void;
}

// Mirrors the kit textarea's surface: the same control-height, focus,
// disabled and resize treatment, spelled with the shared utility tokens.
const DRAFT_EDITOR_CLASS =
  "text-body min-h-20 w-full resize-y rounded-(--radius-control) border border-(--border) bg-(--bg) px-(--space-4) py-(--space-3) text-(--text) placeholder:text-(--text-muted) focus:border-(--accent) focus:outline-none disabled:cursor-not-allowed disabled:opacity-50";

export function SectionEditor({ workspaceSlug, section, scope, onClose }: SectionEditorProps): React.ReactElement {
  const seed = editorSeedFor(section, scope);
  const defaultBody = section.user?.default_body ?? section.workspace?.default_body ?? "";
  const revertVisible = revertArmedFor(section, scope);
  const [draft, setDraft] = React.useState(seed);
  const [compareOpen, setCompareOpen] = React.useState(false);
  const [confirmOpen, setConfirmOpen] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);
  const upsert = useUpsertPromptSection(workspaceSlug);
  const revert = useRevertPromptSection(workspaceSlug);
  const busy = upsert.isPending || revert.isPending;
  const saveEnabled = draft !== seed && !busy;

  const save = (): void => {
    setError(null);
    upsert.mutate(
      { key: section.key, scope, body: draft },
      {
        onSuccess: () => onClose(),
        onError: (failure) => setError(saveFailureMessage(failure)),
      }
    );
  };
  const confirmRevert = (): void => {
    revert.mutate(
      { key: section.key, scope },
      {
        onSuccess: () => {
          setConfirmOpen(false);
          onClose();
        },
        onError: () => {
          setConfirmOpen(false);
          setError(REVERT_FAILED_MESSAGE);
        },
      }
    );
  };

  return (
    <div className="flex flex-col gap-(--space-4)">
      <p className="text-meta text-(--text-muted)">{editorScopeLabel(scope)}</p>
      <div className={compareOpen ? "grid grid-cols-1 gap-(--space-4) lg:grid-cols-2" : undefined}>
        <textarea
          aria-label="Prompt section text"
          rows={10}
          value={draft}
          disabled={busy}
          className={DRAFT_EDITOR_CLASS}
          onChange={(event) => setDraft(event.target.value)}
        />
        {compareOpen ? (
          <div className="flex flex-col gap-(--space-2)">
            <p className="text-meta text-(--text-muted)">Default</p>
            <pre className="text-body overflow-auto rounded-(--radius-control) border border-(--border) bg-(--subtle) p-(--space-4) text-(--text)">
              {defaultBody}
            </pre>
          </div>
        ) : null}
      </div>
      {error === null ? null : (
        <p role="alert" className="text-body text-(--danger)">
          {error}
        </p>
      )}
      <div className="flex flex-wrap gap-(--space-2)">
        <Button variant="primary" disabled={!saveEnabled} loading={upsert.isPending} onClick={save}>
          Save
        </Button>
        <Button variant="secondary" disabled={busy} onClick={onClose}>
          Cancel
        </Button>
        <Button variant="ghost" disabled={busy} onClick={() => setCompareOpen((open) => !open)}>
          {compareOpen ? "Hide default" : "Show default"}
        </Button>
        {revertVisible ? (
          <Button variant="ghost" disabled={busy} onClick={() => setConfirmOpen(true)}>
            Revert to default
          </Button>
        ) : null}
      </div>
      <RevertDialog
        scope={scope}
        open={confirmOpen}
        pending={revert.isPending}
        onConfirm={confirmRevert}
        onCancel={() => setConfirmOpen(false)}
      />
    </div>
  );
}
