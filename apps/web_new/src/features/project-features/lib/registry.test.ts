// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import {
  findProjectFeature,
  isFeatureEnabled,
  nextFeatureValue,
  PROJECT_FEATURES,
  PROJECT_FEATURE_KEYS,
} from "./registry.js";

describe("project feature registry", () => {
  it("lists exactly the five switches with distinct flags and routes", () => {
    expect(PROJECT_FEATURES.map((feature) => feature.key)).toEqual([...PROJECT_FEATURE_KEYS]);
    expect(new Set(PROJECT_FEATURES.map((feature) => feature.flag)).size).toBe(5);
    expect(new Set(PROJECT_FEATURES.map((feature) => feature.suffix)).size).toBe(5);
    expect(new Set(PROJECT_FEATURES.map((feature) => feature.areaSegment)).size).toBe(5);
  });

  it("maps each key onto its own project boolean", () => {
    expect(findProjectFeature("views")?.flag).toBe("issue_views_view");
    expect(findProjectFeature("cycles")?.flag).toBe("cycle_view");
    expect(findProjectFeature("intake")?.flag).toBe("inbox_view");
    expect(findProjectFeature("modules")?.flag).toBe("module_view");
    expect(findProjectFeature("pages")?.flag).toBe("page_view");
  });

  it("returns undefined for anything outside the five", () => {
    expect(findProjectFeature("estimates")).toBeUndefined();
    expect(findProjectFeature("")).toBeUndefined();
  });

  it("reads only a true flag as on", () => {
    const views = findProjectFeature("views");
    if (views === undefined) throw new Error("missing views feature");
    expect(isFeatureEnabled({ issue_views_view: true }, views)).toBe(true);
    expect(isFeatureEnabled({ issue_views_view: false }, views)).toBe(false);
    expect(isFeatureEnabled({}, views)).toBe(false);
    expect(isFeatureEnabled(undefined, views)).toBe(false);
  });

  it("flips the stored value and turns an absent flag on", () => {
    const pages = findProjectFeature("pages");
    if (pages === undefined) throw new Error("missing pages feature");
    expect(nextFeatureValue({ page_view: true }, pages)).toBe(false);
    expect(nextFeatureValue({ page_view: false }, pages)).toBe(true);
    expect(nextFeatureValue({}, pages)).toBe(true);
  });
});
