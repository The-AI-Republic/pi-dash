// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { AddRelationDialog, RELATION_EMPTY_SELECTION } from "./AddRelationDialog.js";
import { ISSUE, PROJECT, WS, newQueryClient, stubTransport } from "./widgetTestUtils.js";

const candidate = {
  id: "a23e4567-e89b-12d3-a456-426614174009",
  name: "Candidate one",
  sequence_id: 9,
  project__name: "Web",
  project__identifier: "WEB",
  project_id: PROJECT,
  workspace__slug: WS,
  state__name: "Todo",
  state__group: "unstarted",
  state__color: "#000",
  start_date: null,
};

function renderDialog(options: Parameters<typeof stubTransport>[0] = {}, kind: "blocking" | "relates_to" = "blocking") {
  const { calls, transport } = stubTransport(options);
  createAppClient("https://api.test", transport as never);
  const onSubmit = vi.fn();
  const onEmptySelection = vi.fn();
  const onOpenChange = vi.fn();
  render(
    <QueryClientProvider client={newQueryClient()}>
      <AddRelationDialog
        open
        onOpenChange={onOpenChange}
        workspaceSlug={WS}
        projectId={PROJECT}
        issueId={ISSUE}
        kind={kind}
        pending={false}
        onSubmit={onSubmit}
        onEmptySelection={onEmptySelection}
      />
    </QueryClientProvider>
  );
  return { calls, onSubmit, onEmptySelection, onOpenChange };
}

beforeEach(() => {
  vi.clearAllMocks();
});
afterEach(() => {
  resetAppClient();
});

describe("add relation dialog", () => {
  it("names the kind in its title and prompt", async () => {
    renderDialog({ candidates: [] });
    expect(await screen.findByRole("dialog", { name: /Add blocking/i })).toBeInTheDocument();
    expect(screen.getByText("Work items that wait on this one.")).toBeInTheDocument();
  });

  it("refuses an empty selection without calling submit", async () => {
    const user = userEvent.setup();
    const { onSubmit, onEmptySelection } = renderDialog({ candidates: [candidate] });
    await user.click(await screen.findByRole("button", { name: "Add selected work items" }));
    expect(onEmptySelection).toHaveBeenCalledTimes(1);
    expect(onSubmit).not.toHaveBeenCalled();
    expect(RELATION_EMPTY_SELECTION).toBe("Pick at least one work item to relate.");
  });

  it("toggles candidates and submits every picked id", async () => {
    const user = userEvent.setup();
    const second = { ...candidate, id: "b23e4567-e89b-12d3-a456-42661417400a", name: "Candidate two", sequence_id: 10 };
    const { onSubmit } = renderDialog({ candidates: [candidate, second] });
    await user.click(await screen.findByRole("option", { name: /Candidate one/ }));
    await user.click(screen.getByRole("option", { name: /Candidate two/ }));
    await user.click(screen.getByRole("button", { name: "Add selected work items" }));
    expect(onSubmit).toHaveBeenCalledWith([candidate.id, second.id]);
  });

  it("unpicks a candidate on a second click", async () => {
    const user = userEvent.setup();
    const { onSubmit, onEmptySelection } = renderDialog({ candidates: [candidate] });
    const option = await screen.findByRole("option", { name: /Candidate one/ });
    await user.click(option);
    expect(option).toHaveAttribute("aria-selected", "true");
    await user.click(option);
    expect(option).toHaveAttribute("aria-selected", "false");
    await user.click(screen.getByRole("button", { name: "Add selected work items" }));
    expect(onEmptySelection).toHaveBeenCalledTimes(1);
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("searches the project and widens to the workspace on the toggle", async () => {
    const user = userEvent.setup();
    const { calls } = renderDialog({ candidates: [candidate] });
    await screen.findByRole("option", { name: /Candidate one/ });
    const first = calls.find((call) => call.url.includes("search-issues"));
    expect(first?.url).toContain("issue_relation=true");
    expect(first?.url).not.toContain("workspace_search=true");
    await user.type(screen.getByLabelText("Search work items"), "cand");
    await waitFor(() => expect(calls.some((call) => call.url.includes("search=cand"))).toBe(true));
    await user.click(screen.getByRole("switch", { name: "Search the whole workspace" }));
    await waitFor(() => expect(calls.some((call) => call.url.includes("workspace_search=true"))).toBe(true));
  });

  it("shows loading, empty and error states", async () => {
    const { calls } = renderDialog({ candidates: [] });
    await waitFor(() => expect(screen.getByText("No work items match this search.")).toBeInTheDocument());
    expect(calls.some((call) => call.url.includes("search-issues"))).toBe(true);
    resetAppClient();
    renderDialog({ candidatesStatus: 500 });
    expect(await screen.findByRole("alert")).toHaveTextContent("Couldn't load work items. Try again.");
  });

  it("dismisses through cancel", async () => {
    const user = userEvent.setup();
    const { onOpenChange } = renderDialog({ candidates: [] });
    await user.click(await screen.findByRole("button", { name: "Cancel" }));
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });
});
