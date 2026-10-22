// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { loadRichFilterBuilder, preloadRichFilterBuilder } from "./index.js";

describe("rich filter builder loader", () => {
  it("loads the builder module once and reuses the request", async () => {
    const first = await loadRichFilterBuilder();
    expect(typeof first.useRichFilterBuilder).toBe("function");
    expect(typeof first.serializeRichExpression).toBe("function");
    expect(typeof first.FieldConfigRegistry).toBe("function");
    expect(typeof first.buildViewCreate).toBe("function");
    expect(typeof first.savedViewKeys.project).toBe("function");
    const second = await loadRichFilterBuilder();
    expect(second).toBe(first);
  });

  it("preloads without throwing", () => {
    expect(() => preloadRichFilterBuilder()).not.toThrow();
  });
});
