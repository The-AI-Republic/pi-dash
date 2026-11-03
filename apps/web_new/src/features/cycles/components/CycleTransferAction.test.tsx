// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Transfer entry action (parity/specs/cycles.md §12, CYC-031): the
// label opens the shared dialog and the dialog closes back.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

vi.mock("./CycleTransferDialog.js", () => ({
  CycleTransferDialog: ({
    open,
    onClose,
    sourceCycleId,
  }: {
    open: boolean;
    onClose: () => void;
    sourceCycleId: string;
  }) =>
    open ? (
      <div data-testid="transfer-dialog" data-source={sourceCycleId}>
        <button type="button" onClick={onClose}>
          close dialog
        </button>
      </div>
    ) : null,
}));

import { CYCLES_CYCLE_ID, CYCLES_PROJECT_ID } from "../testing.js";
import { CycleTransferAction } from "./CycleTransferAction.js";

describe("CycleTransferAction", () => {
  it("opens the shared dialog from the entry label and closes back", () => {
    render(
      <QueryClientProvider client={new QueryClient()}>
        <CycleTransferAction
          workspaceSlug="acme"
          projectId={CYCLES_PROJECT_ID}
          sourceCycleId={CYCLES_CYCLE_ID}
          label="Transfer 2 work items"
        />
      </QueryClientProvider>
    );
    expect(screen.queryByTestId("transfer-dialog")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Transfer 2 work items" }));
    expect(screen.getByTestId("transfer-dialog")).toHaveAttribute("data-source", CYCLES_CYCLE_ID);
    fireEvent.click(screen.getByRole("button", { name: "close dialog" }));
    expect(screen.queryByTestId("transfer-dialog")).not.toBeInTheDocument();
  });
});
