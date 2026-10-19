// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Add-existing modal (parity/specs/issues.md §15, ISS-175): a debounced
// project-scoped search with no workspace toggle, a multi-select list,
// one submit posting the chosen ids, and the empty-pick refusal.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { IssueSearchHit, TransportResponse } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../../../shared/shell/index.js", () => ({
  notifySuccess: vi.fn(),
  notifyError: vi.fn(),
}));

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { notifyError } from "../../../shared/shell/index.js";
import { AddExistingDialog, hitIdentifier, selectAddCandidates, togglePicked } from "./AddExistingDialog.js";

const WS = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const PARENT = "323e4567-e89b-12d3-a456-426614174002";
const ONE = "423e4567-e89b-12d3-a456-426614174003";
const TWO = "523e4567-e89b-12d3-a456-426614174004";

function hit(id: string, name: string, sequence = 4): IssueSearchHit {
  return {
    id,
    name,
    sequence_id: sequence,
    start_date: null,
    project__name: "Atlas",
    project__identifier: "ATL",
    project_id: PROJECT,
    workspace__slug: WS,
    state__name: "In progress",
    state__group: "started",
    state__color: "#d9912b",
  };
}

function ok(url: string, status: number, body: unknown): TransportResponse {
  return {
    status,
    url,
    headers: { get: () => null },
    text: async () => JSON.stringify(body),
  } as TransportResponse;
}

function stubApi(options: { hits?: IssueSearchHit[]; fail?: boolean; addFail?: boolean } = {}) {
  const calls: { method: string; url: string; body: unknown }[] = [];
  createAppClient("https://api.test", (async (url: string, init: { method?: string; body?: string }) => {
    if (url.endsWith("/auth/get-csrf-token/")) return ok(url, 200, { csrf_token: "csrf" });
    const method = init.method ?? "GET";
    const body: unknown = init.body === undefined ? undefined : JSON.parse(init.body);
    calls.push({ method, url, body });
    if (url.includes("/search-issues/")) {
      if (options.fail === true) return ok(url, 500, { detail: "search failed" });
      return ok(url, 200, options.hits ?? [hit(ONE, "First candidate"), hit(TWO, "Second candidate")]);
    }
    if (method === "POST" && url.endsWith(`/sub-issues/`)) {
      if (options.addFail === true) return ok(url, 500, { detail: "attach failed" });
      return ok(url, 201, { sub_issues: [], state_distribution: {} });
    }
    return ok(url, 200, {});
  }) as never);
  return calls;
}

function renderDialog(props: Partial<React.ComponentProps<typeof AddExistingDialog>> = {}) {
  const onOpenChange = vi.fn();
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <AddExistingDialog
        open
        onOpenChange={onOpenChange}
        scope={{ workspaceSlug: WS, projectId: PROJECT, parentId: PARENT, isEpic: false }}
        projectName="Atlas"
        {...props}
      />
    </QueryClientProvider>
  );
  return { onOpenChange };
}

beforeEach(() => {
  vi.clearAllMocks();
});

afterEach(() => {
  resetAppClient();
});

describe("candidate rules", () => {
  it("derives identifiers from the project code and sequence", () => {
    expect(hitIdentifier(hit(ONE, "First", 12))).toBe("ATL-12");
  });

  it("drops the parent and repeated hits, keeping picked rows first", () => {
    const rows = selectAddCandidates(
      [hit(ONE, "One"), hit(PARENT, "Parent"), hit(TWO, "Two"), hit(ONE, "One again")],
      [TWO],
      PARENT
    );
    expect(rows.map((row) => row.id)).toEqual([TWO, ONE]);
  });

  it("toggles a pick in and out, preserving order", () => {
    expect(togglePicked([], ONE)).toEqual([ONE]);
    expect(togglePicked([ONE, TWO], TWO)).toEqual([ONE]);
  });
});

describe("AddExistingDialog", () => {
  it("searches the project with the sub-issue exclusion after the debounce", async () => {
    const calls = stubApi();
    renderDialog();
    expect(await screen.findByRole("option", { name: /First candidate/ })).toBeInTheDocument();
    const search = calls.find((call) => call.url.includes("/search-issues/"));
    expect(search?.url).toContain(`/projects/${PROJECT}/search-issues/`);
    expect(search?.url).toContain("sub_issue=true");
    expect(search?.url).not.toContain("workspace_search");
  });

  it("types a term into the search request", async () => {
    const calls = stubApi();
    const user = userEvent.setup();
    renderDialog();
    await screen.findByRole("option", { name: /First candidate/ });
    await user.type(screen.getByRole("searchbox", { name: "Search work items" }), "second");
    await waitFor(() =>
      expect(calls.some((call) => call.url.includes("/search-issues/") && call.url.includes("search=second"))).toBe(
        true
      )
    );
  });

  it("takes several rows and posts the chosen ids", async () => {
    const calls = stubApi();
    const user = userEvent.setup();
    const { onOpenChange } = renderDialog();
    await user.click(await screen.findByRole("option", { name: /First candidate/ }));
    await user.click(screen.getByRole("option", { name: /Second candidate/ }));
    expect(screen.getByRole("button", { name: "Add selected work items" })).toHaveTextContent("(2)");
    await user.click(screen.getByRole("button", { name: "Add selected work items" }));
    await waitFor(() => expect(onOpenChange).toHaveBeenCalledWith(false));
    const post = calls.find((call) => call.method === "POST");
    expect(post?.url).toContain(`/issues/${PARENT}/sub-issues/`);
    expect(post?.body).toEqual({ sub_issue_ids: [ONE, TWO] });
  });

  it("unpicks a row from the chips above the list", async () => {
    stubApi();
    const user = userEvent.setup();
    renderDialog();
    await user.click(await screen.findByRole("option", { name: /First candidate/ }));
    await user.click(screen.getByRole("button", { name: "Unpick ATL-4" }));
    expect(screen.getByRole("option", { name: /First candidate/ })).toHaveAttribute("aria-selected", "false");
  });

  it("keeps the submit disabled until something is picked", async () => {
    stubApi();
    renderDialog();
    await screen.findByRole("option", { name: /First candidate/ });
    expect(screen.getByRole("button", { name: "Add selected work items" })).toBeDisabled();
  });

  it("shows the search failure in place of the list", async () => {
    stubApi({ fail: true });
    renderDialog();
    expect(await screen.findByRole("alert")).toHaveTextContent("Couldn't search this project");
  });

  it("shows the empty search state", async () => {
    stubApi({ hits: [] });
    renderDialog();
    expect(await screen.findByText("No work items match this search.")).toBeInTheDocument();
  });

  it("closes from Cancel and keeps the parent open after a failure", async () => {
    stubApi({ addFail: true });
    const user = userEvent.setup();
    const { onOpenChange } = renderDialog();
    await user.click(await screen.findByRole("option", { name: /First candidate/ }));
    await user.click(screen.getByRole("button", { name: "Add selected work items" }));
    await waitFor(() => expect(notifyError).toHaveBeenCalled());
    expect(onOpenChange).not.toHaveBeenCalledWith(false);
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("stays closed when the parent's open flag is false", () => {
    stubApi();
    renderDialog({ open: false });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });
});
