// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Freshness invalidates only the affected reads: lists, one cycle's
// reads, archived reads, post-write, and both sides of a transfer
// (CYC-045, NEWFRONT-267 intended).

import { QueryClient } from "@tanstack/react-query";
import { beforeEach, describe, expect, it } from "vitest";

import {
  invalidateAfterCycleTransfer,
  invalidateAfterCycleWrite,
  invalidateArchivedCycleReads,
  invalidateCycleLists,
  invalidateCycleReads,
} from "./invalidate.js";
import { cycleKeys } from "./keys.js";

const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const OTHER_PROJECT = "423e4567-e89b-12d3-a456-426614174003";
const CYCLE = "323e4567-e89b-12d3-a456-426614174002";
const SOURCE = "523e4567-e89b-12d3-a456-426614174004";
const TARGET = "623e4567-e89b-12d3-a456-426614174005";

let queryClient: QueryClient;

beforeEach(() => {
  queryClient = new QueryClient();
  for (const key of [
    cycleKeys.list("acme", PROJECT),
    cycleKeys.list("acme", OTHER_PROJECT),
    cycleKeys.hero("acme", PROJECT),
    cycleKeys.detail("acme", PROJECT, CYCLE),
    cycleKeys.progress("acme", PROJECT, CYCLE),
    cycleKeys.detailedProgress("acme", PROJECT, CYCLE),
    cycleKeys.analytics("acme", PROJECT, CYCLE, "issues"),
    cycleKeys.analytics("acme", PROJECT, CYCLE, "points"),
    cycleKeys.heroIssues("acme", PROJECT, CYCLE),
    cycleKeys.archived("acme", PROJECT),
    cycleKeys.archivedDetail("acme", PROJECT, CYCLE),
    cycleKeys.workspaceCycles("acme"),
  ]) {
    queryClient.setQueryData(key, { seeded: true });
  }
});

function invalidated(key: readonly unknown[]): boolean {
  return queryClient.getQueryState(key)?.isInvalidated === true;
}

describe("invalidateCycleLists", () => {
  it("refreshes the project's list and hero reads only", async () => {
    await invalidateCycleLists(queryClient, "acme", PROJECT);
    expect(invalidated(cycleKeys.list("acme", PROJECT))).toBe(true);
    expect(invalidated(cycleKeys.hero("acme", PROJECT))).toBe(true);
    expect(invalidated(cycleKeys.list("acme", OTHER_PROJECT))).toBe(false);
    expect(invalidated(cycleKeys.detail("acme", PROJECT, CYCLE))).toBe(false);
    expect(invalidated(cycleKeys.archived("acme", PROJECT))).toBe(false);
  });
});

describe("invalidateCycleReads", () => {
  it("refreshes one cycle's detail, progress and analytics reads", async () => {
    await invalidateCycleReads(queryClient, "acme", PROJECT, CYCLE);
    expect(invalidated(cycleKeys.detail("acme", PROJECT, CYCLE))).toBe(true);
    expect(invalidated(cycleKeys.progress("acme", PROJECT, CYCLE))).toBe(true);
    expect(invalidated(cycleKeys.detailedProgress("acme", PROJECT, CYCLE))).toBe(true);
    expect(invalidated(cycleKeys.analytics("acme", PROJECT, CYCLE, "issues"))).toBe(true);
    expect(invalidated(cycleKeys.analytics("acme", PROJECT, CYCLE, "points"))).toBe(true);
    expect(invalidated(cycleKeys.heroIssues("acme", PROJECT, CYCLE))).toBe(true);
    expect(invalidated(cycleKeys.list("acme", PROJECT))).toBe(false);
    expect(invalidated(cycleKeys.archivedDetail("acme", PROJECT, CYCLE))).toBe(false);
    expect(invalidated(cycleKeys.workspaceCycles("acme"))).toBe(false);
  });
});

describe("invalidateArchivedCycleReads", () => {
  it("refreshes the archived list and the peeks nested under it", async () => {
    await invalidateArchivedCycleReads(queryClient, "acme", PROJECT);
    expect(invalidated(cycleKeys.archived("acme", PROJECT))).toBe(true);
    expect(invalidated(cycleKeys.archivedDetail("acme", PROJECT, CYCLE))).toBe(true);
    expect(invalidated(cycleKeys.list("acme", PROJECT))).toBe(false);
    expect(invalidated(cycleKeys.detail("acme", PROJECT, CYCLE))).toBe(false);
  });
});

describe("invalidateAfterCycleWrite", () => {
  it("refreshes lists, the cycle's reads and archived reads", async () => {
    await invalidateAfterCycleWrite(queryClient, "acme", PROJECT, CYCLE);
    expect(invalidated(cycleKeys.list("acme", PROJECT))).toBe(true);
    expect(invalidated(cycleKeys.hero("acme", PROJECT))).toBe(true);
    expect(invalidated(cycleKeys.detail("acme", PROJECT, CYCLE))).toBe(true);
    expect(invalidated(cycleKeys.progress("acme", PROJECT, CYCLE))).toBe(true);
    expect(invalidated(cycleKeys.analytics("acme", PROJECT, CYCLE, "issues"))).toBe(true);
    expect(invalidated(cycleKeys.archived("acme", PROJECT))).toBe(true);
    expect(invalidated(cycleKeys.list("acme", OTHER_PROJECT))).toBe(false);
    expect(invalidated(cycleKeys.workspaceCycles("acme"))).toBe(false);
  });

  it("refreshes the lists only when no cycle id exists yet", async () => {
    await invalidateAfterCycleWrite(queryClient, "acme", PROJECT, null);
    expect(invalidated(cycleKeys.list("acme", PROJECT))).toBe(true);
    expect(invalidated(cycleKeys.hero("acme", PROJECT))).toBe(true);
    expect(invalidated(cycleKeys.detail("acme", PROJECT, CYCLE))).toBe(false);
    expect(invalidated(cycleKeys.archived("acme", PROJECT))).toBe(false);
  });
});

describe("invalidateAfterCycleTransfer", () => {
  it("refreshes the source and the target as well as the lists", async () => {
    for (const id of [SOURCE, TARGET]) {
      queryClient.setQueryData(cycleKeys.detail("acme", PROJECT, id), { seeded: true });
      queryClient.setQueryData(cycleKeys.progress("acme", PROJECT, id), { seeded: true });
    }
    await invalidateAfterCycleTransfer(queryClient, "acme", PROJECT, SOURCE, TARGET);
    expect(invalidated(cycleKeys.list("acme", PROJECT))).toBe(true);
    expect(invalidated(cycleKeys.detail("acme", PROJECT, SOURCE))).toBe(true);
    expect(invalidated(cycleKeys.progress("acme", PROJECT, SOURCE))).toBe(true);
    expect(invalidated(cycleKeys.detail("acme", PROJECT, TARGET))).toBe(true);
    expect(invalidated(cycleKeys.progress("acme", PROJECT, TARGET))).toBe(true);
    expect(invalidated(cycleKeys.detail("acme", PROJECT, CYCLE))).toBe(false);
    expect(invalidated(cycleKeys.archived("acme", PROJECT))).toBe(false);
  });
});
