/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { EViewAccess } from "@pi-dash/types";

export const VIEW_ACCESS_SPECIFIERS: {
  key: EViewAccess;
  i18n_label: string;
}[] = [
  { key: EViewAccess.PUBLIC, i18n_label: "Public" },
  { key: EViewAccess.PRIVATE, i18n_label: "Private" },
];

export const VIEW_SORTING_KEY_OPTIONS = [
  { key: "name", i18n_label: "Name" },
  { key: "created_at", i18n_label: "Created at" },
  { key: "updated_at", i18n_label: "Updated at" },
];

export const VIEW_SORT_BY_OPTIONS = [
  { key: "asc", i18n_label: "Ascending" },
  { key: "desc", i18n_label: "Descending" },
];
