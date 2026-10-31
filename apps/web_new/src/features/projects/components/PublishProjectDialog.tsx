// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Publish dialog (parity/specs/project-settings-lifecycle.md §PSET-016).
// The settings read stays behind a skeleton; at least one of the two
// layouts must remain selected; comments, reactions and votes are
// independent toggles. A published board exposes its live link with open
// and copy actions, update appears only once the form is dirty, and
// unpublishing has its own progress state. Written from the spec; the
// public board itself belongs to the spaces area.

import { Button, Dialog, Input, Skeleton, Switch } from "@pidash/kit";
import { Copy, ExternalLink } from "lucide-react";
import * as React from "react";

import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import {
  boardFormDirty,
  boardFormFrom,
  isBoardPublished,
  publishedBoardLink,
  useDeployBoard,
  usePublishBoard,
  useUnpublishBoard,
  useUpdateBoard,
  type BoardForm,
} from "../api/publish.js";

export const PUBLISH_LINK_COPIED_TITLE = "Published page link copied successfully.";
export const PUBLISH_LINK_COPY_FAILED_TITLE = "Couldn't copy the published link";
export const PUBLISH_LAYOUT_REQUIRED = "Keep at least one layout on before publishing.";
export const PUBLISH_LAYOUT_REMOVE_REFUSED = "A published board needs at least one layout.";

export interface PublishProjectDialogProps {
  workspaceSlug: string;
  projectId: string;
  projectName: string;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

type LayoutKey = "list" | "kanban";

const LAYOUTS: ReadonlyArray<{ key: LayoutKey; label: string }> = [
  { key: "list", label: "List" },
  { key: "kanban", label: "Kanban" },
];

export function PublishProjectDialog({
  workspaceSlug,
  projectId,
  projectName,
  open,
  onOpenChange,
}: PublishProjectDialogProps): React.ReactElement {
  const board = useDeployBoard(workspaceSlug, projectId, open);
  const publish = usePublishBoard();
  const update = useUpdateBoard();
  const unpublish = useUnpublishBoard();
  const [form, setForm] = React.useState<BoardForm>(() => boardFormFrom(undefined));
  const [layoutError, setLayoutError] = React.useState<string | null>(null);
  const seeded = React.useRef(false);

  // Fresh form per visit and reseat when the read lands.
  React.useEffect(() => {
    if (!open) {
      seeded.current = false;
      return;
    }
    if (board.data === undefined || seeded.current) return;
    seeded.current = true;
    setForm(boardFormFrom(board.data));
    setLayoutError(null);
  }, [open, board.data]);

  React.useEffect(() => {
    if (open) return;
    setForm(boardFormFrom(undefined));
    setLayoutError(null);
  }, [open]);

  const published = isBoardPublished(board.data);
  const stored = board.data;
  const dirty = published && boardFormDirty(form, stored);
  const pending = publish.isPending || update.isPending;
  const layoutCount = Number(form.list) + Number(form.kanban);

  function toggleLayout(key: LayoutKey): void {
    const next = { ...form, [key]: !form[key] };
    if (!next.list && !next.kanban) {
      setLayoutError(PUBLISH_LAYOUT_REMOVE_REFUSED);
      return;
    }
    setLayoutError(null);
    setForm(next);
  }

  function toggle(name: "comments" | "reactions" | "voting", next: boolean): void {
    setForm((current) => ({ ...current, [name]: next }));
  }

  function payload(): { views: Record<string, boolean> } {
    return { views: { list: form.list, kanban: form.kanban } };
  }

  function submit(): void {
    if (layoutCount === 0) {
      setLayoutError(PUBLISH_LAYOUT_REQUIRED);
      return;
    }
    const flags = {
      is_comments_enabled: form.comments,
      is_reactions_enabled: form.reactions,
      is_votes_enabled: form.voting,
    };
    if (published && stored !== undefined && "id" in stored) {
      update.mutate(
        {
          workspaceSlug,
          projectId,
          boardId: stored.id,
          input: { view_props: { list: form.list, kanban: form.kanban }, ...flags },
        },
        { onSuccess: () => onOpenChange(false) }
      );
      return;
    }
    publish.mutate({ workspaceSlug, projectId, input: { ...payload(), ...flags } });
  }

  function unpublishBoard(boardId: string): void {
    unpublish.mutate({ workspaceSlug, projectId, boardId });
  }

  async function copyLink(link: string): Promise<void> {
    try {
      if (globalThis.navigator?.clipboard === undefined)
        throw new Error("The clipboard is not available in this window.");
      await globalThis.navigator.clipboard.writeText(link);
      notifySuccess(PUBLISH_LINK_COPIED_TITLE);
    } catch (error) {
      notifyError(PUBLISH_LINK_COPY_FAILED_TITLE, (error as Error).message);
    }
  }

  const link =
    published && stored !== undefined && "anchor" in stored && stored.anchor !== null
      ? publishedBoardLink(stored.anchor)
      : null;
  const loading = board.isPending;
  const unpublishing = unpublish.isPending;
  const boardId = published && stored !== undefined && "id" in stored ? stored.id : null;

  return (
    <Dialog
      title="Publish project"
      description={`Share ${projectName} on a public board.`}
      open={open}
      onOpenChange={onOpenChange}
      className="max-w-xl"
      actions={
        <>
          {boardId === null ? null : (
            <Button variant="secondary" loading={unpublishing} onClick={() => unpublishBoard(boardId)}>
              {unpublishing ? "Unpublishing…" : "Unpublish"}
            </Button>
          )}
          <Button variant="secondary" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          {!published || dirty ? (
            <Button variant="primary" loading={pending} onClick={submit}>
              {pending ? "Publishing…" : published ? "Update" : "Publish"}
            </Button>
          ) : null}
        </>
      }
    >
      {loading ? (
        <div
          data-testid="publish-loading"
          role="status"
          aria-label="Loading publish settings"
          className="flex flex-col gap-(--space-4)"
        >
          <Skeleton className="h-6 w-40" />
          <Skeleton className="h-10 w-full" />
          <Skeleton className="h-10 w-full" />
          <Skeleton className="h-10 w-full" />
        </div>
      ) : (
        <div data-testid="publish-settings" className="flex flex-col gap-(--space-6)">
          <fieldset className="flex flex-col gap-(--space-2)">
            <legend className="text-body font-medium text-(--text)">Layouts</legend>
            {LAYOUTS.map((layout) => (
              <label key={layout.key} className="text-body flex items-center gap-(--space-2) text-(--text)">
                <input
                  type="checkbox"
                  checked={form[layout.key]}
                  aria-label={layout.label}
                  data-testid={`publish-layout-${layout.key}`}
                  onChange={() => toggleLayout(layout.key)}
                />
                {layout.label}
              </label>
            ))}
            {layoutError === null ? null : (
              <p role="alert" className="text-meta text-(--danger)">
                {layoutError}
              </p>
            )}
          </fieldset>

          <div className="flex flex-col gap-(--space-3)">
            <div className="flex items-center justify-between gap-(--space-4)">
              <span className="text-body text-(--text)">Comments</span>
              <Switch
                checked={form.comments}
                label="Comments"
                data-testid="publish-toggle-comments"
                onCheckedChange={(next) => toggle("comments", next)}
              />
            </div>
            <div className="flex items-center justify-between gap-(--space-4)">
              <span className="text-body text-(--text)">Reactions</span>
              <Switch
                checked={form.reactions}
                label="Reactions"
                data-testid="publish-toggle-reactions"
                onCheckedChange={(next) => toggle("reactions", next)}
              />
            </div>
            <div className="flex items-center justify-between gap-(--space-4)">
              <span className="text-body text-(--text)">Voting</span>
              <Switch
                checked={form.voting}
                label="Voting"
                data-testid="publish-toggle-voting"
                onCheckedChange={(next) => toggle("voting", next)}
              />
            </div>
          </div>

          {link === null ? null : (
            <div className="flex flex-col gap-(--space-2)" data-testid="publish-live-link-row">
              <span className="text-body flex items-center gap-(--space-2) font-medium text-(--text)">
                <span aria-hidden="true" className="size-2 rounded-full bg-(--success)" />
                Live
              </span>
              <div className="flex items-end gap-(--space-2)">
                <Input label="Public link" readOnly value={link} data-testid="publish-live-link" className="flex-1" />
                <a
                  href={link}
                  target="_blank"
                  rel="noopener noreferrer"
                  aria-label="Open published board"
                  className="text-body inline-flex h-(--control-height) items-center gap-(--space-1) rounded-(--radius-control) border border-(--border) px-(--space-3) text-(--text) hover:bg-(--subtle)"
                >
                  <ExternalLink size={14} aria-hidden="true" />
                  Open
                </a>
                <Button variant="secondary" aria-label="Copy published link" onClick={() => void copyLink(link)}>
                  <Copy size={14} aria-hidden="true" />
                  Copy
                </Button>
              </div>
            </div>
          )}
          <p className="text-meta text-(--text-muted)">Viewers need only the live link.</p>
        </div>
      )}
    </Dialog>
  );
}
