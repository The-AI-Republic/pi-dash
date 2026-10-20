// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Draft block property strip (parity/specs/issues.md §12, ISS-138): the
// inline pickers write one field each, conditional pickers follow the
// project's features, and the due day reads overdue.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import type { DraftIssue, ProjectDetails } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { DraftProperties, stateGroupOf } from "./DraftProperties.js";

const SLUG = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const STATE = "323e4567-e89b-12d3-a456-426614174002";
const DEFAULT_STATE = "423e4567-e89b-12d3-a456-426614174003";
const ESTIMATE = "823e4567-e89b-12d3-a456-426614174007";

const pickers = vi.hoisted(() => ({
  calls: [] as Array<Record<string, unknown>>,
}));

vi.mock("../../../shared/pickers/index.js", () => {
  const stub = (name: string) => (props: Record<string, unknown>) => {
    pickers.calls.push({ name, ...props });
    return <button type="button">{name}</button>;
  };
  return {
    CyclePicker: stub("cycle"),
    EstimatePicker: stub("estimate"),
    MemberPicker: stub("member"),
    ModulePicker: stub("module"),
    PriorityPicker: stub("priority"),
    StatePicker: stub("state"),
  };
});

vi.mock("../../../shared/pickers/reference.js", () => ({
  usePickerStates: vi.fn(),
  usePickerEstimates: vi.fn(),
}));

vi.mock("../dropdowns/DatePicker.js", () => ({
  DatePicker: (props: Record<string, unknown>) => {
    pickers.calls.push({ name: "date", ...props });
    return <button type="button">date</button>;
  },
}));

vi.mock("../labels/LabelPicker.js", () => ({
  LabelPicker: (props: Record<string, unknown>) => {
    pickers.calls.push({ name: "label", ...props });
    return <button type="button">label</button>;
  },
}));

vi.mock("../dropdowns/weekStart.js", () => ({ useWeekStart: () => 0 }));

import { usePickerEstimates, usePickerStates } from "../../../shared/pickers/reference.js";

const states = vi.mocked(usePickerStates);
const estimates = vi.mocked(usePickerEstimates);

function draft(overrides: Partial<DraftIssue> = {}): DraftIssue {
  return {
    id: "draft-1",
    name: "Half written",
    state_id: STATE,
    sort_order: 1,
    completed_at: null,
    priority: "high",
    start_date: "2026-02-01",
    target_date: "2020-01-01",
    project_id: PROJECT,
    parent_id: null,
    cycle_id: null,
    module_ids: [],
    label_ids: ["label-1"],
    assignee_ids: ["user-1"],
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    created_by: "user-1",
    updated_by: null,
    type_id: null,
    description_html: "",
    ...overrides,
  } as DraftIssue;
}

function project(overrides: Partial<ProjectDetails> = {}): ProjectDetails {
  return {
    id: PROJECT,
    name: "Atlas",
    identifier: "PAR",
    member_role: 20,
    module_view: false,
    cycle_view: false,
    estimate: null,
    ...overrides,
  } as unknown as ProjectDetails;
}

function stubClient(): void {
  createAppClient("https://api.test", (async (url: string) => {
    const body = url.endsWith("/auth/get-csrf-token/") ? { csrf_token: "csrf" } : {};
    return { status: 200, url, headers: { get: () => null }, text: async () => JSON.stringify(body) };
  }) as never);
}

function renderStrip(row = draft(), properties = project(), editable = true) {
  pickers.calls = [];
  const onWrite = vi.fn();
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const result = render(
    <QueryClientProvider client={queryClient}>
      <DraftProperties workspaceSlug={SLUG} draft={row} project={properties} editable={editable} onWrite={onWrite} />
    </QueryClientProvider>
  );
  return { ...result, onWrite, calls: pickers.calls };
}

afterEach(() => {
  resetAppClient();
});

describe("stateGroupOf", () => {
  it("finds the group of the draft's state", () => {
    expect(stateGroupOf([{ id: STATE, group: "completed" }], STATE)).toBe("completed");
  });

  it("answers null without a state or a read", () => {
    expect(stateGroupOf([{ id: STATE, group: "completed" }], null)).toBeNull();
    expect(stateGroupOf(undefined, STATE)).toBeNull();
    expect(stateGroupOf([{ id: "other", group: "started" }], STATE)).toBeNull();
  });
});

describe("DraftProperties", () => {
  it("renders the always-on strip and writes each pick as its own field", () => {
    states.mockReturnValue({ data: [{ id: STATE, group: "started" }] } as never);
    estimates.mockReturnValue({ data: [] } as never);
    stubClient();
    const { onWrite, calls } = renderStrip();
    const state = calls.find((call) => call.name === "state")!;
    (state["onChange"] as (id: string) => void)("state-2");
    expect(onWrite).toHaveBeenCalledWith({ state_id: "state-2" });
    const priority = calls.find((call) => call.name === "priority")!;
    (priority["onChange"] as (value: string) => void)("low");
    expect(onWrite).toHaveBeenCalledWith({ priority: "low" });
    const label = calls.find((call) => call.name === "label")!;
    (label["onChange"] as (ids: string[]) => void)(["label-2"]);
    expect(onWrite).toHaveBeenCalledWith({ label_ids: ["label-2"] });
    const member = calls.find((call) => call.name === "member")!;
    (member["onChange"] as (ids: string[]) => void)(["user-2"]);
    expect(onWrite).toHaveBeenCalledWith({ assignee_ids: ["user-2"] });
    const start = calls.find((call) => call.name === "date" && call.label === "Start date")!;
    (start["onChange"] as (date: string | null) => void)("2026-02-03");
    expect(onWrite).toHaveBeenCalledWith({ start_date: "2026-02-03" });
    const due = calls.find((call) => call.name === "date" && call.label === "Due date")!;
    (due["onChange"] as (date: string | null) => void)(null);
    expect(onWrite).toHaveBeenCalledWith({ target_date: null });
  });

  it("passes an unknown priority through as none", () => {
    states.mockReturnValue({ data: [] } as never);
    estimates.mockReturnValue({ data: [] } as never);
    stubClient();
    const { calls } = renderStrip(draft({ priority: "bogus" as never }));
    expect(calls.find((call) => call.name === "priority")?.["value"]).toBe("none");
  });

  it("adds module, cycle and estimate pickers only when the project enables them", () => {
    states.mockReturnValue({ data: [] } as never);
    estimates.mockReturnValue({ data: [{ id: ESTIMATE }] } as never);
    stubClient();
    const off = renderStrip();
    expect(off.calls.some((call) => call.name === "module")).toBe(false);
    expect(off.calls.some((call) => call.name === "cycle")).toBe(false);
    expect(off.calls.some((call) => call.name === "estimate")).toBe(false);
    off.unmount();
    const on = renderStrip(
      draft({ module_ids: ["module-1"], cycle_id: "cycle-1", estimate_point: ESTIMATE }),
      project({ module_view: true, cycle_view: true, estimate: ESTIMATE })
    );
    const module = on.calls.find((call) => call.name === "module")!;
    (module["onChange"] as (ids: string[]) => void)(["module-2"]);
    expect(on.onWrite).toHaveBeenCalledWith({ module_ids: ["module-2"] });
    const cycle = on.calls.find((call) => call.name === "cycle")!;
    (cycle["onChange"] as (id: string | null) => void)(null);
    expect(on.onWrite).toHaveBeenCalledWith({ cycle_id: null });
    const estimate = on.calls.find((call) => call.name === "estimate")!;
    expect(estimate["value"]).toBe(ESTIMATE);
    (estimate["onChange"] as (id: string | null) => void)("point-2");
    expect(on.onWrite).toHaveBeenCalledWith({ estimate_point: "point-2" });
  });

  it("hides the estimate picker without a project point scale or points", () => {
    states.mockReturnValue({ data: [] } as never);
    estimates.mockReturnValue({ data: [] } as never);
    stubClient();
    const { calls } = renderStrip(draft({ estimate_point: null }), project({ estimate: ESTIMATE }));
    expect(calls.some((call) => call.name === "estimate")).toBe(false);
  });

  it("reads a null estimate point as no value", () => {
    states.mockReturnValue({ data: [] } as never);
    estimates.mockReturnValue({ data: [{ id: ESTIMATE }] } as never);
    stubClient();
    const { calls } = renderStrip(draft({ estimate_point: null }), project({ estimate: ESTIMATE }));
    expect(calls.find((call) => call.name === "estimate")?.["value"]).toBeNull();
  });

  it("keeps every picker locked for a guest and for a project-less draft", () => {
    states.mockReturnValue({ data: [] } as never);
    estimates.mockReturnValue({ data: [] } as never);
    stubClient();
    const guest = renderStrip(draft(), project(), false);
    expect(guest.calls.every((call) => call["disabled"] === true)).toBe(true);
    guest.unmount();
    const homeless = renderStrip(draft({ project_id: null }), undefined);
    expect(homeless.calls.every((call) => call["disabled"] === true)).toBe(true);
  });

  it("reads the project default state while the draft holds none", () => {
    states.mockReturnValue({
      data: [
        { id: STATE, group: "unstarted" },
        { id: DEFAULT_STATE, group: "unstarted" },
      ],
    } as never);
    estimates.mockReturnValue({ data: [] } as never);
    stubClient();
    const { calls } = renderStrip(draft({ state_id: null }), project({ default_state: DEFAULT_STATE }));
    expect(calls.find((call) => call.name === "state")?.["value"]).toBe(DEFAULT_STATE);
  });

  it("keeps the overdue tone off an unset draft state even with a default", () => {
    states.mockReturnValue({ data: [{ id: DEFAULT_STATE, group: "unstarted" }] } as never);
    estimates.mockReturnValue({ data: [] } as never);
    stubClient();
    const { container } = renderStrip(
      draft({ state_id: null, target_date: "2000-01-01" }),
      project({ default_state: DEFAULT_STATE })
    );
    expect(container.querySelector(".text-\\(--danger\\)")).toBeNull();
  });

  it("flags an overdue due day in the danger colour", () => {
    states.mockReturnValue({ data: [{ id: STATE, group: "started" }] } as never);
    estimates.mockReturnValue({ data: [] } as never);
    stubClient();
    const { container } = renderStrip(draft({ target_date: "2000-01-01" }));
    expect(container.querySelector(".text-\\(--danger\\)")).not.toBeNull();
  });

  it("does not flag a completed draft's passed due day", () => {
    states.mockReturnValue({ data: [{ id: STATE, group: "completed" }] } as never);
    estimates.mockReturnValue({ data: [] } as never);
    stubClient();
    const { container } = renderStrip(draft({ target_date: "2000-01-01" }));
    expect(container.querySelector(".text-\\(--danger\\)")).toBeNull();
  });

  it("stops clicks and double-clicks from reaching the block's edit handler", () => {
    states.mockReturnValue({ data: [] } as never);
    estimates.mockReturnValue({ data: [] } as never);
    stubClient();
    const { getByTestId } = renderStrip();
    const strip = getByTestId("draft-properties");
    const click = new MouseEvent("click", { bubbles: true });
    const dblclick = new MouseEvent("dblclick", { bubbles: true });
    const clickSpy = vi.fn();
    const dblSpy = vi.fn();
    strip.addEventListener("click", clickSpy);
    strip.addEventListener("dblclick", dblSpy);
    strip.dispatchEvent(click);
    strip.dispatchEvent(dblclick);
    expect(clickSpy).toHaveBeenCalledTimes(1);
    expect(dblSpy).toHaveBeenCalledTimes(1);
  });

  it("appends the caller's class", () => {
    states.mockReturnValue({ data: [] } as never);
    estimates.mockReturnValue({ data: [] } as never);
    stubClient();
    pickers.calls = [];
    const queryClient = new QueryClient();
    render(
      <QueryClientProvider client={queryClient}>
        <DraftProperties
          workspaceSlug={SLUG}
          draft={draft()}
          project={project()}
          editable
          onWrite={vi.fn()}
          className="extra"
        />
      </QueryClientProvider>
    );
    expect(screen.getByTestId("draft-properties").className).toContain("extra");
  });
});
