// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// No-active-cycle hero view (parity/specs/cycles.md §15, CYC-042):
// the explanation for every role, the creation shortcut for members
// and admins only.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

vi.mock("../dialog/CycleDialog.js", () => ({
  CreateCycleDialog: (props: { open: boolean; onOpenChange: (open: boolean) => void }) =>
    props.open ? (
      <div data-testid="create-dialog">
        <button type="button" onClick={() => props.onOpenChange(false)}>
          close create
        </button>
      </div>
    ) : null,
}));

import { CYCLES_PROJECT_ID } from "../testing.js";
import { NO_ACTIVE_CYCLE_DESCRIPTION, NO_ACTIVE_CYCLE_TITLE, NoActiveCycle } from "./NoActiveCycle.js";

function renderEmpty(canCreate: boolean): void {
  render(
    <QueryClientProvider client={new QueryClient()}>
      <NoActiveCycle
        workspaceSlug="acme"
        projectId={CYCLES_PROJECT_ID}
        canCreate={canCreate}
        currentProject={{ id: CYCLES_PROJECT_ID, name: "Website", identifier: "WEB" }}
      />
    </QueryClientProvider>
  );
}

describe("NoActiveCycle", () => {
  it("explains the missing cycle with the pinned copy", () => {
    renderEmpty(true);
    expect(screen.getByText(NO_ACTIVE_CYCLE_TITLE)).toBeInTheDocument();
    expect(screen.getByText(NO_ACTIVE_CYCLE_DESCRIPTION)).toBeInTheDocument();
    expect(NO_ACTIVE_CYCLE_TITLE).toMatch(/active cycle/i);
    expect(NO_ACTIVE_CYCLE_DESCRIPTION).toMatch(/today/i);
  });

  it("offers the creation shortcut to members and admins", () => {
    renderEmpty(true);
    expect(screen.queryByTestId("create-dialog")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Create cycle" }));
    expect(screen.getByTestId("create-dialog")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "close create" }));
    expect(screen.queryByTestId("create-dialog")).not.toBeInTheDocument();
  });

  it("offers no shortcut to guests", () => {
    renderEmpty(false);
    expect(screen.getByText(NO_ACTIVE_CYCLE_TITLE)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Create cycle" })).not.toBeInTheDocument();
  });
});
