// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The module's add surface (MOD-031): the header button mounts only for an
// editing standing and opens the shared creation dialog locked to the
// module; its close control hands the page back; the picker is the shared
// add-existing dialog scoped to the module with the module's search hint
// and exclude filter. Every dialog is stubbed — their own suites cover the
// forms.

import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

vi.mock("../../issues/components/WorkItemCreateDialog.js", () => ({
  WorkItemCreateDialog: (props: Record<string, unknown>) => (
    <div
      data-testid="create-dialog"
      data-project={String(props.initialProjectId)}
      data-locked={String(props.lockedProjectId)}
      data-context={JSON.stringify(props.context)}
      data-projects={JSON.stringify(props.projects)}
    >
      <button type="button" onClick={() => (props.onOpenChange as (open: boolean) => void)(false)}>
        close create
      </button>
      <button type="button" onClick={() => (props.onCreated as (id: string) => void)("issue-1")}>
        created
      </button>
    </div>
  ),
}));

vi.mock("../../issues/list/AddExistingModal.js", () => ({
  AddExistingModal: (props: Record<string, unknown>) => (
    <div
      data-testid="picker"
      data-module={String(props.excludeModuleId)}
      data-project={String(props.projectId)}
      data-placeholder={String(props.searchPlaceholder)}
    >
      <button type="button" onClick={() => void (props.onAttach as (ids: string[]) => Promise<void>)(["issue-1"])}>
        attach
      </button>
      <button type="button" onClick={() => (props.onOpenChange as (open: boolean) => void)(false)}>
        close picker
      </button>
    </div>
  ),
}));

import { ModuleAddWorkItem } from "./ModuleAddWorkItem.js";
import type { ModuleAddWorkItemController } from "../lib/useModuleAddWorkItem.js";

const PROJECT = {
  id: "223e4567-e89b-12d3-a456-426614174001",
  name: "Website",
  identifier: "WEB",
  memberRole: 20,
  archived: false,
};
const MODULE = "523e4567-e89b-12d3-a456-426614174004";

function controller(overrides: Partial<ModuleAddWorkItemController> = {}): ModuleAddWorkItemController {
  return {
    canAdd: true,
    createOpen: false,
    openCreate: vi.fn(),
    setCreateOpen: vi.fn(),
    pickerOpen: false,
    openPicker: vi.fn(),
    setPickerOpen: vi.fn(),
    addExisting: vi.fn(async () => true),
    isAdding: false,
    handleCreated: vi.fn(),
    ...overrides,
  };
}

function setup(control: ModuleAddWorkItemController) {
  return render(<ModuleAddWorkItem workspaceSlug="acme" project={PROJECT} moduleId={MODULE} controller={control} />);
}

describe("ModuleAddWorkItem", () => {
  it("renders nothing without the standing to add", () => {
    const control = controller({ canAdd: false });
    setup(control);
    expect(screen.queryByTestId("module-add-work-item")).not.toBeInTheDocument();
    expect(screen.queryByTestId("create-dialog")).not.toBeInTheDocument();
    expect(screen.queryByTestId("picker")).not.toBeInTheDocument();
  });

  it("offers the header add and opens the page's creation flow", () => {
    const control = controller();
    setup(control);
    fireEvent.click(screen.getByTestId("module-add-work-item"));
    expect(control.openCreate).toHaveBeenCalledTimes(1);
  });

  it("mounts the shared creation dialog locked to the module and closes it through the page", async () => {
    const control = controller({ createOpen: true });
    setup(control);
    const dialog = await screen.findByTestId("create-dialog");
    expect(dialog).toHaveAttribute("data-project", PROJECT.id);
    expect(dialog).toHaveAttribute("data-locked", PROJECT.id);
    expect(dialog).toHaveAttribute("data-context", JSON.stringify({ moduleId: MODULE }));
    expect(JSON.parse(dialog.getAttribute("data-projects") ?? "[]")).toEqual([PROJECT]);
    fireEvent.click(screen.getByRole("button", { name: "close create" }));
    expect(control.setCreateOpen).toHaveBeenCalledWith(false);
  });

  it("hands a successful create to the page without closing the dialog", async () => {
    const control = controller({ createOpen: true });
    setup(control);
    fireEvent.click(await screen.findByRole("button", { name: "created" }));
    expect(control.handleCreated).toHaveBeenCalledTimes(1);
    // Create more leaves the form up; only the dialog's own close closes it.
    expect(control.setCreateOpen).not.toHaveBeenCalled();
    expect(await screen.findByTestId("create-dialog")).toBeInTheDocument();
  });

  it("mounts the shared picker scoped to the module with the module search hint", async () => {
    const control = controller({ pickerOpen: true });
    setup(control);
    const picker = await screen.findByTestId("picker");
    expect(picker).toHaveAttribute("data-module", MODULE);
    expect(picker).toHaveAttribute("data-project", PROJECT.id);
    expect(picker).toHaveAttribute("data-placeholder", "Type to search");
    fireEvent.click(screen.getByRole("button", { name: "close picker" }));
    expect(control.setPickerOpen).toHaveBeenCalledWith(false);
  });

  it("hands the picker's picks to the module write", async () => {
    const control = controller({ pickerOpen: true });
    setup(control);
    fireEvent.click(await screen.findByRole("button", { name: "attach" }));
    expect(control.addExisting).toHaveBeenCalledWith(["issue-1"]);
  });
});
