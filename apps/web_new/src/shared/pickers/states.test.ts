// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { STATE_GROUPS } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import { selectStateOptions, STATE_GROUP_ICONS } from "./states.js";

describe("STATE_GROUP_ICONS", () => {
  it("covers every workflow group", () => {
    expect(Object.keys(STATE_GROUP_ICONS).sort()).toEqual([...STATE_GROUPS].sort());
    for (const icon of Object.values(STATE_GROUP_ICONS)) {
      expect(icon.displayName ?? icon.name).toBeDefined();
    }
  });
});

describe("selectStateOptions", () => {
  it("keeps server order with group and color", () => {
    const options = selectStateOptions([
      {
        id: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
        project_id: "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
        workspace_id: "cccccccc-cccc-4ccc-8ccc-cccccccccccc",
        name: "Todo",
        color: "#5e6ad2",
        group: "unstarted",
        default: true,
        description: "",
        sequence: 1,
      },
      {
        id: "dddddddd-dddd-4ddd-8ddd-dddddddddddd",
        project_id: "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
        workspace_id: "cccccccc-cccc-4ccc-8ccc-cccccccccccc",
        name: "Done",
        color: "#2f9e5b",
        group: "completed",
        default: false,
        description: "",
        sequence: 2,
      },
    ]);
    expect(options).toEqual([
      { id: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa", name: "Todo", group: "unstarted", color: "#5e6ad2" },
      { id: "dddddddd-dddd-4ddd-8ddd-dddddddddddd", name: "Done", group: "completed", color: "#2f9e5b" },
    ]);
  });
});
