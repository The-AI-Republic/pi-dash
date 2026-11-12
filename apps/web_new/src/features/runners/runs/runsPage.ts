// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The runs page address (NEWFRONT-545). Route modules import only this
// small, dependency-free module so the search validator never pulls the
// runs screen's data and presentation code into the initial bundle.

/** Reads `?page=`; absent, malformed and page-one values all mean page 1. */
export function parseRunsPage(search: Record<string, unknown>): number {
  const raw = search["page"];
  const candidate = typeof raw === "number" ? raw : typeof raw === "string" ? Number.parseInt(raw, 10) : Number.NaN;
  return Number.isFinite(candidate) && candidate > 1 ? Math.trunc(candidate) : 1;
}

/** Search object for a page; page 1 leaves the query out of the address. */
export function runsPageSearch(page: number): { page?: number } {
  return page <= 1 ? {} : { page };
}

/** Address for the list or one run, carrying a non-first page in the query. */
export function runsHref(base: string, page: number, runId: string | null = null): string {
  const path = runId === null ? `${base}/runs` : `${base}/runs/${encodeURIComponent(runId)}`;
  return page <= 1 ? path : `${path}?page=${page}`;
}
