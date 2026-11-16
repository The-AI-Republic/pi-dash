// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The module write form (parity/specs/modules.md §7.1, §7.2, MOD-022/023/
// 024). One form serves both modes: create starts blank with the project
// picker (a new module may land in another project the viewer may write
// to), update starts prefilled without it. The keyboard sequence is the
// visual field order with the date control third (bug NEWFRONT-330): the
// picker is ambient and carries no position. A successful write closes
// the dialog and shows the success notice; a failed write keeps every
// entered field and shows the server's own sentence (bug NEWFRONT-328).

import { useQuery, useQueryClient } from "@tanstack/react-query";
import {
  createModule,
  updateModule,
  MODULE_STATUSES,
  type ModuleRow,
  type ModuleStatus,
  type ProjectList,
} from "@pidash/api-client";
import { Badge, Button, Combobox, Input, Textarea, type ComboboxOption } from "@pidash/kit";
import * as React from "react";

import { getAppClient } from "../../../../core/api/client.js";
import { DateRangePicker, type RangeValue } from "../../../issues/index.js";
import { projectsQueryOptions } from "../../../projects/index.js";
import { MemberPicker, ProjectPicker, type ProjectOption } from "../../../../shared/pickers/index.js";
import { notifyError, notifySuccess } from "../../../../shared/shell/index.js";
import { invalidateAfterModuleWrite } from "../../api/invalidate.js";
import { MODULE_STATUS_LABELS, MODULE_STATUS_TONES } from "../sidebar/index.js";
import {
  creatableModuleProjectIds,
  emptyModuleForm,
  MODULE_FAILURE_TITLE,
  MODULE_FORM_TAB_ORDER,
  MODULE_SUCCESS_TITLE,
  MODULE_TITLE_MAX,
  moduleCreateNotice,
  moduleCreatePayload,
  moduleFailureBody,
  moduleFormFromRecord,
  moduleSubmitLabel,
  moduleTargetProjectId,
  moduleTitleError,
  moduleUpdateNotice,
  moduleUpdatePayload,
  type ModuleDialogMode,
  type ModuleFormState,
} from "../../lib/moduleForm.js";

/** One project as the creation picker labels it before its read resolves. */
export interface ModuleFormProject {
  id: string;
  name: string;
  /** Short identifier the picker shows; empty when the host lacks it. */
  identifier: string;
}

export interface ModuleFormProps {
  mode: ModuleDialogMode;
  workspaceSlug: string;
  /** The project the host is on (create's preselection, update's target). */
  currentProject: ModuleFormProject;
  /** The record being edited; present exactly in update mode. */
  module?: ModuleRow | undefined;
  onOpenChange: (open: boolean) => void;
  onSubmittingChange: (submitting: boolean) => void;
}

function knownProjectOption(project: ModuleFormProject): ProjectOption {
  return {
    id: project.id,
    name: project.name,
    identifier: project.identifier,
    logoProps: {},
    searchText: `${project.name} ${project.identifier}`,
    selected: true,
  };
}

function toStatusOption(status: ModuleStatus): ComboboxOption {
  return {
    id: status,
    label: MODULE_STATUS_LABELS[status],
    trailing: (
      <Badge tone={MODULE_STATUS_TONES[status]} dot>
        {MODULE_STATUS_LABELS[status]}
      </Badge>
    ),
  };
}

interface ModuleStatusFieldProps {
  value: ModuleStatus;
  disabled: boolean;
  tabIndex: number;
  onChange: (status: ModuleStatus) => void;
}

/** The required lifecycle select; the trigger carries the tab position. */
function ModuleStatusField({ value, disabled, tabIndex, onChange }: ModuleStatusFieldProps): React.ReactElement {
  const [open, setOpen] = React.useState(false);
  const label = MODULE_STATUS_LABELS[value];
  return (
    <div data-testid="module-dialog-status" className="inline-flex">
      <Combobox
        trigger={
          <Button variant="secondary" type="button" tabIndex={tabIndex} aria-label={`Status: ${label}`}>
            <Badge tone={MODULE_STATUS_TONES[value]} dot>
              {label}
            </Badge>
          </Button>
        }
        options={MODULE_STATUSES.map(toStatusOption)}
        value={value}
        onSelect={(next) => {
          setOpen(false);
          onChange(next as ModuleStatus);
        }}
        listLabel="Module statuses"
        searchEnabled={false}
        emptyText="No statuses available"
        disabled={disabled}
        open={open}
        onOpenChange={setOpen}
      />
    </div>
  );
}

export function ModuleForm({
  mode,
  workspaceSlug,
  currentProject,
  module,
  onOpenChange,
  onSubmittingChange,
}: ModuleFormProps): React.ReactElement {
  // The form remounts per open (the dialog keys it), so the initializers
  // run once per visit and never carry one visit's input into the next.
  const [form, setForm] = React.useState<ModuleFormState>(() =>
    mode === "update" && module !== undefined
      ? moduleFormFromRecord(module, currentProject.id)
      : emptyModuleForm(currentProject.id)
  );
  const queryClient = useQueryClient();
  const [titleError, setTitleError] = React.useState<string | null>(null);
  const [submitting, setSubmitting] = React.useState(false);
  const titleRef = React.useRef<HTMLInputElement | null>(null);

  // Only creation offers a project; the read stays off in update mode.
  const projects = useQuery({
    ...projectsQueryOptions(getAppClient(), workspaceSlug),
    enabled: mode === "create",
  });
  const creatableIds = React.useMemo(
    () => (projects.data === undefined ? [] : creatableModuleProjectIds(projects.data as ProjectList)),
    [projects.data]
  );

  React.useEffect(() => {
    titleRef.current?.focus();
  }, []);

  function update(patch: Partial<ModuleFormState>): void {
    setForm((current) => ({ ...current, ...patch }));
  }

  function setPending(next: boolean): void {
    setSubmitting(next);
    onSubmittingChange(next);
  }

  async function submit(event: React.FormEvent): Promise<void> {
    event.preventDefault();
    const error = moduleTitleError(form.title);
    if (error !== null) {
      setTitleError(error);
      titleRef.current?.focus();
      return;
    }
    const target = moduleTargetProjectId(form);
    setPending(true);
    try {
      if (mode === "update" && module !== undefined) {
        await updateModule(getAppClient(), workspaceSlug, currentProject.id, module.id, moduleUpdatePayload(form));
        await invalidateAfterModuleWrite(queryClient, workspaceSlug, currentProject.id, module.id);
        notifySuccess(MODULE_SUCCESS_TITLE, moduleUpdateNotice());
      } else {
        const created = await createModule(getAppClient(), workspaceSlug, target, moduleCreatePayload(form));
        await invalidateAfterModuleWrite(queryClient, workspaceSlug, target, created.id);
        notifySuccess(MODULE_SUCCESS_TITLE, moduleCreateNotice(created.name));
      }
      onOpenChange(false);
    } catch (writeError) {
      // Nothing is stored; the dialog stays up with every field as it was.
      notifyError(MODULE_FAILURE_TITLE, moduleFailureBody(writeError));
    } finally {
      setPending(false);
    }
  }

  return (
    <form
      data-testid="module-dialog-form"
      onSubmit={(event) => {
        void submit(event);
      }}
    >
      <div className="flex flex-col gap-(--space-5)">
        {mode === "create" ? (
          <div data-testid="module-dialog-project" className="flex items-center gap-(--space-3)">
            <span className="text-body font-medium text-(--text)">Project</span>
            <ProjectPicker
              workspaceSlug={workspaceSlug}
              value={form.projectId}
              onChange={(id) => {
                update({ projectId: id });
              }}
              includeProjectIds={creatableIds}
              knownProjects={[knownProjectOption(currentProject)]}
              disabled={submitting}
            />
          </div>
        ) : null}
        <Input
          label="Title"
          name="name"
          placeholder="Title"
          tabIndex={MODULE_FORM_TAB_ORDER.title}
          value={form.title}
          inputRef={titleRef}
          {...(titleError === null ? {} : { error: titleError })}
          disabled={submitting}
          onBlur={() => {
            // Leaving an untouched blank field is not a submission: surface
            // a length problem early, but hold the required error until the
            // user actually submits (clearing it when the field is empty).
            setTitleError(form.title.trim() === "" ? null : moduleTitleError(form.title));
          }}
          onChange={(event) => {
            const next = event.target.value;
            update({ title: next });
            if (titleError !== null) setTitleError(moduleTitleError(next));
          }}
        />
        <Textarea
          label="Description"
          name="description"
          placeholder="Description"
          rows={3}
          tabIndex={MODULE_FORM_TAB_ORDER.description}
          value={form.description}
          disabled={submitting}
          onChange={(event) => {
            update({ description: event.target.value });
          }}
        />
        <div data-testid="module-dialog-inline" className="flex flex-wrap items-center gap-(--space-2)">
          <div data-testid="module-dialog-dates" className="inline-flex">
            <DateRangePicker
              mode="split"
              label="Module dates"
              value={form.range}
              onChange={(range: RangeValue) => {
                update({ range });
              }}
              tabIndex={MODULE_FORM_TAB_ORDER.dateRange}
              clearable
              disabled={submitting}
              testId="module-dialog-date-range"
            />
          </div>
          <ModuleStatusField
            value={form.status}
            disabled={submitting}
            tabIndex={MODULE_FORM_TAB_ORDER.status}
            onChange={(status) => {
              update({ status });
            }}
          />
          <div data-testid="module-dialog-lead" className="inline-flex">
            <MemberPicker
              workspaceSlug={workspaceSlug}
              projectId={form.projectId}
              multiple={false}
              value={form.leadId}
              onChange={(id) => {
                update({ leadId: id });
              }}
              placeholder="Lead"
              tabIndex={MODULE_FORM_TAB_ORDER.lead}
              preload
              disabled={submitting}
            />
          </div>
          <div data-testid="module-dialog-members" className="inline-flex">
            <MemberPicker
              workspaceSlug={workspaceSlug}
              projectId={form.projectId}
              multiple
              value={form.memberIds}
              onChange={(ids) => {
                update({ memberIds: ids });
              }}
              placeholder="Members"
              tabIndex={MODULE_FORM_TAB_ORDER.members}
              preload
              disabled={submitting}
            />
          </div>
        </div>
        <p className="text-meta text-(--text-muted)">
          A title is required and may be at most {MODULE_TITLE_MAX} characters.
        </p>
      </div>
      <div className="mt-(--space-6) flex justify-end gap-(--space-3)">
        <Button
          variant="secondary"
          type="button"
          tabIndex={MODULE_FORM_TAB_ORDER.cancel}
          onClick={() => {
            onOpenChange(false);
          }}
        >
          Cancel
        </Button>
        <Button
          variant="primary"
          type="submit"
          tabIndex={MODULE_FORM_TAB_ORDER.submit}
          loading={submitting}
          disabled={submitting}
        >
          {moduleSubmitLabel(mode, submitting)}
        </Button>
      </div>
    </form>
  );
}
