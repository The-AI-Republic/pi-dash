// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The cycle save dialog in both modes: create opens on defaults with the
// project picker first in the DOM, update opens prefilled without it;
// title validation, past-start disabling, the overlap check gate, the
// remembered all-tab, shared-read invalidation, and Escape/pending
// behavior (CYC-014–021).

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { CycleRow } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const notify = vi.hoisted(() => ({ notifyError: vi.fn(), notifySuccess: vi.fn() }));
vi.mock("../../../shared/shell/index.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../shared/shell/index.js")>()),
  notifyError: notify.notifyError,
  notifySuccess: notify.notifySuccess,
}));

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { cycleKeys } from "../api/keys.js";
import { projectToday } from "../lib/cycleDates.js";
import { CYCLES_CYCLE_ID, CYCLES_PROJECT_ID, CYCLES_WS_ID, makeCycleRow, makeProjectDetails } from "../testing.js";
import { CreateCycleDialog, UpdateCycleDialog } from "./CycleDialog.js";

const PROJECT_B_ID = "423e4567-e89b-12d3-a456-426614174003";
const CREATED_ID = "523e4567-e89b-12d3-a456-426614174004";

interface Call {
  method: string;
  url: string;
  body: Record<string, unknown> | null;
}

interface Harness {
  roles: Record<string, number>;
  cycleViewB: boolean;
  dateCheckStatus: boolean;
  dateCheckCalls: Call[];
  createCalls: Call[];
  patchCalls: Call[];
  createStatus: number;
  createBody: unknown;
  patchStatus: number;
  gateDateCheck: (() => void) | null;
}

let harness: Harness;

function resetHarness(): Harness {
  return {
    roles: { [CYCLES_PROJECT_ID]: 15, [PROJECT_B_ID]: 15 },
    cycleViewB: true,
    dateCheckStatus: true,
    dateCheckCalls: [],
    createCalls: [],
    patchCalls: [],
    createStatus: 201,
    createBody: null,
    patchStatus: 200,
    gateDateCheck: null,
  };
}

/** A request day as the stored instant the server answers. */
function asInstant(day: unknown): string | null {
  return typeof day === "string" && day !== "" ? `${day}T12:00:00Z` : null;
}

function projectListRow(id: string, name: string, identifier: string): Record<string, unknown> {
  return {
    id,
    name,
    identifier,
    sort_order: null,
    logo_props: {},
    member_role: 15,
    intake_count: 0,
    archived_at: null,
    workspace: CYCLES_WS_ID,
    cycle_view: true,
    issue_views_view: true,
    module_view: true,
    page_view: false,
    inbox_view: false,
    is_default: false,
    guest_view_all_features: false,
    project_lead: null,
    network: 0,
    created_at: "2026-01-02T03:04:05Z",
    updated_at: "2026-02-03T04:05:06Z",
    created_by: null,
    updated_by: null,
  };
}

beforeEach(() => {
  harness = resetHarness();
  window.localStorage.clear();
  createAppClient("https://api.test", (async (url: string, init: { method?: string; body?: string }) => {
    if (url.endsWith("/auth/get-csrf-token/")) {
      return { status: 200, url, headers: { get: () => null }, text: async () => '{"csrf_token":"csrf"}' };
    }
    const call: Call = {
      method: init.method ?? "GET",
      url,
      body: init.body === undefined ? null : (JSON.parse(init.body) as Record<string, unknown>),
    };
    if (url === "https://api.test/api/workspaces/acme/projects/") {
      return {
        status: 200,
        url,
        headers: { get: () => null },
        text: async () =>
          JSON.stringify([
            projectListRow(CYCLES_PROJECT_ID, "Website relaunch", "WEB"),
            projectListRow(PROJECT_B_ID, "Project B", "PB"),
          ]),
      };
    }
    if (url.endsWith("/api/workspaces/acme/projects/details/")) {
      return {
        status: 200,
        url,
        headers: { get: () => null },
        text: async () =>
          JSON.stringify([
            makeProjectDetails(),
            makeProjectDetails({
              id: PROJECT_B_ID,
              name: "Project B",
              identifier: "PB",
              cycle_view: harness.cycleViewB,
            }),
          ]),
      };
    }
    if (url.endsWith("/project-roles/")) {
      return { status: 200, url, headers: { get: () => null }, text: async () => JSON.stringify(harness.roles) };
    }
    if (url.includes("/cycles/date-check/")) {
      harness.dateCheckCalls.push(call);
      if (harness.gateDateCheck !== null) await new Promise<void>((release) => (harness.gateDateCheck = release));
      return {
        status: 200,
        url,
        headers: { get: () => null },
        text: async () => JSON.stringify({ status: harness.dateCheckStatus }),
      };
    }
    if (call.method === "POST" && url.includes("/cycles/")) {
      harness.createCalls.push(call);
      // The server stores datetimes: bare-day request days come back as instants.
      const echo = call.body ?? {};
      const body =
        harness.createBody !== null
          ? harness.createBody
          : makeCycleRow({
              id: CREATED_ID,
              name: typeof echo["name"] === "string" ? echo["name"] : "Cycle 7",
              description: typeof echo["description"] === "string" ? echo["description"] : "",
              start_date: asInstant(echo["start_date"]),
              end_date: asInstant(echo["end_date"]),
            });
      return {
        status: harness.createStatus,
        url,
        headers: { get: () => null },
        text: async () => JSON.stringify(body),
      };
    }
    if (call.method === "PATCH") {
      harness.patchCalls.push(call);
      const echo = call.body ?? {};
      return {
        status: harness.patchStatus,
        url,
        headers: { get: () => null },
        text: async () =>
          JSON.stringify({
            id: CYCLES_CYCLE_ID,
            name: typeof echo["name"] === "string" ? echo["name"] : "Cycle 7",
            project_id: CYCLES_PROJECT_ID,
            description: typeof echo["description"] === "string" ? echo["description"] : "",
            start_date: asInstant(echo["start_date"]),
            end_date: asInstant(echo["end_date"]),
          }),
      };
    }
    throw new Error(`unexpected request: ${call.method} ${url}`);
  }) as never);
});

afterEach(() => {
  resetAppClient();
  notify.notifyError.mockClear();
  notify.notifySuccess.mockClear();
});

const CURRENT_PROJECT = { id: CYCLES_PROJECT_ID, name: "Website relaunch", identifier: "WEB" };

function renderCreate(options: { open?: boolean; onOpenChange?: (open: boolean) => void } = {}): {
  onOpenChange: ReturnType<typeof vi.fn>;
  queryClient: QueryClient;
  rerenderOpen: (open: boolean) => void;
} {
  const onOpenChange = (options.onOpenChange ?? vi.fn()) as ReturnType<typeof vi.fn>;
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const ui = (open: boolean): React.ReactElement => (
    <QueryClientProvider client={queryClient}>
      <CreateCycleDialog
        open={open}
        onOpenChange={onOpenChange}
        workspaceSlug="acme"
        projectId={CYCLES_PROJECT_ID}
        currentProject={CURRENT_PROJECT}
      />
    </QueryClientProvider>
  );
  const view = render(ui(options.open ?? true));
  return { onOpenChange, queryClient, rerenderOpen: (open: boolean) => view.rerender(ui(open)) };
}

function renderUpdate(
  cycle: CycleRow,
  options: { open?: boolean; onOpenChange?: (open: boolean) => void } = {}
): { onOpenChange: ReturnType<typeof vi.fn>; queryClient: QueryClient } {
  const onOpenChange = (options.onOpenChange ?? vi.fn()) as ReturnType<typeof vi.fn>;
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <UpdateCycleDialog
        open={options.open ?? true}
        onOpenChange={onOpenChange}
        workspaceSlug="acme"
        projectId={CYCLES_PROJECT_ID}
        currentProject={CURRENT_PROJECT}
        cycle={cycle}
      />
    </QueryClientProvider>
  );
  return { onOpenChange, queryClient };
}

/** Shift an ISO day by whole days (UTC date math, independent of the dialog). */
function shiftDay(iso: string, delta: number): string {
  const [year, month, day] = iso.split("-").map(Number);
  const at = new Date(Date.UTC(year as number, (month as number) - 1, day as number));
  at.setUTCDate(at.getUTCDate() + delta);
  return at.toISOString().slice(0, 10);
}

/** The local calendar day of an instant (independent of the dialog). */
function localDayOf(instant: string): string {
  const at = new Date(instant);
  return `${at.getFullYear()}-${String(at.getMonth() + 1).padStart(2, "0")}-${String(at.getDate()).padStart(2, "0")}`;
}

async function openCalendar(): Promise<HTMLElement> {
  await screen.findByRole("heading", { name: "Create cycle" });
  const trigger = screen.getByTestId("cycle-date-range-trigger");
  const user = userEvent.setup();
  await user.click(trigger);
  return screen.findByTestId("cycle-date-range-calendar");
}

async function pickIsoDay(calendar: HTMLElement, iso: string): Promise<void> {
  const cell = calendar.querySelector(`button[data-day="${iso}"]`);
  expect(cell).not.toBeNull();
  await userEvent.setup().click(cell as Element);
}

describe("CreateCycleDialog form", () => {
  it("opens with the pinned fields, names and placeholders", async () => {
    renderCreate();
    expect(await screen.findByRole("heading", { name: "Create cycle" })).toBeInTheDocument();
    const title = screen.getByPlaceholderText("Title");
    expect(title).toHaveAttribute("name", "name");
    expect(title).toHaveValue("");
    const description = screen.getByPlaceholderText("Description");
    expect(description).toHaveAttribute("name", "description");
    expect(description).toHaveValue("");
    expect(screen.getByTestId("cycle-date-range-trigger")).toHaveTextContent("Start date");
    expect(screen.getByTestId("cycle-date-range-trigger")).toHaveTextContent("End date");
    expect(screen.getByRole("button", { name: "Cancel" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Create cycle" })).toHaveAttribute("type", "submit");
  });

  it("focuses the title on open", async () => {
    renderCreate();
    expect(await screen.findByPlaceholderText("Title")).toHaveFocus();
  });

  it("mounts closed without focusing anything", () => {
    renderCreate({ open: false });
    expect(screen.queryByRole("heading", { name: "Create cycle" })).not.toBeInTheDocument();
    expect(document.activeElement).toBe(document.body);
  });

  it("renders the project picker ahead of the title in the DOM", async () => {
    renderCreate();
    await screen.findByRole("heading", { name: "Create cycle" });
    const field = screen.getByTestId("cycle-project-field");
    const title = screen.getByPlaceholderText("Title");
    expect(field.compareDocumentPosition(title) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(within(field).getByRole("button")).toHaveTextContent("Website relaunch");
  });

  it("requires a title on submit and refocuses it", async () => {
    const user = userEvent.setup();
    renderCreate();
    await screen.findByRole("heading", { name: "Create cycle" });
    await user.click(screen.getByRole("button", { name: "Create cycle" }));
    expect(screen.getByRole("alert")).toHaveTextContent("Title is required");
    expect(screen.getByPlaceholderText("Title")).toHaveFocus();
    expect(harness.createCalls).toHaveLength(0);
  });

  it("validates the title on blur", async () => {
    const user = userEvent.setup();
    renderCreate();
    const title = await screen.findByPlaceholderText("Title");
    await user.click(title);
    await user.tab();
    expect(screen.getByRole("alert")).toHaveTextContent("Title is required");
  });

  it("caps the title at 255 characters and clears the error while retyping", async () => {
    const user = userEvent.setup();
    renderCreate();
    await screen.findByRole("heading", { name: "Create cycle" });
    await user.type(screen.getByPlaceholderText("Title"), "x".repeat(256));
    await user.click(screen.getByRole("button", { name: "Create cycle" }));
    expect(screen.getByRole("alert").textContent).toContain("255");
    expect(harness.createCalls).toHaveLength(0);
    await user.clear(screen.getByPlaceholderText("Title"));
    await user.type(screen.getByPlaceholderText("Title"), "Cycle 7");
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("disables the past in the start picker while today stays enabled", async () => {
    renderCreate();
    const calendar = await openCalendar();
    const today = projectToday("UTC");
    const past = calendar.querySelector(`button[data-day="${shiftDay(today, -1)}"]`);
    const present = calendar.querySelector(`button[data-day="${today}"]`);
    expect(past).not.toBeNull();
    expect(past).toBeDisabled();
    expect(present).not.toBeNull();
    expect(present).toBeEnabled();
  });

  it("Escape closes without saving", async () => {
    const user = userEvent.setup();
    const { onOpenChange } = renderCreate();
    await screen.findByRole("heading", { name: "Create cycle" });
    await user.type(screen.getByPlaceholderText("Title"), "Never saved");
    await user.keyboard("{Escape}");
    expect(onOpenChange).toHaveBeenCalledWith(false);
    expect(harness.createCalls).toHaveLength(0);
  });

  it("Cancel closes without saving", async () => {
    const user = userEvent.setup();
    const { onOpenChange } = renderCreate();
    await screen.findByRole("heading", { name: "Create cycle" });
    await user.type(screen.getByPlaceholderText("Title"), "Never saved");
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onOpenChange).toHaveBeenCalledWith(false);
    expect(harness.createCalls).toHaveLength(0);
  });

  it("reopening resets the form to defaults", async () => {
    const user = userEvent.setup();
    const { rerenderOpen } = renderCreate();
    await screen.findByRole("heading", { name: "Create cycle" });
    await user.type(screen.getByPlaceholderText("Title"), "Typed");
    rerenderOpen(false);
    expect(screen.queryByRole("heading", { name: "Create cycle" })).not.toBeInTheDocument();
    rerenderOpen(true);
    expect(await screen.findByPlaceholderText("Title")).toHaveValue("");
  });
});

describe("CreateCycleDialog submit", () => {
  it("saves an undated draft without a date check, remembers the all-tab and refreshes the shared reads", async () => {
    const user = userEvent.setup();
    const { onOpenChange, queryClient } = renderCreate();
    const invalidated: { queryKey: readonly unknown[] }[] = [];
    const spy = vi
      .spyOn(queryClient, "invalidateQueries")
      .mockImplementation(async (options?: { queryKey?: readonly unknown[] }) => {
        if (options?.queryKey !== undefined) invalidated.push({ queryKey: options.queryKey });
      });
    try {
      await screen.findByRole("heading", { name: "Create cycle" });
      await user.type(screen.getByPlaceholderText("Title"), "Cycle 7");
      await user.type(screen.getByPlaceholderText("Description"), "Seeded from the test");
      await user.click(screen.getByRole("button", { name: "Create cycle" }));
      await waitFor(() => expect(notify.notifySuccess).toHaveBeenCalledWith("Cycle created successfully."));
      expect(onOpenChange).toHaveBeenCalledWith(false);
      expect(harness.dateCheckCalls).toHaveLength(0);
      expect(harness.createCalls).toHaveLength(1);
      expect(harness.createCalls[0]?.url).toContain(`/projects/${CYCLES_PROJECT_ID}/cycles/`);
      expect(harness.createCalls[0]?.body).toEqual({
        name: "Cycle 7",
        description: "Seeded from the test",
        start_date: null,
        end_date: null,
      });
      expect(window.localStorage.getItem("cycle_tab")).toBe('"all"');
      const keys = invalidated.map((entry) => entry.queryKey);
      expect(keys).toContainEqual(cycleKeys.list("acme", CYCLES_PROJECT_ID));
      expect(keys).toContainEqual(cycleKeys.hero("acme", CYCLES_PROJECT_ID));
      expect(keys).toContainEqual(cycleKeys.detail("acme", CYCLES_PROJECT_ID, CREATED_ID));
    } finally {
      spy.mockRestore();
    }
  });

  it("saves a one-sided range without a date check", async () => {
    const user = userEvent.setup();
    renderCreate();
    const calendar = await openCalendar();
    const start = shiftDay(projectToday("UTC"), 5);
    await pickIsoDay(calendar, start);
    await user.type(screen.getByPlaceholderText("Title"), "One-sided");
    await user.click(screen.getByRole("button", { name: "Create cycle" }));
    await waitFor(() => expect(notify.notifySuccess).toHaveBeenCalled());
    expect(harness.dateCheckCalls).toHaveLength(0);
    expect(harness.createCalls[0]?.body).toEqual({
      name: "One-sided",
      description: "",
      start_date: start,
      end_date: null,
    });
  });

  it("checks a two-sided range once per submit, without a cycle id", async () => {
    const user = userEvent.setup();
    renderCreate();
    const calendar = await openCalendar();
    const today = projectToday("UTC");
    await pickIsoDay(calendar, shiftDay(today, 5));
    await pickIsoDay(calendar, shiftDay(today, 10));
    await user.type(screen.getByPlaceholderText("Title"), "Dated");
    await user.click(screen.getByRole("button", { name: "Create cycle" }));
    await waitFor(() => expect(notify.notifySuccess).toHaveBeenCalled());
    expect(harness.dateCheckCalls).toHaveLength(1);
    expect(harness.dateCheckCalls[0]?.body).toEqual({
      start_date: shiftDay(today, 5),
      end_date: shiftDay(today, 10),
    });
    expect(harness.createCalls).toHaveLength(1);
  });

  it("refuses an overlapping range with the draft workaround and keeps the dialog open", async () => {
    const user = userEvent.setup();
    harness.dateCheckStatus = false;
    const { onOpenChange } = renderCreate();
    const calendar = await openCalendar();
    const today = projectToday("UTC");
    await pickIsoDay(calendar, shiftDay(today, 5));
    await pickIsoDay(calendar, shiftDay(today, 10));
    await user.type(screen.getByPlaceholderText("Title"), "Overlapping");
    await user.click(screen.getByRole("button", { name: "Create cycle" }));
    await waitFor(() => expect(notify.notifyError).toHaveBeenCalled());
    expect(notify.notifyError).toHaveBeenCalledWith("Error!", expect.stringContaining("you already have a cycle"));
    expect(notify.notifyError).toHaveBeenCalledWith("Error!", expect.stringMatching(/draft/i));
    expect(screen.getByRole("heading", { name: "Create cycle" })).toBeInTheDocument();
    expect(screen.getByPlaceholderText("Title")).toHaveValue("Overlapping");
    expect(harness.createCalls).toHaveLength(0);
    expect(onOpenChange).not.toHaveBeenCalled();
  });

  it("retargets the check and the create to the picked project", async () => {
    const user = userEvent.setup();
    renderCreate();
    await screen.findByRole("heading", { name: "Create cycle" });
    await user.click(within(screen.getByTestId("cycle-project-field")).getByRole("button"));
    await user.click(await screen.findByRole("option", { name: "Project B" }));
    expect(within(screen.getByTestId("cycle-project-field")).getByRole("button")).toHaveTextContent("Project B");
    const calendar = await openCalendar();
    const today = projectToday("UTC");
    await pickIsoDay(calendar, shiftDay(today, 5));
    await pickIsoDay(calendar, shiftDay(today, 10));
    await user.type(screen.getByPlaceholderText("Title"), "Moved");
    await user.click(screen.getByRole("button", { name: "Create cycle" }));
    await waitFor(() => expect(notify.notifySuccess).toHaveBeenCalled());
    expect(harness.dateCheckCalls).toHaveLength(1);
    expect(harness.dateCheckCalls[0]?.url).toContain(`/projects/${PROJECT_B_ID}/cycles/date-check/`);
    expect(harness.createCalls).toHaveLength(1);
    expect(harness.createCalls[0]?.url).toContain(`/projects/${PROJECT_B_ID}/cycles/`);
  });

  it("offers only projects where the viewer may create cycles", async () => {
    const user = userEvent.setup();
    harness.roles = { [CYCLES_PROJECT_ID]: 15, [PROJECT_B_ID]: 5 };
    renderCreate();
    await screen.findByRole("heading", { name: "Create cycle" });
    await user.click(within(screen.getByTestId("cycle-project-field")).getByRole("button"));
    expect(await screen.findByRole("option", { name: "Website relaunch" })).toBeInTheDocument();
    expect(screen.queryByRole("option", { name: "Project B" })).not.toBeInTheDocument();
  });

  it("blocks creation into a project with cycles turned off", async () => {
    const user = userEvent.setup();
    harness.cycleViewB = false;
    renderCreate();
    await screen.findByRole("heading", { name: "Create cycle" });
    await user.click(within(screen.getByTestId("cycle-project-field")).getByRole("button"));
    await user.click(await screen.findByRole("option", { name: "Project B" }));
    await user.type(screen.getByPlaceholderText("Title"), "Blocked");
    await user.click(screen.getByRole("button", { name: "Create cycle" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Cycles are turned off for this project.");
    expect(harness.dateCheckCalls).toHaveLength(0);
    expect(harness.createCalls).toHaveLength(0);
  });

  it("shows a pending label, blocks outside presses, and still settles after an Escape", async () => {
    const user = userEvent.setup();
    harness.gateDateCheck = () => undefined;
    const { onOpenChange } = renderCreate();
    const calendar = await openCalendar();
    const today = projectToday("UTC");
    await pickIsoDay(calendar, shiftDay(today, 5));
    await pickIsoDay(calendar, shiftDay(today, 10));
    await user.type(screen.getByPlaceholderText("Title"), "Pending");
    await user.click(screen.getByRole("button", { name: "Create cycle" }));
    const pending = await screen.findByRole("button", { name: "Creating cycle…" });
    expect(pending).toBeDisabled();
    expect(screen.getByPlaceholderText("Title")).toBeDisabled();
    await user.click(document.body);
    expect(onOpenChange).not.toHaveBeenCalled();
    const release = harness.gateDateCheck;
    expect(release).not.toBeNull();
    await user.keyboard("{Escape}");
    expect(onOpenChange).toHaveBeenCalledWith(false);
    release?.();
    await waitFor(() => expect(notify.notifySuccess).toHaveBeenCalledWith("Cycle created successfully."));
  });

  it("shows a server validation error in the dialog with the input intact", async () => {
    const user = userEvent.setup();
    harness.createStatus = 400;
    harness.createBody = { detail: "Window taken." };
    renderCreate();
    await screen.findByRole("heading", { name: "Create cycle" });
    await user.type(screen.getByPlaceholderText("Title"), "Refused");
    await user.click(screen.getByRole("button", { name: "Create cycle" }));
    expect(await screen.findByTestId("cycle-dialog-error")).toHaveTextContent("Window taken.");
    expect(screen.getByRole("heading", { name: "Create cycle" })).toBeInTheDocument();
    expect(screen.getByPlaceholderText("Title")).toHaveValue("Refused");
    expect(screen.getByRole("button", { name: "Create cycle" })).toBeEnabled();
  });

  it("falls back to the fixed copy when the failure carries no detail", async () => {
    const user = userEvent.setup();
    harness.createStatus = 500;
    harness.createBody = {};
    renderCreate();
    await screen.findByRole("heading", { name: "Create cycle" });
    await user.type(screen.getByPlaceholderText("Title"), "Broken");
    await user.click(screen.getByRole("button", { name: "Create cycle" }));
    expect(await screen.findByTestId("cycle-dialog-error")).toHaveTextContent(
      "Nothing was created — check the details and try again."
    );
  });
});

describe("UpdateCycleDialog", () => {
  function datedRecord(): CycleRow {
    const today = projectToday("UTC");
    return makeCycleRow({
      description: "Seeded description",
      start_date: `${shiftDay(today, 5)}T12:00:00Z`,
      end_date: `${shiftDay(today, 10)}T12:00:00Z`,
    });
  }

  it("prefills from the record and hides the project picker", async () => {
    const record = datedRecord();
    renderUpdate(record);
    expect(await screen.findByRole("heading", { name: "Update cycle" })).toBeInTheDocument();
    expect(screen.getByPlaceholderText("Title")).toHaveValue("Cycle 7");
    expect(screen.getByPlaceholderText("Description")).toHaveValue("Seeded description");
    expect(screen.queryByTestId("cycle-project-field")).not.toBeInTheDocument();
    expect(screen.getByPlaceholderText("Title")).toHaveFocus();
    const trigger = screen.getByTestId("cycle-date-range-trigger");
    await userEvent.setup().click(trigger);
    const calendar = await screen.findByTestId("cycle-date-range-calendar");
    const pressed = [...calendar.querySelectorAll('button[aria-pressed="true"]')].map((cell) =>
      cell.getAttribute("data-day")
    );
    expect(pressed).toHaveLength(2);
    expect(pressed).toContain(localDayOf(record.start_date as string));
    expect(pressed).toContain(localDayOf(record.end_date as string));
    expect(trigger).not.toHaveTextContent("Start date");
  });

  it("saves untouched dates without a date check", async () => {
    const user = userEvent.setup();
    const { onOpenChange } = renderUpdate(datedRecord());
    await screen.findByRole("heading", { name: "Update cycle" });
    await user.clear(screen.getByPlaceholderText("Title"));
    await user.type(screen.getByPlaceholderText("Title"), "Renamed");
    await user.click(screen.getByRole("button", { name: "Update cycle" }));
    await waitFor(() => expect(notify.notifySuccess).toHaveBeenCalledWith("Cycle updated successfully."));
    expect(onOpenChange).toHaveBeenCalledWith(false);
    expect(harness.dateCheckCalls).toHaveLength(0);
    expect(harness.patchCalls).toHaveLength(1);
    expect(harness.patchCalls[0]?.url).toContain(`/cycles/${CYCLES_CYCLE_ID}/`);
  });

  it("re-checks changed dates with the cycle id and patches the new range", async () => {
    const user = userEvent.setup();
    const record = datedRecord();
    renderUpdate(record);
    await screen.findByRole("heading", { name: "Update cycle" });
    await user.click(screen.getByTestId("cycle-date-range-trigger"));
    const calendar = await screen.findByTestId("cycle-date-range-calendar");
    const moved = shiftDay(projectToday("UTC"), 12);
    await pickIsoDay(calendar, moved);
    await user.click(screen.getByRole("button", { name: "Update cycle" }));
    await waitFor(() => expect(notify.notifySuccess).toHaveBeenCalledWith("Cycle updated successfully."));
    expect(harness.dateCheckCalls).toHaveLength(1);
    expect(harness.dateCheckCalls[0]?.body).toEqual({
      start_date: localDayOf(record.start_date as string),
      end_date: moved,
      cycle_id: CYCLES_CYCLE_ID,
    });
    expect(harness.patchCalls[0]?.body).toMatchObject({ end_date: moved });
  });

  it("saves cleared dates without a date check", async () => {
    const user = userEvent.setup();
    renderUpdate(datedRecord());
    await screen.findByRole("heading", { name: "Update cycle" });
    await user.click(screen.getByRole("button", { name: "Clear Start date End date" }));
    await user.click(screen.getByRole("button", { name: "Update cycle" }));
    await waitFor(() => expect(notify.notifySuccess).toHaveBeenCalled());
    expect(harness.dateCheckCalls).toHaveLength(0);
    expect(harness.patchCalls[0]?.body).toMatchObject({ start_date: null, end_date: null });
  });

  it("refuses an overlapping edit with the draft workaround and keeps the dialog open", async () => {
    const user = userEvent.setup();
    harness.dateCheckStatus = false;
    const { onOpenChange } = renderUpdate(datedRecord());
    await screen.findByRole("heading", { name: "Update cycle" });
    await user.click(screen.getByTestId("cycle-date-range-trigger"));
    const calendar = await screen.findByTestId("cycle-date-range-calendar");
    await pickIsoDay(calendar, shiftDay(projectToday("UTC"), 12));
    await user.click(screen.getByRole("button", { name: "Update cycle" }));
    await waitFor(() => expect(notify.notifyError).toHaveBeenCalled());
    expect(notify.notifyError).toHaveBeenCalledWith("Error!", expect.stringContaining("you already have a cycle"));
    expect(notify.notifyError).toHaveBeenCalledWith("Error!", expect.stringMatching(/draft/i));
    expect(screen.getByRole("heading", { name: "Update cycle" })).toBeInTheDocument();
    expect(harness.patchCalls).toHaveLength(0);
    expect(onOpenChange).not.toHaveBeenCalled();
  });

  it("saves an undated record without a date check", async () => {
    const user = userEvent.setup();
    renderUpdate(makeCycleRow({ description: undefined, start_date: null, end_date: null }));
    await screen.findByRole("heading", { name: "Update cycle" });
    expect(screen.getByPlaceholderText("Description")).toHaveValue("");
    expect(screen.getByTestId("cycle-date-range-trigger")).toHaveTextContent("Start date");
    await user.click(screen.getByRole("button", { name: "Update cycle" }));
    await waitFor(() => expect(notify.notifySuccess).toHaveBeenCalled());
    expect(harness.dateCheckCalls).toHaveLength(0);
    expect(harness.patchCalls).toHaveLength(1);
  });

  it("requires a title and shows update failures inline", async () => {
    const user = userEvent.setup();
    harness.patchStatus = 500;
    renderUpdate(datedRecord());
    await screen.findByRole("heading", { name: "Update cycle" });
    await user.clear(screen.getByPlaceholderText("Title"));
    await user.click(screen.getByRole("button", { name: "Update cycle" }));
    expect(screen.getByRole("alert")).toHaveTextContent("Title is required");
    expect(harness.patchCalls).toHaveLength(0);
    await user.type(screen.getByPlaceholderText("Title"), "Renamed");
    await user.click(screen.getByRole("button", { name: "Update cycle" }));
    expect(await screen.findByTestId("cycle-dialog-error")).toHaveTextContent(
      "Nothing was changed — check the details and try again."
    );
    expect(screen.getByRole("button", { name: "Update cycle" })).toBeEnabled();
  });
});
