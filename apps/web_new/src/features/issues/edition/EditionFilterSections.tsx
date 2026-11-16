// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Edition-only sections of the sub-issue filter dropdown (ISS-234). Each
// section renders only when its own flag is on AND the edition registered
// the option source; the community build shows neither section, which is
// what the oracle asserts (no word "team", no issue-type label). The
// chrome is the dropdown's own section control, so the edition rows behave
// exactly like the built-in dimensions.

import { X } from "lucide-react";
import { useQuery } from "@tanstack/react-query";
import * as React from "react";

import { FilterSection, type FilterSectionOption } from "../filters/shared/FilterSection.js";
import { isEditionFlagOn, ISSUE_TYPE_FILTER_FLAG, TEAM_PROJECT_FILTER_FLAG } from "./flags.js";
import { appliedChips, type EditionChip, type EditionFilters } from "./filterModel.js";
import { getEditionIssuesProviders } from "./providers.js";

/** Query-key scope for the edition option reads. */
export function editionOptionsKey(workspaceSlug: string, projectId: string, kind: "team-projects" | "types") {
  return ["ws", workspaceSlug, "issues", "edition-options", kind, projectId] as const;
}

/** Team projects the scoping section offers; empty in the community build. */
export function useTeamProjectOptions(workspaceSlug: string, projectId: string, enabled: boolean) {
  const source = getEditionIssuesProviders().teamProjects;
  return useQuery({
    queryKey: editionOptionsKey(workspaceSlug, projectId, "team-projects"),
    // The read stays disabled without a source, so it always has one here.
    queryFn: () => source!({ workspaceSlug, projectId }),
    enabled: enabled && isEditionFlagOn(TEAM_PROJECT_FILTER_FLAG) && source !== undefined,
  });
}

/** Work-item types the type section offers; empty in the community build. */
export function useIssueTypeOptions(workspaceSlug: string, projectId: string, enabled: boolean) {
  const source = getEditionIssuesProviders().workItemTypes;
  return useQuery({
    queryKey: editionOptionsKey(workspaceSlug, projectId, "types"),
    queryFn: () => source!({ workspaceSlug, projectId }),
    enabled: enabled && isEditionFlagOn(ISSUE_TYPE_FILTER_FLAG) && source !== undefined,
  });
}

export interface EditionFilterSectionsProps {
  workspaceSlug: string;
  projectId: string;
  /** True while the dropdown is open; reads stay idle otherwise. */
  open: boolean;
  filters: EditionFilters;
  onToggle: (key: keyof EditionFilters, value: string) => void;
}

export function EditionFilterSections({
  workspaceSlug,
  projectId,
  open,
  filters,
  onToggle,
}: EditionFilterSectionsProps): React.ReactElement {
  const teamProjects = useTeamProjectOptions(workspaceSlug, projectId, open);
  const issueTypes = useIssueTypeOptions(workspaceSlug, projectId, open);
  const teamRows: FilterSectionOption[] = (teamProjects.data ?? []).map((row) => ({
    id: row.id,
    label: row.name,
    searchText: `${row.name} ${row.identifier}`,
  }));
  const typeRows: FilterSectionOption[] = (issueTypes.data ?? []).map((row) => ({
    id: row.id,
    label: row.name,
    searchText: row.name,
  }));
  return (
    <>
      {isEditionFlagOn(TEAM_PROJECT_FILTER_FLAG) ? (
        <FilterSection
          title="Team projects"
          kind="multi"
          applied={filters.teamProject}
          options={teamRows}
          onToggle={(value) => onToggle("teamProject", value)}
          loading={open && teamProjects.isPending}
          loadingText="Loading team projects"
          errorText={teamProjects.isError ? "Couldn't load team projects." : undefined}
          testId="sub-issue-filter-team-project"
          className="py-(--space-2)"
        />
      ) : null}
      {isEditionFlagOn(ISSUE_TYPE_FILTER_FLAG) ? (
        <FilterSection
          title="Work item type"
          kind="multi"
          applied={filters.issueType}
          options={typeRows}
          onToggle={(value) => onToggle("issueType", value)}
          loading={open && issueTypes.isPending}
          loadingText="Loading types"
          errorText={issueTypes.isError ? "Couldn't load types." : undefined}
          testId="sub-issue-filter-type"
          className="py-(--space-2)"
        />
      ) : null}
    </>
  );
}

/** Applied chip row for the edition dimensions; null with nothing applied. */
export function EditionAppliedChips({
  chips,
  onRemove,
}: {
  chips: readonly EditionChip[];
  onRemove: (chip: EditionChip) => void;
}): React.ReactElement | null {
  if (chips.length === 0) return null;
  return (
    <div data-testid="edition-filter-chips" className="flex flex-wrap items-center gap-(--space-1)">
      {chips.map((chip) => (
        <span
          key={`${chip.key}-${chip.value}`}
          className="text-meta inline-flex items-center gap-(--space-1) rounded-(--radius-control) bg-(--subtle) px-(--space-2) py-px text-(--text)"
        >
          {chip.label}
          <button
            type="button"
            aria-label={`Remove ${chip.label} filter`}
            onClick={() => onRemove(chip)}
            className="text-(--text-muted) hover:text-(--text)"
          >
            <X size={11} strokeWidth={2} aria-hidden="true" />
          </button>
        </span>
      ))}
    </div>
  );
}

export { appliedChips };
