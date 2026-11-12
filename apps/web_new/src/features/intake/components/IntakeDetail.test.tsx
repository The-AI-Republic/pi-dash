// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Request detail pane (INT-019/024/026–029): the composed surfaces,
// title save and version restore through the intake update, the
// permission wiring, arrow navigation with its guard, and the
// missing/error states.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type {
  IntakeIssueDetail,
  IntakeIssueDetailItem,
  IssueActivity,
  IssueComment,
  TransportInit,
} from "@pidash/api-client";
import { ToastHost } from "@pidash/kit";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const navigate = vi.fn();
vi.mock("@tanstack/react-router", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  Link: ({ to, children, ...rest }: { to: string; children?: React.ReactNode }) => (
    <a href={to} {...rest}>
      {children}
    </a>
  ),
  useNavigate: () => navigate,
}));

const editionHarness = vi.hoisted(() => ({ flags: {} as Record<string, boolean> }));
vi.mock("../../../core/edition/index.js", async (importOriginal) => {
  const original = await importOriginal<Record<string, unknown>>();
  return {
    ...original,
    get edition() {
      return { id: "test", flags: editionHarness.flags };
    },
  };
});

vi.mock("../../../shared/editor/RichTextEditor.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../shared/editor/RichTextEditor.js")>();
  function FakeEditor({
    initialHtml,
    onChange,
    label,
  }: {
    initialHtml: string;
    onChange: (html: string) => void;
    label: string;
  }) {
    return (
      <textarea
        aria-label={label}
        defaultValue={initialHtml.replace(/<[^>]*>/g, "").trim()}
        onChange={(event) => onChange(`<p>${event.target.value}</p>`)}
      />
    );
  }
  return { ...original, RichTextEditor: FakeEditor, CommentRichTextEditor: FakeEditor };
});

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { edition, INTAKE_SOURCE_PILL_FLAG, ISSUE_DEDUPE_FLAG } from "../../../core/edition/index.js";
import { setSessionClient } from "../../../core/session/queries.js";
import { WORKLOG_FLAG } from "../../issues/detail/widgets/worklog/worklog.js";
import { getToastManager } from "../../../shared/shell/toaster.js";
import { IntakeDetail } from "./IntakeDetail.js";

const SLUG = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const ISSUE = "123e4567-e89b-12d3-a456-426614174000";
const ADA = "323e4567-e89b-12d3-a456-426614174002";
const ME = "023e4567-e89b-12d3-a456-426614174009";
const TRIAGE = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const BACKLOG = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
const LABEL = "b23e4567-e89b-12d3-a456-42661417400a";
const V1 = "c23e4567-e89b-12d3-a456-42661417400c";
const COMMENT = "f23e4567-e89b-12d3-a456-42661417400f";
const CREATED = "f23e4567-e89b-12d3-a456-426614174010";
const ACTIVITY = "f23e4567-e89b-12d3-a456-426614174011";

const ME_PAYLOAD = {
  id: ME,
  email: "me@example.com",
  username: "me",
  first_name: "Me",
  last_name: "M",
  display_name: "Me M",
  avatar: "",
  avatar_url: null,
  cover_image: null,
  cover_image_url: null,
  date_joined: "2026-01-01T00:00:00.000Z",
  user_timezone: "UTC",
  is_active: true,
  is_bot: false,
  is_email_verified: true,
  is_password_autoset: false,
  last_login_medium: "password",
  last_login_time: null,
};

function stateRow(id: string, name: string, group: string) {
  return {
    id,
    project_id: PROJECT,
    workspace_id: "b23e4567-e89b-12d3-a456-42661417400e",
    name,
    color: "#5e6ad2",
    group,
    default: false,
    description: "",
    sequence: 1,
  };
}

const DIRECTORY = [
  {
    id: "b23e4567-e89b-12d3-a456-42661417400b",
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
    is_active: true,
  },
];

const PROJECT_MEMBERS = [
  {
    id: "d23e4567-e89b-12d3-a456-42661417400d",
    role: 15,
    member: ADA,
    project: PROJECT,
    original_role: 15,
    created_at: "2024-01-02T03:04:05Z",
  },
];

const LABELS = [
  {
    id: LABEL,
    project_id: PROJECT,
    workspace_id: "b23e4567-e89b-12d3-a456-42661417400e",
    name: "Bug",
    color: "#ff0000",
    parent: null,
    sort_order: 1,
  },
];

const ATTACHMENT = {
  id: "e23e4567-e89b-12d3-a456-42661417400e",
  attributes: { name: "spec.png" },
  asset: "https://cdn.test/spec.png",
  asset_url: null,
  user: null,
  workspace: null,
  draft_issue: null,
  project: PROJECT,
  issue: ISSUE,
  comment: null,
  page: null,
  entity_type: "issue",
  entity_identifier: ISSUE,
  is_deleted: false,
  is_archived: false,
  external_id: null,
  external_source: null,
  size: 68,
  is_uploaded: true,
  storage_metadata: null,
  created_at: "2026-10-01T00:00:00Z",
  updated_at: "2026-10-01T00:00:00Z",
  created_by: null,
  updated_by: null,
  deleted_at: null,
};

function detailItem(overrides: Partial<IntakeIssueDetailItem> = {}): IntakeIssueDetailItem {
  return {
    id: ISSUE,
    name: "A request",
    state_id: TRIAGE,
    sort_order: 1,
    completed_at: null,
    priority: "none",
    sequence_id: 7,
    project_id: PROJECT,
    parent_id: null,
    label_ids: [],
    assignee_ids: [],
    created_at: "2026-10-01T00:00:00Z",
    updated_at: "2026-10-02T00:00:00Z",
    created_by: null,
    updated_by: null,
    is_draft: false,
    archived_at: null,
    complexity_score: 0,
    agent_executor: null,
    is_synced: false,
    description_html: "<p>Current words</p>",
    agent_ticker: null,
    agent_status: null,
    relations_summary: { blocked_by: [], blocking: [] },
    has_open_blockers: false,
    ...overrides,
  };
}

function detailPayload(status = -2, overrides: Partial<IntakeIssueDetailItem> = {}): IntakeIssueDetail {
  return {
    id: "aaaaaaaa-aaaa-4aaa-8aaa-000000000001",
    status,
    duplicate_to: null,
    snoozed_till: null,
    duplicate_issue_detail: null,
    source: null,
    issue: detailItem(overrides),
  };
}

interface DetailServer {
  detailStatus: number;
  detail: IntakeIssueDetail;
  writes: Array<{ url: string; body: unknown }>;
  postedComments?: IssueComment[];
  activities?: IssueActivity[];
}

function versionsEnvelope(hourAgo: string) {
  return {
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
  };
}

function versionDetail(hourAgo: string) {
  return {
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
  };
}

function propertyRow(): IssueActivity {
  return {
    id: ACTIVITY,
    issue: ISSUE,
    project: PROJECT,
    workspace: "b23e4567-e89b-12d3-a456-42661417400e",
    actor: ADA,
    actor_detail: DIRECTORY[0]!.member,
    verb: "updated",
    field: "state",
    old_value: "Triage",
    new_value: "Backlog",
    comment: "",
    attachments: [],
    issue_comment: null,
    old_identifier: null,
    new_identifier: null,
    epoch: null,
    source_data: null,
    created_at: "2026-10-01T00:00:01Z",
    updated_at: "2026-10-01T00:00:01Z",
    created_by: ADA,
    updated_by: null,
    deleted_at: null,
    issue_detail: null,
    project_detail: null,
    workspace_detail: null,
  };
}

function commentRow(): IssueComment {
  return {
    id: COMMENT,
    issue: ISSUE,
    project: PROJECT,
    workspace: "b23e4567-e89b-12d3-a456-42661417400e",
    actor: ADA,
    actor_detail: DIRECTORY[0]!.member,
    comment_html: "<p>Intake thread words</p>",
    comment_stripped: "Intake thread words",
    comment_json: {},
    attachments: [],
    labels: [],
    access: "INTERNAL",
    edited_at: null,
    comment_reactions: [],
    speaker_type: "human",
    speaker_label: "",
    speaker_agent_run_id: null,
    parent: null,
    external_source: null,
    external_id: null,
    is_synced: false,
    description: null,
    created_at: "2026-10-01T00:00:00Z",
    updated_at: "2026-10-01T00:00:00Z",
    created_by: ADA,
    updated_by: null,
    deleted_at: null,
  };
}

function harness(server: DetailServer, options: { meStatus?: number; searchHits?: unknown[] } = {}) {
  const hourAgo = new Date(Date.now() - 3_600_000).toISOString();
  const searches: string[] = [];
  const conversation = async (url: string, init: TransportInit) => {
    const ok = (body: unknown) => ({
      status: 200,
      url,
      headers: { get: () => null },
      text: async () => JSON.stringify(body),
    });
    if (url.endsWith("/auth/get-csrf-token/")) return ok({ csrf_token: "cs" });
    if (url.includes(`/projects/${PROJECT}/search-issues/`)) {
      searches.push(url);
      return ok(options.searchHits ?? []);
    }
    if (url.includes(`/intake-issues/${ISSUE}/`) && init.method === "GET") {
      if (server.detailStatus >= 400) {
        return { status: server.detailStatus, url, headers: { get: () => null }, text: async () => "{}" };
      }
      return ok(server.detail);
    }
    if (url.includes(`/intake-issues/${ISSUE}/`) && init.method === "PATCH") {
      const body = JSON.parse(init.body ?? "{}") as { issue?: Record<string, unknown> };
      server.writes.push({ url, body });
      const echo = detailPayload(server.detail.status, {
        ...server.detail.issue,
        ...((body.issue ?? {}) as Partial<IntakeIssueDetailItem>),
      });
      return ok(echo);
    }
    if (url.includes(`/description-versions/${V1}/`)) return ok(versionDetail(hourAgo));
    if (url.includes("description-versions")) return ok(versionsEnvelope(hourAgo));
    if (url.endsWith(`/projects/${PROJECT}/intake-state/`)) return ok(stateRow(TRIAGE, "Triage", "triage"));
    if (url.endsWith(`/projects/${PROJECT}/states/`))
      return ok([stateRow(TRIAGE, "Triage", "triage"), stateRow(BACKLOG, "Backlog", "backlog")]);
    if (url.includes(`/projects/${PROJECT}/members/`)) return ok(PROJECT_MEMBERS);
    if (url.endsWith(`/workspaces/${SLUG}/members/`)) return ok(DIRECTORY);
    if (url.endsWith(`/projects/${PROJECT}/issue-labels/`)) return ok(LABELS);
    if (url.includes("/reactions/")) return ok([]);
    if (url.includes("activity_type=issue-property")) return ok(server.activities ?? []);
    if (url.includes("activity_type=issue-comment"))
      return ok([commentRow(), ...(server.postedComments ?? []).map((comment) => ({ ...comment }))]);
    if (url.includes(`/issues/${ISSUE}/comments/`) && init.method === "POST") {
      const body = JSON.parse(init.body ?? "{}") as { comment_html?: string; access?: IssueComment["access"] };
      server.writes.push({ url, body });
      const created = {
        ...commentRow(),
        id: CREATED,
        comment_html: body.comment_html ?? "",
        comment_stripped: (body.comment_html ?? "").replace(/<[^>]*>/g, ""),
        access: body.access ?? "INTERNAL",
      };
      server.postedComments = [...(server.postedComments ?? []), created];
      return ok({ data: created });
    }
    if (url.includes("/attachments/")) return ok([ATTACHMENT]);
    if (url.endsWith("/api/users/me/")) {
      const status = options.meStatus ?? 200;
      if (status >= 400) {
        return { status, url, headers: { get: () => null }, text: async () => "{}" };
      }
      return ok(ME_PAYLOAD);
    }
    return { status: 404, url, headers: { get: () => null }, text: async () => "{}" };
  };
  const client = createAppClient("https://api.test", conversation as never);
  setSessionClient(client);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  if (options.meStatus === undefined) queryClient.setQueryData(["session", "me"], ME_PAYLOAD);
  queryClient.setQueryData(["session", "profile"], { start_of_the_week: 0 });
  return { client, queryClient, searches };
}

function renderDetail(queryClient: QueryClient, props: Partial<React.ComponentProps<typeof IntakeDetail>> = {}) {
  const onPrev = vi.fn();
  const onNext = vi.fn();
  const onMissing = vi.fn();
  const onAdvance = vi.fn(() => false);
  const onStay = vi.fn();
  render(
    <QueryClientProvider client={queryClient}>
      <IntakeDetail
        workspaceSlug={SLUG}
        projectId={PROJECT}
        issueId={ISSUE}
        projectIdentifier="APO"
        projectName="Atlas"
        published={false}
        role={20}
        onPrev={onPrev}
        onNext={onNext}
        onMissing={onMissing}
        onAdvance={onAdvance}
        onStay={onStay}
        {...props}
      />
    </QueryClientProvider>
  );
  return { onPrev, onNext, onMissing, onAdvance };
}

afterEach(() => {
  editionHarness.flags = {};
  resetAppClient();
  vi.clearAllMocks();
});

beforeEach(() => {
  createAppClient("https://api.test");
});

describe("IntakeDetail content (INT-019/024)", () => {
  it("composes the header, fields, reactions, attachments and properties", async () => {
    const server: DetailServer = { detailStatus: 200, detail: detailPayload(), writes: [] };
    const { queryClient } = harness(server);
    renderDetail(queryClient);

    expect(await screen.findByTestId("intake-detail-header")).toBeInTheDocument();
    expect(screen.getByText("APO-7")).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "Title" })).toHaveValue("A request");
    expect(screen.getByRole("region", { name: "Description" })).toHaveTextContent("Current words");
    expect(screen.getByRole("region", { name: "Reactions on this work item" })).toBeInTheDocument();
    expect(await screen.findByText("spec.png")).toBeInTheDocument();
    expect(screen.getByTestId("intake-properties")).toBeInTheDocument();
    expect(screen.getByTestId("intake-detail-menu")).toBeInTheDocument();
  });

  it("names the workflow state for an accepted request", async () => {
    const server: DetailServer = {
      detailStatus: 200,
      detail: detailPayload(1, { state_id: BACKLOG }),
      writes: [],
    };
    const { queryClient } = harness(server);
    renderDetail(queryClient);
    await screen.findByTestId("intake-detail-header");
    const state = screen.getByRole("group", { name: "State" });
    expect(await within(state).findByRole("button", { name: "Backlog" })).toBeDisabled();
  });

  it("saves a title edit through the intake update", async () => {
    const server: DetailServer = { detailStatus: 200, detail: detailPayload(), writes: [] };
    const { queryClient } = harness(server);
    renderDetail(queryClient);
    const box = await screen.findByRole("textbox", { name: "Title" });
    fireEvent.change(box, { target: { value: "Renamed" } });
    fireEvent.blur(box);
    await waitFor(() => expect(server.writes).toHaveLength(1));
    expect(server.writes[0]!.body).toEqual({ issue: { name: "Renamed" } });
  });

  it("restores a description version through the intake update", async () => {
    const user = userEvent.setup();
    const server: DetailServer = { detailStatus: 200, detail: detailPayload(), writes: [] };
    const { queryClient } = harness(server);
    renderDetail(queryClient);
    await user.click(await screen.findByRole("button", { name: /Edited by/ }));
    await user.click(await screen.findByRole("menuitem", { name: "Ada L 1 hour ago" }));
    const dialog = await screen.findByRole("dialog", { name: "Description history" });
    await within(dialog).findByText("version one");
    await user.click(within(dialog).getByRole("button", { name: "Restore" }));
    await waitFor(() => expect(server.writes).toHaveLength(1));
    expect(server.writes[0]!.body).toEqual({ issue: { description_html: "<p>version one</p>" } });
    await waitFor(() => expect(screen.getByRole("region", { name: "Description" })).toHaveTextContent("version one"));
  });

  it("mounts the shared activity thread with comment and activity rendering (CMT-018)", async () => {
    const server: DetailServer = {
      detailStatus: 200,
      detail: detailPayload(),
      writes: [],
      activities: [propertyRow()],
    };
    const { queryClient } = harness(server);
    renderDetail(queryClient);

    expect(await screen.findByRole("heading", { name: "Activity" })).toBeInTheDocument();
    expect(await screen.findByText("Intake thread words")).toBeInTheDocument();
    expect(await screen.findByText(/moved the state to Backlog/)).toBeInTheDocument();
    expect(screen.getByRole("group", { name: "Add comment" })).toBeInTheDocument();
  });

  it("offers neither copy-link nor the worklog aid on intake, even in a worklog edition", async () => {
    edition.flags[WORKLOG_FLAG] = true;
    try {
      const server: DetailServer = {
        detailStatus: 200,
        detail: detailPayload(-2, { assignee_ids: [ME] }),
        writes: [],
      };
      const { queryClient } = harness(server);
      renderDetail(queryClient, { role: 20 });

      await screen.findByText("Intake thread words");
      fireEvent.click(await screen.findByRole("button", { name: "Comment actions" }));
      const menu = await screen.findByRole("menu");
      expect(within(menu).queryByRole("menuitem", { name: "Copy link" })).not.toBeInTheDocument();
      expect(screen.queryByRole("button", { name: "Log time" })).not.toBeInTheDocument();
    } finally {
      delete edition.flags[WORKLOG_FLAG];
    }
  });

  it("posts from intake through the ordinary comment endpoint and shows the new row (CMT-018)", async () => {
    const server: DetailServer = { detailStatus: 200, detail: detailPayload(), writes: [] };
    const { queryClient } = harness(server);
    renderDetail(queryClient);

    await screen.findByText("Intake thread words");
    const composer = screen.getByRole("group", { name: "Add comment" });
    fireEvent.change(within(composer).getByLabelText("Add comment"), { target: { value: "Fresh intake words" } });
    fireEvent.keyDown(within(composer).getByLabelText("Add comment"), { key: "Enter" });

    await waitFor(() => expect(server.writes).toHaveLength(1));
    expect(server.writes[0]!.url).toBe(
      `https://api.test/api/workspaces/${SLUG}/projects/${PROJECT}/issues/${ISSUE}/comments/`
    );
    expect(server.writes[0]!.body).toEqual({ comment_html: "<p>Fresh intake words</p>", access: "INTERNAL" });
    expect(await screen.findByText("Fresh intake words")).toBeInTheDocument();
  });

  it("shows the access switch only on a published intake project", async () => {
    const draft: DetailServer = { detailStatus: 200, detail: detailPayload(), writes: [] };
    const draftView = harness(draft);
    renderDetail(draftView.queryClient);
    await screen.findByText("Intake thread words");
    expect(screen.queryByRole("radiogroup", { name: "Comment visibility" })).not.toBeInTheDocument();

    document.body.innerHTML = "";
    resetAppClient();

    const published: DetailServer = { detailStatus: 200, detail: detailPayload(), writes: [] };
    const publishedView = harness(published);
    renderDetail(publishedView.queryClient, { published: true });

    expect(await screen.findByRole("radiogroup", { name: "Comment visibility" })).toBeInTheDocument();
    expect(await screen.findByText("Intake thread words")).toBeInTheDocument();
  });
});

describe("IntakeDetail permissions (INT-026)", () => {
  it("reads another member's request read-only with no versions line", async () => {
    const server: DetailServer = { detailStatus: 200, detail: detailPayload(-2, { created_by: ADA }), writes: [] };
    const { queryClient } = harness(server);
    renderDetail(queryClient, { role: 15 });
    expect(await screen.findByRole("heading", { name: "A request" })).toBeInTheDocument();
    expect(screen.queryByRole("textbox", { name: "Title" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Edited by/ })).not.toBeInTheDocument();
    expect(within(screen.getByRole("group", { name: "Priority" })).getByRole("button")).toBeDisabled();
    expect(screen.queryByTestId("label-picker-trigger")).not.toBeInTheDocument();
  });

  it("lets a guest creator edit the content with versions while properties stay read-only", async () => {
    const server: DetailServer = { detailStatus: 200, detail: detailPayload(-2, { created_by: ME }), writes: [] };
    const { queryClient } = harness(server);
    renderDetail(queryClient, { role: 5 });
    expect(await screen.findByRole("textbox", { name: "Title" })).toBeInTheDocument();
    expect(await screen.findByRole("button", { name: /Edited by/ })).toBeInTheDocument();
    expect(within(screen.getByRole("group", { name: "Priority" })).getByRole("button")).toBeDisabled();
    expect(screen.queryByTestId("intake-detail-menu")).not.toBeInTheDocument();
  });

  it("treats an unknown viewer as a non-creator while the session read fails", async () => {
    const server: DetailServer = { detailStatus: 200, detail: detailPayload(-2, { created_by: ME }), writes: [] };
    const { queryClient } = harness(server, { meStatus: 500 });
    renderDetail(queryClient, { role: 15 });
    expect(await screen.findByRole("heading", { name: "A request" })).toBeInTheDocument();
    expect(screen.queryByRole("textbox", { name: "Title" })).not.toBeInTheDocument();
  });

  it("reads a resolved request read-only with direct copy and open controls", async () => {
    const server: DetailServer = { detailStatus: 200, detail: detailPayload(1, { created_by: ME }), writes: [] };
    const { queryClient } = harness(server);
    renderDetail(queryClient);
    expect(await screen.findByRole("heading", { name: "A request" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Edited by/ })).not.toBeInTheDocument();
    expect(screen.queryByTestId("intake-detail-menu")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Copy work item link" })).toBeInTheDocument();
    expect(screen.getByTestId("intake-detail-open-work-item")).toHaveAttribute("href", "/acme/browse/APO-7/");
  });
});

describe("IntakeDetail navigation (INT-027)", () => {
  it("steps on bare arrows and ignores other keys, repeats and modifiers", async () => {
    const server: DetailServer = { detailStatus: 200, detail: detailPayload(), writes: [] };
    const { queryClient } = harness(server);
    const { onPrev, onNext } = renderDetail(queryClient);
    await screen.findByTestId("intake-detail");

    fireEvent.keyDown(document.body, { key: "ArrowDown" });
    fireEvent.keyDown(document.body, { key: "ArrowUp" });
    expect(onNext).toHaveBeenCalledTimes(1);
    expect(onPrev).toHaveBeenCalledTimes(1);

    fireEvent.keyDown(document.body, { key: "Enter" });
    fireEvent.keyDown(document.body, { key: "ArrowDown", repeat: true });
    fireEvent.keyDown(document.body, { key: "ArrowDown", ctrlKey: true });
    fireEvent.keyDown(document.body, { key: "ArrowDown", metaKey: true });
    fireEvent.keyDown(document.body, { key: "ArrowDown", altKey: true });
    fireEvent.keyDown(document.body, { key: "ArrowDown", shiftKey: true });
    expect(onNext).toHaveBeenCalledTimes(1);
    expect(onPrev).toHaveBeenCalledTimes(1);
  });

  it("stays quiet while typing in the title", async () => {
    const server: DetailServer = { detailStatus: 200, detail: detailPayload(), writes: [] };
    const { queryClient } = harness(server);
    const { onPrev, onNext } = renderDetail(queryClient);
    const box = await screen.findByRole("textbox", { name: "Title" });
    fireEvent.keyDown(box, { key: "ArrowDown" });
    fireEvent.keyDown(box, { key: "ArrowUp" });
    expect(onNext).not.toHaveBeenCalled();
    expect(onPrev).not.toHaveBeenCalled();
  });

  it("stays quiet while a dialog is open", async () => {
    const server: DetailServer = { detailStatus: 200, detail: detailPayload(), writes: [] };
    const { queryClient } = harness(server);
    const { onNext } = renderDetail(queryClient);
    await screen.findByTestId("intake-detail");
    const dialog = document.createElement("div");
    dialog.setAttribute("role", "dialog");
    document.body.appendChild(dialog);
    try {
      fireEvent.keyDown(document.body, { key: "ArrowDown" });
      expect(onNext).not.toHaveBeenCalled();
    } finally {
      dialog.remove();
    }
  });
});

describe("IntakeDetail missing and error states (INT-028)", () => {
  it("reports a stale id as missing", async () => {
    const server: DetailServer = { detailStatus: 404, detail: detailPayload(), writes: [] };
    const { queryClient } = harness(server);
    const { onMissing } = renderDetail(queryClient);
    await waitFor(() => expect(onMissing).toHaveBeenCalledTimes(1));
    expect(screen.queryByTestId("intake-detail")).not.toBeInTheDocument();
  });

  it("offers a retry when the read fails", async () => {
    const server: DetailServer = { detailStatus: 500, detail: detailPayload(), writes: [] };
    const { queryClient } = harness(server);
    renderDetail(queryClient);
    expect(await screen.findByTestId("intake-detail-error")).toBeInTheDocument();
    server.detailStatus = 200;
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(await screen.findByTestId("intake-detail")).toBeInTheDocument();
  });
});

describe("IntakeDetail triage wiring (INT-020–025)", () => {
  it("shows the buttons, chip and full menu to an admin on an open request", async () => {
    const user = userEvent.setup();
    const server: DetailServer = { detailStatus: 200, detail: detailPayload(), writes: [] };
    const { queryClient } = harness(server);
    renderDetail(queryClient);

    expect(await screen.findByTestId("intake-detail-header")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Accept" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Decline" })).toBeInTheDocument();
    expect(screen.getByTestId("intake-status-chip")).toHaveTextContent("Pending");
    await user.click(screen.getByRole("button", { name: "Request actions" }));
    const items = await screen.findAllByRole("menuitem");
    expect(items.map((item) => item.textContent)).toEqual([
      "Snooze",
      "Mark as duplicate",
      "Copy work item link",
      "Delete",
    ]);
  });

  it("declines through the confirmation and advances", async () => {
    const user = userEvent.setup();
    const server: DetailServer = { detailStatus: 200, detail: detailPayload(), writes: [] };
    const { queryClient } = harness(server);
    const { onAdvance } = renderDetail(queryClient);

    expect(await screen.findByTestId("intake-detail-header")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Decline" }));
    expect(screen.getByRole("dialog", { name: "Decline work item" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Decline" }));
    await waitFor(() => expect(server.writes).toHaveLength(1));
    expect(server.writes[0]?.body).toEqual({ status: -1 });
    expect(onAdvance).toHaveBeenCalledWith(ISSUE);
  });

  it("un-snoozes from the menu and advances", async () => {
    const user = userEvent.setup();
    const future = new Date();
    future.setDate(future.getDate() + 3);
    const snoozed = detailPayload(0);
    snoozed.snoozed_till = future.toISOString();
    const server: DetailServer = { detailStatus: 200, detail: snoozed, writes: [] };
    const { queryClient } = harness(server);
    const { onAdvance } = renderDetail(queryClient);

    expect(await screen.findByTestId("intake-detail-header")).toBeInTheDocument();
    expect(screen.getByTestId("intake-status-chip")).toHaveTextContent("3 days to go");
    await user.click(screen.getByRole("button", { name: "Request actions" }));
    await user.click(await screen.findByRole("menuitem", { name: "Un snooze" }));
    await waitFor(() => expect(server.writes).toHaveLength(1));
    expect(server.writes[0]?.body).toEqual({ status: -2, snoozed_till: null });
    expect(onAdvance).toHaveBeenCalledWith(ISSUE);
  });

  it("refuses a member accept with a permission notice", async () => {
    const user = userEvent.setup();
    const server: DetailServer = { detailStatus: 200, detail: detailPayload(), writes: [] };
    const { queryClient } = harness(server);
    renderDetail(queryClient, { role: 15 });
    render(<ToastHost manager={getToastManager()} />);

    expect(await screen.findByTestId("intake-detail-header")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Accept" }));
    expect(await screen.findByText("Permission denied")).toBeInTheDocument();
    expect(screen.getByText(/project admin/)).toBeInTheDocument();
    expect(server.writes).toEqual([]);
  });

  it("opens delete from the overflow menu and cancels without writing", async () => {
    const user = userEvent.setup();
    const server: DetailServer = { detailStatus: 200, detail: detailPayload(), writes: [] };
    const { queryClient } = harness(server);
    renderDetail(queryClient);

    expect(await screen.findByTestId("intake-detail-header")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Request actions" }));
    await user.click(await screen.findByRole("menuitem", { name: "Delete" }));
    expect(screen.getByRole("dialog", { name: "Delete work item" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(server.writes).toEqual([]);
  });

  it("omits delete from the menu on someone else's request for a member", async () => {
    const user = userEvent.setup();
    const server: DetailServer = { detailStatus: 200, detail: detailPayload(), writes: [] };
    const { queryClient } = harness(server);
    renderDetail(queryClient, { role: 15 });

    expect(await screen.findByTestId("intake-detail-header")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Request actions" }));
    const items = await screen.findAllByRole("menuitem");
    expect(items.map((item) => item.textContent)).toEqual(["Snooze", "Mark as duplicate", "Copy work item link"]);
  });

  it("hides every triage control from a guest on an open request", async () => {
    const server: DetailServer = { detailStatus: 200, detail: detailPayload(), writes: [] };
    const { queryClient } = harness(server);
    renderDetail(queryClient, { role: 5 });

    expect(await screen.findByTestId("intake-detail-header")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Accept" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Decline" })).not.toBeInTheDocument();
    expect(screen.queryByTestId("intake-detail-menu")).not.toBeInTheDocument();
    expect(screen.getByTestId("intake-status-chip")).toHaveTextContent("Pending");
  });

  it("hides triage on a resolved request and links the duplicate target", async () => {
    const duplicate = detailPayload(2);
    duplicate.duplicate_to = "22222222-2222-4222-8222-222222222222";
    duplicate.duplicate_issue_detail = {
      id: "22222222-2222-4222-8222-222222222222",
      name: "Original work",
      priority: "none",
      sequence_id: 3,
      project_id: PROJECT,
      created_at: "2026-10-01T00:00:00Z",
      created_by: null,
    };
    const server: DetailServer = { detailStatus: 200, detail: duplicate, writes: [] };
    const { queryClient } = harness(server);
    renderDetail(queryClient);

    expect(await screen.findByTestId("intake-detail-header")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Accept" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Decline" })).not.toBeInTheDocument();
    expect(screen.queryByTestId("intake-detail-menu")).not.toBeInTheDocument();
    expect(screen.getByTestId("intake-status-chip")).toHaveTextContent("Duplicate");
    const link = screen.getByTestId("intake-duplicate-of");
    expect(link).toHaveTextContent("APO-3");
    expect(link).toHaveAttribute("href", "/acme/browse/APO-3/");
  });
});

describe("IntakeDetail mobile header (INT-030)", () => {
  function renderNarrow(props: Partial<React.ComponentProps<typeof IntakeDetail>> = {}, detail = detailPayload()) {
    const server: DetailServer = { detailStatus: 200, detail, writes: [] };
    const { queryClient } = harness(server);
    const onToggleList = vi.fn();
    const rendered = renderDetail(queryClient, { narrow: true, listOpen: true, onToggleList, ...props });
    return { server, onToggleList, ...rendered };
  }

  it("renders the mobile header on narrow viewports and toggles the list", async () => {
    const { onToggleList } = renderNarrow();
    expect(await screen.findByTestId("intake-mobile-header")).toBeInTheDocument();
    expect(screen.queryByTestId("intake-detail-header")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Request list" }));
    expect(onToggleList).toHaveBeenCalledTimes(1);
  });

  it("keeps the desktop header on narrow viewports without the toggle", async () => {
    const server: DetailServer = { detailStatus: 200, detail: detailPayload(), writes: [] };
    const { queryClient } = harness(server);
    renderDetail(queryClient, { narrow: true });
    expect(await screen.findByTestId("intake-detail-header")).toBeInTheDocument();
    expect(screen.queryByTestId("intake-mobile-header")).not.toBeInTheDocument();
  });

  it("keeps every triage action reachable from the mobile menu", async () => {
    const user = userEvent.setup();
    const { server } = renderNarrow();
    expect(await screen.findByTestId("intake-mobile-header")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Request actions" }));
    const items = await screen.findAllByRole("menuitem");
    expect(items.map((item) => item.textContent)).toEqual([
      "Snooze",
      "Mark as duplicate",
      "Accept",
      "Decline",
      "Delete",
      "Copy work item link",
    ]);
    await user.click(screen.getByRole("menuitem", { name: "Delete" }));
    expect(screen.getByRole("dialog", { name: "Delete work item" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(server.writes).toEqual([]);
  });

  it("refuses a member accept from the mobile menu with the desktop notice", async () => {
    const user = userEvent.setup();
    const { server } = renderNarrow({ role: 15 });
    render(<ToastHost manager={getToastManager()} />);
    expect(await screen.findByTestId("intake-mobile-header")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Request actions" }));
    await user.click(await screen.findByRole("menuitem", { name: "Accept" }));
    expect(await screen.findByText("Permission denied")).toBeInTheDocument();
    expect(server.writes).toEqual([]);
  });

  it("shows only the link and open entries on a resolved request, and opens the work item", async () => {
    const user = userEvent.setup();
    renderNarrow({ role: 5 }, detailPayload(1));
    expect(await screen.findByTestId("intake-mobile-header")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Request actions" }));
    const items = await screen.findAllByRole("menuitem");
    expect(items.map((item) => item.textContent)).toEqual(["Copy work item link", "Open work item"]);
    await user.click(screen.getByRole("menuitem", { name: "Open work item" }));
    expect(navigate).toHaveBeenCalledWith({ to: "/acme/browse/APO-7/" });
  });

  it("offers a guest an empty mobile menu on an open request", async () => {
    const user = userEvent.setup();
    renderNarrow({ role: 5 });
    expect(await screen.findByTestId("intake-mobile-header")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Request actions" }));
    expect(await screen.findByRole("menu")).toBeInTheDocument();
    expect(screen.queryByRole("menuitem")).not.toBeInTheDocument();
  });

  it("offers a guest creator delete on their open request (INT-025/030)", async () => {
    const user = userEvent.setup();
    const server: DetailServer = {
      detailStatus: 200,
      detail: detailPayload(-2, { created_by: ME }),
      writes: [],
    };
    const { queryClient } = harness(server);
    renderDetail(queryClient, { role: 5, narrow: true, listOpen: true, onToggleList: () => undefined });
    expect(await screen.findByTestId("intake-mobile-header")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Request actions" }));
    expect((await screen.findAllByRole("menuitem")).map((item) => item.textContent)).toEqual(["Delete"]);
    await user.click(screen.getByRole("menuitem", { name: "Delete" }));
    expect(screen.getByRole("dialog", { name: "Delete work item" })).toBeInTheDocument();
  });

  it("fires the snooze, duplicate and decline entries through the shared dialogs", async () => {
    const user = userEvent.setup();
    renderNarrow();
    expect(await screen.findByTestId("intake-mobile-header")).toBeInTheDocument();
    const cases = [
      ["Snooze", "Snooze request"],
      ["Mark as duplicate", "Mark as duplicate"],
      ["Decline", "Decline work item"],
    ] as Array<[string, string]>;
    for (const [entry, dialog] of cases) {
      await user.click(screen.getByRole("button", { name: "Request actions" }));
      await user.click(await screen.findByRole("menuitem", { name: entry }));
      expect(screen.getByRole("dialog", { name: dialog })).toBeInTheDocument();
      await user.click(screen.getByRole("button", { name: "Cancel" }));
      await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    }
  });

  it("un-snoozes from the mobile menu and advances", async () => {
    const user = userEvent.setup();
    const future = new Date();
    future.setDate(future.getDate() + 3);
    const snoozed = detailPayload(0);
    snoozed.snoozed_till = future.toISOString();
    const server: DetailServer = { detailStatus: 200, detail: snoozed, writes: [] };
    const { queryClient } = harness(server);
    const onAdvance = vi.fn(() => false);
    renderDetail(queryClient, { narrow: true, listOpen: true, onToggleList: () => undefined, onAdvance });
    expect(await screen.findByTestId("intake-mobile-header")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Request actions" }));
    await user.click(await screen.findByRole("menuitem", { name: "Un snooze" }));
    await waitFor(() => expect(server.writes).toHaveLength(1));
    expect(server.writes[0]?.body).toEqual({ status: -2, snoozed_till: null });
    expect(onAdvance).toHaveBeenCalledWith(ISSUE);
  });

  it("copies the work-item link from the mobile menu", async () => {
    const user = userEvent.setup();
    const writeText = vi.fn(async () => undefined);
    Object.defineProperty(globalThis.navigator, "clipboard", { configurable: true, value: { writeText } });
    renderNarrow();
    render(<ToastHost manager={getToastManager()} />);
    expect(await screen.findByTestId("intake-mobile-header")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Request actions" }));
    await user.click(await screen.findByRole("menuitem", { name: "Copy work item link" }));
    expect(writeText).toHaveBeenCalledWith(expect.stringContaining("/acme/browse/APO-7/"));
    expect(await screen.findByText("Copied the work item link.")).toBeInTheDocument();
  });
});

describe("IntakeDetail duplicate detection (INT-018)", () => {
  function searchHit(id: string, name: string) {
    return {
      name,
      id,
      start_date: null,
      sequence_id: 9,
      project__name: "Atlas",
      project__identifier: "APO",
      project_id: PROJECT,
      workspace__slug: SLUG,
      state__name: null,
      state__group: null,
      state__color: null,
    };
  }

  it("renders no indicator and issues no search traffic in OSS", async () => {
    const server: DetailServer = { detailStatus: 200, detail: detailPayload(), writes: [] };
    const { queryClient, searches } = harness(server);
    renderDetail(queryClient);
    expect(await screen.findByTestId("intake-detail-header")).toBeInTheDocument();
    // Any debounced lookup would have fired by now; the seam issues none.
    await new Promise((resolve) => setTimeout(resolve, 800));
    expect(screen.queryByTestId("intake-duplicate-indicator")).not.toBeInTheDocument();
    expect(searches).toEqual([]);
  });

  it("renders the indicator and opens the comparison with the seam on, excluding the viewed request", async () => {
    const user = userEvent.setup();
    editionHarness.flags = { [ISSUE_DEDUPE_FLAG]: true };
    const server: DetailServer = { detailStatus: 200, detail: detailPayload(), writes: [] };
    const { queryClient, searches } = harness(server, {
      searchHits: [searchHit(ISSUE, "A request"), searchHit(ADA, "A request followup")],
    });
    renderDetail(queryClient);
    const indicator = await screen.findByTestId("intake-duplicate-indicator");
    expect(searches).toHaveLength(1);
    const button = within(indicator).getByRole("button", { name: "1 possible duplicate" });
    await user.click(button);
    const panel = within(indicator).getByRole("complementary", { name: "Possible duplicates" });
    expect(panel).toHaveTextContent("A request followup");
  });
});

describe("IntakeDetail source indicator (INT-034)", () => {
  it("renders no pill in OSS even with a cloud origin", async () => {
    const server: DetailServer = { detailStatus: 200, detail: { ...detailPayload(), source: "EMAIL" }, writes: [] };
    const { queryClient } = harness(server);
    renderDetail(queryClient);
    expect(await screen.findByTestId("intake-detail-header")).toBeInTheDocument();
    expect(screen.queryByTestId("intake-source-pill")).not.toBeInTheDocument();
  });

  it("renders the pill with the flag on and hides the in-app default", async () => {
    editionHarness.flags = { [INTAKE_SOURCE_PILL_FLAG]: true };
    const server: DetailServer = { detailStatus: 200, detail: { ...detailPayload(), source: "EMAIL" }, writes: [] };
    const { queryClient } = harness(server);
    renderDetail(queryClient);
    expect(await screen.findByTestId("intake-source-pill")).toHaveTextContent("Email");
  });

  it("renders no pill for the in-app default even with the flag on", async () => {
    editionHarness.flags = { [INTAKE_SOURCE_PILL_FLAG]: true };
    const server: DetailServer = { detailStatus: 200, detail: { ...detailPayload(), source: "IN_APP" }, writes: [] };
    const { queryClient } = harness(server);
    renderDetail(queryClient);
    expect(await screen.findByTestId("intake-detail-header")).toBeInTheDocument();
    expect(screen.queryByTestId("intake-source-pill")).not.toBeInTheDocument();
  });
});
