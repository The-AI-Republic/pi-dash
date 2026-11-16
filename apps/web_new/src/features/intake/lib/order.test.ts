// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import {
  DEFAULT_INTAKE_ORDER_VALUE,
  INTAKE_ORDER_DIRECTION_LABELS,
  INTAKE_ORDER_DIRECTIONS,
  INTAKE_ORDER_FIELD_LABELS,
  INTAKE_ORDER_FIELDS,
  orderByForOrder,
} from "./order.js";

describe("intake ordering (INT-011)", () => {
  it("defaults to created date, newest first", () => {
    expect(DEFAULT_INTAKE_ORDER_VALUE).toEqual({ field: "created", direction: "desc" });
    expect(orderByForOrder(DEFAULT_INTAKE_ORDER_VALUE)).toBe("-issue__created_at");
  });

  it("menus three fields and two directions in order", () => {
    expect([...INTAKE_ORDER_FIELDS]).toEqual(["created", "updated", "id"]);
    expect([...INTAKE_ORDER_DIRECTIONS]).toEqual(["desc", "asc"]);
    expect(INTAKE_ORDER_FIELD_LABELS).toEqual({ created: "Created date", updated: "Last updated date", id: "ID" });
    expect(INTAKE_ORDER_DIRECTION_LABELS).toEqual({ asc: "Ascending", desc: "Descending" });
  });

  it("encodes every field and direction server-side", () => {
    expect(orderByForOrder({ field: "created", direction: "asc" })).toBe("issue__created_at");
    expect(orderByForOrder({ field: "updated", direction: "desc" })).toBe("-issue__updated_at");
    expect(orderByForOrder({ field: "updated", direction: "asc" })).toBe("issue__updated_at");
    expect(orderByForOrder({ field: "id", direction: "desc" })).toBe("-issue__sequence_id");
    expect(orderByForOrder({ field: "id", direction: "asc" })).toBe("issue__sequence_id");
  });
});
