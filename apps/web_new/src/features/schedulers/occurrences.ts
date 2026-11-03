// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Occurrence reads for parity/specs/agents.md §5 (AGT-015–017). The
// calendar asks for one window at a time and the server answers with the
// firings inside it plus a truncation flag; the window never exceeds the
// server's 90-day cap, so stepping and view switching always produce a
// legal read.

import { queryOptions, useQuery } from "@tanstack/react-query";
import { listSchedulerOccurrences, type ApiClient } from "@pidash/api-client";

import { getAppClient } from "../../core/api/client.js";
import { LIST_STALE_TIME_MS, workspaceScope } from "../../core/query/client.js";

/** The widest window the API accepts; a longer one answers 400. */
export const MAX_WINDOW_DAYS = 90;

const DAY_MS = 24 * 60 * 60 * 1000;

export interface OccurrenceWindow {
  /** Inclusive lower bound, ISO instant. */
  from: string;
  /** Inclusive upper bound, ISO instant. */
  to: string;
}

export const occurrenceKeys = {
  all: (workspaceSlug: string, projectId: string) =>
    [...workspaceScope(workspaceSlug), "scheduler-occurrences", projectId] as const,
  window: (workspaceSlug: string, projectId: string, window: OccurrenceWindow) =>
    [...occurrenceKeys.all(workspaceSlug, projectId), window.from, window.to] as const,
};

/**
 * Pull the end of a window back to the cap when the caller asked for more.
 * The start is the anchor; a window that already fits is returned as is so
 * the query key stays stable between renders.
 */
export function clampWindow(window: OccurrenceWindow, maxDays: number = MAX_WINDOW_DAYS): OccurrenceWindow {
  const from = new Date(window.from).getTime();
  const to = new Date(window.to).getTime();
  if (!Number.isFinite(from) || !Number.isFinite(to)) return window;
  if (to - from <= maxDays * DAY_MS) return window;
  return { from: window.from, to: new Date(from + maxDays * DAY_MS).toISOString() };
}

export function occurrencesQueryOptions(
  client: ApiClient,
  workspaceSlug: string,
  projectId: string,
  window: OccurrenceWindow
) {
  const clamped = clampWindow(window);
  return queryOptions({
    queryKey: occurrenceKeys.window(workspaceSlug, projectId, clamped),
    queryFn: () => listSchedulerOccurrences(client, workspaceSlug, projectId, clamped),
    staleTime: LIST_STALE_TIME_MS,
  });
}

export function useSchedulerOccurrences(workspaceSlug: string, projectId: string, window: OccurrenceWindow) {
  return useQuery(occurrencesQueryOptions(getAppClient(), workspaceSlug, projectId, window));
}

/** Truncation hint shown beside the calendar title. */
export const TRUNCATION_NOTICE = "Too many occurrences in this window";
