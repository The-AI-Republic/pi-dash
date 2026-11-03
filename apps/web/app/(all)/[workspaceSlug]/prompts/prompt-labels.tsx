/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useMemo, useState } from "react";
import useSWR from "swr";
import { useTranslation } from "@pi-dash/i18n";
import { TOAST_TYPE, setToast } from "@pi-dash/propel/toast";
import type { IPromptLabel, IPromptLabelListResponse, TPromptLabelTargetType } from "@pi-dash/types";
import { Button, ColorPicker, EModalPosition, EModalWidth, ModalCore } from "@pi-dash/ui";
import { usePromptLabel } from "@/hooks/store/use-prompt-label";

const DEFAULT_NEW_LABEL_COLOR = "#6b7280";

function targetMapKey(targetType: TPromptLabelTargetType, targetKey: string): string {
  return `${targetType}:${targetKey}`;
}

/** Extract a friendly error message from the service's thrown `err.response.data`. */
function labelError(e: unknown, fallback: string): string {
  const err = e as { error?: string; detail?: string } | null;
  return err?.detail ?? err?.error ?? fallback;
}

/**
 * Fetch the workspace's prompt labels once and expose helpers to look them up
 * per target (section/receipt). The prompts page holds the SWR cache so chips
 * everywhere on the page stay in sync after an attach/detach/edit.
 */
export function useWorkspaceLabels(slug: string) {
  const promptLabelStore = usePromptLabel();
  const key = slug ? (["prompt-labels", slug] as const) : null;
  const { data, error, isLoading, mutate } = useSWR<IPromptLabelListResponse>(key, () =>
    promptLabelStore.fetchLabels(slug)
  );
  const labels = useMemo<IPromptLabel[]>(() => data?.labels ?? [], [data]);

  const labelsByTarget = useMemo(() => {
    const map = new Map<string, IPromptLabel[]>();
    for (const label of labels) {
      for (const target of label.targets) {
        const mapKey = targetMapKey(target.target_type, target.target_key);
        const bucket = map.get(mapKey);
        if (bucket) bucket.push(label);
        else map.set(mapKey, [label]);
      }
    }
    return map;
  }, [labels]);

  const labelsFor = useMemo(
    () =>
      (targetType: TPromptLabelTargetType, targetKey: string): IPromptLabel[] =>
        labelsByTarget.get(targetMapKey(targetType, targetKey)) ?? [],
    [labelsByTarget]
  );

  return { labels, labelsFor, error, isLoading, mutate };
}

/** True when `label` is attached to the given target. */
export function labelHasTarget(label: IPromptLabel, targetType: TPromptLabelTargetType, targetKey: string): boolean {
  return label.targets.some((t) => t.target_type === targetType && t.target_key === targetKey);
}

/** Count of each target type a label carries, for the filter summary. */
export function labelTargetCounts(label: IPromptLabel): { sections: number; receipts: number } {
  let sections = 0;
  let receipts = 0;
  for (const t of label.targets) {
    if (t.target_type === "section") sections += 1;
    else if (t.target_type === "receipt") receipts += 1;
  }
  return { sections, receipts };
}

type ChipProps = {
  label: IPromptLabel;
  active?: boolean;
  onClick?: () => void;
  /** Optional detach affordance (admin, on a specific card). */
  onRemove?: () => void;
  title?: string;
};

/** A small colored, clickable label chip. */
export function PromptLabelChip({ label, active = false, onClick, onRemove, title }: ChipProps) {
  return (
    <span
      className={`inline-flex items-center gap-1 rounded-full border py-0.5 pr-1 pl-1.5 text-11 transition-colors ${
        active ? "border-accent-strong bg-layer-2 text-primary" : "border-subtle text-secondary hover:text-primary"
      }`}
      style={active ? undefined : { borderColor: `${label.color}66` }}
    >
      <button type="button" onClick={onClick} className="inline-flex items-center gap-1" title={title}>
        <span className="size-2 shrink-0 rounded-full" style={{ backgroundColor: label.color }} />
        <span className="max-w-[10rem] truncate">{label.name}</span>
      </button>
      {onRemove && (
        <button
          type="button"
          onClick={onRemove}
          aria-label="Remove label"
          className="ml-0.5 rounded-full px-1 text-placeholder hover:text-danger-primary"
        >
          ×
        </button>
      )}
    </span>
  );
}

type CardLabelsProps = {
  slug: string;
  targetType: TPromptLabelTargetType;
  targetKey: string;
  labels: IPromptLabel[];
  allLabels: IPromptLabel[];
  activeLabelId: string | null;
  isAdmin: boolean;
  onSelect: (labelId: string) => void;
  onChanged: () => Promise<unknown>;
};

/**
 * The row of chips shown on a section/receipt card: the labels attached to this
 * target (clickable to filter), plus an admin "＋ Label" attach/detach menu.
 */
export function PromptCardLabels({
  slug,
  targetType,
  targetKey,
  labels,
  allLabels,
  activeLabelId,
  isAdmin,
  onSelect,
  onChanged,
}: CardLabelsProps) {
  const { t } = useTranslation();
  const promptLabelStore = usePromptLabel();
  const [busyId, setBusyId] = useState<string | null>(null);

  async function toggle(label: IPromptLabel) {
    if (busyId) return;
    setBusyId(label.id);
    const attached = labelHasTarget(label, targetType, targetKey);
    try {
      if (attached)
        await promptLabelStore.detachLabel(slug, label.id, { target_type: targetType, target_key: targetKey });
      else await promptLabelStore.attachLabel(slug, label.id, { target_type: targetType, target_key: targetKey });
      await onChanged();
    } catch (e: unknown) {
      setToast({
        type: TOAST_TYPE.ERROR,
        title: t("Something went wrong"),
        message: labelError(e, t("Could not update the label.")),
      });
    } finally {
      setBusyId(null);
    }
  }

  async function detach(label: IPromptLabel) {
    if (busyId) return;
    setBusyId(label.id);
    try {
      await promptLabelStore.detachLabel(slug, label.id, { target_type: targetType, target_key: targetKey });
      await onChanged();
    } catch (e: unknown) {
      setToast({
        type: TOAST_TYPE.ERROR,
        title: t("Something went wrong"),
        message: labelError(e, t("Could not remove the label.")),
      });
    } finally {
      setBusyId(null);
    }
  }

  if (labels.length === 0 && !isAdmin) return null;

  return (
    <div className="flex flex-wrap items-center gap-1.5">
      {labels.map((label) => (
        <PromptLabelChip
          key={label.id}
          label={label}
          active={activeLabelId === label.id}
          onClick={() => onSelect(label.id)}
          onRemove={isAdmin ? () => detach(label) : undefined}
          title={t("Filter by this label")}
        />
      ))}
      {isAdmin && (
        <details className="relative">
          <summary className="flex cursor-pointer list-none items-center rounded-full border border-dashed border-subtle px-2 py-0.5 text-11 text-secondary hover:text-primary">
            {t("＋ Label")}
          </summary>
          <div className="absolute left-0 z-20 mt-1 max-h-64 w-56 overflow-auto rounded-md border border-subtle bg-surface-1 p-1 shadow-raised-200">
            {allLabels.length === 0 ? (
              <p className="px-2 py-1.5 text-11 text-secondary">
                {t("No labels yet. Create one from “Manage labels”.")}
              </p>
            ) : (
              allLabels.map((label) => {
                const attached = labelHasTarget(label, targetType, targetKey);
                return (
                  <button
                    key={label.id}
                    type="button"
                    onClick={() => toggle(label)}
                    disabled={busyId === label.id}
                    className="flex w-full items-center gap-2 rounded px-2 py-1.5 text-left text-12 text-primary hover:bg-layer-2 disabled:opacity-60"
                  >
                    <span
                      className="flex size-3.5 shrink-0 items-center justify-center rounded-sm border border-subtle text-10 text-on-color"
                      style={{ backgroundColor: attached ? label.color : "transparent" }}
                    >
                      {attached ? "✓" : ""}
                    </span>
                    <span className="size-2 shrink-0 rounded-full" style={{ backgroundColor: label.color }} />
                    <span className="flex-1 truncate">{label.name}</span>
                  </button>
                );
              })
            )}
          </div>
        </details>
      )}
    </div>
  );
}

type BarProps = {
  slug: string;
  labels: IPromptLabel[];
  activeLabelId: string | null;
  isAdmin: boolean;
  onSelect: (labelId: string | null) => void;
  onChanged: () => Promise<unknown>;
};

/**
 * Top-of-page label bar: every workspace label as a clickable filter chip, a
 * "clear filter" control, and (for admins) a "Manage labels" button that opens
 * the create/rename/recolor/delete modal.
 */
export function PromptLabelBar({ slug, labels, activeLabelId, isAdmin, onSelect, onChanged }: BarProps) {
  const { t } = useTranslation();
  const [managing, setManaging] = useState(false);
  const activeLabel = labels.find((l) => l.id === activeLabelId) ?? null;
  const activeCounts = activeLabel ? labelTargetCounts(activeLabel) : null;

  return (
    <div className="flex flex-col gap-2 rounded-md border border-subtle bg-layer-1 px-4 py-3">
      <div className="flex items-center justify-between gap-2">
        <span className="text-11 font-medium text-secondary">{t("Labels")}</span>
        {isAdmin && (
          <Button size="sm" variant="outline-primary" onClick={() => setManaging(true)}>
            {t("Manage labels")}
          </Button>
        )}
      </div>
      {labels.length === 0 ? (
        <p className="text-11 text-secondary">
          {isAdmin
            ? t("No labels yet. Create labels to tag sections and receipts, then click a label to filter.")
            : t("No labels yet.")}
        </p>
      ) : (
        <div className="flex flex-wrap items-center gap-1.5">
          {labels.map((label) => (
            <PromptLabelChip
              key={label.id}
              label={label}
              active={activeLabelId === label.id}
              onClick={() => onSelect(activeLabelId === label.id ? null : label.id)}
              title={t("Filter by this label")}
            />
          ))}
          {activeLabel && (
            <button
              type="button"
              onClick={() => onSelect(null)}
              className="ml-1 rounded-full border border-subtle px-2 py-0.5 text-11 text-secondary hover:text-primary"
            >
              {t("Clear filter")}
            </button>
          )}
        </div>
      )}
      {activeLabel && activeCounts && (
        <p className="text-11 text-secondary">
          {t("Showing items labelled “{{name}}” — {{sections}} sections, {{receipts}} receipts.", {
            name: activeLabel.name,
            sections: activeCounts.sections,
            receipts: activeCounts.receipts,
          })}
        </p>
      )}
      {isAdmin && (
        <PromptLabelManagerModal
          slug={slug}
          labels={labels}
          isOpen={managing}
          onClose={() => setManaging(false)}
          onChanged={onChanged}
        />
      )}
    </div>
  );
}

type ManagerProps = {
  slug: string;
  labels: IPromptLabel[];
  isOpen: boolean;
  onClose: () => void;
  onChanged: () => Promise<unknown>;
};

function PromptLabelManagerModal({ slug, labels, isOpen, onClose, onChanged }: ManagerProps) {
  const { t } = useTranslation();
  const promptLabelStore = usePromptLabel();

  const [newName, setNewName] = useState("");
  const [newColor, setNewColor] = useState(DEFAULT_NEW_LABEL_COLOR);
  const [creating, setCreating] = useState(false);
  const [busyId, setBusyId] = useState<string | null>(null);

  async function handleCreate() {
    const name = newName.trim();
    if (!name || creating) return;
    setCreating(true);
    try {
      await promptLabelStore.createLabel(slug, { name, color: newColor });
      await onChanged();
      setNewName("");
      setNewColor(DEFAULT_NEW_LABEL_COLOR);
      setToast({ type: TOAST_TYPE.SUCCESS, title: t("Label created") });
    } catch (e: unknown) {
      setToast({
        type: TOAST_TYPE.ERROR,
        title: t("Something went wrong"),
        message: labelError(e, t("Could not create the label.")),
      });
    } finally {
      setCreating(false);
    }
  }

  async function handleRename(label: IPromptLabel, name: string) {
    const next = name.trim();
    if (!next || next === label.name || busyId) return;
    setBusyId(label.id);
    try {
      await promptLabelStore.updateLabel(slug, label.id, { name: next });
      await onChanged();
    } catch (e: unknown) {
      setToast({
        type: TOAST_TYPE.ERROR,
        title: t("Something went wrong"),
        message: labelError(e, t("Could not rename the label.")),
      });
    } finally {
      setBusyId(null);
    }
  }

  async function handleRecolor(label: IPromptLabel, color: string) {
    if (busyId) return;
    setBusyId(label.id);
    try {
      await promptLabelStore.updateLabel(slug, label.id, { color });
      await onChanged();
    } catch (e: unknown) {
      setToast({
        type: TOAST_TYPE.ERROR,
        title: t("Something went wrong"),
        message: labelError(e, t("Could not recolor the label.")),
      });
    } finally {
      setBusyId(null);
    }
  }

  async function handleDelete(label: IPromptLabel) {
    if (busyId) return;
    setBusyId(label.id);
    try {
      await promptLabelStore.deleteLabel(slug, label.id);
      await onChanged();
    } catch (e: unknown) {
      setToast({
        type: TOAST_TYPE.ERROR,
        title: t("Something went wrong"),
        message: labelError(e, t("Could not delete the label.")),
      });
    } finally {
      setBusyId(null);
    }
  }

  return (
    <ModalCore isOpen={isOpen} handleClose={onClose} position={EModalPosition.CENTER} width={EModalWidth.XL}>
      <div className="flex flex-col gap-4 p-5">
        <div className="flex items-center justify-between">
          <h2 className="text-14 font-semibold text-primary">{t("Manage labels")}</h2>
          <button type="button" onClick={onClose} className="text-13 text-secondary hover:text-primary">
            {t("Close")}
          </button>
        </div>

        <div className="flex items-end gap-2 rounded-md border border-subtle bg-layer-1 p-3">
          <div className="flex flex-1 flex-col gap-1">
            <label className="text-11 font-medium text-secondary">{t("New label")}</label>
            <input
              value={newName}
              onChange={(e) => setNewName(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") handleCreate();
              }}
              placeholder={t("Label name")}
              maxLength={64}
              className="rounded-md border border-subtle bg-surface-1 px-3 py-1.5 text-12 text-primary focus:border-accent-strong focus:outline-none"
            />
          </div>
          <div className="flex flex-col items-center gap-1">
            <span className="text-11 text-secondary">{t("Color")}</span>
            <ColorPicker value={newColor} onChange={setNewColor} />
          </div>
          <Button size="sm" onClick={handleCreate} loading={creating} disabled={!newName.trim() || creating}>
            {t("Create")}
          </Button>
        </div>

        <div className="flex max-h-[50vh] flex-col gap-1 overflow-auto">
          {labels.length === 0 ? (
            <p className="px-1 py-2 text-12 text-secondary">{t("No labels yet.")}</p>
          ) : (
            labels.map((label) => {
              const counts = labelTargetCounts(label);
              return (
                <div
                  key={label.id}
                  className="flex items-center gap-2 rounded-md border border-subtle bg-layer-1 px-3 py-2"
                >
                  <ColorPicker value={label.color} onChange={(c) => handleRecolor(label, c)} />
                  <input
                    defaultValue={label.name}
                    maxLength={64}
                    onBlur={(e) => handleRename(label, e.target.value)}
                    onKeyDown={(e) => {
                      if (e.key === "Enter") (e.target as HTMLInputElement).blur();
                    }}
                    className="flex-1 rounded-md border border-transparent bg-transparent px-2 py-1 text-12 text-primary hover:border-subtle focus:border-accent-strong focus:outline-none"
                  />
                  <span className="text-10 text-placeholder">
                    {t("{{sections}} sec · {{receipts}} rec", { sections: counts.sections, receipts: counts.receipts })}
                  </span>
                  <Button
                    size="sm"
                    variant="tertiary-danger"
                    onClick={() => handleDelete(label)}
                    disabled={busyId === label.id}
                  >
                    {t("Delete")}
                  </Button>
                </div>
              );
            })
          )}
        </div>
      </div>
    </ModalCore>
  );
}
