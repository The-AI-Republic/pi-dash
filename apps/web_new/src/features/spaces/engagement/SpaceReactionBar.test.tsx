// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import type { SpaceReactionGroup } from "./model.js";
import { SpaceReactionBar } from "./SpaceReactionBar.js";

function group(overrides: Partial<SpaceReactionGroup> = {}): SpaceReactionGroup {
  return {
    code: "127881",
    emoji: "🎉",
    count: 2,
    names: [
      { name: "Ada", mine: false },
      { name: "Bo", mine: true },
    ],
    reacted: true,
    ...overrides,
  };
}

interface Rendered {
  onToggle: ReturnType<typeof vi.fn>;
  onPick: ReturnType<typeof vi.fn>;
}

function renderBar(
  groups: readonly SpaceReactionGroup[] = [group()],
  overrides: Partial<React.ComponentProps<typeof SpaceReactionBar>> = {}
): Rendered {
  const onToggle = vi.fn();
  const onPick = vi.fn();
  render(
    <SpaceReactionBar
      issueName="Public work item"
      groups={groups}
      embedded={false}
      onToggle={onToggle}
      onPick={onPick}
      addLabel="Add a reaction to Public work item"
      panelLabel="Choose a reaction"
      testId="reactions"
      pillTestId="reaction"
      addTestId="reaction-add"
      {...overrides}
    />
  );
  return { onToggle, onPick };
}

describe("SpaceReactionBar", () => {
  it("shows one pill per emoji with its count and the own mark", () => {
    renderBar([
      group(),
      group({ code: "128077", emoji: "👍", count: 1, names: [{ name: "Cy", mine: false }], reacted: false }),
    ]);
    const pills = screen.getAllByTestId("reaction");
    expect(pills.map((pill) => pill.getAttribute("data-reaction"))).toEqual(["127881", "128077"]);
    expect(pills[0]).toHaveTextContent("2");
    expect(pills[0]).toHaveAccessibleName("React with 🎉 to Public work item");
    expect(pills[0]).toHaveAttribute("aria-pressed", "true");
    expect(pills[1]).toHaveAttribute("aria-pressed", "false");
  });

  it("toggles the pill's group on activation", async () => {
    const { onToggle } = renderBar();
    await userEvent.click(screen.getByTestId("reaction"));
    expect(onToggle).toHaveBeenCalledWith("127881");
  });

  it("names the reactors in order on focus, with the visitor's own marked", () => {
    renderBar();
    fireEvent.focus(screen.getByTestId("reaction"));
    expect(screen.getByRole("tooltip")).toHaveTextContent("Ada, Bo (you)");
  });

  it("adds through the picker", async () => {
    const { onPick } = renderBar([]);
    expect(screen.queryByTestId("reaction")).not.toBeInTheDocument();
    await userEvent.click(screen.getByTestId("reaction-add"));
    await userEvent.click(screen.getByRole("button", { name: "🚀" }));
    expect(onPick).toHaveBeenCalledWith("128640");
  });

  it("hides the add control inside a frame but keeps the pills live", async () => {
    const { onToggle } = renderBar([group()], { embedded: true });
    expect(screen.queryByTestId("reaction-add")).not.toBeInTheDocument();
    await userEvent.click(screen.getByTestId("reaction"));
    expect(onToggle).toHaveBeenCalledWith("127881");
  });

  it("renders static counts with no toggle when read-only (SPC-057)", async () => {
    const { onToggle } = renderBar([group()], { embedded: true, readOnly: true });
    const pill = screen.getByTestId("reaction");
    expect(pill.tagName).toBe("SPAN");
    expect(pill).toHaveTextContent("2");
    expect(pill.getAttribute("data-reaction")).toBe("127881");
    await userEvent.click(pill);
    expect(onToggle).not.toHaveBeenCalled();
    expect(screen.queryByTestId("reaction-add")).not.toBeInTheDocument();
  });
});
