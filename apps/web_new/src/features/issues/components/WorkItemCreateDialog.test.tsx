// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Create-dialog behavior (parity/specs/issues.md §11, ISS-117–123). The
// transport stub answers every read the dialog opens plus the writes it
// makes, so each case drives the real composition: project selector,
// title rules and fake-timer generation, Create more, the discard guard,
// the OSS duplicate seam, project-change resets and the submit payload.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { generateAssistantTitle } from "@pidash/api-client";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { edition } from "../../../core/edition/index.js";
import { FULL_RUN_TEST_TIMEOUT_MS } from "../../../testing/testBudget.js";
import { pickerKeys } from "../../../shared/pickers/reference.js";
import { withTimeout } from "../create/createApi.js";
import { ISSUE_DEDUPE_FLAG } from "../create/dedupe.js";
import {
  registerIssueTypeExtension,
  resetIssueTypeExtension,
  type IssueTypeExtensionProps,
} from "../create/issueTypeSeam.js";
import { loadWorkItemDraft } from "../lib/createDraft.js";
import { CREATE_DIALOG_TITLE, WorkItemCreateDialog, type WorkItemCreateProject } from "./WorkItemCreateDialog.js";

// These cases drive the whole strip through real menus, calendars and a
// debounced editor; each interaction is a full popover round trip. Under a
// loaded CI worker a single case can need well over the 5 s default, so
// use the measured full-run budget without relaxing any assertion.
vi.setConfig({ testTimeout: FULL_RUN_TEST_TIMEOUT_MS });

// The real editor is a lazy tiptap surface with its own suite. The dialog
// only composes it, so a textarea stands in and reports HTML through the
// same callbacks; the sanitizer helpers stay real. Both editor variants
// share the stand-in because the modal swaps to the mention-aware one.
vi.mock("../../../shared/editor/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../shared/editor/index.js")>();
  // Mirrors the real editor: the starting HTML is read once, at mount,
  // so a caller that wants a cleared surface must remount this.
  function StandInEditor({
    onChange,
    label,
    initialHtml,
    onReady,
    onMentionQuery,
    handleKeyDown,
    tabIndex,
  }: {
    onChange: (html: string) => void;
    label: string;
    initialHtml: string;
    onReady?: ((handle: unknown) => void) | undefined;
    onMentionQuery?: ((query: string | null) => void) | undefined;
    handleKeyDown?: ((event: KeyboardEvent) => boolean) | undefined;
    tabIndex?: number | undefined;
  }) {
    const [value, setValue] = React.useState(() => initialHtml.replace(/^<p>/, "").replace(/<\/p>$/, ""));
    const valueRef = React.useRef(value);
    valueRef.current = value;
    React.useEffect(() => {
      onReady?.({
        insertImage: (src: string) => {
          // The real editor reports the inserted image through onChange;
          // the stand-in does the same so a payload round-trip is real.
          const html = `<img src="${src}">`;
          setValue((current) => `${current}${html}`);
          onChange(html);
        },
        insertMention: (target: { id: string; name: string }) => {
          setValue((current) => `${current}@${target.name}`);
        },
        insertHtml: (html: string) => {
          setValue((current) => `${current}${html}`);
        },
        textBeforeCaret: () => valueRef.current,
      });
    }, [onReady]);
    return (
      <textarea
        aria-label={label}
        {...(tabIndex === undefined ? {} : { tabIndex })}
        value={value}
        onChange={(event) => {
          const next = event.target.value;
          setValue(next);
          onChange(next === "" ? "" : `<p>${next}</p>`);
          const at = next.lastIndexOf("@");
          const tail = at === -1 ? "" : next.slice(at + 1);
          onMentionQuery?.(at !== -1 && !/\s/.test(tail) ? tail : null);
        }}
        onKeyDown={(event) => {
          if (handleKeyDown?.(event.nativeEvent) === true) event.preventDefault();
        }}
      />
    );
  }
  return { ...original, RichTextEditor: StandInEditor, IssueDescriptionEditor: StandInEditor };
});

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return { ...original, generateAssistantTitle: vi.fn() };
});

vi.mock("../../../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../shared/shell/index.js")>();
  return { ...original, notifyError: vi.fn(), notifySuccess: vi.fn(), notifySuccessWithAction: vi.fn() };
});

import { notifyError, notifySuccess, notifySuccessWithAction } from "../../../shared/shell/index.js";

const SLUG = "acme";
const PROJECT_ATLAS = "223e4567-e89b-12d3-a456-426614174001";
const PROJECT_BEACON = "323e4567-e89b-12d3-a456-426614174002";
const PROJECT_GUEST = "423e4567-e89b-12d3-a456-426614174003";
const WORKSPACE_ID = "523e4567-e89b-12d3-a456-426614174004";
const CREATED_ID = "623e4567-e89b-12d3-a456-426614174005";
const STATE_ID = "723e4567-e89b-12d3-a456-426614174006";
const PARENT_ID = "823e4567-e89b-12d3-a456-426614174007";

const PROJECTS: WorkItemCreateProject[] = [
  { id: PROJECT_ATLAS, name: "Atlas", identifier: "ATL", memberRole: 15, archived: false },
  { id: PROJECT_BEACON, name: "Beacon", identifier: "BCN", memberRole: 20, archived: false },
  { id: PROJECT_GUEST, name: "Guestland", identifier: "GST", memberRole: 5, archived: false },
];

function projectRow(
  id: string,
  name: string,
  identifier: string,
  role: number | null,
  overrides: Record<string, unknown> = {}
) {
  return {
    id,
    name,
    identifier,
    sort_order: 1,
    logo_props: {},
    member_role: role,
    intake_count: 0,
    archived_at: null,
    workspace: WORKSPACE_ID,
    cycle_view: false,
    issue_views_view: false,
    module_view: false,
    page_view: false,
    inbox_view: false,
    is_default: false,
    guest_view_all_features: false,
    project_lead: null,
    network: 0,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    created_by: null,
    updated_by: null,
    ...overrides,
  };
}

function projectDetail(id: string, name: string, overrides: Record<string, unknown> = {}) {
  const { intake_count: _count, ...row } = projectRow(id, name, name.slice(0, 3).toUpperCase(), 15);
  return {
    ...row,
    description: "",
    description_text: null,
    description_html: null,
    default_assignee: null,
    emoji: null,
    icon_prop: null,
    intake_view: false,
    is_time_tracking_enabled: false,
    is_issue_type_enabled: false,
    members_can_edit_states: false,
    cover_image: null,
    cover_image_asset: null,
    cover_image_url: null,
    estimate: null,
    archive_in: 0,
    close_in: 0,
    default_state: null,
    timezone: "UTC",
    external_source: null,
    external_id: null,
    repo_url: "",
    base_branch: "main",
    is_favorite: false,
    anchor: null,
    members: [],
    next_work_item_sequence: 1,
    agent_executor_options: [],
    default_agent_executor: "local_runner",
    deleted_at: null,
    ...overrides,
  };
}

/** A thin project cycle row the picker lists. */
function cycleRow(id: string, name: string) {
  return {
    id,
    name,
    project_id: PROJECT_ATLAS,
    is_favorite: false,
    status: "ACTIVE",
  };
}

/** A thin project module row the picker lists. */
function moduleRow(id: string, name: string) {
  return {
    id,
    name,
    project_id: PROJECT_ATLAS,
    status: "in-progress",
    is_favorite: false,
  };
}

function createdRow(projectId: string, name: string): Record<string, unknown> {
  return {
    id: CREATED_ID,
    name,
    state_id: null,
    sort_order: 65535,
    completed_at: null,
    estimate_point: null,
    priority: "none",
    start_date: null,
    target_date: null,
    sequence_id: 7,
    project_id: projectId,
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
    created_by: null,
    updated_by: null,
    is_draft: false,
    archived_at: null,
    deleted_at: null,
  };
}

/** The create serializer's row, as the promote endpoint answers it. */
function promotedRow(projectId: string, name: string): Record<string, unknown> {
  return {
    id: CREATED_ID,
    name,
    project_id: projectId,
    sequence_id: 8,
    state_id: null,
    priority: "none",
    sort_order: 65535,
    parent_id: null,
    description_html: "",
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-02-03T04:05:06Z",
    deleted_at: null,
  };
}

interface StubOptions {
  createStatus?: number;
  createDelayMs?: number;
  editStatus?: number;
  publishStatus?: number;
  assistant?: { model_name: string; has_api_key: boolean };
  generate?: { status: number; body: unknown; delayMs?: number };
  draftStatus?: number;
  membershipStatus?: number;
  projects?: Record<string, Record<string, unknown>>;
  role?: number;
  estimateSystem?: boolean;
  /** The detail row an issue edit reads; absent answers 404. */
  issueDetail?: Record<string, unknown>;
  /** Hold a description image upload so the editor's busy state shows. */
  uploadDelayMs?: number;
}

interface Call {
  url: string;
  method: string;
  body: unknown;
}

function stubTransport(options: StubOptions = {}) {
  const calls: Call[] = [];
  const sent: unknown[] = [];
  const details: Record<string, Record<string, unknown>> = options.projects ?? {
    [PROJECT_ATLAS]: projectDetail(PROJECT_ATLAS, "Atlas"),
    [PROJECT_BEACON]: projectDetail(PROJECT_BEACON, "Beacon"),
  };
  createAppClient("https://api.test", (async (url: string, init: { method?: string; body?: string }) => {
    const method = init.method ?? "GET";
    const body: unknown = init.body === undefined ? undefined : JSON.parse(init.body);
    calls.push({ url, method, body });
    const json = (status: number, payload: unknown) => ({
      status,
      url,
      headers: { get: () => null },
      text: async () => JSON.stringify(payload),
    });
    if (url.endsWith("/auth/get-csrf-token/")) return json(200, { csrf_token: "csrf" });
    if (url.endsWith("/api/users/me/workspaces/")) {
      return json(200, [
        { id: WORKSPACE_ID, name: "Acme", slug: SLUG, logo_url: null, total_members: 3, role: options.role ?? 15 },
      ]);
    }
    if (url.endsWith("/api/users/me/")) {
      return json(200, {
        id: WORKSPACE_ID,
        email: "pilot@example.com",
        username: "pilot",
        first_name: "Pilot",
        last_name: "User",
        display_name: "Pilot User",
        avatar: "",
        avatar_url: null,
        cover_image: null,
        cover_image_url: null,
        date_joined: "2026-01-01T00:00:00Z",
        user_timezone: "UTC",
        is_active: true,
        is_bot: false,
        is_email_verified: true,
        is_password_autoset: false,
        last_login_medium: "email",
        last_login_time: null,
      });
    }
    if (url.endsWith("/api/users/me/profile/")) {
      return json(200, {
        id: WORKSPACE_ID,
        user: WORKSPACE_ID,
        theme: {},
        is_app_rail_docked: false,
        is_tour_completed: true,
        onboarding_step: {},
        use_case: null,
        role: null,
        is_onboarded: true,
        last_workspace_id: WORKSPACE_ID,
        billing_address_country: "US",
        billing_address: null,
        has_billing_address: false,
        company_name: "",
        notification_view_mode: "inline",
        is_smooth_cursor_enabled: false,
        is_mobile_onboarded: true,
        mobile_onboarding_step: {},
        mobile_timezone_auto_set: false,
        language: "en",
        start_of_the_week: 0,
        goals: {},
        background_color: "",
        is_navigation_tour_completed: true,
        has_marketing_email_consent: false,
        is_subscribed_to_changelog: false,
        product_tour: {},
        settings: {},
        created_at: "2026-01-01T00:00:00Z",
        updated_at: "2026-01-01T00:00:00Z",
      });
    }
    if (url.includes("/workspace-members/me/")) {
      return json(200, {
        id: WORKSPACE_ID,
        workspace: WORKSPACE_ID,
        member: WORKSPACE_ID,
        role: options.role ?? 15,
        company_role: null,
        view_props: {},
        default_props: {},
        issue_props: {},
        is_active: true,
        getting_started_checklist: {},
        tips: {},
        explored_features: {},
        draft_issue_count: 0,
        created_by: null,
        updated_by: null,
        created_at: "2026-01-01T00:00:00Z",
        updated_at: "2026-01-01T00:00:00Z",
      });
    }
    if (url.endsWith("/api/users/me/ai-assistant/config/")) {
      const assistant = options.assistant ?? { model_name: "", has_api_key: false };
      return json(200, {
        provider_kind: "openai",
        base_url: "",
        model_name: assistant.model_name,
        has_api_key: assistant.has_api_key,
        last_verified_at: null,
      });
    }
    if (url.endsWith("/ai-assistant/generate-title/")) {
      const generate = options.generate ?? { status: 200, body: { title: "Generated title" } };
      if (generate.delayMs !== undefined) {
        await new Promise((resolve) => globalThis.setTimeout(resolve, generate.delayMs));
      }
      return json(generate.status, generate.body);
    }
    if (url.endsWith("/draft-issues/") && method === "POST") {
      sent.push(body);
      const status = options.draftStatus ?? 201;
      return json(
        status,
        status === 201
          ? {
              id: CREATED_ID,
              name: "Draft",
              state_id: null,
              sort_order: 1,
              completed_at: null,
              priority: "none",
              project_id: null,
              parent_id: null,
              cycle_id: null,
              module_ids: [],
              label_ids: [],
              assignee_ids: [],
              created_at: "2026-01-01T00:00:00Z",
              updated_at: "2026-01-01T00:00:00Z",
              created_by: null,
              updated_by: null,
              type_id: null,
              description_html: "",
            }
          : {}
      );
    }
    if (url.includes("/search-issues/")) {
      return json(200, [
        {
          name: "Parent work",
          id: PARENT_ID,
          start_date: null,
          sequence_id: 12,
          project__name: "Atlas",
          project__identifier: "ATL",
          project_id: PROJECT_ATLAS,
          workspace__slug: SLUG,
          state__name: "In Progress",
          state__group: "started",
          state__color: "#d9912b",
        },
      ]);
    }
    if (url.endsWith("/states/")) {
      return json(200, [
        {
          id: STATE_ID,
          project_id: PROJECT_ATLAS,
          workspace_id: WORKSPACE_ID,
          name: "Todo",
          color: "#5e6ad2",
          group: "unstarted",
          default: true,
          description: "",
          sequence: 1,
        },
      ]);
    }
    if (url.endsWith("/runners/pods/") || /\/runners\/pods\/\?/.test(url)) {
      return json(200, [
        {
          id: PARENT_ID,
          name: "Local pod",
          description: "",
          is_default: true,
          workspace: WORKSPACE_ID,
          project: PROJECT_ATLAS,
          project_identifier: "ATL",
          created_by: null,
          runner_count: 1,
          created_at: "2026-01-01T00:00:00Z",
          updated_at: "2026-01-01T00:00:00Z",
        },
      ]);
    }
    if (url.endsWith("/issue-labels/")) {
      return json(200, [
        {
          id: STATE_ID,
          project_id: PROJECT_ATLAS,
          workspace_id: WORKSPACE_ID,
          name: "Bug",
          color: "#eb5757",
          parent: null,
          sort_order: 1,
        },
      ]);
    }
    if (url.endsWith(`/projects/${PROJECT_ATLAS}/members/`)) {
      return json(200, [
        {
          id: STATE_ID,
          role: 15,
          member: WORKSPACE_ID,
          project: PROJECT_ATLAS,
          original_role: 15,
          created_at: "2026-01-01T00:00:00Z",
        },
      ]);
    }
    if (url.includes("/workspace-members/")) {
      return json(200, []);
    }
    if (url.endsWith("/members/") && url.includes("/workspaces/")) {
      return json(200, [
        {
          id: STATE_ID,
          member: {
            id: WORKSPACE_ID,
            first_name: "Pilot",
            last_name: "User",
            avatar: "",
            avatar_url: null,
            display_name: "Pilot User",
            is_bot: false,
          },
          role: 15,
          is_active: true,
        },
      ]);
    }
    if (url.endsWith("/cycles/") && method === "GET") {
      return json(200, [
        { id: PARENT_ID, project_id: PROJECT_ATLAS, name: "Sprint 4", is_favorite: false, status: "ACTIVE" },
      ]);
    }
    if (url.endsWith("/modules/") && method === "GET") {
      return json(200, [
        { id: PARENT_ID, project_id: PROJECT_ATLAS, name: "Checkout", status: "in-progress", is_favorite: false },
      ]);
    }
    if (url.endsWith("/estimates/")) {
      return json(
        200,
        options.estimateSystem === true
          ? [
              {
                id: STATE_ID,
                workspace: WORKSPACE_ID,
                project: PROJECT_ATLAS,
                name: "Points",
                description: "",
                type: "points",
                last_used: true,
                points: [{ id: PARENT_ID, key: 1, value: "3", description: "" }],
                created_at: "2026-01-01T00:00:00Z",
                updated_at: "2026-01-01T00:00:00Z",
                created_by: null,
                updated_by: null,
              },
            ]
          : []
      );
    }
    const detailMatch = /\/projects\/([^/]+)\/$/.exec(url);
    if (detailMatch && method === "GET") {
      const row = details[detailMatch[1] ?? ""];
      return row === undefined ? json(404, {}) : json(200, row);
    }
    if (url.endsWith("/projects/") && method === "GET") {
      // A cold picker cache reads the joined list; serve the same rows the
      // dialog filters so choosing a project after an edit still works.
      return json(
        200,
        PROJECTS.map((project) =>
          projectRow(
            project.id,
            project.name,
            project.identifier ?? project.name.slice(0, 3),
            project.memberRole ?? null
          )
        )
      );
    }
    if (url.endsWith("/issues/") && method === "POST") {
      sent.push(body);
      const projectMatch = /\/projects\/([^/]+)\/issues\/$/.exec(url);
      if (options.createDelayMs !== undefined) {
        await new Promise((resolve) => globalThis.setTimeout(resolve, options.createDelayMs));
      }
      const status = options.createStatus ?? 201;
      const name = (body as { name?: string } | undefined)?.name ?? "Created";
      return json(status, status === 201 ? createdRow(projectMatch?.[1] ?? PROJECT_ATLAS, name) : {});
    }
    if (url.includes("/cycle-issues/") && method === "POST") {
      const status = options.membershipStatus ?? 201;
      return json(status, status === 201 ? { message: "ok" } : {});
    }
    if (url.includes("/cycle-issues/") && method === "DELETE") {
      return json(204, {});
    }
    if (/\/issues\/[^/]+\/$/.test(url) && method === "PATCH") {
      sent.push(body);
      const status = options.editStatus ?? 200;
      return json(status, status === 200 ? createdRow(PROJECT_ATLAS, "Edited") : {});
    }
    if (/\/issues\/[^/]+\/$/.test(url) && method === "GET") {
      const row = options.issueDetail;
      return row === undefined ? json(404, {}) : json(200, row);
    }
    if (/\/draft-issues\/[^/]+\/$/.test(url) && method === "PATCH") {
      sent.push(body);
      const status = options.editStatus ?? 200;
      return json(status, { id: "draft-1", name: "Draft" });
    }
    if (/\/draft-to-issue\/[^/]+\/$/.test(url) && method === "POST") {
      const status = options.publishStatus ?? 200;
      return json(status, status === 200 ? promotedRow(PROJECT_ATLAS, "Published") : {});
    }
    if (url.includes("/api/assets/v2/") && method === "POST") {
      if (options.uploadDelayMs !== undefined) {
        await new Promise((resolve) => globalThis.setTimeout(resolve, options.uploadDelayMs));
      }
      return json(200, {
        asset_id: "asset-1",
        asset_url: "https://cdn.test/asset-1.png",
        upload_data: { url: "https://s3.test/upload", fields: {} },
      });
    }
    if (url.includes("/api/assets/v2/")) {
      return json(204, {});
    }
    if (/\/issues\/[^/]+\/modules\/$/.test(url) && method === "POST") {
      const status = options.membershipStatus ?? 201;
      return json(status, status === 201 ? { message: "ok" } : {});
    }
    if (url.endsWith(`/projects/${PROJECT_ATLAS}/cycles/`)) {
      return json(200, [cycleRow(PARENT_ID, "Sprint 4")]);
    }
    if (url.endsWith(`/projects/${PROJECT_ATLAS}/modules/`)) {
      return json(200, [moduleRow(PARENT_ID, "Checkout")]);
    }
    return json(200, {});
  }) as never);
  return { calls, sent };
}

function setup(
  options: StubOptions = {},
  props: Partial<React.ComponentProps<typeof WorkItemCreateDialog>> = {},
  testOptions: { seedPicker?: boolean } = {}
) {
  const stub = stubTransport(options);
  const onOpenChange = vi.fn();
  const onCreated = vi.fn();
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  // The project picker reads its own joined-project list; seed it from
  // the same rows the dialog filters. A cold-cache case skips this so
  // the trigger must name the project from the dialog's own rows.
  if (testOptions.seedPicker !== false) {
    queryClient.setQueryData(pickerKeys.projects(SLUG), [
      ...PROJECTS.map((project) =>
        projectRow(project.id, project.name, project.identifier ?? project.name.slice(0, 3), project.memberRole ?? null)
      ),
      projectRow("923e4567-e89b-12d3-a456-426614174009", "Archived Atlas", "ARC", 15, {
        archived_at: "2026-01-01T00:00:00Z",
      }),
    ]);
  }
  // The cycle and module pickers list the seeded rows; their triggers name
  // the rows a stored edit holds, so the cached lists must carry them.
  queryClient.setQueryData(pickerKeys.cycles(SLUG, PROJECT_ATLAS), [cycleRow(PARENT_ID, "Sprint 4")]);
  queryClient.setQueryData(pickerKeys.modules(SLUG, PROJECT_ATLAS), [moduleRow(PARENT_ID, "Checkout")]);
  const view = render(
    <QueryClientProvider client={queryClient}>
      <WorkItemCreateDialog
        open
        workspaceSlug={SLUG}
        projects={PROJECTS}
        onOpenChange={onOpenChange}
        onCreated={onCreated}
        {...props}
      />
    </QueryClientProvider>
  );
  return { ...stub, onOpenChange, onCreated, queryClient, view };
}

function titleField(): HTMLInputElement {
  return screen.getByPlaceholderText("Title") as HTMLInputElement;
}

/** The dialog's submit button lives in the footer, outside the form. */
function save(): void {
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
}

/** Type into the rich description; the editor's own content node takes
 * the keystrokes so its change callback fires with real HTML. */
/** Type into the stand-in description field. */
async function typeDescription(_user: ReturnType<typeof userEvent.setup>, text: string): Promise<void> {
  const field = await screen.findByRole("textbox", { name: "Description" });
  fireEvent.change(field, { target: { value: text } });
  expect(field).toHaveValue(text);
}

function posts(sent: unknown[]): Array<Record<string, unknown>> {
  return sent.filter((entry): entry is Record<string, unknown> => typeof entry === "object" && entry !== null);
}

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(generateAssistantTitle).mockReset();
});

afterEach(() => {
  resetAppClient();
  globalThis.localStorage.clear();
  vi.useRealTimers();
});

describe("WorkItemCreateDialog core form", () => {
  it("focuses the title and shows the full field strip", async () => {
    setup();
    await waitFor(() => expect(titleField()).toHaveFocus());
    expect(screen.getByRole("dialog", { name: "Create new work item" })).toBeInTheDocument();
    for (const field of ["State", "Runs on", "Priority", "Assignees", "Parent"]) {
      expect(screen.getByText(field)).toBeInTheDocument();
    }
    // Labels and the dates name themselves on their own controls, as the
    // old form did, so no caption repeats them.
    expect(screen.getByRole("group", { name: "Labels" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Start date" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Due date" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Add parent" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Add parent" })).toBeInTheDocument();
  });

  it("leaves focus in the description while the user types there", async () => {
    const user = userEvent.setup();
    setup();
    await waitFor(() => expect(titleField()).toHaveFocus());
    await user.type(titleField(), "Has a title");
    const description = await screen.findByRole("textbox", { name: "Description" });
    await user.click(description);
    await user.keyboard("abc");
    expect(description).toHaveValue("abc");
    expect(description).toHaveFocus();
    expect(titleField()).toHaveValue("Has a title");
    expect(titleField()).not.toHaveFocus();
  });

  it("offers only create-permission projects and pre-selects the requested one", async () => {
    const user = userEvent.setup();
    setup({}, { initialProjectId: PROJECT_BEACON });
    await waitFor(() => expect(screen.getByText("Beacon")).toBeInTheDocument());
    void user;
  });

  it("shows the no-project notice when nothing is creatable", () => {
    setup({}, { projects: [PROJECTS[2]!] });
    expect(screen.getByText(/Join a project as a member or admin/)).toBeInTheDocument();
    expect(screen.queryByPlaceholderText("Title")).not.toBeInTheDocument();
  });

  it("refuses guests through the shell standing flag", () => {
    setup({}, { canCreate: false });
    expect(screen.getByText(/Join a project as a member or admin/)).toBeInTheDocument();
  });

  it("closes the no-project notice from its Close button", async () => {
    const user = userEvent.setup();
    const h = setup({}, { projects: [PROJECTS[2]!] });
    await user.click(screen.getByRole("button", { name: "Close" }));
    expect(h.onOpenChange).toHaveBeenCalledWith(false);
  });

  it("closes the no-project notice from Escape as well", async () => {
    const h = setup({}, { projects: [PROJECTS[2]!] });
    fireEvent.keyDown(document.activeElement ?? document.body, { key: "Escape" });
    await waitFor(() => expect(h.onOpenChange).toHaveBeenCalledWith(false));
  });

  it("keeps the kept title when the dialog opens again", async () => {
    globalThis.localStorage.setItem(
      "pidash:work-item-draft:acme",
      JSON.stringify({ title: "Half typed", projectId: PROJECT_BEACON })
    );
    setup({}, { initialProjectId: PROJECT_BEACON });
    await waitFor(() => expect(titleField()).toHaveValue("Half typed"));
    expect(loadWorkItemDraft(SLUG)?.title).toBe("Half typed");
  });

  it("treats a role-bearing row without identifier or archive flag as creatable", async () => {
    const h = setup(
      {},
      {
        projects: [{ id: PROJECT_BEACON, name: "Beacon", memberRole: 15 }],
        initialProjectId: PROJECT_BEACON,
      }
    );
    await userEvent.setup().type(titleField(), "Bare row");
    save();
    await waitFor(() => expect(posts(h.sent)).toHaveLength(1));
    expect(h.calls.some((call) => call.url.endsWith(`/projects/${PROJECT_BEACON}/issues/`))).toBe(true);
  });

  it("omits a row that carries no role at all from the create list", async () => {
    const h = setup({}, { projects: [{ id: PROJECT_ATLAS, name: "Atlas" }] });
    expect(screen.getByText(/Join a project as a member or admin/)).toBeInTheDocument();
    await waitFor(() => expect(h.sent).toHaveLength(0));
  });

  it("keeps the same visit when its own state changes while open", async () => {
    const user = userEvent.setup();
    const h = setup();
    await user.type(titleField(), "Still here");
    // A state change re-runs the effect with the same visit key; the
    // typed title must survive rather than reset to the kept draft.
    await user.click(screen.getByRole("button", { name: "Select target" }));
    await waitFor(() => expect(titleField()).toHaveValue("Still here"));
    expect(h.onOpenChange).not.toHaveBeenCalled();
  });

  it("keeps the typed title when the parent re-renders with a fresh projects array", async () => {
    const user = userEvent.setup();
    const h = setup();
    await user.type(titleField(), "Untouched");
    // The shell rebuilds the project list on every poll; a new array
    // identity re-runs the visit effect without starting a new visit.
    h.view.rerender(
      <QueryClientProvider client={h.queryClient}>
        <WorkItemCreateDialog
          open
          workspaceSlug={SLUG}
          projects={PROJECTS.map((project) => ({ ...project }))}
          onOpenChange={h.onOpenChange}
        />
      </QueryClientProvider>
    );
    await waitFor(() => expect(titleField()).toHaveValue("Untouched"));
    expect(screen.getByRole("dialog", { name: "Create new work item" })).toBeInTheDocument();
  });

  it("hides the form again when the parent closes the dialog", () => {
    const h = setup();
    expect(screen.getByPlaceholderText("Title")).toBeInTheDocument();
    h.view.rerender(
      <QueryClientProvider client={h.queryClient}>
        <WorkItemCreateDialog open={false} workspaceSlug={SLUG} projects={PROJECTS} onOpenChange={h.onOpenChange} />
      </QueryClientProvider>
    );
    expect(screen.queryByPlaceholderText("Title")).not.toBeInTheDocument();
  });
});

describe("WorkItemCreateDialog title rules (ISS-118)", () => {
  it("blocks an empty or whitespace title with an inline error", async () => {
    const user = userEvent.setup();
    const h = setup();
    save();
    expect(await screen.findByText("Give the work item a title.")).toBeInTheDocument();
    await user.type(titleField(), "   ");
    save();
    expect(screen.getByText("Give the work item a title.")).toBeInTheDocument();
    expect(h.sent).toHaveLength(0);
  });

  it("rejects a 256-character title", async () => {
    setup();
    fireEvent.change(titleField(), { target: { value: "n".repeat(256) } });
    save();
    expect(await screen.findByText(/255/)).toBeInTheDocument();
    expect(screen.getByRole("dialog", { name: "Create new work item" })).toBeInTheDocument();
  });

  it("posts the trimmed title and the set fields, then closes", async () => {
    const user = userEvent.setup();
    const h = setup();
    await user.type(titleField(), "  Fresh report  ");
    save();
    await waitFor(() => expect(h.onOpenChange).toHaveBeenCalledWith(false));
    expect(posts(h.sent)[0]).toEqual({ name: "Fresh report" });
    expect(h.onCreated).toHaveBeenCalledWith(CREATED_ID);
    expect(notifySuccess).toHaveBeenCalled();
    expect(h.calls.some((call) => call.url.endsWith(`/projects/${PROJECT_ATLAS}/issues/`))).toBe(true);
  });

  it("submits once while the first create is still in flight", async () => {
    const user = userEvent.setup();
    const h = setup({ createDelayMs: 60 });
    await user.type(titleField(), "One request only");
    // Click Save twice in the same tick: the second submit arrives while
    // the first write is pending and must not queue another create.
    const saveButton = screen.getByRole("button", { name: "Save" });
    fireEvent.click(saveButton);
    fireEvent.click(saveButton);
    await waitFor(() => expect(h.onOpenChange).toHaveBeenCalledWith(false));
    expect(posts(h.sent)).toHaveLength(1);
  });

  it("offers the created item as a toast follow-up when the caller knows the prefix", async () => {
    const user = userEvent.setup();
    setup({}, { projectIdentifier: "ATL" });
    await user.type(titleField(), "Linked report");
    save();
    await waitFor(() =>
      expect(notifySuccessWithAction).toHaveBeenCalledWith(
        "Work item created successfully",
        undefined,
        expect.objectContaining({ actionLabel: "View work item" })
      )
    );
  });

  it("accepts the next submit once the in-flight create failed", async () => {
    const user = userEvent.setup();
    const h = setup({ createStatus: 400 });
    await user.type(titleField(), "Try again");
    save();
    await waitFor(() => expect(notifyError).toHaveBeenCalled());
    save();
    await waitFor(() => expect(posts(h.sent)).toHaveLength(2));
  });

  it("re-arms submit after an inline title refusal", async () => {
    const user = userEvent.setup();
    const h = setup();
    // A blank title is refused without writing; typing a title and
    // submitting again must go through (the in-flight guard resets).
    save();
    expect(await screen.findByText("Give the work item a title.")).toBeInTheDocument();
    await user.type(titleField(), "Second try");
    save();
    await waitFor(() => expect(posts(h.sent)).toHaveLength(1));
    expect(posts(h.sent)[0]).toEqual({ name: "Second try" });
  });

  it("keeps the form open and toasts when the create fails", async () => {
    const user = userEvent.setup();
    const h = setup({ createStatus: 400 });
    await user.type(titleField(), "Fresh report");
    save();
    await waitFor(() => expect(notifyError).toHaveBeenCalled());
    expect(h.onOpenChange).not.toHaveBeenCalled();
    expect(titleField()).toHaveValue("Fresh report");
  });
});

describe("WorkItemCreateDialog full field strip (ISS-117)", () => {
  it("posts the state, target, priority, member and label strip", async () => {
    const user = userEvent.setup();
    const h = setup({
      estimateSystem: true,
      projects: {
        [PROJECT_ATLAS]: projectDetail(PROJECT_ATLAS, "Atlas", { cycle_view: true, module_view: true }),
        [PROJECT_BEACON]: projectDetail(PROJECT_BEACON, "Beacon"),
      },
    });
    await waitFor(() => expect(screen.getByRole("button", { name: "Cycle" })).toBeInTheDocument());
    await waitFor(() => expect(screen.getByText("Estimate")).toBeInTheDocument());
    await user.type(titleField(), "Everything at once");

    await user.click(screen.getByRole("button", { name: "Select state" }));
    await user.click(await screen.findByRole("option", { name: "Todo" }));

    await user.click(screen.getByRole("button", { name: "Select target" }));
    await user.click(await screen.findByRole("option", { name: /Local pod/ }));

    const prioritySlot = within(screen.getByText("Priority").parentElement as HTMLElement);
    await user.click(prioritySlot.getByRole("button", { name: "None" }));
    await user.click(await screen.findByRole("option", { name: "High" }));

    await user.click(screen.getByRole("button", { name: "Select members" }));
    // The seeded directory member is the signed-in user.
    await user.click(await screen.findByRole("option", { name: "You" }));

    await user.click(screen.getByRole("button", { name: "Labels" }));
    await user.click(await screen.findByTestId(/label-picker-option/));

    save();
    await waitFor(() => expect(posts(h.sent)).toHaveLength(1));
    expect(posts(h.sent)[0]).toMatchObject({
      name: "Everything at once",
      state_id: STATE_ID,
      priority: "high",
      assignee_ids: [WORKSPACE_ID],
      label_ids: [STATE_ID],
    });
  });

  it("posts the dates and the estimate from the strip", async () => {
    const user = userEvent.setup();
    const h = setup({
      estimateSystem: true,
      projects: {
        [PROJECT_ATLAS]: projectDetail(PROJECT_ATLAS, "Atlas", { cycle_view: true, module_view: true }),
        [PROJECT_BEACON]: projectDetail(PROJECT_BEACON, "Beacon"),
      },
    });
    await waitFor(() => expect(screen.getByText("Estimate")).toBeInTheDocument());
    await user.type(titleField(), "Placed and estimated");

    await user.click(screen.getByRole("button", { name: "Select target" }));
    await user.click(await screen.findByRole("option", { name: /Local pod/ }));

    const inMonthDays = (): HTMLButtonElement[] =>
      Array.from(document.querySelectorAll<HTMLButtonElement>('[data-day][data-in-month="true"]'));
    await user.click(screen.getByRole("button", { name: "Start date" }));
    await waitFor(() => expect(inMonthDays().length).toBeGreaterThan(12));
    const startDay = inMonthDays()[3]!;
    const startIso = startDay.dataset.day as string;
    await user.click(startDay);

    await user.click(screen.getByRole("button", { name: "Due date" }));
    await waitFor(() => expect(inMonthDays().length).toBeGreaterThan(12));
    const dueDay = inMonthDays().find((day) => (day.dataset.day as string) > startIso)!;
    const dueIso = dueDay.dataset.day as string;
    await user.click(dueDay);

    const estimateSlot = within(screen.getByText("Estimate").parentElement as HTMLElement);
    await user.click(estimateSlot.getByRole("button", { name: "None" }));
    await user.click(await screen.findByRole("option", { name: "3" }));

    save();
    await waitFor(() => expect(posts(h.sent)).toHaveLength(1));
    expect(posts(h.sent)[0]).toMatchObject({
      name: "Placed and estimated",
      start_date: startIso,
      target_date: dueIso,
      estimate_point: PARENT_ID,
      assigned_pod_id: PARENT_ID,
      agent_executor: "local_runner",
    });
  });

  it("posts the cycle and module picks as bridge writes, not create fields", async () => {
    const user = userEvent.setup();
    const h = setup({
      projects: {
        [PROJECT_ATLAS]: projectDetail(PROJECT_ATLAS, "Atlas", { cycle_view: true, module_view: true }),
        [PROJECT_BEACON]: projectDetail(PROJECT_BEACON, "Beacon"),
      },
    });
    await waitFor(() => expect(screen.getByRole("button", { name: "Cycle" })).toBeInTheDocument());
    await user.type(titleField(), "Sprint and module");

    await user.click(screen.getByRole("button", { name: "Cycle" }));
    await user.click((await screen.findAllByRole("option", { name: "Sprint 4" }))[0]!);

    await user.click(screen.getByRole("button", { name: "Modules" }));
    await user.click((await screen.findAllByRole("option", { name: "Checkout" }))[0]!);

    save();
    await waitFor(() => expect(posts(h.sent)).toHaveLength(1));
    expect(posts(h.sent)[0]).toMatchObject({ name: "Sprint and module" });
    // The create endpoint takes no cycle or module: both attach after
    // the row exists, through their bridge writes (ISS-117).
    expect(posts(h.sent)[0]).not.toHaveProperty("cycle_id");
    expect(posts(h.sent)[0]).not.toHaveProperty("module_ids");
    await waitFor(() =>
      expect(h.calls.some((call) => call.url.endsWith(`/cycles/${PARENT_ID}/cycle-issues/`))).toBe(true)
    );
    await waitFor(() => expect(h.calls.some((call) => call.url.endsWith(`/issues/${CREATED_ID}/modules/`))).toBe(true));
    const cycleCall = h.calls.find((call) => call.url.endsWith(`/cycles/${PARENT_ID}/cycle-issues/`));
    expect(cycleCall?.body).toEqual({ issues: [CREATED_ID] });
    const moduleCall = h.calls.find((call) => call.url.endsWith(`/issues/${CREATED_ID}/modules/`));
    expect(moduleCall?.body).toEqual({ modules: [PARENT_ID] });
  });

  it("keeps the created row and says so when the cycle attach fails", async () => {
    const user = userEvent.setup();
    const h = setup({
      membershipStatus: 500,
      projects: {
        [PROJECT_ATLAS]: projectDetail(PROJECT_ATLAS, "Atlas", { cycle_view: true }),
        [PROJECT_BEACON]: projectDetail(PROJECT_BEACON, "Beacon"),
      },
    });
    await waitFor(() => expect(screen.getByRole("button", { name: "Cycle" })).toBeInTheDocument());
    await user.type(titleField(), "Placed later");
    await user.click(screen.getByRole("button", { name: "Cycle" }));
    await user.click((await screen.findAllByRole("option", { name: "Sprint 4" }))[0]!);
    save();
    await waitFor(() =>
      expect(notifyError).toHaveBeenCalledWith("Work item created, but not placed", expect.any(String))
    );
    expect(posts(h.sent)).toHaveLength(1);
    expect(h.onCreated).toHaveBeenCalledWith(CREATED_ID);
  });
});

describe("WorkItemCreateDialog assistant titles (ISS-118)", () => {
  it("generates the title from the description before posting", async () => {
    vi.mocked(generateAssistantTitle).mockResolvedValue({ title: "  Named by the assistant  " });
    const user = userEvent.setup();
    const h = setup({
      assistant: { model_name: "pilot", has_api_key: true },
      generate: { status: 200, body: { title: "  Named by the assistant  " } },
    });
    await waitFor(() => expect(h.calls.some((call) => call.url.includes("ai-assistant/config"))).toBe(true));
    await typeDescription(user, "Words about the work");
    save();
    await waitFor(() => expect(posts(h.sent)).toHaveLength(1));
    expect(posts(h.sent)[0]).toEqual({
      name: "Named by the assistant",
      description_html: "<p>Words about the work</p>",
    });
  });

  it("refuses to generate without description text", async () => {
    const h = setup({ assistant: { model_name: "pilot", has_api_key: true } });
    // The config query lands a beat after its request; until then the
    // form cannot know an assistant is configured. Re-submitting is
    // harmless because nothing posts without a title.
    await waitFor(() => {
      save();
      expect(
        screen.getByText("Add a description first so the assistant has something to name it from.")
      ).toBeInTheDocument();
    });
    expect(h.sent).toHaveLength(0);
  });

  it("reports an empty assistant answer and keeps the form open", async () => {
    vi.mocked(generateAssistantTitle).mockResolvedValue({ title: "   " });
    const user = userEvent.setup();
    const h = setup({
      assistant: { model_name: "pilot", has_api_key: true },
      generate: { status: 200, body: { title: "   " } },
    });
    await waitFor(() => expect(h.calls.some((call) => call.url.includes("ai-assistant/config"))).toBe(true));
    await typeDescription(user, "Words");
    save();
    await waitFor(() =>
      expect(notifyError).toHaveBeenCalledWith("Couldn't generate a title", expect.stringContaining("no title"))
    );
    expect(h.sent).toHaveLength(0);
  });

  it("reports a failed generation and keeps the form open", async () => {
    vi.mocked(generateAssistantTitle).mockRejectedValue(new Error("offline"));
    const user = userEvent.setup();
    const h = setup({
      assistant: { model_name: "pilot", has_api_key: true },
      generate: { status: 500, body: {} },
    });
    await waitFor(() => expect(h.calls.some((call) => call.url.includes("ai-assistant/config"))).toBe(true));
    await typeDescription(user, "Words");
    save();
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith("Couldn't generate a title", expect.any(String)));
    expect(h.sent).toHaveLength(0);
  });
});

describe("WorkItemCreateDialog Create more (ISS-119)", () => {
  it("names the switch once for assistive tech", async () => {
    setup();
    expect(screen.getAllByRole("switch", { name: "Create more" })).toHaveLength(1);
    expect(screen.getByText("Create more")).toHaveAttribute("aria-hidden", "true");
  });

  it("keeps the dialog open, clears the form, keeps the project and refocuses", async () => {
    const user = userEvent.setup();
    const h = setup({}, { initialProjectId: PROJECT_BEACON });
    await user.click(screen.getByRole("switch", { name: "Create more" }));
    await user.type(titleField(), "First item");
    save();
    await waitFor(() => expect(posts(h.sent)).toHaveLength(1));
    await waitFor(() => expect(titleField()).toHaveValue(""));
    expect(h.onOpenChange).not.toHaveBeenCalled();
    expect(screen.getByText("Beacon")).toBeInTheDocument();
    await waitFor(() => expect(titleField()).toHaveFocus());
    await user.type(titleField(), "Second item");
    save();
    await waitFor(() => expect(posts(h.sent)).toHaveLength(2));
    expect(posts(h.sent)[1]).toEqual({ name: "Second item" });
  });

  it("clears the description editor between items", async () => {
    const user = userEvent.setup();
    const h = setup();
    await user.click(screen.getByRole("switch", { name: "Create more" }));
    await user.type(titleField(), "First item");
    await typeDescription(user, "Words for the first");
    save();
    await waitFor(() => expect(posts(h.sent)).toHaveLength(1));
    const description = await screen.findByRole("textbox", { name: "Description" });
    await waitFor(() => expect(description).toHaveValue(""));
    await user.type(titleField(), "Second item");
    save();
    await waitFor(() => expect(posts(h.sent)).toHaveLength(2));
    expect(posts(h.sent)[1]).toEqual({ name: "Second item" });
  });
});

describe("WorkItemCreateDialog discard guard (ISS-120)", () => {
  it("closes silently when the form holds nothing", async () => {
    const h = setup();
    fireEvent.click(screen.getByRole("button", { name: "Discard" }));
    expect(h.onOpenChange).toHaveBeenCalledWith(false);
    expect(screen.queryByText("Save this draft?")).not.toBeInTheDocument();
  });

  it("asks before discarding a dirty form and saves the sanitized draft", async () => {
    const user = userEvent.setup();
    const h = setup();
    await user.type(titleField(), "Draft me");
    fireEvent.click(screen.getByRole("button", { name: "Discard" }));
    expect(await screen.findByText("Save this draft?")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Save to Drafts" }));
    await waitFor(() =>
      expect(posts(h.sent)[0]).toEqual({ name: "Draft me", project_id: PROJECT_ATLAS, description_html: "" })
    );
    await waitFor(() => expect(h.onOpenChange).toHaveBeenCalledWith(false));
  });

  it("discards without saving from the prompt", async () => {
    const user = userEvent.setup();
    const h = setup();
    await user.type(titleField(), "Throw me away");
    fireEvent.click(screen.getByRole("button", { name: "Discard" }));
    await screen.findByText("Save this draft?");
    const prompt = screen.getByRole("dialog", { name: "Save this draft?" });
    await user.click(within(prompt).getByRole("button", { name: "Discard" }));
    expect(h.onOpenChange).toHaveBeenCalledWith(false);
    expect(h.sent).toHaveLength(0);
  });

  it("forgets the kept title when the prompt discards", async () => {
    const user = userEvent.setup();
    const h = setup();
    await user.type(titleField(), "Gone for good");
    expect(loadWorkItemDraft(SLUG)?.title).toBe("Gone for good");
    fireEvent.click(screen.getByRole("button", { name: "Discard" }));
    const prompt = await screen.findByRole("dialog", { name: "Save this draft?" });
    await user.click(within(prompt).getByRole("button", { name: "Discard" }));
    // Discard means discard: the next open must start blank rather than
    // restore the title the user just threw away (ISS-120).
    expect(loadWorkItemDraft(SLUG)).toBeNull();
    expect(h.onOpenChange).toHaveBeenCalledWith(false);
  });

  it("also forgets the kept title when Escape is confirmed as discard", async () => {
    const user = userEvent.setup();
    setup();
    await user.type(titleField(), "Escape me");
    fireEvent.keyDown(document.activeElement ?? document.body, { key: "Escape" });
    const prompt = await screen.findByRole("dialog", { name: "Save this draft?" });
    await user.click(within(prompt).getByRole("button", { name: "Discard" }));
    expect(loadWorkItemDraft(SLUG)).toBeNull();
  });

  it("refuses to close while a description upload is still running", async () => {
    const user = userEvent.setup();
    const h = setup({ uploadDelayMs: 300 });
    await user.type(titleField(), "Busy body");
    fireEvent.change(screen.getByLabelText("Attach an image to the description"), {
      target: { files: [new File(["b"], "shot.png", { type: "image/png" })] },
    });
    fireEvent.click(screen.getByRole("button", { name: "Discard" }));
    // The panel's own upload sits outside the dialog's write state; the
    // close is refused and nothing is dropped (ISS-125).
    expect(notifyError).toHaveBeenCalledWith("Couldn't close the form", expect.stringMatching(/still processing/));
    expect(h.onOpenChange).not.toHaveBeenCalled();
    expect(screen.queryByText("Save this draft?")).not.toBeInTheDocument();
    // Once the upload settles the same click closes as usual.
    await waitFor(() => expect(screen.getByLabelText("Attach an image to the description")).not.toBeDisabled(), {
      timeout: 3000,
    });
    fireEvent.click(screen.getByRole("button", { name: "Discard" }));
    expect(await screen.findByText("Save this draft?")).toBeInTheDocument();
  });

  it("refuses Escape too while a description upload is still running", async () => {
    const user = userEvent.setup();
    const h = setup({ uploadDelayMs: 300 });
    await user.type(titleField(), "Busy escape");
    fireEvent.change(screen.getByLabelText("Attach an image to the description"), {
      target: { files: [new File(["b"], "shot.png", { type: "image/png" })] },
    });
    fireEvent.keyDown(document.activeElement ?? document.body, { key: "Escape" });
    expect(notifyError).toHaveBeenCalledWith("Couldn't close the form", expect.any(String));
    expect(h.onOpenChange).not.toHaveBeenCalled();
    expect(screen.queryByText("Save this draft?")).not.toBeInTheDocument();
    expect(await screen.findByRole("textbox", { name: "Description" })).toBeInTheDocument();
  });

  it("returns to the form on Cancel and keeps the values", async () => {
    const user = userEvent.setup();
    const h = setup();
    await user.type(titleField(), "Keep me");
    fireEvent.click(screen.getByRole("button", { name: "Discard" }));
    const prompt = await screen.findByRole("dialog", { name: "Save this draft?" });
    await user.click(within(prompt).getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(screen.queryByText("Save this draft?")).not.toBeInTheDocument());
    expect(h.onOpenChange).not.toHaveBeenCalled();
    expect(titleField()).toHaveValue("Keep me");
  });

  it("keeps the prompt open and explains a failed draft write", async () => {
    const user = userEvent.setup();
    setup({ draftStatus: 500 });
    await user.type(titleField(), "Draft me");
    fireEvent.click(screen.getByRole("button", { name: "Discard" }));
    await user.click(await screen.findByRole("button", { name: "Save to Drafts" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(/didn't save/);
    expect(screen.getByText("Save this draft?")).toBeInTheDocument();
  });

  it("links a state-bound draft to the project so it can be promoted", async () => {
    // The server validates the state and parent against the project in
    // the request body and stores it on the draft; without it a draft
    // holding a state fails to save (ISS-120, ISS-138/ISS-139).
    const user = userEvent.setup();
    const h = setup({}, { initialProjectId: PROJECT_BEACON });
    await user.type(titleField(), "State draft");
    await user.click(screen.getByRole("button", { name: "Select state" }));
    await user.click(await screen.findByRole("option", { name: "Todo" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "Todo" })).toBeInTheDocument());
    await user.click(screen.getByRole("button", { name: "Save to Drafts" }));
    await waitFor(() => expect(posts(h.sent)).toHaveLength(1));
    expect(posts(h.sent)[0]).toEqual({
      name: "State draft",
      project_id: PROJECT_BEACON,
      description_html: "",
      state_id: STATE_ID,
    });
  });

  it("saves a draft inline from the footer", async () => {
    const user = userEvent.setup();
    const h = setup();
    await user.type(titleField(), "Draft inline");
    await user.click(screen.getByRole("button", { name: "Save to Drafts" }));
    await waitFor(() => expect(h.onOpenChange).toHaveBeenCalledWith(false));
    expect(posts(h.sent)[0]).toEqual({ name: "Draft inline", project_id: PROJECT_ATLAS, description_html: "" });
  });

  it("toasts when the inline draft fails and keeps the form", async () => {
    const user = userEvent.setup();
    const h = setup({ draftStatus: 500 });
    await user.type(titleField(), "Draft inline");
    await user.click(screen.getByRole("button", { name: "Save to Drafts" }));
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith("Couldn't save the draft", expect.any(String)));
    expect(h.onOpenChange).not.toHaveBeenCalled();
  });

  it("asks on Escape and on the close control too", async () => {
    const user = userEvent.setup();
    setup();
    await user.type(titleField(), "Dirty");
    fireEvent.keyDown(document.activeElement ?? document.body, { key: "Escape" });
    expect(await screen.findByText("Save this draft?")).toBeInTheDocument();
  });

  it("closes from the close control on a clean form", async () => {
    const h = setup();
    fireEvent.click(screen.getByRole("button", { name: "Close dialog" }));
    expect(h.onOpenChange).toHaveBeenCalledWith(false);
  });
});

describe("WorkItemCreateDialog description content (ISS-127)", () => {
  it("posts an image-only description uploaded into an empty body", async () => {
    const user = userEvent.setup();
    const upload = vi.spyOn(globalThis, "fetch").mockResolvedValue({ ok: true, status: 200 } as Response);
    const h = setup();
    await user.type(titleField(), "Shot");
    fireEvent.change(screen.getByLabelText("Attach an image to the description"), {
      target: { files: [new File(["b"], "shot.png", { type: "image/png" })] },
    });
    await waitFor(() => expect(screen.getByLabelText("Attach an image to the description")).not.toBeDisabled());
    save();
    await waitFor(() => expect(posts(h.sent)).toHaveLength(1));
    expect(posts(h.sent)[0]).toMatchObject({
      name: "Shot",
      description_html: expect.stringContaining("<img"),
    });
    upload.mockRestore();
  });
});

describe("WorkItemCreateDialog duplicate seam (ISS-121)", () => {
  it("shows no duplicate surface in this build", async () => {
    const user = userEvent.setup();
    const h = setup();
    await user.type(titleField(), "Seeded duplicate title");
    await new Promise((resolve) => globalThis.setTimeout(resolve, 50));
    expect(screen.queryByText(/possible duplicate/i)).not.toBeInTheDocument();
    expect(screen.queryByRole("complementary", { name: "Possible duplicates" })).not.toBeInTheDocument();
    expect(h.calls.some((call) => call.url.includes("duplicate"))).toBe(false);
  });

  it("finds the near-duplicate title the cloud leg seeds", async () => {
    edition.flags[ISSUE_DEDUPE_FLAG] = true;
    const user = userEvent.setup();
    setup();
    // The transport seeds "Parent work"; the typed word is a rephrasing of
    // it, which is what §23 expects the cloud leg to surface (ISS-121).
    await user.type(titleField(), "parent");
    expect(await screen.findByRole("button", { name: "1 possible duplicate" }, { timeout: 4000 })).toBeInTheDocument();
    delete edition.flags[ISSUE_DEDUPE_FLAG];
  });

  it("surfaces the panel when the edition flag turns the seam on", async () => {
    edition.flags[ISSUE_DEDUPE_FLAG] = true;
    const user = userEvent.setup();
    const h = setup();
    await user.type(titleField(), "Parent work");
    const count = await screen.findByRole("button", { name: "1 possible duplicate" }, { timeout: 4000 });
    await user.click(count);
    expect(screen.getByRole("complementary", { name: "Possible duplicates" })).toBeInTheDocument();
    expect(h.calls.some((call) => call.url.includes("/search-issues/"))).toBe(true);
    delete edition.flags[ISSUE_DEDUPE_FLAG];
  });
});

describe("WorkItemCreateDialog parent picker (ISS-122)", () => {
  it("searches after the debounce, links results and sets the parent id", async () => {
    const user = userEvent.setup();
    const h = setup();
    await user.click(screen.getByRole("button", { name: "Add parent" }));
    const search = await screen.findByRole("searchbox", { name: "Search work items" });
    await user.type(search, "par");
    await waitFor(() => expect(h.calls.some((call) => call.url.includes("search=par"))).toBe(true), { timeout: 3000 });
    await waitFor(() => expect(screen.getByText("Parent work")).toBeInTheDocument(), { timeout: 3000 });
    const link = screen.getByRole("link", { name: "Open ATL-12 in a new tab" });
    expect(link).toHaveAttribute("href", `/${SLUG}/browse/ATL-12`);
    expect(link).toHaveAttribute("target", "_blank");
    await user.click(screen.getByRole("button", { name: /ATL-12/ }));
    const tag = await screen.findByTestId("parent-tag");
    expect(tag).toHaveTextContent("ATL-12");
    expect(tag).toHaveTextContent("Parent work");
    await user.type(titleField(), "Child item");
    save();
    await waitFor(() => expect(posts(h.sent)).toHaveLength(1));
    expect(posts(h.sent)[0]).toMatchObject({ name: "Child item", parent_id: PARENT_ID });
    const requests = h.calls.filter((call) => call.url.includes("/search-issues/"));
    expect(requests[0]?.url).toContain("parent=true");
    expect(requests.at(-1)?.url).toContain("search=par");
  });

  it("removes a chosen parent", async () => {
    const user = userEvent.setup();
    const h = setup();
    await user.click(screen.getByRole("button", { name: "Add parent" }));
    await user.click(await screen.findByRole("button", { name: /ATL-12/ }));
    await user.click(await screen.findByRole("button", { name: "Remove parent ATL-12" }));
    expect(screen.queryByTestId("parent-tag")).not.toBeInTheDocument();
    await user.type(titleField(), "No parent");
    save();
    await waitFor(() => expect(posts(h.sent)).toHaveLength(1));
    expect(posts(h.sent)[0]).toEqual({ name: "No parent" });
  });
});

describe("WorkItemCreateDialog project selector (ISS-123)", () => {
  it("applies the listing prefill, including the section's cycle membership", async () => {
    const user = userEvent.setup();
    const h = setup(
      {
        projects: {
          [PROJECT_ATLAS]: projectDetail(PROJECT_ATLAS, "Atlas", { cycle_view: true }),
        },
      },
      { initialValues: { priority: "high", cycleId: PARENT_ID } }
    );
    await waitFor(() => expect(screen.getByRole("heading", { name: CREATE_DIALOG_TITLE })).toBeInTheDocument());
    await user.type(titleField(), "From the section");
    save();
    await waitFor(() => expect(posts(h.sent)).toHaveLength(1));
    expect(posts(h.sent)[0]).toMatchObject({ name: "From the section", priority: "high" });
    await waitFor(() =>
      expect(h.calls.some((call) => call.url.endsWith(`/cycles/${PARENT_ID}/cycle-issues/`))).toBe(true)
    );
  });

  it("lists cycles, modules and estimate only when the project enables them", async () => {
    const setupWith = (overrides: Record<string, unknown>) =>
      setup({
        estimateSystem: true,
        projects: {
          [PROJECT_ATLAS]: projectDetail(PROJECT_ATLAS, "Atlas", overrides),
          [PROJECT_BEACON]: projectDetail(PROJECT_BEACON, "Beacon", overrides),
        },
      });
    setupWith({ cycle_view: true, module_view: true, estimate: STATE_ID });
    await waitFor(() => expect(screen.getByRole("button", { name: "Cycle" })).toBeInTheDocument());
    expect(screen.getByRole("button", { name: "Modules" })).toBeInTheDocument();
    expect(screen.getByText("Estimate")).toBeInTheDocument();
  });

  it("names the picked project before the picker's own read resolves", async () => {
    // A cold visit never seeds the picker cache; the dialog already
    // knows the create-permission rows, so the trigger must read the
    // project name rather than "Select project" (ISS-117/ISS-123).
    const h = setup({}, { initialProjectId: PROJECT_BEACON }, { seedPicker: false });
    expect(screen.getByRole("button", { name: "Beacon" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Select project" })).not.toBeInTheDocument();
    expect(h.calls.some((call) => call.url.endsWith("/projects/"))).toBe(false);
  });

  it("hides them for a project without the features", async () => {
    setup();
    await waitFor(() => expect(screen.getByText("Atlas")).toBeInTheDocument());
    expect(screen.queryByText("Cycle")).not.toBeInTheDocument();
    expect(screen.queryByText("Modules")).not.toBeInTheDocument();
    expect(screen.queryByText("Estimate")).not.toBeInTheDocument();
  });

  it("keeps the Atlas target when the picker cannot offer a guest row", async () => {
    const user = userEvent.setup();
    const h = setup();
    await user.type(titleField(), "Stays in Atlas");
    fireEvent.click(screen.getByRole("button", { name: "Atlas" }));
    const list = await screen.findByRole("listbox", { name: "Projects" });
    // The dialog's change handler is the last gate: a guest row can
    // neither be picked nor become the submit target (ISS-123).
    expect(within(list).queryByRole("option", { name: /Guestland/ })).not.toBeInTheDocument();
    await user.keyboard("{Escape}");
    save();
    await waitFor(() => expect(posts(h.sent)).toHaveLength(1));
    expect(h.calls.some((call) => call.url.endsWith(`/projects/${PROJECT_GUEST}/issues/`))).toBe(false);
  });

  it("hides guest and archived projects from the selector", async () => {
    const user = userEvent.setup();
    setup();
    await user.click(screen.getByRole("button", { name: "Atlas" }));
    const list = await screen.findByRole("listbox", { name: "Projects" });
    expect(within(list).getByRole("option", { name: /Beacon/ })).toBeInTheDocument();
    expect(within(list).queryByRole("option", { name: /Guestland/ })).not.toBeInTheDocument();
    expect(within(list).queryByRole("option", { name: /Archive/ })).not.toBeInTheDocument();
  });

  it("resets dependent fields and keeps the title when the project changes", async () => {
    const user = userEvent.setup();
    const h = setup();
    await user.type(titleField(), "Stays");
    await typeDescription(user, "Old project words");
    await user.click(screen.getByRole("button", { name: "Select state" }));
    await user.click(await screen.findByRole("option", { name: "Todo" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "Todo" })).toBeInTheDocument());
    await user.click(screen.getByRole("button", { name: "Atlas" }));
    // Picking the project that is already selected keeps the form as it is.
    await user.click(await screen.findByRole("option", { name: "Atlas" }));
    await waitFor(() => expect(screen.queryByRole("listbox")).not.toBeInTheDocument());
    await user.click(screen.getByRole("button", { name: "Atlas" }));
    await user.click(await screen.findByRole("option", { name: "Beacon" }));
    await waitFor(() => expect(screen.getByText("Beacon")).toBeInTheDocument());
    expect(titleField()).toHaveValue("Stays");
    // The editor reads its content once at mount, so the reset must give
    // it a fresh surface (ISS-123).
    const description = await screen.findByRole("textbox", { name: "Description" });
    await waitFor(() => expect(description).toHaveValue(""));
    expect(screen.getByRole("button", { name: "Select state" })).toBeInTheDocument();
    expect(loadWorkItemDraft(SLUG)?.projectId).toBe(PROJECT_BEACON);
    save();
    await waitFor(() => expect(posts(h.sent)).toHaveLength(1));
    expect(posts(h.sent)[0]).toEqual({ name: "Stays" });
    expect(h.calls.some((call) => call.url.endsWith(`/projects/${PROJECT_BEACON}/issues/`))).toBe(true);
  });
});

describe("WorkItemCreateDialog timeouts", () => {
  it("times out through the shared helper", async () => {
    vi.mocked(generateAssistantTitle).mockImplementation(() =>
      withTimeout(new Promise<{ title: string }>(() => undefined), 10)
    );
    const user = userEvent.setup();
    const h = setup({ assistant: { model_name: "pilot", has_api_key: true } });
    await waitFor(() => expect(h.calls.some((call) => call.url.includes("ai-assistant/config"))).toBe(true));
    await typeDescription(user, "Words");
    save();
    await waitFor(
      () =>
        expect(notifyError).toHaveBeenCalledWith("Couldn't generate a title", expect.stringContaining("took too long")),
      { timeout: 5000 }
    );
    expect(h.sent.filter((entry) => (entry as { name?: string }).name !== undefined)).toHaveLength(0);
  });
});

describe("WorkItemCreateDialog preset parent (ISS-175)", () => {
  const preset = { id: PARENT_ID, name: "Parent work", identifier: "ATL", sequenceId: 12 };

  it("seeds the parent and keeps it after Create more", async () => {
    const user = userEvent.setup();
    const h = setup({}, { initialProjectId: PROJECT_ATLAS, initialParent: preset });
    const tag = await screen.findByTestId("parent-tag");
    expect(tag).toHaveTextContent("ATL-12");
    await user.click(screen.getByRole("switch", { name: "Create more" }));
    await user.type(titleField(), "First child");
    save();
    await waitFor(() => expect(posts(h.sent)).toHaveLength(1));
    expect(posts(h.sent)[0]).toMatchObject({ name: "First child", parent_id: PARENT_ID });
    await waitFor(() => expect(titleField()).toHaveValue(""));
    expect(screen.getByTestId("parent-tag")).toHaveTextContent("ATL-12");
    await user.type(titleField(), "Second child");
    save();
    await waitFor(() => expect(posts(h.sent)).toHaveLength(2));
    expect(posts(h.sent)[1]).toMatchObject({ name: "Second child", parent_id: PARENT_ID });
  });

  it("locks the project and the parent when asked", async () => {
    setup(
      {},
      { initialProjectId: PROJECT_ATLAS, initialParent: preset, lockedProjectId: PROJECT_ATLAS, lockedParent: true }
    );
    expect(screen.getByRole("button", { name: "Atlas" })).toBeDisabled();
    expect(await screen.findByRole("button", { name: "Remove parent ATL-12" })).toBeDisabled();
  });

  it("offers only the locked project", async () => {
    setup({}, { initialProjectId: PROJECT_ATLAS, lockedProjectId: PROJECT_ATLAS });
    expect(screen.getByRole("button", { name: "Atlas" })).toBeDisabled();
    expect(screen.queryByRole("button", { name: "Beacon" })).not.toBeInTheDocument();
  });

  it("clears the form back to no parent on Create more without a preset", async () => {
    const user = userEvent.setup();
    const h = setup({}, { initialProjectId: PROJECT_ATLAS });
    await user.click(screen.getByRole("button", { name: "Add parent" }));
    await user.click(await screen.findByRole("button", { name: /ATL-12/ }));
    expect(screen.getByTestId("parent-tag")).toBeInTheDocument();
    await user.click(screen.getByRole("switch", { name: "Create more" }));
    await user.type(titleField(), "One");
    save();
    await waitFor(() => expect(posts(h.sent)).toHaveLength(1));
    await waitFor(() => expect(titleField()).toHaveValue(""));
    expect(screen.queryByTestId("parent-tag")).not.toBeInTheDocument();
    await user.type(titleField(), "Two");
    save();
    await waitFor(() => expect(posts(h.sent)).toHaveLength(2));
    expect(posts(h.sent)[1]).toEqual({ name: "Two" });
  });
});

const EDIT_ID = "a23e4567-e89b-12d3-a456-42661417400a";
const CYCLE_ID = "b23e4567-e89b-12d3-a456-42661417400b";
const MODULE_ID = "c23e4567-e89b-12d3-a456-42661417400c";

/** A stored row the modal may open for editing. */
function editRow(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    id: EDIT_ID,
    name: "Existing work",
    description_html: "<p>stored words</p>",
    state_id: STATE_ID,
    priority: "medium",
    assignee_ids: [],
    label_ids: [],
    start_date: null,
    target_date: null,
    cycle_id: PARENT_ID,
    module_ids: [PARENT_ID],
    estimate_point: null,
    parent_id: null,
    ...overrides,
  };
}

function editProps(overrides: Record<string, unknown> = {}) {
  const onChanged = vi.fn();
  return {
    onChanged,
    props: {
      edit: {
        kind: "issue" as const,
        id: EDIT_ID,
        projectId: PROJECT_ATLAS,
        snapshot: editRow(overrides),
        onChanged,
      },
    },
  };
}

/** The detail endpoint's full row for an edit (ISS-126). */
function detailRow(descriptionHtml: string, overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    ...editRow({ description_html: descriptionHtml }),
    sort_order: 1,
    completed_at: null,
    sequence_id: 9,
    project_id: PROJECT_ATLAS,
    sub_issues_count: null,
    attachment_count: null,
    link_count: null,
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-02-03T04:05:06Z",
    created_by: null,
    updated_by: null,
    is_draft: false,
    archived_at: null,
    complexity_score: 0,
    agent_executor: null,
    is_synced: false,
    is_subscribed: false,
    agent_ticker: null,
    agent_status: null,
    relations_summary: { blocked_by: [], blocking: [] },
    has_open_blockers: false,
    ...overrides,
  };
}

/** The PATCH bodies this run sent, in order. */
function patches(calls: Array<{ url: string; method: string; body: unknown }>): unknown[] {
  return calls.filter((call) => call.method === "PATCH").map((call) => call.body);
}

describe("WorkItemCreateDialog edit mode (ISS-126)", () => {
  it("opens the stored row with a fixed project and the update heading", async () => {
    const { props } = editProps({ description_html: "" });
    setup(
      {
        issueDetail: detailRow("<p>stored words</p>"),
      },
      props
    );
    expect(screen.getByRole("dialog", { name: "Update work item" })).toBeInTheDocument();
    await waitFor(() => expect(titleField()).toHaveValue("Existing work"));
    // The list snapshot carries no body; the editor shows the detail read,
    // which a title-only save must treat as unchanged (ISS-126).
    await waitFor(() => expect(screen.getByRole("textbox", { name: "Description" })).toHaveValue("stored words"));
    expect(screen.getByRole("button", { name: "Atlas" })).toBeDisabled();
    expect(screen.queryByRole("button", { name: "Save to Drafts" })).not.toBeInTheDocument();
    expect(screen.queryByRole("group", { name: "Create more" })).not.toBeInTheDocument();
  });

  it("sends only the fields that changed", async () => {
    const user = userEvent.setup();
    const { onChanged, props } = editProps({ description_html: "" });
    const h = setup({ issueDetail: detailRow("<p>stored words</p>") }, props);
    await waitFor(() => expect(titleField()).toHaveValue("Existing work"));
    await waitFor(() => expect(screen.getByRole("textbox", { name: "Description" })).toHaveValue("stored words"));
    await user.clear(titleField());
    await user.type(titleField(), "Renamed work");
    save();
    await waitFor(() => expect(patches(h.calls)).toHaveLength(1));
    // The fetched body is not an edit: only the title travels.
    expect(patches(h.calls)[0]).toEqual({ name: "Renamed work" });
    await waitFor(() => expect(h.onOpenChange).toHaveBeenCalledWith(false));
    expect(onChanged).toHaveBeenCalled();
    expect(notifySuccess).toHaveBeenCalledWith("Work item updated successfully");
  });

  it("sends nothing for an untouched form but still closes", async () => {
    const { props } = editProps();
    const h = setup({}, props);
    await waitFor(() => expect(titleField()).toHaveValue("Existing work"));
    save();
    await waitFor(() => expect(h.onOpenChange).toHaveBeenCalledWith(false));
    expect(patches(h.calls)).toEqual([]);
  });

  it("sends an image uploaded into an empty description", async () => {
    const upload = vi.spyOn(globalThis, "fetch").mockResolvedValue({ ok: true, status: 200 } as Response);
    const { props } = editProps({ description_html: "" });
    const h = setup({ issueDetail: detailRow("") }, props);
    await waitFor(() => expect(titleField()).toHaveValue("Existing work"));
    await waitFor(() => expect(screen.getByRole("textbox", { name: "Description" })).toHaveValue(""));
    fireEvent.change(screen.getByLabelText("Attach an image to the description"), {
      target: { files: [new File(["b"], "shot.png", { type: "image/png" })] },
    });
    await waitFor(() => expect(screen.getByLabelText("Attach an image to the description")).not.toBeDisabled());
    save();
    // The upload's completion PATCH carries no body; wait for the issue PATCH.
    const issuePatch = () => h.calls.find((call) => call.method === "PATCH" && /\/issues\/[^/]+\/$/.test(call.url));
    await waitFor(() => expect(issuePatch()).toBeDefined());
    // A picture-only body is a real description; it must not be dropped
    // before the PATCH (ISS-127).
    expect(issuePatch()?.body).toEqual({ description_html: expect.stringContaining("<img") });
    upload.mockRestore();
  });

  it("reconciles a swapped cycle and added module after the write", async () => {
    const user = userEvent.setup();
    const { props } = editProps({ cycle_id: null, module_ids: [] });
    const h = setup(
      { projects: { [PROJECT_ATLAS]: projectDetail(PROJECT_ATLAS, "Atlas", { cycle_view: true, module_view: true }) } },
      props
    );
    await waitFor(() => expect(screen.getByRole("button", { name: "Cycle" })).toBeInTheDocument());
    await user.click(screen.getByRole("button", { name: "Cycle" }));
    await user.click((await screen.findAllByRole("option", { name: "Sprint 4" }))[0]!);
    await user.click(screen.getByRole("button", { name: "Modules" }));
    await user.click((await screen.findAllByRole("option", { name: "Checkout" }))[0]!);
    await user.clear(titleField());
    await user.type(titleField(), "Moved bridges");
    save();
    await waitFor(() => expect(h.onOpenChange).toHaveBeenCalledWith(false));
    expect(patches(h.calls)).toEqual([{ name: "Moved bridges" }]);
    expect(h.calls.some((call) => call.method === "POST" && call.url.includes("/cycle-issues/"))).toBe(true);
    const modules = h.calls.find((call) => call.method === "POST" && call.url.includes(`/issues/${EDIT_ID}/modules/`));
    expect((modules?.body as { modules?: string[] } | undefined)?.modules).toHaveLength(1);
  });

  it("detaches the old cycle and module when they are cleared", async () => {
    const user = userEvent.setup();
    const { props } = editProps();
    const h = setup(
      { projects: { [PROJECT_ATLAS]: projectDetail(PROJECT_ATLAS, "Atlas", { cycle_view: true, module_view: true }) } },
      props
    );
    // The pickers name the stored rows from their cached lists.
    await waitFor(() => expect(screen.getByRole("button", { name: "Sprint 4" })).toBeInTheDocument());
    await user.click(screen.getByRole("button", { name: "Sprint 4" }));
    await user.click(await screen.findByRole("option", { name: "No cycle" }));
    await user.click(screen.getByRole("button", { name: "Checkout" }));
    await user.click((await screen.findAllByRole("option", { name: "Checkout" }))[0]!);
    save();
    await waitFor(() => expect(h.onOpenChange).toHaveBeenCalledWith(false));
    expect(h.calls.some((call) => call.method === "DELETE" && call.url.includes("/cycle-issues/"))).toBe(true);
    // The module clear travels through the same one-call diff: an empty
    // attach list plus the stored id to detach (ISS-124).
    const modules = h.calls.find((call) => call.method === "POST" && call.url.includes(`/issues/${EDIT_ID}/modules/`));
    expect(modules?.body).toEqual({ modules: [], removed_modules: [PARENT_ID] });
  });

  it("closes an edit without the discard prompt", async () => {
    const user = userEvent.setup();
    const { props } = editProps();
    const h = setup({}, props);
    await waitFor(() => expect(titleField()).toHaveValue("Existing work"));
    await user.type(titleField(), " plus words");
    await user.click(screen.getByRole("button", { name: "Discard" }));
    await waitFor(() => expect(h.onOpenChange).toHaveBeenCalledWith(false));
    expect(screen.queryByText("Save this draft?")).not.toBeInTheDocument();
  });

  it("keeps the modal open and toasts when the update fails", async () => {
    const user = userEvent.setup();
    const { onChanged, props } = editProps();
    const h = setup({ editStatus: 500 }, props);
    await waitFor(() => expect(titleField()).toHaveValue("Existing work"));
    await user.type(titleField(), "!");
    save();
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith("Couldn't update the work item", expect.any(String)));
    expect(h.onOpenChange).not.toHaveBeenCalled();
    expect(onChanged).not.toHaveBeenCalled();
  });

  it("opens a draft edit without a project and PATCHes the draft", async () => {
    const user = userEvent.setup();
    const onChanged = vi.fn();
    const h = setup(
      {},
      {
        edit: {
          kind: "draft",
          id: "draft-1",
          projectId: PROJECT_ATLAS,
          snapshot: editRow({ name: "Draft title", description_html: "" }),
          onChanged,
        },
      }
    );
    await waitFor(() => expect(titleField()).toHaveValue("Draft title"));
    await user.clear(titleField());
    await user.type(titleField(), "Draft renamed");
    save();
    await waitFor(() => expect(patches(h.calls)).toHaveLength(1));
    expect(patches(h.calls)[0]).toEqual({ name: "Draft renamed" });
    expect(h.calls.some((call) => call.url.includes("/draft-issues/draft-1/"))).toBe(true);
    expect(h.calls.some((call) => call.url.includes("/cycle-issues/"))).toBe(false);
  });

  it("sends a draft's cycle and module changes on the draft itself (ISS-124/126)", async () => {
    const user = userEvent.setup();
    const h = setup(
      { projects: { [PROJECT_ATLAS]: projectDetail(PROJECT_ATLAS, "Atlas", { cycle_view: true, module_view: true }) } },
      {
        edit: {
          kind: "draft",
          id: "draft-1",
          projectId: PROJECT_ATLAS,
          snapshot: editRow({ name: "Draft title", cycle_id: PARENT_ID, module_ids: [] }),
          onChanged: vi.fn(),
        },
      }
    );
    await waitFor(() => expect(titleField()).toHaveValue("Draft title"));
    await waitFor(() => expect(screen.getByRole("button", { name: "Sprint 4" })).toBeInTheDocument());
    // Clear the cycle and add a module: the draft serializer replaces
    // both sets, so the PATCH carries the complete values.
    await user.click(screen.getByRole("button", { name: "Sprint 4" }));
    await user.click(await screen.findByRole("option", { name: "No cycle" }));
    await user.click(screen.getByRole("button", { name: "Modules" }));
    await user.click((await screen.findAllByRole("option", { name: "Checkout" }))[0]!);
    save();
    await waitFor(() => expect(h.onOpenChange).toHaveBeenCalledWith(false));
    const draftPatch = h.calls.find((call) => call.method === "PATCH" && call.url.includes("/draft-issues/draft-1/"));
    expect(draftPatch?.body).toMatchObject({ cycle_id: null });
    expect((draftPatch?.body as { module_ids?: string[] } | undefined)?.module_ids).toHaveLength(1);
    // Drafts never touch bridge endpoints.
    expect(h.calls.some((call) => call.url.includes("/cycle-issues/"))).toBe(false);
    expect(h.calls.some((call) => call.url.includes(`/issues/${EDIT_ID}/modules/`))).toBe(false);
  });
});

describe("WorkItemCreateDialog context prefill (ISS-124)", () => {
  it("seeds the cycle from the route and links it after create", async () => {
    const user = userEvent.setup();
    const h = setup(
      { projects: { [PROJECT_ATLAS]: projectDetail(PROJECT_ATLAS, "Atlas", { cycle_view: true, module_view: true }) } },
      { context: { cycleId: CYCLE_ID } }
    );
    await user.type(titleField(), "From a cycle");
    // The context surface keeps the membership pickers out of the form.
    expect(screen.queryByText("Cycle")).not.toBeInTheDocument();
    expect(screen.queryByText("Modules")).not.toBeInTheDocument();
    save();
    await waitFor(() => expect(posts(h.sent)).toHaveLength(1));
    expect(posts(h.sent)[0]).toEqual({ name: "From a cycle" });
    await waitFor(() =>
      expect(h.calls.some((call) => call.url.includes(`/cycles/${CYCLE_ID}/cycle-issues/`))).toBe(true)
    );
    expect(h.calls.some((call) => call.url.includes("/modules/"))).toBe(false);
  });

  it("seeds the module from the route and links it after create", async () => {
    const user = userEvent.setup();
    const h = setup(
      { projects: { [PROJECT_ATLAS]: projectDetail(PROJECT_ATLAS, "Atlas", { cycle_view: true, module_view: true }) } },
      { context: { moduleId: MODULE_ID } }
    );
    await user.type(titleField(), "From a module");
    save();
    await waitFor(() => expect(h.onOpenChange).toHaveBeenCalledWith(false));
    const modules = h.calls.find((call) => call.url.includes(`/issues/${CREATED_ID}/modules/`));
    expect(modules?.body).toEqual({ modules: [MODULE_ID] });
    expect(h.calls.some((call) => call.url.includes("/cycle-issues/"))).toBe(false);
  });

  it("lets the route context win over a section prefill of the same membership", async () => {
    // A grouping section pre-fills its own cycle; opening from a cycle
    // page must still land the item on the page's cycle (ISS-011 vs
    // ISS-124), so context overwrites the listing value.
    const user = userEvent.setup();
    const h = setup(
      { projects: { [PROJECT_ATLAS]: projectDetail(PROJECT_ATLAS, "Atlas", { cycle_view: true }) } },
      { context: { cycleId: CYCLE_ID }, initialValues: { priority: "high", cycleId: PARENT_ID } }
    );
    await user.type(titleField(), "Route wins");
    save();
    await waitFor(() => expect(posts(h.sent)).toHaveLength(1));
    expect(posts(h.sent)[0]).toMatchObject({ name: "Route wins", priority: "high" });
    await waitFor(() =>
      expect(h.calls.some((call) => call.url.includes(`/cycles/${CYCLE_ID}/cycle-issues/`))).toBe(true)
    );
    expect(h.calls.some((call) => call.url.includes(`/cycles/${PARENT_ID}/cycle-issues/`))).toBe(false);
  });

  it("ignores a listing prefill when an existing row is edited", async () => {
    // The section prefill belongs to new items only: an edit visit starts
    // from the stored row, so Save must not carry the section's values.
    const { props } = editProps();
    const h = setup({}, { ...props, initialValues: { priority: "urgent", stateId: STATE_ID } });
    await waitFor(() => expect(titleField()).toHaveValue("Existing work"));
    save();
    await waitFor(() => expect(h.onOpenChange).toHaveBeenCalledWith(false));
    expect(patches(h.calls)).toEqual([]);
  });

  it("reopens the project form without context", async () => {
    setup({}, { context: {} });
    await waitFor(() => expect(screen.getByRole("textbox", { name: "Description" })).toBeInTheDocument());
    expect(screen.queryByText("Cycle")).not.toBeInTheDocument();
  });
});

describe("WorkItemCreateDialog context close and Create more (ISS-124/125)", () => {
  it("closes an untouched cycle-context form without offering a draft", async () => {
    const h = setup(
      { projects: { [PROJECT_ATLAS]: projectDetail(PROJECT_ATLAS, "Atlas", { cycle_view: true, module_view: true }) } },
      { context: { cycleId: CYCLE_ID } }
    );
    await waitFor(() => expect(screen.getByRole("textbox", { name: "Description" })).toBeInTheDocument());
    fireEvent.click(screen.getByRole("button", { name: "Discard" }));
    // The cycle came from the route, not the user: nothing to keep.
    expect(h.onOpenChange).toHaveBeenCalledWith(false);
    expect(screen.queryByText("Save this draft?")).not.toBeInTheDocument();
  });

  it("closes an untouched module-context form without offering a draft", async () => {
    const h = setup(
      { projects: { [PROJECT_ATLAS]: projectDetail(PROJECT_ATLAS, "Atlas", { cycle_view: true, module_view: true }) } },
      { context: { moduleId: MODULE_ID } }
    );
    await waitFor(() => expect(screen.getByRole("textbox", { name: "Description" })).toBeInTheDocument());
    fireEvent.keyDown(document.activeElement ?? document.body, { key: "Escape" });
    expect(h.onOpenChange).toHaveBeenCalledWith(false);
    expect(screen.queryByText("Save this draft?")).not.toBeInTheDocument();
  });

  it("still asks once the user types inside a cycle-context form", async () => {
    const user = userEvent.setup();
    setup(
      { projects: { [PROJECT_ATLAS]: projectDetail(PROJECT_ATLAS, "Atlas", { cycle_view: true, module_view: true }) } },
      { context: { cycleId: CYCLE_ID } }
    );
    await user.type(titleField(), "Keep me");
    fireEvent.click(screen.getByRole("button", { name: "Discard" }));
    expect(await screen.findByText("Save this draft?")).toBeInTheDocument();
  });

  it("asks before discarding a cycle the user picked on a project page", async () => {
    const user = userEvent.setup();
    setup(
      { projects: { [PROJECT_ATLAS]: projectDetail(PROJECT_ATLAS, "Atlas", { cycle_view: true, module_view: true }) } },
      { initialProjectId: PROJECT_ATLAS }
    );
    await waitFor(() => expect(screen.getByRole("button", { name: "Cycle" })).toBeInTheDocument());
    await user.click(screen.getByRole("button", { name: "Cycle" }));
    await user.click((await screen.findAllByRole("option", { name: "Sprint 4" }))[0]!);
    fireEvent.click(screen.getByRole("button", { name: "Discard" }));
    expect(await screen.findByText("Save this draft?")).toBeInTheDocument();
  });

  it("asks before discarding a module the user picked on a project page", async () => {
    const user = userEvent.setup();
    setup(
      { projects: { [PROJECT_ATLAS]: projectDetail(PROJECT_ATLAS, "Atlas", { cycle_view: true, module_view: true }) } },
      { initialProjectId: PROJECT_ATLAS }
    );
    await waitFor(() => expect(screen.getByRole("button", { name: "Modules" })).toBeInTheDocument());
    await user.click(screen.getByRole("button", { name: "Modules" }));
    await user.click((await screen.findAllByRole("option", { name: "Checkout" }))[0]!);
    fireEvent.click(screen.getByRole("button", { name: "Discard" }));
    expect(await screen.findByText("Save this draft?")).toBeInTheDocument();
  });

  it("keeps the route cycle for the second item under Create more", async () => {
    const user = userEvent.setup();
    const h = setup(
      { projects: { [PROJECT_ATLAS]: projectDetail(PROJECT_ATLAS, "Atlas", { cycle_view: true, module_view: true }) } },
      { context: { cycleId: CYCLE_ID } }
    );
    await user.click(screen.getByRole("switch", { name: "Create more" }));
    await user.type(titleField(), "First in cycle");
    save();
    await waitFor(() => expect(posts(h.sent)).toHaveLength(1));
    await waitFor(() => expect(titleField()).toHaveValue(""));
    await user.type(titleField(), "Second in cycle");
    save();
    await waitFor(() => expect(posts(h.sent)).toHaveLength(2));
    await waitFor(() =>
      expect(h.calls.filter((call) => call.url.includes(`/cycles/${CYCLE_ID}/cycle-issues/`))).toHaveLength(2)
    );
  });

  it("keeps the route module for the second item under Create more", async () => {
    const user = userEvent.setup();
    const h = setup(
      { projects: { [PROJECT_ATLAS]: projectDetail(PROJECT_ATLAS, "Atlas", { cycle_view: true, module_view: true }) } },
      { context: { moduleId: MODULE_ID } }
    );
    await user.click(screen.getByRole("switch", { name: "Create more" }));
    await user.type(titleField(), "First in module");
    save();
    await waitFor(() => expect(posts(h.sent)).toHaveLength(1));
    await waitFor(() => expect(titleField()).toHaveValue(""));
    await user.type(titleField(), "Second in module");
    save();
    await waitFor(() => expect(posts(h.sent)).toHaveLength(2));
    await waitFor(() =>
      expect(
        h.calls.filter(
          (call) =>
            call.url.includes(`/issues/${CREATED_ID}/modules/`) &&
            (call.body as { modules?: string[] }).modules?.[0] === MODULE_ID
        )
      ).toHaveLength(2)
    );
  });
});

describe("WorkItemCreateDialog advanced git branch (ISS-128)", () => {
  it("starts collapsed and opens on request", async () => {
    const user = userEvent.setup();
    setup();
    expect(screen.queryByLabelText("Work branch")).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Advanced — git" }));
    expect(screen.getByLabelText("Work branch")).toBeInTheDocument();
  });

  it("rejects an illegal branch and keeps the form open", async () => {
    const user = userEvent.setup();
    const h = setup();
    await user.type(titleField(), "Branch guard");
    await user.click(screen.getByRole("button", { name: "Advanced — git" }));
    fireEvent.change(screen.getByLabelText("Work branch"), { target: { value: "bad branch!" } });
    expect(screen.getByText(/Use only letters, numbers/)).toBeInTheDocument();
    save();
    expect(screen.getByText(/Use only letters, numbers/)).toBeInTheDocument();
    expect(h.onOpenChange).not.toHaveBeenCalled();
    expect(posts(h.sent)).toHaveLength(0);
  });

  it("rejects a branch longer than the cap", async () => {
    const user = userEvent.setup();
    const h = setup();
    await user.type(titleField(), "Too long");
    await user.click(screen.getByRole("button", { name: "Advanced — git" }));
    fireEvent.change(screen.getByLabelText("Work branch"), { target: { value: "b".repeat(129) } });
    expect(screen.getByText(/128 characters/)).toBeInTheDocument();
    save();
    expect(posts(h.sent)).toHaveLength(0);
    expect(h.onOpenChange).not.toHaveBeenCalled();
  });

  it("sends a valid branch with the create", async () => {
    const user = userEvent.setup();
    const h = setup();
    await user.type(titleField(), "Branch here");
    await user.click(screen.getByRole("button", { name: "Advanced — git" }));
    fireEvent.change(screen.getByLabelText("Work branch"), { target: { value: "  team/feature-1  " } });
    save();
    await waitFor(() => expect(posts(h.sent)).toHaveLength(1));
    expect(posts(h.sent)[0]).toEqual({ name: "Branch here", git_work_branch: "team/feature-1" });
  });

  it("counts a branch as draft-worthy work", async () => {
    const user = userEvent.setup();
    setup();
    await user.click(screen.getByRole("button", { name: "Advanced — git" }));
    fireEvent.change(screen.getByLabelText("Work branch"), { target: { value: "team/x" } });
    await user.click(screen.getByRole("button", { name: "Discard" }));
    expect(await screen.findByText("Save this draft?")).toBeInTheDocument();
  });
});

describe("WorkItemCreateDialog type seam (ISS-129)", () => {
  afterEach(() => {
    resetIssueTypeExtension();
  });

  it("renders no type, template or extra-property slots in this build", () => {
    setup();
    expect(screen.queryByText("Issue type")).not.toBeInTheDocument();
    expect(screen.queryByText("Template")).not.toBeInTheDocument();
    expect(screen.queryByText("Additional properties")).not.toBeInTheDocument();
  });

  it("renders the registered slots and applies a template prefill", async () => {
    const user = userEvent.setup();
    registerIssueTypeExtension({
      typePicker: () => <span data-testid="cloud-type">Type picker</span>,
      templatePicker: () => <span data-testid="cloud-template">Template picker</span>,
      additionalProperties: () => <span>Additional properties</span>,
    });
    setup();
    expect(screen.getByTestId("cloud-type")).toBeInTheDocument();
    expect(screen.getByTestId("cloud-template")).toBeInTheDocument();
    expect(screen.getByText("Additional properties")).toBeInTheDocument();
    void user;
  });

  it("applies a template's prefill to the form and the editor", async () => {
    function TemplatePicker(props: IssueTypeExtensionProps) {
      return (
        <button
          type="button"
          onClick={() =>
            props.onTemplateApply({
              name: "From template",
              descriptionHtml: "<p>template body</p>",
              priority: "high",
            })
          }
        >
          Template
        </button>
      );
    }
    registerIssueTypeExtension({ templatePicker: TemplatePicker });
    const user = userEvent.setup();
    const h = setup();
    await user.click(screen.getByRole("button", { name: "Template" }));
    await waitFor(() => expect(titleField()).toHaveValue("From template"));
    expect(screen.getByRole("textbox", { name: "Description" })).toHaveValue("template body");
    save();
    await waitFor(() => expect(posts(h.sent)).toHaveLength(1));
    expect(posts(h.sent)[0]).toMatchObject({ name: "From template", priority: "high" });
  });
});

describe("WorkItemCreateDialog publish (ISS-130)", () => {
  it("links images uploaded for a draft to the issue it becomes", async () => {
    const user = userEvent.setup();
    const upload = vi.spyOn(globalThis, "fetch").mockResolvedValue({ ok: true, status: 200 } as Response);
    const h = setup(
      {},
      {
        draftId: "draft-1",
        edit: { kind: "draft", id: "draft-1", projectId: PROJECT_ATLAS, snapshot: editRow({ name: "Picture draft" }) },
      }
    );
    await waitFor(() => expect(titleField()).toHaveValue("Picture draft"));
    fireEvent.change(screen.getByLabelText("Attach an image to the description"), {
      target: { files: [new File(["b"], "shot.png", { type: "image/png" })] },
    });
    await waitFor(() => expect(screen.getByLabelText("Attach an image to the description")).not.toBeDisabled());
    await user.click(screen.getByRole("button", { name: "Publish issue" }));
    await waitFor(() => expect(h.onOpenChange).toHaveBeenCalledWith(false));
    // The upload parked an asset against the draft; publishing links it
    // to the promoted issue so the picture is not orphaned (ISS-127/130).
    const attach = h.calls.find((call) => call.method === "POST" && call.url.endsWith("/bulk/"));
    expect(attach?.url).toContain(CREATED_ID);
    expect(attach?.body).toEqual({ asset_ids: ["asset-1"] });
    upload.mockRestore();
  });

  it("publishes a stored draft into its project", async () => {
    const user = userEvent.setup();
    const h = setup(
      {},
      {
        draftId: "draft-1",
        edit: {
          kind: "draft",
          id: "draft-1",
          projectId: PROJECT_ATLAS,
          snapshot: editRow({ name: "Draft to publish" }),
        },
      }
    );
    await waitFor(() => expect(titleField()).toHaveValue("Draft to publish"));
    await user.click(screen.getByRole("button", { name: "Publish issue" }));
    await waitFor(() => expect(h.onOpenChange).toHaveBeenCalledWith(false));
    expect(notifySuccess).toHaveBeenCalledWith("Draft published to project.", expect.any(String));
    const promote = h.calls.find((call) => call.url.includes("/draft-to-issue/draft-1/"));
    expect(promote?.body).toMatchObject({ name: "Draft to publish", project_id: PROJECT_ATLAS });
    // The draft's cycle and modules travel with the promote; the endpoint
    // files the new issue in them rather than dropping the placement
    // (ISS-130).
    expect(promote?.body).toMatchObject({ cycle_id: PARENT_ID, module_ids: [PARENT_ID] });
  });

  it("refuses to publish a draft with no project", async () => {
    const user = userEvent.setup();
    const onChanged = vi.fn();
    const h = setup(
      {},
      {
        draftId: "draft-1",
        edit: { kind: "draft", id: "draft-1", projectId: null, snapshot: editRow({ name: "Loose draft" }), onChanged },
      },
      { seedPicker: false }
    );
    await waitFor(() => expect(titleField()).toHaveValue("Loose draft"));
    await user.click(screen.getByRole("button", { name: "Publish issue" }));
    await waitFor(() =>
      expect(notifyError).toHaveBeenCalledWith(
        "Couldn't publish the draft",
        "Select a project before publishing this draft."
      )
    );
    expect(h.calls.some((call) => call.url.includes("/draft-to-issue/"))).toBe(false);
    expect(h.onOpenChange).not.toHaveBeenCalled();
  });

  it("reports a failed promote and keeps the form", async () => {
    const user = userEvent.setup();
    const h = setup(
      { publishStatus: 400 },
      {
        draftId: "draft-1",
        edit: { kind: "draft", id: "draft-1", projectId: PROJECT_ATLAS, snapshot: editRow({ name: "Retry" }) },
      }
    );
    await waitFor(() => expect(titleField()).toHaveValue("Retry"));
    await user.click(screen.getByRole("button", { name: "Publish issue" }));
    await waitFor(() =>
      expect(notifyError).toHaveBeenCalledWith(
        "Couldn't publish the draft",
        "Pick a project this draft can move into, then try again."
      )
    );
    expect(h.onOpenChange).not.toHaveBeenCalled();
  });

  it("lets a project-less draft pick a project before publishing", async () => {
    const user = userEvent.setup();
    const h = setup(
      {},
      {
        draftId: "draft-1",
        edit: { kind: "draft", id: "draft-1", projectId: null, snapshot: editRow({ name: "Needs project" }) },
      },
      { seedPicker: false }
    );
    await waitFor(() => expect(titleField()).toHaveValue("Needs project"));
    await user.click(screen.getByRole("button", { name: "Select project" }));
    await new Promise((resolve) => globalThis.setTimeout(resolve, 200));
    await user.click(await screen.findByRole("option", { name: /Beacon/ }));
    await user.click(screen.getByRole("button", { name: "Publish issue" }));
    await waitFor(() => expect(h.onOpenChange).toHaveBeenCalledWith(false));
    // The promote endpoint files under the draft's stored project, so the
    // picked one must be written to the draft first (ISS-130).
    const stored = h.calls.find((call) => call.method === "PATCH" && call.url.includes("/draft-issues/draft-1/"));
    expect(stored?.body).toEqual({ project_id: PROJECT_BEACON });
    const storedIndex = h.calls.indexOf(stored!);
    const promote = h.calls.find((call) => call.url.includes("/draft-to-issue/draft-1/"));
    expect(promote?.body).toMatchObject({ name: "Needs project", project_id: PROJECT_BEACON });
    expect(h.calls.indexOf(promote!)).toBeGreaterThan(storedIndex);
    expect(notifySuccess).toHaveBeenCalledWith("Draft published to project.", expect.any(String));
  });

  it("keeps the form and explains when the draft's project cannot be stored", async () => {
    const user = userEvent.setup();
    const h = setup(
      { editStatus: 500 },
      {
        draftId: "draft-1",
        edit: { kind: "draft", id: "draft-1", projectId: null, snapshot: editRow({ name: "Needs project" }) },
      },
      { seedPicker: false }
    );
    await waitFor(() => expect(titleField()).toHaveValue("Needs project"));
    await user.click(screen.getByRole("button", { name: "Select project" }));
    await user.click(await screen.findByRole("option", { name: /Beacon/ }));
    await user.click(screen.getByRole("button", { name: "Publish issue" }));
    await waitFor(() =>
      expect(notifyError).toHaveBeenCalledWith(
        "Couldn't publish the draft",
        "Pick a project this draft can move into, then try again."
      )
    );
    expect(h.calls.some((call) => call.url.includes("/draft-to-issue/"))).toBe(false);
    expect(h.onOpenChange).not.toHaveBeenCalled();
  });

  it("stores a picked project when a project-less draft is saved, not just published (ISS-126/130)", async () => {
    const user = userEvent.setup();
    const h = setup(
      {},
      {
        draftId: "draft-1",
        edit: { kind: "draft", id: "draft-1", projectId: null, snapshot: editRow({ name: "Needs project" }) },
      },
      { seedPicker: false }
    );
    await waitFor(() => expect(titleField()).toHaveValue("Needs project"));
    await user.click(screen.getByRole("button", { name: "Select project" }));
    await new Promise((resolve) => globalThis.setTimeout(resolve, 200));
    await user.click(await screen.findByRole("option", { name: /Beacon/ }));
    save();
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith("Work item updated successfully"));
    const stored = h.calls.find((call) => call.method === "PATCH" && call.url.includes("/draft-issues/draft-1/"));
    expect(stored?.body).toEqual({ project_id: PROJECT_BEACON });
    expect(h.onOpenChange).toHaveBeenCalledWith(false);
  });
});

describe("WorkItemCreateDialog tab order (ISS-125)", () => {
  /** The oracle's reading: every [tabindex] in the modal, in DOM order. */
  function modalTabOrder(): string[] {
    const scope = screen.getByRole("dialog", { name: /work item/ });
    return Array.from(scope.querySelectorAll("[tabindex]")).map((element) => {
      const labelled = element.getAttribute("placeholder") ?? element.getAttribute("id") ?? element.textContent ?? "";
      return `${element.tagName.toLowerCase()}#${element.getAttribute("tabindex")}:${labelled.replace(/\s+/g, " ").trim().slice(0, 40)}`;
    });
  }

  it("numbers the title, description, strip and footer in the old order", async () => {
    setup(
      { projects: { [PROJECT_ATLAS]: projectDetail(PROJECT_ATLAS, "Atlas", { cycle_view: true, module_view: true }) } },
      {}
    );
    await waitFor(() => expect(screen.getByRole("button", { name: "Cycle" })).toBeInTheDocument());
    expect(screen.getByRole("textbox", { name: "Description" })).toHaveAttribute("tabindex", "2");
    const order = modalTabOrder();
    expect(order).toContain("input#1:Title");
    expect(order.some((entry) => entry.startsWith("button#4:"))).toBe(true);
    expect(order.some((entry) => entry.startsWith("button#5:"))).toBe(true);
    expect(order.some((entry) => entry.startsWith("button#6:"))).toBe(true);
    expect(order).toContain("div#7:Labels");
    expect(order).toContain("div#8:Start date");
    expect(order).toContain("div#9:Due date");
    expect(order).toContain("button#10:Cycle");
    expect(order).toContain("button#11:Modules");
    expect(order).toContain("div#15:Discard");
    expect(order).toContain("div#16:Save");
  });

  it("hides the cycle and module stops on a project without those views", async () => {
    setup();
    await waitFor(() => expect(screen.getByText("State")).toBeInTheDocument());
    const order = modalTabOrder();
    expect(order.some((entry) => entry.includes("#10:"))).toBe(false);
    expect(order.some((entry) => entry.includes("#11:"))).toBe(false);
  });
});
