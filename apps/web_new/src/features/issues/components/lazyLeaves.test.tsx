// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const leaves = vi.hoisted(() => ({ listFactory: 0, analyticsFactory: 0 }));

vi.mock("./IssueList.js", () => {
  leaves.listFactory += 1;
  return {
    IssueList: function IssueList() {
      return <div data-testid="issue-list">rows</div>;
    },
  };
});

vi.mock("../analytics/AnalyticsEntry.js", () => {
  leaves.analyticsFactory += 1;
  return {
    AnalyticsEntry: function AnalyticsEntry() {
      return <button type="button">Analytics</button>;
    },
  };
});

type LeavesModule = typeof import("./lazyLeaves.js");

/** A fresh module instance, so each case starts with no settled request. */
async function freshLeaves(): Promise<LeavesModule> {
  vi.resetModules();
  leaves.listFactory = 0;
  leaves.analyticsFactory = 0;
  return import("./lazyLeaves.js");
}

beforeEach(() => {
  vi.clearAllMocks();
});

describe("issue list lazy leaves", () => {
  it("starts the chunk once and reuses the settled request on later visits", async () => {
    const module = await freshLeaves();
    expect(leaves.listFactory).toBe(0);
    await module.loadIssueListLeaves();
    module.primeIssueListLeaves();
    await module.loadIssueListLeaves();
    expect(leaves.listFactory).toBe(1);
    expect(leaves.analyticsFactory).toBe(1);
    expect(module.resolvedIssueListLeaves().IssueList).toBeTypeOf("function");
  });

  it("primes the chunk without awaiting it, sharing the request with beforeLoad", async () => {
    const module = await freshLeaves();
    module.primeIssueListLeaves();
    expect(() => module.resolvedIssueListLeaves()).toThrow(/await loadIssueListLeaves\(\) in beforeLoad/);
    await module.loadIssueListLeaves();
    expect(module.resolvedIssueListLeaves().IssueList).toBeTypeOf("function");
    expect(module.resolvedIssueListLeaves().AnalyticsEntry).toBeTypeOf("function");
  });

  it("renders the preloaded list leaf without wrapping it in a suspense boundary", async () => {
    const module = await freshLeaves();
    await module.loadIssueListLeaves();
    render(<module.ResolvedIssueList issues={[]} projectIdentifier="PF" layout="list" />);
    expect(screen.getByTestId("issue-list")).toBeInTheDocument();
  });

  it("renders the preloaded analytics entry for the header", async () => {
    const module = await freshLeaves();
    await module.loadIssueListLeaves();
    render(<module.ResolvedAnalyticsEntry workspaceSlug="acme" projectId="p1" projectName="Project" totalCount={4} />);
    expect(screen.getByRole("button", { name: "Analytics" })).toBeInTheDocument();
  });

  it("fails loudly when a route renders the leaves before awaiting the chunk", async () => {
    const module = await freshLeaves();
    expect(() => module.resolvedIssueListLeaves()).toThrow(/await loadIssueListLeaves\(\) in beforeLoad/);
    expect(() => render(<module.ResolvedIssueList issues={[]} projectIdentifier="PF" layout="list" />)).toThrow(
      /await loadIssueListLeaves\(\) in beforeLoad/
    );
    expect(() =>
      render(<module.ResolvedAnalyticsEntry workspaceSlug="acme" projectId="p1" projectName="Project" />)
    ).toThrow(/await loadIssueListLeaves\(\) in beforeLoad/);
  });

  it("leaves a primed chunk failure for beforeLoad to report, not a stray rejection", async () => {
    vi.doMock("../analytics/AnalyticsEntry.js", () => ({
      get AnalyticsEntry(): never {
        throw new Error("chunk unavailable");
      },
    }));
    const module = await freshLeaves();
    expect(() => module.primeIssueListLeaves()).not.toThrow();
    await expect(module.loadIssueListLeaves()).rejects.toThrow(/chunk unavailable/);
    vi.doUnmock("../analytics/AnalyticsEntry.js");
  });
});
