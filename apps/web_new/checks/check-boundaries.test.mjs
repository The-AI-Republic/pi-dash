// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Self-test for the feature-to-feature barrel rule (NEWFRONT-498). The
// barrel lives at index.ts on disk but is imported as index.js per the
// repo's ESM style; both spellings name the same file, so both pass.
// Exercises the pure crossFeatureFailure helper directly.
import { describe, expect, it } from "vitest";
import { crossFeatureFailure } from "./check-boundaries.mjs";

const FROM = "features/search/panel.ts";

describe("crossFeatureFailure", () => {
  it("passes the .js barrel the codebase actually imports", () => {
    expect(crossFeatureFailure(FROM, "features/issues/index.js")).toBeNull();
  });

  it("passes the .js barrel from a nested importer file", () => {
    expect(crossFeatureFailure("features/search/nested/panel.ts", "features/issues/index.js")).toBeNull();
  });

  it("still passes the .ts spelling", () => {
    expect(crossFeatureFailure(FROM, "features/issues/index.ts")).toBeNull();
  });

  it("fails deep cross-feature imports bypassing the barrel", () => {
    for (const target of [
      "features/issues/store.js",
      "features/issues/store.ts",
      "features/issues/store",
      "features/issues/lib/store.js",
    ]) {
      expect(crossFeatureFailure(FROM, target)).toBe("cross-feature import must go through features/issues/index.ts");
    }
  });

  it("fails a bare directory import that never names the barrel file", () => {
    expect(crossFeatureFailure(FROM, "features/issues")).toBe(
      "cross-feature import must go through features/issues/index.ts"
    );
  });

  it("passes imports within the same feature", () => {
    expect(crossFeatureFailure("features/issues/panel.ts", "features/issues/store.js")).toBeNull();
  });

  it("passes targets with no feature segment", () => {
    expect(crossFeatureFailure(FROM, "features")).toBeNull();
  });
});
