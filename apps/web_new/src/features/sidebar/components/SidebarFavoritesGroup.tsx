// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Sidebar favorites section (parity/specs/shell.md §4, SHELL-056/057).
// The collapsing Favorites group: folders with an inline validated
// create/rename form, entries deep-linking to their targets by entity
// type with a workspace-root fallback, hover-revealed drag handles
// reordering each list with server persistence, and quick menus for
// removal and folder rename. The section hides entirely while the
// top-level list is empty; the first add opens a closed section live
// through the SHELL-032 open flag. The route mounts this for admins
// and members only.

import type { Favorite } from "@pidash/api-client";
import { IconButton, Menu } from "@pidash/kit";
import {
  Check,
  ChevronRight,
  Ellipsis,
  Eye,
  FileText,
  Folder,
  FolderKanban,
  FolderPlus,
  GripVertical,
  Layers,
  Repeat,
} from "lucide-react";
import * as React from "react";

import { resolveDropTarget, type SidebarDropRow, type SidebarDropTarget } from "../../projects/index.js";
import { isFavoritesSectionOpen, notifyError, setFavoritesSectionOpen } from "../../../shared/shell/index.js";
import {
  FOLDER_DUPLICATE_TITLE,
  FOLDER_EMPTY_TITLE,
  favoriteOrderPatch,
  moveFavoriteToIndex,
  selectOrderedFavorites,
  selectTopLevelFavorites,
  useCreateFavoriteFolder,
  useDeleteFavorite,
  useFavoriteGroup,
  useFavorites,
  useMoveFavorite,
  useRenameFavoriteFolder,
  validateFolderName,
  type FavoriteMoveInput,
} from "../api/favorites.js";
import { resolveFavoriteHref, resolveFavoriteIconKind, type FavoriteIconKind } from "../favoriteLinks.js";

/** Entry quick-menu label the parity driver reads (SHELL-056). */
export const FAVORITE_REMOVE_LABEL = "Remove from favorites";
const FAVORITE_RENAME_LABEL = "Rename Folder";

const ROW_ICONS: Record<FavoriteIconKind, typeof Folder> = {
  project: FolderKanban,
  page: FileText,
  view: Eye,
  module: Layers,
  cycle: Repeat,
  folder: Folder,
  fallback: FileText,
};

const ROW_LINK_TONE =
  "text-body flex min-w-0 flex-1 items-center gap-(--space-2) rounded-(--radius-control) px-(--space-2) py-(--space-2) text-left text-(--text-muted) hover:bg-(--subtle) hover:text-(--text)";

/**
 * Row title: the target's live name when the projection carries one,
 * else the stored favorite name.
 */
export function favoriteDisplayName(row: Pick<Favorite, "entity_data" | "name">): string {
  if (typeof row.entity_data === "object" && row.entity_data !== null) {
    const nested = (row.entity_data as Record<string, unknown>)["name"];
    if (typeof nested === "string" && nested !== "") return nested;
  }
  return row.name ?? "";
}

type FolderFormState = { mode: "create" } | { mode: "rename"; favoriteId: string; current: string };

/**
 * Pointer-drag reorder for one favorites list (SHELL-056): the
 * top-level list and every open folder's list each run their own.
 * Drops commit through the caller's move; releases elsewhere or
 * no-op drops stay silent.
 */
function useFavoriteReorder(args: {
  rows: readonly Pick<Favorite, "id" | "sequence">[];
  groupId: string | null;
  commit: (input: FavoriteMoveInput) => void;
}): {
  listRef: React.RefObject<HTMLUListElement | null>;
  sourceId: string | null;
  target: SidebarDropTarget | null;
  onDragStart: (favoriteId: string) => void;
} {
  const [sourceId, setSourceId] = React.useState<string | null>(null);
  const [target, setTarget] = React.useState<SidebarDropTarget | null>(null);
  const targetRef = React.useRef<SidebarDropTarget | null>(null);
  const listRef = React.useRef<HTMLUListElement>(null);

  React.useEffect(() => {
    if (sourceId === null) return;
    const spans = (): SidebarDropRow[] => {
      const nodes = listRef.current?.querySelectorAll("[data-favorite-row]") ?? [];
      return Array.from(nodes).map((node) => {
        const rect = node.getBoundingClientRect();
        return { id: (node as HTMLElement).dataset["favoriteRow"] ?? "", top: rect.top, bottom: rect.bottom };
      });
    };
    const move = (event: PointerEvent): void => {
      const next = resolveDropTarget(spans(), event.clientY, sourceId);
      targetRef.current = next;
      setTarget(next);
    };
    const finish = (commitDrop: boolean): void => {
      const drop = targetRef.current;
      targetRef.current = null;
      setTarget(null);
      setSourceId(null);
      if (!commitDrop || drop === null) return;
      const ids = args.rows.map((row) => row.id);
      const rest = ids.filter((id) => id !== sourceId);
      const slot = rest.indexOf(drop.id);
      const order = moveFavoriteToIndex(ids, sourceId, drop.after ? slot + 1 : slot);
      const patch = favoriteOrderPatch(args.rows, order, sourceId);
      if (patch !== null) args.commit({ ...patch, groupId: args.groupId });
    };
    const up = (): void => finish(true);
    const cancel = (): void => finish(false);
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    window.addEventListener("pointercancel", cancel);
    window.addEventListener("blur", cancel);
    return () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      window.removeEventListener("pointercancel", cancel);
      window.removeEventListener("blur", cancel);
    };
  }, [sourceId, args.rows, args.groupId, args.commit]);

  return { listRef, sourceId, target, onDragStart: setSourceId };
}

export interface SidebarFavoritesGroupProps {
  workspaceSlug: string;
}

export function SidebarFavoritesGroup({ workspaceSlug }: SidebarFavoritesGroupProps): React.ReactElement | null {
  const list = useFavorites(workspaceSlug);
  const top = React.useMemo(() => selectTopLevelFavorites(list.data ?? []), [list.data]);
  const [open, setOpen] = React.useState(isFavoritesSectionOpen);
  const [form, setForm] = React.useState<FolderFormState | null>(null);
  const [foldersOpen, setFoldersOpen] = React.useState<ReadonlySet<string>>(new Set());
  const create = useCreateFavoriteFolder(workspaceSlug);
  const rename = useRenameFavoriteFolder(workspaceSlug);
  const remove = useDeleteFavorite(workspaceSlug);
  const mover = useMoveFavorite(workspaceSlug);
  const commit = React.useCallback((input: FavoriteMoveInput) => mover.mutate(input), [mover]);
  const reorder = useFavoriteReorder({ rows: top, groupId: null, commit });

  // The first add opens the section live (SHELL-032 seam): starring
  // writes the flag and re-reads the list without remounting the
  // sidebar, so the stored flag is re-checked on every arrival.
  React.useEffect(() => {
    if (top.length > 0 && isFavoritesSectionOpen()) setOpen(true);
  }, [top.length]);

  function setPersisted(next: boolean): void {
    setFavoritesSectionOpen(next);
    setOpen(next);
  }

  function toggleFolder(favoriteId: string): void {
    setFoldersOpen((previous) => {
      const next = new Set(previous);
      if (next.has(favoriteId)) next.delete(favoriteId);
      else next.add(favoriteId);
      return next;
    });
  }

  function submitForm(name: string, current: FolderFormState): void {
    if (name === "") return;
    const existing = top.filter((row) => row.is_folder).map((row) => favoriteDisplayName(row));
    const issue = validateFolderName(name, existing);
    if (issue === "empty") {
      notifyError(FOLDER_EMPTY_TITLE);
      return;
    }
    if (issue === "duplicate") {
      notifyError(FOLDER_DUPLICATE_TITLE);
      return;
    }
    if (current.mode === "rename") {
      rename.mutate({ favoriteId: current.favoriteId, name }, { onSuccess: () => setForm(null) });
    } else {
      create.mutate({ name }, { onSuccess: () => setForm(null) });
    }
  }

  if (top.length === 0) return null;

  return (
    <section aria-label="Favorites" data-testid="sidebar-favorites-group" className="flex flex-col gap-(--space-1)">
      <div className="group flex w-full items-center justify-between rounded-(--radius-control) px-(--space-2) py-(--space-1)">
        <button
          type="button"
          aria-expanded={open}
          aria-label={open ? "Collapse favorites section" : "Expand favorites section"}
          data-testid="favorites-section-toggle"
          onClick={() => setPersisted(!open)}
          className="text-caption flex flex-1 cursor-pointer items-center gap-(--space-1) text-left text-(--text-muted)"
        >
          <span className="font-semibold">Favorites</span>
          <ChevronRight
            size={14}
            strokeWidth={2}
            aria-hidden="true"
            className={open ? "rotate-90 transition-transform" : "transition-transform"}
          />
        </button>
        <button
          type="button"
          aria-label="Create favorites folder"
          data-testid="favorites-folder-create"
          onClick={() => {
            setPersisted(true);
            setForm({ mode: "create" });
          }}
          className="cursor-pointer rounded-(--radius-control) p-(--space-1) text-(--text-muted) hover:bg-(--subtle)"
        >
          <FolderPlus size={14} strokeWidth={2} aria-hidden="true" />
        </button>
      </div>
      {!open ? null : (
        <>
          {form === null ? null : (
            <FavoriteFolderForm
              key={form.mode === "rename" ? form.favoriteId : "create"}
              initial={form.mode === "rename" ? form.current : ""}
              onSubmit={(name) => submitForm(name, form)}
              onDismiss={() => setForm(null)}
            />
          )}
          <ul data-testid="favorites-entry-list" ref={reorder.listRef} className="flex flex-col gap-px">
            {top.map((row) => (
              <FavoriteRow
                key={row.id}
                workspaceSlug={workspaceSlug}
                row={row}
                foldersOpen={foldersOpen}
                onToggleFolder={toggleFolder}
                onRename={(favoriteId, current) => setForm({ mode: "rename", favoriteId, current })}
                onRemove={(favoriteId) => remove.mutate({ favoriteId })}
                dropIndicator={
                  reorder.target !== null && reorder.target.id === row.id
                    ? reorder.target.after
                      ? "below"
                      : "above"
                    : null
                }
                isDragging={reorder.sourceId === row.id}
                onDragStart={reorder.onDragStart}
              />
            ))}
          </ul>
        </>
      )}
    </section>
  );
}

interface FavoriteRowProps {
  workspaceSlug: string;
  row: Favorite;
  foldersOpen: ReadonlySet<string>;
  onToggleFolder: (favoriteId: string) => void;
  onRename: (favoriteId: string, current: string) => void;
  onRemove: (favoriteId: string) => void;
  dropIndicator: "above" | "below" | null;
  isDragging: boolean;
  onDragStart: (favoriteId: string) => void;
}

function FavoriteRow({
  workspaceSlug,
  row,
  foldersOpen,
  onToggleFolder,
  onRename,
  onRemove,
  dropIndicator,
  isDragging,
  onDragStart,
}: FavoriteRowProps): React.ReactElement {
  const name = favoriteDisplayName(row);
  const Icon = ROW_ICONS[resolveFavoriteIconKind(row)];
  const folderOpen = foldersOpen.has(row.id);
  const menuItems = row.is_folder
    ? [
        { label: FAVORITE_REMOVE_LABEL, itemKey: "remove", onSelect: () => onRemove(row.id) },
        { label: FAVORITE_RENAME_LABEL, itemKey: "rename", onSelect: () => onRename(row.id, name) },
      ]
    : [{ label: FAVORITE_REMOVE_LABEL, itemKey: "remove", onSelect: () => onRemove(row.id) }];

  return (
    <li data-favorite-row={row.id} data-testid="favorite-entry" data-entry-name={name} className="relative">
      {dropIndicator === "above" ? (
        <span data-testid="favorite-drop-indicator" className="absolute inset-x-2 top-0 h-0.5 bg-(--accent)" />
      ) : null}
      <div
        className={
          isDragging
            ? "group/row relative flex w-full items-center rounded-(--radius-control) opacity-60"
            : "group/row relative flex w-full items-center rounded-(--radius-control)"
        }
      >
        <button
          type="button"
          aria-label={`Rearrange ${name}`}
          title="Drag to reorder"
          data-testid="favorite-drag-handle"
          onPointerDown={(event) => {
            event.preventDefault();
            onDragStart(row.id);
          }}
          className="absolute top-1/2 -left-3 hidden -translate-y-1/2 cursor-grab items-center justify-center rounded-(--radius-control) p-px text-(--text-muted) group-hover/row:flex"
        >
          <GripVertical size={14} strokeWidth={2} aria-hidden="true" />
        </button>
        {row.is_folder ? (
          <button
            type="button"
            aria-expanded={folderOpen}
            data-testid="favorite-folder-toggle"
            onClick={() => onToggleFolder(row.id)}
            className={ROW_LINK_TONE}
          >
            <Icon size={16} strokeWidth={1.5} aria-hidden="true" />
            <span className="truncate">{name}</span>
          </button>
        ) : (
          <a href={resolveFavoriteHref(workspaceSlug, row)} className={ROW_LINK_TONE}>
            <Icon size={16} strokeWidth={1.5} aria-hidden="true" />
            <span className="truncate">{name}</span>
          </a>
        )}
        <Menu
          trigger={
            <IconButton
              label={`Actions for ${name}`}
              data-testid="favorite-row-menu"
              className="shrink-0 opacity-0 group-hover/row:opacity-100 focus-visible:opacity-100"
            >
              <Ellipsis size={16} strokeWidth={1.5} aria-hidden="true" />
            </IconButton>
          }
          items={menuItems}
        />
      </div>
      {row.is_folder && folderOpen ? (
        <FavoriteFolderChildren
          workspaceSlug={workspaceSlug}
          folderId={row.id}
          foldersOpen={foldersOpen}
          onToggleFolder={onToggleFolder}
          onRename={onRename}
          onRemove={onRemove}
        />
      ) : null}
      {dropIndicator === "below" ? (
        <span data-testid="favorite-drop-indicator" className="absolute inset-x-2 bottom-0 h-0.5 bg-(--accent)" />
      ) : null}
    </li>
  );
}

interface FavoriteFolderChildrenProps {
  workspaceSlug: string;
  folderId: string;
  foldersOpen: ReadonlySet<string>;
  onToggleFolder: (favoriteId: string) => void;
  onRename: (favoriteId: string, current: string) => void;
  onRemove: (favoriteId: string) => void;
}

function FavoriteFolderChildren({
  workspaceSlug,
  folderId,
  foldersOpen,
  onToggleFolder,
  onRename,
  onRemove,
}: FavoriteFolderChildrenProps): React.ReactElement {
  const group = useFavoriteGroup(workspaceSlug, folderId);
  const rows = React.useMemo(() => selectOrderedFavorites(group.data ?? []), [group.data]);
  const mover = useMoveFavorite(workspaceSlug);
  const commit = React.useCallback((input: FavoriteMoveInput) => mover.mutate(input), [mover]);
  const reorder = useFavoriteReorder({ rows, groupId: folderId, commit });

  return (
    <ul data-testid="favorite-folder-children" ref={reorder.listRef} className="flex flex-col gap-px pl-6">
      {rows.map((row) => (
        <FavoriteRow
          key={row.id}
          workspaceSlug={workspaceSlug}
          row={row}
          foldersOpen={foldersOpen}
          onToggleFolder={onToggleFolder}
          onRename={onRename}
          onRemove={onRemove}
          dropIndicator={
            reorder.target !== null && reorder.target.id === row.id ? (reorder.target.after ? "below" : "above") : null
          }
          isDragging={reorder.sourceId === row.id}
          onDragStart={reorder.onDragStart}
        />
      ))}
    </ul>
  );
}

interface FavoriteFolderFormProps {
  initial: string;
  onSubmit: (name: string) => void;
  onDismiss: () => void;
}

function FavoriteFolderForm({ initial, onSubmit, onDismiss }: FavoriteFolderFormProps): React.ReactElement {
  const [value, setValue] = React.useState(initial);
  const formRef = React.useRef<HTMLFormElement>(null);

  React.useEffect(() => {
    // Set: effects run after mount, so the form is always present here.
    const form = formRef.current as HTMLFormElement;
    const outside = (event: PointerEvent): void => {
      if (!form.contains(event.target as Node)) onDismiss();
    };
    document.addEventListener("pointerdown", outside);
    return () => document.removeEventListener("pointerdown", outside);
  }, [onDismiss]);

  return (
    <form
      ref={formRef}
      data-testid="favorites-folder-form"
      noValidate
      onSubmit={(event) => {
        event.preventDefault();
        onSubmit(value);
      }}
      className="flex w-full items-center gap-(--space-1) px-(--space-2)"
    >
      <input
        autoFocus
        value={value}
        onChange={(event) => setValue(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === "Escape") onDismiss();
        }}
        aria-label="Folder name"
        placeholder="Folder name"
        data-testid="favorites-folder-input"
        className="text-body min-w-0 flex-1 rounded-(--radius-control) border border-(--border) bg-(--bg) px-(--space-2) py-(--space-1) text-(--text)"
      />
      <button
        type="submit"
        aria-label="Save folder"
        className="shrink-0 cursor-pointer rounded-(--radius-control) p-(--space-1) text-(--text-muted) hover:bg-(--subtle)"
      >
        <Check size={14} strokeWidth={2} aria-hidden="true" />
      </button>
    </form>
  );
}
