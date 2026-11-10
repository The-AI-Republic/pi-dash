// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runner model catalog page (NEWPIDASH-169, DIFF-9): one section per
// agent, each a table of models. Admins add, edit, delete, reorder,
// enable/disable, and preselect models; the add-runner picker reads
// the catalog this page manages (the runner itself accepts any slug,
// so deleting a model never breaks a runner already using it).
//
// Page contract (for the scenario author): the root is
// `runner-models-page` with `runner-models-title` /
// `runner-models-description` and `runner-models-note`. Each agent
// section is `runner-models-section-<agent>` with a table
// `runner-models-table-<agent>`; rows are `runner-models-row-<id>`.
// Add/edit open `runner-models-dialog` with `runner-models-model`,
// `runner-models-label` and `runner-models-effort-<effort>` fields
// plus `runner-models-save` / `runner-models-cancel`. Other controls
// are `runner-models-toggle-<id>`, `runner-models-preselect-<id>`,
// `runner-models-up-<id>` / `runner-models-down-<id>`,
// `runner-models-edit-<id>`, `runner-models-delete-<id>` (two-step
// confirm) and `runner-models-add-<agent>`. Loading shows
// `runner-models-skeleton`, a failed read `runner-models-error` with
// Retry, and form failures inline in `runner-models-form-error`.
import { Button, Dialog, Input, Skeleton, Switch } from "@pidash/kit";
import type { RunnerModelRow } from "@pidash/api-client";
import * as React from "react";

import { useCreateRunnerModel, useDeleteRunnerModel, useRunnerModels, useUpdateRunnerModel } from "../hooks.js";
import {
  RUNNER_MODEL_AGENTS,
  RUNNER_MODEL_EFFORTS,
  RUNNER_MODEL_EFFORT_LABELS,
  RUNNER_MODELS_DELETE_NOTE,
  RUNNER_MODELS_DESCRIPTION,
  RUNNER_MODELS_EMPTY_TEXT,
  RUNNER_MODELS_HEADERS,
  RUNNER_MODELS_HEADING,
  formatEfforts,
  formErrorMessage,
  nextSortOrder,
  rowsForAgent,
  swapSortOrders,
} from "../runnerModels.js";

function ModelsSkeleton(): React.ReactElement {
  return (
    <div role="status" aria-label="Loading runner models" data-testid="runner-models-skeleton">
      {Array.from({ length: 3 }).map((_, index) => (
        <Skeleton key={index} height="96px" />
      ))}
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

interface FormValues {
  model: string;
  label: string;
  efforts: string[];
  is_preselected: boolean;
}

function effortCheckboxId(effort: string): string {
  return `runner-models-effort-${effort}`;
}

function ModelDialog({
  title,
  initial,
  pending,
  error,
  onSave,
  onClose,
}: {
  title: string;
  initial: FormValues;
  pending: boolean;
  error: string | null;
  onSave: (values: FormValues) => void;
  onClose: () => void;
}): React.ReactElement {
  const [model, setModel] = React.useState(initial.model);
  const [label, setLabel] = React.useState(initial.label);
  const [efforts, setEfforts] = React.useState(initial.efforts);
  const [preselected, setPreselected] = React.useState(initial.is_preselected);

  function toggleEffort(effort: string, checked: boolean): void {
    setEfforts((current) => (checked ? [...current, effort] : current.filter((value) => value !== effort)));
  }

  return (
    <Dialog
      open
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
      title={title}
    >
      <div data-testid="runner-models-dialog" className="flex flex-col gap-(--space-4)">
        <Input
          id="runner-models-model-input"
          label="Model"
          data-testid="runner-models-model"
          value={model}
          onChange={(event) => setModel(event.target.value)}
          placeholder="gpt-5.5"
        />
        <Input
          id="runner-models-label-input"
          label="Label"
          data-testid="runner-models-label"
          value={label}
          onChange={(event) => setLabel(event.target.value)}
          placeholder="GPT-5.5"
        />
        <fieldset className="flex flex-col gap-(--space-2)">
          <legend className="text-body font-medium text-(--text)">Reasoning efforts</legend>
          {RUNNER_MODEL_EFFORTS.map((effort) => (
            <label
              key={effort}
              htmlFor={effortCheckboxId(effort)}
              className="text-body flex items-center gap-(--space-2) text-(--text)"
            >
              <input
                id={effortCheckboxId(effort)}
                type="checkbox"
                data-testid={effortCheckboxId(effort)}
                checked={efforts.includes(effort)}
                onChange={(event) => toggleEffort(effort, event.target.checked)}
              />
              {RUNNER_MODEL_EFFORT_LABELS[effort] ?? effort}
            </label>
          ))}
        </fieldset>
        <label className="text-body flex items-center gap-(--space-2) text-(--text)">
          <input
            type="checkbox"
            data-testid="runner-models-preselected"
            checked={preselected}
            onChange={(event) => setPreselected(event.target.checked)}
          />
          Preselect for this agent
        </label>
        {error !== null ? (
          <p data-testid="runner-models-form-error" role="alert" className="text-body text-(--danger)">
            {error}
          </p>
        ) : null}
        <div className="flex items-center gap-(--space-3)">
          <Button
            variant="primary"
            data-testid="runner-models-save"
            disabled={pending}
            onClick={() => {
              onSave({
                model: model.trim(),
                label: label.trim(),
                efforts,
                is_preselected: preselected,
              });
            }}
          >
            Save
          </Button>
          <Button variant="secondary" data-testid="runner-models-cancel" onClick={onClose}>
            Cancel
          </Button>
        </div>
      </div>
    </Dialog>
  );
}

type DialogState = { kind: "add"; agent: string } | { kind: "edit"; agent: string; row: RunnerModelRow } | null;

function AgentSection({
  agent,
  label,
  rows,
  onAdd,
  onEdit,
  onDelete,
  onToggle,
  onPreselect,
  onMove,
}: {
  agent: string;
  label: string;
  rows: RunnerModelRow[];
  onAdd: (agent: string) => void;
  onEdit: (agent: string, row: RunnerModelRow) => void;
  onDelete: (row: RunnerModelRow) => void;
  onToggle: (row: RunnerModelRow, enabled: boolean) => void;
  onPreselect: (row: RunnerModelRow) => void;
  onMove: (rows: RunnerModelRow[], index: number, direction: "up" | "down") => void;
}): React.ReactElement {
  const [confirming, setConfirming] = React.useState<string | null>(null);
  return (
    <section data-testid={`runner-models-section-${agent}`} className="flex flex-col gap-(--space-2)">
      <div className="flex items-center justify-between">
        <h2 data-testid={`runner-models-heading-${agent}`} className="text-h3 text-(--text)">
          {label}
        </h2>
        <Button variant="secondary" data-testid={`runner-models-add-${agent}`} onClick={() => onAdd(agent)}>
          Add model
        </Button>
      </div>
      <table data-testid={`runner-models-table-${agent}`} className="w-full border-collapse text-left">
        <thead>
          <tr>
            {RUNNER_MODELS_HEADERS.map((header) => (
              <th
                key={header}
                scope="col"
                className="text-meta border-b border-(--border) px-(--space-4) py-(--space-2) font-medium text-(--text-muted)"
              >
                {header}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {rows.map((entry, index) => (
            <tr key={entry.id} data-testid={`runner-models-row-${entry.id}`}>
              <td className="text-body border-b border-(--border) px-(--space-4) py-(--space-3) font-medium text-(--text)">
                {entry.model}
              </td>
              <td className="text-body border-b border-(--border) px-(--space-4) py-(--space-3) text-(--text)">
                {entry.label}
              </td>
              <td className="text-body border-b border-(--border) px-(--space-4) py-(--space-3) text-(--text)">
                {formatEfforts(entry)}
              </td>
              <td className="text-body border-b border-(--border) px-(--space-4) py-(--space-3) text-(--text)">
                <Switch
                  checked={entry.is_enabled}
                  data-testid={`runner-models-toggle-${entry.id}`}
                  onCheckedChange={(next) => onToggle(entry, next)}
                />
              </td>
              <td className="text-body border-b border-(--border) px-(--space-4) py-(--space-3) text-(--text)">
                {entry.is_preselected ? (
                  "Yes"
                ) : (
                  <Button
                    variant="secondary"
                    data-testid={`runner-models-preselect-${entry.id}`}
                    onClick={() => onPreselect(entry)}
                  >
                    Mark
                  </Button>
                )}
              </td>
              <td className="text-body border-b border-(--border) px-(--space-4) py-(--space-3) text-(--text)">
                <div className="flex items-center gap-(--space-2)">
                  <Button
                    variant="secondary"
                    data-testid={`runner-models-up-${entry.id}`}
                    disabled={index === 0}
                    onClick={() => onMove(rows, index, "up")}
                  >
                    Up
                  </Button>
                  <Button
                    variant="secondary"
                    data-testid={`runner-models-down-${entry.id}`}
                    disabled={index === rows.length - 1}
                    onClick={() => onMove(rows, index, "down")}
                  >
                    Down
                  </Button>
                </div>
              </td>
              <td className="text-body border-b border-(--border) px-(--space-4) py-(--space-3) text-(--text)">
                <div className="flex items-center gap-(--space-2)">
                  <Button
                    variant="secondary"
                    data-testid={`runner-models-edit-${entry.id}`}
                    onClick={() => onEdit(agent, entry)}
                  >
                    Edit
                  </Button>
                  {confirming === entry.id ? (
                    <>
                      <Button
                        variant="primary"
                        data-testid={`runner-models-confirm-${entry.id}`}
                        onClick={() => {
                          setConfirming(null);
                          onDelete(entry);
                        }}
                      >
                        Confirm delete
                      </Button>
                      <Button
                        variant="secondary"
                        data-testid={`runner-models-cancel-delete-${entry.id}`}
                        onClick={() => setConfirming(null)}
                      >
                        Cancel
                      </Button>
                    </>
                  ) : (
                    <Button
                      variant="secondary"
                      data-testid={`runner-models-delete-${entry.id}`}
                      onClick={() => setConfirming(entry.id)}
                    >
                      Delete
                    </Button>
                  )}
                </div>
              </td>
            </tr>
          ))}
          {rows.length === 0 ? (
            <tr>
              <td
                colSpan={RUNNER_MODELS_HEADERS.length}
                data-testid={`runner-models-empty-${agent}`}
                className="text-body px-(--space-4) py-(--space-4) text-center text-(--text-muted)"
              >
                {RUNNER_MODELS_EMPTY_TEXT}
              </td>
            </tr>
          ) : null}
        </tbody>
      </table>
    </section>
  );
}

export function AdminRunnerModelsPage(): React.ReactElement {
  const catalog = useRunnerModels();
  const createModel = useCreateRunnerModel();
  const updateModel = useUpdateRunnerModel();
  const deleteModel = useDeleteRunnerModel();
  const [dialog, setDialog] = React.useState<DialogState>(null);
  const [formError, setFormError] = React.useState<string | null>(null);
  const [pending, setPending] = React.useState(false);

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
    body = (
      <div className="flex flex-col gap-(--space-8)">
        {RUNNER_MODEL_AGENTS.map(({ agent, label }) => (
          <AgentSection
            key={agent}
            agent={agent}
            label={label}
            rows={rowsForAgent(catalog.data.agents[agent] ?? [], agent)}
            onAdd={(next) => {
              setFormError(null);
              setDialog({ kind: "add", agent: next });
            }}
            onEdit={(next, target) => {
              setFormError(null);
              setDialog({ kind: "edit", agent: next, row: target });
            }}
            onDelete={(target) => {
              void deleteModel.mutateAsync(target.id);
            }}
            onToggle={(target, enabled) => {
              void updateModel.mutateAsync({ id: target.id, patch: { is_enabled: enabled } });
            }}
            onPreselect={(target) => {
              void updateModel.mutateAsync({ id: target.id, patch: { is_preselected: true } });
            }}
            onMove={(rows, index, direction) => {
              const swap = swapSortOrders(rows, index, direction);
              if (swap === null) return;
              const current = rows[index];
              const neighbour = rows[direction === "up" ? index - 1 : index + 1];
              if (current === undefined || neighbour === undefined) return;
              void (async () => {
                await updateModel.mutateAsync({
                  id: current.id,
                  patch: { sort_order: neighbour.sort_order },
                });
                await updateModel.mutateAsync({
                  id: neighbour.id,
                  patch: { sort_order: current.sort_order },
                });
              })();
            }}
          />
        ))}
      </div>
    );
  }

  async function handleSave(values: FormValues): Promise<void> {
    if (dialog === null) return;
    setPending(true);
    setFormError(null);
    try {
      if (dialog.kind === "add") {
        const peerRows = rowsForAgent(catalog.isSuccess ? (catalog.data.agents[dialog.agent] ?? []) : [], dialog.agent);
        await createModel.mutateAsync({
          agent: dialog.agent,
          model: values.model,
          label: values.label,
          reasoning_efforts: values.efforts,
          sort_order: nextSortOrder(peerRows),
          is_preselected: values.is_preselected,
        });
      } else {
        await updateModel.mutateAsync({
          id: dialog.row.id,
          patch: {
            model: values.model,
            label: values.label,
            reasoning_efforts: values.efforts,
            is_preselected: values.is_preselected,
          },
        });
      }
      setDialog(null);
    } catch (error) {
      setFormError(formErrorMessage(error));
    } finally {
      setPending(false);
    }
  }

  return (
    <div data-testid="runner-models-page" className="w-full p-(--space-6)">
      <div className="mb-(--space-4)">
        <h1 data-testid="runner-models-title" className="text-h2 text-(--text)">
          {RUNNER_MODELS_HEADING}
        </h1>
        <p data-testid="runner-models-description" className="text-body text-(--text-muted)">
          {RUNNER_MODELS_DESCRIPTION}
        </p>
        <p data-testid="runner-models-note" className="text-body text-(--text-muted)">
          {RUNNER_MODELS_DELETE_NOTE}
        </p>
      </div>
      {body}
      {dialog !== null ? (
        <ModelDialog
          title={dialog.kind === "add" ? "Add model" : "Edit model"}
          initial={
            dialog.kind === "add"
              ? { model: "", label: "", efforts: [], is_preselected: false }
              : {
                  model: dialog.row.model,
                  label: dialog.row.label,
                  efforts: [...dialog.row.reasoning_efforts],
                  is_preselected: dialog.row.is_preselected,
                }
          }
          pending={pending}
          error={formError}
          onSave={(values) => void handleSave(values)}
          onClose={() => setDialog(null)}
        />
      ) : null}
    </div>
  );
}
