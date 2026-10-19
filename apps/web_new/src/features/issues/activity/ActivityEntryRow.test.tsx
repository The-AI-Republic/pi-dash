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

import { ActivityEntryRow } from "./ActivityEntryRow.js";

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

  it("credits the product for its own actions without a member link", () => {
    render(
      <ActivityEntryRow
        workspaceSlug="acme"
        activity={activity({ field: "agent_wait", verb: "updated", old_value: "Review", new_value: "NEWFRONT-1" })}
      />
    );
    expect(screen.queryByRole("link", { name: "Pi Dash" })).not.toBeInTheDocument();
    expect(screen.getByText("Pi Dash")).toBeInTheDocument();
    expect(screen.getByText("started waiting on NEWFRONT-1 (Review).")).toBeInTheDocument();
  });

  it("falls back for an unknown actor", () => {
    render(<ActivityEntryRow workspaceSlug="acme" activity={activity({ actor: null, actor_detail: null })} />);
    expect(screen.getByText("Unknown author")).toBeInTheDocument();
  });
});
