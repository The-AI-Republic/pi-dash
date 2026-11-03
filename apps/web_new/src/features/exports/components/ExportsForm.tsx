// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace export form (parity/specs/exports.md §2, EXP-003/004/005). The
// guide's top half: a project picker scoped to creatable projects, a format
// picker, and the submit. The settings-page slice (NEWFRONT-768) mounts this
// inside the frame. The box titles match the parity driver's titled boxes and
// the pinned labels are oracle behavior, not old-app copy.

import { Button, Combobox } from "@pidash/kit";
import type { ExportProvider } from "@pidash/api-client";
import * as React from "react";

import { ProjectPicker, usePickerProjects } from "../../../shared/pickers/index.js";
import { useCreateWorkspaceExport } from "../api/mutations.js";
import {
  DEFAULT_EXPORT_PROVIDER,
  EXPORT_FORMAT_OPTIONS,
  formatLabel,
  parseExportForm,
  selectExportProjectIds,
  selectPickedIdentifiers,
  selectProjectTriggerLabel,
} from "../exportForm.js";

export interface ExportsFormProps {
  workspaceSlug: string;
}

/** Box titles the parity driver reads the picker labels through. */
const PROJECTS_HEADING = "Exporting project";
const FORMAT_HEADING = "Format";

const FORMAT_COMBOBOX_OPTIONS = EXPORT_FORMAT_OPTIONS.map((option) => ({
  id: option.provider,
  label: option.label,
}));

/**
 * The workspace export form (EXP-003/004/005): pick creatable projects (empty
 * means all), pick CSV/Excel/JSON, and submit. The project trigger compacts
 * picks to their short keys; the submit works in flight, confirms with the
 * format on success, prompts a retry on failure, and stays usable after.
 */
export function ExportsForm({ workspaceSlug }: ExportsFormProps): React.ReactElement {
  const [provider, setProvider] = React.useState<ExportProvider>(DEFAULT_EXPORT_PROVIDER);
  const [projectIds, setProjectIds] = React.useState<string[]>([]);
  // The picker's lazy read shares this key, so the form's eager read feeds
  // both the permission filter and the option list with a single fetch.
  const projectsQuery = usePickerProjects(workspaceSlug, true);
  const creatableIds = React.useMemo(
    () => (projectsQuery.data === undefined ? [] : selectExportProjectIds(projectsQuery.data)),
    [projectsQuery.data]
  );
  const projectLabel = selectProjectTriggerLabel(selectPickedIdentifiers(projectIds, projectsQuery.data ?? []));
  const createExport = useCreateWorkspaceExport();
  const busy = createExport.isPending;
  // State flips on render; the ref closes the synchronous double-submit
  // window (a second submit before the disabled button paints).
  const submitGuard = React.useRef(false);

  function handleSubmit(event: React.FormEvent<HTMLFormElement>): void {
    event.preventDefault();
    if (submitGuard.current) return;
    submitGuard.current = true;
    createExport.mutate(
      { workspaceSlug, value: parseExportForm({ provider, projectIds }) },
      {
        onSettled: () => {
          submitGuard.current = false;
        },
      }
    );
  }

  return (
    <form onSubmit={handleSubmit} className="flex flex-col gap-(--space-6)">
      <section className="flex flex-col gap-(--space-2)">
        <h3 className="text-emphasis font-medium text-(--text)">{PROJECTS_HEADING}</h3>
        <ProjectPicker
          workspaceSlug={workspaceSlug}
          multiple
          value={projectIds}
          onChange={setProjectIds}
          includeProjectIds={creatableIds}
          trigger={
            <Button>
              <span className="truncate">{projectLabel}</span>
            </Button>
          }
        />
      </section>
      <section className="flex flex-col gap-(--space-2)">
        <h3 className="text-emphasis font-medium text-(--text)">{FORMAT_HEADING}</h3>
        <Combobox
          trigger={
            <Button>
              <span className="truncate">{formatLabel(provider)}</span>
            </Button>
          }
          options={FORMAT_COMBOBOX_OPTIONS}
          value={provider}
          onSelect={(id) => setProvider(id as ExportProvider)}
          listLabel="Format"
          searchEnabled={false}
          emptyText="No formats available"
        />
      </section>
      <div>
        <Button type="submit" variant="primary" disabled={busy}>
          {busy ? "Exporting..." : "Export"}
        </Button>
      </div>
    </form>
  );
}
