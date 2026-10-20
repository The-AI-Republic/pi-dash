// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cloud issue-type seam for the modal (parity/specs/issues.md §11/§23,
// ISS-129). The work-item type picker, the template picker and the
// additional-properties block belong to the cloud edition, which
// supplies them through this slot; the community build registers
// nothing, so the modal renders no trace of any of the three.
//
// §23 proof: the OSS leg asserts absence (no "Issue type" or
// "Template" text in the modal), the cloud leg drives the real pickers.

import type { ComponentType } from "react";

/** Props every cloud modal extension receives. */
export interface IssueTypeExtensionProps {
  workspaceSlug: string;
  /** Project the form is filing under; empty until one is picked. */
  projectId: string;
  /** True while a write is in flight; the pickers disable. */
  disabled: boolean;
  /** The chosen type, or null (the project default applies). */
  typeId: string | null;
  onTypeChange: (typeId: string | null) => void;
  /** A template's prefill, already shaped for the form. */
  onTemplateApply: (prefill: TemplatePrefill) => void;
  /** Extra property values the create/update payload carries. */
  propertyValues: Record<string, unknown>;
  onPropertyValuesChange: (values: Record<string, unknown>) => void;
}

/** What applying a template pre-fills. */
export interface TemplatePrefill {
  name?: string;
  descriptionHtml?: string;
  stateId?: string | null;
  priority?: string;
  assigneeIds?: string[];
  labelIds?: string[];
  typeId?: string | null;
  propertyValues?: Record<string, unknown>;
}

/** The three cloud pieces; any may be absent. */
export interface IssueTypeExtension {
  typePicker?: ComponentType<IssueTypeExtensionProps>;
  templatePicker?: ComponentType<IssueTypeExtensionProps>;
  additionalProperties?: ComponentType<IssueTypeExtensionProps>;
}

let extension: IssueTypeExtension | null = null;

/**
 * Register the cloud extension. Called once by the edition bootstrap;
 * the community build never calls it, so the seam stays empty.
 */
export function registerIssueTypeExtension(next: IssueTypeExtension | null): void {
  extension = next;
}

/** The registered extension, or null in the community build. */
export function getIssueTypeExtension(): IssueTypeExtension | null {
  return extension;
}

/** Whether the type picker, template picker or property block exists here. */
export function hasIssueTypeExtension(): boolean {
  return extension !== null;
}

/** For tests: forget any registration. */
export function resetIssueTypeExtension(): void {
  extension = null;
}
