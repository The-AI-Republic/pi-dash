// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";
import type { StateList } from "@pidash/api-client";
import { selectIntakeFallback } from "./intakeStates.js";

const TRIAGE = {
  id: "123e4567-e89b-12d3-a456-426614174000",
  project_id: "223e4567-e89b-12d3-a456-426614174001",
  workspace_id: "323e4567-e89b-12d3-a456-426614174002",
  name: "Triage",
  color: "#5e6ad2",
  group: "triage",
  default: false,
  description: "",
  sequence: 1,
} as const;

describe("selectIntakeFallback", () => {
  it("falls back to the first state, never the default flag", () => {
    expect(
      selectIntakeFallback([TRIAGE, { ...TRIAGE, id: "423e4567-e89b-12d3-a456-426614174003" }] as StateList)
    ).toEqual({
      id: TRIAGE.id,
      name: "Triage",
      group: "triage",
      color: "#5e6ad2",
    });
    expect(selectIntakeFallback([])).toBe(null);
  });
});
