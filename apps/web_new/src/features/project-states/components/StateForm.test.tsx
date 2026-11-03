// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { cleanup, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { StateForm, STATE_FORM_PALETTE } from "./StateForm.js";

afterEach(() => {
  cleanup();
});

function renderForm(props: Partial<React.ComponentProps<typeof StateForm>> = {}) {
  const onSubmit = vi.fn();
  const onCancel = vi.fn();
  render(
    <StateForm
      mode="create"
      groupLabel="Backlog"
      initialColor="#9CA3AF"
      pending={false}
      onSubmit={onSubmit}
      onCancel={onCancel}
      {...props}
    />
  );
  return { onSubmit, onCancel };
}

describe("StateForm", () => {
  it("submits the trimmed-shape value with the chosen swatch and description", async () => {
    const user = userEvent.setup();
    const { onSubmit } = renderForm();
    await user.type(screen.getByLabelText("State name"), "Triage queue");
    await user.type(screen.getByLabelText("Description"), "Waiting for a first look");
    await user.click(screen.getByRole("button", { name: "Pick a state color" }));
    const swatches = screen.getAllByTestId("state-swatch");
    expect(swatches).toHaveLength(STATE_FORM_PALETTE.length);
    await user.click(screen.getByRole("button", { name: "#8B5CF6" }));
    await user.click(screen.getByRole("button", { name: "Create state" }));
    expect(onSubmit).toHaveBeenCalledWith({
      name: "Triage queue",
      description: "Waiting for a first look",
      color: "#8B5CF6",
    });
  });

  it("keeps a blank name in the form with an inline error and sends nothing", async () => {
    const user = userEvent.setup();
    const { onSubmit } = renderForm();
    await user.click(screen.getByRole("button", { name: "Create state" }));
    expect(onSubmit).not.toHaveBeenCalled();
    expect(screen.getByText("State name is required")).toBeInTheDocument();
    await user.type(screen.getByLabelText("State name"), "x");
    expect(screen.queryByText("State name is required")).not.toBeInTheDocument();
  });

  it("treats whitespace as blank", async () => {
    const user = userEvent.setup();
    const { onSubmit } = renderForm();
    await user.type(screen.getByLabelText("State name"), "    ");
    await user.click(screen.getByRole("button", { name: "Create state" }));
    expect(onSubmit).not.toHaveBeenCalled();
    expect(screen.getByText("State name is required")).toBeInTheDocument();
  });

  it("caps a pasted name at a hundred characters", async () => {
    const user = userEvent.setup();
    renderForm();
    const input = screen.getByLabelText("State name") as HTMLInputElement;
    await user.click(input);
    await user.paste("y".repeat(150));
    expect(input.value).toHaveLength(100);
  });

  it("prefills the edit form and labels its submit as a save", () => {
    renderForm({
      mode: "edit",
      initialName: "Review",
      initialDescription: "Ready for a look",
      initialColor: "#3B82F6",
    });
    expect(screen.getByLabelText("State name")).toHaveValue("Review");
    expect(screen.getByLabelText("Description")).toHaveValue("Ready for a look");
    expect(screen.getAllByRole("button", { name: "Save changes" })).toHaveLength(1);
    expect(screen.getByRole("form", { name: "Edit state Review" })).toBeInTheDocument();
  });

  it("discards edits through cancel without submitting", async () => {
    const user = userEvent.setup();
    const { onCancel, onSubmit } = renderForm({ mode: "edit", initialName: "Review" });
    await user.type(screen.getByLabelText("State name"), " changed");
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onCancel).toHaveBeenCalledTimes(1);
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("locks the fields and cancel while a save is in flight", () => {
    renderForm({ pending: true });
    expect(screen.getByLabelText("State name")).toBeDisabled();
    expect(screen.getByLabelText("Description")).toBeDisabled();
    expect(screen.getByRole("button", { name: "Cancel" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Pick a state color" })).toBeDisabled();
  });

  it("marks the current swatch as pressed", async () => {
    const user = userEvent.setup();
    renderForm({ initialColor: "#3B82F6" });
    await user.click(screen.getByRole("button", { name: "Pick a state color" }));
    const pressed = within(screen.getByRole("group", { name: "Pick a color" }))
      .getAllByRole("button")
      .filter((button) => button.getAttribute("aria-pressed") === "true");
    expect(pressed.map((button) => button.getAttribute("aria-label"))).toEqual(["#3B82F6"]);
  });
});
