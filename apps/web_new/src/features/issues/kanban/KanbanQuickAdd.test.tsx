// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Board quick-add (parity/specs/issues.md §5, ISS-035): the compact entry,
// the inline form's focus and project prefix, trimmed submits, the form
// staying open for the next title, Escape, the empty guard, the disabled
// gate and the failure path that keeps the typed title.
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { KanbanQuickAdd } from "./KanbanQuickAdd.js";

function setup(overrides: Partial<React.ComponentProps<typeof KanbanQuickAdd>> = {}) {
  const onCreate = vi.fn();
  const props: React.ComponentProps<typeof KanbanQuickAdd> = {
    projectIdentifier: "WEB",
    disabled: false,
    onCreate,
    ...overrides,
  };
  const view = render(<KanbanQuickAdd {...props} />);
  return { onCreate, ...view };
}

/** Focus the inline field after opening the entry. */
async function openForm(user: ReturnType<typeof userEvent.setup>) {
  await user.click(screen.getByRole("button", { name: /New work item/ }));
  return screen.getByPlaceholderText("Work item title");
}

describe("KanbanQuickAdd", () => {
  it("renders nothing when creation is unavailable", () => {
    setup({ disabled: true, disabledReason: "This cycle is complete" });
    expect(screen.queryByTestId("kanban-quick-add")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /New work item/ })).not.toBeInTheDocument();
  });

  it("carries the disabled reason on the entry's tooltip", () => {
    setup({ disabledReason: "New work items are read-only for you" });
    expect(screen.getByRole("button", { name: /New work item/ })).toHaveAttribute(
      "title",
      "New work items are read-only for you"
    );
  });

  it("opens an inline form with the project prefix and focuses the title", async () => {
    const user = userEvent.setup();
    setup();
    const field = await openForm(user);
    expect(field).toHaveFocus();
    expect(field).toHaveAttribute("aria-label", "Work item title");
    expect(field).toHaveAttribute("maxLength", "255");
    expect(screen.getByTestId("kanban-quick-add-form")).toHaveTextContent("WEB");
  });

  it("submits the trimmed title and stays open for the next one", async () => {
    const user = userEvent.setup();
    const onCreate = vi.fn().mockResolvedValue(undefined);
    setup({ onCreate });
    const field = await openForm(user);
    await user.type(field, "  Tighten login  {enter}");
    expect(onCreate).toHaveBeenCalledTimes(1);
    expect(onCreate).toHaveBeenCalledWith("Tighten login");
    await waitFor(() => expect(screen.getByPlaceholderText("Work item title")).toHaveValue(""));
    expect(screen.getByPlaceholderText("Work item title")).toBeInTheDocument();
  });

  it("does nothing for an empty or whitespace title", async () => {
    const user = userEvent.setup();
    const { onCreate } = setup();
    const field = await openForm(user);
    await user.type(field, "   {enter}");
    expect(onCreate).not.toHaveBeenCalled();
    expect(screen.getByPlaceholderText("Work item title")).toHaveValue("   ");
  });

  it("does not submit twice while the first create is in flight", async () => {
    const user = userEvent.setup();
    let settle: () => void = () => undefined;
    const onCreate = vi.fn(
      () =>
        new Promise<void>((resolve) => {
          settle = resolve;
        })
    );
    setup({ onCreate });
    const field = await openForm(user);
    await user.type(field, "First{enter}");
    fireEvent.submit(screen.getByTestId("kanban-quick-add-form"));
    expect(onCreate).toHaveBeenCalledTimes(1);
    settle();
  });

  it("closes on Escape and clears the draft", async () => {
    const user = userEvent.setup();
    setup();
    const field = await openForm(user);
    await user.type(field, "Draft{escape}");
    expect(screen.queryByPlaceholderText("Work item title")).not.toBeInTheDocument();
    const reopened = await openForm(user);
    expect(reopened).toHaveValue("");
  });

  it("keeps the typed title when the create fails and re-enables the form", async () => {
    const user = userEvent.setup();
    const onCreate = vi.fn().mockRejectedValue(new Error("offline"));
    setup({ onCreate });
    const field = await openForm(user);
    await user.type(field, "Keep me{enter}");
    await waitFor(() => expect(onCreate).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(screen.getByPlaceholderText("Work item title")).toHaveValue("Keep me"));
    await user.type(screen.getByPlaceholderText("Work item title"), "{enter}");
    await waitFor(() => expect(onCreate).toHaveBeenCalledTimes(2));
  });
});
