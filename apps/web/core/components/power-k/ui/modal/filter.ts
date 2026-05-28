/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

export const powerKCommandFilter = (value: string, search: string, keywords?: string[]) => {
  if (value === "no-results") return 1;

  const normalizedSearch = search.trim().toLowerCase();
  if (value.toLowerCase().includes(normalizedSearch)) return 1;
  if (keywords?.some((keyword) => keyword.toLowerCase().includes(normalizedSearch))) return 1;

  return 0;
};
