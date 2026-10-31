// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import {
  PRIORITY_ICONS,
  PRIORITY_LABELS,
  PRIORITY_LEVELS,
  PRIORITY_TONES,
  selectPriorityOptions,
} from "./priorities.js";

describe("priority options", () => {
  it("covers the five levels in order with labels, glyphs and tones", () => {
    expect(PRIORITY_LEVELS).toEqual(["urgent", "high", "medium", "low", "none"]);
    expect(selectPriorityOptions().map((option) => option.label)).toEqual(["Urgent", "High", "Medium", "Low", "None"]);
    for (const level of PRIORITY_LEVELS) {
      expect(PRIORITY_LABELS[level].length).toBeGreaterThan(0);
      expect(PRIORITY_ICONS[level].displayName ?? PRIORITY_ICONS[level].name).toBeDefined();
      expect(PRIORITY_TONES[level].length).toBeGreaterThan(0);
    }
  });

  it("uses the alert for urgent and the barred circle for none", () => {
    expect(PRIORITY_ICONS.urgent.displayName ?? PRIORITY_ICONS.urgent.name).toBe("CircleAlert");
    expect(PRIORITY_ICONS.none.displayName ?? PRIORITY_ICONS.none.name).toBe("Ban");
    expect(PRIORITY_ICONS.high.displayName ?? PRIORITY_ICONS.high.name).toBe("SignalHigh");
    expect(PRIORITY_ICONS.medium.displayName ?? PRIORITY_ICONS.medium.name).toBe("SignalMedium");
    expect(PRIORITY_ICONS.low.displayName ?? PRIORITY_ICONS.low.name).toBe("SignalLow");
  });
});
