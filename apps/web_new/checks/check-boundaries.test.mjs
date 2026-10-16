// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Self-test for the feature-to-feature barrel rule (NEWFRONT-498). The
// barrel lives at index.ts on disk but is imported as index.js per the
// repo's ESM style; both spellings name the same file, so both pass.
// Exercises the pure crossFeatureFailure helper directly.
import { describe, expect, it } from "vitest";
import { crossFeatureFailure, entriesCatalogFailure } from "./check-boundaries.mjs";

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

describe("entriesCatalogFailure", () => {
  const MESSAGE =
    "shared/shell/entries/* must not be statically imported outside entries/, the shell barrel and UpdatesDialog";

  it("fails eager shell components importing a catalog (the NEWFRONT-287 split collapse)", () => {
    expect(entriesCatalogFailure("shared/shell/HelpMenu.tsx", "shared/shell/entries/account.js")).toBe(MESSAGE);
  });

  it("fails routes importing a catalog directly", () => {
    expect(entriesCatalogFailure("routes/$ws/_gated/route.tsx", "shared/shell/entries/account.js")).toBe(MESSAGE);
  });

  it("passes catalog-internal imports, the shell barrel and the lazy updates dialog", () => {
    expect(
      entriesCatalogFailure("shared/shell/entries/WorkspaceEntries.tsx", "shared/shell/entries/account.js")
    ).toBeNull();
    expect(entriesCatalogFailure("shared/shell/index.ts", "shared/shell/entries/account.js")).toBeNull();
    expect(entriesCatalogFailure("shared/shell/index.js", "shared/shell/entries/account.js")).toBeNull();
    expect(entriesCatalogFailure("shared/shell/UpdatesDialog.tsx", "shared/shell/entries/account.js")).toBeNull();
  });

  it("passes non-catalog targets", () => {
    expect(entriesCatalogFailure("shared/shell/HelpMenu.tsx", "shared/shell/externalLinks.js")).toBeNull();
    expect(entriesCatalogFailure("shared/shell/HelpMenu.tsx", "shared/shell/chromeState.js")).toBeNull();
  });
});
