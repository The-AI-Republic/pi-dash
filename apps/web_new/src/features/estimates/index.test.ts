// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public API of the estimates feature: the settings entry imports only
// through this barrel, so every export the surface needs must resolve.
import { describe, expect, it } from "vitest";

import * as estimates from "./index.js";

describe("estimates feature barrel", () => {
  it("exports the screen, dialog, hooks and model helpers", () => {
    expect(typeof estimates.EstimatesScreen).toBe("function");
    expect(typeof estimates.EstimateCreateDialog).toBe("function");
    expect(typeof estimates.useProjectEstimates).toBe("function");
    expect(typeof estimates.useCreateEstimate).toBe("function");
    expect(typeof estimates.useDeleteEstimate).toBe("function");
    expect(typeof estimates.useToggleEstimates).toBe("function");
    expect(typeof estimates.projectEstimatesQueryOptions).toBe("function");
    expect(typeof estimates.selectActiveSystem).toBe("function");
    expect(typeof estimates.valueSetProblems).toBe("function");
    expect(estimates.estimatesKeys.project("acme", "p1")).toEqual(["ws", "acme", "estimates", "p1"]);
    expect(estimates.ESTIMATE_CREATED_TITLE).toBe("Estimate created");
    expect(estimates.ESTIMATE_ENABLED_TITLE).toBe("Estimates enabled");
    expect(estimates.ESTIMATE_TOGGLE_FAILED_BODY).toBe("The project’s estimate setting is unchanged.");
    expect(estimates.ESTIMATE_DELETE_FAILED_BODY).toBe(
      "The delete did not finish. Reload the page to see whether the system is still listed."
    );
    expect(estimates.ESTIMATE_DISABLED_BODY).toBe("Work items no longer offer estimate values.");
    expect(estimates.ESTIMATE_CREATE_FAILED_TITLE).toContain("creation failed");
    expect(estimates.ESTIMATE_DELETED_BODY).toBe("The system and its values were removed.");
    expect(estimates.ESTIMATE_FAMILY_POINTS).toBe("points");
    expect(estimates.ESTIMATE_FAMILY_CATEGORIES).toBe("categories");
    expect(estimates.CUSTOM_STARTER_ID).toBe("custom");
    expect(estimates.ESTIMATE_VALUE_MAX_LENGTH).toBe(20);
    expect(estimates.ESTIMATE_FAMILIES).toHaveLength(2);
    expect(estimates.familySystemName("categories")).toBe("Categories");
    expect(estimates.renumberValues(["1", "2"])).toEqual([
      { key: 1, value: "1" },
      { key: 2, value: "2" },
    ]);
    expect(estimates.estimateStarters("points")[0]?.id).toBe(estimates.CUSTOM_STARTER_ID);
    expect(estimates.starterValues("points", "missing")).toBeUndefined();
    expect(
      estimates.systemValueLabels({ points: [{ id: "a", key: 1, value: "1", description: "" }] } as never)
    ).toEqual(["1"]);
    expect(estimates.selectSupersededSystems([])).toEqual([]);
  });
});
