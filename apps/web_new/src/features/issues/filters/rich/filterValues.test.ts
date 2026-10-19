// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import type { MemberOption } from "../../../../shared/pickers/index.js";
import {
  cycleFilterRows,
  orderAppliedByName,
  orderLabelOptions,
  orderMemberOptions,
  selectedIds,
} from "./filterValues.js";

const ME = "11111111-1111-4111-8111-111111111111";
const ANA = "22222222-2222-4222-8222-222222222222";
const ZOE = "33333333-3333-4333-8333-333333333333";

function member(userId: string, name: string, selected = false): MemberOption {
  return {
    userId,
    label: userId === ME ? "You" : name,
    searchText: name,
    avatarUrl: null,
    suspended: false,
    selected,
  };
}

function label(id: string, name: string, color = "#ff0000") {
  return {
    id,
    name,
    color,
    project_id: "p",
    workspace_id: "w",
    parent: null,
    sort_order: 1,
  };
}

describe("selectedIds", () => {
  it("reads arrays and ignores non-strings and empties", () => {
    expect(selectedIds(["a", 1, "b"])).toEqual(["a", "b"]);
    expect(selectedIds("a")).toEqual(["a"]);
    expect(selectedIds("")).toEqual([]);
    expect(selectedIds(null)).toEqual([]);
    expect(selectedIds({})).toEqual([]);
  });
});

describe("orderMemberOptions", () => {
  it("puts applied rows first, then the signed-in member, then alphabetical", () => {
    const options = [member(ZOE, "Zoe"), member(ANA, "Ana"), member(ME, "Parity User")];
    expect(orderMemberOptions(options, [ZOE], ME).map((option) => option.userId)).toEqual([ZOE, ME, ANA]);
  });

  it("sorts alphabetically without a signed-in member", () => {
    const options = [member(ZOE, "Zoe"), member(ANA, "Ana")];
    expect(orderMemberOptions(options, [], null).map((option) => option.searchText)).toEqual(["Ana", "Zoe"]);
  });

  it("keeps applied rows in directory order when both are applied", () => {
    const options = [member(ZOE, "Zoe"), member(ANA, "Ana")];
    expect(orderMemberOptions(options, [ZOE, ANA], null).map((option) => option.userId)).toEqual([ANA, ZOE]);
  });
});

describe("orderLabelOptions", () => {
  it("puts applied labels first then sorts by name case-insensitively", () => {
    const labels = [label("1", "zeta"), label("2", "Alpha"), label("3", "beta")];
    expect(orderLabelOptions(labels, ["1"]).map((entry) => entry.id)).toEqual(["1", "2", "3"]);
    expect(orderLabelOptions(labels, []).map((entry) => entry.name)).toEqual(["Alpha", "beta", "zeta"]);
  });

  it("sorts applied labels among themselves and leaves the rest after them", () => {
    const labels = [label("1", "zeta"), label("2", "Alpha"), label("3", "beta")];
    expect(orderLabelOptions(labels, ["1", "2"]).map((entry) => entry.name)).toEqual(["Alpha", "zeta", "beta"]);
    expect(orderLabelOptions(labels, ["3", "1"]).map((entry) => entry.name)).toEqual(["beta", "zeta", "Alpha"]);
  });
});

describe("orderAppliedByName", () => {
  it("puts applied rows first and sorts both halves by name", () => {
    const rows = [
      { id: "3", name: "zeta" },
      { id: "1", name: "Beta" },
      { id: "2", name: "alpha" },
    ];
    expect(orderAppliedByName(rows, ["3", "2"]).map((row) => row.id)).toEqual(["2", "3", "1"]);
  });

  it("keeps incoming order for equal names and leaves the input untouched", () => {
    const rows = [
      { id: "b", name: "Same" },
      { id: "a", name: "same" },
    ];
    expect(orderAppliedByName(rows, []).map((row) => row.id)).toEqual(["b", "a"]);
    expect(rows.map((row) => row.id)).toEqual(["b", "a"]);
  });

  it("orders with no selection alphabetically", () => {
    expect(
      orderAppliedByName(
        [
          { id: "c", name: "Sprint 10" },
          { id: "m", name: "Billing" },
        ],
        []
      ).map((row) => row.name)
    ).toEqual(["Billing", "Sprint 10"]);
  });
});

describe("cycleFilterRows", () => {
  it("keeps every project cycle, finished ones included, as id and name", () => {
    const cycles = [
      { id: "c1", name: "Sprint 1", status: "COMPLETED", extra: true },
      { id: "c2", name: "Sprint 2", status: "ACTIVE" },
    ];
    expect(cycleFilterRows(cycles)).toEqual([
      { id: "c1", name: "Sprint 1" },
      { id: "c2", name: "Sprint 2" },
    ]);
    expect(cycleFilterRows([])).toEqual([]);
  });
});
