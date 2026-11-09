// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runner LLM model catalog page (PDASHOSS01-364): god-mode CRUD for
// the picker source. One section per agent, each a table of models
// with add, edit, delete, reorder, enable/disable and preselect.
// Deleting a model does not affect runners already using it — their
// model lives in their own config — and the page says so.

import { Button, FieldScaffold, Input, Skeleton, Switch } from "@pidash/kit";
import {
  RUNNER_CATALOG_AGENTS,
  RUNNER_CATALOG_EFFORTS,
  type RunnerCatalogAgent,
  type RunnerCatalogRow,
} from "@pidash/api-client";
import * as React from "react";

import { useCreateRunnerModel, useDeleteRunnerModel, useRunnerModelCatalog, useUpdateRunnerModel } from "../hooks.js";

const AGENT_LABELS: Record<RunnerCatalogAgent, string> = {
  "claude-code": "Claude Code",
  codex: "Codex",
  "cursor-agent": "Cursor",
  "open-claw": "OpenClaw",
  grok: "Grok",
  "muse-code": "Muse Code",
};

const PAGE_HEADING = "Runner models";
const PAGE_DESCRIPTION =
  "LLM catalog the add-runner picker reads. Add, reorder, enable, or preselect models per agent without a redeploy.";
const DELETE_NOTE =
  "Deleting a model does not affect runners already using it — their model is stored in their own config.";

const EFFORT_LABEL: Record<string, string> = {
  low: "Low",
  medium: "Medium",
  high: "High",
  xhigh: "Extra High",
};

function effortsText(efforts: string[]): string {
  if (efforts.length === 0) return "—";
  return efforts.join(", ");
}

function AgentSection({
  agent,
  rows,
  onMutate,
}: {
  agent: RunnerCatalogAgent;
  rows: RunnerCatalogRow[];
  onMutate: () => void;
}): React.ReactElement {
  const create = useCreateRunnerModel();
  const update = useUpdateRunnerModel();
  const remove = useDeleteRunnerModel();

  const [showAdd, setShowAdd] = React.useState(false);
  const [editingId, setEditingId] = React.useState<string | null>(null);

  // Add form state
  const [newModel, setNewModel] = React.useState("");
  const [newLabel, setNewLabel] = React.useState("");
  const [newEfforts, setNewEfforts] = React.useState<string[]>([]);
  const [newEnabled, setNewEnabled] = React.useState(true);
  const [newPreselected, setNewPreselected] = React.useState(false);
  const [addError, setAddError] = React.useState<string | null>(null);

  const sorted = React.useMemo(
    () =>
      [...rows].sort((a, b) => a.sort_order - b.sort_order || (a.created_at ?? "").localeCompare(b.created_at ?? "")),
    [rows]
  );

  const nextSortOrder = sorted.length === 0 ? 0 : Math.max(...sorted.map((r) => r.sort_order)) + 1;

  const submitAdd = async () => {
    setAddError(null);
    if (newModel.trim() === "" || newLabel.trim() === "") {
      setAddError("Model and label are required.");
      return;
    }
    try {
      await create.mutateAsync({
        agent,
        model: newModel.trim(),
        label: newLabel.trim(),
        reasoning_efforts: [...newEfforts],
        sort_order: nextSortOrder,
        is_enabled: newEnabled,
        is_preselected: newPreselected,
      });
      setNewModel("");
      setNewLabel("");
      setNewEfforts([]);
      setNewEnabled(true);
      setNewPreselected(false);
      setShowAdd(false);
      onMutate();
    } catch {
      setAddError("Could not add the model. The model may already exist for this agent.");
    }
  };

  const toggleEnabled = async (row: RunnerCatalogRow) => {
    await update.mutateAsync({ id: row.id, patch: { is_enabled: !row.is_enabled } });
    onMutate();
  };

  const setPreselected = async (row: RunnerCatalogRow) => {
    await update.mutateAsync({ id: row.id, patch: { is_preselected: true } });
    onMutate();
  };

  const move = async (index: number, dir: -1 | 1) => {
    const other = index + dir;
    if (other < 0 || other >= sorted.length) return;
    const a = sorted[index]!;
    const b = sorted[other]!;
    // Swap sort_order values so the order flips while keeping the rows stable.
    await Promise.all([
      update.mutateAsync({ id: a.id, patch: { sort_order: b.sort_order } }),
      update.mutateAsync({ id: b.id, patch: { sort_order: a.sort_order } }),
    ]);
    onMutate();
  };

  const handleDelete = async (row: RunnerCatalogRow) => {
    if (
      !globalThis.window.confirm(
        `Delete ${row.label} (${row.model})? Runners already using it keep their stored model.`
      )
    ) {
      return;
    }
    await remove.mutateAsync(row.id);
    onMutate();
  };

  const toggleEffort = (effort: string) => {
    setNewEfforts((current) => (current.includes(effort) ? current.filter((e) => e !== effort) : [...current, effort]));
  };

  return (
    <section data-testid={`runner-models-section-${agent}`} className="flex flex-col gap-(--space-3)">
      <div className="flex items-center justify-between gap-(--space-4)">
        <h2 data-testid={`runner-models-agent-${agent}`} className="text-h3 text-(--text)">
          {AGENT_LABELS[agent]}
        </h2>
        <Button
          variant="secondary"
          onClick={() => setShowAdd((v) => !v)}
          data-testid={`runner-models-add-toggle-${agent}`}
        >
          {showAdd ? "Cancel" : "Add model"}
        </Button>
      </div>

      {showAdd && (
        <div
          data-testid={`runner-models-add-form-${agent}`}
          className="flex flex-col gap-(--space-3) rounded-(--radius-control) border border-(--border) p-(--space-4)"
        >
          <div className="grid grid-cols-1 gap-(--space-3) md:grid-cols-2">
            <Input
              label="Model slug (passed to --model)"
              id={`${agent}-new-model`}
              value={newModel}
              onChange={(e) => setNewModel(e.target.value)}
              placeholder="gpt-5.5"
            />
            <Input
              label="Label shown in the picker"
              id={`${agent}-new-label`}
              value={newLabel}
              onChange={(e) => setNewLabel(e.target.value)}
              placeholder="GPT-5.5"
            />
          </div>
          <div>
            <FieldScaffold
              label="Reasoning efforts (optional; empty when this agent has no effort flag)"
              fieldId={`${agent}-new-efforts`}
            >
              {() => (
                <div className="flex flex-wrap gap-(--space-2)" id={`${agent}-new-efforts`}>
                  {RUNNER_CATALOG_EFFORTS.map((effort) => (
                    <label key={effort} className="flex items-center gap-(--space-2)">
                      <input
                        type="checkbox"
                        checked={newEfforts.includes(effort)}
                        onChange={() => toggleEffort(effort)}
                      />
                      <span className="text-body text-(--text)">{EFFORT_LABEL[effort] ?? effort}</span>
                    </label>
                  ))}
                </div>
              )}
            </FieldScaffold>
          </div>
          <div className="flex flex-wrap items-center gap-(--space-6)">
            <label className="flex items-center gap-(--space-2)">
              <Switch checked={newEnabled} onCheckedChange={setNewEnabled} aria-label="Enabled" />
              <span className="text-body text-(--text)">Enabled</span>
            </label>
            <label className="flex items-center gap-(--space-2)">
              <Switch checked={newPreselected} onCheckedChange={setNewPreselected} aria-label="Preselected" />
              <span className="text-body text-(--text)">Preselected (default)</span>
            </label>
          </div>
          {addError && (
            <p data-testid={`runner-models-add-error-${agent}`} className="text-body text-(--danger)">
              {addError}
            </p>
          )}
          <div className="flex justify-end gap-(--space-3)">
            <Button
              variant="primary"
              onClick={() => void submitAdd()}
              disabled={create.isPending}
              data-testid={`runner-models-add-submit-${agent}`}
            >
              {create.isPending ? "Adding…" : "Add model"}
            </Button>
          </div>
        </div>
      )}

      <table data-testid={`runner-models-table-${agent}`} className="w-full border-collapse text-left">
        <thead>
          <tr>
            {["Label", "Model", "Efforts", "Order", "Enabled", "Preselected", "Actions"].map((h) => (
              <th
                key={h}
                className="text-caption border-b border-(--border) px-(--space-3) py-(--space-2) font-medium text-(--text-muted)"
              >
                {h}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {sorted.map((row, index) => (
            <React.Fragment key={row.id}>
              <tr>
                <td className="text-body border-b border-(--border) px-(--space-3) py-(--space-2) font-medium text-(--text)">
                  {row.label}
                </td>
                <td className="text-body border-b border-(--border) px-(--space-3) py-(--space-2) text-(--text-muted)">
                  {row.model}
                </td>
                <td className="text-body border-b border-(--border) px-(--space-3) py-(--space-2) text-(--text-muted)">
                  {effortsText(row.reasoning_efforts)}
                </td>
                <td className="text-body border-b border-(--border) px-(--space-3) py-(--space-2) text-(--text-muted)">
                  {row.sort_order}
                </td>
                <td className="text-body border-b border-(--border) px-(--space-3) py-(--space-2) text-(--text)">
                  <Switch
                    checked={row.is_enabled}
                    onCheckedChange={() => void toggleEnabled(row)}
                    aria-label={`${row.label} enabled`}
                  />
                </td>
                <td className="text-body border-b border-(--border) px-(--space-3) py-(--space-2) text-(--text)">
                  {row.is_preselected ? (
                    <span className="text-body font-medium text-(--text)">Default</span>
                  ) : (
                    <Button
                      variant="secondary"
                      onClick={() => void setPreselected(row)}
                      data-testid={`runner-models-preselect-${row.id}`}
                    >
                      Set default
                    </Button>
                  )}
                </td>
                <td className="border-b border-(--border) px-(--space-3) py-(--space-2)">
                  <div className="flex flex-wrap gap-(--space-2)">
                    <Button
                      variant="secondary"
                      onClick={() => void move(index, -1)}
                      disabled={index === 0 || update.isPending}
                      data-testid={`runner-models-up-${row.id}`}
                    >
                      ↑
                    </Button>
                    <Button
                      variant="secondary"
                      onClick={() => void move(index, 1)}
                      disabled={index === sorted.length - 1 || update.isPending}
                      data-testid={`runner-models-down-${row.id}`}
                    >
                      ↓
                    </Button>
                    <Button
                      variant="secondary"
                      onClick={() => setEditingId(editingId === row.id ? null : row.id)}
                      data-testid={`runner-models-edit-${row.id}`}
                    >
                      Edit
                    </Button>
                    <Button
                      variant="secondary"
                      onClick={() => void handleDelete(row)}
                      data-testid={`runner-models-delete-${row.id}`}
                    >
                      Delete
                    </Button>
                  </div>
                </td>
              </tr>
              {editingId === row.id && <EditRow row={row} onClose={() => setEditingId(null)} onSaved={onMutate} />}
            </React.Fragment>
          ))}
          {sorted.length === 0 && (
            <tr>
              <td colSpan={7} className="text-body px-(--space-3) py-(--space-4) text-(--text-muted)">
                No models for this agent yet. Add the first one above.
              </td>
            </tr>
          )}
        </tbody>
      </table>
    </section>
  );
}

function EditRow({
  row,
  onClose,
  onSaved,
}: {
  row: RunnerCatalogRow;
  onClose: () => void;
  onSaved: () => void;
}): React.ReactElement {
  const update = useUpdateRunnerModel();
  const [label, setLabel] = React.useState(row.label);
  const [model, setModel] = React.useState(row.model);
  const [efforts, setEfforts] = React.useState<string[]>([...row.reasoning_efforts]);
  const [sortOrder, setSortOrder] = React.useState(String(row.sort_order));
  const [enabled, setEnabled] = React.useState(row.is_enabled);
  const [preselected, setPreselected] = React.useState(row.is_preselected);
  const [error, setError] = React.useState<string | null>(null);

  const toggleEffort = (effort: string) => {
    setEfforts((current) => (current.includes(effort) ? current.filter((e) => e !== effort) : [...current, effort]));
  };

  const save = async () => {
    setError(null);
    if (model.trim() === "" || label.trim() === "") {
      setError("Model and label are required.");
      return;
    }
    const parsedSort = Number.parseInt(sortOrder, 10);
    if (Number.isNaN(parsedSort) || parsedSort < 0) {
      setError("Sort order must be a number ≥ 0.");
      return;
    }
    try {
      await update.mutateAsync({
        id: row.id,
        patch: {
          label: label.trim(),
          model: model.trim(),
          reasoning_efforts: efforts,
          sort_order: parsedSort,
          is_enabled: enabled,
          is_preselected: preselected,
        },
      });
      onSaved();
      onClose();
    } catch {
      setError("Could not save. The model may already exist for this agent.");
    }
  };

  return (
    <tr>
      <td colSpan={7} className="bg-(--subtle) px-(--space-4) py-(--space-4)">
        <div className="flex flex-col gap-(--space-3)">
          <div className="grid grid-cols-1 gap-(--space-3) md:grid-cols-3">
            <Input label="Label" id={`${row.id}-label`} value={label} onChange={(e) => setLabel(e.target.value)} />
            <Input label="Model" id={`${row.id}-model`} value={model} onChange={(e) => setModel(e.target.value)} />
            <Input
              label="Sort order"
              id={`${row.id}-sort`}
              value={sortOrder}
              onChange={(e) => setSortOrder(e.target.value)}
              type="number"
              min={0}
            />
          </div>
          <div className="flex flex-wrap gap-(--space-4)">
            {RUNNER_CATALOG_EFFORTS.map((effort) => (
              <label key={effort} className="flex items-center gap-(--space-2)">
                <input type="checkbox" checked={efforts.includes(effort)} onChange={() => toggleEffort(effort)} />
                <span className="text-body text-(--text)">{EFFORT_LABEL[effort] ?? effort}</span>
              </label>
            ))}
          </div>
          <div className="flex flex-wrap items-center gap-(--space-6)">
            <label className="flex items-center gap-(--space-2)">
              <Switch checked={enabled} onCheckedChange={setEnabled} aria-label="Enabled" />
              <span className="text-body text-(--text)">Enabled</span>
            </label>
            <label className="flex items-center gap-(--space-2)">
              <Switch checked={preselected} onCheckedChange={setPreselected} aria-label="Preselected" />
              <span className="text-body text-(--text)">Preselected</span>
            </label>
          </div>
          {error && (
            <p data-testid={`runner-models-edit-error-${row.id}`} className="text-body text-(--danger)">
              {error}
            </p>
          )}
          <div className="flex justify-end gap-(--space-3)">
            <Button variant="secondary" onClick={onClose}>
              Cancel
            </Button>
            <Button
              variant="primary"
              onClick={() => void save()}
              disabled={update.isPending}
              data-testid={`runner-models-save-${row.id}`}
            >
              {update.isPending ? "Saving…" : "Save"}
            </Button>
          </div>
        </div>
      </td>
    </tr>
  );
}

function ModelsSkeleton(): React.ReactElement {
  return (
    <div
      role="status"
      aria-label="Loading runner models"
      data-testid="runner-models-skeleton"
      className="flex flex-col gap-(--space-4)"
    >
      <Skeleton height="24px" width="60%" />
      <Skeleton height="120px" />
      <Skeleton height="120px" />
    </div>
  );
}

function ModelsError({ onRetry }: { onRetry: () => void }): React.ReactElement {
  return (
    <div data-testid="runner-models-error" className="flex flex-col items-start gap-(--space-3)">
      <p className="text-body text-(--text)">The runner models failed to load.</p>
      <Button variant="primary" onClick={onRetry}>
        Retry
      </Button>
    </div>
  );
}

export function AdminRunnerModelsPage(): React.ReactElement {
  const catalog = useRunnerModelCatalog();
  let body: React.ReactElement;
  if (catalog.isPending) {
    body = <ModelsSkeleton />;
  } else if (catalog.isError) {
    body = (
      <ModelsError
        onRetry={() => {
          void catalog.refetch();
        }}
      />
    );
  } else {
    const rows = catalog.data;
    const byAgent = new Map<RunnerCatalogAgent, RunnerCatalogRow[]>();
    for (const row of rows) {
      const key = row.agent as RunnerCatalogAgent;
      const list = byAgent.get(key) ?? [];
      list.push(row);
      byAgent.set(key, list);
    }
    body = (
      <div className="flex flex-col gap-(--space-8)">
        <p data-testid="runner-models-note" className="text-body text-(--text-muted)">
          {DELETE_NOTE}
        </p>
        {RUNNER_CATALOG_AGENTS.map((agent) => (
          <AgentSection
            key={agent}
            agent={agent}
            rows={byAgent.get(agent) ?? []}
            onMutate={() => void catalog.refetch()}
          />
        ))}
      </div>
    );
  }
  return (
    <div data-testid="runner-models-page" className="w-full p-(--space-6)">
      <div className="mb-(--space-4)">
        <h1 data-testid="runner-models-title" className="text-h2 text-(--text)">
          {PAGE_HEADING}
        </h1>
        <p data-testid="runner-models-description" className="text-body text-(--text-muted)">
          {PAGE_DESCRIPTION}
        </p>
      </div>
      {body}
    </div>
  );
}
