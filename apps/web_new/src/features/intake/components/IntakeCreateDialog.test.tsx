// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Intake create dialog (parity/specs/intake.md INT-013–017, 033). Reads
// and writes are stubbed at the transport, the editor is a stand-in
// textarea and uploads resolve through a mocked contract, so each case
// drives the real composition: the defaults, the over-long-only title
// rule, the filed payload, create-another resets, the busy guard on
// close, submit and Escape, and the post-create asset link.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import type { TransportResponse } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { notifyMocks } = vi.hoisted(() => ({
  notifyMocks: { notifyError: vi.fn(), notifySuccess: vi.fn() },
}));

vi.mock("../../../shared/shell/index.js", () => notifyMocks);

const { uploadMocks } = vi.hoisted(() => ({
  uploadMocks: { uploadDescriptionAsset: vi.fn() },
}));

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return { ...original, ...uploadMocks };
});

vi.mock("../../../shared/editor/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../shared/editor/index.js")>();
  // Mirrors the real editor: the starting HTML is read once, at mount,
  // so a caller that wants a cleared surface must remount this.
  function StandInEditor({
    onChange,
    label,
    initialHtml,
    onReady,
  }: {
    onChange: (html: string) => void;
    label: string;
    initialHtml: string;
    onReady?: ((handle: unknown) => void) | undefined;
  }) {
    const [value, setValue] = React.useState(() => initialHtml.replace(/^<p>/, "").replace(/<\/p>$/, ""));
    const handle = React.useMemo(
      () => ({
        insertImage: (src: string) => {
          setValue((current) => {
            const next = `${current}<img src="${src}">`;
            onChange(`<p>${next}</p>`);
            return next;
          });
        },
        insertMention: () => {},
        insertHtml: () => {},
        textBeforeCaret: () => "",
      }),
      [onChange]
    );
    React.useEffect(() => {
      onReady?.(handle);
    }, [onReady, handle]);
    return (
      <textarea
        aria-label={label}
        value={value}
        onChange={(event) => {
          setValue(event.target.value);
          onChange(`<p>${event.target.value}</p>`);
        }}
      />
    );
  }
  return { ...original, IssueDescriptionEditor: StandInEditor };
});

import { formatDayLong } from "../../issues/dropdowns/calendar.js";
import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { setSessionClient } from "../../../core/session/queries.js";
import {
  INTAKE_CREATED_BODY,
  INTAKE_CREATED_TITLE,
  INTAKE_CREATE_DIALOG_TITLE,
  INTAKE_CREATE_FAILED_BODY,
  INTAKE_CREATE_FAILED_TITLE,
  INTAKE_EDITOR_BUSY_BODY,
  INTAKE_EDITOR_BUSY_TITLE,
  IntakeCreateDialog,
} from "./IntakeCreateDialog.js";

const WS = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const ISSUE = "123e4567-e89b-12d3-a456-426614174000";
const INTAKE_ENTRY = "aaaaaaaa-aaaa-4aaa-8aaa-000000000001";
const TRIAGE_ID = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
const ME_ID = "cccccccc-cccc-4ccc-8ccc-cccccccccccc";
const ADA_ID = "dddddddd-dddd-4ddd-8ddd-dddddddddddd";
const LABEL_ID = "eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee";

function jsonResponse(url: string, status: number, body: unknown): TransportResponse {
  return { status, url, headers: { get: () => null }, text: async () => JSON.stringify(body) } as TransportResponse;
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((accept, fail) => {
    resolve = accept;
    reject = fail;
  });
  return { promise, resolve, reject };
}

interface Call {
  method: string;
  url: string;
  body: unknown;
}

interface StubOptions {
  /** Answer the create POST with this status (default 200). */
  createStatus?: number;
  /** Hold the create POST on this gate before answering. */
  holdCreate?: Promise<unknown>;
  /** Answer the triage read with 404 (no triage state yet). */
  triageMissing?: boolean;
  /** Answer the instance read with an LLM-configured instance. */
  instanceLlm?: boolean;
}

function createdDetail(name: string) {
  return {
    id: INTAKE_ENTRY,
    status: -2,
    duplicate_to: null,
    snoozed_till: null,
    duplicate_issue_detail: null,
    source: "IN_APP",
    issue: {
      id: ISSUE,
      name,
      description_html: "",
      priority: "none",
      state_id: TRIAGE_ID,
      sequence_id: 7,
      project_id: PROJECT,
      parent_id: null,
      label_ids: [],
      assignee_ids: [],
      target_date: null,
      start_date: null,
      estimate_point: null,
      sort_order: 1,
      completed_at: null,
      created_at: "2026-10-23T00:00:00Z",
      updated_at: "2026-10-23T00:00:00Z",
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
    },
  };
}

function installServer(options: StubOptions = {}) {
  const calls: Call[] = [];
  const client = createAppClient("https://api.test", (async (url: string, init: { method?: string; body?: string }) => {
    const method = init.method ?? "GET";
    const body: unknown = init.body === undefined ? undefined : JSON.parse(init.body);
    calls.push({ method, url, body });
    const path = new URL(url).pathname;
    if (path.endsWith("/auth/get-csrf-token/")) return jsonResponse(url, 200, { csrf_token: "csrf" });
    if (path === `/api/workspaces/${WS}/projects/${PROJECT}/intake-issues/` && method === "POST") {
      if (options.holdCreate !== undefined) await options.holdCreate;
      const status = options.createStatus ?? 200;
      if (status !== 200) return jsonResponse(url, status, { error: "refused" });
      const name = (body as { issue?: { name?: string } })?.issue?.name ?? "";
      return jsonResponse(url, 200, createdDetail(name));
    }
    if (path.includes("/bulk/") && method === "POST") return jsonResponse(url, 204, "");
    if (path.endsWith("/intake-state/")) {
      if (options.triageMissing === true) return jsonResponse(url, 404, { error: "missing" });
      return jsonResponse(url, 200, {
        id: TRIAGE_ID,
        project_id: PROJECT,
        workspace_id: ME_ID,
        name: "Triage",
        color: "#5e6ad2",
        group: "triage",
        default: false,
        description: "",
        sequence: 1,
      });
    }
    if (path.includes("/workspace-members/me/")) {
      return jsonResponse(url, 200, {
        id: ME_ID,
        workspace: ME_ID,
        member: ME_ID,
        role: 15,
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
    if (path.endsWith("/project-roles/")) return jsonResponse(url, 200, { [PROJECT]: 15 });
    if (path.endsWith(`/projects/${PROJECT}/members/`)) {
      return jsonResponse(url, 200, [
        { id: ME_ID, role: 15, member: ME_ID, project: PROJECT, original_role: 15, created_at: "2026-01-01T00:00:00Z" },
        {
          id: ADA_ID,
          role: 15,
          member: ADA_ID,
          project: PROJECT,
          original_role: 15,
          created_at: "2026-01-01T00:00:00Z",
        },
      ]);
    }
    if (path.endsWith("/members/") && path.includes("/workspaces/")) {
      return jsonResponse(url, 200, [
        {
          id: ME_ID,
          member: {
            id: ME_ID,
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
        {
          id: ADA_ID,
          member: {
            id: ADA_ID,
            first_name: "Ada",
            last_name: "Lovelace",
            avatar: "",
            avatar_url: null,
            display_name: "Ada Lovelace",
            is_bot: false,
          },
          role: 15,
          is_active: true,
        },
      ]);
    }
    if (path.endsWith("/api/users/me/")) {
      return jsonResponse(url, 200, {
        id: ME_ID,
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
    if (path.endsWith("/issue-labels/")) {
      return jsonResponse(url, 200, [
        {
          id: LABEL_ID,
          project_id: PROJECT,
          workspace_id: ME_ID,
          name: "Bug",
          color: "#eb5757",
          parent: null,
          sort_order: 1,
        },
      ]);
    }
    if (path.endsWith("/api/instances/")) {
      if (options.instanceLlm !== true) return jsonResponse(url, 404, { error: "missing" });
      return jsonResponse(url, 200, {
        config: {
          enable_signup: false,
          is_workspace_creation_disabled: false,
          is_google_enabled: false,
          is_github_enabled: false,
          is_gitlab_enabled: false,
          is_gitea_enabled: false,
          is_magic_login_enabled: false,
          is_email_password_enabled: true,
          is_smtp_configured: false,
          is_self_managed: true,
          instance_changelog_url: "",
          has_llm_configured: true,
        },
        instance: {
          id: "33333333-3333-4333-8333-333333333333",
          created_at: "2026-01-01T00:00:00Z",
          updated_at: "2026-01-01T00:00:00Z",
          created_by: null,
          updated_by: null,
          deleted_at: null,
          instance_name: "test",
          whitelist_emails: null,
          instance_id: "test",
          current_version: "1.0.0",
          latest_version: null,
          edition: "oss",
          domain: "test",
          last_checked_at: "2026-01-01T00:00:00Z",
          namespace: null,
          is_telemetry_enabled: false,
          is_support_required: false,
          is_setup_done: true,
          is_signup_screen_visited: true,
          is_verified: false,
          is_test: true,
          is_current_version_deprecated: false,
          workspaces_exist: true,
        },
      });
    }
    // The profile read backs the date picker's week start; a miss falls
    // back to the Sunday default without failing the dialog.
    if (path.endsWith("/api/users/me/profile/")) return jsonResponse(url, 404, { error: "missing" });
    throw new Error(`unexpected request: ${method} ${url}`);
  }) as never);
  setSessionClient(client);
  return calls;
}

function renderDialog(options: StubOptions = {}, props: { open?: boolean } = {}) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const calls = installServer(options);
  const onOpenChange = vi.fn();
  const onCreated = vi.fn();
  render(
    <QueryClientProvider client={client}>
      <IntakeCreateDialog
        workspaceSlug={WS}
        projectId={PROJECT}
        open={props.open ?? true}
        onOpenChange={onOpenChange}
        onCreated={onCreated}
      />
    </QueryClientProvider>
  );
  return { calls, onOpenChange, onCreated };
}

function titleField() {
  return screen.getByTestId("intake-create-title");
}

function descriptionField() {
  return screen.getByRole("textbox", { name: "Description" });
}

function submitButton() {
  return screen.getByTestId("intake-create-submit");
}

function postsTo(calls: Call[], suffix: string) {
  return calls.filter((call) => call.method === "POST" && call.url.includes(suffix));
}

function currentMonthDay15() {
  const now = new Date();
  const iso = `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, "0")}-15`;
  return { iso, label: formatDayLong(iso) };
}

beforeEach(() => {
  vi.clearAllMocks();
  uploadMocks.uploadDescriptionAsset.mockResolvedValue({
    assetId: "asset-1",
    assetUrl: "https://cdn.test/asset-1.png",
  });
});

afterEach(() => {
  resetAppClient();
});

describe("IntakeCreateDialog defaults (INT-013)", () => {
  it("opens blank with the triage fallback uncommitted and create-another off", async () => {
    renderDialog();
    expect(await screen.findByRole("heading", { name: INTAKE_CREATE_DIALOG_TITLE })).toBeInTheDocument();
    expect(titleField()).toHaveValue("");
    expect(descriptionField()).toHaveValue("");
    expect(submitButton()).toBeEnabled();
    within(screen.getByRole("group", { name: "Intake state" })).getByRole("button", { name: "Select intake state" });
    within(screen.getByRole("group", { name: "Priority" })).getByRole("button", { name: "None" });
    within(screen.getByRole("group", { name: "Assignees" })).getByRole("button", { name: "Assignees" });
    expect(screen.getByTestId("label-picker-trigger")).toHaveTextContent("Select labels");
    within(screen.getByRole("group", { name: "Due date" })).getByRole("button", { name: "Due date" });
    expect(screen.getByRole("switch", { name: "Create another" })).toHaveAttribute("aria-checked", "false");
  });

  it("shows the writing helper once the instance configures it", async () => {
    renderDialog({ instanceLlm: true });
    fireEvent.change(titleField(), { target: { value: "Triage me" } });
    expect(await screen.findByRole("button", { name: "I'm feeling lucky" })).toBeInTheDocument();
  });
});

describe("IntakeCreateDialog submit (INT-013/INT-033)", () => {
  it("files the request with every property and opens the new item", async () => {
    const { calls, onOpenChange, onCreated } = renderDialog();
    await screen.findByRole("heading", { name: INTAKE_CREATE_DIALOG_TITLE });
    const day = currentMonthDay15();

    fireEvent.change(titleField(), { target: { value: "Triage me" } });
    fireEvent.change(descriptionField(), { target: { value: "Details" } });

    fireEvent.click(
      within(screen.getByRole("group", { name: "Intake state" })).getByRole("button", { name: "Select intake state" })
    );
    fireEvent.click(await screen.findByRole("option", { name: "Triage" }));

    fireEvent.click(within(screen.getByRole("group", { name: "Priority" })).getByRole("button", { name: "None" }));
    fireEvent.click(await screen.findByRole("option", { name: "High" }));

    fireEvent.click(
      within(screen.getByRole("group", { name: "Assignees" })).getByRole("button", { name: "Assignees" })
    );
    fireEvent.click(await screen.findByRole("option", { name: "Ada Lovelace" }));

    fireEvent.click(screen.getByTestId("label-picker-trigger"));
    const labelOptions = await screen.findAllByTestId("label-picker-option");
    fireEvent.click(labelOptions[0]!);

    fireEvent.click(within(screen.getByRole("group", { name: "Due date" })).getByRole("button", { name: "Due date" }));
    fireEvent.click(await screen.findByRole("button", { name: day.label }));

    fireEvent.click(submitButton());

    await waitFor(() =>
      expect(notifyMocks.notifySuccess).toHaveBeenCalledWith(INTAKE_CREATED_TITLE, INTAKE_CREATED_BODY)
    );
    const posts = postsTo(calls, "/intake-issues/");
    expect(posts).toHaveLength(1);
    expect(posts[0]?.body).toEqual({
      source: "IN_APP",
      issue: {
        name: "Triage me",
        description_html: "<p>Details</p>",
        state_id: TRIAGE_ID,
        priority: "high",
        assignee_ids: [ADA_ID],
        label_ids: [LABEL_ID],
        target_date: day.iso,
      },
    });
    expect(onOpenChange).toHaveBeenCalledWith(false);
    expect(onCreated).toHaveBeenCalledWith(ISSUE);
  });

  it("sends an empty state while unset and omits untouched properties", async () => {
    const { calls } = renderDialog();
    await screen.findByRole("heading", { name: INTAKE_CREATE_DIALOG_TITLE });
    fireEvent.change(titleField(), { target: { value: "Triage me" } });
    fireEvent.click(submitButton());
    await waitFor(() => expect(notifyMocks.notifySuccess).toHaveBeenCalled());
    const posts = postsTo(calls, "/intake-issues/");
    expect(posts).toHaveLength(1);
    expect(posts[0]?.body).toEqual({ source: "IN_APP", issue: { name: "Triage me", state_id: "" } });
  });

  it("ignores a second submit while one is still in flight", async () => {
    const gate = deferred<unknown>();
    const { calls } = renderDialog({ holdCreate: gate.promise });
    await screen.findByRole("heading", { name: INTAKE_CREATE_DIALOG_TITLE });
    fireEvent.change(titleField(), { target: { value: "Triage me" } });
    fireEvent.click(submitButton());
    await waitFor(() => expect(submitButton()).toBeDisabled());
    expect(submitButton()).toHaveTextContent("Creating…");
    fireEvent.submit(screen.getByTestId("intake-create-form"));
    gate.resolve(undefined);
    await waitFor(() => expect(notifyMocks.notifySuccess).toHaveBeenCalled());
    expect(postsTo(calls, "/intake-issues/")).toHaveLength(1);
  });
});

describe("IntakeCreateDialog validation (INT-014)", () => {
  it("blocks an over-long title with a hint and sends nothing", async () => {
    const { calls } = renderDialog();
    await screen.findByRole("heading", { name: INTAKE_CREATE_DIALOG_TITLE });
    fireEvent.change(titleField(), { target: { value: "x".repeat(256) } });
    expect(await screen.findByRole("alert")).toHaveTextContent("Titles stop at 255 characters.");
    expect(submitButton()).toBeDisabled();
    fireEvent.submit(screen.getByTestId("intake-create-form"));
    expect(postsTo(calls, "/intake-issues/")).toHaveLength(0);

    fireEvent.change(titleField(), { target: { value: "Triage me" } });
    await waitFor(() => expect(screen.queryByRole("alert")).toBeNull());
    expect(submitButton()).toBeEnabled();
  });

  it("posts an empty title for the server to refuse and keeps the draft", async () => {
    const { calls, onOpenChange, onCreated } = renderDialog({ createStatus: 400 });
    await screen.findByRole("heading", { name: INTAKE_CREATE_DIALOG_TITLE });
    fireEvent.click(submitButton());
    await waitFor(() =>
      expect(notifyMocks.notifyError).toHaveBeenCalledWith(INTAKE_CREATE_FAILED_TITLE, INTAKE_CREATE_FAILED_BODY)
    );
    const posts = postsTo(calls, "/intake-issues/");
    expect(posts).toHaveLength(1);
    expect(((posts[0] as Call).body as { issue: { name: string } }).issue.name).toBe("");
    expect(onOpenChange).not.toHaveBeenCalled();
    expect(onCreated).not.toHaveBeenCalled();
    expect(screen.getByRole("heading", { name: INTAKE_CREATE_DIALOG_TITLE })).toBeInTheDocument();
  });

  it("stays open with the input intact when the save fails", async () => {
    const { onOpenChange, onCreated } = renderDialog({ createStatus: 500 });
    await screen.findByRole("heading", { name: INTAKE_CREATE_DIALOG_TITLE });
    fireEvent.change(titleField(), { target: { value: "Triage me" } });
    fireEvent.click(submitButton());
    await waitFor(() =>
      expect(notifyMocks.notifyError).toHaveBeenCalledWith(INTAKE_CREATE_FAILED_TITLE, INTAKE_CREATE_FAILED_BODY)
    );
    expect(onOpenChange).not.toHaveBeenCalled();
    expect(onCreated).not.toHaveBeenCalled();
    expect(titleField()).toHaveValue("Triage me");
  });
});

describe("IntakeCreateDialog create-another (INT-015)", () => {
  it("resets and stays open, then files the next request from the cleared form", async () => {
    const { calls, onOpenChange, onCreated } = renderDialog();
    await screen.findByRole("heading", { name: INTAKE_CREATE_DIALOG_TITLE });
    fireEvent.click(screen.getByRole("switch", { name: "Create another" }));
    expect(screen.getByRole("switch", { name: "Create another" })).toHaveAttribute("aria-checked", "true");

    fireEvent.change(titleField(), { target: { value: "First" } });
    fireEvent.change(descriptionField(), { target: { value: "First body" } });
    fireEvent.click(submitButton());
    await waitFor(() => expect(notifyMocks.notifySuccess).toHaveBeenCalledTimes(1));

    expect(onOpenChange).not.toHaveBeenCalled();
    expect(onCreated).not.toHaveBeenCalled();
    expect(screen.getByRole("heading", { name: INTAKE_CREATE_DIALOG_TITLE })).toBeInTheDocument();
    expect(titleField()).toHaveValue("");
    expect(descriptionField()).toHaveValue("");
    within(screen.getByRole("group", { name: "Priority" })).getByRole("button", { name: "None" });

    fireEvent.change(titleField(), { target: { value: "Second" } });
    fireEvent.click(submitButton());
    await waitFor(() => expect(notifyMocks.notifySuccess).toHaveBeenCalledTimes(2));
    const posts = postsTo(calls, "/intake-issues/");
    expect(posts).toHaveLength(2);
    expect(((posts[1] as Call).body as { issue: { name: string } }).issue.name).toBe("Second");
  });
});

describe("IntakeCreateDialog dismiss guard (INT-016)", () => {
  function attachImage() {
    fireEvent.change(screen.getByLabelText("Attach an image to the description"), {
      target: { files: [new File(["img"], "shot.png", { type: "image/png" })] },
    });
  }

  function pressEscape() {
    fireEvent.keyDown(document.activeElement ?? document.body, { key: "Escape" });
  }

  it("blocks close, submit and Escape with a notice while the editor is busy", async () => {
    const gate = deferred<{ assetId: string; assetUrl: string }>();
    uploadMocks.uploadDescriptionAsset.mockReturnValueOnce(gate.promise);
    const { calls, onOpenChange } = renderDialog();
    await screen.findByRole("heading", { name: INTAKE_CREATE_DIALOG_TITLE });
    fireEvent.change(titleField(), { target: { value: "Triage me" } });
    attachImage();
    await waitFor(() => expect(uploadMocks.uploadDescriptionAsset).toHaveBeenCalled());

    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    await waitFor(() =>
      expect(notifyMocks.notifyError).toHaveBeenCalledWith(INTAKE_EDITOR_BUSY_TITLE, INTAKE_EDITOR_BUSY_BODY)
    );
    expect(onOpenChange).not.toHaveBeenCalled();

    fireEvent.click(submitButton());
    expect(postsTo(calls, "/intake-issues/")).toHaveLength(0);

    pressEscape();
    expect(onOpenChange).not.toHaveBeenCalled();
    expect(screen.getByRole("heading", { name: INTAKE_CREATE_DIALOG_TITLE })).toBeInTheDocument();
    expect(titleField()).toHaveValue("Triage me");

    gate.resolve({ assetId: "asset-1", assetUrl: "https://cdn.test/asset-1.png" });
    // The settled upload lands in the editor, which also flushes the
    // readiness report the guard reads.
    await screen.findByDisplayValue(/cdn\.test\/asset-1\.png/);
    fireEvent.click(submitButton());
    await waitFor(() => expect(notifyMocks.notifySuccess).toHaveBeenCalled());
    expect(postsTo(calls, "/intake-issues/")).toHaveLength(1);
  });

  it("dismisses a settled dialog from Cancel and from Escape without asking", async () => {
    const { onOpenChange, onCreated } = renderDialog();
    await screen.findByRole("heading", { name: INTAKE_CREATE_DIALOG_TITLE });
    fireEvent.change(titleField(), { target: { value: "Triage me" } });
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onOpenChange).toHaveBeenCalledWith(false);
    expect(onCreated).not.toHaveBeenCalled();
    expect(notifyMocks.notifyError).not.toHaveBeenCalled();
  });

  it("closes from Escape once settled", async () => {
    const { onOpenChange } = renderDialog();
    await screen.findByRole("heading", { name: INTAKE_CREATE_DIALOG_TITLE });
    pressEscape();
    await waitFor(() => expect(onOpenChange).toHaveBeenCalledWith(false));
  });
});

describe("IntakeCreateDialog attachments (INT-017)", () => {
  it("links composed images to the created work item on save", async () => {
    const { calls } = renderDialog();
    await screen.findByRole("heading", { name: INTAKE_CREATE_DIALOG_TITLE });
    fireEvent.change(titleField(), { target: { value: "Triage me" } });
    fireEvent.change(screen.getByLabelText("Attach an image to the description"), {
      target: { files: [new File(["img"], "shot.png", { type: "image/png" })] },
    });
    await waitFor(() => expect(uploadMocks.uploadDescriptionAsset).toHaveBeenCalled());
    expect(await screen.findByDisplayValue(/<img src="https:\/\/cdn\.test\/asset-1\.png">/)).toBeInTheDocument();

    fireEvent.click(submitButton());
    await waitFor(() => expect(notifyMocks.notifySuccess).toHaveBeenCalled());

    const links = postsTo(calls, "/bulk/");
    expect(links).toHaveLength(1);
    expect(links[0]?.url).toContain(`/projects/${PROJECT}/${ISSUE}/bulk/`);
    expect(links[0]?.url).not.toContain(INTAKE_ENTRY);
    expect(links[0]?.body).toEqual({ asset_ids: ["asset-1"] });
  });

  it("skips the link call when nothing was composed", async () => {
    const { calls } = renderDialog();
    await screen.findByRole("heading", { name: INTAKE_CREATE_DIALOG_TITLE });
    fireEvent.change(titleField(), { target: { value: "Triage me" } });
    fireEvent.click(submitButton());
    await waitFor(() => expect(notifyMocks.notifySuccess).toHaveBeenCalled());
    expect(postsTo(calls, "/bulk/")).toHaveLength(0);
  });
});
