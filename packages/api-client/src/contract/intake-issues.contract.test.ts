// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: intake-issues list/create/retrieve/update/delete against
// live Django. Probe rows carry an `intake-probe-` prefix and are
// deleted by contract; crash leftovers are swept by prefix at the start
// of the filter test. Role paths run on throwaway workspaces (invite +
// join at the role under test) so the seeded CT memberships — whose
// roles other suites move — are never depended on.
import { beforeAll, describe, expect, it } from "vitest";
import { ApiError } from "../errors.js";
import {
  INTAKE_STATUS,
  createIntakeIssue,
  deleteIntakeIssue,
  getIntakeIssue,
  listIntakeIssues,
  updateIntakeIssue,
} from "../contracts/intake-issues.js";
import { getIntakeState, listLabels, listMembers, listStates } from "../contracts/reference.js";
import { listProjects } from "../contracts/projects.js";
import { createProject, deleteProject, updateProject } from "../contracts/project-management.js";
import { updateProjectMember } from "../contracts/project-settings-writes.js";
import { createWorkspace, deleteWorkspace } from "../contracts/workspace-management.js";
import {
  answerWorkspaceInvitation,
  joinProjects,
  listWorkspaceInvitations,
  sendWorkspaceInvitations,
} from "../contracts/invitations.js";
import { getMe } from "../contracts/users.js";
import { getDescriptionVersion, listDescriptionVersions } from "../contracts/issue-versions.js";
import { getIssue } from "../contracts/issues.js";
import { createIssue, deleteIssue } from "../contracts/issue-actions.js";
import { signIn } from "../contracts/auth-entry.js";
import {
  CONTRACT_PASSWORD,
  CONTRACT_PROJECT_IDENTIFIER,
  CONTRACT_SECONDARY_EMAIL,
  CONTRACT_WORKSPACE,
  contractClient,
  contractEnabled,
  sharedSignedIn,
} from "./setup.js";
import type { ApiClient } from "../client.js";

const PROBE_PREFIX = "intake-probe";

let nonce = 0;
function probeName(): string {
  nonce += 1;
  return `${PROBE_PREFIX}-${Date.now().toString(36)}-${nonce}`;
}

function isProbe(name: string): boolean {
  return name === PROBE_PREFIX || name.startsWith(`${PROBE_PREFIX}-`);
}

// One session per user for the file: the scratch stack throttles anonymous
// hits (CSRF fetch + sign-in POST) to 30/minute per IP, so per-test
// sign-ins would 429 each other when files run together.
let admin: ApiClient;
let probe: ApiClient;

beforeAll(async () => {
  admin = await sharedSignedIn();
  const secondary = contractClient();
  const result = await signIn(secondary, { email: CONTRACT_SECONDARY_EMAIL, password: CONTRACT_PASSWORD });
  expect(result.ok).toBe(true);
  probe = secondary;
});

async function seededProjectId(client: ApiClient): Promise<string> {
  const projects = await listProjects(client, CONTRACT_WORKSPACE);
  const seeded = projects.find((project) => project.identifier === CONTRACT_PROJECT_IDENTIFIER);
  expect(seeded).toBeDefined();
  return seeded?.id ?? "";
}

/** Remove probe rows a crashed run may have left behind, in every status. */
async function clearProbeIntakes(client: ApiClient, projectId: string): Promise<void> {
  for (const status of [
    INTAKE_STATUS.pending,
    INTAKE_STATUS.declined,
    INTAKE_STATUS.snoozed,
    INTAKE_STATUS.accepted,
    INTAKE_STATUS.duplicate,
  ]) {
    const page = await listIntakeIssues(client, CONTRACT_WORKSPACE, projectId, { statuses: [status], perPage: 100 });
    for (const row of page.results) {
      if (isProbe(row.issue.name)) {
        await deleteIntakeIssue(client, CONTRACT_WORKSPACE, projectId, row.issue.id);
        // Accepted deletes keep the work item; remove the residue too.
        // Non-accepted deletes cascade, so a 404 here is the good path.
        await deleteIssue(client, CONTRACT_WORKSPACE, projectId, row.issue.id).catch(() => undefined);
      }
    }
  }
}

async function caughtStatus(promise: Promise<unknown>): Promise<number> {
  const error = await promise.then(
    () => null,
    (caught: unknown) => caught
  );
  expect(error).toBeInstanceOf(ApiError);
  return (error as ApiError).status;
}

describe.skipIf(!contractEnabled)("intake create contract", () => {
  it("creates a request in the triage state and reads it back", async () => {
    const projectId = await seededProjectId(admin);
    const triage = await getIntakeState(admin, CONTRACT_WORKSPACE, projectId);
    const labels = await listLabels(admin, CONTRACT_WORKSPACE, projectId);
    const bug = labels.find((label) => label.name === "contract-bug");
    expect(bug).toBeDefined();
    const me = await getMe(admin);

    const name = probeName();
    const created = await createIntakeIssue(admin, CONTRACT_WORKSPACE, projectId, {
      source: "IN_APP",
      issue: {
        name,
        description_html: "<p>probe body</p>",
        priority: "high",
        label_ids: [bug?.id ?? ""],
        assignee_ids: [me.id],
        target_date: "2026-11-01",
      },
    });
    try {
      expect(created.status).toBe(INTAKE_STATUS.pending);
      expect(created.source).toBe("IN_APP");
      expect(created.issue.name).toBe(name);
      expect(created.issue.priority).toBe("high");
      expect(created.issue.state_id).toBe(triage.id);
      expect(created.issue.label_ids).toContain(bug?.id ?? "");
      expect(created.issue.assignee_ids).toContain(me.id);
      expect(created.issue.target_date).toBe("2026-11-01");

      const reread = await getIntakeIssue(admin, CONTRACT_WORKSPACE, projectId, created.issue.id);
      expect(reread.id).toBe(created.id);
      expect(reread.issue.name).toBe(name);

      const listed = await listIntakeIssues(admin, CONTRACT_WORKSPACE, projectId, {
        statuses: [INTAKE_STATUS.pending],
      });
      expect(listed.results.some((row) => row.issue.id === created.issue.id)).toBe(true);
    } finally {
      await deleteIntakeIssue(admin, CONTRACT_WORKSPACE, projectId, created.issue.id);
    }
    const after = await getIntakeIssue(admin, CONTRACT_WORKSPACE, projectId, created.issue.id).then(
      () => null,
      (caught: unknown) => caught
    );
    expect(after).toBeInstanceOf(ApiError);
    expect((after as ApiError).status).toBe(404);
  });

  it("refuses nameless and mistyped creates with 400", async () => {
    const projectId = await seededProjectId(admin);
    const before = await listIntakeIssues(admin, CONTRACT_WORKSPACE, projectId, {
      statuses: [INTAKE_STATUS.pending],
    });

    expect(await caughtStatus(createIntakeIssue(admin, CONTRACT_WORKSPACE, projectId, { issue: { name: "" } }))).toBe(
      400
    );
    expect(
      await caughtStatus(
        createIntakeIssue(admin, CONTRACT_WORKSPACE, projectId, { issue: { name: probeName(), priority: "bogus" } })
      )
    ).toBe(400);

    const after = await listIntakeIssues(admin, CONTRACT_WORKSPACE, projectId, {
      statuses: [INTAKE_STATUS.pending],
    });
    expect(after.total_results).toBe(before.total_results);
  });

  it("404s unknown request ids", async () => {
    const projectId = await seededProjectId(admin);
    expect(
      await caughtStatus(getIntakeIssue(admin, CONTRACT_WORKSPACE, projectId, "123e4567-e89b-12d3-a456-426614174000"))
    ).toBe(404);
  });
});

describe.skipIf(!contractEnabled)("intake list contract", () => {
  it("filters, orders and pages the list", async () => {
    const projectId = await seededProjectId(admin);
    await clearProbeIntakes(admin, projectId);
    const labels = await listLabels(admin, CONTRACT_WORKSPACE, projectId);
    const bug = labels.find((label) => label.name === "contract-bug");
    const me = await getMe(admin);
    const createdIds: string[] = [];
    try {
      const urgent = await createIntakeIssue(admin, CONTRACT_WORKSPACE, projectId, {
        issue: { name: probeName(), priority: "urgent", label_ids: [bug?.id ?? ""], assignee_ids: [me.id] },
      });
      const high = await createIntakeIssue(admin, CONTRACT_WORKSPACE, projectId, {
        issue: { name: probeName(), priority: "high" },
      });
      const low = await createIntakeIssue(admin, CONTRACT_WORKSPACE, projectId, {
        issue: { name: probeName(), priority: "low" },
      });
      createdIds.push(urgent.issue.id, high.issue.id, low.issue.id);

      const pending = await listIntakeIssues(admin, CONTRACT_WORKSPACE, projectId, {
        statuses: [INTAKE_STATUS.pending],
      });
      for (const id of createdIds) {
        expect(pending.results.some((row) => row.issue.id === id)).toBe(true);
      }
      const accepted = await listIntakeIssues(admin, CONTRACT_WORKSPACE, projectId, {
        statuses: [INTAKE_STATUS.accepted],
      });
      for (const id of createdIds) {
        expect(accepted.results.some((row) => row.issue.id === id)).toBe(false);
      }

      const union = await listIntakeIssues(admin, CONTRACT_WORKSPACE, projectId, {
        statuses: [INTAKE_STATUS.pending],
        priorities: ["urgent", "high"],
      });
      expect(union.results.some((row) => row.issue.id === urgent.issue.id)).toBe(true);
      expect(union.results.some((row) => row.issue.id === high.issue.id)).toBe(true);
      expect(union.results.some((row) => row.issue.id === low.issue.id)).toBe(false);

      const byLabel = await listIntakeIssues(admin, CONTRACT_WORKSPACE, projectId, {
        statuses: [INTAKE_STATUS.pending],
        labels: [bug?.id ?? ""],
      });
      expect(byLabel.results.some((row) => row.issue.id === urgent.issue.id)).toBe(true);
      expect(byLabel.results.some((row) => row.issue.id === high.issue.id)).toBe(false);

      const byAssignee = await listIntakeIssues(admin, CONTRACT_WORKSPACE, projectId, {
        statuses: [INTAKE_STATUS.pending],
        assignees: [me.id],
      });
      expect(byAssignee.results.some((row) => row.issue.id === urgent.issue.id)).toBe(true);
      expect(byAssignee.results.some((row) => row.issue.id === high.issue.id)).toBe(false);

      // `created_by` filters the work item's creator, not the intake
      // entry's audit user: every probe here was created by the admin.
      const byCreator = await listIntakeIssues(admin, CONTRACT_WORKSPACE, projectId, {
        statuses: [INTAKE_STATUS.pending],
        createdBy: [me.id],
      });
      for (const id of createdIds) {
        expect(byCreator.results.some((row) => row.issue.id === id)).toBe(true);
      }
      const byStranger = await listIntakeIssues(admin, CONTRACT_WORKSPACE, projectId, {
        statuses: [INTAKE_STATUS.pending],
        createdBy: ["123e4567-e89b-12d3-a456-426614174000"],
      });
      expect(byStranger.results).toHaveLength(0);

      const today = new Date().toISOString().slice(0, 10);
      const byDate = await listIntakeIssues(admin, CONTRACT_WORKSPACE, projectId, {
        statuses: [INTAKE_STATUS.pending],
        createdAt: `${today};after,${today};before`,
      });
      for (const id of createdIds) {
        expect(byDate.results.some((row) => row.issue.id === id)).toBe(true);
      }
      const byPast = await listIntakeIssues(admin, CONTRACT_WORKSPACE, projectId, {
        statuses: [INTAKE_STATUS.pending],
        createdAt: "2001-01-01;after,2001-01-31;before",
      });
      for (const id of createdIds) {
        expect(byPast.results.some((row) => row.issue.id === id)).toBe(false);
      }

      const ascending = await listIntakeIssues(admin, CONTRACT_WORKSPACE, projectId, {
        statuses: [INTAKE_STATUS.pending],
        orderBy: "issue__sequence_id",
        perPage: 100,
      });
      const sequences = ascending.results.map((row) => row.issue.sequence_id);
      expect([...sequences].sort((a, b) => a - b)).toEqual(sequences);
      const descending = await listIntakeIssues(admin, CONTRACT_WORKSPACE, projectId, {
        statuses: [INTAKE_STATUS.pending],
        orderBy: "-issue__sequence_id",
        perPage: 100,
      });
      const reversed = descending.results.map((row) => row.issue.sequence_id);
      expect([...reversed].sort((a, b) => b - a)).toEqual(reversed);

      const first = await listIntakeIssues(admin, CONTRACT_WORKSPACE, projectId, {
        statuses: [INTAKE_STATUS.pending],
        orderBy: "issue__sequence_id",
        perPage: 2,
      });
      expect(first.results).toHaveLength(2);
      expect(first.next_page_results).toBe(true);
      const second = await listIntakeIssues(admin, CONTRACT_WORKSPACE, projectId, {
        statuses: [INTAKE_STATUS.pending],
        orderBy: "issue__sequence_id",
        perPage: 2,
        cursor: first.next_cursor,
      });
      expect(second.results).toHaveLength(1);
      const combined = [...first.results, ...second.results].map((row) => row.issue.id);
      expect(new Set(combined).size).toBe(3);
      for (const id of createdIds) {
        expect(combined).toContain(id);
      }
    } finally {
      for (const id of createdIds) {
        await deleteIntakeIssue(admin, CONTRACT_WORKSPACE, projectId, id).catch(() => 0);
      }
    }
  });
});

describe.skipIf(!contractEnabled)("intake triage contract", () => {
  it("snoozes and un-snoozes", async () => {
    const projectId = await seededProjectId(admin);
    const created = await createIntakeIssue(admin, CONTRACT_WORKSPACE, projectId, { issue: { name: probeName() } });
    try {
      const snoozed = await updateIntakeIssue(admin, CONTRACT_WORKSPACE, projectId, created.issue.id, {
        status: INTAKE_STATUS.snoozed,
        snoozed_till: "2030-01-15T00:00:00Z",
      });
      expect(snoozed.status).toBe(INTAKE_STATUS.snoozed);
      expect(snoozed.snoozed_till).toContain("2030-01-15");

      const snoozedList = await listIntakeIssues(admin, CONTRACT_WORKSPACE, projectId, {
        statuses: [INTAKE_STATUS.snoozed],
      });
      expect(snoozedList.results.some((row) => row.issue.id === created.issue.id)).toBe(true);

      const awake = await updateIntakeIssue(admin, CONTRACT_WORKSPACE, projectId, created.issue.id, {
        status: INTAKE_STATUS.pending,
        snoozed_till: null,
      });
      expect(awake.status).toBe(INTAKE_STATUS.pending);
      expect(awake.snoozed_till).toBeNull();
    } finally {
      await deleteIntakeIssue(admin, CONTRACT_WORKSPACE, projectId, created.issue.id);
    }
  });

  it("declines", async () => {
    const projectId = await seededProjectId(admin);
    const created = await createIntakeIssue(admin, CONTRACT_WORKSPACE, projectId, { issue: { name: probeName() } });
    try {
      const declined = await updateIntakeIssue(admin, CONTRACT_WORKSPACE, projectId, created.issue.id, {
        status: INTAKE_STATUS.declined,
      });
      expect(declined.status).toBe(INTAKE_STATUS.declined);
    } finally {
      await deleteIntakeIssue(admin, CONTRACT_WORKSPACE, projectId, created.issue.id);
    }
  });

  it("marks duplicate with the target summary", async () => {
    const projectId = await seededProjectId(admin);
    // The duplicate target is a regular work item, not another pending
    // request: the validator's issue queryset excludes triage-state
    // rows, so a pending request's id is refused as unknown.
    const states = await listStates(admin, CONTRACT_WORKSPACE, projectId);
    const fallback = states.find((state) => state.default);
    const target = await createIssue(admin, CONTRACT_WORKSPACE, projectId, {
      name: probeName(),
      state_id: fallback?.id ?? null,
    });
    const created = await createIntakeIssue(admin, CONTRACT_WORKSPACE, projectId, { issue: { name: probeName() } });
    try {
      const duplicate = await updateIntakeIssue(admin, CONTRACT_WORKSPACE, projectId, created.issue.id, {
        status: INTAKE_STATUS.duplicate,
        duplicate_to: target.id,
      });
      expect(duplicate.status).toBe(INTAKE_STATUS.duplicate);
      expect(duplicate.duplicate_to).toBe(target.id);
      expect(duplicate.duplicate_issue_detail?.id).toBe(target.id);
      expect(duplicate.duplicate_issue_detail?.sequence_id).toBe(target.sequence_id);
    } finally {
      await deleteIntakeIssue(admin, CONTRACT_WORKSPACE, projectId, created.issue.id);
      await deleteIssue(admin, CONTRACT_WORKSPACE, projectId, target.id);
    }
  });

  it("accepts into the default state", async () => {
    const projectId = await seededProjectId(admin);
    const states = await listStates(admin, CONTRACT_WORKSPACE, projectId);
    const fallback = states.find((state) => state.default);
    expect(fallback).toBeDefined();
    const created = await createIntakeIssue(admin, CONTRACT_WORKSPACE, projectId, { issue: { name: probeName() } });
    try {
      expect(created.issue.state_id).not.toBe(fallback?.id ?? "");
      const accepted = await updateIntakeIssue(admin, CONTRACT_WORKSPACE, projectId, created.issue.id, {
        status: INTAKE_STATUS.accepted,
      });
      expect(accepted.status).toBe(INTAKE_STATUS.accepted);
      expect(accepted.issue.state_id).toBe(fallback?.id ?? "");
    } finally {
      await deleteIntakeIssue(admin, CONTRACT_WORKSPACE, projectId, created.issue.id);
      await deleteIssue(admin, CONTRACT_WORKSPACE, projectId, created.issue.id).catch(() => undefined);
    }
  });

  it("edits work-item fields inline", async () => {
    const projectId = await seededProjectId(admin);
    const labels = await listLabels(admin, CONTRACT_WORKSPACE, projectId);
    const bug = labels.find((label) => label.name === "contract-bug");
    const me = await getMe(admin);
    const created = await createIntakeIssue(admin, CONTRACT_WORKSPACE, projectId, { issue: { name: probeName() } });
    try {
      const renamed = `${PROBE_PREFIX}-edited`;
      const edited = await updateIntakeIssue(admin, CONTRACT_WORKSPACE, projectId, created.issue.id, {
        issue: {
          name: renamed,
          description_html: "<p>edited</p>",
          priority: "urgent",
          label_ids: [bug?.id ?? ""],
          assignee_ids: [me.id],
          target_date: "2026-12-01",
        },
      });
      expect(edited.issue.name).toBe(renamed);
      expect(edited.issue.description_html).toBe("<p>edited</p>");
      expect(edited.issue.priority).toBe("urgent");
      expect(edited.issue.label_ids).toContain(bug?.id ?? "");
      expect(edited.issue.assignee_ids).toContain(me.id);
      expect(edited.issue.target_date).toBe("2026-12-01");
    } finally {
      await deleteIntakeIssue(admin, CONTRACT_WORKSPACE, projectId, created.issue.id);
    }
  });
});

describe.skipIf(!contractEnabled)("intake delete contract", () => {
  it("cascades to the work item except for accepted requests", async () => {
    const projectId = await seededProjectId(admin);
    const pending = await createIntakeIssue(admin, CONTRACT_WORKSPACE, projectId, { issue: { name: probeName() } });
    expect(await deleteIntakeIssue(admin, CONTRACT_WORKSPACE, projectId, pending.issue.id)).toBe(204);
    expect(await caughtStatus(getIssue(admin, CONTRACT_WORKSPACE, projectId, pending.issue.id))).toBe(404);

    const accepted = await createIntakeIssue(admin, CONTRACT_WORKSPACE, projectId, { issue: { name: probeName() } });
    await updateIntakeIssue(admin, CONTRACT_WORKSPACE, projectId, accepted.issue.id, {
      status: INTAKE_STATUS.accepted,
    });
    expect(await deleteIntakeIssue(admin, CONTRACT_WORKSPACE, projectId, accepted.issue.id)).toBe(204);
    const kept = await getIssue(admin, CONTRACT_WORKSPACE, projectId, accepted.issue.id);
    expect(kept.id).toBe(accepted.issue.id);
    await deleteIssue(admin, CONTRACT_WORKSPACE, projectId, accepted.issue.id);
  });
});

describe.skipIf(!contractEnabled)("intake versions contract", () => {
  it("reads an intake work item's versions through the shared work-items contract", async () => {
    // INT-029: no second contract for the intake-work-items/ alias —
    // the work-items/ read serves intake rows because they are work
    // items. This pins that reuse against the seeded intake row (live
    // version rows are written by celery, and no worker runs in
    // contract tests, so the row and its version are seeded).
    const projectId = await seededProjectId(admin);
    const page = await listIntakeIssues(admin, CONTRACT_WORKSPACE, projectId, {
      statuses: [INTAKE_STATUS.accepted],
      perPage: 100,
    });
    const seeded = page.results.find((row) => row.issue.name === "Contract intake version probe");
    expect(seeded).toBeDefined();
    const versions = await listDescriptionVersions(admin, CONTRACT_WORKSPACE, projectId, seeded?.issue.id ?? "");
    expect(versions.results.length).toBeGreaterThan(0);
    const first = versions.results[0];
    expect(first).toBeDefined();
    const reread = await getDescriptionVersion(
      admin,
      CONTRACT_WORKSPACE,
      projectId,
      seeded?.issue.id ?? "",
      first?.id ?? ""
    );
    expect(reread.id).toBe(first?.id ?? "");
    expect(reread.description_html).toBe("<p>Contract intake version probe</p>");
  });
});

/** Fresh slug per test: the backend reserves slugs of deleted rows. */
function intakeSlug(prefix: string): string {
  return `${prefix}-${Date.now().toString(36)}${Math.floor(Math.random() * 1296).toString(36)}`;
}

let projectNonce = 0;
function intakeProject(): { name: string; identifier: string } {
  projectNonce += 1;
  const suffix = `${Date.now().toString(36).toUpperCase().slice(-6)}${projectNonce.toString(36).toUpperCase()}`;
  return { name: `Contract Intake ${suffix}`, identifier: `CTIN${suffix}` };
}

/** Invite the probe user to a workspace and have them accept at a role. */
async function acceptProbeInvite(slug: string, role: number): Promise<void> {
  const sent = await sendWorkspaceInvitations(admin, slug, {
    emails: [{ email: CONTRACT_SECONDARY_EMAIL, role }],
  });
  expect(sent).toEqual({ message: "Emails sent successfully" });
  const invitations = await listWorkspaceInvitations(probe);
  const invitation = invitations.find((row) => row.workspace.slug === slug);
  expect(invitation).toBeDefined();
  const accepted = await answerWorkspaceInvitation(probe, slug, invitation?.id ?? "", {
    accepted: true,
    token: invitation?.token ?? "",
  });
  expect(accepted).toEqual({ message: "Workspace Invitation Accepted" });
}

describe.skipIf(!contractEnabled)("intake enablement contract", () => {
  it("404s the list until the intake view is enabled", async () => {
    const input = intakeProject();
    const created = await createProject(admin, CONTRACT_WORKSPACE, input);
    try {
      expect(await caughtStatus(listIntakeIssues(admin, CONTRACT_WORKSPACE, created.id))).toBe(404);
      await updateProject(admin, CONTRACT_WORKSPACE, created.id, { inbox_view: true });
      const empty = await listIntakeIssues(admin, CONTRACT_WORKSPACE, created.id, {
        statuses: [INTAKE_STATUS.pending],
      });
      expect(empty.total_results).toBe(0);
      expect(empty.results).toHaveLength(0);
    } finally {
      await deleteProject(admin, CONTRACT_WORKSPACE, created.id).catch(() => 0);
    }
  });
});

describe.skipIf(!contractEnabled)("intake member contract", () => {
  it("lets members create and manage their own requests, and refuses others'", async () => {
    const slug = intakeSlug("ct-intake-mem");
    await createWorkspace(admin, { name: `Contract ${slug}`, slug, organization_size: "2-10" });
    try {
      const input = intakeProject();
      const project = await createProject(admin, slug, input);
      await updateProject(admin, slug, project.id, { inbox_view: true });
      await acceptProbeInvite(slug, 15);
      await joinProjects(probe, slug, [project.id]);

      const own = await createIntakeIssue(probe, slug, project.id, { issue: { name: probeName() } });
      expect(own.status).toBe(INTAKE_STATUS.pending);

      // A member's status write on their own request is ignored with a
      // 200: the creator bypass admits them, but no intake serializer
      // runs for a non-admin.
      const untouched = await updateIntakeIssue(probe, slug, project.id, own.issue.id, {
        status: INTAKE_STATUS.accepted,
      });
      expect(untouched.status).toBe(INTAKE_STATUS.pending);

      const edited = await updateIntakeIssue(probe, slug, project.id, own.issue.id, {
        issue: { name: `${PROBE_PREFIX}-member-edit`, priority: "low" },
      });
      expect(edited.issue.name).toBe(`${PROBE_PREFIX}-member-edit`);
      expect(edited.issue.priority).toBe("low");

      const admins = await createIntakeIssue(admin, slug, project.id, { issue: { name: probeName() } });
      expect(
        await caughtStatus(updateIntakeIssue(probe, slug, project.id, admins.issue.id, { issue: { name: "nope" } }))
      ).toBe(403);
      expect(await caughtStatus(deleteIntakeIssue(probe, slug, project.id, admins.issue.id))).toBe(403);

      // The creator may delete their own request.
      expect(await deleteIntakeIssue(probe, slug, project.id, own.issue.id)).toBe(204);
    } finally {
      await deleteWorkspace(admin, slug).catch(() => 0);
    }
  });
});

describe.skipIf(!contractEnabled)("intake guest contract", () => {
  it("scopes guests to their own requests and to name/description writes", async () => {
    const slug = intakeSlug("ct-intake-gst");
    await createWorkspace(admin, { name: `Contract ${slug}`, slug, organization_size: "2-10" });
    try {
      const input = intakeProject();
      const project = await createProject(admin, slug, input);
      await updateProject(admin, slug, project.id, { inbox_view: true });
      // Guests cannot join projects, so the probe joins as a member and
      // is re-roled to guest on the project once inside.
      await acceptProbeInvite(slug, 15);
      await joinProjects(probe, slug, [project.id]);
      const me = await getMe(probe);
      const members = await listMembers(admin, slug, project.id);
      const membership = members.find((row) => row.member === me.id);
      expect(membership).toBeDefined();
      await updateProjectMember(admin, slug, project.id, membership?.id ?? "", 5);

      const own = await createIntakeIssue(probe, slug, project.id, { issue: { name: probeName() } });
      expect(own.status).toBe(INTAKE_STATUS.pending);

      // A guest creator's write keeps name and description; the
      // priority change is ignored with a 200.
      const narrowed = await updateIntakeIssue(probe, slug, project.id, own.issue.id, {
        issue: { name: `${PROBE_PREFIX}-guest-edit`, priority: "urgent" },
      });
      expect(narrowed.issue.name).toBe(`${PROBE_PREFIX}-guest-edit`);
      expect(narrowed.issue.priority).toBe("none");

      const admins = await createIntakeIssue(admin, slug, project.id, { issue: { name: probeName() } });
      // A guest editing someone else's request never reaches the view:
      // the admin-only decorator (with its creator bypass) refuses with
      // 403 first.
      expect(
        await caughtStatus(updateIntakeIssue(probe, slug, project.id, admins.issue.id, { issue: { name: "nope" } }))
      ).toBe(403);
      expect(await caughtStatus(getIntakeIssue(probe, slug, project.id, admins.issue.id))).toBe(403);

      const reread = await getIntakeIssue(probe, slug, project.id, own.issue.id);
      expect(reread.issue.name).toBe(`${PROBE_PREFIX}-guest-edit`);

      const listed = await listIntakeIssues(probe, slug, project.id, { statuses: [INTAKE_STATUS.pending] });
      expect(listed.results.some((row) => row.issue.id === own.issue.id)).toBe(true);
      expect(listed.results.some((row) => row.issue.id === admins.issue.id)).toBe(false);
    } finally {
      await deleteWorkspace(admin, slug).catch(() => 0);
    }
  });
});
