// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Socket-loss fallback persistence (parity/specs/pages.md §8.1, PAGE-064).
// While the collaboration socket carries keystrokes this module stays
// quiet, so a connected page issues zero description writes. Once the
// sync state settles on the fault badge, every save fetches the latest
// binary, reconciles the local body onto it through the live convert
// call, and patches all three forms together. Saves fire on activation,
// on a roughly half-minute interval, and early on the standard save
// gesture, which also resets the interval. A refused patch surfaces a
// notice and retries on the next tick instead of wedging; a too-large
// refusal raises the oversized strip instead.

import {
  convertDocument,
  getPageDescriptionBinary,
  isApiError,
  updatePageDescription,
  type ApiClient,
} from "@pidash/api-client";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { notifyError } from "../../../shared/shell/toaster.js";
import { reportPageContentTooLarge } from "./pageRealtimeErrors.js";

/** Idle cadence of the fallback patch while the socket is down. */
export const PAGE_SAVE_FALLBACK_INTERVAL_MS = 30_000;
/** The manual save waits out this beat before forcing a patch early. */
export const PAGE_SAVE_MANUAL_DEBOUNCE_MS = 500;
/** The live convert call rejects an empty body, so a blank page saves this. */
export const PAGE_SAVE_EMPTY_HTML = "<p></p>";

/** Notice when a fallback patch fails and will be retried. */
export const PAGE_BODY_SAVE_FAILED_TITLE = "Page content could not be saved.";

export interface PageSaveFallbackInput {
  workspaceSlug: string;
  projectId: string;
  pageId: string;
  /** The newest local body (draft, live editor, or stored snapshot). */
  html: string;
  /** Record name, so the convert keeps the title fragment fresh. */
  title: string;
}

export interface PageSaveFallbackResult {
  description_binary: string;
  description_html: string;
  description_json: Record<string, unknown>;
}

/** Binary bytes as the base64 the convert call and the patch expect. */
export function pageBinaryToBase64(bytes: Uint8Array): string {
  let binary = "";
  const CHUNK = 0x8000;
  for (let offset = 0; offset < bytes.length; offset += CHUNK) {
    binary += String.fromCharCode(...bytes.subarray(offset, offset + CHUNK));
  }
  return btoa(binary);
}

/**
 * One fallback save: fetch the latest binary, reconcile the local body
 * onto it, patch all three forms. Answers the converted forms; rejects
 * with the underlying failure (a 413 means the body is too large).
 */
export async function savePageDescriptionFallback(
  client: ApiClient,
  input: PageSaveFallbackInput
): Promise<PageSaveFallbackResult> {
  const body = input.html.trim() === "" ? PAGE_SAVE_EMPTY_HTML : input.html;
  const latest = await getPageDescriptionBinary(client, input.workspaceSlug, input.projectId, input.pageId);
  const converted = await convertDocument(client, {
    description_html: body,
    variant: "document",
    ...(latest.length === 0 ? {} : { description_binary: pageBinaryToBase64(latest) }),
    title: input.title,
  });
  await updatePageDescription(client, input.workspaceSlug, input.projectId, input.pageId, {
    description_binary: converted.description_binary,
    description_html: converted.description_html,
    description_json: converted.description_json,
  });
  return converted;
}

export interface UsePageSaveFallbackOptions {
  workspaceSlug: string;
  projectId: string;
  pageId: string;
  title: string;
  /** True on an editable page whose socket reports the fault state. */
  active: boolean;
  /** Read the newest local body at save time. */
  readHtml: () => string;
}

export interface PageSaveFallbackControls {
  /** Debounced manual save for the standard save gesture. */
  requestManualSave: () => void;
}

/**
 * Run the fallback while active: one patch on activation, then the idle
 * interval, plus a debounced manual fast path that resets the interval.
 * A patch already in flight skips its tick rather than piling up, and a
 * refused patch reports itself and retries on the next tick.
 */
export function usePageSaveFallback(options: UsePageSaveFallbackOptions): PageSaveFallbackControls {
  const latest = React.useRef(options);
  latest.current = options;
  const inFlight = React.useRef(false);
  const intervalRef = React.useRef<ReturnType<typeof setInterval> | null>(null);
  const manualRef = React.useRef<ReturnType<typeof setTimeout> | null>(null);

  const save = React.useCallback(async (): Promise<void> => {
    if (inFlight.current) return;
    const current = latest.current;
    if (!current.active) return;
    inFlight.current = true;
    try {
      await savePageDescriptionFallback(getAppClient(), {
        workspaceSlug: current.workspaceSlug,
        projectId: current.projectId,
        pageId: current.pageId,
        html: current.readHtml(),
        title: current.title,
      });
    } catch (error) {
      if (isApiError(error) && error.status === 413) {
        reportPageContentTooLarge(current.pageId);
      } else {
        notifyError(PAGE_BODY_SAVE_FAILED_TITLE);
      }
    } finally {
      inFlight.current = false;
    }
  }, []);

  const stopInterval = React.useCallback(() => {
    const pending = intervalRef.current;
    intervalRef.current = null;
    if (pending !== null) clearInterval(pending);
  }, []);

  const startInterval = React.useCallback(() => {
    stopInterval();
    intervalRef.current = setInterval(() => {
      void save();
    }, PAGE_SAVE_FALLBACK_INTERVAL_MS);
  }, [save, stopInterval]);

  const requestManualSave = React.useCallback(() => {
    if (manualRef.current !== null) clearTimeout(manualRef.current);
    manualRef.current = setTimeout(() => {
      manualRef.current = null;
      void save();
      if (latest.current.active) startInterval();
    }, PAGE_SAVE_MANUAL_DEBOUNCE_MS);
  }, [save, startInterval]);

  const { active, pageId } = options;
  React.useEffect(() => {
    if (!active) return;
    void save();
    startInterval();
    return () => {
      stopInterval();
      if (manualRef.current !== null) {
        clearTimeout(manualRef.current);
        manualRef.current = null;
      }
    };
  }, [active, pageId, save, startInterval, stopInterval]);

  return { requestManualSave };
}
