// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// BYOK model-ID list rules (NEWPIDASH-394, design sections 4.4, 7.2 and
// 7.3): normalization, display status, failure details, "Save anyway".
import { describe, expect, it } from "vitest";

import type { ModelTestResults } from "@pidash/api-client";

import { modelFailureDetails, modelIdStatus, normalizeModelIds, saveAnywayModels } from "./byokModels.js";

function results(models: ModelTestResults["models"]): ModelTestResults {
  return { models };
}

describe("normalizeModelIds", () => {
  it("trims, drops empties and duplicates, keeps the typed order", () => {
    expect(normalizeModelIds(["  b ", "", "a", "b", "  ", "a", "c"])).toEqual(["b", "a", "c"]);
  });

  it("reads an empty list as empty", () => {
    expect(normalizeModelIds([])).toEqual([]);
  });
});

describe("modelIdStatus", () => {
  it("reads a passing stamp as verified even with a stale failure", () => {
    expect(modelIdStatus("m", { m: "2026-11-23T00:00:00Z" }, { m: "gone" })).toBe("verified");
  });

  it("reads a failure message as failed", () => {
    expect(modelIdStatus("m", {}, { m: "Model not accepted by the provider." })).toBe("failed");
  });

  it("reads an untested ID as unverified", () => {
    expect(modelIdStatus("m", { other: "2026-11-23T00:00:00Z" }, {})).toBe("unverified");
  });
});

describe("modelFailureDetails", () => {
  it("maps failed and unverified entries to their messages, skipping verified ones", () => {
    expect(
      modelFailureDetails(
        results([
          { model: "good", status: "verified" },
          { model: "bad", status: "failed", code: "model_invalid", detail: "Model not accepted." },
          { model: "flaky", status: "unverified", code: "provider_unreachable" },
        ])
      )
    ).toEqual({ bad: "Model not accepted.", flaky: "provider_unreachable" });
  });
});

describe("saveAnywayModels", () => {
  it("offers the unverified IDs when nothing failed", () => {
    expect(
      saveAnywayModels(
        results([
          { model: "good", status: "verified" },
          { model: "flaky", status: "unverified", code: "provider_unreachable" },
        ])
      )
    ).toEqual(["flaky"]);
  });

  it("refuses the retry while any ID failed", () => {
    expect(
      saveAnywayModels(
        results([
          { model: "bad", status: "failed", code: "model_invalid" },
          { model: "flaky", status: "unverified", code: "provider_unreachable" },
        ])
      )
    ).toBeNull();
  });

  it("offers nothing when every ID verified", () => {
    expect(saveAnywayModels(results([{ model: "good", status: "verified" }]))).toBeNull();
  });
});
