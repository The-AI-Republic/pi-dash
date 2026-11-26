// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project repository list (NEWPIDASH-413). The merged `repos` project
// field renders as one row per repository: URL, provider label and base
// branch (or "repo default" when blank). Rows are added, edited and
// removed here; every confirmed change writes the whole list through the
// settings PATCH and the section re-reads it, so the list always matches
// the server after a write. The Repository tab (git_repository_bindings)
// is independent of this list and untouched.
import { Button, Combobox, Input } from "@pidash/kit";
import { ChevronDown, Pencil, Plus, Trash2, X } from "lucide-react";
import * as React from "react";

import {
  inferRepoProvider,
  newRepoId,
  REPO_DEFAULT_BRANCH_LABEL,
  REPO_PROVIDER_LABELS,
  REPO_PROVIDERS,
  validateRepoRows,
  type RepoProvider,
  type RepoRow,
  type RepoRowErrors,
  type ReposServerErrors,
} from "../rules.js";

export interface RepositoriesSectionProps {
  rows: RepoRow[];
  disabled: boolean;
  /** True while a list write is in flight; the forms hold still. */
  saving: boolean;
  /** The last failed write, mapped onto rows and the section. */
  serverErrors: ReposServerErrors;
  /** Helper line under the list; defaults to the settings-tab copy. */
  hint?: string;
  /**
   * Write the whole candidate list; resolves true when the server kept
   * it. The caller maps refusals onto `serverErrors` and re-reads the
   * project, so the form closes only on a kept write.
   */
  onCommit: (rows: RepoRow[]) => Promise<boolean>;
}

interface Draft {
  url: string;
  provider: RepoProvider;
  /** False until the provider select is touched; the URL drives it meanwhile. */
  providerTouched: boolean;
  baseBranch: string;
}

function draftFromRow(row: RepoRow): Draft {
  return { url: row.url, provider: row.provider, providerTouched: true, baseBranch: row.baseBranch };
}

function blankDraft(): Draft {
  return { url: "", provider: "other", providerTouched: false, baseBranch: "" };
}

function draftToRow(id: string, draft: Draft): RepoRow {
  return { id, url: draft.url, provider: draft.provider, baseBranch: draft.baseBranch };
}

/** Provider picker the add and edit forms share. */
function ProviderPicker({
  value,
  disabled,
  testid,
  onSelect,
}: {
  value: RepoProvider;
  disabled: boolean;
  testid: string;
  onSelect: (provider: RepoProvider) => void;
}): React.ReactElement {
  return (
    <Combobox
      trigger={
        <Button variant="secondary" disabled={disabled} data-testid={testid} className="w-full justify-between">
          <span>{REPO_PROVIDER_LABELS[value]}</span>
          <ChevronDown size={16} strokeWidth={1.5} aria-hidden="true" />
        </Button>
      }
      options={REPO_PROVIDERS.map((provider) => ({ id: provider, label: REPO_PROVIDER_LABELS[provider] }))}
      value={value}
      onSelect={(choice) => onSelect(choice as RepoProvider)}
      listLabel="Provider"
      searchEnabled={false}
      emptyText="No providers"
      disabled={disabled}
    />
  );
}

export function RepositoriesSection({
  rows,
  disabled,
  saving,
  serverErrors,
  hint = "Saving writes the whole list at once. Removing the repository the project was created with also clears it from the project.",
  onCommit,
}: RepositoriesSectionProps): React.ReactElement {
  // Which form is open: a fresh add, or the index of the row under edit.
  const [editing, setEditing] = React.useState<number | "new" | null>(null);
  const [draft, setDraft] = React.useState<Draft>(blankDraft);
  const [draftErrors, setDraftErrors] = React.useState<RepoRowErrors>({});
  // A failed write's row index names the sent list; the open form reads
  // its own slot, idle rows read theirs.
  const sentIndex = editing === null ? null : editing === "new" ? rows.length : editing;
  const sentError = sentIndex === null ? undefined : serverErrors.rows.get(sentIndex);

  function openAdd(): void {
    setDraft(blankDraft());
    setDraftErrors({});
    setEditing("new");
  }

  function openEdit(index: number): void {
    const row = rows[index];
    if (row === undefined) return;
    setDraft(draftFromRow(row));
    setDraftErrors({});
    setEditing(index);
  }

  function closeForm(): void {
    setEditing(null);
    setDraftErrors({});
  }

  function patchDraft(patch: Partial<Draft>): void {
    setDraft((current) => {
      const next = { ...current, ...patch };
      if (patch.url !== undefined && !next.providerTouched) {
        next.provider = inferRepoProvider(patch.url);
      }
      return next;
    });
    setDraftErrors({});
  }

  async function confirm(candidate: RepoRow[]): Promise<void> {
    const checked = validateRepoRows(candidate);
    const slot = editing === "new" ? rows.length : (editing ?? -1);
    const errors = checked[slot] ?? {};
    if (Object.keys(errors).length > 0) {
      setDraftErrors(errors);
      return;
    }
    const kept = await onCommit(candidate);
    if (kept) closeForm();
  }

  const formDisabled = disabled || saving;
  return (
    <fieldset className="flex flex-col gap-(--space-2)" aria-label="Repositories" data-testid="general-repos">
      <legend className="text-body font-medium text-(--text)">Repositories</legend>
      {rows.length === 0 && editing !== "new" ? (
        <p className="text-meta text-(--text-muted)" data-testid="general-repos-empty">
          No repositories yet. Add one below.
        </p>
      ) : null}
      {rows.length > 0 ? (
        <ul className="flex flex-col gap-(--space-2)" data-testid="general-repos-list">
          {rows.map((row, index) =>
            editing === index ? (
              <li
                key={row.id}
                data-testid="general-repos-row"
                className="flex flex-col gap-(--space-2) rounded-(--radius-control) border border-(--border) p-(--space-3)"
              >
                <Input
                  id="general-repos-edit-url"
                  label="Repository URL"
                  value={draft.url}
                  disabled={formDisabled}
                  autoComplete="off"
                  data-testid="general-repos-edit-url"
                  {...(draftErrors.url === undefined ? {} : { error: draftErrors.url })}
                  onChange={(event) => patchDraft({ url: event.target.value })}
                />
                <div className="grid grid-cols-1 gap-(--space-2) md:grid-cols-2">
                  <div className="flex flex-col gap-(--space-2)">
                    <span className="text-body font-medium text-(--text)">Provider</span>
                    <ProviderPicker
                      value={draft.provider}
                      disabled={formDisabled}
                      testid="general-repos-edit-provider-trigger"
                      onSelect={(provider) => patchDraft({ provider, providerTouched: true })}
                    />
                    {draftErrors.provider === undefined ? null : (
                      <p className="text-meta text-(--danger)" data-testid="general-repos-edit-provider-error">
                        {draftErrors.provider}
                      </p>
                    )}
                  </div>
                  <Input
                    id="general-repos-edit-branch"
                    label="Base branch"
                    value={draft.baseBranch}
                    disabled={formDisabled}
                    autoComplete="off"
                    placeholder={REPO_DEFAULT_BRANCH_LABEL}
                    data-testid="general-repos-edit-branch"
                    {...(draftErrors.baseBranch === undefined ? {} : { error: draftErrors.baseBranch })}
                    onChange={(event) => patchDraft({ baseBranch: event.target.value })}
                  />
                </div>
                {draftErrors.row === undefined && sentError === undefined ? null : (
                  <p className="text-meta text-(--danger)" data-testid="general-repos-edit-error">
                    {draftErrors.row ?? sentError}
                  </p>
                )}
                <div className="flex justify-end gap-(--space-2)">
                  <Button
                    variant="secondary"
                    disabled={formDisabled}
                    data-testid="general-repos-form-cancel"
                    onClick={closeForm}
                  >
                    <X size={14} strokeWidth={1.5} aria-hidden="true" />
                    Cancel
                  </Button>
                  <Button
                    variant="primary"
                    disabled={formDisabled}
                    loading={saving}
                    data-testid="general-repos-edit-confirm"
                    onClick={() =>
                      void confirm(rows.map((entry, at) => (at === index ? draftToRow(entry.id, draft) : entry)))
                    }
                  >
                    Save
                  </Button>
                </div>
              </li>
            ) : (
              <li
                key={row.id}
                data-testid="general-repos-row"
                className="flex items-center gap-(--space-2) rounded-(--radius-control) border border-(--border) p-(--space-3)"
              >
                <div className="flex min-w-0 flex-1 flex-col gap-(--space-1)">
                  <span className="text-body truncate font-medium text-(--text)" data-testid="general-repos-row-url">
                    {row.url}
                  </span>
                  <span className="text-meta text-(--text-muted)" data-testid="general-repos-row-meta">
                    <span data-testid="general-repos-row-provider">{REPO_PROVIDER_LABELS[row.provider]}</span>
                    {" · "}
                    <span data-testid="general-repos-row-branch">
                      {row.baseBranch === "" ? REPO_DEFAULT_BRANCH_LABEL : row.baseBranch}
                    </span>
                  </span>
                  {serverErrors.rows.get(index) === undefined ? null : (
                    <span className="text-meta text-(--danger)" data-testid="general-repos-row-error">
                      {serverErrors.rows.get(index)}
                    </span>
                  )}
                </div>
                <Button
                  variant="secondary"
                  size="small"
                  disabled={formDisabled || editing !== null}
                  aria-label={`Edit ${row.url}`}
                  data-testid="general-repos-edit"
                  onClick={() => openEdit(index)}
                >
                  <Pencil size={14} strokeWidth={1.5} aria-hidden="true" />
                </Button>
                <Button
                  variant="secondary"
                  size="small"
                  disabled={formDisabled || editing !== null}
                  aria-label={`Remove ${row.url}`}
                  data-testid="general-repos-remove"
                  onClick={() => void onCommit(rows.filter((_, at) => at !== index))}
                >
                  <Trash2 size={14} strokeWidth={1.5} aria-hidden="true" />
                </Button>
              </li>
            )
          )}
        </ul>
      ) : null}
      {editing === "new" ? (
        <div
          data-testid="general-repos-new"
          className="flex flex-col gap-(--space-2) rounded-(--radius-control) border border-(--border) p-(--space-3)"
        >
          <Input
            id="general-repos-new-url"
            label="Repository URL"
            value={draft.url}
            disabled={formDisabled}
            autoComplete="off"
            placeholder="Repository URL, e.g. https://github.com/team/project"
            data-testid="general-repos-new-url"
            {...(draftErrors.url === undefined ? {} : { error: draftErrors.url })}
            onChange={(event) => patchDraft({ url: event.target.value })}
          />
          <div className="grid grid-cols-1 gap-(--space-2) md:grid-cols-2">
            <div className="flex flex-col gap-(--space-2)">
              <span className="text-body font-medium text-(--text)">Provider</span>
              <ProviderPicker
                value={draft.provider}
                disabled={formDisabled}
                testid="general-repos-new-provider-trigger"
                onSelect={(provider) => patchDraft({ provider, providerTouched: true })}
              />
              {draftErrors.provider === undefined ? null : (
                <p className="text-meta text-(--danger)" data-testid="general-repos-new-provider-error">
                  {draftErrors.provider}
                </p>
              )}
            </div>
            <Input
              id="general-repos-new-branch"
              label="Base branch"
              value={draft.baseBranch}
              disabled={formDisabled}
              autoComplete="off"
              placeholder={REPO_DEFAULT_BRANCH_LABEL}
              data-testid="general-repos-new-branch"
              {...(draftErrors.baseBranch === undefined ? {} : { error: draftErrors.baseBranch })}
              onChange={(event) => patchDraft({ baseBranch: event.target.value })}
            />
          </div>
          {draftErrors.row === undefined && sentError === undefined ? null : (
            <p className="text-meta text-(--danger)" data-testid="general-repos-new-error">
              {draftErrors.row ?? sentError}
            </p>
          )}
          <div className="flex justify-end gap-(--space-2)">
            <Button
              variant="secondary"
              disabled={formDisabled}
              data-testid="general-repos-form-cancel"
              onClick={closeForm}
            >
              <X size={14} strokeWidth={1.5} aria-hidden="true" />
              Cancel
            </Button>
            <Button
              variant="primary"
              disabled={formDisabled}
              loading={saving}
              data-testid="general-repos-new-confirm"
              onClick={() => void confirm([...rows, draftToRow(newRepoId(), draft)])}
            >
              Add
            </Button>
          </div>
        </div>
      ) : (
        <div>
          <Button
            variant="secondary"
            disabled={formDisabled || editing !== null}
            data-testid="general-repos-add"
            onClick={openAdd}
          >
            <Plus size={14} strokeWidth={1.5} aria-hidden="true" />
            Add repository
          </Button>
        </div>
      )}
      {serverErrors.general.length === 0 ? null : (
        <div className="flex flex-col gap-(--space-1)" data-testid="general-repos-error">
          {serverErrors.general.map((message) => (
            <p key={message} className="text-meta text-(--danger)">
              {message}
            </p>
          ))}
        </div>
      )}
      <p className="text-meta text-(--text-muted)">{hint}</p>
    </fieldset>
  );
}
