// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Contract unit tests: each contract parses a representative payload through
// a stub transport. Live-server proof lives in contract/*.contract.test.ts.
import { describe, expect, it } from "vitest";
import { ApiError } from "./errors.js";
import { createClient } from "./client.js";
import { getCsrfToken, signIn } from "./contracts/auth.js";
import { getMe, getMeSettings } from "./contracts/users.js";
import { listWorkspaces } from "./contracts/workspaces.js";
import { listProjects } from "./contracts/projects.js";
import { listLabels, listMembers, listStates } from "./contracts/reference.js";
import { getIssue, listIssues } from "./contracts/issues.js";
import { fakeResponse, recordingTransport } from "./client.test.js";

const UUID = "123e4567-e89b-12d3-a456-426614174000";
const UUID2 = "223e4567-e89b-12d3-a456-426614174001";

function json(value: unknown) {
  return recordingTransport(() => fakeResponse({ body: JSON.stringify(value), url: "https://api.test/" }));
}

describe("auth contracts", () => {
  it("parses the CSRF response", async () => {
    const client = createClient({ baseUrl: "https://api.test", transport: json({ csrf_token: "tok" }) });
    await expect(getCsrfToken(client)).resolves.toEqual({ csrf_token: "tok" });
  });

  it("rejects a CSRF response without a token", async () => {
    const client = createClient({ baseUrl: "https://api.test", transport: json({}) });
    const error = await getCsrfToken(client).catch((e: unknown) => e);
    expect(error).toBeInstanceOf(ApiError);
    expect((error as ApiError).code).toBe("contract");
  });

  it("reads sign-in success from the landing URL", async () => {
    const transport = recordingTransport((call) =>
      call.url.endsWith("/auth/get-csrf-token/")
        ? fakeResponse({ body: JSON.stringify({ csrf_token: "csrf" }) })
        : fakeResponse({ status: 200, body: "<html>", url: "https://app.test/" })
    );
    const client = createClient({ baseUrl: "https://api.test", transport });
    const result = await signIn(client, { email: "a@b.c", password: "secret" });
    expect(result).toEqual({ ok: true, location: "https://app.test/" });
    const signInCall = transport.calls.find((call) => call.url.endsWith("/auth/sign-in/"));
    expect(signInCall?.init.headers["Content-Type"]).toBe("application/x-www-form-urlencoded");
    expect(signInCall?.init.headers["X-CSRFTOKEN"]).toBeDefined();
  });

  it("reads sign-in failure from error query params", async () => {
    const transport = recordingTransport((call) =>
      call.url.endsWith("/auth/get-csrf-token/")
        ? fakeResponse({ body: JSON.stringify({ csrf_token: "csrf" }) })
        : fakeResponse({
            status: 200,
            body: "<html>",
            url: "https://app.test/sign-in?error_code=USER_DOES_NOT_EXIST&error_message=USER_DOES_NOT_EXIST",
          })
    );
    const client = createClient({ baseUrl: "https://api.test", transport });
    const result = await signIn(client, { email: "a@b.c", password: "nope" });
    expect(result).toEqual({
      ok: false,
      location: "https://app.test/sign-in?error_code=USER_DOES_NOT_EXIST&error_message=USER_DOES_NOT_EXIST",
      code: "USER_DOES_NOT_EXIST",
      message: "USER_DOES_NOT_EXIST",
    });
  });
});

describe("users contracts", () => {
  const me = {
    id: UUID,
    email: "ada@example.com",
    username: "ada",
    first_name: "Ada",
    last_name: "Lovelace",
    display_name: "Ada Lovelace",
    avatar: "",
    avatar_url: null,
    cover_image: null,
    cover_image_url: null,
    date_joined: "2024-01-02T03:04:05Z",
    user_timezone: "UTC",
    is_active: true,
    is_bot: false,
    is_email_verified: true,
    is_password_autoset: false,
    last_login_medium: "email",
    last_login_time: null,
    extra_future_field: "ignored",
  };

  it("parses me and tolerates unknown fields", async () => {
    const client = createClient({ baseUrl: "https://api.test", transport: json(me) });
    const parsed = await getMe(client);
    expect(parsed.email).toBe("ada@example.com");
    expect(parsed).not.toHaveProperty("extra_future_field");
  });

  it("rejects me without an id", async () => {
    const client = createClient({ baseUrl: "https://api.test", transport: json({ ...me, id: "nope" }) });
    const error = await getMe(client).catch((e: unknown) => e);
    expect((error as ApiError).code).toBe("contract");
  });

  it("parses me settings with workspace pointers", async () => {
    const client = createClient({
      baseUrl: "https://api.test",
      transport: json({
        id: UUID,
        email: "ada@example.com",
        workspace: {
          last_workspace_id: UUID,
          last_workspace_slug: "acme",
          fallback_workspace_id: UUID,
          fallback_workspace_slug: "acme",
          invites: 0,
        },
      }),
    });
    const parsed = await getMeSettings(client);
    expect(parsed.workspace.invites).toBe(0);
  });
});

describe("workspace contracts", () => {
  it("parses the workspace list", async () => {
    const client = createClient({
      baseUrl: "https://api.test",
      transport: json([{ id: UUID, name: "Acme", slug: "acme", logo_url: "", total_members: 3, role: 20 }]),
    });
    const parsed = await listWorkspaces(client);
    expect(parsed).toHaveLength(1);
    expect(parsed[0]?.role).toBe(20);
  });
});

describe("project contracts", () => {
  const project = {
    id: UUID,
    name: "Web",
    identifier: "WEB",
    sort_order: 1,
    logo_props: {},
    member_role: 20,
    intake_count: 0,
    archived_at: null,
    workspace: UUID2,
    cycle_view: false,
    issue_views_view: false,
    module_view: false,
    page_view: true,
    inbox_view: false,
    is_default: true,
    guest_view_all_features: false,
    project_lead: null,
    network: 2,
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-02-03T04:05:06Z",
    created_by: null,
    updated_by: null,
  };

  it("parses the project list", async () => {
    const client = createClient({ baseUrl: "https://api.test", transport: json([project]) });
    const parsed = await listProjects(client, "acme");
    expect(parsed[0]?.identifier).toBe("WEB");
    expect(parsed[0]?.member_role).toBe(20);
  });

  it("hits the workspace-scoped URL", async () => {
    const transport = recordingTransport(() => fakeResponse({ body: "[]" }));
    const client = createClient({ baseUrl: "https://api.test", transport });
    await listProjects(client, "acme");
    expect(transport.calls[0]?.url).toBe("https://api.test/api/workspaces/acme/projects/");
  });
});

describe("reference contracts", () => {
  it("parses states", async () => {
    const client = createClient({
      baseUrl: "https://api.test",
      transport: json([
        {
          id: UUID,
          project_id: UUID2,
          workspace_id: UUID2,
          name: "Todo",
          color: "#ff0000",
          group: "unstarted",
          default: true,
          description: "",
          sequence: 65535,
        },
      ]),
    });
    const parsed = await listStates(client, "acme", UUID2);
    expect(parsed[0]?.group).toBe("unstarted");
  });

  it("rejects unknown state groups", async () => {
    const client = createClient({
      baseUrl: "https://api.test",
      transport: json([
        {
          id: UUID,
          project_id: UUID2,
          workspace_id: UUID2,
          name: "X",
          color: "#fff",
          group: "warp",
          default: false,
          description: "",
          sequence: 1,
        },
      ]),
    });
    const error = await listStates(client, "acme", UUID2).catch((e: unknown) => e);
    expect((error as ApiError).code).toBe("contract");
  });

  it("parses labels", async () => {
    const client = createClient({
      baseUrl: "https://api.test",
      transport: json([
        {
          id: UUID,
          project_id: UUID2,
          workspace_id: UUID2,
          name: "bug",
          color: "#e11d48",
          parent: null,
          sort_order: 1,
        },
      ]),
    });
    const parsed = await listLabels(client, "acme", UUID2);
    expect(parsed[0]?.parent).toBeNull();
  });

  it("parses members as role projections", async () => {
    const member = {
      id: UUID,
      role: 20,
      member: UUID,
      project: UUID2,
      original_role: 20,
      created_at: "2024-01-02T03:04:05Z",
    };
    const client = createClient({ baseUrl: "https://api.test", transport: json([member]) });
    const parsed = await listMembers(client, "acme", UUID2);
    expect(parsed[0]?.role).toBe(20);
    expect(parsed[0]?.member).toBe(UUID);
  });
});

describe("issue contracts", () => {
  const issue = {
    id: UUID,
    name: "Fix login",
    state_id: UUID2,
    sort_order: 1.5,
    completed_at: null,
    estimate_point: null,
    priority: "high",
    start_date: null,
    target_date: null,
    sequence_id: 42,
    project_id: UUID2,
    parent_id: null,
    cycle_id: null,
    module_ids: [],
    label_ids: [],
    assignee_ids: [],
    sub_issues_count: 0,
    attachment_count: null,
    link_count: 0,
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-02-03T04:05:06Z",
    created_by: null,
    updated_by: null,
    is_draft: false,
    archived_at: null,
  };

  const issueDetail = {
    ...issue,
    complexity_score: 0,
    agent_executor: null,
    is_synced: false,
    description_html: "<p></p>",
    is_subscribed: false,
    agent_ticker: null,
    agent_status: null,
    relations_summary: { blocked_by: [], blocking: [] },
    has_open_blockers: false,
  };

  const envelope = {
    grouped_by: null,
    sub_grouped_by: null,
    total_count: 1,
    next_cursor: "1:1:0",
    prev_cursor: "1:0:0",
    next_page_results: false,
    prev_page_results: false,
    count: 1,
    total_pages: 1,
    total_results: 1,
    extra_stats: null,
    results: [issue],
  };

  it("parses the ungrouped list envelope", async () => {
    const client = createClient({ baseUrl: "https://api.test", transport: json(envelope) });
    const parsed = await listIssues(client, "acme", UUID2);
    expect(parsed.total_count).toBe(1);
    expect(parsed.results[0]?.sequence_id).toBe(42);
  });

  it("parses issue detail", async () => {
    const client = createClient({ baseUrl: "https://api.test", transport: json(issueDetail) });
    const parsed = await getIssue(client, "acme", UUID2, UUID);
    expect(parsed.name).toBe("Fix login");
    expect(parsed.has_open_blockers).toBe(false);
  });

  it("hits the detail URL", async () => {
    const transport = recordingTransport(() => fakeResponse({ body: JSON.stringify(issueDetail) }));
    const client = createClient({ baseUrl: "https://api.test", transport });
    await getIssue(client, "acme", UUID2, UUID);
    expect(transport.calls[0]?.url).toBe(`https://api.test/api/workspaces/acme/projects/${UUID2}/issues/${UUID}/`);
  });
});
