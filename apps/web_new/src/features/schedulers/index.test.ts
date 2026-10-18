// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import {
  SchedulerCatalog,
  SchedulerDefinitionDialog,
  SchedulerDeleteDialog,
  deriveSchedulerHandle,
  schedulerKeys,
  schedulersQueryOptions,
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
});
