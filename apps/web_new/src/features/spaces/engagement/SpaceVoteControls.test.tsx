// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import type { SpaceVoteTally } from "./model.js";
import { SpaceVoteControls } from "./SpaceVoteControls.js";

function tally(overrides: Partial<SpaceVoteTally> = {}): SpaceVoteTally {
  return {
    up: {
      count: 2,
      voters: [
        { name: "Ada", mine: false },
        { name: "Bo", mine: true },
      ],
      mine: true,
    },
    down: { count: 0, voters: [], mine: false },
    mine: 1,
    ...overrides,
  };
}

function renderControls(overrides: Partial<React.ComponentProps<typeof SpaceVoteControls>> = {}) {
  const onVote = vi.fn();
  render(
    <SpaceVoteControls
      issueName="Public work item"
      tally={tally()}
      embedded={false}
      onVote={onVote}
      testId="votes"
      upTestId="vote-up"
      downTestId="vote-down"
      {...overrides}
    />
  );
  return onVote;
}

describe("SpaceVoteControls", () => {
  it("shows both counts and marks the visitor's own side", () => {
    renderControls();
    expect(screen.getByTestId("vote-up")).toHaveTextContent("2");
    expect(screen.getByTestId("vote-down")).toHaveTextContent("0");
    expect(screen.getByTestId("vote-up")).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByTestId("vote-down")).toHaveAttribute("aria-pressed", "false");
    expect(screen.getByTestId("vote-up")).toHaveAccessibleName("Upvote Public work item");
    expect(screen.getByTestId("vote-down")).toHaveAccessibleName("Downvote Public work item");
  });

  it("votes the clicked side", async () => {
    const onVote = renderControls();
    await userEvent.click(screen.getByTestId("vote-up"));
    await userEvent.click(screen.getByTestId("vote-down"));
    expect(onVote).toHaveBeenNthCalledWith(1, 1);
    expect(onVote).toHaveBeenNthCalledWith(2, -1);
  });

  it("names the voters in order on focus, with the visitor's own marked", () => {
    renderControls();
    fireEvent.focus(screen.getByTestId("vote-up"));
    expect(screen.getByRole("tooltip")).toHaveTextContent("Ada, Bo (you)");
  });

  it("shows the empty note on a side with no voters", () => {
    renderControls();
    fireEvent.focus(screen.getByTestId("vote-down"));
    expect(screen.getByRole("tooltip")).toHaveTextContent("No votes yet");
  });

  it("keeps counts and marks but stays inert inside a frame", async () => {
    const onVote = renderControls({ embedded: true });
    const up = screen.getByTestId("vote-up");
    const down = screen.getByTestId("vote-down");
    expect(up).toHaveTextContent("2");
    expect(up).toHaveAttribute("aria-pressed", "true");
    expect(up).toBeDisabled();
    expect(down).toBeDisabled();
    expect(up).toHaveAttribute("aria-disabled", "true");
    expect(down).toHaveAttribute("aria-disabled", "true");
    await userEvent.click(up);
    await userEvent.click(down);
    expect(onVote).not.toHaveBeenCalled();
  });
});
