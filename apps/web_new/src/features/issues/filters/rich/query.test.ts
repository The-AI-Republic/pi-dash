// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { richFiltersParam, withRichFilters } from "./query.js";

describe("richFiltersParam", () => {
  it("omits an empty expression", () => {
    expect(richFiltersParam({})).toBeNull();
  });

  it("serializes leaves, AND and negation", () => {
    expect(richFiltersParam({ priority__in: "urgent" })).toBe('{"priority__in":"urgent"}');
    expect(richFiltersParam({ and: [{ priority__in: "urgent" }, { not: { state_id__in: ["a"] } }] })).toBe(
      '{"and":[{"priority__in":"urgent"},{"not":{"state_id__in":["a"]}}]}'
    );
  });
});

describe("withRichFilters", () => {
  it("leaves the query untouched for an empty expression", () => {
    const query = { perPage: 50 };
    expect(withRichFilters(query, {})).toBe(query);
  });

  it("adds the filters parameter beside existing ones", () => {
    expect(withRichFilters({ perPage: 50, filters: { state: "todo" } }, { priority__in: "urgent" })).toEqual({
      perPage: 50,
      filters: { state: "todo", filters: '{"priority__in":"urgent"}' },
    });
  });
});
