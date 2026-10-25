// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Module inline date-range control (parity/specs/modules.md §4.1,
// MOD-013): the trigger reads the saved range or the pinned start/end
// placeholders, the shared calendar opens from it, and every pick saves
// both sides at once — extending a lone start, clearing on a re-click
// of the lone day, re-anchoring a complete range. Guests and archived
// rows get the same control disabled.

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

vi.mock("../../../issues/index.js", async (importOriginal) => {
  const original = await importOriginal<Record<string, unknown>>();
  return { ...original, useWeekStart: () => 0 };
});

import { makeModuleRow, MODULES_MODULE_ID, MODULES_PROJECT_ID } from "../../testing.js";
import { ModuleDateControl } from "./ModuleDateControl.js";

function renderControl(
  dates: { start_date: string | null; target_date: string | null } = { start_date: null, target_date: null },
  disabled = false
): void {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <ModuleDateControl
        workspaceSlug="acme"
        projectId={MODULES_PROJECT_ID}
        module={makeModuleRow({ id: MODULES_MODULE_ID, ...dates })}
        disabled={disabled}
        testId="modules-row-dates"
      />
    </QueryClientProvider>
  );
}

function openCalendar(): HTMLElement {
  fireEvent.click(screen.getByTestId("modules-row-dates-trigger"));
  return screen.getByTestId("modules-row-dates-calendar");
}

beforeEach(() => {
  vi.clearAllMocks();
  mocks.updateModule.mockResolvedValue({ id: MODULES_MODULE_ID });
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe("ModuleDateControl", () => {
  it("reads the pinned placeholders while both sides are empty", () => {
    renderControl();
    const trigger = screen.getByTestId("modules-row-dates-trigger");
    expect(trigger).toHaveTextContent("Start date End date");
    expect(trigger).toHaveAccessibleName("Module Delivery dates");
    expect(trigger).toBeEnabled();
  });

  it("reads the saved range as two day labels", () => {
    renderControl({ start_date: "2026-11-04", target_date: "2026-11-11" });
    expect(screen.getByTestId("modules-row-dates-trigger")).toHaveTextContent("Nov 04, 2026 Nov 11, 2026");
  });

  it("extends a lone start to the clicked day", async () => {
    renderControl({ start_date: "2026-11-04", target_date: null });
    const calendar = openCalendar();
    fireEvent.click(within(calendar).getByRole("button", { name: "November 11, 2026" }));
    await waitFor(() => {
      expect(mocks.updateModule).toHaveBeenCalledTimes(1);
    });
    expect(mocks.updateModule).toHaveBeenCalledWith(expect.anything(), "acme", MODULES_PROJECT_ID, MODULES_MODULE_ID, {
      start_date: "2026-11-04",
      target_date: "2026-11-11",
    });
    expect(mocks.notifySuccess).toHaveBeenCalledWith("Success!", "Module updated successfully.");
  });

  it("clears both sides when the lone day is clicked again", async () => {
    renderControl({ start_date: "2026-11-04", target_date: null });
    const calendar = openCalendar();
    fireEvent.click(within(calendar).getByRole("button", { name: "November 4, 2026" }));
    await waitFor(() => {
      expect(mocks.updateModule).toHaveBeenCalledTimes(1);
    });
    expect(mocks.updateModule).toHaveBeenCalledWith(expect.anything(), "acme", MODULES_PROJECT_ID, MODULES_MODULE_ID, {
      start_date: null,
      target_date: null,
    });
  });

  it("re-anchors a complete range to the clicked day", async () => {
    renderControl({ start_date: "2026-11-04", target_date: "2026-11-11" });
    const calendar = openCalendar();
    fireEvent.click(within(calendar).getByRole("button", { name: "November 20, 2026" }));
    await waitFor(() => {
      expect(mocks.updateModule).toHaveBeenCalledTimes(1);
    });
    expect(mocks.updateModule).toHaveBeenCalledWith(expect.anything(), "acme", MODULES_PROJECT_ID, MODULES_MODULE_ID, {
      start_date: "2026-11-20",
      target_date: null,
    });
  });

  it("stays shut while disabled", () => {
    renderControl({ start_date: null, target_date: null }, true);
    const trigger = screen.getByTestId("modules-row-dates-trigger");
    expect(trigger).toBeDisabled();
    fireEvent.click(trigger);
    expect(screen.queryByTestId("modules-row-dates-calendar")).not.toBeInTheDocument();
  });
});
