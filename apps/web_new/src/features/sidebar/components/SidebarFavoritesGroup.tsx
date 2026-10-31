// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Sidebar favorites section (parity/specs/shell.md §4, SHELL-056/057/109).
// The collapsing Favorites group: folders with an inline validated
// create/rename form, entries deep-linking to their targets by entity
// type with a workspace-root fallback, hover-revealed drag handles
// reordering each list with server persistence, drops onto a folder
// nesting with a parent write and nested rows dragged out un-nesting,
// and quick menus for removal and folder rename. The section hides
// entirely while the top-level list is empty; the first add opens a
// closed section live through the SHELL-032 open flag. The route mounts
// this for admins and members only.

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

import { resolveDropTarget, type SidebarDropTarget } from "../../projects/index.js";
import { CrossAppLink } from "../../../shared/coexistence/index.js";
import { isFavoritesSectionOpen, notifyError, setFavoritesSectionOpen } from "../../../shared/shell/index.js";
import {
  FOLDER_DUPLICATE_TITLE,
  FOLDER_EMPTY_TITLE,
  favoriteOrderPatch,
  moveFavoriteToIndex,
  resolveIntoFolder,
  selectOrderedFavorites,
  selectTopLevelFavorites,
  sequenceForInsert,
  useCreateFavoriteFolder,
  useDeleteFavorite,
  useFavoriteGroup,
  useFavorites,
  useMoveFavorite,
  useRenameFavoriteFolder,
  validateFolderName,
  type FavoriteDropSpan,
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
 * Drop preview shared by every favorites list: the sibling slot in the
 * dragged list, a folder absorbing the row, or a slot in the top-level
 * list for rows dragged out of a folder. Only the dragging list ever
 * reports a non-empty preview.
 */
interface FavoriteDropPreview {
  slot: SidebarDropTarget | null;
  /** Folder absorbing the row, when the pointer holds its header band. */
  into: string | null;
  outer: SidebarDropTarget | null;
}

const EMPTY_DROP: FavoriteDropPreview = { slot: null, into: null, outer: null };

function inside(list: Element, x: number, y: number): boolean {
  const rect = list.getBoundingClientRect();
  return x >= rect.left && x <= rect.right && y >= rect.top && y <= rect.bottom;
}

/** Per-row drop marker: a sibling slot, a folder absorbing the row, or nothing. */
type FavoriteDropMarker = "above" | "below" | "into" | null;

function dropMarkerFor(preview: FavoriteDropPreview, rowId: string): FavoriteDropMarker {
  if (preview.into === rowId) return "into";
  const slot = preview.slot ?? preview.outer;
  return slot?.id === rowId ? (slot.after ? "below" : "above") : null;
}

/** Drag state shared by every favorites list, threaded as one prop. */
interface FavoriteDragState {
  preview: FavoriteDropPreview;
  report: (preview: FavoriteDropPreview) => void;
  topRows: readonly Pick<Favorite, "id" | "sequence">[];
  topListRef: React.RefObject<HTMLUListElement | null>;
}

/**
 * Pointer-drag organization for one favorites list (SHELL-056,
 * SHELL-109): the top-level list and every open folder's list each run
 * their own. Sibling drops reorder with a sequence write; drops onto a
 * folder header's middle band nest with a parent write; nested rows
 * released over the top-level list un-nest with a parent-clearing
 * write. Drops commit through the caller's move; releases elsewhere,
 * drops onto the row's own folder and no-op drops stay silent. Folder
 * sources never nest: over a folder band they reorder instead.
 */
function useFavoriteReorder(args: {
  rows: readonly Pick<Favorite, "id" | "sequence" | "parent" | "is_folder">[];
  groupId: string | null;
  commit: (input: FavoriteMoveInput) => void;
  report: (preview: FavoriteDropPreview) => void;
  /** Top-level rows for nested lists (drag-out target); null on the top list itself. */
  outer: {
    rows: readonly Pick<Favorite, "id" | "sequence">[];
    listRef: React.RefObject<HTMLUListElement | null>;
  } | null;
}): {
  listRef: React.RefObject<HTMLUListElement | null>;
  sourceId: string | null;
  onDragStart: (favoriteId: string) => void;
} {
  const [sourceId, setSourceId] = React.useState<string | null>(null);
  const dropRef = React.useRef<FavoriteDropPreview>(EMPTY_DROP);
  const listRef = React.useRef<HTMLUListElement>(null);

  React.useEffect(() => {
    if (sourceId === null) return;
    const outerConfig = args.outer;
    const spans = (list: HTMLUListElement | null): FavoriteDropSpan[] =>
      Array.from(list?.children ?? [], (node) => {
        const element = node as HTMLElement;
        const rect = element.getBoundingClientRect();
        // A row is a folder iff it renders the folder toggle, so one
        // lookup answers both questions; plain entries reuse the row.
        const toggle = element.querySelector('[data-testid="favorite-folder-toggle"]');
        const header = toggle?.getBoundingClientRect() ?? rect;
        return {
          id: element.dataset.favoriteRow ?? "",
          top: rect.top,
          bottom: rect.bottom,
          isFolder: !!toggle,
          headerTop: header.top,
          headerBottom: header.bottom,
          headerLeft: header.left,
          headerRight: header.right,
        };
      });
    const preview = (next: FavoriteDropPreview): void => {
      dropRef.current = next;
      args.report(next);
    };
    const move = (event: PointerEvent): void => {
      const x = event.clientX;
      const y = event.clientY;
      const own = spans(listRef.current);
      const outer = outerConfig === null ? [] : spans(outerConfig.listRef.current);
      const into = resolveIntoFolder([...own, ...outer], x, y, sourceId);
      const source = args.rows.find((row) => row.id === sourceId);
      if (into !== null) {
        if (into === source?.parent) {
          preview(EMPTY_DROP);
          return;
        }
        if (!source?.is_folder) {
          preview({ ...EMPTY_DROP, into });
          return;
        }
      }
      if (outerConfig !== null) {
        // Effects run after mount, so both lists are always present here.
        const topList = outerConfig.listRef.current as HTMLUListElement;
        const ownList = listRef.current as HTMLUListElement;
        if (inside(topList, x, y) && !inside(ownList, x, y)) {
          preview({ ...EMPTY_DROP, outer: resolveDropTarget(outer, y, sourceId) });
          return;
        }
      }
      preview({ ...EMPTY_DROP, slot: resolveDropTarget(own, y, sourceId) });
    };
    const finish = (commitDrop: boolean): void => {
      const drop = dropRef.current;
      dropRef.current = EMPTY_DROP;
      args.report(EMPTY_DROP);
      setSourceId(null);
      if (!commitDrop) return;
      if (drop.into !== null) {
        args.commit({ favoriteId: sourceId, groupId: args.groupId, parent: drop.into });
        return;
      }
      if (drop.outer !== null) {
        const outerRows = outerConfig?.rows ?? [];
        const target = drop.outer;
        const slot = outerRows.findIndex((row) => row.id === target.id);
        args.commit({
          favoriteId: sourceId,
          groupId: args.groupId,
          parent: null,
          sequence: sequenceForInsert(outerRows, target.after ? slot + 1 : slot),
        });
        return;
      }
      if (drop.slot === null) return;
      const ids = args.rows.map((row) => row.id);
      const rest = ids.filter((id) => id !== sourceId);
      const slot = rest.indexOf(drop.slot.id);
      const order = moveFavoriteToIndex(ids, sourceId, drop.slot.after ? slot + 1 : slot);
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
  }, [sourceId, args.rows, args.groupId, args.commit, args.report, args.outer]);

  return { listRef, sourceId, onDragStart: setSourceId };
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
  const [preview, setPreview] = React.useState<FavoriteDropPreview>(EMPTY_DROP);
  const reorder = useFavoriteReorder({
    rows: top,
    groupId: null,
    commit,
    report: setPreview,
    outer: null,
  });
  const drag = React.useMemo<FavoriteDragState>(
    () => ({ preview, report: setPreview, topRows: top, topListRef: reorder.listRef }),
    [preview, top, reorder.listRef]
  );

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
                drop={dropMarkerFor(preview, row.id)}
                isDragging={reorder.sourceId === row.id}
                onDragStart={reorder.onDragStart}
                drag={drag}
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
  drop: FavoriteDropMarker;
  isDragging: boolean;
  onDragStart: (favoriteId: string) => void;
  drag: FavoriteDragState;
}

function FavoriteRow({
  workspaceSlug,
  row,
  foldersOpen,
  onToggleFolder,
  onRename,
  onRemove,
  drop,
  isDragging,
  onDragStart,
  drag,
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
      {drop === "above" ? (
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
            data-drop-into={drop === "into" ? "true" : undefined}
            onClick={() => onToggleFolder(row.id)}
            className={drop === "into" ? `${ROW_LINK_TONE} ring-2 ring-(--accent)` : ROW_LINK_TONE}
          >
            <Icon size={16} strokeWidth={1.5} aria-hidden="true" />
            <span className="truncate">{name}</span>
          </button>
        ) : (
          <CrossAppLink to={resolveFavoriteHref(workspaceSlug, row)} className={ROW_LINK_TONE}>
            <Icon size={16} strokeWidth={1.5} aria-hidden="true" />
            <span className="truncate">{name}</span>
          </CrossAppLink>
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
          drag={drag}
        />
      ) : null}
      {drop === "below" ? (
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
  drag: FavoriteDragState;
}

function FavoriteFolderChildren({
  workspaceSlug,
  folderId,
  foldersOpen,
  onToggleFolder,
  onRename,
  onRemove,
  drag,
}: FavoriteFolderChildrenProps): React.ReactElement {
  const group = useFavoriteGroup(workspaceSlug, folderId);
  const rows = React.useMemo(() => selectOrderedFavorites(group.data ?? []), [group.data]);
  const mover = useMoveFavorite(workspaceSlug);
  const commit = React.useCallback((input: FavoriteMoveInput) => mover.mutate(input), [mover]);
  const outer = React.useMemo(
    () => ({ rows: drag.topRows, listRef: drag.topListRef }),
    [drag.topRows, drag.topListRef]
  );
  const reorder = useFavoriteReorder({
    rows,
    groupId: folderId,
    commit,
    report: drag.report,
    outer,
  });

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
          drop={dropMarkerFor(drag.preview, row.id)}
          isDragging={reorder.sourceId === row.id}
          onDragStart={reorder.onDragStart}
          drag={drag}
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
