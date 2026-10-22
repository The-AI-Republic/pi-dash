// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { FieldConfigRegistry, type RichCondition } from "../../issues/index.js";
import { readRegistryExpression } from "./registryExpression.js";

const CONDITION: RichCondition = {
  key: "a",
  property: "priority",
  operator: "in",
  negated: false,
  value: "urgent",
};

describe("readRegistryExpression", () => {
  it("reads the empty expression from an untouched registry", () => {
    expect(readRegistryExpression(new FieldConfigRegistry())).toEqual({});
  });

  it("serializes the held conditions", () => {
    const registry = new FieldConfigRegistry();
    registry.register("priority", [CONDITION]);
    expect(readRegistryExpression(registry)).toEqual({ priority__in: "urgent" });
  });

  it("drops half-built conditions the serializer rejects", () => {
    const registry = new FieldConfigRegistry();
    registry.register("priority", [{ ...CONDITION, value: null }]);
    expect(readRegistryExpression(registry)).toEqual({});
  });
});
