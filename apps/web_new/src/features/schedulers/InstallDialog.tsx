// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project-side "New Scheduler": install an eligible catalog definition
// (parity/specs/agents.md §2, AGT-008) or author a definition and install
// it in one submit (AGT-009). Both paths share one schedule. The create
// path exists only for workspace admins, opens by itself when nothing is
// installable, maps handle conflicts onto the handle field, and when the
// definition saves but its install fails it flips to the install path with
// the fresh entry picked so nothing is left orphaned in the catalog.

import { createScheduler, isApiError, type Scheduler } from "@pidash/api-client";
import { Button, Dialog, Input, Textarea } from "@pidash/kit";
import * as React from "react";

import { getAppClient } from "../../core/api/client.js";
import { notifyError, notifySuccess } from "../../shared/shell/index.js";
import { useSchedulers } from "./api.js";
import { DEFAULT_SCHEDULER_COLOR, deriveSchedulerHandle, validateSchedulerDefinition } from "./definition.js";
import { eligibleDefinitions } from "./installResults.js";
import { useInstallSchedulerBinding } from "./installData.js";
import type { SchedulePayload } from "./schedule.js";
import { emptySchedule } from "./schedule.js";
import { ScheduleFields, useScheduleDraft } from "./ScheduleFields.js";

export type InstallMode = "install" | "create" | "dead-end";

export interface InstallDialogProps {
  workspaceSlug: string;
  projectId: string;
  open: boolean;
  onClose: () => void;
  /** Project role: only project admins may submit. */
  canInstall: boolean;
  /** Workspace role: only workspace admins may author definitions. */
  canAuthor: boolean;
  /** Definitions already installed on the project; they are not offered. */
  installedSchedulerIds: readonly string[];
  /** Runs after a successful install so the caller can settle its list. */
  onInstalled: () => void;
}

interface DefinitionDraft {
  name: string;
  handle: string;
  description: string;
  prompt: string;
  color: string;
}

type DefinitionErrors = Partial<Record<keyof DefinitionDraft, string>>;

function emptyDefinition(): DefinitionDraft {
  return { name: "", handle: "", description: "", prompt: "", color: DEFAULT_SCHEDULER_COLOR };
}

/** Which path a freshly opened dialog starts on. */
export function initialInstallMode(canAuthor: boolean, eligibleCount: number): InstallMode {
  if (eligibleCount > 0) return "install";
  return canAuthor ? "create" : "dead-end";
}

/** Field-level text for a definition create refusal, if the server named one. */
export function definitionFieldError(error: unknown, field: string): string | undefined {
  if (!isApiError(error)) return undefined;
  const messages = error.fields?.[field];
  return messages === undefined || messages.length === 0 ? undefined : messages.join(" ");
}

/** Server detail worth showing for a refused install. */
export function failureMessage(error: unknown): string {
  if (!isApiError(error)) return "Check your connection and try again.";
  const fields = error.fields ?? {};
  for (const field of ["rrule", "dtstart", "tzid", "scheduler", "pod", "project"]) {
    const messages = fields[field];
    if (messages !== undefined && messages.length > 0) return messages.join(" ");
  }
  return error.message.trim() === "" ? "The install was refused." : error.message;
}

/** Draft validation for the author path, keyed by the fields as rendered. */
export function validateDefinitionDraft(draft: DefinitionDraft): DefinitionErrors {
  const errors = validateSchedulerDefinition({
    name: draft.name.trim(),
    slug: draft.handle,
    description: draft.description,
    prompt: draft.prompt,
    color: draft.color,
    isEnabled: true,
  });
  const mapped: DefinitionErrors = {};
  if (errors.name !== undefined) mapped.name = errors.name;
  if (errors.slug !== undefined) mapped.handle = errors.slug;
  if (errors.description !== undefined) mapped.description = errors.description;
  if (errors.prompt !== undefined) mapped.prompt = errors.prompt;
  if (errors.color !== undefined) mapped.color = errors.color;
  return mapped;
}

interface InstallFormProps {
  workspaceSlug: string;
  projectId: string;
  canInstall: boolean;
  canAuthor: boolean;
  installable: readonly Scheduler[];
  onClose: () => void;
  onInstalled: () => void;
  refreshDefinitions: () => Promise<unknown>;
}

export function InstallForm({
  workspaceSlug,
  projectId,
  canInstall,
  canAuthor,
  installable,
  onClose,
  onInstalled,
  refreshDefinitions,
}: InstallFormProps): React.ReactElement {
  const install = useInstallSchedulerBinding(workspaceSlug);
  const [mode, setMode] = React.useState<InstallMode>(() => initialInstallMode(canAuthor, installable.length));
  const [definitionId, setDefinitionId] = React.useState<string>(installable[0]?.id ?? "");
  const [flipped, setFlipped] = React.useState<Scheduler | undefined>();
  const [definition, setDefinition] = React.useState<DefinitionDraft>(emptyDefinition);
  const [handleTouched, setHandleTouched] = React.useState(false);
  const [definitionErrors, setDefinitionErrors] = React.useState<DefinitionErrors>({});
  const [definitionIdError, setDefinitionIdError] = React.useState<string | undefined>();
  const schedule = useScheduleDraft(emptySchedule());
  const pending = install.isPending;
  const options = React.useMemo(
    () =>
      flipped === undefined || installable.some((scheduler) => scheduler.id === flipped.id)
        ? installable
        : [flipped, ...installable],
    [flipped, installable]
  );
  const installEmpty = options.length === 0;

  React.useEffect(() => {
    // A definition created here joins the catalog; offer it when the
    // dialog is on the install path with nothing chosen yet.
    if (mode === "install" && definitionId === "" && options.length > 0) {
      setDefinitionId(options[0]?.id ?? "");
    }
  }, [definitionId, options, mode]);

  const switchMode = (next: "install" | "create"): void => {
    setDefinitionIdError(undefined);
    setMode(next);
  };

  const changeDefinition = <K extends keyof DefinitionDraft>(field: K, value: DefinitionDraft[K]): void => {
    setDefinition((current) => ({ ...current, [field]: value }));
    setDefinitionErrors((current) => ({ ...current, [field]: undefined }));
  };

  async function installExisting(schedulerId: string, payload: SchedulePayload): Promise<void> {
    try {
      await install.mutateAsync({
        projectId,
        input: { scheduler: schedulerId, ...payload, outcome_mode: "create_issue" },
      });
      notifySuccess("Scheduler installed", "It will fire on the configured schedule.");
      onInstalled();
      onClose();
    } catch (error) {
      notifyError("Something went wrong", failureMessage(error));
    }
  }

  async function authorAndInstall(payload: SchedulePayload): Promise<void> {
    let created: Scheduler;
    try {
      created = await createScheduler(getAppClient(), workspaceSlug, {
        name: definition.name.trim(),
        slug: definition.handle,
        description: definition.description,
        prompt: definition.prompt,
        color: definition.color,
        is_enabled: true,
      });
    } catch (error) {
      const handleMessage = definitionFieldError(error, "slug");
      if (handleMessage !== undefined) {
        setDefinitionErrors((current) => ({ ...current, handle: handleMessage }));
        return;
      }
      notifyError("Something went wrong", failureMessage(error));
      return;
    }
    await refreshDefinitions();
    try {
      await install.mutateAsync({
        projectId,
        input: { scheduler: created.id, ...payload, outcome_mode: "create_issue" },
      });
      notifySuccess("Scheduler created and installed", "It will fire on the configured schedule.");
      onInstalled();
      onClose();
    } catch {
      notifyError(
        "Scheduler created but not installed",
        "Its definition is in the workspace catalog. Install it from the Install existing path."
      );
      setFlipped(created);
      setMode("install");
      setDefinitionId(created.id);
    }
  }

  async function submit(): Promise<void> {
    if (!canInstall || pending) return;
    if (mode === "create") {
      const errors = validateDefinitionDraft(definition);
      setDefinitionErrors(errors);
      if (Object.keys(errors).length > 0) return;
    } else if (definitionId === "") {
      setDefinitionIdError("Pick a scheduler.");
      return;
    }
    if (!schedule.validate()) return;
    const payload = schedule.payload();
    if (mode === "create") {
      await authorAndInstall(payload);
      return;
    }
    await installExisting(definitionId, payload);
  }

  if (mode === "dead-end") {
    return (
      <div className="flex flex-col gap-(--space-4)">
        <p className="text-h3 text-(--text)">No schedulers available</p>
        <p className="text-body text-(--text-muted)">
          Every workspace scheduler is either installed here already or disabled. Ask a workspace admin to add one to
          the catalog.
        </p>
        <div className="flex justify-end">
          <Button variant="secondary" onClick={onClose}>
            Cancel
          </Button>
        </div>
      </div>
    );
  }

  const modeTabs: Array<{ key: "install" | "create"; label: string }> = canAuthor
    ? [
        { key: "install", label: "Install existing" },
        { key: "create", label: "Create new" },
      ]
    : [];

  return (
    <form
      aria-label="New Scheduler"
      className="flex flex-col gap-(--space-6)"
      onSubmit={(event) => {
        event.preventDefault();
        void submit();
      }}
    >
      {modeTabs.length > 0 ? (
        <div role="tablist" aria-label="Install or create" className="flex w-fit items-center gap-(--space-1)">
          {modeTabs.map((tab) => (
            <button
              key={tab.key}
              type="button"
              role="tab"
              aria-selected={mode === tab.key}
              className={
                mode === tab.key
                  ? "text-body rounded-(--radius-control) border border-(--border) bg-(--subtle) px-(--space-3) py-(--space-1) font-medium text-(--text)"
                  : "text-body rounded-(--radius-control) border border-transparent px-(--space-3) py-(--space-1) text-(--text-muted) hover:text-(--text)"
              }
              onClick={() => switchMode(tab.key)}
            >
              {tab.label}
            </button>
          ))}
        </div>
      ) : null}

      {mode === "install" ? (
        installEmpty ? (
          <p className="text-body text-(--text-muted)">
            Every enabled workspace definition is installed here already. Create a new one to schedule more work.
          </p>
        ) : (
          <div className="flex flex-col gap-(--space-2)">
            <label htmlFor="project-install-definition" className="text-body font-medium text-(--text)">
              Scheduler
            </label>
            <select
              id="project-install-definition"
              value={definitionId}
              disabled={pending}
              aria-invalid={definitionIdError === undefined ? undefined : true}
              className="text-body h-(--control-height) w-full rounded-(--radius-control) border border-(--border) bg-(--bg) px-(--space-4) text-(--text) disabled:cursor-not-allowed disabled:opacity-50"
              onChange={(event) => {
                setDefinitionId(event.target.value);
                setDefinitionIdError(undefined);
              }}
            >
              {options.map((scheduler) => (
                <option key={scheduler.id} value={scheduler.id}>
                  {scheduler.name} ({scheduler.slug})
                </option>
              ))}
            </select>
            {definitionIdError === undefined ? (
              <p className="text-meta text-(--text-muted)">
                Enabled definitions this project has not installed yet.{" "}
                {canAuthor ? null : "Ask a workspace admin to add more to the catalog."}
              </p>
            ) : (
              <p role="alert" className="text-meta text-(--danger)">
                {definitionIdError}
              </p>
            )}
          </div>
        )
      ) : (
        <div className="flex flex-col gap-(--space-4)">
          <Input
            label="Name"
            value={definition.name}
            disabled={pending}
            autoFocus
            {...(definitionErrors.name === undefined ? {} : { error: definitionErrors.name })}
            onChange={(event) => {
              const name = event.target.value;
              setDefinition((current) => ({
                ...current,
                name,
                handle: handleTouched ? current.handle : deriveSchedulerHandle(name),
              }));
              setDefinitionErrors((current) => {
                const next: DefinitionErrors = { ...current };
                delete next.name;
                delete next.handle;
                return next;
              });
            }}
          />
          <Input
            label="Slug"
            value={definition.handle}
            disabled={pending}
            autoComplete="off"
            hint="Used in scheduler links. Lowercase letters, numbers and single dashes."
            {...(definitionErrors.handle === undefined ? {} : { error: definitionErrors.handle })}
            onChange={(event) => {
              setHandleTouched(true);
              changeDefinition("handle", event.target.value.toLowerCase());
            }}
          />
          <Textarea
            label="Description"
            value={definition.description}
            rows={2}
            disabled={pending}
            {...(definitionErrors.description === undefined ? {} : { error: definitionErrors.description })}
            onChange={(event) => changeDefinition("description", event.target.value)}
          />
          <Textarea
            label="Prompt"
            value={definition.prompt}
            rows={5}
            disabled={pending}
            {...(definitionErrors.prompt === undefined ? {} : { error: definitionErrors.prompt })}
            onChange={(event) => changeDefinition("prompt", event.target.value)}
          />
          <Input
            label="Color"
            type="color"
            value={definition.color}
            disabled={pending}
            {...(definitionErrors.color === undefined ? {} : { error: definitionErrors.color })}
            onChange={(event) => changeDefinition("color", event.target.value)}
          />
        </div>
      )}

      <ScheduleFields
        idPrefix="project-install"
        value={schedule.value}
        errors={schedule.errors}
        disabled={pending}
        onChange={schedule.change}
      />

      <div className="flex justify-end gap-(--space-3)">
        <Button variant="ghost" disabled={pending} onClick={onClose}>
          Cancel
        </Button>
        <Button
          type="submit"
          variant="primary"
          loading={pending}
          disabled={pending || !canInstall || (mode === "install" && installEmpty)}
        >
          {mode === "create"
            ? pending
              ? "Creating & installing…"
              : "Create & install"
            : pending
              ? "Installing…"
              : "Install"}
        </Button>
      </div>
    </form>
  );
}

export function InstallDialog({
  workspaceSlug,
  projectId,
  open,
  onClose,
  canInstall,
  canAuthor,
  installedSchedulerIds,
  onInstalled,
}: InstallDialogProps): React.ReactElement {
  const schedulers = useSchedulers(workspaceSlug, { enabled: open });
  const installable = React.useMemo(
    () => eligibleDefinitions(schedulers.data ?? [], installedSchedulerIds),
    [schedulers.data, installedSchedulerIds]
  );
  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next) onClose();
      }}
      title="New Scheduler"
      description="Install a catalog definition on this project, or write a new one and install it right away."
      className="max-w-3xl"
    >
      {open ? (
        <InstallForm
          workspaceSlug={workspaceSlug}
          projectId={projectId}
          canInstall={canInstall}
          canAuthor={canAuthor}
          installable={installable}
          onClose={onClose}
          onInstalled={onInstalled}
          refreshDefinitions={() => schedulers.refetch()}
        />
      ) : null}
    </Dialog>
  );
}
