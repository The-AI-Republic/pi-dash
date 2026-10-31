// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The two cache shapes a notification stream can take (parity/specs/
// notifications.md §6-§8): slice A's single first page and slice B's cursor
// pages. Every writer — card toggles and the header's bulk read — walks
// both shapes through these helpers, so the page layout never leaks into
// the read rules.
import type { InfiniteData } from "@tanstack/react-query";
import type { NotificationListResponse } from "@pidash/api-client";

/** A cached stream: either the first page alone or the cursor pages. */
export type StreamData = NotificationListResponse | InfiniteData<NotificationListResponse, string>;

function isPage(value: unknown): value is NotificationListResponse {
  return typeof value === "object" && value !== null && Array.isArray((value as { results?: unknown }).results);
}

/** Whether a cached value is a single stream page. */
export function isStreamPage(value: unknown): value is NotificationListResponse {
  return isPage(value);
}

/** Every page a cached stream holds, oldest page last. */
export function streamPages(data: StreamData | undefined): NotificationListResponse[] {
  if (data === undefined) return [];
  if (isPage(data)) return [data];
  return data.pages;
}

/** Rebuild a cached stream with each page mapped, keeping its shape. */
export function mapStreamPages(
  data: StreamData,
  map: (page: NotificationListResponse) => NotificationListResponse
): StreamData {
  if (isPage(data)) return map(data);
  return { ...data, pages: data.pages.map(map) };
}
