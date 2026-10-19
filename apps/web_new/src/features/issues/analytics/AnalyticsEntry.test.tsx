// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, describe, expect, it } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { ROLE_GUEST, ROLE_MEMBER } from "../../../core/session/permissions.js";
import { sessionKeys } from "../../../core/session/queries.js";
import { authKeys } from "../../auth/queries.js";
import { pickerKeys } from "../../../shared/pickers/index.js";
import { AnalyticsEntry, groupCounts, type AnalyticsRow } from "./AnalyticsEntry.js";

const WORKSPACE = "123e4567-e89b-12d3-a456-426614174000";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const TODO = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const IN_PROGRESS = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";

const STATES = [
  { id: TODO, group: "unstarted" },
  { id: IN_PROGRESS, group: "started" },
];

function harness(
  role: number,
  props: Partial<React.ComponentProps<typeof AnalyticsEntry>> = {},
  projectRole: number | null = role
): React.ReactElement {
  const workspaces = [{ id: WORKSPACE, name: "Acme", slug: "acme", logo_url: null, total_members: 3, role }];
  createAppClient("https://api.test", (async () => ({
    status: 200,
    url: "",
    headers: { get: () => "application/json" },
    text: async () => "[]",
  })) as never);
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  client.setQueryData(sessionKeys.workspaces, workspaces);
  client.setQueryData(authKeys.projectRoles("acme"), projectRole === null ? {} : { [PROJECT]: projectRole });
  client.setQueryData(pickerKeys.states("acme", PROJECT), STATES);
  return (
    <QueryClientProvider client={client}>
      <AnalyticsEntry workspaceSlug="acme" projectId={PROJECT} projectName="Website" {...props} />
    </QueryClientProvider>
  );
}

afterEach(() => {
  resetAppClient();
});

describe("AnalyticsEntry", () => {
  it("hides from guests", async () => {
    const { container } = render(harness(ROLE_GUEST));
    // Let the role read settle, then assert it never rendered.
    await waitFor(() => expect(screen.queryByTestId("issues-analytics-entry")).not.toBeInTheDocument());
    expect(container).toBeEmptyDOMElement();
  });

  it("hides from a workspace member who is not a member of this project", async () => {
    const { container } = render(harness(ROLE_MEMBER, {}, null));
    await waitFor(() => expect(screen.queryByTestId("issues-analytics-entry")).not.toBeInTheDocument());
    expect(container).toBeEmptyDOMElement();
  });

  it("hides from a project guest", async () => {
    render(harness(ROLE_MEMBER, {}, ROLE_GUEST));
    await waitFor(() => expect(screen.queryByTestId("issues-analytics-entry")).not.toBeInTheDocument());
  });

  it("shows to a project member even without workspace edit rights", async () => {
    // A guest-role workspace row with project membership is unusual but
    // the gate is the project role; the workspace admin path is separate.
    const { container } = render(harness(ROLE_GUEST, { totalCount: 1 }, ROLE_MEMBER));
    await waitFor(() => expect(container).not.toBeEmptyDOMElement());
    expect(await screen.findByTestId("issues-analytics-entry")).toBeInTheDocument();
  });

  it("shows to a workspace admin regardless of project membership", async () => {
    render(harness(20, { totalCount: 1 }, null));
    expect(await screen.findByTestId("issues-analytics-entry")).toBeInTheDocument();
  });

  it("opens a project-scoped dialog with the total and group counts", async () => {
    const user = userEvent.setup();
    render(
      harness(ROLE_MEMBER, { rows: [{ state_id: TODO }, { state_id: TODO }, { state_id: IN_PROGRESS }], totalCount: 3 })
    );
    const entry = await screen.findByTestId("issues-analytics-entry");
    await user.click(entry);
    const dialog = await screen.findByRole("dialog");
    expect(within(dialog).getByText("Work items analytics")).toBeInTheDocument();
    expect(dialog).toHaveTextContent("Work items in Website");
    expect(within(dialog).getByTestId("analytics-total")).toHaveTextContent("3");
    expect(within(dialog).getByText("unstarted")).toBeInTheDocument();
    expect(within(dialog).getByText("started")).toBeInTheDocument();
  });

  it("keeps the server total and skips partial-page group counts", async () => {
    const user = userEvent.setup();
    render(harness(ROLE_MEMBER, { rows: [{ state_id: TODO }], totalCount: 40 }));
    await user.click(await screen.findByTestId("issues-analytics-entry"));
    const dialog = await screen.findByRole("dialog");
    expect(within(dialog).getByTestId("analytics-total")).toHaveTextContent("40");
    expect(within(dialog).queryByText("unstarted")).not.toBeInTheDocument();
  });

  it("names the epic dialog and its empty state", async () => {
    const user = userEvent.setup();
    render(harness(ROLE_MEMBER, { isEpics: true, totalCount: 0 }));
    await user.click(await screen.findByTestId("issues-analytics-entry"));
    const dialog = await screen.findByRole("dialog");
    expect(within(dialog).getByText("Epic analytics")).toBeInTheDocument();
    expect(within(dialog).getByText("No grouped counts to show yet.")).toBeInTheDocument();
    expect(within(dialog).getByTestId("analytics-total")).toHaveTextContent("0");
  });
});

describe("groupCounts", () => {
  it("groups by workflow group in first-seen order", () => {
    const rows: AnalyticsRow[] = [{ state_id: IN_PROGRESS }, { state_id: TODO }, { state_id: IN_PROGRESS }];
    expect(groupCounts(rows, STATES)).toEqual([
      { label: "started", count: 2 },
      { label: "unstarted", count: 1 },
    ]);
    expect(groupCounts([], STATES)).toEqual([]);
  });

  it("labels rows without a state or with an unknown state", () => {
    expect(groupCounts([{ state_id: null }, { state_id: "missing" }], STATES)).toEqual([
      { label: "No state", count: 1 },
      { label: "Other", count: 1 },
    ]);
  });

  it("falls back to Other until the state list resolves", () => {
    expect(groupCounts([{ state_id: TODO }], undefined)).toEqual([{ label: "Other", count: 1 }]);
  });
});
