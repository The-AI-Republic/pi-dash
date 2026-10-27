// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { POPOVER_LAYER_CLASS } from "../lib/styles";
import { Button } from "./Button";
import { Popover } from "./Popover";
import { Tooltip } from "./Tooltip";
import { positionerOf } from "./overlays.testSupport";

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

  it("lands its popup on the popover tier above in-page chrome", async () => {
    // A sticky row with a z-index used to paint over the popup and take
    // its clicks; the positioner inside the portal carries the tier.
    const user = userEvent.setup();
    render(
      <Popover trigger={<Button>Details</Button>}>
        <p>Popover body</p>
      </Popover>
    );
    await user.click(screen.getByRole("button", { name: "Details" }));
    expect(positionerOf(screen.getByText("Popover body")).className).toContain(POPOVER_LAYER_CLASS);
    expect(document.body.contains(positionerOf(screen.getByText("Popover body")))).toBe(true);
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
    const hint = screen.getByRole("tooltip");
    expect(hint).toHaveTextContent("Create issue");
    expect(positionerOf(hint).className).toContain(POPOVER_LAYER_CLASS);
    await user.keyboard("{Escape}");
    expect(onOpenChange).toHaveBeenCalledWith(false, expect.anything());
  });
});
