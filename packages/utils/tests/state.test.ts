/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { describe, expect, it } from "vitest";

import type { IState, TStateGroups } from "@pi-dash/types";

import { orderStateGroups, sortStates } from "../src/work-item/state";

const makeState = (group: TStateGroups, sequence: number): IState => ({
  id: `${group}-${sequence}`,
  color: "#000000",
  default: false,
  description: "",
  group,
  name: group,
  project_id: "project-id",
  sequence,
  workspace_id: "workspace-id",
  order: sequence,
});

describe("state-group ordering", () => {
  it("creates an empty Test bucket in lifecycle order", () => {
    const ordered = orderStateGroups({ completed: [makeState("completed", 1)] });

    expect(Object.keys(ordered ?? {})).toEqual([
      "backlog",
      "unstarted",
      "started",
      "review",
      "test",
      "completed",
      "cancelled",
    ]);
    expect(ordered?.test).toEqual([]);
  });

  it("sorts Test between Review and Completed without mutating the input", () => {
    const states = [makeState("completed", 1), makeState("test", 2), makeState("review", 3), makeState("test", 1)];

    const sorted = sortStates(states);

    expect(sorted?.map((state) => `${state.group}:${state.sequence}`)).toEqual([
      "review:3",
      "test:1",
      "test:2",
      "completed:1",
    ]);
    expect(states.map((state) => `${state.group}:${state.sequence}`)).toEqual([
      "completed:1",
      "test:2",
      "review:3",
      "test:1",
    ]);
  });
});
