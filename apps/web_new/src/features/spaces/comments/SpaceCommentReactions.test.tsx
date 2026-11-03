// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import type { SpaceReactionGroup } from "../engagement/model.js";
import { SpaceCommentReactions } from "./SpaceCommentReactions.js";

function group(overrides: Partial<SpaceReactionGroup> = {}): SpaceReactionGroup {
  return {
    code: "127881",
    emoji: "🎉",
    count: 1,
    names: [{ name: "Guest", mine: false }],
    reacted: false,
    ...overrides,
  };
}

function renderReactions(
  groups: readonly SpaceReactionGroup[] = [group()],
  overrides: Partial<React.ComponentProps<typeof SpaceCommentReactions>> = {}
) {
  const onToggle = vi.fn();
  const onPick = vi.fn();
  render(
    <SpaceCommentReactions
      groups={groups}
      authorName="Ada L"
      embedded={false}
      onToggle={onToggle}
      onPick={onPick}
      {...overrides}
    />
  );
  return { onToggle, onPick };
}

describe("SpaceCommentReactions", () => {
  it("toggles a pill and names its reactors with the visitor's own marked (SPC-057)", async () => {
    const { onToggle } = renderReactions([
      group({
        names: [
          { name: "Guest", mine: false },
          { name: "Ada L", mine: true },
        ],
        count: 2,
        reacted: true,
      }),
    ]);
    const pill = screen.getByTestId("space-comment-reaction");
    expect(pill).toHaveAttribute("data-reaction", "127881");
    expect(pill).toHaveTextContent("2");
    expect(pill).toHaveAttribute("aria-pressed", "true");
    fireEvent.focus(pill);
    expect(screen.getByRole("tooltip")).toHaveTextContent("Guest, Ada L (you)");
    await userEvent.click(pill);
    expect(onToggle).toHaveBeenCalledWith("127881");
  });

  it("adds through the picker (SPC-057)", async () => {
    const { onPick } = renderReactions([]);
    expect(screen.queryByTestId("space-comment-reaction")).not.toBeInTheDocument();
    await userEvent.click(screen.getByTestId("space-comment-reaction-add"));
    await userEvent.click(screen.getByRole("button", { name: "🎉" }));
    expect(onPick).toHaveBeenCalledWith("127881");
  });

  it("shows static counts with no add control inside a frame (SPC-057)", async () => {
    const { onToggle } = renderReactions([group()], { embedded: true });
    const pill = screen.getByTestId("space-comment-reaction");
    expect(pill.tagName).toBe("SPAN");
    expect(pill).toHaveTextContent("1");
    await userEvent.click(pill);
    expect(onToggle).not.toHaveBeenCalled();
    expect(screen.queryByTestId("space-comment-reaction-add")).not.toBeInTheDocument();
  });
});
