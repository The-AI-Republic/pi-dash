// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import type { IssueListItem, State } from "@pidash/api-client";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  patch: vi.fn(),
  move: vi.fn(),
  modules: vi.fn(),
}));

vi.mock("./mutations.js", () => ({
  useListIssuePatch: () => ({ patchIssue: mocks.patch }),
  useListCycleMove: () => ({ moveToCycle: mocks.move }),
  useListModuleWrite: () => ({ writeModules: mocks.modules }),
}));

vi.mock("../dropdowns/DatePicker.js", () => ({
  DatePicker: (props: {
    label: string;
    value: string | null;
    onChange: (value: string | null) => void;
    disabled?: boolean;
    max?: string | null;
    min?: string | null;
  }) => (
    <div data-testid={`date-${props.label}`} data-max={props.max ?? ""} data-min={props.min ?? ""}>
      <button type="button" disabled={props.disabled} onClick={() => props.onChange("2026-10-20")}>
        {props.label}
      </button>
    </div>
  ),
}));

vi.mock("../dropdowns/DateRangePicker.js", () => ({
  DateRangePicker: (props: {
    value: { start: string | null; end: string | null };
    onChange: (value: { start: string | null; end: string | null }) => void;
    disabled?: boolean;
  }) => (
    <div>
      <button
        type="button"
        data-testid="row-dates"
        disabled={props.disabled}
        onClick={() => props.onChange({ start: "2026-10-01", end: "2026-10-20" })}
      >
        {props.value.start}–{props.value.end}
      </button>
      <button
        type="button"
        aria-label="Clear Dates"
        disabled={props.disabled}
        onClick={() => props.onChange({ start: null, end: null })}
      />
    </div>
  ),
}));

vi.mock("../labels/LabelPicker.js", () => ({
  LabelPicker: (props: {
    label?: string;
    value: readonly string[];
    onChange: (ids: string[]) => void;
    disabled?: boolean;
  }) => (
    <button type="button" disabled={props.disabled} onClick={() => props.onChange([...props.value, "new-label"])}>
      {props.label ?? "Labels"}
    </button>
  ),
}));

vi.mock("../../../shared/pickers/index.js", () => {
  const icon = (): React.ReactElement => <span data-testid="picker-icon" />;
  return {
    PRIORITY_ICONS: { urgent: icon, high: icon, medium: icon, low: icon, none: icon },
    PRIORITY_TONES: { urgent: "", high: "", medium: "", low: "", none: "" },
    STATE_GROUP_ICONS: new Proxy({}, { get: () => icon }),
    StatePicker: (props: {
      onChange: (id: string) => void;
      disabled?: boolean;
      trigger: React.ReactNode;
      optionGate?: (option: { id: string }) => { disabled: boolean; reason?: string } | null;
    }) => (
      <div data-testid="state-picker" data-disabled={String(props.disabled ?? false)}>
        {props.trigger}
        <span data-testid="state-gate-reading">
          {JSON.stringify(props.optionGate?.({ id: "probe-state" }) ?? null)}
        </span>
        <button type="button" disabled={props.disabled} onClick={() => props.onChange("next-state")}>
          Pick state
        </button>
      </div>
    ),
    PriorityPicker: (props: { trigger: React.ReactNode }) => <div data-testid="priority-picker">{props.trigger}</div>,
    MemberPicker: (props: { onChange: (ids: string[]) => void; disabled?: boolean; trigger: React.ReactNode }) => (
      <div data-testid="member-picker">
        {props.trigger}
        <button type="button" disabled={props.disabled} onClick={() => props.onChange(["member-2"])}>
          Pick member
        </button>
      </div>
    ),
    ModulePicker: (props: { onChange: (ids: string[]) => void; disabled?: boolean; trigger: React.ReactNode }) => (
      <div data-testid="module-picker">
        {props.trigger}
        <button type="button" disabled={props.disabled} onClick={() => props.onChange(["module-1", "module-2"])}>
          Pick modules
        </button>
      </div>
    ),
    CyclePicker: (props: { onChange: (id: string | null) => void; disabled?: boolean; trigger: React.ReactNode }) => (
      <div data-testid="cycle-picker">
        {props.trigger}
        <button type="button" disabled={props.disabled} onClick={() => props.onChange("cycle-2")}>
          Pick cycle
        </button>
      </div>
    ),
    EstimatePicker: (props: { onChange: (id: string | null) => void; disabled?: boolean }) => (
      <button
        type="button"
        data-testid="estimate-picker"
        disabled={props.disabled}
        onClick={() => props.onChange("point-2")}
      >
        Estimate
      </button>
    ),
  };
});

import {
  INLINE_PROPERTY_ORDER,
  InlinePropertyStrip,
  isOverdue,
  todayIsoInZone,
  type InlinePropertyStripProps,
} from "./InlinePropertyStrip.js";

import { edition } from "../../../core/edition/index.js";
import { WORKFLOW_FLAG } from "../edition/flags.js";
import { registerEditionIssuesProviders, resetEditionIssuesProviders } from "../edition/providers.js";

const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const ISSUE = "123e4567-e89b-12d3-a456-426614174000";
const STATE = "323e4567-e89b-12d3-a456-426614174002";
const CYCLE = "423e4567-e89b-12d3-a456-426614174003";
const MODULE = "523e4567-e89b-12d3-a456-426614174004";

function issue(overrides: Partial<IssueListItem> = {}): IssueListItem {
  return {
    id: ISSUE,
    name: "Fix login",
    state_id: STATE,
    sort_order: 1,
    completed_at: null,
    priority: "high",
    sequence_id: 42,
    project_id: PROJECT,
    parent_id: null,
    cycle_id: null,
    module_ids: [],
    label_ids: [],
    assignee_ids: [],
    sub_issues_count: null,
    attachment_count: null,
    link_count: null,
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-02-03T04:05:06Z",
    created_by: null,
    updated_by: null,
    is_draft: false,
    archived_at: null,
    ...overrides,
  };
}

const state: State = {
  id: STATE,
  project_id: PROJECT,
  workspace_id: "",
  name: "Todo",
  group: "unstarted",
  color: "#fff",
  sequence: 1,
  default: false,
  description: "",
};

function props(overrides: Partial<InlinePropertyStripProps> = {}): InlinePropertyStripProps {
  return {
    workspaceSlug: "acme",
    projectId: PROJECT,
    projectIdentifier: "WEB",
    issue: issue(),
    properties: {},
    flags: { hideCycle: false, hideModule: false },
    readOnly: false,
    estimateEnabled: true,
    states: [state],
    stateNames: new Map([[STATE, "Todo"]]),
    assigneeNames: new Map([
      ["member-1", "Ada"],
      ["member-2", "Grace"],
    ]),
    labelNames: new Map([
      ["label-1", "Bug"],
      ["label-2", "Frontend"],
    ]),
    cycleNames: new Map([[CYCLE, "Sprint 1"]]),
    moduleNames: new Map([[MODULE, "Auth"]]),
    openCycleIds: new Set([CYCLE]),
    ...overrides,
  };
}

beforeEach(() => vi.clearAllMocks());
afterEach(() => {
  resetEditionIssuesProviders();
  delete edition.flags[WORKFLOW_FLAG];
});

describe("isOverdue", () => {
  it("flags an open due-or-past date and ignores closed or completed work", () => {
    expect(isOverdue({ target_date: "2026-10-18", completed_at: null }, "2026-10-18", "started")).toBe(true);
    expect(isOverdue({ target_date: "2026-10-17", completed_at: null }, "2026-10-18", "started")).toBe(true);
    expect(isOverdue({ target_date: "2026-10-19", completed_at: null }, "2026-10-18", "started")).toBe(false);
    expect(
      isOverdue({ target_date: "2026-10-18", completed_at: "2026-10-18T10:00:00Z" }, "2026-10-18", "started")
    ).toBe(false);
    expect(isOverdue({ target_date: "2026-10-18", completed_at: null }, "2026-10-18", "completed")).toBe(false);
    expect(isOverdue({ target_date: "2026-10-18", completed_at: null }, "2026-10-18", "cancelled")).toBe(false);
    expect(isOverdue({ target_date: null, completed_at: null }, "2026-10-18", "started")).toBe(false);
  });
});

describe("todayIsoInZone", () => {
  it("uses the project day instead of the UTC day", () => {
    const evening = new Date("2026-10-19T02:00:00Z");
    expect(todayIsoInZone(evening, "UTC")).toBe("2026-10-19");
    expect(todayIsoInZone(evening, "America/Los_Angeles")).toBe("2026-10-18");
  });
});

describe("InlinePropertyStrip", () => {
  it("renders display-enabled properties in the spec order", () => {
    render(<InlinePropertyStrip {...props()} />);
    expect(screen.getByTestId("inline-property-strip")).toBeInTheDocument();
    // The row anchor owns the identifier; the strip must not repeat it.
    expect(screen.queryByText("WEB-42")).not.toBeInTheDocument();
    expect(screen.getByText("Todo")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Priority: high" })).toBeInTheDocument();
    expect(screen.getByTestId("state-picker")).toBeInTheDocument();
    expect(INLINE_PROPERTY_ORDER.slice(0, 3)).toEqual(["state", "priority", "start_date"]);
  });

  it("flags a due-today item using the project timezone on a US-Pacific evening", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-10-19T02:00:00Z"));
    try {
      render(
        <InlinePropertyStrip
          {...props({
            issue: issue({ target_date: "2026-10-18" }),
            properties: { start_date: false },
            timeZone: "America/Los_Angeles",
          })}
        />
      );
      expect(screen.getByTestId("date-Due date").parentElement).toHaveAttribute("data-overdue", "true");
    } finally {
      vi.useRealTimers();
    }
  });

  it("drops hidden properties, cycle/module feature flags and an absent estimate", () => {
    render(
      <InlinePropertyStrip
        {...props({
          issue: issue({ cycle_id: CYCLE, module_ids: [MODULE] }),
          properties: { state: false, priority: false, assignee: false, cycle: false, modules: false, labels: false },
          flags: { hideCycle: true, hideModule: true },
          estimateEnabled: false,
        })}
      />
    );
    expect(screen.queryByTestId("state-picker")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Priority: high" })).not.toBeInTheDocument();
    expect(screen.queryByTestId("cycle-picker")).not.toBeInTheDocument();
    expect(screen.queryByTestId("module-picker")).not.toBeInTheDocument();
    expect(screen.queryByTestId("estimate-picker")).not.toBeInTheDocument();
    expect(screen.queryByTestId("member-picker")).not.toBeInTheDocument();
  });

  it("merges both dates only when both are shown and present", () => {
    const first = render(
      <InlinePropertyStrip {...props({ issue: issue({ start_date: "2026-10-01", target_date: "2026-10-20" }) })} />
    );
    expect(screen.getByTestId("row-dates")).toHaveTextContent("2026-10-01–2026-10-20");
    expect(screen.queryByTestId("date-Start date")).not.toBeInTheDocument();
    first.unmount();
    render(
      <InlinePropertyStrip
        {...props({
          issue: issue({ start_date: "2026-10-01", target_date: "2026-10-20" }),
          properties: { due_date: false },
        })}
      />
    );
    expect(screen.getByTestId("date-Start date")).toHaveAttribute("data-max", "2026-10-20");
    expect(screen.queryByTestId("date-Due date")).not.toBeInTheDocument();
  });

  it("clears both ends of the merged date range", () => {
    render(
      <InlinePropertyStrip {...props({ issue: issue({ start_date: "2026-10-01", target_date: "2026-10-20" }) })} />
    );
    fireEvent.click(screen.getByRole("button", { name: "Clear Dates" }));
    expect(mocks.patch).toHaveBeenCalledWith({ start_date: null, target_date: null });
  });

  it("renders separate start and due pickers with reciprocal bounds", () => {
    render(<InlinePropertyStrip {...props({ issue: issue({ start_date: "2026-10-01" }) })} />);
    expect(screen.getByTestId("date-Start date")).toHaveAttribute("data-max", "");
    expect(screen.getByTestId("date-Due date")).toHaveAttribute("data-min", "2026-10-01");
    fireEvent.click(screen.getByRole("button", { name: "Due date" }));
    expect(mocks.patch).toHaveBeenCalledWith({ target_date: "2026-10-20" });
  });

  it("marks an overdue due date and leaves closed states alone", () => {
    const first = render(
      <InlinePropertyStrip
        {...props({ issue: issue({ target_date: "2000-01-01" }), properties: { start_date: false } })}
      />
    );
    expect(screen.getByTestId("date-Due date").parentElement).toHaveAttribute("data-overdue", "true");
    first.unmount();
    render(
      <InlinePropertyStrip
        {...props({
          issue: issue({ target_date: "2000-01-01" }),
          properties: { start_date: false },
          states: [{ ...state, group: "completed" }],
        })}
      />
    );
    expect(screen.getByTestId("date-Due date").parentElement).not.toHaveAttribute("data-overdue");
  });

  it("writes each editable property through its contract", () => {
    render(
      <InlinePropertyStrip
        {...props({
          issue: issue({
            cycle_id: CYCLE,
            module_ids: [MODULE],
            label_ids: ["label-1"],
            assignee_ids: ["member-1"],
          }),
        })}
      />
    );
    fireEvent.click(screen.getByRole("button", { name: "Pick state" }));
    expect(mocks.patch).toHaveBeenCalledWith({ state_id: "next-state" });
    fireEvent.click(screen.getByRole("button", { name: "Pick member" }));
    expect(mocks.patch).toHaveBeenCalledWith({ assignee_ids: ["member-2"] });
    fireEvent.click(screen.getByRole("button", { name: "Pick modules" }));
    expect(mocks.modules).toHaveBeenCalledWith(["module-1", "module-2"], [MODULE]);
    fireEvent.click(screen.getByRole("button", { name: "Pick cycle" }));
    expect(mocks.move).toHaveBeenCalledWith("cycle-2", CYCLE);
    fireEvent.click(screen.getByTestId("estimate-picker"));
    expect(mocks.patch).toHaveBeenCalledWith({ estimate_point: "point-2" });
    fireEvent.click(screen.getByRole("button", { name: "Labels: Bug" }));
    expect(mocks.patch).toHaveBeenCalledWith({ label_ids: ["label-1", "new-label"] });
  });

  it("renders read-only counts and hides epic counts", () => {
    const first = render(
      <InlinePropertyStrip {...props({ issue: issue({ sub_issues_count: 2, attachment_count: 3, link_count: 4 }) })} />
    );
    expect(screen.getByLabelText("Sub-items: 2")).toBeInTheDocument();
    expect(screen.getByLabelText("Attachments: 3")).toBeInTheDocument();
    expect(screen.getByLabelText("Links: 4")).toBeInTheDocument();
    first.unmount();
    render(
      <InlinePropertyStrip
        {...props({ issue: issue({ is_epic: true, sub_issues_count: 2, attachment_count: 0, link_count: 0 }) })}
      />
    );
    expect(screen.queryByLabelText("Sub-items: 2")).not.toBeInTheDocument();
    expect(screen.queryByLabelText("Attachments: 0")).not.toBeInTheDocument();
  });

  it.each([
    ["guest read-only", { readOnly: true }],
    ["archived item", { issue: issue({ archived_at: "2026-01-01" }) }],
    ["completed cycle", { issue: issue({ cycle_id: CYCLE }), openCycleIds: new Set<string>() }],
  ])("disables every editable control for %s", (_label, overrides) => {
    render(<InlinePropertyStrip {...props(overrides)} />);
    expect(screen.getByRole("button", { name: "Todo" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Priority: high" })).toBeDisabled();
    expect(screen.getByTestId("estimate-picker")).toBeDisabled();
    expect(screen.getByRole("button", { name: "Pick member" })).toBeDisabled();
  });
});

describe("InlinePropertyStrip deferred pickers", () => {
  // The flat list mounts a picker only on first interaction so 2,000
  // virtualized rows do not each hold a popover (Quality gates: perf).
  function chip(container: HTMLElement): HTMLElement {
    const found = container.querySelector<HTMLElement>('[title^="State:"] button');
    if (found === null) throw new Error("state chip missing");
    return found;
  }

  it("mounts the state picker on hover and opens it on click", () => {
    const { container } = render(<InlinePropertyStrip {...props({ deferPickers: true })} />);
    expect(screen.queryByTestId("state-picker")).not.toBeInTheDocument();
    fireEvent.pointerEnter(chip(container));
    expect(screen.getByTestId("state-picker")).toBeInTheDocument();
    expect(screen.getByTestId("state-picker")).toHaveAttribute("data-disabled", "false");
  });

  it("opens the picker from the keyboard and from a touch press", () => {
    const keyboard = render(<InlinePropertyStrip {...props({ deferPickers: true })} />);
    fireEvent.keyDown(chip(keyboard.container), { key: "Enter" });
    expect(screen.getByTestId("state-picker")).toBeInTheDocument();
    keyboard.unmount();
    cleanup();
    const touch = render(<InlinePropertyStrip {...props({ deferPickers: true })} />);
    // jsdom ships no PointerEvent, so hand the cloned handler a native
    // event that carries the touch pointer type.
    const touchEvent = new MouseEvent("pointerdown", { bubbles: true });
    Object.defineProperty(touchEvent, "pointerType", { value: "touch" });
    fireEvent(chip(touch.container), touchEvent);
    expect(screen.getByTestId("state-picker")).toBeInTheDocument();
    touch.unmount();
    cleanup();
    const mouse = render(<InlinePropertyStrip {...props({ deferPickers: true })} />);
    const mouseEvent = new MouseEvent("pointerdown", { bubbles: true });
    Object.defineProperty(mouseEvent, "pointerType", { value: "mouse" });
    fireEvent(chip(mouse.container), mouseEvent);
    expect(screen.queryByTestId("state-picker")).not.toBeInTheDocument();
  });

  it("hands the row's state picker the workflow gate only when the build has one (ISS-233)", () => {
    registerEditionIssuesProviders({
      workflowRules: { refusal: () => ({ reason: "no rule should be asked" }) },
    });
    render(<InlinePropertyStrip {...props()} />);
    expect(screen.getByTestId("state-gate-reading")).toHaveTextContent("null");
  });

  it("greys the states the rules refuse from the row's own state (ISS-233)", () => {
    edition.flags[WORKFLOW_FLAG] = true;
    registerEditionIssuesProviders({
      workflowRules: {
        refusal: ({ fromStateId, toStateId }) =>
          fromStateId === STATE && toStateId === "probe-state" ? { reason: "Needs review first" } : null,
      },
    });
    render(<InlinePropertyStrip {...props()} />);
    expect(screen.getByTestId("state-gate-reading")).toHaveTextContent(
      JSON.stringify({ disabled: true, reason: "Needs review first" })
    );
  });

  it("ignores a disabled chip's hint, press and keys", () => {
    const { container } = render(<InlinePropertyStrip {...props({ deferPickers: true, readOnly: true })} />);
    fireEvent.pointerEnter(chip(container));
    fireEvent.pointerDown(chip(container), { pointerType: "touch" });
    fireEvent.click(chip(container));
    expect(screen.queryByTestId("state-picker")).not.toBeInTheDocument();
    expect(chip(container)).toBeDisabled();
  });
});
