// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import type { RichCondition } from "../../issues/index.js";
// Test-only deep imports (checks skip test files): the registry class
// and the serializer the dialog receives from the lazily loaded
// builder module.
import { FieldConfigRegistry } from "../../issues/filters/rich/temporary.js";
import { serializeRichExpression } from "../../issues/filters/rich/expression.js";
import { readRegistryExpression } from "./registryExpression.js";

const CONDITION: RichCondition = {
  key: "a",
  property: "priority",
  operator: "in",
  negated: false,
  value: "urgent",
};

describe("readRegistryExpression", () => {
  it("reads the empty expression before the builder chunk has loaded", () => {
    expect(readRegistryExpression(null, serializeRichExpression)).toEqual({});
  });

  it("reads the empty expression from an untouched registry", () => {
    expect(readRegistryExpression(new FieldConfigRegistry(), serializeRichExpression)).toEqual({});
  });

  it("serializes the held conditions", () => {
    const registry = new FieldConfigRegistry();
    registry.register("priority", [CONDITION]);
    expect(readRegistryExpression(registry, serializeRichExpression)).toEqual({ priority__in: "urgent" });
  });

  it("drops half-built conditions the serializer rejects", () => {
    const registry = new FieldConfigRegistry();
    registry.register("priority", [{ ...CONDITION, value: null }]);
    expect(readRegistryExpression(registry, serializeRichExpression)).toEqual({});
  });
});
