// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Duplicate seam (parity/specs/issues.md §11 ISS-121, §23): the OSS
// edition answers none, so no button or panel can mount, and the length
// guard never spends a search on a very short title.

// @vitest-environment jsdom
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it } from "vitest";

import type { IssueSearchHit } from "@pidash/api-client";

import {
  DEDUPE_DEBOUNCE_MS,
  DuplicatePanel,
  ISSUE_DEDUPE_FLAG,
  shouldSearchDuplicates,
  useDuplicateMatches,
} from "./dedupe.js";

function hit(id: string, name: string): IssueSearchHit {
  return {
    id,
    name,
    start_date: null,
    sequence_id: 1,
    project__name: "Atlas",
    project__identifier: "ATL",
    project_id: "p",
    workspace__slug: "acme",
    state__name: "Todo",
    state__group: "unstarted",
    state__color: "#5e6ad2",
  };
}

describe("useDuplicateMatches", () => {
  it("returns no matches and keeps the surface off in the community build", () => {
    const result = useDuplicateMatches({ workspaceSlug: "acme", projectId: "p", title: "Fix login", enabled: true });
    expect(result).toEqual({ matches: [], isLoading: false, isEnabled: false });
  });

  it("stays off even with the edition flag turned on (the hook is the seam)", () => {
    const result = useDuplicateMatches({ workspaceSlug: "acme", projectId: "p", title: "Fix login", enabled: true });
    expect(result.matches).toHaveLength(0);
    expect(result.isEnabled).toBe(false);
  });

  it("exposes the stable flag the cloud build replaces behind", () => {
    expect(ISSUE_DEDUPE_FLAG).toBe("issue-dedupe");
    expect(DEDUPE_DEBOUNCE_MS).toBeGreaterThan(0);
  });
});

describe("DuplicatePanel", () => {
  it("renders nothing without matches", () => {
    const { container } = render(React.createElement(DuplicatePanel, { matches: [] }));
    expect(container).toBeEmptyDOMElement();
  });

  it("counts one match in the singular and lists it", () => {
    render(React.createElement(DuplicatePanel, { matches: [hit("1", "Fix login")] }));
    expect(screen.getByText("1 possible duplicate")).toBeInTheDocument();
    expect(screen.getByText("Fix login")).toBeInTheDocument();
    expect(screen.getByRole("complementary", { name: "Possible duplicates" })).toBeInTheDocument();
  });

  it("pluralizes the count above two matches", () => {
    render(React.createElement(DuplicatePanel, { matches: [hit("1", "Fix login"), hit("2", "Fix logout")] }));
    expect(screen.getByText("2 possible duplicates")).toBeInTheDocument();
    expect(screen.getByText("Fix logout")).toBeInTheDocument();
  });
});

describe("shouldSearchDuplicates", () => {
  it("needs the flag and a title of some substance", () => {
    expect(shouldSearchDuplicates("Fix login", true)).toBe(true);
    expect(shouldSearchDuplicates("Fix login", false)).toBe(false);
    expect(shouldSearchDuplicates("  ab  ", true)).toBe(false);
    expect(shouldSearchDuplicates("   ", true)).toBe(false);
  });
});
