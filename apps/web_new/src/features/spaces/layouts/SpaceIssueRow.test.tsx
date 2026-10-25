// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import type { SpaceBoardIssue, SpaceBoardState } from "@pidash/api-client";
import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

const embed = vi.hoisted(() => ({ framed: false }));

vi.mock("../lib/spaceEmbed.js", () => ({ useSpaceEmbedded: () => embed.framed }));

import {
  formatSpaceCardDate,
  spaceCardLabels,
  spaceCardPriority,
  spaceCardReactionEmoji,
  spaceCardReactionCounts,
  spaceCardVoteCounts,
  SpaceIssueRow,
} from "./SpaceIssueRow.js";
import { boardIssue, issueHref, LABEL_ID, LABELS, STATES, TODO_ID } from "./testFixtures.js";

const TODO: SpaceBoardState = (() => {
  const found = STATES.find((state) => state.id === TODO_ID);
  if (found === undefined) throw new Error("missing Todo fixture");
  return found;
})();

const ACTOR = {
  id: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
  display_name: "Ada",
  first_name: "Ada",
  last_name: "Lovelace",
  avatar: "",
  avatar_url: null,
};

function richIssue(overrides: Partial<SpaceBoardIssue> = {}): SpaceBoardIssue {
  return {
    ...boardIssue(7, TODO_ID, "A very long public work item title"),
    priority: "high",
    target_date: "2027-03-15",
    label_ids: [LABEL_ID],
    vote_items: [
      { vote: 1, actor_details: ACTOR },
      { vote: 1, actor_details: { ...ACTOR, id: "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb" } },
      { vote: -1, actor_details: { ...ACTOR, id: "cccccccc-cccc-4ccc-8ccc-cccccccccccc" } },
      { vote: 0, actor_details: { ...ACTOR, id: "dddddddd-dddd-4ddd-8ddd-dddddddddddd" } },
    ],
    reaction_items: [
      { reaction: "128077", actor_details: ACTOR },
      { reaction: "128077", actor_details: { ...ACTOR, id: "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb" } },
      { reaction: "127881", actor_details: { ...ACTOR, id: "cccccccc-cccc-4ccc-8ccc-cccccccccccc" } },
    ],
    ...overrides,
  };
}

function renderCard(overrides: Partial<React.ComponentProps<typeof SpaceIssueRow>> = {}) {
  const { state = TODO, ...rest } = overrides;
  const issue = overrides.issue ?? richIssue();
  return render(
    <SpaceIssueRow
      issue={issue}
      state={state}
      labels={LABELS}
      projectIdentifier="SPC"
      issueHref={issueHref(issue)}
      {...rest}
    />
  );
}

describe("SpaceIssueRow identity and markers (SPC-027/028)", () => {
  it("renders a list row with the key outside a title-only open link", () => {
    renderCard();
    const link = screen.getByRole("link", { name: "A very long public work item title" });
    expect(link).toHaveAttribute("href", expect.stringContaining("peekId="));
    expect(link).toHaveAttribute("title", "A very long public work item title");
    expect(screen.getByTestId("space-issue-key")).toHaveTextContent("SPC-7");
    expect(link).not.toContainElement(screen.getByTestId("space-issue-key"));
  });

  it("shows the list row's full title in a tooltip on keyboard focus", () => {
    renderCard();
    const link = screen.getByRole("link", { name: "A very long public work item title" });
    fireEvent.focus(link);
    expect(screen.getByRole("tooltip")).toHaveTextContent("A very long public work item title");
    fireEvent.blur(link);
    expect(screen.queryByRole("tooltip")).not.toBeInTheDocument();
  });

  it("renders the board card body as one link with the key, title and tooltip", () => {
    renderCard({ card: true });
    const card = screen.getByTestId("space-board-card");
    const link = screen.getByRole("link", { name: /SPC-7 A very long public work item title/ });
    expect(link).toContainElement(card.querySelector("a > div:nth-child(1)") as HTMLElement);
    expect(link.querySelector("div[title='A very long public work item title']")).not.toBeNull();
    expect(screen.getByText("SPC-7")).toBeInTheDocument();
  });

  it("shows the board title in a tooltip on keyboard focus", () => {
    renderCard({ card: true });
    const link = screen.getByRole("link", { name: /SPC-7 A very long public work item title/ });
    fireEvent.focus(link);
    expect(screen.getByRole("tooltip")).toHaveTextContent("A very long public work item title");
  });

  it("shows state, priority, label and due-date markers", () => {
    renderCard();
    expect(screen.getByTestId("space-card-state")).toHaveTextContent("Todo");
    expect(screen.getByTestId("space-state-marker")).toHaveAttribute("title", "Todo");
    expect(screen.getByTestId("space-card-priority")).toHaveAttribute("data-priority", "high");
    expect(screen.getByTestId("space-card-priority")).toHaveAttribute("title", "High");
    expect(screen.getByTestId("space-card-priority").querySelector("svg")).toHaveClass("lucide-signal-high");
    expect(screen.getByTestId("space-card-label")).toHaveTextContent("Bug");
    expect(screen.getByTestId("space-card-date")).toHaveTextContent("Mar 15, 2027");
  });

  it("always renders the priority marker, with none for an unset level", () => {
    renderCard({ issue: richIssue({ priority: null, target_date: null, label_ids: [] }) });
    expect(screen.getByTestId("space-card-priority")).toHaveAttribute("data-priority", "none");
    expect(screen.getByTestId("space-card-priority").querySelector("svg")).toHaveClass("lucide-ban");
    expect(screen.queryByTestId("space-card-label")).not.toBeInTheDocument();
    expect(screen.queryByTestId("space-card-date")).not.toBeInTheDocument();
  });

  it("does not show the start date, cycle or module", () => {
    const issue = richIssue({ start_date: "2027-01-02", cycle_id: "eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee" });
    issue.module_ids = ["ffffffff-ffff-4fff-8fff-ffffffffffff"];
    renderCard({ issue });
    expect(screen.getByTestId("space-card-markers").textContent).not.toContain("Jan 2, 2027");
    expect(screen.getByTestId("space-card-markers").textContent).not.toContain("Cycle");
    expect(screen.getByTestId("space-card-markers").textContent).not.toContain("Module");
  });

  it("drops stale label ids and keeps the issue's label order", () => {
    const second = { id: "99999999-9999-4999-8999-999999999999", name: "Core", color: "#000", parent: null };
    expect(spaceCardLabels([second.id, "missing", LABEL_ID], [...LABELS, second]).map((label) => label.name)).toEqual([
      "Core",
      "Bug",
    ]);
    expect(spaceCardLabels([], LABELS)).toEqual([]);
  });

  it("formats only valid ISO dates", () => {
    expect(formatSpaceCardDate(null)).toBeNull();
    expect(formatSpaceCardDate("not-a-date")).toBeNull();
    expect(formatSpaceCardDate("2027-03-15")).toBe("Mar 15, 2027");
    expect(spaceCardPriority(null)).toBe("none");
    expect(spaceCardPriority("urgent")).toBe("urgent");
  });
});

describe("SpaceIssueRow read-only and open behavior (SPC-031/032)", () => {
  it("highlights exactly the selected row and exposes the current state", () => {
    const issue = richIssue();
    const { rerender } = renderCard({ issue });
    expect(screen.getByTestId("space-board-issue")).not.toHaveAttribute("data-selected");
    expect(screen.getByRole("link")).not.toHaveAttribute("aria-current");
    rerender(
      <SpaceIssueRow
        issue={issue}
        state={TODO}
        labels={LABELS}
        projectIdentifier="SPC"
        issueHref={issueHref(issue)}
        selected
      />
    );
    expect(screen.getByTestId("space-board-issue")).toHaveAttribute("data-selected", "true");
    expect(screen.getByTestId("space-board-issue").className).toContain("border-(--accent-strong)");
    expect(screen.getByRole("link")).toHaveAttribute("aria-current", "true");
  });

  it("highlights a selected board card through the same exact-match flag", () => {
    renderCard({ card: true, selected: true });
    expect(screen.getByTestId("space-board-card")).toHaveAttribute("data-selected", "true");
    expect(screen.getByTestId("space-board-card").className).toContain("border-(--accent-strong)");
    expect(screen.getByRole("link", { name: /A very long public work item title/ })).toHaveAttribute(
      "aria-current",
      "true"
    );
  });

  it("calls the open action without navigating, and leaves the link alone without one", async () => {
    const onOpenIssue = vi.fn();
    const issue = richIssue();
    const { rerender } = renderCard({ issue, onOpenIssue });
    await userEvent.click(screen.getByRole("link"));
    expect(onOpenIssue).toHaveBeenCalledWith(issue);
    rerender(
      <SpaceIssueRow issue={issue} state={TODO} labels={LABELS} projectIdentifier="SPC" issueHref={issueHref(issue)} />
    );
    expect(screen.getByRole("link")).toHaveAttribute("href", issueHref(issue));
    const click = new MouseEvent("click", { bubbles: true, cancelable: true });
    let preventedByCard = true;
    const observe = (event: MouseEvent): void => {
      preventedByCard = event.defaultPrevented;
      // Keep jsdom from attempting a real navigation in this unit test.
      event.preventDefault();
    };
    window.addEventListener("click", observe, { once: true });
    screen.getByRole("link").dispatchEvent(click);
    expect(preventedByCard).toBe(false);
  });

  it("has no draggable cards, drag handles or inline edit controls", () => {
    renderCard({ card: true });
    const card = screen.getByTestId("space-board-card");
    expect(card).not.toHaveAttribute("draggable");
    expect(card.querySelector("[draggable='true']")).toBeNull();
    expect(card.querySelector("[data-drag-handle], [class*='cursor-grab'], [class*='cursor-move']")).toBeNull();
    expect(card.querySelector("input, textarea, select, [contenteditable='true']")).toBeNull();
  });
});

describe("SpaceIssueRow board footer (SPC-029)", () => {
  it("renders nothing when both engagement flags are off", () => {
    renderCard({ card: true });
    expect(screen.queryByTestId("space-board-card-footer")).not.toBeInTheDocument();
  });

  it("renders only votes when votes are enabled", () => {
    renderCard({ card: true, votesEnabled: true });
    expect(screen.getByTestId("space-card-votes")).toBeInTheDocument();
    expect(screen.getByTestId("space-card-vote-up")).toHaveTextContent("2");
    expect(screen.getByTestId("space-card-vote-down")).toHaveTextContent("1");
    expect(screen.queryByTestId("space-card-reactions")).not.toBeInTheDocument();
  });

  it("renders only reactions when reactions are enabled", () => {
    renderCard({ card: true, reactionsEnabled: true });
    expect(screen.queryByTestId("space-card-votes")).not.toBeInTheDocument();
    const reactions = screen.getAllByTestId("space-card-reaction");
    expect(reactions.map((node) => node.getAttribute("data-reaction"))).toEqual(["128077", "127881"]);
    expect(reactions.map((node) => node.textContent)).toEqual(["👍2", "🎉1"]);
    expect(reactions[0]).toHaveAccessibleName("React with 👍 to A very long public work item title");
    expect(reactions[0]).toHaveTextContent("2");
    expect(screen.getByTestId("space-card-reaction-add")).toHaveAttribute("aria-haspopup", "dialog");
  });

  it("sends vote and reaction actions and tolerates missing handlers", async () => {
    const onVote = vi.fn();
    const onReact = vi.fn();
    const issue = richIssue();
    const { rerender } = renderCard({ issue, card: true, votesEnabled: true, reactionsEnabled: true, onVote, onReact });
    await userEvent.click(screen.getByTestId("space-card-vote-up"));
    await userEvent.click(screen.getByTestId("space-card-vote-down"));
    await userEvent.click(screen.getAllByTestId("space-card-reaction")[0]!);
    await userEvent.click(screen.getByTestId("space-card-reaction-add"));
    expect(onVote).toHaveBeenNthCalledWith(1, issue, 1);
    expect(onVote).toHaveBeenNthCalledWith(2, issue, -1);
    expect(onReact).toHaveBeenCalledWith(issue, "128077");
    rerender(
      <SpaceIssueRow
        issue={issue}
        state={TODO}
        labels={LABELS}
        projectIdentifier="SPC"
        issueHref={issueHref(issue)}
        card
        votesEnabled
        reactionsEnabled
      />
    );
    // Missing handlers leave the controls inert rather than throwing.
    expect(() => screen.getByTestId("space-card-vote-up").click()).not.toThrow();
    expect(() => screen.getAllByTestId("space-card-reaction")[0]!.click()).not.toThrow();
  });

  it("never renders a footer on a list row in any flag combination", () => {
    renderCard({ votesEnabled: true, reactionsEnabled: true });
    expect(screen.queryByTestId("space-board-card-footer")).not.toBeInTheDocument();
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
  });

  it("groups votes and reactions and keeps first-seen reaction order", () => {
    expect(spaceCardVoteCounts(richIssue())).toEqual({ up: 2, down: 1 });
    expect(spaceCardReactionCounts(richIssue().reaction_items)).toEqual([
      { reaction: "128077", emoji: "👍", count: 2 },
      { reaction: "127881", emoji: "🎉", count: 1 },
    ]);
    expect(spaceCardReactionEmoji("128077-127995")).toBe("👍🏻");
    expect(spaceCardReactionEmoji("not-a-code")).toBe("not-a-code");
    expect(spaceCardReactionEmoji("1114112")).toBe("1114112");
  });
});

describe("SpaceIssueRow embed behavior (SPC-015)", () => {
  it("hides the reaction add control inside a frame but keeps counts readable", () => {
    embed.framed = true;
    try {
      renderCard({ card: true, reactionsEnabled: true });
      expect(screen.queryByTestId("space-card-reaction-add")).not.toBeInTheDocument();
      expect(screen.getAllByTestId("space-card-reaction")).toHaveLength(2);
    } finally {
      embed.framed = false;
    }
  });

  it("keeps vote counts visible but makes both vote controls inert inside a frame", async () => {
    embed.framed = true;
    const onVote = vi.fn();
    try {
      renderCard({ card: true, votesEnabled: true, onVote });
      const up = screen.getByTestId("space-card-vote-up");
      const down = screen.getByTestId("space-card-vote-down");
      expect(up).toHaveTextContent("2");
      expect(down).toHaveTextContent("1");
      expect(up).toBeDisabled();
      expect(down).toBeDisabled();
      expect(up).toHaveAttribute("aria-disabled", "true");
      expect(down).toHaveAttribute("aria-disabled", "true");
      await userEvent.click(up);
      await userEvent.click(down);
      expect(onVote).not.toHaveBeenCalled();
    } finally {
      embed.framed = false;
    }
  });
});
