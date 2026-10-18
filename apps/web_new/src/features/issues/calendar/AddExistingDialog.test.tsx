// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { ADD_EXISTING_EMPTY, ADD_EXISTING_SUBMIT, AddExistingDialog, attachableHits } from "./AddExistingDialog.js";

const PROJECT_ID = "223e4567-e89b-12d3-a456-426614174001";

const HITS = [
  {
    id: "323e4567-e89b-12d3-a456-426614174002",
    name: "Plain item",
    start_date: null,
    sequence_id: 42,
    project__name: "Website",
    project__identifier: "WEB",
    project_id: PROJECT_ID,
    workspace__slug: "acme",
    state__name: "Todo",
    state__group: "unstarted",
    state__color: "#999999",
  },
  {
    id: "423e4567-e89b-12d3-a456-426614174003",
    name: "Late item",
    start_date: "2026-10-20",
    sequence_id: 43,
    project__name: "Website",
    project__identifier: "WEB",
    project_id: PROJECT_ID,
    workspace__slug: "acme",
    state__name: "Todo",
    state__group: "unstarted",
    state__color: "#999999",
  },
];

function wrapper(client: QueryClient) {
  return function Wrapper({ children }: { children: React.ReactNode }): React.ReactElement {
    return React.createElement(QueryClientProvider, { client }, children);
  };
}

function renderDialog(open = true) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const onSubmit = vi.fn();
  const onOpenChange = vi.fn();
  render(
    <AddExistingDialog
      open={open}
      day="2026-10-15"
      workspaceSlug="acme"
      projectId={PROJECT_ID}
      submitting={false}
      onOpenChange={onOpenChange}
      onSubmit={onSubmit}
    />,
    { wrapper: wrapper(queryClient) }
  );
  return { onSubmit, onOpenChange, queryClient };
}

function stubSearch(status = 200, hits: unknown = HITS) {
  createAppClient("https://api.test", (async (url: string) => ({
    status,
    url,
    headers: { get: () => null },
    text: async () => (status === 200 ? JSON.stringify(hits) : "{}"),
  })) as never);
}

afterEach(() => resetAppClient());

describe("AddExistingDialog", () => {
  it("filters choices whose start date is after the day", () => {
    expect(attachableHits(HITS, "2026-10-15").map((hit) => hit.name)).toEqual(["Plain item"]);
    expect(attachableHits(HITS, "2026-10-20")).toHaveLength(2);
  });

  it("lists choices, selects, and submits the checked ids", async () => {
    stubSearch();
    const { onSubmit } = renderDialog();
    const option = await screen.findByText("Plain item");
    expect(screen.queryByText("Late item")).not.toBeInTheDocument();
    await userEvent.setup().click(option);
    await userEvent.setup().click(screen.getByRole("button", { name: ADD_EXISTING_SUBMIT }));
    expect(onSubmit).toHaveBeenCalledWith([HITS[0]?.id]);
  });

  it("shows the empty state when nothing can land on the day", async () => {
    stubSearch(200, [HITS[1]]);
    renderDialog();
    await waitFor(() => expect(screen.getByText(ADD_EXISTING_EMPTY)).toBeInTheDocument());
  });

  it("shows loading and handles close and an empty selection", async () => {
    stubSearch();
    const { onOpenChange } = renderDialog();
    expect(screen.getByText("Loading work items…")).toBeInTheDocument();
    await screen.findByText("Plain item");
    expect(screen.getByRole("button", { name: ADD_EXISTING_SUBMIT })).toBeDisabled();
    await userEvent.setup().click(screen.getByRole("button", { name: "Cancel" }));
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });
});
