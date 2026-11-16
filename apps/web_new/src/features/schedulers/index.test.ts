// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import {
  CalendarMonthView,
  CalendarWeekView,
  FiringCalendar,
  OccurrencePanel,
  SchedulerCatalog,
  SchedulerDefinitionDialog,
  SchedulerDeleteDialog,
  VisibilityRail,
  clampWindow,
  deriveSchedulerHandle,
  formatMonthYear,
  occurrenceKeys,
  planMonthDay,
  railCalendars,
  schedulerKeys,
  schedulersQueryOptions,
  useSchedulerOccurrences,
  useSchedulerVisibility,
  visibilityStorageKey,
} from "./index.js";

describe("scheduler feature public API", () => {
  it("exports the catalog, shared dialogs, data contract, and handle rule", () => {
    expect(SchedulerCatalog).toBeTypeOf("function");
    expect(SchedulerDefinitionDialog).toBeTypeOf("function");
    expect(SchedulerDeleteDialog).toBeTypeOf("function");
    expect(deriveSchedulerHandle("Weekly Report")).toBe("weekly-report");
    expect(schedulerKeys.list("acme")).toEqual(["ws", "acme", "schedulers", "list"]);
    expect(schedulersQueryOptions).toBeTypeOf("function");
  });

  it("exports the calendar, rail and occurrence panel API", () => {
    for (const value of [
      FiringCalendar,
      CalendarWeekView,
      CalendarMonthView,
      VisibilityRail,
      OccurrencePanel,
      useSchedulerOccurrences,
      useSchedulerVisibility,
      clampWindow,
      planMonthDay,
      railCalendars,
      formatMonthYear,
      visibilityStorageKey,
    ]) {
      expect(value).toBeTypeOf("function");
    }
    expect(occurrenceKeys.all("acme", "p1")[2]).toBe("scheduler-occurrences");
  });
});
