// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Detail de-dupe popover (ISS-231): the community build shows no trace of
// it, and a cloud build lists the near-duplicates the create seam finds,
// minus the viewed item, behind a count button.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, describe, expect, it } from "vitest";

import type { IssueSearchHit } from "@pidash/api-client";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { edition } from "../../../core/edition/index.js";
import { DetailDuplicatePopover, duplicateCountLabel } from "./DetailDuplicatePopover.js";
import { ISSUE_DEDUPE_FLAG } from "./flags.js";

const PROJECT = "323e4567-e89b-12d3-a456-426614174002";
const VIEWED = "623e4567-e89b-12d3-a456-426614174010";
const OTHER = "723e4567-e89b-12d3-a456-426614174011";

afterEach(() => {
  cleanup();
  resetAppClient();
  delete edition.flags[ISSUE_DEDUPE_FLAG];
});

function hit(id: string, name: string, sequence = 7): IssueSearchHit {
  return {
    id,
    name,
    start_date: null,
    sequence_id: sequence,
    project__name: "Atlas",
    project__identifier: "ATL",
    project_id: PROJECT,
    workspace__slug: "acme",
    state__name: "Todo",
    state__group: "unstarted",
    state__color: "#5e6ad2",
  };
}

function serve(hits: IssueSearchHit[]): string[] {
  const calls: string[] = [];
  createAppClient("https://api.test", (async (url: string) => {
    calls.push(url);
    if (url.endsWith("/auth/get-csrf-token/")) {
      return { status: 200, url, headers: { get: () => null }, text: async () => JSON.stringify({ csrf_token: "c" }) };
    }
    return { status: 200, url, headers: { get: () => null }, text: async () => JSON.stringify(hits) };
  }) as never);
  return calls;
}

function renderPopover(title = "Fix login redirect"): void {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <DetailDuplicatePopover workspaceSlug="acme" projectId={PROJECT} title={title} issueId={VIEWED} />
    </QueryClientProvider>
  );
}

describe("duplicateCountLabel", () => {
  it("singularizes one match", () => {
    expect(duplicateCountLabel(1)).toBe("1 possible duplicate");
  });

  it("pluralizes several", () => {
    expect(duplicateCountLabel(3)).toBe("3 possible duplicates");
  });
});

describe("DetailDuplicatePopover", () => {
  it("mounts nothing in the community build and never searches", async () => {
    const calls = serve([hit(OTHER, "Fix login redirect")]);
    renderPopover();
    await new Promise((resolve) => setTimeout(resolve, 700));
    expect(screen.queryByTestId("detail-duplicate-popover")).toBeNull();
    expect(calls.some((url) => url.includes("search-issues"))).toBe(false);
  });

  it("lists the near-duplicates in a cloud build, behind a count button", async () => {
    edition.flags[ISSUE_DEDUPE_FLAG] = true;
    serve([hit(OTHER, "Fix login redirect"), hit(VIEWED, "Fix login redirect")]);
    renderPopover();
    await waitFor(() => expect(screen.getByTestId("detail-duplicate-popover")).toBeInTheDocument(), {
      timeout: 3000,
    });
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: /1 possible duplicate/ }));
    expect(screen.getByRole("link", { name: "Fix login redirect" })).toHaveAttribute("href", "/acme/browse/ATL-7");
  });

  it("stays away when the only match is the viewed item", async () => {
    edition.flags[ISSUE_DEDUPE_FLAG] = true;
    serve([hit(VIEWED, "Fix login redirect")]);
    renderPopover();
    await new Promise((resolve) => setTimeout(resolve, 700));
    expect(screen.queryByTestId("detail-duplicate-popover")).toBeNull();
  });

  it("stays away when no near-duplicate exists", async () => {
    edition.flags[ISSUE_DEDUPE_FLAG] = true;
    serve([hit(OTHER, "Unrelated billing bug")]);
    renderPopover();
    await new Promise((resolve) => setTimeout(resolve, 700));
    expect(screen.queryByTestId("detail-duplicate-popover")).toBeNull();
  });
});
