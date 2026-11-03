// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Transfer dialog (parity/specs/cycles.md §12, CYC-031): the open-only
// options with their pills, search narrowing, the two zero states, and
// the pick flow that closes on success and holds on a refusal.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({ cycles: vi.fn(), transfer: vi.fn(), pending: false, onClose: vi.fn() }));

vi.mock("../../../shared/pickers/reference.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../shared/pickers/reference.js")>()),
  usePickerCycles: mocks.cycles,
}));
vi.mock("../api/cycleTransfer.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../api/cycleTransfer.js")>()),
  useTransferCycleWork: () => ({ transfer: mocks.transfer, pending: mocks.pending }),
}));

import { CYCLES_PROJECT_ID } from "../testing.js";
import {
  CYCLE_TRANSFER_NO_MATCH,
  CYCLE_TRANSFER_NO_TARGET,
  CYCLE_TRANSFER_SEARCH_PLACEHOLDER,
  CYCLE_TRANSFER_TITLE,
  CycleTransferDialog,
} from "./CycleTransferDialog.js";

const WS = "acme";
const SOURCE = "323e4567-e89b-12d3-a456-426614174002";
const TARGET = "423e4567-e89b-12d3-a456-426614174003";

function cycles(): unknown[] {
  return [
    { id: SOURCE, project_id: CYCLES_PROJECT_ID, name: "Source cycle", is_favorite: false, status: "COMPLETED" },
    { id: TARGET, project_id: CYCLES_PROJECT_ID, name: "Target cycle", is_favorite: false, status: "CURRENT" },
    { id: "draft", project_id: CYCLES_PROJECT_ID, name: "Draft cycle", is_favorite: false, status: "DRAFT" },
  ];
}

function setup(open = true): void {
  render(
    <QueryClientProvider client={new QueryClient()}>
      <CycleTransferDialog
        workspaceSlug={WS}
        projectId={CYCLES_PROJECT_ID}
        sourceCycleId={SOURCE}
        open={open}
        onClose={mocks.onClose}
      />
    </QueryClientProvider>
  );
}

beforeEach(() => {
  vi.clearAllMocks();
  mocks.pending = false;
  mocks.cycles.mockReturnValue({ data: cycles(), isPending: false });
  mocks.transfer.mockResolvedValue(true);
});

describe("CycleTransferDialog", () => {
  it("lists open destinations with pills, never the source or a finished cycle", () => {
    setup();
    expect(screen.getByText(CYCLE_TRANSFER_TITLE)).toBeInTheDocument();
    expect(screen.getByPlaceholderText(CYCLE_TRANSFER_SEARCH_PLACEHOLDER)).toBeInTheDocument();
    const target = screen.getByRole("option", { name: /^Target cycle/ });
    expect(target).toHaveTextContent("Current");
    expect(screen.getByRole("option", { name: /^Draft cycle/ })).toHaveTextContent("Draft");
    expect(screen.queryByText("Source cycle")).not.toBeInTheDocument();
    // Accessible names start with the cycle name.
    expect(target.getAttribute("aria-label") ?? target.textContent ?? "").toMatch(/^Target cycle/);
  });

  it("narrows the options by name as the search types", async () => {
    const user = userEvent.setup();
    setup();
    await user.type(screen.getByPlaceholderText(CYCLE_TRANSFER_SEARCH_PLACEHOLDER), "draft");
    expect(screen.getByRole("option", { name: /^Draft cycle/ })).toBeInTheDocument();
    await waitFor(() => expect(screen.queryByText("Target cycle")).not.toBeInTheDocument());
    await user.clear(screen.getByPlaceholderText(CYCLE_TRANSFER_SEARCH_PLACEHOLDER));
    await waitFor(() => expect(screen.getByRole("option", { name: /^Target cycle/ })).toBeInTheDocument());
  });

  it("explains that an open cycle must be created when no destination exists", () => {
    mocks.cycles.mockReturnValue({ data: [cycles()[0]], isPending: false });
    setup();
    expect(screen.getByText(CYCLE_TRANSFER_NO_TARGET)).toBeInTheDocument();
  });

  it("reports a search that matches nothing separately", async () => {
    const user = userEvent.setup();
    setup();
    await user.type(screen.getByPlaceholderText(CYCLE_TRANSFER_SEARCH_PLACEHOLDER), "zzz");
    expect(await screen.findByText(CYCLE_TRANSFER_NO_MATCH)).toBeInTheDocument();
  });

  it("holds the loading line while the picker read flies", () => {
    mocks.cycles.mockReturnValue({ data: undefined, isPending: true });
    setup();
    expect(screen.getByText("Loading cycles…")).toBeInTheDocument();
  });

  it("moves on pick and closes on success", async () => {
    setup();
    fireEvent.click(screen.getByRole("option", { name: /^Target cycle/ }));
    await waitFor(() => expect(mocks.transfer).toHaveBeenCalledWith(TARGET));
    await waitFor(() => expect(mocks.onClose).toHaveBeenCalledTimes(1));
  });

  it("holds the dialog open on a refusal", async () => {
    mocks.transfer.mockResolvedValue(false);
    setup();
    fireEvent.click(screen.getByRole("option", { name: /^Target cycle/ }));
    await waitFor(() => expect(mocks.transfer).toHaveBeenCalledWith(TARGET));
    await waitFor(() => expect(mocks.transfer).toHaveBeenCalledTimes(1));
    expect(mocks.onClose).not.toHaveBeenCalled();
  });

  it("locks the options while the move flies", () => {
    mocks.pending = true;
    setup();
    expect(screen.getByRole("option", { name: /^Target cycle/ })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Cancel" })).toBeDisabled();
  });

  it("cancels the dialog", () => {
    setup();
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(mocks.onClose).toHaveBeenCalledTimes(1);
  });

  it("closes on Escape through the open change", async () => {
    setup();
    await userEvent.setup().keyboard("{Escape}");
    expect(mocks.onClose).toHaveBeenCalledTimes(1);
  });

  it("clears the search when the dialog reopens", async () => {
    const user = userEvent.setup();
    const { rerender } = render(
      <QueryClientProvider client={new QueryClient()}>
        <CycleTransferDialog
          workspaceSlug={WS}
          projectId={CYCLES_PROJECT_ID}
          sourceCycleId={SOURCE}
          open={true}
          onClose={mocks.onClose}
        />
      </QueryClientProvider>
    );
    await user.type(screen.getByPlaceholderText(CYCLE_TRANSFER_SEARCH_PLACEHOLDER), "draft");
    await waitFor(() => expect(screen.queryByText("Target cycle")).not.toBeInTheDocument());
    rerender(
      <QueryClientProvider client={new QueryClient()}>
        <CycleTransferDialog
          workspaceSlug={WS}
          projectId={CYCLES_PROJECT_ID}
          sourceCycleId={SOURCE}
          open={false}
          onClose={mocks.onClose}
        />
      </QueryClientProvider>
    );
    rerender(
      <QueryClientProvider client={new QueryClient()}>
        <CycleTransferDialog
          workspaceSlug={WS}
          projectId={CYCLES_PROJECT_ID}
          sourceCycleId={SOURCE}
          open={true}
          onClose={mocks.onClose}
        />
      </QueryClientProvider>
    );
    expect(screen.getByPlaceholderText(CYCLE_TRANSFER_SEARCH_PLACEHOLDER)).toHaveValue("");
  });
});
