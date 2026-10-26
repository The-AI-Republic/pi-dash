// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Realtime error channel (parity/specs/pages.md §8.6, PAGE-069) and the
// oversized-content incident it feeds (PAGE-063). The collaboration slice
// owns the socket frames; it reports each decoded error here, and the
// strip listens for the too-large code and fires once per incident with
// an optional dismiss. The fallback save reports through the same latch
// when its own patch is refused as too large while the socket is down.

import * as React from "react";

/** Which side of the document lifecycle the failure interrupted. */
export type PageRealtimeErrorPhase = "load" | "persist";

/** One structured failure from the collaboration channel. */
export interface PageRealtimeError {
  pageId: string;
  phase: PageRealtimeErrorPhase;
  /** Machine code, e.g. PAGE_CONTENT_TOO_LARGE. */
  code: string;
  message: string;
  shouldDisconnect: boolean;
}

/** The live server's oversized-content code (PAGE-063). */
export const PAGE_CONTENT_TOO_LARGE = "content_too_large";

type ErrorListener = (error: PageRealtimeError) => void;
const errorListeners = new Set<ErrorListener>();

type IncidentListener = () => void;
const incidentListeners = new Set<IncidentListener>();

let incidentSeq = 0;
/** The newest incident per page; a dismiss records the seq it cleared. */
const incidents = new Map<string, number>();
const dismissed = new Map<string, number>();

function emitIncident(): void {
  for (const listener of incidentListeners) listener();
}

function subscribeIncidents(listener: IncidentListener): () => void {
  incidentListeners.add(listener);
  return () => {
    incidentListeners.delete(listener);
  };
}

function fireIncident(pageId: string): void {
  incidentSeq += 1;
  incidents.set(pageId, incidentSeq);
  emitIncident();
}

/**
 * Report one decoded realtime error to every subscriber. A too-large
 * report also raises the oversized incident the strip reads.
 */
export function reportPageRealtimeError(error: PageRealtimeError): void {
  if (error.code === PAGE_CONTENT_TOO_LARGE) fireIncident(error.pageId);
  for (const listener of errorListeners) listener(error);
}

/** Subscribe to realtime errors; the strip filters its own code. */
export function subscribePageRealtimeErrors(listener: ErrorListener): () => void {
  errorListeners.add(listener);
  return () => {
    errorListeners.delete(listener);
  };
}

/** Record a too-large rejection outside the socket (a 413 fallback patch). */
export function reportPageContentTooLarge(pageId: string): void {
  fireIncident(pageId);
}

/** Dismiss the current incident for a page; the next report re-fires it. */
export function dismissPageOversizedIncident(pageId: string): void {
  dismissed.set(pageId, incidents.get(pageId) ?? 0);
  emitIncident();
}

function isIncidentVisible(pageId: string): boolean {
  const seq = incidents.get(pageId) ?? 0;
  return seq > 0 && seq > (dismissed.get(pageId) ?? 0);
}

export interface PageOversizedIncident {
  visible: boolean;
  dismiss: () => void;
}

/**
 * The oversized strip's state for one page: visible from the first
 * too-large report until dismissed, and again on the next report.
 */
export function usePageOversizedIncident(pageId: string): PageOversizedIncident {
  const getSnapshot = React.useCallback(() => isIncidentVisible(pageId), [pageId]);
  const visible = React.useSyncExternalStore(subscribeIncidents, getSnapshot, getSnapshot);
  const dismiss = React.useCallback(() => dismissPageOversizedIncident(pageId), [pageId]);
  return { visible, dismiss };
}

/** Tests-only reset; incidents must not leak between suites. */
export function resetPageRealtimeErrors(): void {
  errorListeners.clear();
  incidentListeners.clear();
  incidents.clear();
  dismissed.clear();
  incidentSeq = 0;
}
