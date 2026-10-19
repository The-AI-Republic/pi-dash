// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Issue } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { EMOJI_PICKER_LABEL } from "../reactions/EmojiPicker.js";
import { ISSUE_REACTIONS_LABEL } from "../reactions/IssueReactions.js";
import { DESCRIPTION_LABEL } from "./IssueDescriptionField.js";
import { TITLE_LABEL } from "./IssueTitleField.js";
import { DetailMainColumn } from "./DetailMainColumn.js";

const SLUG = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const ISSUE = "123e4567-e89b-12d3-a456-426614174000";
const ADA = "523e4567-e89b-12d3-a456-426614174004";
const V1 = "323e4567-e89b-12d3-a456-426614174002";

function issueFixture(overrides: Partial<Issue> = {}): Issue {
  return {
    id: ISSUE,
    name: "Fix login",
    state_id: null,
    sort_order: 1,
    completed_at: null,
    estimate_point: null,
    priority: "high",
    sequence_id: 42,
    project_id: PROJECT,
    parent_id: null,
    cycle_id: null,
    module_ids: [],
    label_ids: [],
    assignee_ids: [],
    sub_issues_count: null,
    attachment_count: null,
    link_count: null,
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-02-03T04:05:06Z",
    created_by: ADA,
    updated_by: null,
    is_draft: false,
    archived_at: null,
    complexity_score: 0,
    agent_executor: null,
    is_synced: false,
    description_html: "<p>Current words</p>",
    is_subscribed: false,
    agent_ticker: null,
    agent_status: null,
    relations_summary: { blocked_by: [], blocking: [] },
    has_open_blockers: false,
    ...overrides,
  };
}

interface RenderedColumn {
  patches: { url: string; body: string }[];
}

function renderColumn(
  props: Partial<React.ComponentProps<typeof DetailMainColumn>> = {},
  stub: { patchStatus?: number } = {}
): RenderedColumn & { rerender: (next: Partial<React.ComponentProps<typeof DetailMainColumn>>) => void } {
  const patches: { url: string; body: string }[] = [];
  const hourAgo = new Date(Date.now() - 3_600_000).toISOString();
  createAppClient("https://api.test", async (url: string, init: { method: string; body?: string | undefined }) => {
    let status = 404;
    let body = "{}";
    if (url.includes(`/description-versions/${V1}/`)) {
      status = 200;
      body = JSON.stringify({
        id: V1,
        workspace: "423e4567-e89b-12d3-a456-426614174003",
        project: PROJECT,
        issue: ISSUE,
        last_saved_at: hourAgo,
        owned_by: ADA,
        created_at: hourAgo,
        updated_at: hourAgo,
        created_by: ADA,
        updated_by: ADA,
        description_binary: null,
        description_html: "<p>version one</p>",
        description_stripped: "version one",
        description_json: {},
      });
    } else if (url.includes("description-versions")) {
      status = 200;
      body = JSON.stringify({
        prev_cursor: "",
        cursor: "",
        next_cursor: null,
        prev_page_results: false,
        next_page_results: false,
        page_count: 1,
        total_results: 1,
        total_pages: 1,
        results: [
          {
            id: V1,
            workspace: "423e4567-e89b-12d3-a456-426614174003",
            project: PROJECT,
            issue: ISSUE,
            last_saved_at: hourAgo,
            owned_by: ADA,
            created_at: hourAgo,
            updated_at: hourAgo,
            created_by: ADA,
            updated_by: ADA,
          },
        ],
      });
    } else if (url.endsWith("/members/")) {
      status = 200;
      body = JSON.stringify([
        {
          id: "823e4567-e89b-12d3-a456-426614174007",
          member: {
            id: ADA,
            first_name: "Ada",
            last_name: "L",
            avatar: "",
            avatar_url: null,
            display_name: "Ada L",
            is_bot: false,
          },
          role: 15,
        },
      ]);
    } else if (init.method === "PATCH") {
      status = stub.patchStatus ?? 204;
      patches.push({ url, body: init.body ?? "" });
      body = "";
    }
    return { status, url, headers: { get: () => null }, text: async () => body };
  });
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  const base = { workspaceSlug: SLUG, projectId: PROJECT, issue: issueFixture(), editable: true };
  const mounted = render(
    <QueryClientProvider client={queryClient}>
      <DetailMainColumn {...base} {...props} />
    </QueryClientProvider>
  );
  return {
    patches,
    rerender: (next: Partial<React.ComponentProps<typeof DetailMainColumn>>) => {
      mounted.rerender(
        <QueryClientProvider client={queryClient}>
          <DetailMainColumn {...base} {...props} {...next} />
        </QueryClientProvider>
      );
    },
  };
}

function descriptionView(): HTMLElement {
  return screen.getByRole("region", { name: DESCRIPTION_LABEL });
}

afterEach(() => {
  resetAppClient();
});

describe("DetailMainColumn", () => {
  it("saves title edits through the item update", async () => {
    const { patches } = renderColumn();
    const box = screen.getByRole("textbox", { name: TITLE_LABEL });
    fireEvent.change(box, { target: { value: "Renamed" } });
    fireEvent.blur(box);
    await waitFor(() => expect(patches).toHaveLength(1));
    expect(patches[0]?.url).toContain(`/issues/${ISSUE}/`);
    expect(JSON.parse(patches[0]?.body ?? "{}")).toEqual({ name: "Renamed" });
  });

  it("restores a version at once and holds it until the server confirms", async () => {
    const user = userEvent.setup();
    const { patches, rerender } = renderColumn();
    await user.click(await screen.findByRole("button", { name: /Edited by/ }));
    await user.click(await screen.findByRole("menuitem", { name: "Ada L 1 hour ago" }));
    const dialog = await screen.findByRole("dialog", { name: "Description history" });
    await within(dialog).findByText("version one");
    await user.click(within(dialog).getByRole("button", { name: "Restore" }));
    await waitFor(() => expect(patches).toHaveLength(1));
    expect(JSON.parse(patches[0]?.body ?? "{}")).toEqual({ description_html: "<p>version one</p>" });
    // The view shows the version ahead of the server round-trip.
    expect(descriptionView()).toHaveTextContent("version one");
    // A divergent server value does not dislodge the pending restore.
    rerender({ issue: issueFixture({ description_html: "<p>external</p>" }) });
    expect(descriptionView()).toHaveTextContent("version one");
    // The confirming value clears it back onto the server copy.
    rerender({ issue: issueFixture({ description_html: "<p>version one</p>" }) });
    expect(descriptionView()).toHaveTextContent("version one");
  });

  it("reverts the view when a restore is refused", async () => {
    const user = userEvent.setup();
    renderColumn({}, { patchStatus: 403 });
    await user.click(await screen.findByRole("button", { name: /Edited by/ }));
    await user.click(await screen.findByRole("menuitem", { name: "Ada L 1 hour ago" }));
    const dialog = await screen.findByRole("dialog", { name: "Description history" });
    await within(dialog).findByText("version one");
    await user.click(within(dialog).getByRole("button", { name: "Restore" }));
    await waitFor(() => expect(descriptionView()).toHaveTextContent("Current words"));
  });

  it("reads only for guests, with no history line", () => {
    renderColumn({ editable: false });
    expect(screen.getByRole("heading", { name: "Fix login" })).toBeInTheDocument();
    expect(screen.queryByRole("textbox", { name: TITLE_LABEL })).not.toBeInTheDocument();
    expect(descriptionView()).toHaveTextContent("Current words");
    expect(screen.queryByRole("button", { name: /Edited by/ })).not.toBeInTheDocument();
  });

  it("keeps the reaction row available to a guest who cannot edit", () => {
    renderColumn({ editable: false });
    expect(screen.getByRole("region", { name: ISSUE_REACTIONS_LABEL })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: EMOJI_PICKER_LABEL })).toBeEnabled();
  });

  it("keeps the reaction row available on a synced issue", () => {
    renderColumn({ issue: issueFixture({ is_synced: true }) });
    expect(screen.getByRole("region", { name: ISSUE_REACTIONS_LABEL })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: EMOJI_PICKER_LABEL })).toBeEnabled();
  });

  it("leaves the reaction row on an archived issue without the add control", () => {
    renderColumn({ issue: issueFixture({ archived_at: "2024-03-04T00:00:00Z" }) });
    expect(screen.getByRole("region", { name: ISSUE_REACTIONS_LABEL })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: EMOJI_PICKER_LABEL })).not.toBeInTheDocument();
  });

  it("reads only for synced and archived issues, with restore hidden", async () => {
    const user = userEvent.setup();
    const { rerender } = renderColumn({ issue: issueFixture({ is_synced: true }) });
    expect(screen.getByRole("heading", { name: "Fix login" })).toBeInTheDocument();
    await user.click(await screen.findByRole("button", { name: /Edited by/ }));
    await user.click(await screen.findByRole("menuitem", { name: "Ada L 1 hour ago" }));
    const dialog = await screen.findByRole("dialog", { name: "Description history" });
    await within(dialog).findByText("version one");
    expect(within(dialog).queryByRole("button", { name: "Restore" })).not.toBeInTheDocument();
    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
    rerender({ issue: issueFixture({ archived_at: "2024-03-04T00:00:00Z" }) });
    expect(screen.getByRole("heading", { name: "Fix login" })).toBeInTheDocument();
    expect(screen.queryByRole("textbox", { name: TITLE_LABEL })).not.toBeInTheDocument();
    await user.click(await screen.findByRole("button", { name: /Edited by/ }));
    await user.click(await screen.findByRole("menuitem", { name: "Ada L 1 hour ago" }));
    const reopened = await screen.findByRole("dialog", { name: "Description history" });
    await within(reopened).findByText("version one");
    expect(within(reopened).queryByRole("button", { name: "Restore" })).not.toBeInTheDocument();
  });

  it("saves description edits through the item update", async () => {
    const user = userEvent.setup();
    const { patches } = renderColumn();
    await user.click(screen.getByRole("button", { name: "Edit description" }));
    await vi.dynamicImportSettled();
    const editable = await screen.findByLabelText(DESCRIPTION_LABEL);
    // The field autofocuses once the lazy editor mounts; typing before
    // that move lands can have the focus change swallow keystrokes.
    await waitFor(() => expect(editable).toHaveFocus());
    await user.keyboard(" more");
    expect(editable).toHaveTextContent("Current words more");
    fireEvent.blur(editable);
    await waitFor(() => expect(patches).toHaveLength(1));
    expect(patches[0]?.url).toContain(`/issues/${ISSUE}/`);
    expect(JSON.parse(patches[0]?.body ?? "{}")).toEqual({
      description_html: "<p>Current words more</p>",
    });
  }, 30_000);

  it("renders the header and activity slots", () => {
    renderColumn({ topSlot: <p>header slot</p>, activity: <p>activity slot</p> });
    expect(screen.getByText("header slot")).toBeInTheDocument();
    expect(screen.getByText("activity slot")).toBeInTheDocument();
  });

  it("keeps the reaction row on the description-meta row beside the history line", async () => {
    renderColumn({ activity: <p>activity slot</p> });
    const reactions = screen.getByRole("region", { name: ISSUE_REACTIONS_LABEL });
    const history = await screen.findByRole("button", { name: /Edited by/ });
    const meta = reactions.closest('[data-slot="description-meta"]');
    expect(meta).not.toBeNull();
    expect(meta).toContainElement(history);
    const activity = screen.getByText("activity slot");
    expect(meta).not.toContainElement(activity);
    expect(meta?.compareDocumentPosition(activity) ?? 0).toBe(Node.DOCUMENT_POSITION_FOLLOWING);
  });

  it("keeps the reaction row on the meta row for a guest with no history line", () => {
    renderColumn({ editable: false, activity: <p>activity slot</p> });
    const reactions = screen.getByRole("region", { name: ISSUE_REACTIONS_LABEL });
    const meta = reactions.closest('[data-slot="description-meta"]');
    expect(meta).not.toBeNull();
    expect(meta).not.toContainElement(screen.getByText("activity slot"));
    expect(screen.queryByRole("button", { name: /Edited by/ })).not.toBeInTheDocument();
  });
});
