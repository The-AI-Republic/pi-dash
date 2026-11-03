// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Work-item embed and entity-reference producers (parity/specs/pages.md
// §6.2–§6.3, §11.5, PAGE-040/043). The picker resolves a typed term through
// the shared entity-search contract; the pure builders below turn the chosen
// row into the identifiers the atomic nodes store. Keeping this separate from
// the interaction layer makes the identifier mapping testable without a
// browser or a live server.

import {
  searchEntityPreviews,
  type ApiClient,
  type EntitySearchIssue,
  type EntitySearchPage,
} from "@pidash/api-client";

import type { PageEntityReferenceAttributes } from "../../../shared/editor/pageBlockExtensions.js";
import type { PageEmbedAttributes } from "./pageBlocks.js";

/** One searchable work item a page may embed or reference. */
export interface PageEntityChoice {
  id: string;
  name: string;
  sequenceId: number;
  projectId: string;
  projectIdentifier: string;
  kind: "issue" | "page";
  workspaceSlug: string;
}

/** Attributes an embed stores from a chosen work item (PAGE-040). */
export function pageEmbedFor(choice: PageEntityChoice, workspaceIdentifier: string): PageEmbedAttributes {
  return {
    id: choice.id,
    entity_identifier: choice.id,
    project_identifier: choice.projectIdentifier,
    workspace_identifier: workspaceIdentifier,
    entity_name: choice.name,
  };
}

/** Attributes an entity reference stores from a chosen work item (PAGE-043). */
export function pageEntityReferenceFor(
  choice: PageEntityChoice,
  workspaceIdentifier: string
): PageEntityReferenceAttributes {
  return {
    entity_name: choice.kind,
    entity_identifier: choice.id,
    project_identifier: choice.projectIdentifier,
    workspace_identifier: workspaceIdentifier,
    project_id: choice.projectId,
    "data-label": choice.name,
  };
}

/** Normalise an issue search row into a picker choice. */
export function entityChoiceFromIssue(row: EntitySearchIssue, workspaceSlug: string): PageEntityChoice {
  return {
    id: row.id,
    name: row.name,
    sequenceId: row.sequence_id,
    projectId: row.project_id,
    projectIdentifier: row.project__identifier,
    kind: "issue",
    workspaceSlug,
  };
}

/** Normalise a page search row into a picker choice. */
export function entityChoiceFromPage(row: EntitySearchPage, workspaceSlug: string): PageEntityChoice {
  return {
    id: row.id,
    name: row.name,
    sequenceId: 0,
    projectId: row.projects__id ?? "",
    projectIdentifier: "",
    kind: "page",
    workspaceSlug,
  };
}

/** The visible key a work item shows in the picker. */
export function entityChoiceKey(choice: PageEntityChoice): string {
  return choice.kind === "issue" ? `${choice.projectIdentifier}-${choice.sequenceId}` : choice.name;
}

/** The stored element an entity reference serialises to (PAGE-043). */
export function entityReferenceHtml(attributes: PageEntityReferenceAttributes): string {
  return `<p><entity-reference-component entity_name="${escapeAttribute(attributes.entity_name)}" entity_identifier="${escapeAttribute(attributes.entity_identifier)}" project_identifier="${escapeAttribute(attributes.project_identifier)}" workspace_identifier="${escapeAttribute(attributes.workspace_identifier)}"${attributes.project_id === undefined ? "" : ` project_id="${escapeAttribute(attributes.project_id)}"`} data-label="${escapeAttribute(attributes["data-label"])}"></entity-reference-component></p>`;
}

function escapeAttribute(value: string): string {
  return value.replace(/&/g, "&amp;").replace(/"/g, "&quot;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}

/** Resolve a parsed key through entity search; null when no exact row matches. */
export async function resolvePastedEntity(
  client: ApiClient,
  workspaceSlug: string,
  projectId: string,
  parsed: { projectIdentifier: string; sequenceId: number }
): Promise<PageEntityChoice | null> {
  const sections = await searchEntityPreviews(client, workspaceSlug, {
    query: `${parsed.projectIdentifier}-${parsed.sequenceId}`,
    kinds: ["issue"],
    projectId,
    count: 10,
  });
  const match = (sections.issue ?? []).find(
    (row) => row.project__identifier.toUpperCase() === parsed.projectIdentifier && row.sequence_id === parsed.sequenceId
  );
  return match === undefined ? null : entityChoiceFromIssue(match, workspaceSlug);
}

/** Search issues and pages for the picker, in one project-scoped read. */
export async function searchPageEntityChoices(
  client: ApiClient,
  workspaceSlug: string,
  projectId: string,
  query: string,
  kinds: readonly ("issue" | "page")[] = ["issue"]
): Promise<PageEntityChoice[]> {
  const sections = await searchEntityPreviews(client, workspaceSlug, {
    query,
    kinds,
    projectId,
    count: 10,
  });
  return [
    ...(sections.issue ?? []).map((row) => entityChoiceFromIssue(row, workspaceSlug)),
    ...(sections.page ?? []).map((row) => entityChoiceFromPage(row, workspaceSlug)),
  ];
}
