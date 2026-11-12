// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace export form model (parity/specs/exports.md §2, EXP-003/004/005).
// Provider options, form parsing, the submit wire shape, and the project
// scoping behind the picker trigger. Labels pinned by the oracle scenario
// (exports/workspace-exports.spec.ts) are behavior, not old-app copy.
import { isKnownExportProvider, type ExportProvider, type Project } from "@pidash/api-client";

import { selectCreatableProjects } from "../issues/index.js";

/** One file format the picker offers. */
export interface ExportFormatOption {
  provider: ExportProvider;
  label: string;
}

/** The three formats in oracle order; the first is the default. */
export const EXPORT_FORMAT_OPTIONS: readonly ExportFormatOption[] = [
  { provider: "csv", label: "CSV" },
  { provider: "xlsx", label: "Excel" },
  { provider: "json", label: "JSON" },
];

export const DEFAULT_EXPORT_PROVIDER: ExportProvider = "csv";

/** Trigger text when no project is picked: the choice covers every project. */
export const ALL_PROJECTS_LABEL = "All projects";

/** Visible label for a provider id; unknown ids pass through untouched. */
export function formatLabel(provider: string): string {
  return EXPORT_FORMAT_OPTIONS.find((option) => option.provider === provider)?.label ?? provider;
}

export interface ExportFormValue {
  provider: ExportProvider;
  projectIds: string[];
}

export interface ExportFormInput {
  provider?: unknown;
  projectIds?: unknown;
}

/** Lenient parse of a form value: unknown providers fall back to CSV, non-lists to empty. */
export function parseExportForm(input: ExportFormInput): ExportFormValue {
  const provider =
    typeof input.provider === "string" && isKnownExportProvider(input.provider)
      ? input.provider
      : DEFAULT_EXPORT_PROVIDER;
  const projectIds = Array.isArray(input.projectIds)
    ? input.projectIds.filter((id): id is string => typeof id === "string")
    : [];
  return { provider, projectIds };
}

export interface ExportRequestBody {
  provider: ExportProvider;
  project: string[];
  multiple: boolean;
}

/**
 * The submit wire shape: the provider, the picked ids (empty means every
 * project the caller belongs to), and the separate-files flag for picks
 * of more than one project. The create contract derives the same flag;
 * this copy pins the shape the submit tests assert on the wire.
 */
export function exportRequestBody(value: ExportFormValue): ExportRequestBody {
  return {
    provider: value.provider,
    project: [...value.projectIds],
    multiple: value.projectIds.length > 1,
  };
}

/**
 * Ids the picker may offer: joined, unarchived projects where the caller's
 * role is member or admin — the same rule the create entry points use.
 */
export function selectExportProjectIds(projects: readonly Project[]): string[] {
  // The list rows always carry identifier and role facts, so the projection
  // is total; the creatable rule itself stays the entry points' own.
  const options = projects.map((project) => ({
    id: project.id,
    name: project.name,
    identifier: project.identifier,
    memberRole: project.member_role,
    archived: project.archived_at !== null,
  }));
  return selectCreatableProjects(options).map((option) => option.id);
}

/** Short keys of the picked projects in pick order; stale ids drop out. */
export function selectPickedIdentifiers(
  projectIds: readonly string[],
  projects: readonly Pick<Project, "id" | "identifier">[]
): string[] {
  const keysById = new Map(projects.map((project) => [project.id, project.identifier] as const));
  return projectIds
    .map((id) => keysById.get(id))
    .filter((identifier): identifier is string => identifier !== undefined);
}

/** Trigger text for the picked keys: joined, or All projects when empty. */
export function selectProjectTriggerLabel(identifiers: readonly string[]): string {
  return identifiers.length === 0 ? ALL_PROJECTS_LABEL : identifiers.join(", ");
}
