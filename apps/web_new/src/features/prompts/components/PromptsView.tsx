// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Prompts page (parity/specs/agents.md §6-7, AGT-023, AGT-032). Two tabs:
// the sections browser lists every prompt section with its effective
// body and provenance, an anchor side nav, and the editor for the scope
// under edit; the receipt tab shows the assembled templates. A loading
// line covers the resolving lists, a banner the failed member lists, and
// an admin-only warning the failed baseline.

import { Button } from "@pidash/kit";
import * as React from "react";

import { usePermissions } from "../../../core/session/index.js";
import { usePromptSectionLists } from "../api/queries.js";
import type { PromptEditorScope, PromptViewerGates } from "../lib/sections.js";
import { SectionCard } from "./SectionCard.js";
import { PromptsTabs, type PromptTab } from "./PromptsTabs.js";
import { ReceiptView } from "./ReceiptView.js";

/** Loading line while the required lists resolve. */
export const PROMPTS_LOADING_TEXT = "Loading…";
/** Banner when a member list fails. */
export const PROMPTS_LOAD_FAILED_TEXT = "Could not load prompt sections for this workspace.";
/** Admin-only warning when the baseline fails. */
export const PROMPTS_BASELINE_WARNING_TEXT =
  "The workspace baseline failed to load, so workspace editing is unavailable. The sections below still show.";

export interface PromptsViewProps {
  workspaceSlug: string;
}

interface OpenEditor {
  key: string;
  scope: PromptEditorScope;
}

export function PromptsView({ workspaceSlug }: PromptsViewProps): React.ReactElement {
  const { isAdmin, canEdit } = usePermissions(workspaceSlug);
  const lists = usePromptSectionLists(workspaceSlug, isAdmin);
  const [tab, setTab] = React.useState<PromptTab>("sections");
  const [openEditor, setOpenEditor] = React.useState<OpenEditor | null>(null);

  const selectTab = (next: PromptTab): void => {
    setTab(next);
    setOpenEditor(null);
  };

  if (tab === "receipt") {
    return (
      <div className="flex flex-col gap-(--space-4)">
        <h1 className="text-h2 text-(--text)">Prompts</h1>
        <PromptsTabs active={tab} onSelect={selectTab} />
        <ReceiptView workspaceSlug={workspaceSlug} isAdmin={isAdmin} />
      </div>
    );
  }
  const viewer: PromptViewerGates = { isAdmin, canEdit, baselineLoaded: lists.baselineLoaded };

  if (lists.memberPending) {
    return (
      <div className="flex flex-col gap-(--space-4)">
        <h1 className="text-h2 text-(--text)">Prompts</h1>
        <PromptsTabs active={tab} onSelect={selectTab} />
        <p className="text-body text-(--text-muted)">{PROMPTS_LOADING_TEXT}</p>
      </div>
    );
  }
  if (lists.memberFailed) {
    return (
      <div className="flex flex-col gap-(--space-4)">
        <h1 className="text-h2 text-(--text)">Prompts</h1>
        <PromptsTabs active={tab} onSelect={selectTab} />
        <p role="alert" className="text-body text-(--danger)">
          {PROMPTS_LOAD_FAILED_TEXT}
        </p>
        <div>
          <Button variant="secondary" onClick={lists.refetchMember}>
            Try again
          </Button>
        </div>
      </div>
    );
  }
  if (lists.baselinePending) {
    return (
      <div className="flex flex-col gap-(--space-4)">
        <h1 className="text-h2 text-(--text)">Prompts</h1>
        <PromptsTabs active={tab} onSelect={selectTab} />
        <p className="text-body text-(--text-muted)">{PROMPTS_LOADING_TEXT}</p>
      </div>
    );
  }
  return (
    <div className="flex flex-col gap-(--space-6)">
      <h1 className="text-h2 text-(--text)">Prompts</h1>
      <PromptsTabs active={tab} onSelect={selectTab} />
      {lists.baselineFailed ? (
        <p role="alert" className="text-body text-(--warning)">
          {PROMPTS_BASELINE_WARNING_TEXT}
        </p>
      ) : null}
      <div className="grid grid-cols-1 gap-(--space-8) lg:grid-cols-[12rem_1fr]">
        <nav aria-label="Prompt sections">
          <ul className="flex flex-col gap-(--space-1)">
            {lists.sections.map((section) => (
              <li key={section.key}>
                <a
                  href={`#prompt-section-${section.key}`}
                  className="text-body flex flex-col rounded-(--radius-control) px-(--space-3) py-(--space-2) text-(--text) hover:bg-(--subtle)"
                >
                  <span>{section.title}</span>
                  <span className="text-meta text-(--text-muted)">{section.key}</span>
                </a>
              </li>
            ))}
          </ul>
        </nav>
        <div className="flex flex-col gap-(--space-8)">
          {lists.sections.map((section) => (
            <SectionCard
              key={section.key}
              workspaceSlug={workspaceSlug}
              section={section}
              viewer={viewer}
              editorScope={openEditor !== null && openEditor.key === section.key ? openEditor.scope : null}
              onEdit={(scope) => setOpenEditor({ key: section.key, scope })}
              onCloseEditor={() => setOpenEditor(null)}
            />
          ))}
        </div>
      </div>
    </div>
  );
}
