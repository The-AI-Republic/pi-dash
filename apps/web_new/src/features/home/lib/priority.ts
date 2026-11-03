// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Priority display labels for recent rows (SHELL-017 target per main#894;
// written from parity/specs/shell.md §11). The badge and its tooltip show
// the display label, never the raw lowercase enum value. Unknown values
// resolve to null so the marker is omitted instead of guessed.

import type { BadgeTone } from "@pidash/kit";

const LABELS: Record<string, string> = {
  urgent: "Urgent",
  high: "High",
  medium: "Medium",
  low: "Low",
  none: "None",
};

export function priorityDisplayLabel(priority: string | null | undefined): string | null {
  if (priority === null || priority === undefined) return null;
  return LABELS[priority] ?? null;
}

const TONES: Record<string, BadgeTone> = {
  urgent: "danger",
  high: "warning",
  medium: "accent",
  low: "neutral",
  none: "neutral",
};

export function priorityTone(priority: string): BadgeTone {
  return TONES[priority] ?? "neutral";
}
