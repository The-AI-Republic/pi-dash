// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { Input } from "./Input";
import { Textarea } from "./Textarea";

describe("Input", () => {
  it("associates the label and hint with the field", () => {
    render(<Input label="Title" hint="Shown in the list" placeholder="Name it" />);
    const field = screen.getByLabelText("Title");
    expect(field).toHaveAttribute("placeholder", "Name it");
    expect(field).toHaveAttribute("aria-describedby", expect.stringContaining("-hint"));
    expect(screen.getByText("Shown in the list")).toBeInTheDocument();
  });

  it("takes keyboard input", async () => {
    const user = userEvent.setup();
    render(<Input label="Title" />);
    const field = screen.getByLabelText("Title");
    await user.click(field);
    expect(field).toHaveFocus();
    await user.keyboard("hello");
    expect(field).toHaveValue("hello");
  });

  it("announces errors and marks the field invalid", () => {
    render(<Input label="Title" error="Required" />);
    const field = screen.getByLabelText("Title");
    expect(field).toHaveAttribute("aria-invalid", "true");
    const message = screen.getByRole("alert");
    expect(message).toHaveTextContent("Required");
    expect(field).toHaveAttribute("aria-describedby", message.id);
  });

  it("keeps a hidden label associated with the field", () => {
    render(<Input label="Search rows" placeholder="Search" hideLabel />);
    const field = screen.getByLabelText("Search rows");
    expect(field).toHaveAttribute("placeholder", "Search");
    expect(screen.getByText("Search rows")).toHaveClass("sr-only");
  });

  it("carries the visible focus ring classes", () => {
    render(<Input label="Title" />);
    expect(screen.getByLabelText("Title").className).toContain("focus-visible:outline-2");
  });

  it("pins a trailing control inside the field", async () => {
    const user = userEvent.setup();
    let cleared = 0;
    render(
      <Input
        label="Title"
        trailing={
          <button type="button" aria-label="Clear" onClick={() => (cleared += 1)}>
            Clear
          </button>
        }
      />
    );
    const field = screen.getByLabelText("Title");
    expect(field.className).toContain("pr-(--space-12)");
    await user.click(screen.getByRole("button", { name: "Clear" }));
    expect(cleared).toBe(1);
  });

  it("adds no trailing padding without a trailing control", () => {
    render(<Input label="Title" />);
    expect(screen.getByLabelText("Title").className).not.toContain("pr-(--space-12)");
  });
});

describe("Textarea", () => {
  it("associates the label and accepts multiline input", async () => {
    const user = userEvent.setup();
    render(<Textarea label="Description" />);
    const field = screen.getByLabelText("Description");
    await user.click(field);
    await user.keyboard("line one{Enter}line two");
    expect(field).toHaveValue("line one\nline two");
  });

  it("announces errors and marks the field invalid", () => {
    render(<Textarea label="Description" error="Too long" />);
    const field = screen.getByLabelText("Description");
    expect(field).toHaveAttribute("aria-invalid", "true");
    expect(screen.getByRole("alert")).toHaveTextContent("Too long");
  });
});
