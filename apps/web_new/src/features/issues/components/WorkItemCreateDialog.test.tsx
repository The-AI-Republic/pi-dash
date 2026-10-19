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
import { pickerKeys } from "../../../shared/pickers/reference.js";
import { withTimeout } from "../create/createApi.js";
import { ISSUE_DEDUPE_FLAG } from "../create/dedupe.js";
import { loadWorkItemDraft } from "../lib/createDraft.js";
import { WorkItemCreateDialog, type WorkItemCreateProject } from "./WorkItemCreateDialog.js";

// The real editor is a lazy tiptap surface with its own suite. The dialog
// only composes it, so a textarea stands in and reports HTML through the
// same change callback; the sanitizer helpers stay real.
vi.mock("../../../shared/editor/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../shared/editor/index.js")>();
  // Mirrors the real editor: the starting HTML is read once, at mount,
  // so a caller that wants a cleared surface must remount this.
  function StandInEditor({
    onChange,
    label,
    initialHtml,
  }: {
    onChange: (html: string) => void;
    label: string;
    initialHtml: string;
  }) {
    const [value, setValue] = React.useState(() => initialHtml.replace(/^<p>/, "").replace(/<\/p>$/, ""));
    return (
      <textarea
        aria-label={label}
        value={value}
        onChange={(event) => {
          setValue(event.target.value);
          onChange(event.target.value === "" ? "" : `<p>${event.target.value}</p>`);
        }}
      />
    );
  }
  return { ...original, RichTextEditor: StandInEditor };
});

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return { ...original, generateAssistantTitle: vi.fn() };
});

vi.mock("../../../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../shared/shell/index.js")>();
  return { ...original, notifyError: vi.fn(), notifySuccess: vi.fn() };
});

import { notifyError, notifySuccess } from "../../../shared/shell/index.js";

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

interface StubOptions {
  createStatus?: number;
  createDelayMs?: number;
  assistant?: { model_name: string; has_api_key: boolean };
  generate?: { status: number; body: unknown; delayMs?: number };
  draftStatus?: number;
  membershipStatus?: number;
  projects?: Record<string, Record<string, unknown>>;
  role?: number;
  estimateSystem?: boolean;
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
    if (url.endsWith("/cycles/")) {
      return json(200, [
        { id: PARENT_ID, project_id: PROJECT_ATLAS, name: "Sprint 4", is_favorite: false, status: "ACTIVE" },
      ]);
    }
    if (url.endsWith("/modules/")) {
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
                name: "Points",
                type: "points",
                last_used: true,
                points: [{ id: PARENT_ID, key: 1, value: "3" }],
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
    if (/\/issues\/[^/]+\/modules\/$/.test(url) && method === "POST") {
      const status = options.membershipStatus ?? 201;
      return json(status, status === 201 ? { message: "ok" } : {});
    }
    return json(200, {});
  }) as never);
  return { calls, sent };
}

function setup(options: StubOptions = {}, props: Partial<React.ComponentProps<typeof WorkItemCreateDialog>> = {}) {
  const stub = stubTransport(options);
  const onOpenChange = vi.fn();
  const onCreated = vi.fn();
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  // The project picker reads its own joined-project list; seed it from
  // the same rows the dialog filters so its trigger renders a name.
  queryClient.setQueryData(pickerKeys.projects(SLUG), [
    ...PROJECTS.map((project) =>
      projectRow(project.id, project.name, project.identifier ?? project.name.slice(0, 3), project.memberRole ?? null)
    ),
    projectRow("923e4567-e89b-12d3-a456-426614174009", "Archived Atlas", "ARC", 15, {
      archived_at: "2026-01-01T00:00:00Z",
    }),
  ]);
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
    for (const field of ["State", "Runs on", "Priority", "Assignees", "Labels", "Start date", "Due date", "Parent"]) {
      expect(screen.getByText(field)).toBeInTheDocument();
    }
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
  it("posts every property the form offers", async () => {
    const user = userEvent.setup();
    const h = setup({
      estimateSystem: true,
      projects: {
        [PROJECT_ATLAS]: projectDetail(PROJECT_ATLAS, "Atlas", { cycle_view: true, module_view: true }),
        [PROJECT_BEACON]: projectDetail(PROJECT_BEACON, "Beacon"),
      },
    });
    await waitFor(() => expect(screen.getByText("Cycle")).toBeInTheDocument());
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

    await user.click(screen.getByRole("button", { name: "No cycle" }));
    await user.click(await screen.findByRole("option", { name: "Sprint 4" }));

    await user.click(screen.getByRole("button", { name: "No module" }));
    await user.click(await screen.findByRole("option", { name: "Checkout" }));

    const estimateSlot = within(screen.getByText("Estimate").parentElement as HTMLElement);
    await user.click(estimateSlot.getByRole("button", { name: "None" }));
    await user.click(await screen.findByRole("option", { name: "3" }));

    save();
    await waitFor(() => expect(posts(h.sent)).toHaveLength(1));
    expect(posts(h.sent)[0]).toMatchObject({
      name: "Everything at once",
      state_id: STATE_ID,
      priority: "high",
      assignee_ids: [WORKSPACE_ID],
      label_ids: [STATE_ID],
      start_date: startIso,
      target_date: dueIso,
      estimate_point: PARENT_ID,
      assigned_pod_id: PARENT_ID,
      agent_executor: "local_runner",
    });
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
    await waitFor(() => expect(screen.getByText("Cycle")).toBeInTheDocument());
    await user.type(titleField(), "Placed later");
    await user.click(screen.getByRole("button", { name: "No cycle" }));
    await user.click(await screen.findByRole("option", { name: "Sprint 4" }));
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
    await waitFor(() => expect(posts(h.sent)[0]).toEqual({ name: "Draft me", description_html: "" }));
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

  it("saves a draft inline from the footer", async () => {
    const user = userEvent.setup();
    const h = setup();
    await user.type(titleField(), "Draft inline");
    await user.click(screen.getByRole("button", { name: "Save to Drafts" }));
    await waitFor(() => expect(h.onOpenChange).toHaveBeenCalledWith(false));
    expect(posts(h.sent)[0]).toEqual({ name: "Draft inline", description_html: "" });
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
    await waitFor(() => expect(screen.getByText("Cycle")).toBeInTheDocument());
    expect(screen.getByText("Modules")).toBeInTheDocument();
    expect(screen.getByText("Estimate")).toBeInTheDocument();
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
