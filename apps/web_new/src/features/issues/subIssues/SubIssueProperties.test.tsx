// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Inline property controls on a child row (parity/specs/issues.md §15 +
// §17, ISS-177): each visible column owns one control and each choice
// writes exactly one field, the merged range writes both ends in one
// patch, hidden columns disappear, guests get disabled triggers, and the
// compact time cell marks an overdue child.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, within } from "@testing-library/react";
import type { SubIssueItem } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { defaultDisplayProperties, type SubIssueDisplayProperties } from "./rules.js";
import { SubIssuePropertyControls } from "./SubIssueProperties.js";
import type { SubIssueRowActions } from "./types.js";

const WS = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const PARENT = "323e4567-e89b-12d3-a456-426614174002";
const CHILD = "423e4567-e89b-12d3-a456-426614174003";
const STATE = "523e4567-e89b-12d3-a456-426614174005";
const MEMBER = "623e4567-e89b-12d3-a456-426614174006";

function row(overrides: Partial<SubIssueItem> = {}): SubIssueItem {
  return {
    id: CHILD,
    name: "Child one",
    state_id: null,
    sort_order: 1,
    completed_at: null,
    estimate_point: null,
    priority: "medium",
    start_date: null,
    target_date: null,
    sequence_id: 7,
    project_id: PROJECT,
    parent_id: PARENT,
    cycle_id: null,
    module_ids: [],
    label_ids: [],
    assignee_ids: [],
    sub_issues_count: null,
    attachment_count: null,
    link_count: null,
    created_at: "2026-10-01T00:00:00Z",
    updated_at: "2026-10-01T00:00:00Z",
    created_by: null,
    updated_by: null,
    is_draft: false,
    archived_at: null,
    state_group: "started",
    ...overrides,
  };
}

function stubPickers(): void {
  const state = {
    id: STATE,
    project_id: PROJECT,
    workspace_id: PARENT,
    name: "In progress",
    group: "started",
    color: "#d9912b",
    default: false,
    description: "",
    sequence: 1,
  };
  const directory = {
    id: MEMBER,
    email: "ada@example.test",
    username: "ada",
    first_name: "Ada",
    last_name: "L",
    display_name: "Ada L",
    avatar: "",
    avatar_url: null,
    cover_image: null,
    cover_image_url: null,
    date_joined: "2026-01-01T00:00:00Z",
    user_timezone: "UTC",
    is_active: true,
    is_bot: false,
    is_email_verified: true,
    is_password_autoset: true,
    last_login_medium: "",
    last_login_time: null,
  };
  createAppClient("https://api.test", (async (url: string) => {
    const body = url.includes("/states/")
      ? [state]
      : url.endsWith(`/projects/${PROJECT}/members/`)
        ? [
            {
              id: MEMBER,
              role: 15,
              member: MEMBER,
              project: PROJECT,
              original_role: 15,
              created_at: "2026-01-01T00:00:00.000Z",
            },
          ]
        : url.endsWith("/api/users/me/")
          ? directory
          : url.endsWith("/members/")
            ? [{ id: MEMBER, member: directory, role: 15, is_active: true }]
            : {};
    return { status: 200, url, headers: { get: () => null }, text: async () => JSON.stringify(body) };
  }) as never);
}

function renderControls(
  options: {
    row?: SubIssueItem;
    properties?: SubIssueDisplayProperties;
    editable?: boolean;
    merged?: boolean;
    overdue?: boolean;
  } = {}
) {
  createAppClient(
    "https://api.test",
    (async (url: string) =>
      ({
        status: 200,
        url,
        headers: { get: () => null },
        text: async () => "{}",
      }) as never) as never
  );
  const actions: SubIssueRowActions = { isBusy: () => false, write: vi.fn() };
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const view = render(
    <QueryClientProvider client={client}>
      <SubIssuePropertyControls
        workspaceSlug={WS}
        projectId={PROJECT}
        row={options.row ?? row()}
        properties={options.properties ?? defaultDisplayProperties()}
        editable={options.editable ?? true}
        merged={options.merged ?? false}
        overdue={options.overdue ?? false}
        actions={actions}
      />
    </QueryClientProvider>
  );
  return { actions, ...view };
}

describe("SubIssuePropertyControls", () => {
  it("offers the row's priority as its own trigger", () => {
    renderControls();
    expect(screen.getByRole("button", { name: "Priority: medium" })).toBeInTheDocument();
  });

  it("writes only due_date from the due column", async () => {
    const { actions } = renderControls({ row: row({ target_date: "2026-10-20" }) });
    fireEvent.click(screen.getByTestId("sub-issue-due-date-clear"));
    expect(actions.write).toHaveBeenCalledWith(
      expect.objectContaining({ patch: { target_date: null }, field: "target_date" })
    );
  });

  it("writes both ends in one patch from the merged range", async () => {
    const { actions } = renderControls({
      row: row({ start_date: "2026-10-01", target_date: "2026-10-10" }),
      merged: true,
    });
    fireEvent.click(screen.getByTestId("sub-issue-date-range-clear"));
    expect(actions.write).toHaveBeenCalledTimes(1);
    expect(actions.write).toHaveBeenCalledWith({
      row: expect.objectContaining({ id: CHILD }),
      patch: { start_date: null, target_date: null },
      field: "start_date",
    });
  });

  it("stops a control's click from reaching the row link", () => {
    const onRowClick = vi.fn();
    createAppClient(
      "https://api.test",
      (async (url: string) =>
        ({ status: 200, url, headers: { get: () => null }, text: async () => "{}" }) as never) as never
    );
    const actions: SubIssueRowActions = { isBusy: () => false, write: vi.fn() };
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={client}>
        <div onClick={onRowClick}>
          <SubIssuePropertyControls
            workspaceSlug={WS}
            projectId={PROJECT}
            row={row()}
            properties={defaultDisplayProperties()}
            editable
            merged={false}
            overdue={false}
            actions={actions}
          />
        </div>
      </QueryClientProvider>
    );
    const trigger = screen.getByRole("button", { name: "Priority: medium" });

    fireEvent.click(trigger);

    expect(onRowClick).not.toHaveBeenCalled();
  });

  it("hides every column the display turned off", () => {
    renderControls({
      properties: {
        ...defaultDisplayProperties(),
        state: false,
        priority: false,
        start_date: false,
        due_date: false,
        assignee: false,
      },
    });
    expect(screen.queryByRole("button", { name: /Priority/ })).not.toBeInTheDocument();
    expect(screen.queryByTestId("sub-issue-start-date")).not.toBeInTheDocument();
    expect(screen.queryByTestId("sub-issue-due-date")).not.toBeInTheDocument();
  });

  it("draws the two date columns separately when not merged", () => {
    renderControls({ row: row({ start_date: "2026-10-01", target_date: "2026-10-10" }) });
    expect(screen.getByTestId("sub-issue-start-date")).toBeInTheDocument();
    expect(screen.getByTestId("sub-issue-due-date")).toBeInTheDocument();
  });

  it("disables every trigger for a guest", () => {
    renderControls({
      editable: false,
      row: row({ start_date: "2026-10-01", target_date: "2026-10-10" }),
    });
    expect(screen.getByTestId("sub-issue-start-date-trigger")).toBeDisabled();
    expect(screen.getByTestId("sub-issue-due-date-trigger")).toBeDisabled();
  });

  it("draws the grading tone on an overdue due column", () => {
    const overdue = renderControls({ row: row({ target_date: "2026-10-10" }), overdue: true });
    expect(screen.getByTestId("sub-issue-due-date").parentElement?.className).toContain("text-(--danger)");
    overdue.unmount();
    renderControls({ row: row({ target_date: "2026-10-10" }), overdue: false });
    expect(screen.getByTestId("sub-issue-due-date").parentElement?.className ?? "").not.toContain("text-(--danger)");
  });
});

afterEach(() => {
  resetAppClient();
});

describe("SubIssuePropertyControls writers", () => {
  it("draws the urgent ring on an urgent row", () => {
    renderControls({ row: row({ priority: "urgent" }) });
    const trigger = screen.getByRole("button", { name: "Priority: urgent" });
    expect(trigger.querySelector("span[class*='rounded-full']")).not.toBeNull();
  });

  it("writes only state_id from the state column", async () => {
    const { actions } = renderControls();
    stubPickers();
    fireEvent.click(screen.getByRole("button", { name: "State" }));
    const listbox = await screen.findByRole("listbox", { name: "States" }, { timeout: 10_000 });
    fireEvent.click(within(listbox).getByRole("option", { name: /In progress/ }));
    expect(actions.write).toHaveBeenCalledTimes(1);
    expect(actions.write).toHaveBeenCalledWith(
      expect.objectContaining({ patch: { state_id: expect.any(String) }, field: "state_id" })
    );
  });

  it("writes only priority from the priority column", async () => {
    const { actions } = renderControls();
    fireEvent.click(screen.getByRole("button", { name: "Priority: medium" }));
    const listbox = await screen.findByRole("listbox", { name: "Priorities" });
    fireEvent.click(within(listbox).getByRole("option", { name: "High" }));
    expect(actions.write).toHaveBeenCalledTimes(1);
    expect(actions.write).toHaveBeenCalledWith(
      expect.objectContaining({ patch: { priority: "high" }, field: "priority" })
    );
  });

  it("writes only assignee_ids from the assignee column", async () => {
    const { actions } = renderControls();
    stubPickers();
    fireEvent.click(screen.getByRole("button", { name: "Select members" }));
    const listbox = await screen.findByRole("listbox", { name: "Members" }, { timeout: 10_000 });
    fireEvent.click(within(listbox).getByRole("option", { name: "Ada L" }));
    expect(actions.write).toHaveBeenCalledTimes(1);
    expect(actions.write).toHaveBeenCalledWith(
      expect.objectContaining({ patch: { assignee_ids: [expect.any(String)] }, field: "assignee_ids" })
    );
  });

  it("writes only start_date from the start column", async () => {
    const { actions } = renderControls({ row: row({ start_date: "2026-10-01" }) });
    fireEvent.click(screen.getByTestId("sub-issue-start-date-clear"));
    expect(actions.write).toHaveBeenCalledWith(
      expect.objectContaining({ patch: { start_date: null }, field: "start_date" })
    );
  });
});
