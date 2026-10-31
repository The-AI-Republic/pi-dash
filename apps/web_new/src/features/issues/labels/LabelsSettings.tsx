// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Labels settings (parity/specs/issues.md §19, ISS-226–230): the
// two-level label list with inline create/edit, drag grouping, delete
// confirmation, and empty/loading states. Members read; admins edit;
// guests see the denial view. Groups collapse; drops preview an
// insertion line or a nesting ring.

import type { Label } from "@pidash/api-client";
import { Button, EmptyState, IconButton, Menu, Skeleton, cn, type MenuEntry } from "@pidash/kit";
import { ChevronDown, ChevronRight, Ellipsis, GripVertical, Tags, Trash2 } from "lucide-react";
import * as React from "react";

import { ROLE_ADMIN } from "../../../core/session/permissions.js";
import { useProjectRoles, useWorkspaceMembership } from "../../auth/index.js";
import { isProjectAdmin, isProjectAuthorized } from "../../projects/index.js";
import { MENU_PORTAL_ID, OverlayPortal, notifyError } from "../../../shared/shell/index.js";
import {
  LABELS_LOAD_FAILED_TITLE,
  useCreateLabel,
  useLabels,
  useMoveLabel,
  useUpdateLabel,
  type LabelMoveInput,
} from "./api.js";
import { LabelDeleteDialog } from "./LabelDeleteDialog.js";
import { LabelForm, type LabelFormValue } from "./LabelForm.js";
import {
  effectiveLabelChildren,
  EMPTY_LABEL_DROP,
  readLabelDropSpans,
  resolveLabelChildSlot,
  resolveLabelDrop,
  resolveLabelInto,
  resolveLabelSlot,
  type LabelDropPreview,
} from "./drops.js";
import { selectLabelTree, type LabelGroup } from "./tree.js";

export interface LabelsSettingsProps {
  workspaceSlug: string;
  projectId: string;
}

type LabelFormState = { mode: "create" } | { mode: "edit"; labelId: string };

/** Drop marker for one row: a sibling line, a nesting ring, or nothing. */
type LabelDropMarker = "above" | "below" | "into" | null;

/** Where the drag chip follows the pointer. */
interface PointerPosition {
  x: number;
  y: number;
}

function dropMarkerFor(preview: LabelDropPreview, rowId: string): LabelDropMarker {
  if (preview.into === rowId) return "into";
  if (preview.slot?.id !== rowId) return null;
  return preview.slot.after ? "below" : "above";
}

/**
 * Pointer-drag grouping for the label list: grip handles start a drag,
 * the pointer previews a sibling slot or a nesting target, and the
 * release commits the resolved move. Releases elsewhere, cancelled
 * drags, and no-op drops stay silent.
 */
function useLabelReorder(args: { labels: readonly Label[]; commit: (input: LabelMoveInput) => void }): {
  listRef: React.RefObject<HTMLUListElement | null>;
  sourceId: string | null;
  pointer: PointerPosition | null;
  preview: LabelDropPreview;
  onGripDown: (labelId: string) => void;
} {
  const { labels, commit } = args;
  const [sourceId, setSourceId] = React.useState<string | null>(null);
  const [pointer, setPointer] = React.useState<PointerPosition | null>(null);
  const [preview, setPreview] = React.useState<LabelDropPreview>(EMPTY_LABEL_DROP);
  const listRef = React.useRef<HTMLUListElement>(null);
  const dropRef = React.useRef<LabelDropPreview>(EMPTY_LABEL_DROP);

  React.useEffect(() => {
    if (sourceId === null) return;
    const spans = () => readLabelDropSpans(listRef.current);
    const report = (next: LabelDropPreview): void => {
      dropRef.current = next;
      setPreview(next);
    };
    const move = (event: PointerEvent): void => {
      setPointer({ x: event.clientX, y: event.clientY });
      const own = spans();
      const into = resolveLabelInto(own, event.clientX, event.clientY, sourceId);
      if (into !== null) {
        if (effectiveLabelChildren(labels, sourceId).length > 0) {
          report({ slot: { id: into, after: false }, into: null });
        } else {
          report({ slot: null, into });
        }
        return;
      }
      const child = resolveLabelChildSlot(own, event.clientX, event.clientY, sourceId);
      if (child !== null) {
        report({ slot: child, into: null });
        return;
      }
      report({ slot: resolveLabelSlot(own, event.clientY, sourceId), into: null });
    };
    const finish = (commitDrop: boolean): void => {
      const drop = dropRef.current;
      dropRef.current = EMPTY_LABEL_DROP;
      setPreview(EMPTY_LABEL_DROP);
      setPointer(null);
      setSourceId(null);
      if (!commitDrop) return;
      const resolved = resolveLabelDrop({ labels, sourceId, slot: drop.slot, into: drop.into });
      if (resolved === null) return;
      commit({
        labelId: sourceId,
        parent: resolved.parent,
        ...(resolved.sortOrder === undefined ? {} : { sortOrder: resolved.sortOrder }),
      });
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
  }, [sourceId, labels, commit]);

  return { listRef, sourceId, pointer, preview, onGripDown: setSourceId };
}

/**
 * The chip that follows the pointer during a drag: the dragged label's
 * swatch and name, taken out of pointer events so it never becomes a
 * drop target itself.
 */
function LabelDragChip({ label, pointer }: { label: Label; pointer: PointerPosition }): React.ReactNode {
  const color = label.color;
  return (
    <OverlayPortal rootId={MENU_PORTAL_ID}>
      <div
        aria-hidden="true"
        data-testid="label-drag-chip"
        style={{ transform: `translate3d(${pointer.x + 12}px, ${pointer.y + 12}px, 0)` }}
        className="shadow-lg pointer-events-none fixed top-0 left-0 z-50 flex items-center gap-(--space-2) rounded-(--radius-control) border border-(--border) bg-(--surface) px-(--space-2) py-(--space-1)"
      >
        <span aria-hidden="true" style={{ backgroundColor: color }} className="size-2.5 shrink-0 rounded-full" />
        <span className="text-body max-w-56 truncate text-(--text)">{label.name}</span>
      </div>
    </OverlayPortal>
  );
}

/** Four-row loading skeleton for the label list. */
function LabelsSkeleton(): React.ReactElement {
  return (
    <div
      role="status"
      aria-label="Loading labels"
      data-testid="labels-skeleton"
      className="flex flex-col gap-(--space-3)"
    >
      {[0, 1, 2, 3].map((index) => (
        <div key={index} className="flex items-center gap-(--space-3)">
          <Skeleton round width="20px" height="20px" />
          <Skeleton width="160px" height="14px" />
        </div>
      ))}
    </div>
  );
}

interface LabelRowChromeProps {
  row: Label;
  canEdit: boolean;
  marker: LabelDropMarker;
  dragging: boolean;
  indented: boolean;
  menuItems: MenuEntry[];
  onGripDown: (labelId: string) => void;
  onDelete: (row: Label) => void;
  children: React.ReactNode;
}

/** One slot row: grip, marker lines, and the caller's body. */
function LabelRowChrome({
  row,
  canEdit,
  marker,
  dragging,
  indented,
  menuItems,
  onGripDown,
  onDelete,
  children,
}: LabelRowChromeProps): React.ReactElement {
  return (
    <li data-testid="label-row" data-label-id={row.id} className="relative list-none">
      {marker === "above" ? (
        <span
          data-testid="label-drop-line"
          data-position="above"
          className="absolute inset-x-2 top-0 h-0.5 bg-(--accent)"
        />
      ) : null}
      <div
        data-label-slot
        data-label-id={row.id}
        data-label-parent={row.parent ?? undefined}
        data-nestable={indented ? "false" : "true"}
        data-drop-into={marker === "into" ? "true" : undefined}
        className={cn(
          "group/row relative flex w-full items-center gap-(--space-2) rounded-(--radius-control) px-(--space-2) py-(--space-2)",
          indented && "ml-(--space-8)",
          dragging && "opacity-60",
          marker === "into" && "ring-2 ring-(--accent)"
        )}
      >
        {canEdit ? (
          <button
            type="button"
            aria-label={`Rearrange ${row.name}`}
            title="Drag to reorder"
            data-testid="label-drag-handle"
            onPointerDown={(event) => {
              event.preventDefault();
              onGripDown(row.id);
            }}
            className="hidden shrink-0 cursor-grab items-center justify-center rounded-(--radius-control) p-px text-(--text-muted) group-hover/row:flex"
          >
            <GripVertical size={14} strokeWidth={2} aria-hidden="true" />
          </button>
        ) : null}
        {children}
        {canEdit ? (
          <span className="ml-auto flex shrink-0 items-center">
            <Menu
              trigger={
                <IconButton
                  label={`Actions for ${row.name}`}
                  data-testid="label-row-menu"
                  className="opacity-0 group-hover/row:opacity-100 focus-visible:opacity-100"
                >
                  <Ellipsis size={16} strokeWidth={1.5} aria-hidden="true" />
                </IconButton>
              }
              items={menuItems}
            />
            <IconButton label={`Delete ${row.name}`} data-testid="label-delete" onClick={() => onDelete(row)}>
              <Trash2 size={16} strokeWidth={1.5} aria-hidden="true" />
            </IconButton>
          </span>
        ) : null}
      </div>
      {marker === "below" ? (
        <span
          data-testid="label-drop-line"
          data-position="below"
          className="absolute inset-x-2 bottom-0 h-0.5 bg-(--accent)"
        />
      ) : null}
    </li>
  );
}

export function LabelsSettings({ workspaceSlug, projectId }: LabelsSettingsProps): React.ReactElement {
  const labelsQuery = useLabels(workspaceSlug, projectId);
  const rolesQuery = useProjectRoles(workspaceSlug);
  const membershipQuery = useWorkspaceMembership(workspaceSlug);
  const create = useCreateLabel(workspaceSlug, projectId);
  const update = useUpdateLabel(workspaceSlug, projectId);
  const mover = useMoveLabel(workspaceSlug, projectId);
  const [form, setForm] = React.useState<LabelFormState | null>(null);
  const [deleteTarget, setDeleteTarget] = React.useState<Label | null>(null);
  const [collapsed, setCollapsed] = React.useState<ReadonlySet<string>>(new Set());

  const rows = labelsQuery.data ?? [];
  const tree = React.useMemo(() => selectLabelTree(rows), [rows]);

  const loadFailed = labelsQuery.isError;
  React.useEffect(() => {
    if (loadFailed) notifyError(LABELS_LOAD_FAILED_TITLE);
  }, [loadFailed]);

  // Standing errors fail open for viewing but closed for editing: the
  // list is readable reference data either way, while writes stay
  // server-enforced.
  const standingFailed = rolesQuery.isError || membershipQuery.isError;
  const projectRole = rolesQuery.data?.[projectId] ?? null;
  const workspaceRole = membershipQuery.data?.role ?? 0;
  const canView = standingFailed || isProjectAuthorized(projectRole) || workspaceRole >= ROLE_ADMIN;
  const canEdit = !standingFailed && (isProjectAdmin(projectRole) || workspaceRole >= ROLE_ADMIN);

  const commitMove = React.useCallback((input: LabelMoveInput) => mover.mutate(input), [mover]);
  const drag = useLabelReorder({ labels: rows, commit: commitMove });

  function toggleGroup(labelId: string): void {
    setCollapsed((previous) => {
      const next = new Set(previous);
      if (next.has(labelId)) next.delete(labelId);
      else next.add(labelId);
      return next;
    });
  }

  const submitEdit = React.useCallback(
    (target: Label, value: LabelFormValue) => {
      update.mutate(
        { labelId: target.id, patch: { name: value.name, color: value.color } },
        { onSuccess: () => setForm(null) }
      );
    },
    [update]
  );

  const pending = labelsQuery.isPending || rolesQuery.isPending || membershipQuery.isPending;
  const creating = form?.mode === "create";

  if (!pending && !canView) {
    return (
      <section
        aria-label="Labels"
        data-testid="labels-settings"
        className="flex flex-col gap-(--space-6) p-(--space-8)"
      >
        <h2 className="text-h3 text-(--text)">Labels</h2>
        <div className="flex flex-col items-center gap-(--space-2) py-(--space-16) text-center">
          <h3 className="text-h3 text-(--text)">Not authorized to view labels</h3>
          <p className="text-body text-(--text-muted)">Members of this project can view its labels.</p>
        </div>
      </section>
    );
  }

  return (
    <section aria-label="Labels" data-testid="labels-settings" className="flex flex-col gap-(--space-6) p-(--space-8)">
      <div className="flex items-center justify-between gap-(--space-4)">
        <h2 className="text-h3 text-(--text)">Labels</h2>
        {canEdit && form === null ? (
          <Button data-testid="label-add" onClick={() => setForm({ mode: "create" })}>
            Add label
          </Button>
        ) : null}
      </div>
      {creating ? (
        <LabelForm
          mode="create"
          pending={create.isPending}
          onSubmit={(value) =>
            create.mutate({ name: value.name, color: value.color }, { onSuccess: () => setForm(null) })
          }
          onCancel={() => setForm(null)}
        />
      ) : null}
      {pending && !creating ? <LabelsSkeleton /> : null}
      {!pending && tree.length === 0 && !creating ? (
        <EmptyState
          variant="marketing"
          art={{ category: "general", type: "empty" }}
          title="No labels yet"
          description="Create labels to organize and filter the project's work."
          {...(canEdit
            ? {
                primaryAction: {
                  label: "Create your first label",
                  onSelect: () => setForm({ mode: "create" }),
                },
              }
            : {})}
        />
      ) : null}
      {!pending && tree.length > 0 ? (
        <div className="max-h-160 overflow-y-auto">
          <ul ref={drag.listRef} aria-label="Project labels" className="flex flex-col gap-(--space-1)">
            {tree.map((group) => (
              <LabelGroupCard
                key={group.label.id}
                group={group}
                canEdit={canEdit}
                collapsed={collapsed.has(group.label.id)}
                preview={drag.preview}
                sourceId={drag.sourceId}
                form={form}
                updatePending={update.isPending}
                onToggleGroup={toggleGroup}
                onGripDown={drag.onGripDown}
                onEdit={(row) => setForm({ mode: "edit", labelId: row.id })}
                onUngroup={(row) => mover.mutate({ labelId: row.id, parent: null })}
                onDelete={setDeleteTarget}
                onSubmitEdit={submitEdit}
                onCancelForm={() => setForm(null)}
              />
            ))}
          </ul>
        </div>
      ) : null}
      <DraggedLabelChip rows={rows} sourceId={drag.sourceId} pointer={drag.pointer} />
      <LabelDeleteDialog
        workspaceSlug={workspaceSlug}
        projectId={projectId}
        label={deleteTarget}
        onClose={() => setDeleteTarget(null)}
      />
    </section>
  );
}

/** Render the chip only while a known row is dragged under the pointer. */
function DraggedLabelChip({
  rows,
  sourceId,
  pointer,
}: {
  rows: readonly Label[];
  sourceId: string | null;
  pointer: PointerPosition | null;
}): React.ReactNode {
  const label = sourceId === null ? undefined : rows.find((row) => row.id === sourceId);
  if (label === undefined || pointer === null) return null;
  return <LabelDragChip label={label} pointer={pointer} />;
}

interface LabelGroupCardProps {
  group: LabelGroup;
  canEdit: boolean;
  collapsed: boolean;
  preview: LabelDropPreview;
  sourceId: string | null;
  form: LabelFormState | null;
  updatePending: boolean;
  onToggleGroup: (labelId: string) => void;
  onGripDown: (labelId: string) => void;
  onEdit: (row: Label) => void;
  onUngroup: (row: Label) => void;
  onDelete: (row: Label) => void;
  onSubmitEdit: (target: Label, value: LabelFormValue) => void;
  onCancelForm: () => void;
}

function LabelGroupCard({
  group,
  canEdit,
  collapsed,
  preview,
  sourceId,
  form,
  updatePending,
  onToggleGroup,
  onGripDown,
  onEdit,
  onUngroup,
  onDelete,
  onSubmitEdit,
  onCancelForm,
}: LabelGroupCardProps): React.ReactElement {
  const { label, children } = group;
  const editing = form?.mode === "edit" && form.labelId === label.id;
  const marker = dropMarkerFor(preview, label.id);
  const dragging = sourceId === label.id;

  function plainMenuItems(row: Label): MenuEntry[] {
    return [{ label: "Edit label", onSelect: () => onEdit(row) }];
  }

  function childMenuItems(row: Label): MenuEntry[] {
    return [
      { label: "Remove from group", onSelect: () => onUngroup(row) },
      { label: "Edit label", onSelect: () => onEdit(row) },
    ];
  }

  function groupMenuItems(): MenuEntry[] {
    return [
      { label: "Edit label", onSelect: () => onEdit(label) },
      { label: "Delete label", onSelect: () => onDelete(label) },
    ];
  }

  if (children.length === 0) {
    return (
      <LabelRowChrome
        row={label}
        canEdit={canEdit}
        marker={marker}
        dragging={dragging}
        indented={false}
        menuItems={plainMenuItems(label)}
        onGripDown={onGripDown}
        onDelete={onDelete}
      >
        {editing ? (
          <div className="min-w-0 flex-1">
            <LabelForm
              mode="edit"
              initialName={label.name}
              initialColor={label.color}
              pending={updatePending}
              onSubmit={(value) => onSubmitEdit(label, value)}
              onCancel={onCancelForm}
            />
          </div>
        ) : (
          <>
            <span
              aria-hidden="true"
              style={{ backgroundColor: label.color }}
              className="size-2.5 shrink-0 rounded-full"
            />
            <span data-testid="label-row-name" className="text-body min-w-0 flex-1 truncate text-(--text)">
              {label.name}
            </span>
          </>
        )}
      </LabelRowChrome>
    );
  }

  return (
    <li data-testid="label-row" data-label-id={label.id} data-group="true" className="relative list-none">
      {marker === "above" ? (
        <span
          data-testid="label-drop-line"
          data-position="above"
          className="absolute inset-x-2 top-0 h-0.5 bg-(--accent)"
        />
      ) : null}
      {editing ? (
        <div className="min-w-0">
          <LabelForm
            mode="edit"
            initialName={label.name}
            initialColor={label.color}
            pending={updatePending}
            onSubmit={(value) => onSubmitEdit(label, value)}
            onCancel={onCancelForm}
          />
        </div>
      ) : (
        <div
          data-label-slot
          data-label-id={label.id}
          data-label-parent={label.parent ?? undefined}
          data-nestable="true"
          data-drop-into={marker === "into" ? "true" : undefined}
          className={cn(
            "group/row relative flex w-full items-center gap-(--space-2) rounded-(--radius-control) border border-(--border) bg-(--subtle) px-(--space-2) py-(--space-2)",
            dragging && "opacity-60",
            marker === "into" && "ring-2 ring-(--accent)"
          )}
        >
          {canEdit ? (
            <button
              type="button"
              aria-label={`Rearrange ${label.name}`}
              title="Drag to reorder"
              data-testid="label-drag-handle"
              onPointerDown={(event) => {
                event.preventDefault();
                onGripDown(label.id);
              }}
              className="hidden shrink-0 cursor-grab items-center justify-center rounded-(--radius-control) p-px text-(--text-muted) group-hover/row:flex"
            >
              <GripVertical size={14} strokeWidth={2} aria-hidden="true" />
            </button>
          ) : null}
          <button
            type="button"
            aria-expanded={!collapsed}
            aria-label={`${collapsed ? "Expand" : "Collapse"} ${label.name}`}
            data-testid="label-group-toggle"
            onClick={() => onToggleGroup(label.id)}
            className="flex shrink-0 cursor-pointer items-center text-(--text-muted)"
          >
            {collapsed ? (
              <ChevronRight size={16} strokeWidth={1.5} aria-hidden="true" />
            ) : (
              <ChevronDown size={16} strokeWidth={1.5} aria-hidden="true" />
            )}
          </button>
          <Tags size={16} strokeWidth={1.5} aria-hidden="true" style={{ color: label.color }} />
          <span data-testid="label-row-name" className="text-body min-w-0 flex-1 truncate font-medium text-(--text)">
            {label.name}
          </span>
          {canEdit ? (
            <span className="ml-auto flex shrink-0 items-center">
              <Menu
                trigger={
                  <IconButton
                    label={`Actions for ${label.name}`}
                    data-testid="label-row-menu"
                    className="opacity-0 group-hover/row:opacity-100 focus-visible:opacity-100"
                  >
                    <Ellipsis size={16} strokeWidth={1.5} aria-hidden="true" />
                  </IconButton>
                }
                items={groupMenuItems()}
              />
            </span>
          ) : null}
        </div>
      )}
      {collapsed
        ? null
        : children.map((child) => {
            const childEditing = form?.mode === "edit" && form.labelId === child.id;
            return (
              <ul key={child.id} className="flex flex-col">
                <LabelRowChrome
                  row={child}
                  canEdit={canEdit}
                  marker={dropMarkerFor(preview, child.id)}
                  dragging={sourceId === child.id}
                  indented
                  menuItems={childMenuItems(child)}
                  onGripDown={onGripDown}
                  onDelete={onDelete}
                >
                  {childEditing ? (
                    <div className="min-w-0 flex-1">
                      <LabelForm
                        mode="edit"
                        initialName={child.name}
                        initialColor={child.color}
                        pending={updatePending}
                        onSubmit={(value) => onSubmitEdit(child, value)}
                        onCancel={onCancelForm}
                      />
                    </div>
                  ) : (
                    <>
                      <span
                        aria-hidden="true"
                        style={{ backgroundColor: child.color }}
                        className="size-2.5 shrink-0 rounded-full"
                      />
                      <span data-testid="label-row-name" className="text-body min-w-0 flex-1 truncate text-(--text)">
                        {child.name}
                      </span>
                    </>
                  )}
                </LabelRowChrome>
              </ul>
            );
          })}
      {marker === "below" ? (
        <span
          data-testid="label-drop-line"
          data-position="below"
          className="absolute inset-x-2 bottom-0 h-0.5 bg-(--accent)"
        />
      ) : null}
    </li>
  );
}
