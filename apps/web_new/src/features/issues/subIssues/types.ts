// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Shared widget types (parity/specs/issues.md §15, ISS-174/176/177). The
// row and the section speak through this one handle: what a row write
// looks like, which row is busy, and the row actions the menu offers.

import type { IssuePatch, SubIssueItem } from "@pidash/api-client";

import type { FilterSectionOption } from "../filters/shared/FilterSection.js";

/** One inline edit: a single field with its value. */
export interface SubIssueWrite {
  row: SubIssueItem;
  patch: IssuePatch;
  field: keyof IssuePatch;
}

/** The handle the section hands every row. */
export interface SubIssueRowActions {
  /** True while the row's latest write is in flight. */
  isBusy: (rowId: string) => boolean;
  /** Persist one field; the promise settles when the cache revalidated. */
  write: (input: SubIssueWrite) => void;
}

/** Display name and avatar for one member id, resolved from the project's members. */
export interface SubIssueMemberIdentity {
  label: string;
  avatarUrl: string | null;
}

/** Reference rows the filter menu draws from the section's own reads. */
export interface SubIssueFilterSources {
  /** Assignee rows; already narrowed to the project's members. */
  assigneeRows: FilterSectionOption[];
  /** Assignee rows are still resolving. */
  assigneesLoading: boolean;
}
