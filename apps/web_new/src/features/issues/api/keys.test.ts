// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { issueKeys } from "./keys.js";

describe("issueKeys", () => {
  it("nests the comment list under the workspace issue scope", () => {
    expect(issueKeys.comments("acme", "project-1", "issue-1")).toEqual([
      "ws",
      "acme",
      "issues",
      "comments",
      "project-1",
      "issue-1",
    ]);
  });

  it("keeps comment lists for different issues apart while sharing one list prefix", () => {
    const first = issueKeys.comments("acme", "project-1", "issue-1");
    const second = issueKeys.comments("acme", "project-1", "issue-2");

    expect(first).not.toEqual(second);
    expect(first.slice(0, -1)).toEqual(second.slice(0, -1));
  });
});
