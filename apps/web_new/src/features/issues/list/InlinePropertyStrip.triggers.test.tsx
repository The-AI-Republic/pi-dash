// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import type { IssueListItem, Label, State } from "@pidash/api-client";
import { fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

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

// The real priority picker stays: its popover merges trigger props into
// the strip's chip, which is exactly the path under test. The data-heavy
// pickers are inert trigger shells.
vi.mock("../../../shared/pickers/index.js", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../../shared/pickers/index.js")>();
  const shell = (props: { trigger?: React.ReactNode }): React.ReactElement => <>{props.trigger ?? null}</>;
  return {
    ...actual,
    StatePicker: shell,
    MemberPicker: shell,
    ModulePicker: shell,
    CyclePicker: shell,
    EstimatePicker: shell,
    usePickerEstimates: () => ({ data: undefined }),
  };
});

vi.mock("../dropdowns/DatePicker.js", () => ({
  DatePicker: (props: { trigger?: React.ReactNode }): React.ReactElement => <>{props.trigger ?? null}</>,
}));

vi.mock("../dropdowns/DateRangePicker.js", () => ({
  DateRangePicker: (props: { trigger?: React.ReactNode }): React.ReactElement => <>{props.trigger ?? null}</>,
}));

const LABEL_ROWS = vi.hoisted(() => ({
  rows: [] as Label[],
}));

vi.mock("../labels/api.js", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../labels/api.js")>();
  return {
    ...actual,
    useLabels: () => ({ data: LABEL_ROWS.rows, isPending: false }),
    useCreateLabel: () => ({ mutateAsync: vi.fn(), isPending: false }),
  };
});

vi.mock("../../auth/index.js", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../auth/index.js")>();
  return {
    ...actual,
    useProjectRoles: () => ({ data: { ["223e4567-e89b-12d3-a456-426614174001"]: 20 }, isError: false }),
    useWorkspaceMembership: () => ({ data: { role: 20 }, isError: false }),
  };
});

vi.mock("../../../shared/shell/index.js", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../../shared/shell/index.js")>();
  return { ...actual, notifyError: vi.fn() };
});

import { InlinePropertyStrip, type InlinePropertyStripProps } from "./InlinePropertyStrip.js";

const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const ISSUE = "123e4567-e89b-12d3-a456-426614174000";
const STATE = "323e4567-e89b-12d3-a456-426614174002";
const CYCLE = "423e4567-e89b-12d3-a456-426614174003";
const AID = "123e4567-e89b-12d3-a456-42661417400a";
const BID = "123e4567-e89b-12d3-a456-42661417400b";

function labelRow(id: string, name: string, sort: number): Label {
  return {
    id,
    project_id: PROJECT,
    workspace_id: "323e4567-e89b-12d3-a456-426614174002",
    name,
    color: "#FF6900",
    parent: null,
    sort_order: sort,
  };
}

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
    assigneeNames: new Map(),
    labelNames: new Map([
      [AID, "Alpha"],
      [BID, "Beta"],
    ]),
    cycleNames: new Map([[CYCLE, "Sprint 1"]]),
    moduleNames: new Map(),
    openCycleIds: new Set([CYCLE]),
    ...overrides,
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  LABEL_ROWS.rows = [labelRow(AID, "Alpha", 10000), labelRow(BID, "Beta", 20000)];
});

describe("InlinePropertyStrip priority trigger (NEWFRONT-935)", () => {
  it("opens the priority popup on click in grouped lists and writes the pick", () => {
    render(<InlinePropertyStrip {...props()} />);
    fireEvent.click(screen.getByRole("button", { name: "Priority: high" }));
    const listbox = screen.getByRole("listbox", { name: "Priorities" });
    fireEvent.click(within(listbox).getByRole("option", { name: "Low" }));
    expect(mocks.patch).toHaveBeenCalledWith({ priority: "low" });
    expect(screen.queryByRole("listbox", { name: "Priorities" })).not.toBeInTheDocument();
  });

  it("opens the priority popup from the keyboard in grouped lists", async () => {
    const user = userEvent.setup();
    render(<InlinePropertyStrip {...props()} />);
    screen.getByRole("button", { name: "Priority: high" }).focus();
    await user.keyboard("{Enter}");
    expect(screen.getByRole("listbox", { name: "Priorities" })).toBeInTheDocument();
  });

  it("opens the priority popup on Space in grouped lists", async () => {
    const user = userEvent.setup();
    render(<InlinePropertyStrip {...props()} />);
    screen.getByRole("button", { name: "Priority: high" }).focus();
    await user.keyboard(" ");
    expect(screen.getByRole("listbox", { name: "Priorities" })).toBeInTheDocument();
  });

  it("opens the priority popup on click in flat lists without a prior hover", () => {
    render(<InlinePropertyStrip {...props({ deferPickers: true })} />);
    fireEvent.click(screen.getByRole("button", { name: "Priority: high" }));
    expect(screen.getByRole("listbox", { name: "Priorities" })).toBeInTheDocument();
  });

  it("opens the priority popup from the keyboard in flat lists", () => {
    render(<InlinePropertyStrip {...props({ deferPickers: true })} />);
    fireEvent.keyDown(screen.getByRole("button", { name: "Priority: high" }), { key: "Enter" });
    expect(screen.getByRole("listbox", { name: "Priorities" })).toBeInTheDocument();
  });

  it("keeps a disabled priority chip closed in flat lists", () => {
    render(<InlinePropertyStrip {...props({ deferPickers: true, readOnly: true })} />);
    const chip = screen.getByRole("button", { name: "Priority: high" });
    expect(chip).toBeDisabled();
    fireEvent.click(chip);
    fireEvent.keyDown(chip, { key: "Enter" });
    expect(screen.queryByRole("listbox", { name: "Priorities" })).not.toBeInTheDocument();
  });
});

describe("InlinePropertyStrip labels trigger (NEWFRONT-935)", () => {
  it("opens the labels popup on click in grouped lists and toggles a label", async () => {
    const user = userEvent.setup();
    render(<InlinePropertyStrip {...props({ issue: issue({ label_ids: [AID] }) })} />);
    await user.click(screen.getByRole("button", { name: "Labels: Alpha" }));
    expect(screen.getByTestId("label-picker-search")).toBeInTheDocument();
    const options = screen.getAllByTestId("label-picker-option");
    expect(options).toHaveLength(2);
    await user.click(options[1]!);
    expect(mocks.patch).toHaveBeenCalledWith({ label_ids: [AID, BID] });
  });

  it("opens the labels popup from the keyboard in grouped lists", async () => {
    const user = userEvent.setup();
    render(<InlinePropertyStrip {...props({ issue: issue({ label_ids: [AID] }) })} />);
    screen.getByRole("button", { name: "Labels: Alpha" }).focus();
    await user.keyboard("{Enter}");
    expect(screen.getByTestId("label-picker-search")).toBeInTheDocument();
  });

  it("opens the labels popup on click in flat lists without a prior hover", () => {
    render(<InlinePropertyStrip {...props({ deferPickers: true, issue: issue({ label_ids: [AID] }) })} />);
    fireEvent.click(screen.getByRole("button", { name: "Labels: Alpha" }));
    expect(screen.getByTestId("label-picker-search")).toBeInTheDocument();
  });

  it("opens the labels popup from the keyboard in flat lists", () => {
    render(<InlinePropertyStrip {...props({ deferPickers: true, issue: issue({ label_ids: [AID] }) })} />);
    fireEvent.keyDown(screen.getByRole("button", { name: "Labels: Alpha" }), { key: "Enter" });
    expect(screen.getByTestId("label-picker-search")).toBeInTheDocument();
  });
});
