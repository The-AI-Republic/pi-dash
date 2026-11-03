// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Repository row (parity/specs/project-settings-general.md §PSET-008). Its
// own admin-only control: typing edits only the local draft, the normal save
// never carries the URL, and only Bind persists (the bind answer's canonical
// URL is written back). On a matching canonical URL the control reads as a
// disabled Bound pill.

import { Button, Input } from "@pidash/kit";
import { Link2 } from "lucide-react";
import * as React from "react";

import { notifyError } from "../../../../shared/shell/index.js";
import { GENERAL_REPO_URL_MAX_LENGTH, repositoryBoundState, REPO_EMPTY_MESSAGE, REPO_EMPTY_TITLE } from "../rules.js";
import type { ProjectRepositoryStatus } from "@pidash/api-client";

export interface RepositoryRowProps {
  value: string;
  disabled: boolean;
  status: ProjectRepositoryStatus | undefined;
  statusLoading: boolean;
  binding: boolean;
  onChange: (value: string) => void;
  /** Fired with the trimmed URL; the row refuses empty input first. */
  onBind: (repoUrl: string) => void;
}

export function RepositoryRow({
  value,
  disabled,
  status,
  statusLoading,
  binding,
  onChange,
  onBind,
}: RepositoryRowProps): React.ReactElement {
  const boundState = repositoryBoundState(
    status === undefined ? undefined : status.bound ? status : { bound: false },
    value
  );
  const controlDisabled = disabled || statusLoading || binding;
  return (
    <fieldset className="flex flex-col gap-(--space-2)" aria-label="Repository">
      <legend className="text-body font-medium text-(--text)">Repository</legend>
      <div className="flex items-end gap-(--space-2)">
        <Input
          id="repoUrl"
          name="repoUrl"
          label="Repository URL"
          value={value}
          maxLength={GENERAL_REPO_URL_MAX_LENGTH}
          disabled={disabled}
          autoComplete="off"
          className="min-w-0 flex-1"
          onChange={(event) => onChange(event.target.value)}
        />
        {boundState === "bound" ? (
          <Button variant="secondary" disabled data-testid="general-repo-bound">
            <Link2 size={14} strokeWidth={1.5} aria-hidden="true" />
            Bound
          </Button>
        ) : (
          <Button
            variant="secondary"
            disabled={controlDisabled}
            loading={binding}
            data-testid="general-repo-bind"
            onClick={() => {
              const trimmed = value.trim();
              if (trimmed === "") {
                notifyError(REPO_EMPTY_TITLE, REPO_EMPTY_MESSAGE);
                return;
              }
              onBind(trimmed);
            }}
          >
            Bind
          </Button>
        )}
      </div>
      <p className="text-meta text-(--text-muted)">
        Binding is saved on its own and never by the form's save. After binding, enable sync in the repository settings
        to import work.
      </p>
    </fieldset>
  );
}
