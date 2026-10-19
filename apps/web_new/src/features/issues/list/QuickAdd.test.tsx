// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  mutate: vi.fn(),
  attach: vi.fn(),
}));

vi.mock("../api/create.js", () => ({
  useCreateWorkItem: () => ({ mutate: mocks.mutate }),
}));

vi.mock("./mutations.js", () => ({
  attachCreatedIssue: mocks.attach,
}));

vi.mock("../../../core/api/client.js", () => ({
  getAppClient: () => ({ client: "stub" }),
}));

import { QuickAdd, type QuickAddProps } from "./QuickAdd.js";

const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const CYCLE = "323e4567-e89b-12d3-a456-426614174002";

function setup(overrides: Partial<QuickAddProps> = {}) {
  const props: QuickAddProps = {
    workspaceSlug: "acme",
    projectId: PROJECT,
    prefill: {},
    defaultStateId: null,
    disabled: false,
    ...overrides,
  };
  render(<QuickAdd {...props} />);
  return props;
}

function submitOnSuccess(): void {
  mocks.mutate.mockImplementation((_input: unknown, options: { onSuccess: (created: { id: string }) => void }) => {
    options.onSuccess({ id: "created-1" });
  });
}

beforeEach(() => {
  vi.clearAllMocks();
  mocks.attach.mockResolvedValue(undefined);
});

describe("QuickAdd", () => {
  it("shows the disabled reason instead of a form", () => {
    setup({ disabled: true, disabledReason: "This cycle is complete" });
    expect(screen.getByTestId("quick-add-disabled")).toHaveTextContent("This cycle is complete");
    expect(screen.queryByTestId("quick-add")).not.toBeInTheDocument();
  });

  it("falls back to the new-item label when no reason is supplied", () => {
    setup({ disabled: true });
    expect(screen.getByTestId("quick-add-disabled")).toHaveTextContent("New work item");
  });

  it("opens on click and closes on Escape or an outside press", async () => {
    const user = userEvent.setup();
    setup();
    fireEvent.click(screen.getByRole("button", { name: /New work item/ }));
    const input = screen.getByPlaceholderText("Work item title");
    expect(input).toHaveFocus();

    await user.keyboard("Draft{escape}");
    expect(screen.queryByPlaceholderText("Work item title")).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: /New work item/ }));
    fireEvent.mouseDown(document.body);
    expect(screen.queryByPlaceholderText("Work item title")).not.toBeInTheDocument();
  });

  it("submits trimmed titles with the section prefill and a default state", async () => {
    const user = userEvent.setup();
    const onCreated = vi.fn();
    setup({
      prefill: { priority: "high", label_ids: ["label-1"] },
      groupKey: "high",
      defaultStateId: "state-default",
      cycleId: CYCLE,
      moduleId: "module-1",
      projectIdentifier: "WEB",
      onCreated,
    });
    submitOnSuccess();

    fireEvent.click(screen.getByRole("button", { name: /New work item/ }));
    await user.type(screen.getByPlaceholderText("Work item title"), "  Tighten login  {enter}");

    expect(mocks.mutate).toHaveBeenCalledTimes(1);
    expect(mocks.mutate).toHaveBeenCalledWith(
      {
        projectId: PROJECT,
        payload: {
          name: "Tighten login",
          priority: "high",
          label_ids: ["label-1"],
          state_id: "state-default",
        },
        projectIdentifier: "WEB",
        tempGroupKey: "high",
      },
      expect.any(Object)
    );
    await waitFor(() =>
      expect(mocks.attach).toHaveBeenCalledWith({ client: "stub" }, "acme", PROJECT, "created-1", {
        cycleId: CYCLE,
        moduleId: "module-1",
      })
    );
    expect(onCreated).toHaveBeenCalledWith("created-1");
    expect(screen.getByPlaceholderText("Work item title")).toHaveValue("");
  });

  it("attaches cycle and module grouping values through the bridge", async () => {
    const user = userEvent.setup();
    setup({ prefill: { priority: "high", cycle_id: CYCLE, module_ids: ["module-1"] } });
    submitOnSuccess();
    fireEvent.click(screen.getByRole("button", { name: /New work item/ }));
    await user.type(screen.getByPlaceholderText("Work item title"), "Grouped{enter}");
    expect(mocks.mutate.mock.calls[0]?.[0]).toEqual({
      projectId: PROJECT,
      payload: { name: "Grouped", priority: "high" },
    });
    await waitFor(() =>
      expect(mocks.attach).toHaveBeenCalledWith({ client: "stub" }, "acme", PROJECT, "created-1", {
        cycleId: CYCLE,
        moduleId: "module-1",
      })
    );
  });

  it("keeps the route target ahead of a section grouping value", async () => {
    const user = userEvent.setup();
    setup({ cycleId: "route-cycle", prefill: { cycle_id: CYCLE } });
    submitOnSuccess();
    fireEvent.click(screen.getByRole("button", { name: /New work item/ }));
    await user.type(screen.getByPlaceholderText("Work item title"), "Routed{enter}");
    await waitFor(() =>
      expect(mocks.attach).toHaveBeenCalledWith({ client: "stub" }, "acme", PROJECT, "created-1", {
        cycleId: "route-cycle",
      })
    );
  });

  it("keeps the section state instead of applying the default state", async () => {
    const user = userEvent.setup();
    setup({ prefill: { state_id: "state-todo" }, defaultStateId: "state-default" });
    submitOnSuccess();
    fireEvent.click(screen.getByRole("button", { name: /New work item/ }));
    await user.type(screen.getByPlaceholderText("Work item title"), "One{enter}");
    expect(mocks.mutate.mock.calls[0]?.[0]).toMatchObject({ payload: { state_id: "state-todo" } });
  });

  it("does not submit an empty title", async () => {
    const user = userEvent.setup();
    setup();
    fireEvent.click(screen.getByRole("button", { name: /New work item/ }));
    await user.type(screen.getByPlaceholderText("Work item title"), "   {enter}");
    expect(mocks.mutate).not.toHaveBeenCalled();
  });

  it("does not pass a group key or attach target outside those contexts", async () => {
    const user = userEvent.setup();
    setup();
    submitOnSuccess();
    fireEvent.click(screen.getByRole("button", { name: /New work item/ }));
    await user.type(screen.getByPlaceholderText("Work item title"), "Plain{enter}");
    expect(mocks.mutate.mock.calls[0]?.[0]).toEqual({ projectId: PROJECT, payload: { name: "Plain" } });
    expect(mocks.attach).toHaveBeenCalledWith({ client: "stub" }, "acme", PROJECT, "created-1", {});
  });

  it("swallows attach failures after a successful create", async () => {
    const user = userEvent.setup();
    mocks.attach.mockRejectedValue(new Error("bridge down"));
    setup({ cycleId: CYCLE });
    submitOnSuccess();
    fireEvent.click(screen.getByRole("button", { name: /New work item/ }));
    await user.type(screen.getByPlaceholderText("Work item title"), "Created{enter}");
    await waitFor(() => expect(mocks.attach).toHaveBeenCalled());
    expect(screen.getByPlaceholderText("Work item title")).toHaveValue("");
  });
});
