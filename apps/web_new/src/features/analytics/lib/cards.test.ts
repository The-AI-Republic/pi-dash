// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Analytics headline card tests (parity/specs/analytics.md §§5, 8, ANL-005, ANL-008).
import { describe, expect, it } from "vitest";

import {
  OVERVIEW_HEADLINE_CARDS,
  WORK_ITEMS_HEADLINE_CARDS,
  headlineCardsForTab,
  selectHeadlineValue,
} from "./cards.js";

describe("headline catalogues", () => {
  it("lists the eleven overview cards in display order", () => {
    expect(OVERVIEW_HEADLINE_CARDS.map((card) => card.label)).toEqual([
      "Total Users",
      "Total Admins",
      "Total Members",
      "Total Guests",
      "Total Projects",
      "Total Work items",
      "Total Cycles",
      "Total Intake",
      "Agent input tokens",
      "Agent output tokens",
      "Agent total tokens",
    ]);
    expect(OVERVIEW_HEADLINE_CARDS.map((card) => card.key)).toEqual([
      "total_users",
      "total_admins",
      "total_members",
      "total_guests",
      "total_projects",
      "total_work_items",
      "total_cycles",
      "total_intake",
      "agent_run_input_tokens",
      "agent_run_output_tokens",
      "agent_run_total_tokens",
    ]);
  });

  it("lists the five work-items cards in display order", () => {
    expect(WORK_ITEMS_HEADLINE_CARDS.map((card) => card.label)).toEqual([
      "Total Work items",
      "Started Work items",
      "Backlog Work items",
      "Unstarted Work items",
      "Completed Work items",
    ]);
    expect(WORK_ITEMS_HEADLINE_CARDS.map((card) => card.key)).toEqual([
      "total_work_items",
      "started_work_items",
      "backlog_work_items",
      "un_started_work_items",
      "completed_work_items",
    ]);
  });

  it("hands each tab its own grid", () => {
    expect(headlineCardsForTab("overview")).toBe(OVERVIEW_HEADLINE_CARDS);
    expect(headlineCardsForTab("work-items")).toBe(WORK_ITEMS_HEADLINE_CARDS);
  });
});

describe("selectHeadlineValue", () => {
  it("reads the metric count", () => {
    expect(selectHeadlineValue({ total_users: { count: 3 } }, "total_users")).toBe(3);
  });

  it("reads zero for missing or malformed payloads", () => {
    expect(selectHeadlineValue(undefined, "total_users")).toBe(0);
    expect(selectHeadlineValue(null, "total_users")).toBe(0);
    expect(selectHeadlineValue("totals", "total_users")).toBe(0);
    expect(selectHeadlineValue({}, "total_users")).toBe(0);
    expect(selectHeadlineValue({ total_users: null }, "total_users")).toBe(0);
    expect(selectHeadlineValue({ total_users: 3 }, "total_users")).toBe(0);
    expect(selectHeadlineValue({ total_users: {} }, "total_users")).toBe(0);
    expect(selectHeadlineValue({ total_users: { count: "3" } }, "total_users")).toBe(0);
    expect(selectHeadlineValue({ total_users: { count: Number.NaN } }, "total_users")).toBe(0);
    expect(selectHeadlineValue({ total_users: { count: Number.POSITIVE_INFINITY } }, "total_users")).toBe(0);
  });
});
