// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One workspace draft (parity/specs/issues.md §12, ISS-137/138/139/140).
// A row-shaped block: project short code with click-to-copy, the draft's
// name, its inline property strip and an ellipsis menu that repeats the
// right-click entries (edit, make a copy, move to project, delete).
// Double-clicking the block edits it. The name paragraph carries the
// draft's title for reading surfaces; the block also carries
// `data-draft-block` so parity hooks can find it without the old app's
// element ids.

import { Tooltip, cn } from "@pidash/kit";
import type { DraftIssue, ProjectDetails } from "@pidash/api-client";
import { Copy, Ellipsis, PenLine, SquareStack, Trash2 } from "lucide-react";
import * as React from "react";

import { notifySuccess } from "../../../shared/shell/index.js";
import { DraftProperties } from "./DraftProperties.js";
import type { DraftFieldPatch } from "./model.js";

export interface DraftBlockProps {
  workspaceSlug: string;
  draft: DraftIssue;
  project: ProjectDetails | undefined;
  /** The project's short code, shown beside the name. */
  projectIdentifier: string | null;
  editable: boolean;
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
  editable,
  onEdit,
  onCopy,
  onMove,
  onDelete,
  onWrite,
}: DraftBlockProps): React.ReactElement {
  const [menuOpen, setMenuOpen] = React.useState(false);
  const title = draft.name ?? "";
  const identifier = projectIdentifier;

  async function copyCode(code: string): Promise<void> {
    const ok = await copyIdentifier(code, globalThis.navigator?.clipboard);
    if (ok) notifyCopied(code);
  }

  return (
    <article
      data-draft-block=""
      data-draft-id={draft.id}
      aria-label={`Draft ${title}`}
      onDoubleClick={onEdit}
      onContextMenu={(event) => {
        event.preventDefault();
        setMenuOpen(true);
      }}
      className={cn(
        "group/draft relative flex min-h-11 cursor-pointer flex-col gap-(--space-2) border-b border-(--border)",
        "px-(--space-4) py-(--space-3) text-(--text) hover:bg-(--subtle)"
      )}
    >
      <div className="flex min-w-0 items-center gap-(--space-2)">
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
        <div className="relative shrink-0" onClick={(event) => event.stopPropagation()}>
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
            />
          ) : null}
        </div>
      </div>
      <DraftProperties
        workspaceSlug={workspaceSlug}
        draft={draft}
        project={project}
        editable={editable}
        onWrite={onWrite}
      />
    </article>
  );
}

function DraftMenu({
  onClose,
  onEdit,
  onCopy,
  onMove,
  onDelete,
}: {
  onClose: () => void;
  onEdit: () => void;
  onCopy: () => void;
  onMove: () => void;
  onDelete: () => void;
}): React.ReactElement {
  const items = [
    { key: "edit", label: "Edit", icon: PenLine, run: onEdit },
    { key: "copy", label: "Make a copy", icon: Copy, run: onCopy },
    { key: "move", label: "Move to project", icon: SquareStack, run: onMove },
    { key: "delete", label: "Delete", icon: Trash2, run: onDelete, danger: true },
  ] as const;
  return (
    <div
      role="menu"
      aria-label="Draft actions"
      className="shadow-popover absolute right-0 z-10 mt-(--space-1) flex min-w-44 flex-col rounded-(--radius-popover) border border-(--border) bg-(--bg) p-(--space-1)"
      onMouseLeave={onClose}
    >
      {items.map((item) => {
        const Icon = item.icon;
        return (
          <button
            key={item.key}
            type="button"
            role="menuitem"
            onClick={() => {
              onClose();
              item.run();
            }}
            className={cn(
              "text-body flex items-center gap-(--space-2) rounded-(--radius-control) px-(--space-2) py-(--space-1) text-left hover:bg-(--subtle) focus-visible:ring-2 focus-visible:ring-(--accent) focus-visible:outline-none",
              "danger" in item && item.danger ? "text-(--danger)" : "text-(--text)"
            )}
          >
            <Icon size={14} strokeWidth={1.5} aria-hidden="true" />
            {item.label}
          </button>
        );
      })}
    </div>
  );
}

function notifyCopied(identifier: string): void {
  notifySuccess(`Copied ${identifier}`);
}
