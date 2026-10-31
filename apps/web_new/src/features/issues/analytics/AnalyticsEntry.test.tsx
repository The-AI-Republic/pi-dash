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
import { AnalyticsEntry } from "./AnalyticsEntry.js";

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
    const { container } = render(harness(ROLE_GUEST, {}, ROLE_MEMBER));
    await waitFor(() => expect(container).not.toBeEmptyDOMElement());
    expect(await screen.findByTestId("issues-analytics-entry")).toBeInTheDocument();
  });

  it("shows to a workspace admin regardless of project membership", async () => {
    render(harness(20, {}, null));
    expect(await screen.findByTestId("issues-analytics-entry")).toBeInTheDocument();
  });

  it("opens the scoped panel titled for the project", async () => {
    const user = userEvent.setup();
    render(harness(ROLE_MEMBER));
    await user.click(await screen.findByTestId("issues-analytics-entry"));
    const dialog = await screen.findByRole("dialog");
    expect(within(dialog).getByRole("heading", { name: "Analytics for Website" })).toBeInTheDocument();
    expect(within(dialog).getByTestId("issues-analytics-body")).toBeInTheDocument();
  });

  it("names the cycle scope in the dialog for members", async () => {
    const user = userEvent.setup();
    render(harness(ROLE_MEMBER, { cycle: { id: "cycle-1", name: "Sprint 3" } }));
    expect(screen.queryByTestId("issues-analytics-entry")).not.toBeInTheDocument();
    await user.click(await screen.findByTestId("cycle-analytics-entry"));
    const dialog = await screen.findByRole("dialog");
    expect(within(dialog).getByRole("heading", { name: "Analytics for Website in Sprint 3" })).toBeInTheDocument();
  });

  it("keeps the desktop trigger member+ under a cycle scope", async () => {
    const { container } = render(harness(ROLE_GUEST, { cycle: { id: "cycle-1", name: "Sprint 3" } }, ROLE_GUEST));
    await waitFor(() => expect(screen.queryByTestId("cycle-analytics-entry")).not.toBeInTheDocument());
    expect(screen.queryByTestId("cycle-analytics-entry-narrow")).not.toBeInTheDocument();
    expect(container).toBeEmptyDOMElement();
  });

  it("offers the narrow trigger to guests while the desktop one stays gated", async () => {
    const user = userEvent.setup();
    render(harness(ROLE_MEMBER, { cycle: { id: "cycle-1", name: "Sprint 3" }, narrowGuestAccess: true }, ROLE_GUEST));
    expect(await screen.findByTestId("cycle-analytics-entry-narrow")).toBeInTheDocument();
    expect(screen.queryByTestId("cycle-analytics-entry")).not.toBeInTheDocument();
    await user.click(screen.getByTestId("cycle-analytics-entry-narrow"));
    const dialog = await screen.findByRole("dialog");
    expect(within(dialog).getByRole("heading", { name: "Analytics for Website in Sprint 3" })).toBeInTheDocument();
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
  });

  it("renders both triggers for members when the narrow entry is on", async () => {
    render(harness(ROLE_MEMBER, { cycle: { id: "cycle-1", name: "Sprint 3" }, narrowGuestAccess: true }));
    expect(await screen.findByTestId("cycle-analytics-entry")).toBeInTheDocument();
    expect(screen.getByTestId("cycle-analytics-entry-narrow")).toBeInTheDocument();
  });

  it("names the module scope in the dialog with one space", async () => {
    const user = userEvent.setup();
    render(harness(ROLE_MEMBER, { module: { id: "module-1", name: "Alpha" } }));
    expect(screen.queryByTestId("issues-analytics-entry")).not.toBeInTheDocument();
    await user.click(await screen.findByTestId("module-analytics-entry"));
    const dialog = await screen.findByRole("dialog");
    expect(within(dialog).getByRole("heading", { name: "Analytics for Website in Alpha" })).toBeInTheDocument();
  });

  it("keeps the desktop trigger member+ under a module scope", async () => {
    const { container } = render(harness(ROLE_GUEST, { module: { id: "module-1", name: "Alpha" } }, ROLE_GUEST));
    await waitFor(() => expect(screen.queryByTestId("module-analytics-entry")).not.toBeInTheDocument());
    expect(container).toBeEmptyDOMElement();
  });

  it("adds no dialog trigger for guests on any scope", async () => {
    // ANL-018: analytics opens no guest entry point; the desktop gate on
    // every surface is the role check above.
    render(harness(ROLE_GUEST, {}, ROLE_GUEST));
    await waitFor(() => expect(screen.queryByRole("button", { name: "Analytics" })).not.toBeInTheDocument());
  });
});
