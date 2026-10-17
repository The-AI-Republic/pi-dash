// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";
import { LABEL_PALETTE } from "./palette.js";
import { LabelForm } from "./LabelForm.js";
import { LABEL_NAME_REQUIRED_MESSAGE, LABEL_NAME_TOO_LONG_MESSAGE } from "./validation.js";

function setup(props?: Partial<React.ComponentProps<typeof LabelForm>>) {
  const onSubmit = vi.fn();
  const onCancel = vi.fn();
  render(<LabelForm mode="create" pending={false} onSubmit={onSubmit} onCancel={onCancel} {...props} />);
  return { onSubmit, onCancel };
}

describe("LabelForm", () => {
  it("seeds a palette color and submits the valid name with it", async () => {
    const user = userEvent.setup();
    const { onSubmit } = setup();
    const dot = screen.getByTestId("label-form-dot");
    expect(LABEL_PALETTE.map(hexToRgb)).toContain(dot.style.backgroundColor);
    await user.type(screen.getByTestId("label-form-name"), "Triage");
    await user.click(screen.getByRole("button", { name: "Create label" }));
    expect(onSubmit).toHaveBeenCalledTimes(1);
    expect(onSubmit.mock.calls[0]?.[0]).toMatchObject({ name: "Triage" });
  });

  it("fails empty and over-long names inline without submitting", async () => {
    const user = userEvent.setup();
    const { onSubmit } = setup();
    await user.click(screen.getByRole("button", { name: "Create label" }));
    expect(screen.getByRole("alert")).toHaveTextContent(LABEL_NAME_REQUIRED_MESSAGE);
    expect(onSubmit).not.toHaveBeenCalled();
    await user.type(screen.getByTestId("label-form-name"), "x".repeat(256));
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Create label" }));
    expect(screen.getByRole("alert")).toHaveTextContent(LABEL_NAME_TOO_LONG_MESSAGE);
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("recolors the dot through the swatch popover", async () => {
    const user = userEvent.setup();
    setup();
    await user.click(screen.getByTestId("label-form-dot"));
    const swatches = screen.getAllByTestId("label-swatch");
    expect(swatches).toHaveLength(10);
    await user.click(screen.getByRole("button", { name: "#00D084" }));
    expect(screen.getByTestId("label-form-dot").style.backgroundColor).toBe("rgb(0, 208, 132)");
    expect(screen.queryByRole("group", { name: "Pick a color" })).not.toBeInTheDocument();
  });

  it("prefills name and color in edit mode", async () => {
    const user = userEvent.setup();
    const { onSubmit, onCancel } = setup({ mode: "edit", initialName: "Old", initialColor: "#9900EF" });
    expect(screen.getByTestId("label-form")).toHaveAttribute("aria-label", "Edit label");
    expect(screen.getByTestId("label-form-name")).toHaveValue("Old");
    expect(screen.getByTestId("label-form-dot").style.backgroundColor).toBe("rgb(153, 0, 239)");
    await user.clear(screen.getByTestId("label-form-name"));
    await user.type(screen.getByTestId("label-form-name"), "New");
    await user.click(screen.getByRole("button", { name: "Save changes" }));
    expect(onSubmit).toHaveBeenCalledWith({ name: "New", color: "#9900EF" });
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onCancel).toHaveBeenCalledTimes(1);
  });

  it("locks the fields while the save lands", () => {
    setup({ pending: true });
    expect(screen.getByTestId("label-form-name")).toBeDisabled();
    expect(screen.getByTestId("label-form-dot")).toBeDisabled();
    expect(screen.getByRole("button", { name: "Create label" })).toBeDisabled();
  });

  it("submits on Enter without reloading", () => {
    const { onSubmit } = setup();
    fireEvent.change(screen.getByTestId("label-form-name"), { target: { value: "Typed" } });
    fireEvent.submit(screen.getByTestId("label-form"));
    expect(onSubmit).toHaveBeenCalledWith(expect.objectContaining({ name: "Typed" }));
  });
});

/** jsdom reports colors as rgb(): normalize the palette for the seed assertion. */
function hexToRgb(hex: string): string {
  const r = parseInt(hex.slice(1, 3), 16);
  const g = parseInt(hex.slice(3, 5), 16);
  const b = parseInt(hex.slice(5, 7), 16);
  return `rgb(${r}, ${g}, ${b})`;
}
