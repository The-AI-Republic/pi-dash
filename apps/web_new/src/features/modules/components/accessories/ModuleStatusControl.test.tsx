// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Module lifecycle status control (parity/specs/modules.md §4.2,
// MOD-014): the pill paints the current status in its color, the menu
// lists all six in order, a pick saves immediately, and re-picking the
// current status writes nothing. Guests and archived rows get the same
// pill disabled with no menu.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  updateModule: vi.fn(),
  notifySuccess: vi.fn(),
  notifyError: vi.fn(),
}));

vi.mock("../../../../shared/shell/index.js", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  notifySuccess: mocks.notifySuccess,
  notifyError: mocks.notifyError,
}));

vi.mock("@pidash/api-client", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@pidash/api-client")>()),
  updateModule: mocks.updateModule,
}));

vi.mock("../../../../core/api/client.js", () => ({ getAppClient: () => ({ stub: true }) }));

import type { ModuleStatus } from "@pidash/api-client";

import { makeModuleRow, MODULES_MODULE_ID, MODULES_PROJECT_ID } from "../../testing.js";
import { ModuleStatusControl } from "./ModuleStatusControl.js";

function renderControl(status: ModuleStatus = "planned", disabled = false): void {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <ModuleStatusControl
        workspaceSlug="acme"
        projectId={MODULES_PROJECT_ID}
        module={makeModuleRow({ id: MODULES_MODULE_ID, status })}
        disabled={disabled}
        testId="modules-row-status"
      />
    </QueryClientProvider>
  );
}

beforeEach(() => {
  vi.clearAllMocks();
  mocks.updateModule.mockResolvedValue({ id: MODULES_MODULE_ID });
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe("ModuleStatusControl", () => {
  it("paints the current status label in its color", () => {
    renderControl("in-progress");
    const pill = screen.getByTestId("modules-row-status");
    expect(pill).toHaveTextContent("In Progress");
    expect(pill).toHaveAttribute("data-status-color", "#f39e1f");
    expect(pill).toHaveStyle({ color: "#f39e1f" });
    expect(pill).toHaveAccessibleName("Change status of Delivery");
  });

  it("lists all six statuses in order", () => {
    renderControl();
    fireEvent.click(screen.getByTestId("modules-row-status"));
    const menu = screen.getByRole("menu");
    expect(
      within(menu)
        .getAllByRole("menuitem")
        .map((node) => (node.textContent ?? "").trim())
    ).toEqual(["Backlog", "Planned", "In Progress", "Paused", "Completed", "Cancelled"]);
  });

  it("saves a picked status immediately", async () => {
    renderControl("backlog");
    fireEvent.click(screen.getByTestId("modules-row-status"));
    fireEvent.click(screen.getByRole("menuitem", { name: "Planned" }));
    await waitFor(() => {
      expect(mocks.updateModule).toHaveBeenCalledTimes(1);
    });
    expect(mocks.updateModule).toHaveBeenCalledWith(expect.anything(), "acme", MODULES_PROJECT_ID, MODULES_MODULE_ID, {
      status: "planned",
    });
    expect(mocks.notifySuccess).toHaveBeenCalledWith("Success!", "Module updated successfully.");
  });

  it("writes nothing when the current status is re-picked", () => {
    renderControl("planned");
    fireEvent.click(screen.getByTestId("modules-row-status"));
    fireEvent.click(screen.getByRole("menuitem", { name: "Planned" }));
    expect(mocks.updateModule).not.toHaveBeenCalled();
    expect(mocks.notifySuccess).not.toHaveBeenCalled();
  });

  it("renders a disabled pill with no menu when disabled", () => {
    renderControl("planned", true);
    const pill = screen.getByTestId("modules-row-status");
    expect(pill).toBeDisabled();
    expect(pill).toHaveTextContent("Planned");
    expect(pill).toHaveAttribute("data-status-color", "#3f76ff");
    fireEvent.click(pill);
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
  });
});
