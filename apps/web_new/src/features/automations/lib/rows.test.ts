// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";
import type { State } from "@pidash/api-client";

import {
  archiveTogglePatch,
  closeTogglePatch,
  isStatePickerEnabled,
  selectCancelledStates,
  selectCloseTarget,
} from "./rows.js";

function state(id: string, group: State["group"], name = id): State {
  return {
    id,
    project_id: "223e4567-e89b-12d3-a456-426614174001",
    workspace_id: "323e4567-e89b-12d3-a456-426614174002",
    name,
    color: "#6b7280",
    group,
    default: false,
    description: "",
    sequence: 0,
  };
}

const DROPPED = state("123e4567-e89b-12d3-a456-42661417400a", "cancelled", "Dropped");
const WONT_DO = state("123e4567-e89b-12d3-a456-42661417400b", "cancelled", "Won't do");
const TODO = state("123e4567-e89b-12d3-a456-42661417400c", "unstarted", "Todo");

describe("selectCancelledStates", () => {
  it("keeps only the cancelled group in server order", () => {
    expect(selectCancelledStates([TODO, DROPPED, WONT_DO])).toEqual([DROPPED, WONT_DO]);
  });

  it("returns an empty list when nothing is cancelled", () => {
    expect(selectCancelledStates([TODO])).toEqual([]);
  });
});

describe("archiveTogglePatch", () => {
  it("writes one month when switched on and zero when switched off", () => {
    expect(archiveTogglePatch(true)).toEqual({ archive_in: 1 });
    expect(archiveTogglePatch(false)).toEqual({ archive_in: 0 });
  });
});

describe("closeTogglePatch", () => {
  it("writes a one-month delay toward the first cancelled state", () => {
    expect(closeTogglePatch(true, [DROPPED, WONT_DO])).toEqual({ close_in: 1, default_state: DROPPED.id });
  });

  it("writes a null target when nothing is cancelled", () => {
    expect(closeTogglePatch(true, [])).toEqual({ close_in: 1, default_state: null });
  });

  it("clears both fields when switched off", () => {
    expect(closeTogglePatch(false, [DROPPED])).toEqual({ close_in: 0, default_state: null });
  });
});

describe("selectCloseTarget", () => {
  it("shows the stored state when it is cancelled", () => {
    expect(selectCloseTarget({ default_state: WONT_DO.id }, [DROPPED, WONT_DO])).toEqual(WONT_DO);
  });

  it("falls back to the first cancelled state for an unknown or missing stored target", () => {
    expect(selectCloseTarget(undefined, [DROPPED])).toEqual(DROPPED);
    expect(selectCloseTarget({ default_state: TODO.id }, [DROPPED])).toEqual(DROPPED);
  });

  it("resolves null when the project has no cancelled state", () => {
    expect(selectCloseTarget({ default_state: DROPPED.id }, [])).toBeNull();
  });
});

describe("isStatePickerEnabled", () => {
  it("stays disabled until a second cancelled state exists", () => {
    expect(isStatePickerEnabled([])).toBe(false);
    expect(isStatePickerEnabled([DROPPED])).toBe(false);
    expect(isStatePickerEnabled([DROPPED, WONT_DO])).toBe(true);
  });
});
