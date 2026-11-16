// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Priority option shaping (parity/specs/issues.md §17, ISS-210). Five
// fixed levels over the issue contract's enum, each with its own glyph;
// the empty level is a real value, not a clear action. Urgent is the
// outlined alert and none the barred circle, matching the glyphs users
// already read on every old surface; the three middle levels are signal
// bars.
import type { LucideIcon } from "lucide-react";
import { AlertCircle, Ban, SignalHigh, SignalLow, SignalMedium } from "lucide-react";

import { ISSUE_PRIORITIES } from "@pidash/api-client";

export type PriorityLevel = (typeof ISSUE_PRIORITIES)[number];

export const PRIORITY_LEVELS: readonly PriorityLevel[] = ISSUE_PRIORITIES;

export const PRIORITY_LABELS: Record<PriorityLevel, string> = {
  urgent: "Urgent",
  high: "High",
  medium: "Medium",
  low: "Low",
  none: "None",
};

export const PRIORITY_ICONS: Record<PriorityLevel, LucideIcon> = {
  urgent: AlertCircle,
  high: SignalHigh,
  medium: SignalMedium,
  low: SignalLow,
  none: Ban,
};

/** Glyph tones; urgent additionally renders an outline. */
export const PRIORITY_TONES: Record<PriorityLevel, string> = {
  urgent: "text-(--danger)",
  high: "text-(--warning)",
  medium: "text-(--accent)",
  low: "text-(--text-muted)",
  none: "text-(--text-muted)",
};

/** One priority row in level order. */
export interface PriorityOption {
  level: PriorityLevel;
  label: string;
}

export function selectPriorityOptions(): PriorityOption[] {
  return PRIORITY_LEVELS.map((level) => ({ level, label: PRIORITY_LABELS[level] }));
}
