/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { render } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const { setSearchParamsSpy, routeParams, searchParamsRef, peekIssueRef, setPeekIssue } = vi.hoisted(() => ({
  setSearchParamsSpy: vi.fn(),
  routeParams: { current: {} as Record<string, string> },
  searchParamsRef: { current: new URLSearchParams() },
  peekIssueRef: { current: undefined as undefined | Record<string, unknown> },
  setPeekIssue: vi.fn(),
}));

vi.mock("react-router", () => ({
  useParams: () => routeParams.current,
  useSearchParams: () => [searchParamsRef.current, setSearchParamsSpy],
}));

vi.mock("mobx-react", () => ({
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  observer: (c: any) => c,
}));

vi.mock("@pi-dash/types", () => ({
  EIssueServiceType: { ISSUES: "ISSUES", EPICS: "EPICS" },
  EIssuesStoreType: { ARCHIVED: "ARCHIVED", EPIC: "EPIC", PROJECT: "PROJECT", GLOBAL: "GLOBAL" },
}));

vi.mock("@/hooks/store/use-issue-detail", () => ({
  useIssueDetail: () => ({ peekIssue: peekIssueRef.current, setPeekIssue, isAnyModalOpen: false }),
}));

vi.mock("@/hooks/use-issue-layout-store", () => ({
  useIssueStoreType: () => "PROJECT",
}));

import { IssuePeekUrlSync } from "@/components/issues/peek-overview/url-sync";

const WORKSPACE = "acme";
const PROJECT_ID = "11111111-1111-4111-8111-111111111111";
const ISSUE_ID = "22222222-2222-4222-8222-222222222222";

describe("IssuePeekUrlSync — store -> URL idempotence", () => {
  beforeEach(() => {
    setSearchParamsSpy.mockClear();
    setPeekIssue.mockClear();
    routeParams.current = { workspaceSlug: WORKSPACE, projectId: PROJECT_ID };
    searchParamsRef.current = new URLSearchParams();
    peekIssueRef.current = undefined;
  });

  // Regression: `searchParams.get` returns `null` for an absent param while the
  // desired value is `undefined`. Comparing them directly made the guard always
  // fail, so every run of the effect issued a redundant `replace` navigation —
  // which interrupted the in-flight `router.push` from the peek expand button
  // and left the user on the list (PDASHOSS01-176).
  it("does not touch the URL when there is no peek and no peek params", () => {
    render(<IssuePeekUrlSync />);
    expect(setSearchParamsSpy).not.toHaveBeenCalled();
  });

  it("does not touch the URL when the open peek already matches the URL", () => {
    searchParamsRef.current = new URLSearchParams({ peekIssueId: ISSUE_ID });
    peekIssueRef.current = { issueId: ISSUE_ID, projectId: PROJECT_ID, workspaceSlug: WORKSPACE };
    render(<IssuePeekUrlSync />);
    expect(setSearchParamsSpy).not.toHaveBeenCalled();
  });

  it("does not touch the URL for a workspace-level peek already encoded in the URL", () => {
    // No route projectId (workspace all-issues), so the peek's project is
    // carried in ?peekProjectId — this is the view the redundant replace broke.
    routeParams.current = { workspaceSlug: WORKSPACE };
    searchParamsRef.current = new URLSearchParams({ peekIssueId: ISSUE_ID, peekProjectId: PROJECT_ID });
    peekIssueRef.current = { issueId: ISSUE_ID, projectId: PROJECT_ID, workspaceSlug: WORKSPACE };
    render(<IssuePeekUrlSync />);
    expect(setSearchParamsSpy).not.toHaveBeenCalled();
  });

  it("still strips the peek params when the peek is cleared", () => {
    searchParamsRef.current = new URLSearchParams({ peekIssueId: ISSUE_ID });
    peekIssueRef.current = undefined;
    render(<IssuePeekUrlSync />);
    expect(setSearchParamsSpy).toHaveBeenCalledTimes(1);
    const [updater, options] = setSearchParamsSpy.mock.calls[0];
    expect(options).toMatchObject({ replace: true });
    const next = updater(new URLSearchParams({ peekIssueId: ISSUE_ID }));
    expect(next.get("peekIssueId")).toBeNull();
  });

  it("still writes the peek params when a peek opens", () => {
    searchParamsRef.current = new URLSearchParams();
    peekIssueRef.current = { issueId: ISSUE_ID, projectId: PROJECT_ID, workspaceSlug: WORKSPACE };
    render(<IssuePeekUrlSync />);
    expect(setSearchParamsSpy).toHaveBeenCalledTimes(1);
    const [updater] = setSearchParamsSpy.mock.calls[0];
    expect(updater(new URLSearchParams()).get("peekIssueId")).toBe(ISSUE_ID);
  });

  it("preserves an existing nesting level rather than rewriting it", () => {
    searchParamsRef.current = new URLSearchParams({ peekIssueId: ISSUE_ID, peekNestingLevel: "2" });
    peekIssueRef.current = { issueId: ISSUE_ID, projectId: PROJECT_ID, workspaceSlug: WORKSPACE, nestingLevel: 2 };
    render(<IssuePeekUrlSync />);
    expect(setSearchParamsSpy).not.toHaveBeenCalled();
  });
});
