// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Hover preview card (parity/specs/issues.md §11, ISS-133): the identifier
// and state name share the first line, the title follows, and the last
// line carries the priority beside the dates. The hover wrapper is
// exercised too, since that is how a reference surface mounts it.

import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it } from "vitest";

import { previewKey, WorkItemPreviewCard, WorkItemPreviewHover } from "./WorkItemPreviewCard.js";

const ISSUE = {
  id: "123e4567-e89b-12d3-a456-426614174000",
  name: "Ship the preview",
  sequence_id: 42,
  priority: "high" as const,
  start_date: null,
  target_date: null,
  state_id: "323e4567-e89b-12d3-a456-426614174001",
};

const TODAY = "2026-03-10";

describe("previewKey", () => {
  it("joins the project code and the sequence", () => {
    expect(previewKey("WEB", 42)).toBe("WEB-42");
  });
});

describe("WorkItemPreviewCard", () => {
  it("shows the key, state, title and priority", () => {
    render(
      <WorkItemPreviewCard
        issue={ISSUE}
        projectIdentifier="WEB"
        state={{ name: "In progress", group: "started" }}
        today={TODAY}
      />
    );
    expect(screen.getByTestId("preview-card-key")).toHaveTextContent("WEB-42");
    expect(screen.getByTestId("preview-card-state")).toHaveTextContent("In progress");
    expect(screen.getByText("Ship the preview")).toBeInTheDocument();
    expect(screen.getByTestId("preview-card-priority")).toHaveAttribute("title", "High");
    expect(screen.getByTestId("preview-card-priority").querySelector("svg")).toHaveClass("lucide-signal-high");
    // With no dates the card keeps only the priority glyph.
    expect(screen.queryByTestId("preview-card-dates")).not.toBeInTheDocument();
  });

  it("omits the state row until the caller resolves it", () => {
    render(<WorkItemPreviewCard issue={ISSUE} projectIdentifier="WEB" today={TODAY} />);
    expect(screen.queryByTestId("preview-card-state")).not.toBeInTheDocument();
    expect(screen.getByTestId("preview-card-key")).toBeInTheDocument();
  });

  it("renders a range and highlights a passed due date on open work", () => {
    render(
      <WorkItemPreviewCard
        issue={{ ...ISSUE, start_date: "2026-03-01", target_date: "2026-03-05" }}
        projectIdentifier="WEB"
        state={{ name: "Doing", group: "started" }}
        today={TODAY}
      />
    );
    const dates = screen.getByTestId("preview-card-dates");
    expect(dates).toHaveTextContent("Mar 01, 2026 - Mar 05, 2026");
    expect(dates).toHaveAttribute("data-overdue", "true");
  });

  it("keeps a finished record's past range in the plain tone", () => {
    render(
      <WorkItemPreviewCard
        issue={{ ...ISSUE, start_date: "2026-03-01", target_date: "2026-03-05" }}
        projectIdentifier="WEB"
        state={{ name: "Done", group: "completed" }}
        today={TODAY}
      />
    );
    expect(screen.getByTestId("preview-card-dates")).not.toHaveAttribute("data-overdue");
  });

  it("shows a lone start or due date", () => {
    const { rerender } = render(
      <WorkItemPreviewCard
        issue={{ ...ISSUE, start_date: "2026-04-01" }}
        projectIdentifier="WEB"
        state={{ name: "Doing", group: "started" }}
        today={TODAY}
      />
    );
    expect(screen.getByTestId("preview-card-dates")).toHaveTextContent("Apr 01, 2026");
    expect(screen.getByTestId("preview-card-dates")).not.toHaveAttribute("data-overdue");
    rerender(
      <WorkItemPreviewCard
        issue={{ ...ISSUE, target_date: "2026-03-09" }}
        projectIdentifier="WEB"
        state={{ name: "Doing", group: "started" }}
        today={TODAY}
      />
    );
    expect(screen.getByTestId("preview-card-dates")).toHaveTextContent("Mar 09, 2026");
    expect(screen.getByTestId("preview-card-dates")).toHaveAttribute("data-overdue", "true");
  });

  it("uses the local calendar day when the caller passes none", () => {
    render(
      <WorkItemPreviewCard
        issue={{ ...ISSUE, target_date: "2999-01-01" }}
        projectIdentifier="WEB"
        state={{ name: "Doing", group: "started" }}
      />
    );
    expect(screen.getByTestId("preview-card-dates")).toHaveTextContent("Jan 01, 2999");
    expect(screen.getByTestId("preview-card-dates")).not.toHaveAttribute("data-overdue");
  });

  it("falls back to a neutral glyph for an unknown state group or priority", () => {
    render(
      <WorkItemPreviewCard
        issue={{ ...ISSUE, priority: "nonsense" as never }}
        projectIdentifier="WEB"
        state={{ name: "Odd", group: "mystery" }}
        today={TODAY}
      />
    );
    expect(screen.getByTestId("preview-card-state")).toHaveTextContent("Odd");
    expect(screen.getByTestId("preview-card-priority")).toHaveAttribute("title", "nonsense");
    expect(screen.getByTestId("preview-card-priority").querySelector("svg")).toHaveClass("lucide-circle-dashed");
  });
});

describe("WorkItemPreviewHover", () => {
  it("opens the card beside its trigger on hover", async () => {
    const user = userEvent.setup();
    render(
      <WorkItemPreviewHover
        trigger={
          <a href="/acme/browse/WEB-42/" onClick={(event: React.MouseEvent) => event.preventDefault()}>
            WEB-42
          </a>
        }
        issue={ISSUE}
        projectIdentifier="WEB"
        state={{ name: "In progress", group: "started" }}
        today={TODAY}
      />
    );
    expect(screen.queryByTestId("work-item-preview")).not.toBeInTheDocument();
    await user.hover(screen.getByRole("link", { name: "WEB-42" }));
    const card = await screen.findByTestId("work-item-preview");
    expect(card).toHaveTextContent("WEB-42");
    expect(card).toHaveTextContent("Ship the preview");
  });
});
