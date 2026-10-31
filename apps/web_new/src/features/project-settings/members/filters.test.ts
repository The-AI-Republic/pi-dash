// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Per-project session memory for the role filter and sort (PSET-020/021,
// NEWFRONT-409): choices survive a remount, stay separate per project,
// notify subscribers, and forget everything on clear.
import { act, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import { clearMemberFilters, readMemberFilters, useMemberFilters, writeMemberFilters } from "./filters.js";
import { EMPTY_MEMBER_ROSTER_FILTERS, type MemberRosterFilters } from "./roster.js";

const PROJECT_A = "223e4567-e89b-12d3-a456-426614174001";
const PROJECT_B = "223e4567-e89b-12d3-a456-426614174002";

const CHOSEN: MemberRosterFilters = {
  search: "",
  roles: ["member"],
  sort: { column: "display_name", direction: "desc" },
};

afterEach(() => {
  clearMemberFilters();
});

describe("member filter memory", () => {
  it("reads the shared empty filters for an untouched project", () => {
    expect(readMemberFilters(PROJECT_A)).toBe(EMPTY_MEMBER_ROSTER_FILTERS);
  });

  it("remembers the choice by project and keeps projects apart", () => {
    writeMemberFilters(PROJECT_A, CHOSEN);
    expect(readMemberFilters(PROJECT_A)).toEqual(CHOSEN);
    expect(readMemberFilters(PROJECT_B)).toBe(EMPTY_MEMBER_ROSTER_FILTERS);
  });

  it("returns the previous choice to a remounted reader", () => {
    writeMemberFilters(PROJECT_A, CHOSEN);
    const first = renderHook(() => useMemberFilters(PROJECT_A));
    expect(first.result.current[0]).toEqual(CHOSEN);
    first.unmount();
    const second = renderHook(() => useMemberFilters(PROJECT_A));
    expect(second.result.current[0]).toEqual(CHOSEN);
  });

  it("repaints subscribers when the filters change elsewhere", () => {
    const { result } = renderHook(() => useMemberFilters(PROJECT_A));
    act(() => {
      writeMemberFilters(PROJECT_A, CHOSEN);
    });
    expect(result.current[0]).toEqual(CHOSEN);
  });

  it("writes through the hook setter", () => {
    const { result } = renderHook(() => useMemberFilters(PROJECT_A));
    act(() => {
      result.current[1](CHOSEN);
    });
    expect(readMemberFilters(PROJECT_A)).toEqual(CHOSEN);
    expect(result.current[0]).toEqual(CHOSEN);
  });

  it("stops notifying a reader that unmounted", () => {
    const first = renderHook(() => useMemberFilters(PROJECT_A));
    first.unmount();
    // A write after unmount only touches the store; the closure stays quiet.
    expect(() => writeMemberFilters(PROJECT_A, CHOSEN)).not.toThrow();
    expect(readMemberFilters(PROJECT_A)).toEqual(CHOSEN);
  });

  it("keeps the choice when the last reader unsubscribes", () => {
    const first = renderHook(() => useMemberFilters(PROJECT_A));
    act(() => {
      first.result.current[1](CHOSEN);
    });
    first.unmount();
    expect(readMemberFilters(PROJECT_A)).toEqual(CHOSEN);
    const second = renderHook(() => useMemberFilters(PROJECT_A));
    expect(second.result.current[0]).toEqual(CHOSEN);
  });

  it("forgets every project on clear", () => {
    writeMemberFilters(PROJECT_A, CHOSEN);
    writeMemberFilters(PROJECT_B, CHOSEN);
    clearMemberFilters();
    expect(readMemberFilters(PROJECT_A)).toBe(EMPTY_MEMBER_ROSTER_FILTERS);
    expect(readMemberFilters(PROJECT_B)).toBe(EMPTY_MEMBER_ROSTER_FILTERS);
  });
});
