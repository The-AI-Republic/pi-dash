// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the issues detail-widget contracts: every function
// parses a representative payload through a stub transport and sends
// the expected method, path and body. Live-server proof lives in
// contract/issue-*.contract.test.ts.
import { describe, expect, it, vi } from "vitest";
import { createClient } from "../client.js";
import { ApiError } from "../errors.js";
import { fakeResponse, recordingTransport } from "../client.test.js";
import { addSubIssues, listSubIssues } from "./issue-subissues.js";
import { addIssueRelations, listIssueRelations, removeIssueRelation } from "./issue-relations.js";
import { createIssueLink, deleteIssueLink, listIssueLinks, updateIssueLink } from "./issue-links.js";
import {
  completeAttachmentUpload,
  deleteIssueAttachment,
  listIssueAttachments,
  requestAttachmentUpload,
  uploadIssueAttachment,
} from "./issue-attachments.js";
import {
  createDraftIssue,
  deleteDraftIssue,
  getDraftIssue,
  listDraftIssues,
  promoteDraftIssue,
  updateDraftIssue,
} from "./issue-drafts.js";
import { attachCodeReview, detachCodeReview, listCodeReviews } from "./issue-code-reviews.js";
import { getDescriptionVersion, listDescriptionVersions } from "./issue-versions.js";
import { searchIssues } from "./issue-search.js";
import { AgentStatus } from "./issue-agent.js";

const UUID = "123e4567-e89b-12d3-a456-426614174000";
const UUID2 = "223e4567-e89b-12d3-a456-426614174001";
const UUID3 = "323e4567-e89b-12d3-a456-426614174002";
const DT = "2026-10-16T12:00:00.000Z";

function json(value: unknown, status = 200) {
  return recordingTransport((call) =>
    call.url.endsWith("/auth/get-csrf-token/")
      ? fakeResponse({ body: JSON.stringify({ csrf_token: "csrf" }) })
      : fakeResponse({ status, body: JSON.stringify(value) })
  );
}

function clientFor(value: unknown, status = 200) {
  return createClient({ baseUrl: "https://api.test", transport: json(value, status) });
}

const subIssueRow = {
  id: UUID,
  name: "Child",
  state_id: UUID2,
  sort_order: 1,
  completed_at: null,
  priority: "medium",
  sequence_id: 3,
  project_id: UUID3,
  parent_id: UUID2,
  cycle_id: null,
  module_ids: [],
  label_ids: [],
  assignee_ids: [],
  sub_issues_count: 0,
  attachment_count: 0,
  link_count: 0,
  created_at: DT,
  updated_at: DT,
  created_by: UUID,
  updated_by: UUID,
  is_draft: false,
  archived_at: null,
  state_group: "backlog",
};

const addedSubIssueRow = {
  id: UUID,
  name: "Child",
  state_id: UUID2,
  sort_order: 1,
  completed_at: null,
  estimate_point: null,
  priority: "medium",
  complexity_score: 0,
  start_date: null,
  target_date: null,
  sequence_id: 3,
  project_id: UUID3,
  parent_id: UUID2,
  assigned_pod_id: null,
  agent_executor: null,
  created_at: DT,
  updated_at: DT,
  created_by: UUID,
  updated_by: UUID,
  is_draft: false,
  archived_at: null,
  is_synced: false,
};

describe("sub-issue contracts", () => {
  it("lists children with the distribution and threads paging params", async () => {
    const transport = json({ sub_issues: [subIssueRow], state_distribution: { backlog: [UUID] } });
    const client = createClient({ baseUrl: "https://api.test", transport });
    const page = await listSubIssues(client, "acme", UUID3, UUID2, {
      perPage: 50,
      cursor: "50:1:0",
      orderBy: "-created_at",
    });
    expect(page.sub_issues).toHaveLength(1);
    expect(page.state_distribution).toEqual({ backlog: [UUID] });
    const url = new URL(transport.calls[0]?.url ?? "");
    expect(url.pathname).toBe(`/api/workspaces/acme/projects/${UUID3}/issues/${UUID2}/sub-issues/`);
    expect(url.searchParams.get("per_page")).toBe("50");
    expect(url.searchParams.get("cursor")).toBe("50:1:0");
    expect(url.searchParams.get("order_by")).toBe("-created_at");
  });

  it("omits unset paging params from the query", async () => {
    const transport = json({ sub_issues: [], state_distribution: {} });
    await listSubIssues(createClient({ baseUrl: "https://api.test", transport }), "acme", UUID3, UUID2, {});
    const url = new URL(transport.calls[0]?.url ?? "");
    expect(url.search).toBe("");
  });

  it("attaches existing issues and parses the attached rows", async () => {
    const transport = json({ sub_issues: [addedSubIssueRow], state_distribution: { backlog: [UUID] } });
    const client = createClient({ baseUrl: "https://api.test", transport });
    const attached = await addSubIssues(client, "acme", UUID3, UUID2, [UUID]);
    expect(attached.sub_issues.map((row) => row.id)).toEqual([UUID]);
    expect(transport.calls.at(-1)?.init.method).toBe("POST");
    expect(JSON.parse(transport.calls.at(-1)?.init.body ?? "{}")).toEqual({ sub_issue_ids: [UUID] });
  });
});

const relatedRow = {
  id: UUID,
  name: "Related",
  state_id: UUID2,
  sort_order: 1,
  priority: "high",
  sequence_id: 4,
  project_id: UUID3,
  label_ids: [],
  assignee_ids: [],
  created_at: DT,
  updated_at: DT,
  created_by: UUID,
  updated_by: null,
  relation_type: "relates_to",
};

const emptyRelations = {
  blocking: [],
  blocked_by: [],
  duplicate: [],
  relates_to: [],
  start_after: [],
  start_before: [],
  finish_after: [],
  finish_before: [],
};

describe("relation contracts", () => {
  it("lists all eight groups", async () => {
    const client = clientFor({ ...emptyRelations, relates_to: [relatedRow] });
    const groups = await listIssueRelations(client, "acme", UUID3, UUID2);
    expect(groups.relates_to.map((row) => row.id)).toEqual([UUID]);
    expect(groups.blocking).toEqual([]);
  });

  it("adds relations with the type and items", async () => {
    const transport = json([{ ...relatedRow }]);
    const client = createClient({ baseUrl: "https://api.test", transport });
    const added = await addIssueRelations(client, "acme", UUID3, UUID2, {
      relationType: "relates_to",
      issues: [UUID],
    });
    expect(added.map((row) => row.id)).toEqual([UUID]);
    expect(transport.calls.at(-1)?.url.endsWith("/issue-relation/")).toBe(true);
    expect(JSON.parse(transport.calls.at(-1)?.init.body ?? "{}")).toEqual({
      relation_type: "relates_to",
      issues: [UUID],
    });
  });

  it("removes a relation through the remove endpoint", async () => {
    const transport = json(null, 204);
    const client = createClient({ baseUrl: "https://api.test", transport });
    await removeIssueRelation(client, "acme", UUID3, UUID2, {
      relationType: "blocking",
      relatedIssue: UUID,
    });
    expect(transport.calls.at(-1)?.url.endsWith("/remove-relation/")).toBe(true);
    expect(JSON.parse(transport.calls.at(-1)?.init.body ?? "{}")).toEqual({
      relation_type: "blocking",
      related_issue: UUID,
    });
  });
});

const linkAuthor = {
  id: UUID,
  first_name: "Contract",
  last_name: "Tester",
  avatar: "",
  avatar_url: null,
  is_bot: false,
  display_name: "Contract Tester",
};

const linkRow = {
  id: UUID,
  created_at: DT,
  updated_at: DT,
  created_by: UUID,
  updated_by: null,
  deleted_at: null,
  project: UUID3,
  workspace: UUID2,
  title: "Spec",
  url: "https://contract.example/spec",
  issue: UUID2,
  metadata: {},
  created_by_detail: linkAuthor,
};

describe("link contracts", () => {
  it("lists, creates, updates and deletes links", async () => {
    const listed = await listIssueLinks(clientFor([linkRow]), "acme", UUID3, UUID2);
    expect(listed.map((row) => row.id)).toEqual([UUID]);

    const transport = json(linkRow, 201);
    const created = await createIssueLink(
      createClient({ baseUrl: "https://api.test", transport }),
      "acme",
      UUID3,
      UUID2,
      { url: "https://contract.example/spec", title: "Spec" }
    );
    expect(created.url).toBe("https://contract.example/spec");
    expect(JSON.parse(transport.calls.at(-1)?.init.body ?? "{}")).toEqual({
      url: "https://contract.example/spec",
      title: "Spec",
    });

    const renamed = await updateIssueLink(clientFor({ ...linkRow, title: "Spec v2" }), "acme", UUID3, UUID2, UUID, {
      title: "Spec v2",
    });
    expect(renamed.title).toBe("Spec v2");

    const deleter = json(null, 204);
    await deleteIssueLink(
      createClient({ baseUrl: "https://api.test", transport: deleter }),
      "acme",
      UUID3,
      UUID2,
      UUID
    );
    expect(deleter.calls.at(-1)?.init.method).toBe("DELETE");
    expect(deleter.calls.at(-1)?.url.endsWith(`/issue-links/${UUID}/`)).toBe(true);
  });
});

const attachmentRow = {
  id: UUID,
  attributes: { name: "note.png", type: "image/png", size: 1024 },
  asset: "workspace-id/note.png",
  user: null,
  workspace: UUID2,
  draft_issue: null,
  project: UUID3,
  issue: UUID2,
  comment: null,
  page: null,
  entity_type: "ISSUE_ATTACHMENT",
  entity_identifier: null,
  is_deleted: false,
  is_archived: false,
  external_id: null,
  external_source: null,
  size: 1024,
  is_uploaded: true,
  storage_metadata: null,
  created_at: DT,
  updated_at: DT,
  created_by: UUID,
  updated_by: null,
  deleted_at: null,
  asset_url: `/api/assets/v2/workspaces/acme/projects/${UUID3}/issues/${UUID2}/attachments/${UUID}/`,
};

const uploadGrant = {
  upload_data: { url: "https://objects.test/upload", fields: { key: "k", "Content-Type": "image/png" } },
  asset_id: UUID,
  attachment: { ...attachmentRow, is_uploaded: false },
  asset_url: attachmentRow.asset_url,
};

describe("attachment contracts", () => {
  it("lists uploads and drives the grant plus complete legs", async () => {
    const listed = await listIssueAttachments(clientFor([attachmentRow]), "acme", UUID3, UUID2);
    expect(listed.map((row) => row.id)).toEqual([UUID]);

    const transport = json(uploadGrant);
    const grant = await requestAttachmentUpload(
      createClient({ baseUrl: "https://api.test", transport }),
      "acme",
      UUID3,
      UUID2,
      { name: "note.png", type: "image/png", size: 1024 }
    );
    expect(grant.asset_id).toBe(UUID);
    expect(JSON.parse(transport.calls.at(-1)?.init.body ?? "{}")).toEqual({
      name: "note.png",
      type: "image/png",
      size: 1024,
    });

    const completer = json(null, 204);
    await completeAttachmentUpload(
      createClient({ baseUrl: "https://api.test", transport: completer }),
      "acme",
      UUID3,
      UUID2,
      UUID
    );
    expect(completer.calls.at(-1)?.init.method).toBe("PATCH");

    const deleter = json(null, 204);
    await deleteIssueAttachment(
      createClient({ baseUrl: "https://api.test", transport: deleter }),
      "acme",
      UUID3,
      UUID2,
      UUID
    );
    expect(deleter.calls.at(-1)?.init.method).toBe("DELETE");
  });

  it("uploads one file through grant, bytes and complete", async () => {
    const seen: Array<{ url: string; body: FormData }> = [];
    const transport = recordingTransport((call) =>
      fakeResponse({
        status: call.init.method === "PATCH" ? 204 : 200,
        body: call.init.method === "PATCH" ? "" : JSON.stringify(uploadGrant),
      })
    );
    const client = createClient({ baseUrl: "https://api.test", transport });
    const file = new File(["bytes"], "note.png", { type: "image/png" });
    const attachment = await uploadIssueAttachment(client, "acme", UUID3, UUID2, file, (url, body) => {
      seen.push({ url, body });
      return Promise.resolve({ ok: true, status: 200 });
    });
    expect(attachment.id).toBe(UUID);
    expect(attachment.is_uploaded).toBe(true);
    expect(seen).toHaveLength(1);
    const upload = seen[0];
    expect(upload?.url).toBe("https://objects.test/upload");
    expect(upload?.body.get("key")).toBe("k");
    expect((upload?.body.get("file") as File | undefined)?.name).toBe("note.png");
    expect(transport.calls.map((call) => call.init.method)).toEqual(["GET", "POST", "PATCH"]);
  });

  it("uploads bytes through the default signed-post fetch", async () => {
    const seen: Array<{ url: string; init: RequestInit }> = [];
    vi.stubGlobal("fetch", (url: string, init: RequestInit) => {
      seen.push({ url, init });
      return Promise.resolve({ ok: true, status: 200 });
    });
    try {
      const transport = recordingTransport((call) =>
        fakeResponse({
          status: call.init.method === "PATCH" ? 204 : 200,
          body: call.init.method === "PATCH" ? "" : JSON.stringify(uploadGrant),
        })
      );
      const client = createClient({ baseUrl: "https://api.test", transport });
      const file = new File(["bytes"], "note.png", { type: "image/png" });
      const attachment = await uploadIssueAttachment(client, "acme", UUID3, UUID2, file);
      expect(attachment.is_uploaded).toBe(true);
      expect(seen).toHaveLength(1);
      expect(seen[0]?.url).toBe("https://objects.test/upload");
      expect(seen[0]?.init.method).toBe("POST");
      expect(seen[0]?.init.body).toBeInstanceOf(FormData);
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it("rejects when the byte leg fails", async () => {
    const client = clientFor(uploadGrant);
    const file = new File(["bytes"], "note.png", { type: "image/png" });
    const error = await uploadIssueAttachment(client, "acme", UUID3, UUID2, file, () =>
      Promise.resolve({ ok: false, status: 500 })
    ).catch((e: unknown) => e);
    expect(error).toBeInstanceOf(ApiError);
    expect((error as ApiError).status).toBe(500);
  });
});

const draftRow = {
  id: UUID,
  name: "Draft",
  state_id: UUID2,
  sort_order: 1,
  completed_at: null,
  priority: "medium",
  start_date: null,
  target_date: null,
  project_id: UUID3,
  parent_id: null,
  cycle_id: null,
  module_ids: [],
  label_ids: [],
  assignee_ids: [],
  created_at: DT,
  updated_at: DT,
  created_by: UUID,
  updated_by: UUID,
  type_id: null,
  description_html: "<p></p>",
};

const draftEnvelope = {
  grouped_by: null,
  sub_grouped_by: null,
  total_count: 1,
  next_cursor: "50:1:0",
  prev_cursor: "50:0:0",
  next_page_results: false,
  prev_page_results: false,
  count: 1,
  total_results: 1,
  results: [draftRow],
};

describe("draft contracts", () => {
  it("lists drafts with paging params and reads one back", async () => {
    const transport = json(draftEnvelope);
    const listed = await listDraftIssues(createClient({ baseUrl: "https://api.test", transport }), "acme", {
      perPage: 50,
      cursor: "50:0:0",
    });
    expect(listed.results.map((row) => row.id)).toEqual([UUID]);
    const url = new URL(transport.calls[0]?.url ?? "");
    expect(url.searchParams.get("per_page")).toBe("50");

    const reread = await getDraftIssue(clientFor(draftRow), "acme", UUID);
    expect(reread.id).toBe(UUID);
  });

  it("omits unset paging params from the query", async () => {
    const transport = json(draftEnvelope);
    await listDraftIssues(createClient({ baseUrl: "https://api.test", transport }), "acme", {});
    const url = new URL(transport.calls[0]?.url ?? "");
    expect(url.search).toBe("");
  });

  it("creates, updates, deletes and promotes a draft", async () => {
    const creator = json(draftRow, 201);
    const created = await createDraftIssue(createClient({ baseUrl: "https://api.test", transport: creator }), "acme", {
      name: "Draft",
      project_id: UUID3,
    });
    expect(created.name).toBe("Draft");
    expect(JSON.parse(creator.calls.at(-1)?.init.body ?? "{}")).toEqual({ name: "Draft", project_id: UUID3 });

    const updater = json(null, 204);
    await updateDraftIssue(createClient({ baseUrl: "https://api.test", transport: updater }), "acme", UUID, {
      name: "Draft v2",
    });
    expect(updater.calls.at(-1)?.init.method).toBe("PATCH");

    // The update serializer replaces the cycle, module and estimate
    // columns, so the draft edit sends the complete values it stored
    // (ISS-124/ISS-126).
    const membership = json(null, 204);
    await updateDraftIssue(createClient({ baseUrl: "https://api.test", transport: membership }), "acme", UUID, {
      estimate_point: null,
      cycle_id: UUID2,
      module_ids: [UUID2, UUID3],
    });
    expect(JSON.parse(membership.calls.at(-1)?.init.body ?? "{}")).toEqual({
      estimate_point: null,
      cycle_id: UUID2,
      module_ids: [UUID2, UUID3],
    });

    const deleter = json(null, 204);
    await deleteDraftIssue(createClient({ baseUrl: "https://api.test", transport: deleter }), "acme", UUID);
    expect(deleter.calls.at(-1)?.init.method).toBe("DELETE");

    const promotedRow = {
      id: UUID2,
      name: "Draft",
      project_id: UUID3,
      sequence_id: 7,
      state_id: UUID,
      priority: "medium",
      sort_order: 1,
      parent_id: null,
      description_html: "<p></p>",
      created_at: DT,
      updated_at: DT,
      deleted_at: null,
    };
    const promoter = json(promotedRow, 201);
    const promoted = await promoteDraftIssue(
      createClient({ baseUrl: "https://api.test", transport: promoter }),
      "acme",
      UUID,
      { name: "Draft" }
    );
    expect(promoted.id).toBe(UUID2);
    expect(promoter.calls.at(-1)?.url.endsWith(`/draft-to-issue/${UUID}/`)).toBe(true);
  });
});

const reviewRow = {
  id: UUID,
  issue: UUID2,
  provider: "github",
  host_url: "https://github.com",
  namespace: "contract-org",
  repo_name: "contract-repo",
  repo_external_id: "",
  external_id: "",
  external_iid: "42",
  url: "https://github.com/contract-org/contract-repo/pull/42",
  title: "Contract PR",
  state: "open",
  merged: false,
  draft: false,
  remote_updated_at: null,
  metadata: {},
  created_at: DT,
  updated_at: DT,
  created_by: UUID,
  created_by_detail: linkAuthor,
};

describe("code-review contracts", () => {
  it("lists, attaches and detaches reviews", async () => {
    const listed = await listCodeReviews(clientFor([reviewRow]), "acme", UUID3, UUID2);
    expect(listed.map((row) => row.id)).toEqual([UUID]);

    const transport = json(reviewRow, 201);
    const attached = await attachCodeReview(
      createClient({ baseUrl: "https://api.test", transport }),
      "acme",
      UUID3,
      UUID2,
      reviewRow.url
    );
    expect(attached.external_iid).toBe("42");
    expect(JSON.parse(transport.calls.at(-1)?.init.body ?? "{}")).toEqual({ url: reviewRow.url });

    const deleter = json(null, 204);
    await detachCodeReview(
      createClient({ baseUrl: "https://api.test", transport: deleter }),
      "acme",
      UUID3,
      UUID2,
      UUID
    );
    expect(deleter.calls.at(-1)?.init.method).toBe("DELETE");
  });
});

const versionRow = {
  id: UUID,
  workspace: UUID2,
  project: UUID3,
  issue: UUID2,
  last_saved_at: DT,
  owned_by: UUID,
  created_at: DT,
  updated_at: DT,
  created_by: UUID,
  updated_by: null,
};

const versionEnvelope = {
  prev_cursor: "1000:-1:0",
  cursor: "1000:0:0",
  next_cursor: null,
  prev_page_results: false,
  next_page_results: false,
  page_count: 1,
  total_results: 1,
  total_pages: 1,
  results: [versionRow],
};

describe("description-version contracts", () => {
  it("lists the history with the cursor param and reads one version", async () => {
    const transport = json(versionEnvelope);
    const page = await listDescriptionVersions(
      createClient({ baseUrl: "https://api.test", transport }),
      "acme",
      UUID3,
      UUID2,
      { cursor: "1000:0:0" }
    );
    expect(page.results.map((row) => row.id)).toEqual([UUID]);
    const url = new URL(transport.calls[0]?.url ?? "");
    expect(url.pathname).toContain("/work-items/");
    expect(url.searchParams.get("cursor")).toBe("1000:0:0");

    const detail = await getDescriptionVersion(
      clientFor({
        ...versionRow,
        description_binary: null,
        description_html: "<p>v</p>",
        description_stripped: "v",
        description_json: {},
      }),
      "acme",
      UUID3,
      UUID2,
      UUID
    );
    expect(detail.description_html).toBe("<p>v</p>");
  });

  it("omits an unset cursor from the query", async () => {
    const transport = json(versionEnvelope);
    await listDescriptionVersions(createClient({ baseUrl: "https://api.test", transport }), "acme", UUID3, UUID2, {});
    const url = new URL(transport.calls[0]?.url ?? "");
    expect(url.search).toBe("");
  });
});

const searchHit = {
  name: "Target",
  id: UUID,
  start_date: null,
  sequence_id: 9,
  project__name: "Contract",
  project__identifier: "CT",
  project_id: UUID3,
  workspace__slug: "acme",
  state__name: "Backlog",
  state__group: "backlog",
  state__color: "#60646C",
};

describe("issue-search contracts", () => {
  it("parses hits and maps the exclusion flags", async () => {
    const transport = json([searchHit]);
    const hits = await searchIssues(createClient({ baseUrl: "https://api.test", transport }), "acme", UUID3, {
      search: "Target",
      workspaceSearch: true,
      parent: true,
      issueRelation: false,
      subIssue: true,
      issueId: UUID2,
    });
    expect(hits.map((hit) => hit.id)).toEqual([UUID]);
    const url = new URL(transport.calls[0]?.url ?? "");
    expect(url.searchParams.get("search")).toBe("Target");
    expect(url.searchParams.get("workspace_search")).toBe("true");
    expect(url.searchParams.get("parent")).toBe("true");
    expect(url.searchParams.get("issue_relation")).toBe("false");
    expect(url.searchParams.get("sub_issue")).toBe("true");
    expect(url.searchParams.get("issue_id")).toBe(UUID2);
  });

  it("omits unset flags from the query", async () => {
    const transport = json([]);
    await searchIssues(createClient({ baseUrl: "https://api.test", transport }), "acme", UUID3, {});
    const url = new URL(transport.calls[0]?.url ?? "");
    expect(url.search).toBe("");
  });
});

const agentTicker = {
  enabled: true,
  user_disabled: false,
  used: 0,
  tick_count: 0,
  granted: 0,
  waited: 0,
  max_ticks: 10,
  remaining: 10,
  interval_seconds: 3600,
  next_run_at: null,
  last_tick_at: null,
  disarm_reason: "",
  pending_entry: false,
  can_re_tick: false,
};

const agentRunSummary = {
  id: UUID,
  status: "completed",
  executor_kind: "local_runner",
  queue_position: null,
  runner: null,
  runner_name: null,
  created_at: DT,
  assigned_at: null,
  started_at: DT,
  ended_at: DT,
  done_payload: null,
  error: "",
  error_code: "",
  error_diagnostic: null,
  llm_model: "contract-model",
  input_tokens: 1200,
  output_tokens: 340,
  total_tokens: 1540,
  live_state: null,
};

describe("agent contracts", () => {
  it("parses the embedded agent status with its token figures", () => {
    const status = AgentStatus.parse({
      ticker: agentTicker,
      active_run: null,
      latest_run: agentRunSummary,
      run_count: 1,
    });
    expect(status.latest_run?.total_tokens).toBe(1540);
    expect(status.ticker?.max_ticks).toBe(10);
  });
});
