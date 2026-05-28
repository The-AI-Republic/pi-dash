/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { describe, expect, it } from "vitest";

import { powerKCommandFilter } from "@/components/power-k/ui/modal/filter";

describe("powerKCommandFilter", () => {
  it("keeps server-matched search results when the query only matches keywords", () => {
    expect(powerKCommandFilter("issue-de4163f5-front end error-PIDASH-1", "38 same errors", ["38 same errors"])).toBe(
      1
    );
  });

  it("keeps server-matched search results when the query has surrounding whitespace", () => {
    expect(powerKCommandFilter("issue-de4163f5-front end error-PIDASH-1", " 38 same errors ", ["38 same errors"])).toBe(
      1
    );
  });

  it("still filters ordinary command values", () => {
    expect(powerKCommandFilter("open-project", "project")).toBe(1);
    expect(powerKCommandFilter("open-project", "38 same errors")).toBe(0);
  });

  it("always keeps the no-results command", () => {
    expect(powerKCommandFilter("no-results", "anything")).toBe(1);
  });
});
