// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Analytics project picker (parity/specs/analytics.md §4, ANL-004).
// The shared multi-select over the caller's selectable projects
// with an analytics summary trigger: the all-projects label, one
// name, a joined name list, or the fixed long-selection literal.
// Options, search, and keyboard behavior are the shared picker's;
// this module owns only the option set and the trigger wording.

import { Button } from "@pidash/kit";
import * as React from "react";

import { ProjectPicker } from "../../../shared/pickers/ProjectPicker.js";
import { selectPickerSummary } from "../lib/scope.js";

/** Selectable project: offered as an option and used for the summary. */
export interface AnalyticsPickerProject {
  id: string;
  name: string;
}

export interface AnalyticsPickerProps {
  workspaceSlug: string;
  projects: readonly AnalyticsPickerProject[];
  /** Address selection in selection order; unresolvable ids are label-silent. */
  value: readonly string[];
  onChange: (ids: string[]) => void;
}

export function AnalyticsPicker({
  workspaceSlug,
  projects,
  value,
  onChange,
}: AnalyticsPickerProps): React.ReactElement {
  const names = React.useMemo(() => {
    const byId = new Map(projects.map((project) => [project.id, project.name] as const));
    return value.flatMap((id) => {
      const name = byId.get(id);
      return name === undefined ? [] : [name];
    });
  }, [projects, value]);
  const includeProjectIds = React.useMemo(() => projects.map((project) => project.id), [projects]);
  return (
    <ProjectPicker
      workspaceSlug={workspaceSlug}
      multiple
      value={value}
      onChange={onChange}
      includeProjectIds={includeProjectIds}
      trigger={
        <Button variant="secondary" data-testid="analytics-picker-trigger">
          <span className="truncate">{selectPickerSummary(names)}</span>
        </Button>
      }
    />
  );
}
