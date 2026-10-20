// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import type { IssueSearchHit } from "@pidash/api-client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({ search: vi.fn() }));

vi.mock("@pidash/api-client", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@pidash/api-client")>()),
  searchIssues: mocks.search,
}));

vi.mock("../../../core/api/client.js", () => ({ getAppClient: () => ({ stub: true }) }));

import { AddExistingModal, addExistingSearchKey, type AddExistingModalProps } from "./AddExistingModal.js";

const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const ONE = "123e4567-e89b-12d3-a456-426614174000";
const TWO = "323e4567-e89b-12d3-a456-426614174002";

function hit(id: string, name: string, sequence: number): IssueSearchHit {
  return {
    id,
    name,
    start_date: null,
    sequence_id: sequence,
    project__name: "Website",
    project__identifier: "WEB",
    project_id: PROJECT,
    workspace__slug: "acme",
    state__name: "Todo",
    state__group: "unstarted",
    state__color: "#fff",
  };
}

function setup(overrides: Partial<AddExistingModalProps> = {}) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const props: AddExistingModalProps = {
    open: true,
    onOpenChange: vi.fn(),
    workspaceSlug: "acme",
    projectId: PROJECT,
    onAttach: vi.fn().mockResolvedValue(undefined),
    ...overrides,
  };
  const view = render(
    <QueryClientProvider client={queryClient}>
      <AddExistingModal {...props} />
    </QueryClientProvider>
  );
  return { props, view };
}

beforeEach(() => vi.clearAllMocks());

describe("addExistingSearchKey", () => {
  it("scopes the search by workspace, project and text", () => {
    expect(addExistingSearchKey("acme", PROJECT, "auth")).toEqual([
      "ws",
      "acme",
      "issues",
      "add-existing",
      PROJECT,
      "auth",
    ]);
  });
});

describe("AddExistingModal", () => {
  it("searches the project with exclusion flags", async () => {
    mocks.search.mockResolvedValue([hit(ONE, "Fix auth", 1)]);
    setup({ excludeCycleMembers: true, excludeModuleId: "module-1" });
    await waitFor(() => expect(mocks.search).toHaveBeenCalled());
    expect(mocks.search).toHaveBeenCalledWith({ stub: true }, "acme", PROJECT, {
      search: "",
      cycle: true,
      module: "module-1",
    });
    expect(await screen.findByText("Fix auth")).toBeInTheDocument();
  });

  it("shows loading, empty and error states", async () => {
    mocks.search.mockReturnValue(new Promise(() => undefined));
    const loading = setup();
    expect(screen.getByText("Searching…")).toBeInTheDocument();
    loading.view.unmount();
    mocks.search.mockResolvedValue([]);
    const empty = setup();
    expect(await screen.findByText("No matching work items.")).toBeInTheDocument();
    empty.view.unmount();
    mocks.search.mockRejectedValue(new Error("boom"));
    setup();
    expect(await screen.findByText("Couldn't search work items.")).toBeInTheDocument();
  });

  it("filters already-attached rows from the results", async () => {
    mocks.search.mockResolvedValue([hit(ONE, "Fix auth", 1), hit(TWO, "Fix billing", 2)]);
    setup({ excludeIds: [ONE] });
    expect(await screen.findByText("Fix billing")).toBeInTheDocument();
    expect(screen.queryByText("Fix auth")).not.toBeInTheDocument();
  });

  it("toggles selections and attaches them", async () => {
    const user = userEvent.setup();
    mocks.search.mockResolvedValue([hit(ONE, "Fix auth", 1), hit(TWO, "Fix billing", 2)]);
    const onOpenChange = vi.fn();
    const onAttach = vi.fn().mockResolvedValue(undefined);
    setup({ onOpenChange, onAttach });
    await user.click(await screen.findByRole("checkbox", { name: /Fix auth/ }));
    await user.click(screen.getByRole("checkbox", { name: /Fix billing/ }));
    await user.click(screen.getByRole("checkbox", { name: /Fix auth/ }));
    await user.click(screen.getByRole("button", { name: "Add selected work items" }));
    await waitFor(() => expect(onAttach).toHaveBeenCalledWith([TWO]));
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("keeps the add action disabled without a selection", () => {
    mocks.search.mockResolvedValue([]);
    setup();
    expect(screen.getByRole("button", { name: "Add selected work items" })).toBeDisabled();
  });

  it("resets the query and selection when closed", async () => {
    const user = userEvent.setup();
    mocks.search.mockResolvedValue([hit(ONE, "Fix auth", 1)]);
    const { props, view } = setup();
    await user.type(screen.getByRole("textbox", { name: "Search work items" }), "auth");
    await user.click(await screen.findByRole("checkbox", { name: /Fix auth/ }));
    view.rerender(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <AddExistingModal {...props} open={false} />
      </QueryClientProvider>
    );
    view.rerender(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <AddExistingModal {...props} open />
      </QueryClientProvider>
    );
    expect(screen.getByRole("textbox", { name: "Search work items" })).toHaveValue("");
    await waitFor(() => expect(screen.getByRole("checkbox", { name: /Fix auth/ })).not.toBeChecked());
  });

  it("closes on cancel without attaching", () => {
    mocks.search.mockResolvedValue([]);
    const onOpenChange = vi.fn();
    setup({ onOpenChange });
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });
});
