// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { priorityDisplayLabel, priorityTone } from "./priority.js";

describe("priorityDisplayLabel", () => {
  it("capitalizes every known enum value", () => {
    expect(priorityDisplayLabel("urgent")).toBe("Urgent");
    expect(priorityDisplayLabel("high")).toBe("High");
    expect(priorityDisplayLabel("medium")).toBe("Medium");
    expect(priorityDisplayLabel("low")).toBe("Low");
    expect(priorityDisplayLabel("none")).toBe("None");
  });

  it("resolves missing and unknown values to null", () => {
    expect(priorityDisplayLabel(null)).toBeNull();
    expect(priorityDisplayLabel(undefined)).toBeNull();
    expect(priorityDisplayLabel("")).toBeNull();
    expect(priorityDisplayLabel("critical")).toBeNull();
  });
});

describe("priorityTone", () => {
  it("grades tones from danger down to neutral", () => {
    expect(priorityTone("urgent")).toBe("danger");
    expect(priorityTone("high")).toBe("warning");
    expect(priorityTone("medium")).toBe("accent");
    expect(priorityTone("low")).toBe("neutral");
    expect(priorityTone("none")).toBe("neutral");
  });

  it("falls back to neutral for unknown values", () => {
    expect(priorityTone("critical")).toBe("neutral");
  });
});
