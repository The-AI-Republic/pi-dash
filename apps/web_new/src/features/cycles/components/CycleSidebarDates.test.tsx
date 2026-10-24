// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Sidebar inline dates: the span with its offset chip, the disabled
// read-only control, completed picks saving through the dialog's
// overlap gate, held one-sided picks, unchanged no-ops, overlap and
// write failures rendering inline beside the control, clearing, and
// the in-flight disabled state (CYC-036).

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { CycleRow } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../../../shared/shell/toaster.js", () => ({ notifyError: vi.fn(), notifySuccess: vi.fn() }));

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { notifyError, notifySuccess } from "../../../shared/shell/toaster.js";
import { CYCLES_CYCLE_ID, CYCLES_PROJECT_ID, jsonResponse, makeCycleRow } from "../testing.js";
import { CYCLE_DATES_UPDATED_TITLE, CycleSidebarDates } from "./CycleSidebarDates.js";

const WS = "acme";
const DETAIL_PATH = `/api/workspaces/${WS}/projects/${CYCLES_PROJECT_ID}/cycles/${CYCLES_CYCLE_ID}/`;
const CHECK_PATH = `/api/workspaces/${WS}/projects/${CYCLES_PROJECT_ID}/cycles/date-check/`;

interface Call {
  url: string;
  body: Record<string, unknown> | null;
}

let patchCalls: Call[];
let checkCalls: Call[];
let checkStatus: boolean;
let checkThrows: boolean;
let patchStatus: number;
let patchDeferred: { promise: Promise<void>; release: () => void } | null;

function deferPatch(): { promise: Promise<void>; release: () => void } {
  let release!: () => void;
  const promise = new Promise<void>((resolve) => {
    release = resolve;
  });
  return { promise, release };
}

/** Let a save that should never fire prove it never fires. */
async function settleNoSave(): Promise<void> {
  await screen.findByTestId("cycle-sidebar-dates-picker-calendar");
  await new Promise((resolve) => setTimeout(resolve, 20));
}

function shiftDay(base: string, delta: number): string {
  const at = new Date(`${base}T12:00:00Z`);
  at.setUTCDate(at.getUTCDate() + delta);
  return at.toISOString().slice(0, 10);
}

function todayUtc(): string {
  return new Date().toISOString().slice(0, 10);
}

function asInstant(day: string): string {
  return `${day}T12:00:00Z`;
}

function updateEnvelope(body: Record<string, unknown>): Record<string, unknown> {
  return {
    id: CYCLES_CYCLE_ID,
    name: "Cycle 7",
    project_id: CYCLES_PROJECT_ID,
    start_date: body["start_date"] === null ? null : asInstant(body["start_date"] as string),
    end_date: body["end_date"] === null ? null : asInstant(body["end_date"] as string),
  };
}

beforeEach(() => {
  patchCalls = [];
  checkCalls = [];
  checkStatus = true;
  checkThrows = false;
  patchStatus = 200;
  patchDeferred = null;
  createAppClient("https://api.test", (async (url: string, init: { method?: string; body?: string }) => {
    if (url.endsWith(DETAIL_PATH) && (init.method ?? "GET") === "PATCH") {
      const body = (init.body === undefined ? null : JSON.parse(init.body)) as Record<string, unknown> | null;
      patchCalls.push({ url, body });
      if (patchDeferred !== null) await patchDeferred.promise;
      if (patchStatus !== 200) return jsonResponse(url, patchStatus, { detail: "boom" });
      return jsonResponse(url, 200, updateEnvelope(body ?? {}));
    }
    if (url.endsWith(CHECK_PATH)) {
      const body = (init.body === undefined ? null : JSON.parse(init.body)) as Record<string, unknown> | null;
      checkCalls.push({ url, body });
      if (checkThrows) return jsonResponse(url, 500, { detail: "check blew up" });
      return jsonResponse(url, 200, { status: checkStatus });
    }
    throw new Error(`unexpected request: ${init.method ?? "GET"} ${url}`);
  }) as never);
});

afterEach(() => {
  resetAppClient();
  vi.restoreAllMocks();
  vi.mocked(notifyError).mockClear();
  vi.mocked(notifySuccess).mockClear();
});

function storedCycle(startDay: string | null, endDay: string | null): CycleRow {
  return makeCycleRow({
    start_date: startDay === null ? null : asInstant(startDay),
    end_date: endDay === null ? null : asInstant(endDay),
  });
}

function renderDates(cycle: CycleRow, editable = true, projectTimeZone = "UTC"): void {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <CycleSidebarDates
        workspaceSlug={WS}
        projectId={CYCLES_PROJECT_ID}
        cycle={cycle}
        projectTimeZone={projectTimeZone}
        viewerTimeZone="UTC"
        editable={editable}
      />
    </QueryClientProvider>
  );
}

async function openCalendar(): Promise<HTMLElement> {
  await userEvent.click(screen.getByTestId("cycle-sidebar-dates-trigger"));
  return screen.findByTestId("cycle-sidebar-dates-picker-calendar");
}

async function pickDay(calendar: HTMLElement, iso: string): Promise<void> {
  const cell = calendar.querySelector(`button[data-day="${iso}"]`);
  if (cell === null) throw new Error(`no cell for ${iso}`);
  await userEvent.click(cell as HTMLElement);
}

describe("CycleSidebarDates", () => {
  it("renders the span with the offset chip for members", async () => {
    const today = todayUtc();
    renderDates(storedCycle(shiftDay(today, 10), shiftDay(today, 30)), true, "America/New_York");
    const trigger = screen.getByTestId("cycle-sidebar-dates-trigger");
    expect(trigger).not.toHaveAttribute("disabled");
    expect(trigger.textContent ?? "").toMatch(/^[A-Z][a-z]{2} \d{2} - ([A-Z][a-z]{2} )?\d{2}, \d{4}$/);
    // The exact offset follows the anchor's DST side; the shape is the contract here.
    expect(screen.getByTestId("cycle-date-offset").textContent ?? "").toMatch(/^UTC [+-]\d{2}:\d{2}$/);
    expect(screen.queryByTestId("cycle-sidebar-dates-error")).toBeNull();
  });

  it("renders no chip for a zero-offset project", () => {
    const today = todayUtc();
    renderDates(storedCycle(shiftDay(today, 10), shiftDay(today, 30)));
    expect(screen.queryByTestId("cycle-date-offset")).toBeNull();
  });

  it("disables the control for read-only viewers", () => {
    const today = todayUtc();
    renderDates(storedCycle(shiftDay(today, 10), shiftDay(today, 30)), false);
    expect(screen.getByTestId("cycle-sidebar-dates-trigger")).toHaveAttribute("disabled");
    expect(screen.queryByTestId("cycle-sidebar-dates-picker-clear")).toBeNull();
  });

  it("disables past days like the dialog", async () => {
    const today = todayUtc();
    renderDates(storedCycle(shiftDay(today, 10), shiftDay(today, 30)));
    const calendar = await openCalendar();
    fireEvent.click(screen.getByRole("button", { name: "Previous month" }));
    const past = calendar.querySelector(`button[data-day="${shiftDay(today, -1)}"]`);
    expect(past?.getAttribute("disabled")).not.toBeNull();
  });

  it("saves a moved end through the overlap check", async () => {
    const today = todayUtc();
    const start = shiftDay(today, 10);
    const end = shiftDay(today, 30);
    const moved = shiftDay(today, 20);
    renderDates(storedCycle(start, end));
    const calendar = await openCalendar();
    await pickDay(calendar, moved);
    await waitFor(() => expect(patchCalls).toHaveLength(1));
    expect(checkCalls).toHaveLength(1);
    expect(checkCalls[0]!.body).toEqual({ start_date: start, end_date: moved, cycle_id: CYCLES_CYCLE_ID });
    expect(patchCalls[0]!.body).toEqual({ start_date: start, end_date: moved });
    expect(vi.mocked(notifySuccess)).toHaveBeenCalledWith(CYCLE_DATES_UPDATED_TITLE);
    expect(screen.queryByTestId("cycle-sidebar-dates-error")).toBeNull();
  });

  it("ignores a click on the settled end", async () => {
    const today = todayUtc();
    renderDates(storedCycle(shiftDay(today, 10), shiftDay(today, 30)));
    const calendar = await openCalendar();
    await pickDay(calendar, shiftDay(today, 30));
    await settleNoSave();
    expect(patchCalls).toHaveLength(0);
    expect(checkCalls).toHaveLength(0);
    expect(vi.mocked(notifySuccess)).not.toHaveBeenCalled();
  });

  it("holds the first pick on an undated cycle until the range completes", async () => {
    const today = todayUtc();
    renderDates(storedCycle(null, null));
    expect(screen.getByTestId("cycle-sidebar-dates-trigger").textContent).toBe("Start date");
    const calendar = await openCalendar();
    await pickDay(calendar, shiftDay(today, 5));
    await settleNoSave();
    expect(patchCalls).toHaveLength(0);
    expect(checkCalls).toHaveLength(0);
    await pickDay(calendar, shiftDay(today, 8));
    await waitFor(() => expect(patchCalls).toHaveLength(1));
    expect(checkCalls).toHaveLength(1);
    expect(patchCalls[0]!.body).toEqual({ start_date: shiftDay(today, 5), end_date: shiftDay(today, 8) });
    expect(vi.mocked(notifySuccess)).toHaveBeenCalledWith(CYCLE_DATES_UPDATED_TITLE);
  });

  it("refuses an overlap inline and by toast, and reverts the draft", async () => {
    checkStatus = false;
    const today = todayUtc();
    const start = shiftDay(today, 10);
    renderDates(storedCycle(start, shiftDay(today, 30)));
    const calendar = await openCalendar();
    await pickDay(calendar, shiftDay(today, 20));
    const alert = await screen.findByTestId("cycle-sidebar-dates-error");
    expect(alert.textContent ?? "").toContain("you already have a cycle");
    expect(vi.mocked(notifyError)).toHaveBeenCalledWith("Error!", expect.stringContaining("you already have a cycle"));
    expect(patchCalls).toHaveLength(0);
    // The revert re-bases the next pick onto the stored start.
    checkStatus = true;
    await pickDay(calendar, shiftDay(today, 25));
    await waitFor(() => expect(patchCalls).toHaveLength(1));
    expect(patchCalls[0]!.body).toEqual({ start_date: start, end_date: shiftDay(today, 25) });
    expect(screen.queryByTestId("cycle-sidebar-dates-error")).toBeNull();
  });

  it("reports a failed overlap check inline without saving", async () => {
    checkThrows = true;
    const today = todayUtc();
    renderDates(storedCycle(shiftDay(today, 10), shiftDay(today, 30)));
    const calendar = await openCalendar();
    await pickDay(calendar, shiftDay(today, 20));
    const alert = await screen.findByTestId("cycle-sidebar-dates-error");
    expect(alert.textContent).toBe("check blew up");
    expect(patchCalls).toHaveLength(0);
    expect(vi.mocked(notifySuccess)).not.toHaveBeenCalled();
  });

  it("reports a failed write inline without confirming", async () => {
    patchStatus = 500;
    const today = todayUtc();
    renderDates(storedCycle(shiftDay(today, 10), shiftDay(today, 30)));
    const calendar = await openCalendar();
    await pickDay(calendar, shiftDay(today, 20));
    const alert = await screen.findByTestId("cycle-sidebar-dates-error");
    expect(alert.textContent).toBe("boom");
    expect(checkCalls).toHaveLength(1);
    expect(vi.mocked(notifySuccess)).not.toHaveBeenCalled();
  });

  it("clears both dates without an overlap check", async () => {
    const today = todayUtc();
    renderDates(storedCycle(shiftDay(today, 10), shiftDay(today, 30)));
    await userEvent.click(screen.getByTestId("cycle-sidebar-dates-picker-clear"));
    await waitFor(() => expect(patchCalls).toHaveLength(1));
    expect(checkCalls).toHaveLength(0);
    expect(patchCalls[0]!.body).toEqual({ start_date: null, end_date: null });
    expect(vi.mocked(notifySuccess)).toHaveBeenCalledWith(CYCLE_DATES_UPDATED_TITLE);
  });

  it("disables the trigger while a save is in flight", async () => {
    const today = todayUtc();
    renderDates(storedCycle(shiftDay(today, 10), shiftDay(today, 30)));
    const calendar = await openCalendar();
    patchDeferred = deferPatch();
    await pickDay(calendar, shiftDay(today, 20));
    await waitFor(() => expect(screen.getByTestId("cycle-sidebar-dates-trigger")).toHaveAttribute("disabled"));
    patchDeferred.release();
    patchDeferred = null;
    await waitFor(() => expect(screen.getByTestId("cycle-sidebar-dates-trigger")).not.toHaveAttribute("disabled"));
    expect(vi.mocked(notifySuccess)).toHaveBeenCalledWith(CYCLE_DATES_UPDATED_TITLE);
  });

  it("re-bases the draft when the stored range changes", async () => {
    const today = todayUtc();
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const first = storedCycle(shiftDay(today, 10), shiftDay(today, 30));
    const rerendered = render(
      <QueryClientProvider client={client}>
        <CycleSidebarDates
          workspaceSlug={WS}
          projectId={CYCLES_PROJECT_ID}
          cycle={first}
          projectTimeZone="UTC"
          editable
        />
      </QueryClientProvider>
    );
    const moved = shiftDay(today, 12);
    rerendered.rerender(
      <QueryClientProvider client={client}>
        <CycleSidebarDates
          workspaceSlug={WS}
          projectId={CYCLES_PROJECT_ID}
          cycle={storedCycle(moved, shiftDay(today, 30))}
          projectTimeZone="UTC"
          editable
        />
      </QueryClientProvider>
    );
    const calendar = await openCalendar();
    await pickDay(calendar, shiftDay(today, 20));
    await waitFor(() => expect(patchCalls).toHaveLength(1));
    expect(patchCalls[0]!.body).toEqual({ start_date: moved, end_date: shiftDay(today, 20) });
  });
});
