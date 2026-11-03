// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { ModuleDescriptionField, ModuleNameField } from "./ModuleIdentityFields.js";

describe("ModuleNameField", () => {
  it("shows the module name and saves a changed value on blur", () => {
    const onSave = vi.fn();
    render(<ModuleNameField value="Delivery" disabled={false} onSave={onSave} />);
    const input = screen.getByLabelText("Module name");
    expect(input).toHaveValue("Delivery");
    fireEvent.change(input, { target: { value: "Launch" } });
    fireEvent.blur(input);
    expect(onSave).toHaveBeenCalledWith("Launch");
  });

  it("saves on Enter and does not send an unchanged name", () => {
    const onSave = vi.fn();
    render(<ModuleNameField value="Delivery" disabled={false} onSave={onSave} />);
    const input = screen.getByLabelText("Module name");
    fireEvent.change(input, { target: { value: "Delivery " } });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(onSave).not.toHaveBeenCalled();
  });

  it("refuses an empty name locally and marks the field invalid", () => {
    const onSave = vi.fn();
    render(<ModuleNameField value="Delivery" disabled={false} onSave={onSave} />);
    const input = screen.getByLabelText("Module name");
    fireEvent.change(input, { target: { value: "   " } });
    fireEvent.blur(input);
    expect(onSave).not.toHaveBeenCalled();
    expect(input).toHaveAttribute("aria-invalid", "true");
  });

  it("restores the saved name on Escape", () => {
    const onSave = vi.fn();
    render(<ModuleNameField value="Delivery" disabled={false} onSave={onSave} />);
    const input = screen.getByLabelText("Module name");
    fireEvent.focus(input);
    fireEvent.change(input, { target: { value: "Different" } });
    fireEvent.keyDown(input, { key: "Escape" });
    expect(input).toHaveValue("Delivery");
    expect(onSave).not.toHaveBeenCalled();
  });

  it("follows a server rename while it is not being edited", () => {
    const onSave = vi.fn();
    const { rerender } = render(<ModuleNameField value="Delivery" disabled={false} onSave={onSave} />);
    rerender(<ModuleNameField value="Launch" disabled={false} onSave={onSave} />);
    expect(screen.getByLabelText("Module name")).toHaveValue("Launch");
  });

  it("stops accepting input when disabled", () => {
    render(<ModuleNameField value="Delivery" disabled onSave={vi.fn()} />);
    expect(screen.getByLabelText("Module name")).toBeDisabled();
  });
});

describe("ModuleDescriptionField", () => {
  it("saves a changed description on blur", () => {
    const onSave = vi.fn();
    render(<ModuleDescriptionField value="Old" disabled={false} onSave={onSave} />);
    const area = screen.getByLabelText("Module description");
    fireEvent.change(area, { target: { value: "New" } });
    fireEvent.blur(area);
    expect(onSave).toHaveBeenCalledWith("New");
  });

  it("does not save an unchanged description", () => {
    const onSave = vi.fn();
    render(<ModuleDescriptionField value="Old" disabled={false} onSave={onSave} />);
    fireEvent.blur(screen.getByLabelText("Module description"));
    expect(onSave).not.toHaveBeenCalled();
  });

  it("clears the description by saving an empty string", () => {
    const onSave = vi.fn();
    render(<ModuleDescriptionField value="Old" disabled={false} onSave={onSave} />);
    const area = screen.getByLabelText("Module description");
    fireEvent.change(area, { target: { value: "" } });
    fireEvent.blur(area);
    expect(onSave).toHaveBeenCalledWith("");
  });

  it("follows a server description change and disables when asked", () => {
    const { rerender } = render(<ModuleDescriptionField value="Old" disabled={false} onSave={vi.fn()} />);
    rerender(<ModuleDescriptionField value="Server" disabled={false} onSave={vi.fn()} />);
    expect(screen.getByLabelText("Module description")).toHaveValue("Server");
    rerender(<ModuleDescriptionField value="Server" disabled onSave={vi.fn()} />);
    expect(screen.getByLabelText("Module description")).toBeDisabled();
  });
});
