// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { X } from "lucide-react";
import { describe, expect, it, vi } from "vitest";
import { IconButton } from "./IconButton";

describe("IconButton", () => {
  it("names itself from the label prop", () => {
    render(
      <IconButton label="Close">
        <X size={16} strokeWidth={1.5} />
      </IconButton>
    );
    expect(screen.getByRole("button", { name: "Close" })).toBeInTheDocument();
  });

  it("activates with the keyboard", async () => {
    const user = userEvent.setup();
    const onClick = vi.fn();
    render(
      <IconButton label="Close" onClick={onClick}>
        <X size={16} strokeWidth={1.5} />
      </IconButton>
    );
    const button = screen.getByRole("button", { name: "Close" });
    button.focus();
    await user.keyboard("{Enter}");
    expect(onClick).toHaveBeenCalledTimes(1);
  });

  it("carries the visible focus ring classes", () => {
    render(
      <IconButton label="Close">
        <X size={16} strokeWidth={1.5} />
      </IconButton>
    );
    expect(screen.getByRole("button", { name: "Close" }).className).toContain("focus-visible:outline-2");
  });

  it("paints the primary variant Google blue", () => {
    render(
      <IconButton label="Add" variant="primary">
        <X size={16} strokeWidth={1.5} />
      </IconButton>
    );
    const button = screen.getByRole("button", { name: "Add" });
    expect(button.className).toContain("bg-(--button-primary)");
    expect(button.className).toContain("text-(--on-button-primary)");
    expect(button.className).toContain("hover:bg-(--button-primary-hover)");
    expect(button.className).not.toContain("bg-(--accent)");
  });
});
