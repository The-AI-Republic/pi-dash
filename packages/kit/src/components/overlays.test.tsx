// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { Button } from "./Button";
import { Popover } from "./Popover";
import { Tooltip } from "./Tooltip";

describe("Popover", () => {
  it("opens from the trigger and closes on Escape with focus back", async () => {
    const user = userEvent.setup();
    render(
      <Popover trigger={<Button>Details</Button>} title="Estimate" description="Point the issue">
        <p>Popover body</p>
      </Popover>
    );
    const trigger = screen.getByRole("button", { name: "Details" });
    await user.click(trigger);
    expect(screen.getByText("Popover body")).toBeInTheDocument();
    await user.keyboard("{Escape}");
    expect(screen.queryByText("Popover body")).not.toBeInTheDocument();
    expect(trigger).toHaveFocus();
  });
});

describe("Tooltip", () => {
  it("labels the hint and closes on Escape", async () => {
    // The open delay is timer-driven, and synthetic hover/focus does not
    // start it under jsdom, so the open state is driven directly while the
    // content, labelling, and Escape path are asserted for real. Hover
    // behavior is covered by the Ladle story.
    const user = userEvent.setup();
    const onOpenChange = vi.fn();
    render(
      <Tooltip content="Create issue" open onOpenChange={onOpenChange}>
        <Button>New</Button>
      </Tooltip>
    );
    expect(screen.getByRole("tooltip")).toHaveTextContent("Create issue");
    await user.keyboard("{Escape}");
    expect(onOpenChange).toHaveBeenCalledWith(false, expect.anything());
  });
});
