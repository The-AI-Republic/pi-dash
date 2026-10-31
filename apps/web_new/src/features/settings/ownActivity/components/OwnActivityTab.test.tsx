// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Own-history tab (parity/specs/profile.md §7, PROF-019): paged rows with
// a load-more control, a themed empty illustration, loading and failure.
import { fireEvent, render, screen, within } from "@testing-library/react";
import * as React from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const harness = vi.hoisted(() => ({
  pages: {} as Record<string, unknown>,
  me: undefined as Record<string, unknown> | undefined,
  scheme: "light" as string,
  customDark: false,
}));

vi.mock("../../../../core/session/index.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../../core/session/index.js")>()),
  useMe: () => ({ data: harness.me }),
}));

vi.mock("../../../../core/theme/index.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../../core/theme/index.js")>()),
  useResolvedScheme: () => ({ scheme: harness.scheme, customDark: harness.customDark }),
}));

vi.mock("../api.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../api.js")>()),
  useOwnActivityPages: () => harness.pages,
}));

import type { IssueActivity } from "@pidash/api-client";

import {
  ACTIVITY_EMPTY_TITLE,
  ACTIVITY_LOAD_FAILED,
  ACTIVITY_LOAD_MORE_LABEL,
  OwnActivityTab,
} from "./OwnActivityTab.js";

// Migrated-route links (F-09, NEWFRONT-528) render through the workspace
// routes this component ships in. The suite has no router; the router
// link's navigation is TanStack's own tested behavior, so it stands in as
// an anchor and the assertions still read the href.
vi.mock("@tanstack/react-router", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@tanstack/react-router")>()),
  Link: (props: { to?: string; children?: React.ReactNode } & Record<string, unknown>) => {
    const { to, children, ...rest } = props;
    return (
      <a href={to} {...rest}>
        {children}
      </a>
    );
  },
}));

const UUID = "123e4567-e89b-12d3-a456-426614174000";
const UUID2 = "223e4567-e89b-12d3-a456-426614174001";

function activity(patch: Partial<IssueActivity> = {}): IssueActivity {
  return {
    id: UUID,
    issue: UUID2,
    project: UUID,
    workspace: UUID2,
    actor: UUID,
    actor_detail: null,
    verb: "updated",
    field: "state",
    old_value: null,
    new_value: "Started",
    comment: "",
    attachments: [],
    issue_comment: null,
    old_identifier: null,
    new_identifier: null,
    epoch: null,
    source_data: null,
    created_at: "2026-10-17T04:00:09Z",
    updated_at: "2026-10-17T04:00:09Z",
    created_by: null,
    updated_by: null,
    deleted_at: null,
    issue_detail: { id: UUID2, name: "Fix the clock", sequence_id: 7 },
    project_detail: { id: UUID, identifier: "CT", name: "Control" },
    workspace_detail: { id: UUID2, name: "Acme", slug: "acme" },
    ...patch,
  };
}

const NOW = new Date("2026-10-17T05:00:09Z");

function ready(patch: Record<string, unknown> = {}): void {
  harness.pages = {
    data: { pages: [{ results: [activity()] }] },
    isPending: false,
    isError: false,
    hasNextPage: false,
    isFetchingNextPage: false,
    fetchNextPage: vi.fn(),
    ...patch,
  };
}

beforeEach(() => {
  harness.scheme = "light";
  harness.me = { user_timezone: "UTC" };
  ready();
});

describe("OwnActivityTab", () => {
  it("holds a loading frame until the first page arrives", () => {
    harness.pages = { data: undefined, isPending: true, isError: false, hasNextPage: false, isFetchingNextPage: false };
    render(<OwnActivityTab now={NOW} />);
    expect(screen.getByTestId("own-activity-loading")).toBeInTheDocument();
    expect(screen.queryByTestId("settings-activity-list")).not.toBeInTheDocument();
  });

  it("reports a failed read", () => {
    harness.pages = { data: undefined, isPending: false, isError: true, hasNextPage: false, isFetchingNextPage: false };
    render(<OwnActivityTab now={NOW} />);
    expect(screen.getByRole("alert")).toHaveTextContent(ACTIVITY_LOAD_FAILED);
  });

  it("renders each row with its reference, sentence, workspace and age", () => {
    render(<OwnActivityTab now={NOW} />);
    const list = screen.getByTestId("settings-activity-list");
    expect(within(list).getAllByRole("listitem")).toHaveLength(1);
    expect(list).toHaveTextContent("CT-7");
    expect(list).toHaveTextContent("Fix the clock");
    expect(list).toHaveTextContent("Moved the state to Started");
    expect(list).toHaveTextContent("Acme");
    expect(list).toHaveTextContent("1 hour ago");
    expect(within(list).getByText("1 hour ago")).toHaveAttribute("dateTime", "2026-10-17T04:00:09Z");
  });

  it("links a row when the item still resolves", () => {
    render(<OwnActivityTab now={NOW} />);
    expect(screen.getByRole("link")).toHaveAttribute("href", "/acme/browse/CT-7");
  });

  it("renders an unlinked row for a deleted item", () => {
    ready({ data: { pages: [{ results: [activity({ issue_detail: null, field: "comment", new_value: "hello" })] }] } });
    render(<OwnActivityTab now={NOW} />);
    const row = screen.getByTestId("own-activity-row");
    expect(row).toHaveTextContent("A deleted work item");
    expect(row).toHaveTextContent("Commented: hello");
    expect(screen.queryByRole("link")).not.toBeInTheDocument();
  });

  it("hides the workspace line when the workspace is gone", () => {
    ready({ data: { pages: [{ results: [activity({ workspace_detail: null })] }] } });
    render(<OwnActivityTab now={NOW} />);
    expect(screen.getByTestId("own-activity-row")).not.toHaveTextContent("Acme");
  });

  it("shows the light illustration for an empty history", () => {
    ready({ data: { pages: [{ results: [] }] } });
    render(<OwnActivityTab now={NOW} />);
    const image = screen.getByTitle(ACTIVITY_EMPTY_TITLE);
    expect(image).toHaveAttribute("src", "/empty-state/activity/activity-light.svg");
  });

  it("swaps to the dark illustration on a dark scheme", () => {
    ready({ data: { pages: [{ results: [] }] } });
    harness.scheme = "dark";
    const { unmount } = render(<OwnActivityTab now={NOW} />);
    expect(screen.getByTitle(ACTIVITY_EMPTY_TITLE)).toHaveAttribute("src", "/empty-state/activity/activity-dark.svg");
    unmount();
    harness.scheme = "dark-contrast";
    render(<OwnActivityTab now={NOW} />);
    expect(screen.getByTitle(ACTIVITY_EMPTY_TITLE)).toHaveAttribute("src", "/empty-state/activity/activity-dark.svg");
  });

  it("follows the custom palette's base for the illustration", () => {
    ready({ data: { pages: [{ results: [] }] } });
    harness.scheme = "custom";
    harness.customDark = false;
    const { unmount } = render(<OwnActivityTab now={NOW} />);
    // A custom theme on a light base keeps the light art.
    expect(screen.getByTitle(ACTIVITY_EMPTY_TITLE)).toHaveAttribute("src", "/empty-state/activity/activity-light.svg");
    unmount();
    harness.customDark = true;
    render(<OwnActivityTab now={NOW} />);
    expect(screen.getByTitle(ACTIVITY_EMPTY_TITLE)).toHaveAttribute("src", "/empty-state/activity/activity-dark.svg");
  });

  it("keeps the light illustration for either light scheme", () => {
    ready({ data: { pages: [{ results: [] }] } });
    harness.customDark = false;
    harness.scheme = "light-contrast";
    render(<OwnActivityTab now={NOW} />);
    expect(screen.getByTitle(ACTIVITY_EMPTY_TITLE)).toHaveAttribute("src", "/empty-state/activity/activity-light.svg");
  });

  it("loads the next page only while the server reports more", async () => {
    const fetchNextPage = vi.fn(async () => undefined);
    ready({ hasNextPage: true, fetchNextPage });
    const { rerender } = render(<OwnActivityTab now={NOW} />);
    fireEvent.click(screen.getByRole("button", { name: ACTIVITY_LOAD_MORE_LABEL }));
    expect(fetchNextPage).toHaveBeenCalledTimes(1);
    ready({ hasNextPage: false, fetchNextPage });
    rerender(<OwnActivityTab now={NOW} />);
    expect(screen.queryByRole("button", { name: ACTIVITY_LOAD_MORE_LABEL })).not.toBeInTheDocument();
  });

  it("marks the load-more control busy while the next page is in flight", () => {
    ready({ hasNextPage: true, isFetchingNextPage: true });
    render(<OwnActivityTab now={NOW} />);
    expect(screen.getByRole("button", { name: ACTIVITY_LOAD_MORE_LABEL })).toBeDisabled();
  });

  it("flattens rows from every loaded page", () => {
    ready({
      data: {
        pages: [{ results: [activity({ id: UUID })] }, { results: [activity({ id: UUID2, new_value: "Done" })] }],
      },
    });
    render(<OwnActivityTab now={NOW} />);
    expect(screen.getAllByTestId("own-activity-row")).toHaveLength(2);
    expect(screen.getByTestId("settings-activity-list")).toHaveTextContent("Moved the state to Done");
  });

  it("reads ages in the member's zone when none is set", () => {
    harness.me = undefined;
    render(<OwnActivityTab now={NOW} />);
    expect(screen.getByTestId("own-activity-row")).toHaveTextContent("1 hour ago");
  });
});
