// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { armedBulkProperties, BulkToolbar } from "./BulkToolbar.js";

vi.mock("../../../shared/pickers/index.js", () => ({
  StatePicker: ({ onChange }: { onChange: (id: string) => void }) => (
    <button type="button" onClick={() => onChange("state-1")}>
      Pick state
    </button>
  ),
  PriorityPicker: ({ onChange }: { onChange: (value: string) => void }) => (
    <button type="button" onClick={() => onChange("high")}>
      Pick priority
    </button>
  ),
  MemberPicker: ({ onChange }: { onChange: (ids: string[]) => void }) => (
    <button type="button" onClick={() => onChange(["user-1"])}>
      Pick member
    </button>
  ),
  CyclePicker: ({ onChange }: { onChange: (id: string) => void }) => (
    <button type="button" onClick={() => onChange("cycle-1")}>
      Pick cycle
    </button>
  ),
  ModulePicker: ({ onChange }: { onChange: (ids: string[]) => void }) => (
    <button type="button" onClick={() => onChange(["module-1"])}>
      Pick module
    </button>
  ),
  EstimatePicker: ({ onChange }: { onChange: (id: string) => void }) => (
    <button type="button" onClick={() => onChange("3")}>
      Pick estimate
    </button>
  ),
}));

vi.mock("../labels/LabelPicker.js", () => ({
  LabelPicker: ({ onChange }: { onChange: (ids: string[]) => void }) => (
    <button type="button" onClick={() => onChange(["label-1"])}>
      Pick label
    </button>
  ),
}));

vi.mock("../dropdowns/DatePicker.js", () => ({
  DatePicker: ({ label, onChange }: { label: string; onChange: (iso: string) => void }) => (
    <button type="button" onClick={() => onChange("2026-05-01")}>
      Pick {label}
    </button>
  ),
}));

function setup(overrides: Partial<React.ComponentProps<typeof BulkToolbar>> = {}) {
  const props = {
    workspaceSlug: "acme",
    projectId: "project-1",
    count: 3,
    hideCycle: false,
    hideModule: false,
    hideEstimate: false,
    busy: false,
    onClear: vi.fn(),
    onUpdate: vi.fn(),
    onDelete: vi.fn(),
    onArchive: vi.fn(),
    onSubscribe: vi.fn(),
    ...overrides,
  };
  render(<BulkToolbar {...props} />);
  return props;
}

describe("BulkToolbar", () => {
  it("calls subscribe, archive and clear directly", async () => {
    const user = userEvent.setup();
    const props = setup();
    await user.click(screen.getByRole("button", { name: /Subscribe/ }));
    await user.click(screen.getByRole("button", { name: /Archive/ }));
    await user.click(screen.getByRole("button", { name: "Clear selection" }));
    expect(props.onSubscribe).toHaveBeenCalledTimes(1);
    expect(props.onArchive).toHaveBeenCalledTimes(1);
    expect(props.onClear).toHaveBeenCalledTimes(1);
  });

  it("confirms before deleting the selection", async () => {
    const user = userEvent.setup();
    const props = setup();
    await user.click(screen.getByRole("button", { name: /Delete/ }));
    expect(props.onDelete).not.toHaveBeenCalled();
    expect(screen.getByRole("dialog", { name: "Delete selected work items?" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Delete" }));
    expect(props.onDelete).toHaveBeenCalledTimes(1);
  });

  it("sends only the armed update fields", async () => {
    const user = userEvent.setup();
    const props = setup();
    await user.click(screen.getByRole("button", { name: "Edit properties" }));
    await user.click(screen.getByRole("checkbox", { name: "State" }));
    await user.click(screen.getByRole("button", { name: "Pick state" }));
    await user.click(screen.getByRole("checkbox", { name: "Labels" }));
    await user.click(screen.getByRole("button", { name: "Pick label" }));
    await user.click(screen.getByRole("button", { name: "Apply" }));
    expect(props.onUpdate).toHaveBeenCalledWith({ state_id: "state-1", label_ids: ["label-1"] });
  });

  it("arms cycle, module and estimate as one payload", async () => {
    const user = userEvent.setup();
    const props = setup();
    await user.click(screen.getByRole("button", { name: "Edit properties" }));
    for (const name of ["Cycle", "Modules", "Estimate"]) {
      await user.click(screen.getByRole("checkbox", { name }));
    }
    await user.click(screen.getByRole("button", { name: "Pick cycle" }));
    await user.click(screen.getByRole("button", { name: "Pick module" }));
    await user.click(screen.getByRole("button", { name: "Pick estimate" }));
    await user.click(screen.getByRole("button", { name: "Apply" }));
    expect(props.onUpdate).toHaveBeenCalledWith({
      cycle_id: "cycle-1",
      module_ids: ["module-1"],
      estimate_point: "3",
    });
  });

  it("does not submit an armed state that has no value", async () => {
    const user = userEvent.setup();
    const props = setup();
    await user.click(screen.getByRole("button", { name: "Edit properties" }));
    await user.click(screen.getByRole("checkbox", { name: "State" }));
    await user.click(screen.getByRole("button", { name: "Apply" }));
    expect(props.onUpdate).not.toHaveBeenCalled();
  });

  it("refuses an empty form without calling update", async () => {
    const user = userEvent.setup();
    const props = setup();
    await user.click(screen.getByRole("button", { name: "Edit properties" }));
    expect(screen.getByRole("button", { name: "Apply" })).toBeDisabled();
    fireEvent.submit(screen.getByRole("button", { name: "Apply" }).closest("form") ?? document.body);
    expect(props.onUpdate).not.toHaveBeenCalled();
  });

  it("hides cycle, module and estimate when their project flags are off", () => {
    setup({ hideCycle: true, hideModule: true, hideEstimate: true });
    fireEvent.click(screen.getByRole("button", { name: "Edit properties" }));
    expect(screen.queryByRole("checkbox", { name: "Cycle" })).toBeNull();
    expect(screen.queryByRole("checkbox", { name: "Modules" })).toBeNull();
    expect(screen.queryByRole("checkbox", { name: "Estimate" })).toBeNull();
    expect(screen.getByRole("checkbox", { name: "State" })).toBeInTheDocument();
  });

  it("labels multiple selections and pluralizes the delete confirmation", async () => {
    const user = userEvent.setup();
    setup({ count: 2 });
    expect(screen.getByTestId("bulk-toolbar")).toHaveTextContent("2 selected");
    await user.click(screen.getByRole("button", { name: /Delete/ }));
    expect(screen.getByText("2 work items will be deleted.")).toBeInTheDocument();
  });

  it("uses singular copy for a one-item selection and delete", async () => {
    const user = userEvent.setup();
    setup({ count: 1 });
    expect(screen.getByTestId("bulk-toolbar")).toHaveTextContent("1 selected");
    await user.click(screen.getByRole("button", { name: /Delete/ }));
    expect(screen.getByText("1 work item will be deleted.")).toBeInTheDocument();
  });

  it("disables every action while a bulk write is pending", () => {
    setup({ busy: true });
    for (const name of [/Edit properties/, /Subscribe/, /Archive/, /Delete/]) {
      expect(screen.getByRole("button", { name })).toBeDisabled();
    }
  });

  it("sends only the date fields when only they are armed", async () => {
    const user = userEvent.setup();
    const props = setup();
    await user.click(screen.getByRole("button", { name: "Edit properties" }));
    await user.click(screen.getByRole("checkbox", { name: "Start date" }));
    await user.click(screen.getByRole("button", { name: "Pick Start date" }));
    await user.click(screen.getByRole("checkbox", { name: "Due date" }));
    await user.click(screen.getByRole("button", { name: "Pick Due date" }));
    await user.click(screen.getByRole("button", { name: "Apply" }));
    expect(props.onUpdate).toHaveBeenCalledWith({ start_date: "2026-05-01", target_date: "2026-05-01" });
  });
});

describe("bulk toolbar helpers", () => {
  it("builds a payload from armed fields only", () => {
    expect(
      armedBulkProperties(
        {
          state_id: false,
          priority: true,
          assignee_ids: true,
          label_ids: false,
          start_date: true,
          target_date: false,
          cycle_id: false,
          module_ids: false,
          estimate_point: false,
        },
        {
          stateId: "state-1",
          priority: "urgent",
          assigneeIds: ["u1"],
          labelIds: ["l1"],
          startDate: null,
          targetDate: "2026-06-01",
          cycleId: "c1",
          moduleIds: ["m1"],
          estimatePoint: "2",
        }
      )
    ).toEqual({ priority: "urgent", assignee_ids: ["u1"], start_date: null });
  });
});
