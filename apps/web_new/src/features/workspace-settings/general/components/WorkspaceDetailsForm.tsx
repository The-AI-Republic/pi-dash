// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// General details form (parity/specs/workspace-settings.md §3, WSET-003).
// Prefills name, size and timezone from the stored row; save validates the
// name inline (no write on error), sends exactly the three fields, reads
// "Updating" in flight and toasts on success. A failed save shows nothing
// and keeps the typed values (§3, the oracle pins it). The address field
// echoes the same host-and-slug text as the copy control and is read-only.

import { Button, Combobox, Input } from "@pidash/kit";
import { ChevronDown } from "lucide-react";
import * as React from "react";

import { detailsUpdateInput, useSaveWorkspaceDetails } from "../api/mutations.js";
import { useTimezoneList } from "../api/timezones.js";
import {
  mergeTimezoneOptions,
  ORGANIZATION_SIZE_BANDS,
  ORGANIZATION_SIZE_PLACEHOLDER,
  timezoneChoiceLabel,
  validateWorkspaceDetailsName,
  workspaceAddress,
} from "../rules.js";

/** Save control copy at rest and while the write is in flight (§3). */
export const DETAILS_SAVE_LABEL = "Update workspace";
export const DETAILS_SAVING_LABEL = "Updating";

export interface StoredDetails {
  name: string;
  organizationSize: string | null;
  timezone: string;
}

export interface WorkspaceDetailsFormProps {
  workspaceSlug: string;
  canManage: boolean;
  stored: StoredDetails;
}

interface FormState {
  name: string;
  organizationSize: string;
  timezone: string;
}

function formFromStored(stored: StoredDetails): FormState {
  return {
    name: stored.name,
    organizationSize: stored.organizationSize ?? "",
    timezone: stored.timezone,
  };
}

export function WorkspaceDetailsForm({
  workspaceSlug,
  canManage,
  stored,
}: WorkspaceDetailsFormProps): React.ReactElement {
  const [form, setForm] = React.useState<FormState>(() => formFromStored(stored));
  const [error, setError] = React.useState<string | null>(null);
  const save = useSaveWorkspaceDetails(workspaceSlug);
  const timezones = useTimezoneList();
  const choices = React.useMemo(() => mergeTimezoneOptions(timezones.data?.timezones ?? []), [timezones.data]);
  const address = workspaceAddress(window.location.origin, workspaceSlug);
  const sizeValue = form.organizationSize === "" ? null : form.organizationSize;

  // A stored row arriving after the first render (or replaced by a save)
  // wins over the draft. A save in flight never resets the form.
  const storedKey = `${stored.name}\u0000${stored.organizationSize ?? ""}\u0000${stored.timezone}`;
  const lastStored = React.useRef(storedKey);
  React.useEffect(() => {
    if (lastStored.current === storedKey) return;
    lastStored.current = storedKey;
    setForm(formFromStored(stored));
  }, [storedKey, stored]);

  const submit = (event: React.FormEvent<HTMLFormElement>): void => {
    event.preventDefault();
    if (!canManage) return;
    const nameError = validateWorkspaceDetailsName(form.name);
    setError(nameError);
    if (nameError !== null) return;
    save.mutate(detailsUpdateInput(form));
  };

  return (
    <form
      data-testid="workspace-general-form"
      onSubmit={submit}
      className={canManage ? "flex flex-col gap-(--space-6)" : "flex flex-col gap-(--space-6) opacity-60"}
    >
      <div className="grid grid-cols-1 gap-(--space-6) lg:grid-cols-2">
        <Input
          id="name"
          name="name"
          label="Workspace name"
          value={form.name}
          disabled={!canManage}
          autoComplete="off"
          {...(error === null ? {} : { error })}
          onChange={(event) => {
            setForm((current) => ({ ...current, name: event.target.value }));
          }}
        />
        <div className="flex flex-col gap-(--space-2)">
          <span className="text-body font-medium text-(--text)">Company size</span>
          <Combobox
            trigger={
              <Button variant="secondary" disabled={!canManage} className="w-full justify-between">
                <span>{sizeValue ?? ORGANIZATION_SIZE_PLACEHOLDER}</span>
                <ChevronDown size={16} strokeWidth={1.5} aria-hidden="true" />
              </Button>
            }
            options={ORGANIZATION_SIZE_BANDS.map((band) => ({ id: band, label: band }))}
            value={sizeValue}
            onSelect={(band) => {
              setForm((current) => ({ ...current, organizationSize: band }));
            }}
            listLabel="Organization size"
            searchEnabled={false}
            emptyText="No sizes"
            disabled={!canManage}
          />
        </div>
        <Input
          id="url"
          name="url"
          type="url"
          label="Workspace URL"
          value={address}
          readOnly
          disabled
          className="cursor-not-allowed"
        />
        <div className="flex flex-col gap-(--space-2)">
          <h4 className="text-body font-medium text-(--text)">Workspace Timezone</h4>
          <Combobox
            trigger={
              <Button variant="secondary" disabled={!canManage || timezones.isError} className="w-full justify-between">
                <span className="truncate">
                  {timezoneChoiceLabel(choices, form.timezone === "" ? null : form.timezone)}
                </span>
                <ChevronDown size={16} strokeWidth={1.5} aria-hidden="true" />
              </Button>
            }
            options={choices.map((choice) => ({ id: choice.id, label: choice.label, searchText: choice.searchText }))}
            value={form.timezone === "" ? null : form.timezone}
            onSelect={(zone) => {
              setForm((current) => ({ ...current, timezone: zone }));
            }}
            listLabel="Workspace timezone"
            searchLabel="Search timezones"
            emptyText="No timezones match"
            loading={timezones.isPending}
            loadingText="Loading timezones"
            {...(timezones.isError ? { errorText: "Timezones could not be loaded." } : {})}
            disabled={!canManage}
            className="w-80"
          />
        </div>
      </div>
      {canManage ? (
        <div>
          <Button type="submit" variant="primary" loading={save.isPending} disabled={save.isPending}>
            {save.isPending ? DETAILS_SAVING_LABEL : DETAILS_SAVE_LABEL}
          </Button>
        </div>
      ) : null}
    </form>
  );
}
