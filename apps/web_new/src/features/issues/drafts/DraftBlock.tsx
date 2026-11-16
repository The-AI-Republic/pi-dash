// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One workspace draft (parity/specs/issues.md §12, ISS-137/138/139/140;
// parity/specs/drafts.md §3.1, DRAFT-007).
// A row-shaped block: the work-type mark when the draft holds a type,
// the project short code with click-to-copy, the draft's name, its
// inline property strip and a hover quick action whose menu repeats the
// right-click entries. Both openers render the shared row catalog
// (draftActions.ts) in pinned order (drafts spec §3.1, DRAFT-009), so
// the two can never drift.
// Double-clicking the block edits it. The name paragraph carries the
// draft's title for reading surfaces; the block also carries
// `data-draft-block` so parity hooks can find it without the old app's
// element ids.

import { Tooltip, cn } from "@pidash/kit";
import type { DraftIssue, ProjectDetails } from "@pidash/api-client";
import { Copy, Ellipsis, PenLine, Shapes, SquareStack, Trash2 } from "lucide-react";
import * as React from "react";

import { notifySuccess } from "../../../shared/shell/index.js";
import { DRAFT_ROW_ACTIONS, type DraftRowActionId } from "./draftActions.js";
import { DraftProperties } from "./DraftProperties.js";
import type { DraftAbilities, DraftFieldPatch } from "./model.js";

export interface DraftBlockProps {
  workspaceSlug: string;
  draft: DraftIssue;
  project: ProjectDetails | undefined;
  /** The project's short code, shown beside the name. */
  projectIdentifier: string | null;
  /** DRAFT-025: which row actions this viewer may use. */
  abilities: DraftAbilities;
  onEdit: () => void;
  onCopy: () => void;
  onMove: () => void;
  onDelete: () => void;
  onWrite: (patch: DraftFieldPatch) => void;
}

/** Clipboard write behind the identifier's click-to-copy. */
export async function copyIdentifier(
  identifier: string,
  clipboard: Pick<Clipboard, "writeText"> | undefined
): Promise<boolean> {
  if (clipboard === undefined) return false;
  try {
    await clipboard.writeText(identifier);
    return true;
  } catch {
    return false;
  }
}

export function DraftBlock({
  workspaceSlug,
  draft,
  project,
  projectIdentifier,
  abilities,
  onEdit,
  onCopy,
  onMove,
  onDelete,
  onWrite,
}: DraftBlockProps): React.ReactElement {
  const [menuOpen, setMenuOpen] = React.useState(false);
  const title = draft.name ?? "";
  const identifier = projectIdentifier;
  const hasActions = abilities.manage || abilities.copy || abilities.convert;

  async function copyCode(code: string): Promise<void> {
    const ok = await copyIdentifier(code, globalThis.navigator?.clipboard);
    if (ok) notifyCopied(code);
  }

  return (
    <article
      data-draft-block=""
      data-draft-id={draft.id}
      aria-label={`Draft ${title}`}
      onDoubleClick={abilities.manage ? onEdit : undefined}
      onContextMenu={(event) => {
        if (!hasActions) return;
        event.preventDefault();
        setMenuOpen(true);
      }}
      className={cn(
        "group/draft relative flex min-h-11 cursor-pointer flex-col gap-(--space-2) border-b border-(--border)",
        "px-(--space-4) py-(--space-3) text-(--text) hover:bg-(--subtle)"
      )}
    >
      <div className="flex min-w-0 items-center gap-(--space-2)">
        {draft.type_id === null ? null : (
          <span
            data-draft-type-mark=""
            role="img"
            aria-label="Work type"
            title="Work type"
            className="inline-flex shrink-0 items-center text-(--text-muted)"
          >
            <Shapes size={14} strokeWidth={1.5} aria-hidden="true" />
          </span>
        )}
        {identifier === null ? null : (
          <button
            type="button"
            onClick={(event) => {
              event.stopPropagation();
              void copyCode(identifier);
            }}
            aria-label={`Copy ${identifier}`}
            className="text-caption shrink-0 rounded-(--radius-control) font-[family-name:var(--font-mono)] text-(--text-muted) hover:text-(--text) focus-visible:ring-2 focus-visible:ring-(--accent) focus-visible:outline-none"
          >
            {identifier}
          </button>
        )}
        <Tooltip content={title}>
          <p className="text-body min-w-0 flex-1 truncate text-(--text)" data-draft-name="">
            {title}
          </p>
        </Tooltip>
        {hasActions ? (
          <div
            className="relative shrink-0 opacity-0 transition-opacity group-hover/draft:opacity-100 focus-within:opacity-100"
            onClick={(event) => event.stopPropagation()}
          >
            <button
              type="button"
              aria-label={`Actions for ${title}`}
              aria-expanded={menuOpen}
              aria-haspopup="menu"
              onClick={() => setMenuOpen((current) => !current)}
              className="rounded-(--radius-control) p-(--space-1) text-(--text-muted) hover:bg-(--border) hover:text-(--text) focus-visible:ring-2 focus-visible:ring-(--accent) focus-visible:outline-none"
            >
              <Ellipsis size={16} strokeWidth={1.5} aria-hidden="true" />
            </button>
            {menuOpen ? (
              <DraftMenu
                onClose={() => setMenuOpen(false)}
                onEdit={onEdit}
                onCopy={onCopy}
                onMove={onMove}
                onDelete={onDelete}
                abilities={abilities}
              />
            ) : null}
          </div>
        ) : null}
      </div>
      <DraftProperties
        workspaceSlug={workspaceSlug}
        draft={draft}
        project={project}
        editable={abilities.manage}
        onWrite={onWrite}
      />
    </article>
  );
}

const DRAFT_MENU_ICONS: Record<DraftRowActionId, typeof PenLine> = {
  edit: PenLine,
  copy: Copy,
  move: SquareStack,
  delete: Trash2,
};

function DraftMenu({
  onClose,
  onEdit,
  onCopy,
  onMove,
  onDelete,
  abilities,
}: {
  onClose: () => void;
  onEdit: () => void;
  onCopy: () => void;
  onMove: () => void;
  onDelete: () => void;
  abilities: DraftAbilities;
}): React.ReactElement {
  const runners: Record<DraftRowActionId, () => void> = {
    edit: onEdit,
    copy: onCopy,
    move: onMove,
    delete: onDelete,
  };
  // DRAFT-025: the catalog keeps its pinned order and labels; the
  // matrix only decides which entries this viewer is offered.
  const shown: Record<DraftRowActionId, boolean> = {
    edit: abilities.manage,
    copy: abilities.copy,
    move: abilities.convert,
    delete: abilities.manage,
  };
  return (
    <div
      role="menu"
      aria-label="Draft actions"
      className="shadow-popover absolute right-0 z-10 mt-(--space-1) flex min-w-44 flex-col rounded-(--radius-popover) border border-(--border) bg-(--bg) p-(--space-1)"
      onMouseLeave={onClose}
      onKeyDown={(event) => {
        if (event.key === "Escape") onClose();
      }}
    >
      {DRAFT_ROW_ACTIONS.filter((action) => shown[action.id]).map((action) => {
        const Icon = DRAFT_MENU_ICONS[action.id];
        const run = runners[action.id];
        return (
          <button
            key={action.id}
            type="button"
            role="menuitem"
            onClick={() => {
              onClose();
              run();
            }}
            className={cn(
              "text-body flex items-center gap-(--space-2) rounded-(--radius-control) px-(--space-2) py-(--space-1) text-left hover:bg-(--subtle) focus-visible:ring-2 focus-visible:ring-(--accent) focus-visible:outline-none",
              action.danger ? "text-(--danger)" : "text-(--text)"
            )}
          >
            <Icon size={14} strokeWidth={1.5} aria-hidden="true" />
            {action.label}
          </button>
        );
      })}
    </div>
  );
}

function notifyCopied(identifier: string): void {
  notifySuccess(`Copied ${identifier}`);
}
