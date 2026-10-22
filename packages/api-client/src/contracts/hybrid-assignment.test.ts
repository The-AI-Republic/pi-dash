// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Hybrid-assignment contracts (dedicated runner on an issue, design
// §14.1–§14.3, §6.6, §7.4, §12.2): the new fields parse on new
// backends and stay absent-tolerant so today's backend still parses.
import { describe, expect, it } from "vitest";
import { ApiError } from "../errors.js";
import { createClient } from "../client.js";
import { fakeResponse, recordingTransport } from "../client.test.js";
import { updateIssue } from "./issue-actions.js";
import { AgentRunSummary } from "./issue-agent.js";
import { AssignedRunnerDetail, getIssue, Issue } from "./issues.js";
import { updateProject } from "./project-management.js";
import { ProjectDetail } from "./projects.js";
import { listRunners, Runner } from "./runners.js";
import { AgentRun, DispatchWait, releaseRunPin, ReleaseRunPinResult } from "./runs.js";

const UUID = "123e4567-e89b-12d3-a456-426614174000";
const UUID2 = "223e4567-e89b-12d3-a456-426614174001";

const POD_MINI = {
  id: UUID,
  name: "CT_probe",
  is_default: true,
  project: UUID2,
  project_identifier: "CT",
};

const RUNNER_DETAIL = {
  id: UUID2,
  name: "gpu-box",
  status: "offline",
  pod: UUID,
  owner: UUID,
  owner_display_name: "Ana",
  host_label: "ana-desktop",
  last_heartbeat_at: "2026-10-11T18:02:11Z",
  is_busy: false,
  queue_depth: 2,
};

const ISSUE = {
  id: UUID,
  name: "Probe issue",
  state_id: null,
  sort_order: 1,
  completed_at: null,
  priority: "medium",
  sequence_id: 3,
  project_id: UUID2,
  parent_id: null,
  cycle_id: null,
  module_ids: [],
  label_ids: [],
  assignee_ids: [],
  sub_issues_count: null,
  attachment_count: null,
  link_count: null,
  created_at: "2026-10-17T04:00:00.000Z",
  updated_at: "2026-10-17T04:00:00.000Z",
  created_by: null,
  updated_by: null,
  is_draft: false,
  archived_at: null,
  complexity_score: 0,
  agent_executor: "local_runner",
  is_synced: false,
  description_html: "<p>probe</p>",
  is_subscribed: false,
  agent_ticker: null,
  agent_status: null,
  relations_summary: { blocked_by: [], blocking: [] },
  has_open_blockers: false,
};

const RUNNER = {
  id: UUID2,
  name: "gpu-box",
  status: "offline",
  host_label: "ana-desktop",
  provisioning: "manual",
  os: "linux",
  arch: "x86_64",
  runner_version: "1.2.3",
  dev_metadata: {},
  protocol_version: 4,
  capabilities: [],
  last_heartbeat_at: "2026-10-11T18:02:11Z",
  owner: UUID,
  dev_machine: null,
  dev_machine_detail: null,
  visibility: 0,
  pod: UUID,
  pod_detail: POD_MINI,
  live_state: null,
  enrolled_at: "2026-10-10T04:00:00.000Z",
  revoked_at: null,
  revoked_reason: "",
  created_at: "2026-10-10T04:00:00.000Z",
  updated_at: "2026-10-17T04:00:00.000Z",
};

const DISPATCH_WAIT = {
  reason: "runner_offline",
  pin_kind: "dedicated",
  runner: UUID2,
  runner_name: "gpu-box",
  queued_at: "2026-10-11T20:14:03Z",
  runner_last_seen_at: "2026-10-11T18:02:11Z",
  runs_ahead: 0,
  releasable: true,
};

const RUN = {
  id: UUID,
  status: "queued",
  executor_kind: "local_runner",
  dispatch_attempts: 0,
  cancel_requested_at: null,
  cancel_reason: "",
  prompt: "Probe prompt",
  thread_id: "",
  agent_metadata: {},
  runner: null,
  work_item: UUID2,
  scheduler_binding: null,
  scheduler_binding_detail: null,
  pod: UUID,
  pod_detail: POD_MINI,
  created_by: UUID,
  owner: UUID,
  created_at: "2026-10-11T20:14:03Z",
  assigned_at: null,
  queue_position: null,
  started_at: null,
  ended_at: null,
  done_payload: null,
  error: "",
  error_code: "",
  error_diagnostic: null,
  refusal_category: "",
  llm_model: "",
  input_tokens: null,
  output_tokens: null,
  total_tokens: null,
  usage: {},
  tool_plan: {},
  tool_calls: [],
};

const RUN_SUMMARY = {
  id: UUID,
  status: "queued",
  executor_kind: "local_runner",
  queue_position: null,
  runner: null,
  runner_name: null,
  created_at: "2026-10-11T20:14:03Z",
  assigned_at: null,
  started_at: null,
  ended_at: null,
  done_payload: null,
  error: "",
  error_code: "",
  error_diagnostic: null,
  llm_model: "",
  input_tokens: null,
  output_tokens: null,
  total_tokens: null,
  live_state: null,
};

const PROJECT_DETAIL = {
  id: UUID2,
  name: "Probe",
  identifier: "CT",
  sort_order: null,
  logo_props: {},
  member_role: 15,
  archived_at: null,
  workspace: UUID,
  cycle_view: true,
  issue_views_view: true,
  module_view: true,
  page_view: true,
  inbox_view: false,
  is_default: false,
  guest_view_all_features: false,
  project_lead: null,
  network: 0,
  created_at: "2026-10-10T04:00:00.000Z",
  updated_at: "2026-10-17T04:00:00.000Z",
  created_by: null,
  updated_by: null,
  agent_executor_options: [{ kind: "local_runner", available: true, reason_code: "" }],
  default_agent_executor: "local_runner",
};

function json(value: unknown) {
  return recordingTransport(() => fakeResponse({ body: JSON.stringify(value), url: "https://api.test/" }));
}

function mutating(value: unknown, status = 200) {
  return recordingTransport((call) =>
    call.url.endsWith("/auth/get-csrf-token/")
      ? fakeResponse({ body: JSON.stringify({ csrf_token: "csrf" }) })
      : fakeResponse({ status, body: JSON.stringify(value) })
  );
}

const PROJECT_DETAILS = {
  ...PROJECT_DETAIL,
  deleted_at: null,
  description: "",
  description_text: null,
  description_html: null,
  default_assignee: null,
  emoji: null,
  icon_prop: null,
  intake_view: false,
  is_time_tracking_enabled: false,
  is_issue_type_enabled: false,
  members_can_edit_states: true,
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
  base_branch: "",
  is_favorite: false,
  anchor: null,
  members: [],
  next_work_item_sequence: 4,
};

describe("hybrid assignment issue contracts", () => {
  it("reads the dedicated runner on the issue detail", async () => {
    const client = createClient({
      baseUrl: "https://api.test",
      transport: json({ ...ISSUE, assigned_runner_id: UUID2, assigned_runner_detail: RUNNER_DETAIL }),
    });
    const parsed = await getIssue(client, "acme", UUID2, UUID);
    expect(parsed.assigned_runner_id).toBe(UUID2);
    expect(parsed.assigned_runner_detail?.name).toBe("gpu-box");
    expect(parsed.assigned_runner_detail?.queue_depth).toBe(2);
  });

  it("reads pod-assigned issues without the runner fields", async () => {
    const client = createClient({ baseUrl: "https://api.test", transport: json(ISSUE) });
    const parsed = await getIssue(client, "acme", UUID2, UUID);
    expect(parsed.assigned_runner_id).toBeUndefined();
    expect(parsed.assigned_runner_detail).toBeUndefined();
  });

  it("accepts a null runner detail on pod-assigned issues", () => {
    const parsed = Issue.parse({ ...ISSUE, assigned_runner_id: null, assigned_runner_detail: null });
    expect(parsed.assigned_runner_id).toBeNull();
    expect(parsed.assigned_runner_detail).toBeNull();
  });

  it("rejects a runner detail without a name", () => {
    expect(() => AssignedRunnerDetail.parse({ ...RUNNER_DETAIL, name: undefined })).toThrow();
  });

  it("patches the dedicated runner triple in one write", async () => {
    const transport = mutating(null, 204);
    const client = createClient({ baseUrl: "https://api.test", transport });
    await updateIssue(client, "acme", UUID2, UUID, {
      assigned_pod_id: UUID,
      assigned_runner_id: UUID2,
      agent_executor: "local_runner",
    });
    const call = transport.calls.find((entry) => !entry.url.endsWith("/auth/get-csrf-token/"));
    expect(call?.init.method).toBe("PATCH");
    expect(call?.init.body).toBe(
      JSON.stringify({ assigned_pod_id: UUID, assigned_runner_id: UUID2, agent_executor: "local_runner" })
    );
  });
});

describe("hybrid assignment runner list contracts", () => {
  it("sends assignable_for and parses the eligibility answers", async () => {
    const transport = recordingTransport(() =>
      fakeResponse({
        body: JSON.stringify([
          { ...RUNNER, assignable: true, not_assignable_code: "", is_busy: false, queue_depth: 2 },
        ]),
        url: "https://api.test/",
      })
    );
    const client = createClient({ baseUrl: "https://api.test", transport });
    const parsed = await listRunners(client, UUID, { projectId: UUID2, assignableFor: UUID });
    expect(parsed).toHaveLength(1);
    expect(parsed[0]?.assignable).toBe(true);
    expect(parsed[0]?.queue_depth).toBe(2);
    const params = new URL(transport.calls[0]?.url ?? "");
    expect(params.searchParams.get("assignable_for")).toBe(UUID);
  });

  it("parses plain runner rows without eligibility answers", () => {
    const parsed = Runner.parse(RUNNER);
    expect(parsed.assignable).toBeUndefined();
    expect(parsed.queue_depth).toBeUndefined();
  });
});

describe("hybrid assignment run contracts", () => {
  it("parses the pin and waiting state on a queued run", () => {
    const parsed = AgentRun.parse({
      ...RUN,
      pinned_runner: UUID2,
      pinned_runner_name: "gpu-box",
      pin_kind: "dedicated",
      dispatch_wait: DISPATCH_WAIT,
    });
    expect(parsed.pinned_runner).toBe(UUID2);
    expect(parsed.pin_kind).toBe("dedicated");
    expect(parsed.dispatch_wait?.reason).toBe("runner_offline");
    expect(parsed.dispatch_wait?.releasable).toBe(true);
  });

  it("parses runs without any pin fields", () => {
    const parsed = AgentRun.parse(RUN);
    expect(parsed.pinned_runner).toBeUndefined();
    expect(parsed.dispatch_wait).toBeUndefined();
  });

  it("parses the waiting state on the issue's embedded run summary", () => {
    const parsed = AgentRunSummary.parse({
      ...RUN_SUMMARY,
      pinned_runner: UUID2,
      pinned_runner_name: "gpu-box",
      pin_kind: "dedicated",
      dispatch_wait: DISPATCH_WAIT,
    });
    expect(parsed.dispatch_wait?.runner_name).toBe("gpu-box");
  });

  it("parses embedded run summaries without the waiting state", () => {
    const parsed = AgentRunSummary.parse(RUN_SUMMARY);
    expect(parsed.dispatch_wait).toBeUndefined();
  });

  it("rejects a waiting state without a reason", () => {
    expect(() => DispatchWait.parse({ ...DISPATCH_WAIT, reason: undefined })).toThrow();
  });

  it("releases a queued run to its pod", async () => {
    const transport = mutating({ ...RUN, pinned_runner: null });
    const client = createClient({ baseUrl: "https://api.test", transport });
    const parsed = await releaseRunPin(client, UUID);
    expect(parsed.id).toBe(UUID);
    expect(parsed.pinned_runner).toBeNull();
    const call = transport.calls.find((entry) => !entry.url.endsWith("/auth/get-csrf-token/"));
    expect(call?.init.method).toBe("POST");
    expect(call?.url.endsWith(`/api/runners/runs/${UUID}/release-pin/`)).toBe(true);
  });

  it("parses the assignment note on newer backends", () => {
    const parsed = ReleaseRunPinResult.parse({
      ...RUN,
      pinned_runner: null,
      assignment: { assigned_runner_id: UUID2, changed: false },
    });
    expect(parsed.assignment?.changed).toBe(false);
    expect(parsed.assignment?.assigned_runner_id).toBe(UUID2);
  });

  it("surfaces a release refusal as an HTTP error", async () => {
    const transport = mutating({ error: "run not queued" }, 409);
    const client = createClient({ baseUrl: "https://api.test", transport });
    const error = await releaseRunPin(client, UUID).catch((caught: unknown) => caught);
    expect(error).toBeInstanceOf(ApiError);
    expect((error as ApiError).status).toBe(409);
  });
});

describe("hybrid assignment project contracts", () => {
  it("reads the lost-runner rule on the project", () => {
    const parsed = ProjectDetail.parse({ ...PROJECT_DETAIL, dedicated_runner_lost_action: "fall_back" });
    expect(parsed.dedicated_runner_lost_action).toBe("fall_back");
  });

  it("reads projects without the lost-runner rule", () => {
    const parsed = ProjectDetail.parse(PROJECT_DETAIL);
    expect(parsed.dedicated_runner_lost_action).toBeUndefined();
  });

  it("writes the lost-runner rule through the project update", async () => {
    const transport = mutating({ ...PROJECT_DETAILS, dedicated_runner_lost_action: "fall_back" });
    const client = createClient({ baseUrl: "https://api.test", transport });
    const updated = await updateProject(client, "acme", UUID2, { dedicated_runner_lost_action: "fall_back" });
    const call = transport.calls.find((entry) => !entry.url.endsWith("/auth/get-csrf-token/"));
    expect(call?.init.method).toBe("PATCH");
    expect(call?.init.body).toBe(JSON.stringify({ dedicated_runner_lost_action: "fall_back" }));
    expect(updated.dedicated_runner_lost_action).toBe("fall_back");
  });
});
