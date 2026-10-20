// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import {
  isFeatureEnabled,
  nextFeatureValue,
  PROJECT_FEATURES,
  PROJECT_FEATURE_KEYS,
  PROJECT_FEATURE_LIST,
} from "./registry.js";

describe("project feature registry", () => {
  it("lists exactly the five switches with distinct flags and routes", () => {
    expect(PROJECT_FEATURE_LIST.map((feature) => feature.key)).toEqual([...PROJECT_FEATURE_KEYS]);
    expect(new Set(PROJECT_FEATURE_LIST.map((feature) => feature.flag)).size).toBe(5);
    expect(new Set(PROJECT_FEATURE_LIST.map((feature) => feature.suffix)).size).toBe(5);
    expect(new Set(PROJECT_FEATURE_LIST.map((feature) => feature.areaSegment)).size).toBe(5);
  });

  it("maps each key onto its own project boolean", () => {
    expect(PROJECT_FEATURES.views.flag).toBe("issue_views_view");
    expect(PROJECT_FEATURES.cycles.flag).toBe("cycle_view");
    expect(PROJECT_FEATURES.intake.flag).toBe("inbox_view");
    expect(PROJECT_FEATURES.modules.flag).toBe("module_view");
    expect(PROJECT_FEATURES.pages.flag).toBe("page_view");
  });

  it("names each definition by its own key and mount suffix", () => {
    for (const key of PROJECT_FEATURE_KEYS) {
      expect(PROJECT_FEATURES[key].key).toBe(key);
      expect(PROJECT_FEATURES[key].suffix).toBe(`/features/${key}`);
    }
  });

  it("reads only a true flag as on", () => {
    const views = PROJECT_FEATURES.views;
    expect(isFeatureEnabled({ issue_views_view: true }, views)).toBe(true);
    expect(isFeatureEnabled({ issue_views_view: false }, views)).toBe(false);
    expect(isFeatureEnabled({}, views)).toBe(false);
    expect(isFeatureEnabled(undefined, views)).toBe(false);
  });

  it("flips the stored value and turns an absent flag on", () => {
    const pages = PROJECT_FEATURES.pages;
    expect(nextFeatureValue({ page_view: true }, pages)).toBe(false);
    expect(nextFeatureValue({ page_view: false }, pages)).toBe(true);
    expect(nextFeatureValue({}, pages)).toBe(true);
  });
});
