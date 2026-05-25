/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

/** 16-color palette mirrored in migration 0140's auto-assignment. */
export const SCHEDULER_COLOR_PALETTE = [
  "#3b82f6",
  "#6366f1",
  "#8b5cf6",
  "#a855f7",
  "#d946ef",
  "#ec4899",
  "#ef4444",
  "#f97316",
  "#eab308",
  "#84cc16",
  "#22c55e",
  "#10b981",
  "#14b8a6",
  "#06b6d4",
  "#0ea5e9",
  "#f59e0b",
];

export const DEFAULT_SCHEDULER_COLOR = SCHEDULER_COLOR_PALETTE[0];

/** Default IANA tz the API falls back to when the binding doesn't specify one. */
export const DEFAULT_TZID = "UTC";
