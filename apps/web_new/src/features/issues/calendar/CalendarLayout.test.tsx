// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { IssueListItem } from "@pidash/api-client";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { FULL_SCREEN_PORTAL_ID } from "../../../shared/shell/index.js";
import { CalendarLayout } from "./CalendarLayout.js";
import { CALENDAR_ADD_EXISTING_ITEM, CALENDAR_ADD_ITEM, CALENDAR_TITLE_PLACEHOLDER } from "./CalendarDay.js";
import { monthTitle, stepMonth } from "./model.js";
import type { PrefsRecord } from "../prefs/entities.js";

const PROJECT_ID = "223e4567-e89b-12d3-a456-426614174001";
const ISSUE_ID = "323e4567-e89b-12d3-a456-426614174002";
const EXTRA_ID = "423e4567-e89b-12d3-a456-426614174003";
const THIRD_ID = "523e4567-e89b-12d3-a456-426614174004";
const SEARCH_ID = "623e4567-e89b-12d3-a456-426614174005";

const state = vi.hoisted(() => ({
  narrow: false,
  touch: false,
  prefs: null as PrefsRecord | null,
  updatePrefs: vi.fn(),
  windowRows: [] as IssueListItem[],
  dayRows: [] as IssueListItem[],
  dayRowsSecond: [] as IssueListItem[],
  searchHits: [] as unknown[],
  holdWindow: false,
  releaseWindow: null as (() => void) | null,
  calls: [] as { method: string; url: string }[],
  reDate: vi.fn(),
  create: vi.fn(),
  createContext: undefined as unknown,
  attach: vi.fn(),
  attachContext: undefined as unknown,
  remove: vi.fn(),
}));

vi.mock("./useMedia.js", () => ({
  useNarrowViewport: () => state.narrow,
  useTouchInput: () => state.touch,
}));

vi.mock("../dropdowns/weekStart.js", () => ({
  useWeekStart: () => 0,
}));

vi.mock("../prefs/displayPrefs.js", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../prefs/displayPrefs.js")>();
  return {
    ...actual,
    useEntityPrefs: () => ({ data: state.prefs }),
    useUpdateDisplayPrefs: () => ({ updateDisplayPrefs: state.updatePrefs }),
  };
});

vi.mock("./actions.js", () => ({
  useRedateIssue: () => ({ isSaving: false, reDate: state.reDate }),
  useCalendarCreate: (_slug: string, _projectId: string, context?: unknown) => {
    state.createContext = context;
    return { mutate: state.create, isPending: false };
  },
  useCalendarAttach: (_slug: string, _projectId: string, context?: unknown) => {
    state.attachContext = context;
    return { isPending: false, mutate: state.attach };
  },
  useCalendarDelete: () => ({ mutate: state.remove, isPending: false }),
}));

const today = new Date();
const todayIso = isoDay(today);
const nextIso = isoDay(shift(today, 1));
const emptyIso = isoDay(shift(today, 2));

function isoDay(date: Date): string {
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}-${String(date.getDate()).padStart(2, "0")}`;
}

function shift(date: Date, days: number): Date {
  return new Date(date.getFullYear(), date.getMonth(), date.getDate() + days);
}

function row(overrides: Partial<IssueListItem> = {}): IssueListItem {
  return {
    id: ISSUE_ID,
    name: "Fix login",
    state_id: null,
    sort_order: 1,
    completed_at: null,
    priority: "none",
    sequence_id: 42,
    project_id: PROJECT_ID,
    parent_id: null,
    cycle_id: null,
    module_ids: [],
    label_ids: [],
    assignee_ids: [],
    sub_issues_count: null,
    attachment_count: null,
    link_count: null,
    created_at: "2026-10-01T00:00:00Z",
    updated_at: "2026-10-01T00:00:00Z",
    created_by: null,
    updated_by: null,
    is_draft: false,
    archived_at: null,
    ...overrides,
  };
}

function prefs(mode: "month" | "week" = "month", showWeekends = true): PrefsRecord {
  return {
    filters: {},
    display_filters: { layout: "calendar", calendar: { layout: mode, show_weekends: showWeekends } },
    display_properties: {},
    rich_filters: {},
  };
}

function envelope(
  results: Record<string, { results: IssueListItem[]; total_results: number }>,
  options: { nextCursor?: string; nextPage?: boolean } = {}
) {
  return {
    grouped_by: "target_date",
    sub_grouped_by: null,
    total_count: 0,
    next_cursor: options.nextCursor ?? "",
    prev_cursor: "",
    next_page_results: options.nextPage ?? false,
    prev_page_results: false,
    count: 0,
    total_results: 0,
    results,
  };
}

function response(
  url: string,
  body: unknown
): Promise<{
  status: number;
  url: string;
  headers: { get: () => null };
  text: () => Promise<string>;
}> {
  return Promise.resolve({
    status: 200,
    url,
    headers: { get: () => null },
    text: async () => JSON.stringify(body),
  });
}

function installTransport(): void {
  createAppClient("https://api.test", (async (url: string, init: { method: string }) => {
    state.calls.push({ method: init.method, url });
    const parsed = new URL(url);
    if (parsed.pathname.endsWith("/states/")) {
      return response(url, [
        {
          id: "723e4567-e89b-12d3-a456-426614174006",
          project_id: PROJECT_ID,
          workspace_id: "823e4567-e89b-12d3-a456-426614174007",
          name: "Todo",
          color: "#00ff00",
          group: "unstarted",
          default: true,
          description: "",
          sequence: 1,
        },
      ]);
    }
    if (parsed.pathname.endsWith("/search-issues/")) return response(url, state.searchHits);
    if (parsed.pathname.endsWith("/issues/")) {
      const target = parsed.searchParams.get("target_date") ?? "";
      const [afterPart, beforePart] = target.split(",");
      const after = afterPart?.split(";")[0] ?? "";
      const before = beforePart?.split(";")[0] ?? "";
      const isDayRead = after === before && after !== "";
      if (isDayRead) {
        const cursor = parsed.searchParams.get("cursor");
        if (cursor === "4:2:0") {
          return response(url, envelope({ [todayIso]: { results: state.dayRowsSecond, total_results: 5 } }));
        }
        return response(
          url,
          envelope(
            { [todayIso]: { results: state.dayRows, total_results: 5 } },
            { nextCursor: "4:2:0", nextPage: true }
          )
        );
      }
      if (state.holdWindow) {
        return new Promise((resolve) => {
          state.releaseWindow = () =>
            resolve(response(url, envelope({ [todayIso]: { results: state.windowRows, total_results: 5 } })) as never);
        });
      }
      return response(url, envelope({ [todayIso]: { results: state.windowRows, total_results: 5 } }));
    }
    return response(url, {});
  }) as never);
}

function renderLayout(overrides: Partial<React.ComponentProps<typeof CalendarLayout>> = {}) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={queryClient}>
      <CalendarLayout
        workspaceSlug="acme"
        projectId={PROJECT_ID}
        projectIdentifier="WEB"
        entity={{ scope: "project", projectId: PROJECT_ID }}
        canEdit
        {...overrides}
      />
    </QueryClientProvider>
  );
}

function tile(iso: string): HTMLElement {
  return document.querySelector(`[data-calendar-day="${iso}"]`) as HTMLElement;
}

function pointerEvent(type: string, clientX: number, clientY: number): MouseEvent {
  return new MouseEvent(type, { bubbles: true, cancelable: true, clientX, clientY });
}

beforeEach(() => {
  state.narrow = false;
  state.touch = false;
  state.prefs = prefs();
  state.windowRows = [row({ target_date: todayIso })];
  state.dayRows = [row({ id: EXTRA_ID, name: "Extra item", sequence_id: 43, target_date: todayIso })];
  state.dayRowsSecond = [row({ id: THIRD_ID, name: "Third item", sequence_id: 44, target_date: todayIso })];
  state.searchHits = [
    {
      id: SEARCH_ID,
      name: "Existing item",
      start_date: null,
      sequence_id: 45,
      project__name: "Website",
      project__identifier: "WEB",
      project_id: PROJECT_ID,
      workspace__slug: "acme",
      state__name: "Todo",
      state__group: "unstarted",
      state__color: "#00ff00",
    },
  ];
  state.holdWindow = false;
  state.releaseWindow = null;
  state.calls = [];
  state.updatePrefs.mockReset();
  state.reDate.mockReset();
  state.create.mockReset();
  state.createContext = undefined;
  state.remove.mockReset();
  state.remove.mockImplementation((_id: string, options?: { onSuccess?: () => void }) => options?.onSuccess?.());
  state.attach.mockReset();
  state.attachContext = undefined;
  state.attach.mockImplementation((_input: unknown, options?: { onSuccess?: () => void }) => {
    options?.onSuccess?.();
  });
  installTransport();
  document.body.innerHTML = `<div id="${FULL_SCREEN_PORTAL_ID}"></div>`;
  window.history.replaceState({}, "", "/acme/projects/1/issues");
});

afterEach(() => {
  resetAppClient();
  vi.restoreAllMocks();
});

describe("CalendarLayout", () => {
  it("persists layout and weekend options", async () => {
    renderLayout();
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Options" }));
    await user.click(screen.getByRole("button", { name: /Week layout/ }));
    expect(state.updatePrefs).toHaveBeenNthCalledWith(1, {
      calendar: { mode: "week", showWeekends: true },
    });
    await user.click(screen.getByRole("button", { name: /Show weekends/ }));
    expect(state.updatePrefs).toHaveBeenNthCalledWith(2, {
      calendar: { mode: "month", showWeekends: false },
    });
  });

  it("does not write options before the preference record arrives", async () => {
    state.prefs = null;
    renderLayout();
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Options" }));
    await user.click(screen.getByRole("button", { name: /Week layout/ }));
    expect(state.updatePrefs).not.toHaveBeenCalled();
  });

  it("starts with five columns when the record has no weekend preference", async () => {
    state.prefs = { filters: {}, display_filters: {}, display_properties: {}, rich_filters: {} };
    renderLayout();
    await screen.findByTestId("issues-calendar");
    expect(screen.getByText("Mon")).toBeInTheDocument();
    expect(screen.queryByText("Sat")).not.toBeInTheDocument();
  });

  it("steps months, jumps to today, and picks a month", async () => {
    renderLayout();
    const user = userEvent.setup();
    const current = monthTitle(today.getFullYear(), today.getMonth());
    const previous = stepMonth(today.getFullYear(), today.getMonth(), -1);
    const previousTitle = monthTitle(previous.year, previous.month0);
    const before = state.calls.length;

    await user.click(screen.getByRole("button", { name: "Previous" }));
    expect(screen.getByRole("button", { name: previousTitle })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Next" }));
    expect(screen.getByRole("button", { name: current })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Next" }));
    await user.click(screen.getByRole("button", { name: "Today" }));
    expect(screen.getByRole("button", { name: current })).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: current }));
    await user.click(screen.getByRole("button", { name: "Jan" }));
    expect(screen.getByRole("button", { name: `January ${today.getFullYear()}` })).toBeInTheDocument();
    await waitFor(() => expect(state.calls.length).toBeGreaterThan(before));
  }, 15_000);

  it("steps weeks when week layout is stored", async () => {
    state.prefs = prefs("week");
    renderLayout();
    const user = userEvent.setup();
    const initialTitle = screen.getByRole("button", { name: /20\d\d/ }).textContent ?? "";
    const initialRange = state.calls
      .map((call) => new URL(call.url))
      .find((url) => url.pathname.endsWith("/issues/"))
      ?.searchParams.get("target_date");
    await user.click(screen.getByRole("button", { name: "Previous" }));
    await waitFor(() => {
      const ranges = state.calls
        .map((call) => new URL(call.url))
        .filter((url) => url.pathname.endsWith("/issues/"))
        .map((url) => url.searchParams.get("target_date"));
      expect(ranges).toContainEqual(expect.not.stringMatching(new RegExp(`^${initialRange}$`)));
    });
    await user.click(screen.getByRole("button", { name: "Today" }));
    expect(screen.getByRole("button", { name: initialTitle })).toBeInTheDocument();
  });

  it("re-dates on drop, highlights the block, and ignores a same-day drop", async () => {
    renderLayout();
    const block = await screen.findByRole("link", { name: "WEB-42" });
    fireEvent(block, pointerEvent("pointerdown", 10, 10));
    fireEvent(block, pointerEvent("pointermove", 30, 10));
    fireEvent.pointerEnter(tile(nextIso));
    fireEvent.pointerUp(tile(nextIso));
    expect(state.reDate).toHaveBeenCalledWith({
      issue: expect.objectContaining({ id: ISSUE_ID, target_date: todayIso }),
      destination: nextIso,
    });
    await waitFor(() => expect(block).toHaveAttribute("data-highlight", "true"));

    state.reDate.mockReset();
    fireEvent(block, pointerEvent("pointerdown", 10, 10));
    fireEvent(block, pointerEvent("pointermove", 30, 10));
    fireEvent.pointerEnter(tile(todayIso));
    fireEvent.pointerUp(tile(todayIso));
    expect(state.reDate).not.toHaveBeenCalled();

    fireEvent.pointerUp(tile(emptyIso));
    expect(state.reDate).not.toHaveBeenCalled();
  });

  it("sends a refused drop through the action hook", async () => {
    state.windowRows = [row({ target_date: emptyIso, start_date: nextIso })];
    renderLayout();
    const block = await screen.findByRole("link", { name: "WEB-42" });
    fireEvent(block, pointerEvent("pointerdown", 10, 10));
    fireEvent(block, pointerEvent("pointermove", 30, 10));
    fireEvent.pointerEnter(tile(todayIso));
    fireEvent.pointerUp(tile(todayIso));
    expect(state.reDate).toHaveBeenCalledWith({
      issue: expect.objectContaining({ id: ISSUE_ID, start_date: nextIso }),
      destination: todayIso,
    });
  });

  it("opens peek from a block and closes it with the URL parameter removed", async () => {
    renderLayout();
    const user = userEvent.setup();
    await user.click(await screen.findByRole("link", { name: "WEB-42" }));
    expect(screen.getByTestId("issue-preview")).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "Work item title" })).toHaveValue("Fix login");
    expect(window.location.search).toContain(`peekIssueId=${ISSUE_ID}`);
    expect(state.reDate).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "Close preview" }));
    expect(screen.queryByTestId("issue-preview")).not.toBeInTheDocument();
    expect(window.location.search).not.toContain("peekIssueId");
  });

  it("opens peek from the deep link and closes on Escape", async () => {
    window.history.replaceState({}, "", `/acme/projects/1/issues?peekIssueId=${ISSUE_ID}`);
    renderLayout();
    expect(await screen.findByRole("textbox", { name: "Work item title" })).toHaveValue("Fix login");
    fireEvent.keyDown(document, { key: "Escape" });
    expect(screen.queryByTestId("issue-preview")).not.toBeInTheDocument();
  });

  it("forwards contextual cycle and module membership to calendar writes", async () => {
    renderLayout({ membership: { cycleId: "823e4567-e89b-12d3-a456-426614174007" } });
    await screen.findByRole("link", { name: "WEB-42" });
    expect(state.createContext).toEqual({ cycleId: "823e4567-e89b-12d3-a456-426614174007" });
    expect(state.attachContext).toEqual({ cycleId: "823e4567-e89b-12d3-a456-426614174007" });
  });

  it("deletes and edits from block quick actions", async () => {
    const originalLocation = Object.getOwnPropertyDescriptor(window, "location");
    const location = { href: "http://localhost/acme/projects/1/issues" };
    Object.defineProperty(window, "location", { configurable: true, value: location });
    try {
      renderLayout();
      const block = await screen.findByRole("link", { name: "WEB-42" });
      fireEvent.click(within(block).getByRole("button", { name: "Quick actions" }));
      fireEvent.click(await screen.findByRole("menuitem", { name: "Delete" }));
      expect(state.remove).not.toHaveBeenCalled();
      expect(screen.getByRole("dialog", { name: /Delete work item/ })).toBeInTheDocument();
      fireEvent.click(screen.getByRole("button", { name: "Delete" }));
      expect(state.remove).toHaveBeenCalledWith(ISSUE_ID, expect.objectContaining({ onSuccess: expect.any(Function) }));

      await waitFor(() => expect(screen.queryByRole("dialog", { name: /Delete work item/ })).not.toBeInTheDocument());
      const blockAfterDelete = await screen.findByRole("link", { name: "WEB-42" });
      fireEvent.click(within(blockAfterDelete).getByRole("button", { name: "Quick actions" }));
      fireEvent.click(await screen.findByRole("menuitem", { name: "Edit" }));
      expect(location.href).toBe("/acme/browse/WEB-42");
    } finally {
      if (originalLocation !== undefined) Object.defineProperty(window, "location", originalLocation);
    }
  });

  it("quick-adds a dated item and opens the existing-item picker", async () => {
    renderLayout();
    const user = userEvent.setup();
    const dayTile = tile(todayIso);
    fireEvent.click(within(dayTile).getByText(CALENDAR_ADD_ITEM));
    fireEvent.click(await screen.findByRole("menuitem", { name: CALENDAR_ADD_ITEM }));
    const field = screen.getByPlaceholderText(CALENDAR_TITLE_PLACEHOLDER);
    await user.type(field, "New thing");
    fireEvent.keyDown(field, { key: "Enter" });
    expect(state.create).toHaveBeenCalledWith(
      { name: "New thing", day: todayIso },
      expect.objectContaining({ onSettled: expect.any(Function) })
    );

    fireEvent.click(within(dayTile).getByText(CALENDAR_ADD_ITEM));
    fireEvent.click(await screen.findByRole("menuitem", { name: CALENDAR_ADD_EXISTING_ITEM }));
    expect(await screen.findByText("Existing item")).toBeInTheDocument();
    state.windowRows = [
      row({ target_date: todayIso }),
      row({ id: SEARCH_ID, name: "Existing item", sequence_id: 45, target_date: todayIso }),
    ];
    await user.click(screen.getByText("Existing item"));
    await user.click(screen.getByRole("button", { name: "Add selected work items" }));
    expect(state.attach).toHaveBeenCalledWith(
      { issueIds: [SEARCH_ID], day: todayIso },
      expect.objectContaining({ onSuccess: expect.any(Function) })
    );
    await waitFor(() => expect(screen.queryByRole("checkbox")).not.toBeInTheDocument());
    await waitFor(() => expect(within(tile(todayIso)).getByText("Existing item")).toBeInTheDocument());
  });

  it("shows a pulsing temp block until the create settles", async () => {
    let settle: (() => void) | undefined;
    state.create.mockImplementation((_input: unknown, options?: { onSettled?: () => void }) => {
      settle = options?.onSettled;
    });
    renderLayout();
    const user = userEvent.setup();
    const dayTile = tile(emptyIso);
    fireEvent.click(within(dayTile).getByText(CALENDAR_ADD_ITEM));
    fireEvent.click(await screen.findByRole("menuitem", { name: CALENDAR_ADD_ITEM }));
    await user.type(screen.getByPlaceholderText(CALENDAR_TITLE_PLACEHOLDER), "Draft block");
    fireEvent.keyDown(screen.getByPlaceholderText(CALENDAR_TITLE_PLACEHOLDER), { key: "Enter" });

    const temp = await screen.findByRole("link", { name: "Draft block, saving" });
    expect(temp).toHaveAttribute("data-temprow", "true");
    expect(temp.querySelector(".animate-pulse")).not.toBeNull();
    expect(state.create).toHaveBeenCalledWith(
      { name: "Draft block", day: emptyIso },
      expect.objectContaining({ onSettled: expect.any(Function) })
    );

    await act(async () => settle?.());
    await waitFor(() => expect(screen.queryByRole("link", { name: "Draft block, saving" })).not.toBeInTheDocument());
  });

  it("closes the existing-item picker from Cancel", async () => {
    renderLayout();
    const user = userEvent.setup();
    fireEvent.click(within(tile(todayIso)).getByText(CALENDAR_ADD_ITEM));
    fireEvent.click(await screen.findByRole("menuitem", { name: CALENDAR_ADD_EXISTING_ITEM }));
    expect(await screen.findByText("Existing item")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(screen.queryByText("Existing item")).not.toBeInTheDocument());
  });

  it("hides quick actions and clears drag sources for read-only viewers", async () => {
    renderLayout({ canEdit: false });
    const block = await screen.findByRole("link", { name: "WEB-42" });
    expect(within(block).queryByRole("button", { name: "Quick actions" })).not.toBeInTheDocument();
    fireEvent(block, pointerEvent("pointerdown", 10, 10));
    fireEvent(block, pointerEvent("pointermove", 40, 40));
    fireEvent.pointerEnter(tile(nextIso));
    fireEvent.pointerUp(tile(nextIso));
    expect(state.reDate).not.toHaveBeenCalled();
  });

  it("keeps blocks visible but non-draggable on touch layouts", async () => {
    state.touch = true;
    renderLayout();
    const block = await screen.findByRole("link", { name: "WEB-42" });
    fireEvent(block, pointerEvent("pointerdown", 10, 10));
    fireEvent(block, pointerEvent("pointermove", 40, 40));
    fireEvent.pointerEnter(tile(nextIso));
    fireEvent.pointerUp(tile(nextIso));
    expect(state.reDate).not.toHaveBeenCalled();
  });

  it("pages a crowded day with the server cursor instead of skipping a page", async () => {
    renderLayout();
    const user = userEvent.setup();
    await user.click(await screen.findByRole("button", { name: "Load more" }));
    expect(await screen.findByText("Extra item")).toBeInTheDocument();
    const dayCalls = () =>
      state.calls
        .map((call) => new URL(call.url))
        .filter((url) => url.searchParams.get("target_date") === `${todayIso};after,${todayIso};before`);
    expect(dayCalls()[0]?.searchParams.get("cursor")).toBe("4:1:0");
    await user.click(screen.getByRole("button", { name: "Load more" }));
    expect(await screen.findByText("Third item")).toBeInTheDocument();
    expect(dayCalls()[1]?.searchParams.get("cursor")).toBe("4:2:0");
  });

  it("shows the compact day detail on touch and hides blocks", async () => {
    state.narrow = true;
    state.touch = true;
    renderLayout();
    const user = userEvent.setup();
    expect(screen.queryByRole("link", { name: "WEB-42" })).not.toBeInTheDocument();
    const face = tile(todayIso).querySelector("div.mx-auto.cursor-pointer") as HTMLElement;
    await user.click(face);
    const detail = screen.getByRole("region", { name: `Issues on ${todayIso}` });
    expect(within(detail).getByText("Fix login")).toBeInTheDocument();

    const emptyFace = tile(emptyIso).querySelector("div.mx-auto.cursor-pointer") as HTMLElement;
    await user.click(emptyFace);
    expect(screen.getByText("No work items on this day.")).toBeInTheDocument();
  });

  it("keeps blocks on a narrow pointer layout", async () => {
    state.narrow = true;
    state.touch = false;
    renderLayout();
    expect(await screen.findByRole("link", { name: "WEB-42" })).toBeInTheDocument();
    expect(screen.queryByRole("region", { name: /Issues on/ })).not.toBeInTheDocument();
  });

  it("shows the reload bar while the windowed read is in flight", async () => {
    state.holdWindow = true;
    renderLayout();
    expect(await screen.findByTestId("issues-calendar-reload-bar")).toBeInTheDocument();
    state.releaseWindow?.();
    await waitFor(() => expect(screen.queryByTestId("issues-calendar-reload-bar")).not.toBeInTheDocument());
  });
});
