// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// MOD-033 surface: the shared sidebar renders the module identity, dates,
// lead, members and counts; each field writes a partial PATCH; guests,
// archived rows and the peek's read-only mount disable every control; the
// points row appears only under a points estimate system; the loader and
// failure states are honest; and slice L's progress node composes below.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import type { ArchivedModuleDetail, ModuleDetail } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../../../../shared/shell/toaster.js", () => ({ notifyError: vi.fn(), notifySuccess: vi.fn() }));
vi.mock("../../../../shared/pickers/index.js", async (importOriginal) => {
  const original = await importOriginal<Record<string, unknown>>();
  return {
    ...original,
    MemberPicker: (props: {
      value: string | readonly string[];
      multiple?: boolean;
      disabled?: boolean;
      placeholder?: string;
      onChange: (value: never) => void;
    }) => (
      <button
        type="button"
        disabled={props.disabled}
        data-testid={props.multiple === true ? "members-picker" : "lead-picker"}
        onClick={() => props.onChange((props.multiple === true ? ["member-2"] : "member-2") as never)}
      >
        {props.multiple === true ? "Members" : props.value === null ? "Lead" : String(props.value)}
      </button>
    ),
  };
});

import { createAppClient, getAppClient, resetAppClient } from "../../../../core/api/client.js";
import { notifyError, notifySuccess } from "../../../../shared/shell/toaster.js";
import { getModule } from "@pidash/api-client";
import {
  isArchivedRecord,
  MODULE_SIDEBAR_LOAD_FAILED,
  ModuleAnalyticsSidebar,
  ModuleAnalyticsSidebarContent,
} from "./ModuleAnalyticsSidebar.js";

const SLUG = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const MODULE = "323e4567-e89b-12d3-a456-426614174002";
const DETAIL_PATH = `/api/workspaces/${SLUG}/projects/${PROJECT}/modules/${MODULE}/`;
const ARCHIVED_PATH = `/api/workspaces/${SLUG}/projects/${PROJECT}/archived-modules/${MODULE}/`;
const ESTIMATES_PATH = `/api/workspaces/${SLUG}/projects/${PROJECT}/estimates/`;
const ROLES_PATH = `/api/users/me/workspaces/${SLUG}/project-roles/`;
const LINKS_PATH = `/api/workspaces/${SLUG}/projects/${PROJECT}/modules/${MODULE}/module-links/`;

function detailFixture(overrides: Partial<ModuleDetail> = {}): ModuleDetail {
  return {
    id: MODULE,
    workspace_id: "123e4567-e89b-12d3-a456-426614174000",
    project_id: PROJECT,
    name: "Delivery",
    description: "",
    start_date: "2026-09-01",
    target_date: "2026-09-30",
    status: "in-progress",
    lead_id: null,
    member_ids: [],
    sort_order: 0,
    is_favorite: false,
    total_issues: 2,
    completed_issues: 1,
    cancelled_issues: 0,
    started_issues: 0,
    unstarted_issues: 0,
    backlog_issues: 0,
    total_estimate_points: 2,
    completed_estimate_points: 0,
    created_at: "2026-01-02T03:04:05Z",
    updated_at: "2026-02-03T04:05:06Z",
    link_module: [],
    sub_issues: null,
    distribution: { assignees: [], labels: [], completion_chart: {} },
    estimate_distribution: { assignees: [], labels: [], completion_chart: {} },
    ...overrides,
  };
}

function archivedFixture(overrides: Partial<ArchivedModuleDetail> = {}): ArchivedModuleDetail {
  return {
    id: MODULE,
    workspace_id: "123e4567-e89b-12d3-a456-426614174000",
    project_id: PROJECT,
    name: "Archived delivery",
    description: "",
    description_text: null,
    description_html: null,
    start_date: "2026-09-01",
    target_date: "2026-09-30",
    status: "completed",
    lead_id: null,
    member_ids: [],
    total_issues: 4,
    completed_issues: 3,
    cancelled_issues: 0,
    started_issues: 0,
    unstarted_issues: 0,
    backlog_issues: 0,
    total_estimate_points: 0,
    completed_estimate_points: 0,
    backlog_estimate_points: 0,
    unstarted_estimate_points: 0,
    started_estimate_points: 0,
    cancelled_estimate_points: 0,
    is_favorite: false,
    created_at: "2026-01-02T03:04:05Z",
    updated_at: "2026-02-03T04:05:06Z",
    archived_at: "2026-03-01T00:00:00Z",
    link_module: [],
    sub_issues: null,
    estimate_distribution: { assignees: [], labels: [], completion_chart: {} },
    distribution: { assignees: [], labels: [], completion_chart: {} },
    ...overrides,
  };
}

function estimate(type: string, lastUsed = true) {
  return {
    id: "423e4567-e89b-12d3-a456-426614174003",
    workspace: "123e4567-e89b-12d3-a456-426614174000",
    project: PROJECT,
    name: "Estimate",
    description: "",
    type,
    last_used: lastUsed,
    points: [],
    created_at: "2026-01-02T03:04:05Z",
    updated_at: "2026-02-03T04:05:06Z",
    created_by: null,
    updated_by: null,
  };
}

interface Harness {
  detail: ModuleDetail | ArchivedModuleDetail;
  detailStatus: number;
  detailDelay: number;
  estimates: ReturnType<typeof estimate>[];
  estimatesStatus: number;
  patchStatus: number;
  patches: Array<Record<string, unknown>>;
  detailCalls: number;
  roles: Record<string, number>;
  linkPosts: Array<Record<string, unknown>>;
}

let harness: Harness;
const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

beforeEach(() => {
  harness = {
    detail: detailFixture(),
    detailStatus: 200,
    detailDelay: 0,
    estimates: [],
    estimatesStatus: 200,
    patchStatus: 200,
    patches: [],
    detailCalls: 0,
    roles: {},
    linkPosts: [],
  };
  createAppClient("https://api.test", (async (url: string, init?: RequestInit) => {
    if (url.endsWith(DETAIL_PATH) || url.endsWith(ARCHIVED_PATH)) {
      harness.detailCalls += 1;
      if (init?.method === "PATCH") {
        const patch = JSON.parse(String(init.body)) as Record<string, unknown>;
        harness.patches.push(patch);
        if (harness.patchStatus === 200) {
          harness.detail = { ...harness.detail, ...patch } as ModuleDetail | ArchivedModuleDetail;
        }
        return {
          status: harness.patchStatus,
          url,
          headers: { get: () => null },
          text: async () => JSON.stringify(harness.patchStatus === 200 ? harness.detail : { error: "Refused." }),
        } as never;
      }
      if (harness.detailDelay > 0) await sleep(harness.detailDelay);
      return {
        status: harness.detailStatus,
        url,
        headers: { get: () => null },
        text: async () => JSON.stringify(harness.detailStatus === 200 ? harness.detail : { detail: "boom" }),
      } as never;
    }
    if (url.endsWith(ESTIMATES_PATH)) {
      return {
        status: harness.estimatesStatus,
        url,
        headers: { get: () => null },
        text: async () => JSON.stringify(harness.estimatesStatus === 200 ? harness.estimates : { detail: "boom" }),
      } as never;
    }
    if (url.endsWith(ROLES_PATH)) {
      return {
        status: 200,
        url,
        headers: { get: () => null },
        text: async () => JSON.stringify(harness.roles),
      } as never;
    }
    if (url.endsWith(LINKS_PATH) && init?.method === "POST") {
      const body = JSON.parse(String(init.body)) as Record<string, unknown>;
      harness.linkPosts.push(body);
      return {
        status: 201,
        url,
        headers: { get: () => null },
        text: async () =>
          JSON.stringify({
            id: "623e4567-e89b-12d3-a456-426614174005",
            title: body["title"] ?? null,
            url: body["url"],
            created_at: "2026-10-20T10:00:00Z",
            updated_at: "2026-10-20T10:00:00Z",
            created_by: null,
            updated_by: null,
          }),
      } as never;
    }
    throw new Error(`unexpected request: ${url}`);
  }) as never);
});

afterEach(() => {
  resetAppClient();
  vi.clearAllMocks();
});

function renderContent(overrides: Partial<React.ComponentProps<typeof ModuleAnalyticsSidebarContent>> = {}) {
  const client = getAppClient();
  return render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <ModuleAnalyticsSidebarContent
        workspaceSlug={SLUG}
        projectId={PROJECT}
        record={detailFixture()}
        readOnly={false}
        archived={false}
        editor
        showPoints={false}
        progress={null}
        client={client}
        {...overrides}
      />
    </QueryClientProvider>
  );
}

function renderSidebar(
  overrides: Partial<React.ComponentProps<typeof ModuleAnalyticsSidebar>> = {},
  omitCanEdit = false
) {
  return render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <ModuleAnalyticsSidebar
        workspaceSlug={SLUG}
        projectId={PROJECT}
        moduleId={MODULE}
        {...(omitCanEdit ? {} : { canEdit: true })}
        {...overrides}
      />
    </QueryClientProvider>
  );
}

describe("ModuleAnalyticsSidebarContent", () => {
  it("renders the identity, dates, counts and the progress slot", () => {
    renderContent({ progress: <p>Progress goes here</p> });
    const sidebar = screen.getByTestId("modules-analytics-sidebar");
    expect(within(sidebar).getByRole("button", { name: "In Progress" })).toBeInTheDocument();
    expect(within(sidebar).getByLabelText("Module name")).toHaveValue("Delivery");
    expect(within(sidebar).getByLabelText("Module description")).toHaveValue("");
    expect(within(sidebar).getByTestId("modules-sidebar-dates-trigger")).toHaveTextContent("Sep 01, 2026 Sep 30, 2026");
    expect(within(sidebar).getByText("Work items")).toBeInTheDocument();
    expect(within(sidebar).getByText("1/2")).toBeInTheDocument();
    expect(within(sidebar).queryByText("Points")).not.toBeInTheDocument();
    expect(within(sidebar).getByText("Progress goes here")).toBeInTheDocument();
  });

  it("shows the points row under a points estimate system", () => {
    renderContent({
      showPoints: true,
      record: detailFixture({ completed_estimate_points: 1, total_estimate_points: 3 }),
    });
    expect(screen.getByText("Points")).toBeInTheDocument();
    expect(screen.getByText("1/3")).toBeInTheDocument();
  });

  it("offers the close control only to hosts that pass one", () => {
    const onClose = vi.fn();
    const { rerender } = render(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <ModuleAnalyticsSidebarContent
          workspaceSlug={SLUG}
          projectId={PROJECT}
          record={detailFixture()}
          readOnly={false}
          archived={false}
          editor
          showPoints={false}
          progress={null}
          onClose={onClose}
          client={getAppClient()}
        />
      </QueryClientProvider>
    );
    const close = screen.getByTestId("modules-sidebar-close");
    expect(close).toHaveAccessibleName("Close sidebar");
    fireEvent.click(close);
    expect(onClose).toHaveBeenCalledTimes(1);
    rerender(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <ModuleAnalyticsSidebarContent
          workspaceSlug={SLUG}
          projectId={PROJECT}
          record={detailFixture()}
          readOnly={false}
          archived={false}
          editor
          showPoints={false}
          progress={null}
          client={getAppClient()}
        />
      </QueryClientProvider>
    );
    expect(screen.queryByTestId("modules-sidebar-close")).not.toBeInTheDocument();
  });

  it("keeps the pinned placeholders for a missing side of the range", () => {
    renderContent({ record: detailFixture({ start_date: null }) });
    const trigger = screen.getByTestId("modules-sidebar-dates-trigger");
    expect(trigger).toHaveTextContent("Start date");
    expect(trigger).toHaveTextContent("Sep 30, 2026");
  });

  it("saves the status through a partial PATCH and confirms", async () => {
    renderContent();
    fireEvent.click(screen.getByRole("button", { name: "In Progress" }));
    fireEvent.click(await screen.findByRole("option", { name: "Completed" }));
    await waitFor(() => expect(harness.patches).toEqual([{ status: "completed" }]));
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith("Module updated successfully."));
  });

  it("does not send a status write when the same status is picked", async () => {
    renderContent();
    fireEvent.click(screen.getByRole("button", { name: "In Progress" }));
    fireEvent.click(await screen.findByRole("option", { name: "In Progress" }));
    expect(harness.patches).toEqual([]);
  });

  it("saves name and description as separate partial patches", async () => {
    renderContent();
    const name = screen.getByLabelText("Module name");
    fireEvent.change(name, { target: { value: "Launch" } });
    fireEvent.blur(name);
    await waitFor(() => expect(harness.patches).toContainEqual({ name: "Launch" }));

    const description = screen.getByLabelText("Module description");
    fireEvent.change(description, { target: { value: "Shipped" } });
    fireEvent.blur(description);
    await waitFor(() => expect(harness.patches).toContainEqual({ description: "Shipped" }));
  });

  it("saves a date range as one partial patch with both sides", async () => {
    renderContent();
    const trigger = screen.getByTestId("modules-sidebar-dates-trigger");
    fireEvent.click(trigger);
    const calendar = await screen.findByTestId("modules-sidebar-dates-calendar");
    fireEvent.click(within(calendar).getByRole("button", { name: "Next month" }));
    fireEvent.click(await screen.findByRole("button", { name: "October 15, 2026" }));
    fireEvent.click(await screen.findByRole("button", { name: "October 18, 2026" }));
    await waitFor(() =>
      expect(harness.patches).toContainEqual({ start_date: "2026-09-01", target_date: "2026-10-18" })
    );
    expect(harness.patches[0]).toEqual({ start_date: "2026-09-01", target_date: "2026-10-15" });
  });

  it("saves the lead and members independently", async () => {
    renderContent();
    fireEvent.click(screen.getByTestId("lead-picker"));
    await waitFor(() => expect(harness.patches).toContainEqual({ lead_id: "member-2" }));
    fireEvent.click(screen.getByTestId("members-picker"));
    await waitFor(() => expect(harness.patches).toContainEqual({ member_ids: ["member-2"] }));
  });

  it("rolls the field back and reports the server refusal", async () => {
    harness.patchStatus = 400;
    renderContent();
    const name = screen.getByLabelText("Module name");
    fireEvent.focus(name);
    fireEvent.change(name, { target: { value: "Launch" } });
    fireEvent.blur(name);
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith("Couldn't update the module", "Refused."));
    await waitFor(() => expect(screen.getByLabelText("Module name")).toHaveValue("Delivery"));
  });

  it("disables every control when read-only", () => {
    renderContent({ readOnly: true });
    expect(screen.getByRole("button", { name: "In Progress" })).toBeDisabled();
    expect(screen.getByLabelText("Module name")).toBeDisabled();
    expect(screen.getByLabelText("Module description")).toBeDisabled();
    expect(screen.getByTestId("modules-sidebar-dates-trigger")).toBeDisabled();
    expect(screen.getByTestId("lead-picker")).toBeDisabled();
    expect(screen.getByTestId("members-picker")).toBeDisabled();
  });

  it("disables every control on an archived module even for an editor", () => {
    renderContent({ archived: true });
    expect(screen.getByRole("button", { name: "In Progress" })).toBeDisabled();
    expect(screen.getByLabelText("Module name")).toBeDisabled();
    expect(screen.getByTestId("modules-sidebar-dates-trigger")).toBeDisabled();
  });

  it("disables every control for a non-editor", () => {
    renderContent({ editor: false });
    expect(screen.getByRole("button", { name: "In Progress" })).toBeDisabled();
    expect(screen.getByLabelText("Module name")).toBeDisabled();
    expect(screen.getByTestId("lead-picker")).toBeDisabled();
  });

  it("closes the tree with the links section below the progress slot", () => {
    renderContent({
      progress: <p data-testid="progress-node">Progress goes here</p>,
      record: detailFixture({
        link_module: [
          {
            id: "523e4567-e89b-12d3-a456-426614174004",
            title: "Launch plan",
            url: "https://docs.example.com/launch",
            created_at: "2026-10-01T10:00:00Z",
            updated_at: "2026-10-01T10:00:00Z",
            created_by: null,
            updated_by: null,
          },
        ],
      }),
    });
    const progress = screen.getByTestId("progress-node");
    const links = screen.getByTestId("modules-links-section");
    expect(progress.compareDocumentPosition(links) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(within(links).getByRole("link", { name: "Launch plan" })).toHaveAttribute(
      "href",
      "https://docs.example.com/launch"
    );
  });

  it("keeps the links rows without actions on the read-only peek mount", () => {
    renderContent({
      readOnly: true,
      record: detailFixture({
        link_module: [
          {
            id: "523e4567-e89b-12d3-a456-426614174004",
            title: null,
            url: "https://docs.example.com/launch",
            created_at: "2026-10-01T10:00:00Z",
            updated_at: "2026-10-01T10:00:00Z",
            created_by: null,
            updated_by: null,
          },
        ],
      }),
    });
    const links = screen.getByTestId("modules-links-section");
    expect(within(links).getByRole("link", { name: "https://docs.example.com/launch" })).toBeInTheDocument();
    expect(within(links).getByRole("button", { name: "Copy link" })).toBeInTheDocument();
    expect(within(links).queryByRole("button", { name: "Add link" })).not.toBeInTheDocument();
    expect(within(links).queryByRole("button", { name: "Edit link" })).not.toBeInTheDocument();
    expect(within(links).queryByRole("button", { name: "Delete link" })).not.toBeInTheDocument();
  });

  it("shows archived links read-only without meta", () => {
    renderContent({
      readOnly: true,
      archived: true,
      record: archivedFixture({
        link_module: [
          { id: "523e4567-e89b-12d3-a456-426614174004", title: "Old plan", url: "https://old.example.com" },
        ],
      }),
    });
    const links = screen.getByTestId("modules-links-section");
    expect(within(links).getByRole("link", { name: "Old plan" })).toBeInTheDocument();
    expect(within(links).queryByText(/Added /)).not.toBeInTheDocument();
    expect(within(links).queryByRole("button", { name: "Add link" })).not.toBeInTheDocument();
  });
});

describe("ModuleAnalyticsSidebar", () => {
  it("loads the detail, renders the counts, and fetches points gating", async () => {
    harness.estimates = [estimate("points")];
    renderSidebar();
    expect(screen.getByTestId("modules-analytics-sidebar-skeleton")).toBeInTheDocument();
    expect(await screen.findByTestId("modules-analytics-sidebar")).toBeInTheDocument();
    expect(screen.getByLabelText("Module name")).toHaveValue("Delivery");
    expect(screen.getByText("Points")).toBeInTheDocument();
  });

  it("shows a loader on a slow fetch", async () => {
    harness.detailDelay = 50;
    renderSidebar();
    expect(screen.getByRole("status", { name: "Loading module" })).toBeInTheDocument();
    expect(await screen.findByTestId("modules-analytics-sidebar")).toBeInTheDocument();
    expect(screen.queryByRole("status", { name: "Loading module" })).not.toBeInTheDocument();
  });

  it("offers a retry when the detail read fails", async () => {
    harness.detailStatus = 500;
    renderSidebar();
    expect(await screen.findByRole("alert")).toHaveTextContent(MODULE_SIDEBAR_LOAD_FAILED);
    harness.detailStatus = 200;
    fireEvent.click(screen.getByRole("button", { name: "Try again" }));
    expect(await screen.findByTestId("modules-analytics-sidebar")).toBeInTheDocument();
  });

  it("uses the archived read and disables the fields for the archived peek", async () => {
    harness.detail = archivedFixture();
    renderSidebar({ archived: true, readOnly: true });
    expect(await screen.findByTestId("modules-analytics-sidebar")).toBeInTheDocument();
    expect(screen.getByLabelText("Module name")).toHaveValue("Archived delivery");
    expect(screen.getByRole("button", { name: "Completed" })).toBeDisabled();
    expect(screen.getByTestId("modules-sidebar-dates-trigger")).toBeDisabled();
  });

  it("does not offer a points row for a time estimate system", async () => {
    harness.estimates = [estimate("time")];
    renderSidebar();
    expect(await screen.findByTestId("modules-analytics-sidebar")).toBeInTheDocument();
    expect(screen.queryByText("Points")).not.toBeInTheDocument();
  });

  it("paints an inline rename across the mounted sidebar and confirms", async () => {
    renderSidebar();
    const name = await screen.findByLabelText("Module name");
    fireEvent.focus(name);
    fireEvent.change(name, { target: { value: "Launch" } });
    fireEvent.blur(name);
    await waitFor(() => expect(harness.patches).toContainEqual({ name: "Launch" }));
    expect(await screen.findByLabelText("Module name")).toHaveValue("Launch");
    expect(notifySuccess).toHaveBeenCalledWith("Module updated successfully.");
  });

  it("composes the progress section below the field rows", async () => {
    renderSidebar({ progress: <p>Progress goes here</p> });
    expect(await screen.findByTestId("modules-analytics-sidebar")).toBeInTheDocument();
    expect(screen.getByText("Progress goes here")).toBeInTheDocument();
  });

  it("creates a link through the section and shows the stored row", async () => {
    renderSidebar();
    const sidebar = await screen.findByTestId("modules-analytics-sidebar");
    const links = within(sidebar).getByTestId("modules-links-section");
    fireEvent.click(within(links).getByRole("button", { name: "Links" }));
    fireEvent.click(within(links).getByRole("button", { name: "Add link" }));
    const dialog = screen.getByRole("dialog", { name: "Add link" });
    fireEvent.change(within(dialog).getByLabelText("URL"), { target: { value: "example.com" } });
    fireEvent.change(within(dialog).getByLabelText("Display title"), { target: { value: "Docs" } });
    fireEvent.click(within(dialog).getByTestId("modules-link-submit"));
    await waitFor(() => expect(harness.linkPosts).toEqual([{ url: "http://example.com", title: "Docs" }]));
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith("Module link created successfully."));
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(await within(links).findByRole("link", { name: "Docs" })).toHaveAttribute("href", "http://example.com");
  });

  it("derives read-only standing from the project role map when no override is passed", async () => {
    renderSidebar({}, true);
    expect(await screen.findByTestId("modules-analytics-sidebar")).toBeInTheDocument();
    expect(screen.getByLabelText("Module name")).toBeDisabled();
  });

  it("lets a project member edit when the role map grants membership", async () => {
    harness.roles = { [PROJECT]: 20 };
    renderSidebar({}, true);
    expect(await screen.findByTestId("modules-analytics-sidebar")).toBeInTheDocument();
    await waitFor(() => expect(screen.getByLabelText("Module name")).not.toBeDisabled());
  });

  it("recognises an archived read by its own record, flag or not", () => {
    expect(isArchivedRecord(archivedFixture())).toBe(true);
    expect(isArchivedRecord(detailFixture())).toBe(false);
  });
});

describe("module detail contract", () => {
  it("uses the parsed detail helper from the api-client", async () => {
    const client = createAppClient("https://api.test", (async (url: string) => ({
      status: 200,
      url,
      headers: { get: () => null },
      text: async () => JSON.stringify(detailFixture()),
    })) as never);
    await expect(getModule(client, SLUG, PROJECT, MODULE)).resolves.toMatchObject({ id: MODULE, name: "Delivery" });
  });
});
