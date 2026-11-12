// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Transfer-hop note body (ISS-236): the latest cycle-to-cycle hop names
// the previous cycle and the move time; adds and non-moves render nothing.
import { cleanup, render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { IssueActivity } from "@pidash/api-client";

const mocks = vi.hoisted(() => ({ useIssueActivities: vi.fn() }));

vi.mock("../activity/api.js", () => ({ useIssueActivities: mocks.useIssueActivities }));

import type { DetailBadgeInput } from "./providers.js";
import {
  CYCLE_ACTIVITY_FIELD,
  selectTransferHop,
  transferHopLabel,
  TransferHopNoteContent,
} from "./TransferHopNoteContent.js";

afterEach(() => {
  cleanup();
  mocks.useIssueActivities.mockReset();
});

function row(overrides: Partial<IssueActivity> = {}): IssueActivity {
  return {
    id: "323e4567-e89b-12d3-a456-426614174001",
    issue: null,
    project: "323e4567-e89b-12d3-a456-426614174002",
    workspace: "323e4567-e89b-12d3-a456-426614174003",
    actor: null,
    actor_detail: null,
    verb: "updated",
    field: CYCLE_ACTIVITY_FIELD,
    old_value: "Sprint 3",
    new_value: "Sprint 4",
    comment: "",
    attachments: [],
    issue_comment: null,
    old_identifier: "cycle-a",
    new_identifier: "cycle-b",
    epoch: null,
    source_data: null,
    created_at: "2026-10-20T10:00:00.000Z",
    updated_at: "2026-10-20T10:00:00.000Z",
    created_by: null,
    updated_by: null,
    deleted_at: null,
    issue_detail: null,
    project_detail: null,
    workspace_detail: null,
    ...overrides,
  };
}

const BASE: DetailBadgeInput = { workspaceSlug: "acme", projectId: "p-1", issueId: "i-1", cycleId: "cycle-b" };

describe("selectTransferHop", () => {
  it("names the previous cycle and the move time", () => {
    expect(selectTransferHop([row()], "cycle-b")).toEqual({
      fromName: "Sprint 3",
      movedAt: "2026-10-20T10:00:00.000Z",
    });
  });

  it("returns null with no history", () => {
    expect(selectTransferHop([], "cycle-b")).toBeNull();
  });

  it("skips rows that are not cycle moves", () => {
    const rows = [row({ field: "state", new_identifier: "cycle-b", old_identifier: "x", old_value: "Todo" })];
    expect(selectTransferHop(rows, "cycle-b")).toBeNull();
  });

  it("skips hops into other cycles", () => {
    expect(selectTransferHop([row()], "cycle-z")).toBeNull();
  });

  it("returns null when the item was added to the cycle directly", () => {
    const rows = [row({ old_identifier: null, old_value: "" })];
    expect(selectTransferHop(rows, "cycle-b")).toBeNull();
  });

  it("returns null when the stored previous name is blank", () => {
    const rows = [row({ old_value: "" })];
    expect(selectTransferHop(rows, "cycle-b")).toBeNull();
  });

  it("returns null when the stored previous name is missing", () => {
    const rows = [row({ old_value: null })];
    expect(selectTransferHop(rows, "cycle-b")).toBeNull();
  });

  it("takes the latest hop when the item moved more than once", () => {
    const rows = [
      row({ old_identifier: "cycle-a", old_value: "Sprint 3", created_at: "2026-10-18T10:00:00.000Z" }),
      row({ old_identifier: "cycle-a", old_value: "Sprint 3", created_at: "2026-10-20T10:00:00.000Z" }),
    ];
    expect(selectTransferHop(rows, "cycle-b")).toEqual({ fromName: "Sprint 3", movedAt: "2026-10-20T10:00:00.000Z" });
  });

  it("prefers the latest row even when an older hop named another cycle", () => {
    const rows = [
      row({ new_identifier: "cycle-b", old_identifier: "cycle-a", old_value: "Sprint 3" }),
      row({ new_identifier: "cycle-c", old_identifier: "cycle-b", old_value: "Sprint 4" }),
    ];
    expect(selectTransferHop(rows, "cycle-b")).toEqual({ fromName: "Sprint 3", movedAt: "2026-10-20T10:00:00.000Z" });
  });
});

describe("transferHopLabel", () => {
  it("names the previous cycle with a relative time", () => {
    expect(transferHopLabel({ fromName: "Sprint 3", movedAt: new Date().toISOString() })).toBe(
      "Moved from Sprint 3, just now"
    );
  });
});

describe("TransferHopNoteContent", () => {
  it("renders the hop sentence", () => {
    mocks.useIssueActivities.mockReturnValue({ data: [row({ created_at: new Date().toISOString() })] });
    render(<TransferHopNoteContent {...BASE} />);
    expect(screen.getByText("Moved from Sprint 3, just now")).toBeInTheDocument();
    expect(mocks.useIssueActivities).toHaveBeenCalledWith({ workspaceSlug: "acme", projectId: "p-1", issueId: "i-1" });
  });

  it("renders nothing while the history is still loading", () => {
    mocks.useIssueActivities.mockReturnValue({ data: undefined });
    const { container } = render(<TransferHopNoteContent {...BASE} />);
    expect(container).toBeEmptyDOMElement();
  });

  it("renders nothing without a hop", () => {
    mocks.useIssueActivities.mockReturnValue({ data: [] });
    const { container } = render(<TransferHopNoteContent {...BASE} />);
    expect(container).toBeEmptyDOMElement();
  });

  it("renders nothing for a row outside any cycle", () => {
    mocks.useIssueActivities.mockReturnValue({ data: [row()] });
    const { container } = render(<TransferHopNoteContent {...BASE} cycleId={null} />);
    expect(container).toBeEmptyDOMElement();
  });
});
