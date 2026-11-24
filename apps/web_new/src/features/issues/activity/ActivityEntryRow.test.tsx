// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One property-change row (ISS-194): the actor link, the paraphrased
// sentence, the age, the null return for unknown fields and the product's
// own actions credited without a member link.
import { render, screen } from "@testing-library/react";
import type { IssueActivity } from "@pidash/api-client";
import * as React from "react";
import { describe, expect, it } from "vitest";

import { ActivityEntryRow, activityBadgeFor } from "./ActivityEntryRow.js";

const ADA = "523e4567-e89b-12d3-a456-426614174004";

const ADA_DETAIL = {
  id: ADA,
  first_name: "Ada",
  last_name: "L",
  avatar: "",
  avatar_url: null,
  display_name: "Ada L",
  is_bot: false,
};

function activity(overrides: Partial<IssueActivity> = {}): IssueActivity {
  return {
    id: "a13e4567-e89b-12d3-a456-426614174001",
    issue: "123e4567-e89b-12d3-a456-426614174000",
    project: "223e4567-e89b-12d3-a456-426614174001",
    workspace: "423e4567-e89b-12d3-a456-426614174003",
    actor: ADA,
    actor_detail: ADA_DETAIL,
    verb: "created",
    field: null,
    old_value: null,
    new_value: null,
    comment: "",
    attachments: [],
    issue_comment: null,
    old_identifier: null,
    new_identifier: null,
    epoch: null,
    source_data: null,
    issue_detail: null,
    project_detail: null,
    workspace_detail: null,
    created_at: new Date(Date.now() - 5 * 60_000).toISOString(),
    updated_at: new Date(Date.now() - 5 * 60_000).toISOString(),
    created_by: null,
    updated_by: null,
    deleted_at: null,
    ...overrides,
  };
}

describe("ActivityEntryRow", () => {
  it("credits the actor, links the profile and paraphrases the change", () => {
    render(<ActivityEntryRow workspaceSlug="acme" activity={activity()} />);
    const link = screen.getByRole("link", { name: "Ada L" });
    expect(link).toHaveAttribute("href", `/acme/profile/${ADA}`);
    expect(screen.getByText("created the work item.")).toBeInTheDocument();
  });

  it("hangs a type badge on the rail instead of an avatar", () => {
    const { container } = render(
      <ActivityEntryRow
        workspaceSlug="acme"
        activity={activity({ actor_detail: { ...ADA_DETAIL, avatar_url: "https://img.test/ada.png" } })}
      />
    );
    expect(screen.getByRole("img", { name: "Creation" })).toBeInTheDocument();
    expect(container.querySelector("img")).toBeNull();
  });

  it("shows the age of the row", () => {
    render(<ActivityEntryRow workspaceSlug="acme" activity={activity()} />);
    expect(screen.getByText("5 minutes ago")).toBeInTheDocument();
  });

  it("drops rows for fields the feed does not describe", () => {
    const { container } = render(
      <ActivityEntryRow workspaceSlug="acme" activity={activity({ field: "mystery", verb: "updated" })} />
    );
    expect(container).toBeEmptyDOMElement();
  });

  it("renders the deletion row the server stamps with field issue", () => {
    render(
      <ActivityEntryRow
        workspaceSlug="acme"
        activity={activity({ field: "issue", verb: "deleted", old_value: null, new_value: null })}
      />
    );
    expect(screen.getByText("deleted the work item.")).toBeInTheDocument();
  });

  it("renders a points estimate change with the actor profile link (EST-023)", () => {
    render(
      <ActivityEntryRow
        workspaceSlug="acme"
        activity={activity({
          field: "estimate_points",
          verb: "updated",
          old_value: null,
          new_value: "1",
        })}
      />
    );
    expect(screen.getByText("set the estimate point to 1.")).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Ada L" })).toHaveAttribute("href", `/acme/profile/${ADA}`);
  });

  it("renders a categories estimate change naming the value (EST-023)", () => {
    render(
      <ActivityEntryRow
        workspaceSlug="acme"
        activity={activity({
          field: "estimate_categories",
          verb: "updated",
          old_value: "Big",
          new_value: "Small",
        })}
      />
    );
    expect(screen.getByText("set the estimate point to Small.")).toBeInTheDocument();
  });

  it("renders a legacy estimate_point row and an unknown estimate family (EST-023)", () => {
    const { unmount } = render(
      <ActivityEntryRow
        workspaceSlug="acme"
        activity={activity({ field: "estimate_point", verb: "updated", old_value: null, new_value: "3" })}
      />
    );
    expect(screen.getByText("set the estimate point to 3.")).toBeInTheDocument();
    unmount();
    render(
      <ActivityEntryRow
        workspaceSlug="acme"
        activity={activity({ field: "estimate_future", verb: "updated", old_value: null, new_value: "5" })}
      />
    );
    expect(screen.getByText("set the estimate point to 5.")).toBeInTheDocument();
  });

  it("renders the removal shape a fixed server will send (EST-023)", () => {
    render(
      <ActivityEntryRow
        workspaceSlug="acme"
        activity={activity({ field: "estimate_points", verb: "removed", old_value: "3", new_value: null })}
      />
    );
    expect(screen.getByText("removed the estimate point 3.")).toBeInTheDocument();
  });

  it("credits the product for its own actions without a member link", () => {
    render(
      <ActivityEntryRow
        workspaceSlug="acme"
        activity={activity({ field: "agent_wait", verb: "created", old_value: "10", new_value: "1" })}
      />
    );
    expect(screen.queryByRole("link", { name: "Pi Dash" })).not.toBeInTheDocument();
    expect(screen.getByText("Pi Dash")).toBeInTheDocument();
    expect(screen.getByText("chose to wait on a blocker (1 of 10).")).toBeInTheDocument();
    expect(screen.getByRole("img", { name: "Agent wait" })).toBeInTheDocument();
  });

  it("keeps the product credit unlinked even when the row names an actor", () => {
    render(
      <ActivityEntryRow
        workspaceSlug="acme"
        activity={activity({ field: "agent_wake", verb: "updated", old_value: "done", new_value: "NEWFRONT-1" })}
      />
    );
    expect(screen.queryByRole("link", { name: "Pi Dash" })).not.toBeInTheDocument();
    expect(screen.getByText("Pi Dash")).toBeInTheDocument();
  });

  it("falls back for an unknown actor", () => {
    render(<ActivityEntryRow workspaceSlug="acme" activity={activity({ actor: null, actor_detail: null })} />);
    expect(screen.getByText("Unknown author")).toBeInTheDocument();
  });
});

describe("activityBadgeFor", () => {
  it("names a badge per entry type", () => {
    const cases: Array<[Partial<IssueActivity>, string]> = [
      [{ field: null, verb: "created" }, "Creation"],
      [{ field: null, verb: "deleted" }, "Deletion"],
      [{ field: "issue", verb: "deleted" }, "Deletion"],
      [{ field: "name", verb: "updated", new_value: "Renamed" }, "Name"],
      [{ field: "description", verb: "updated" }, "Description"],
      [{ field: "state", verb: "updated", new_value: "Done" }, "State"],
      [{ field: "assignees", verb: "created", new_value: "Bob" }, "Assignees"],
      [{ field: "priority", verb: "updated", new_value: "high" }, "Priority"],
      [{ field: "labels", verb: "created", new_value: "bug" }, "Labels"],
      [{ field: "estimate_points", verb: "updated", new_value: "3" }, "Estimate"],
      [{ field: "estimate_point", verb: "updated", new_value: "3" }, "Estimate"],
      [{ field: "parent", verb: "updated", new_value: "PAR-9" }, "Parent"],
      [{ field: "blocking", verb: "created", new_value: "PAR-1" }, "Blocking"],
      [{ field: "blocked_by", verb: "created", new_value: "PAR-2" }, "Blocked by"],
      [{ field: "duplicate", verb: "created", new_value: "PAR-3" }, "Duplicate"],
      [{ field: "relates_to", verb: "created", new_value: "PAR-4" }, "Related"],
      [{ field: "start_date", verb: "updated", new_value: "2026-10-01" }, "Start date"],
      [{ field: "target_date", verb: "updated", new_value: "2026-10-09" }, "Due date"],
      [{ field: "cycles", verb: "created", new_value: "Sprint 4" }, "Cycle"],
      [{ field: "modules", verb: "created", new_value: "Core" }, "Module"],
      [{ field: "link", verb: "created" }, "Link"],
      [{ field: "attachment", verb: "created" }, "Attachment"],
      [{ field: "archived_at", verb: "updated", new_value: "archive" }, "Archived"],
      [{ field: "archived_at", verb: "updated", new_value: "restore" }, "Restored"],
      [{ field: "agent_wait", verb: "created", old_value: "10", new_value: "1" }, "Agent wait"],
      [{ field: "agent_wake", verb: "created", old_value: "completed", new_value: "PAR-1" }, "Agent wake"],
      [{ field: "intake", verb: "1" }, "Intake"],
      [{ field: "inbox", verb: "-1" }, "Intake"],
      [{ field: "worklog", verb: "created", new_value: "90" }, "Worklog"],
      [{ field: "mystery", verb: "updated" }, "Activity"],
    ];
    for (const [row, label] of cases) {
      expect(activityBadgeFor({ field: null, verb: "created", new_value: null, ...row }).label).toBe(label);
    }
  });

  it("gives relations a badge each instead of one shared badge", () => {
    const icons = new Set(
      (["blocking", "blocked_by", "duplicate", "relates_to"] as const).map(
        (field) => activityBadgeFor({ field, verb: "created", new_value: "PAR-1" }).icon
      )
    );
    expect(icons.size).toBe(4);
  });
});
