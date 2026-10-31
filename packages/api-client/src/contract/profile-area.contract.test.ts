// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: the profile area's endpoints against live Django (NEWFRONT-570).
// Covers the segregation/stats/issue/activity reads, the CSV export, own
// history, the email and password writes, token CRUD, upkeep toggles and
// the GitHub App integration reads. Every probe row is removed again.
import { describe, expect, it } from "vitest";
import type { ApiClient } from "../client.js";
import { ApiError } from "../errors.js";
import { changeEmail, changePassword, generateEmailChangeCode } from "../contracts/account-settings.js";
import { createApiToken, deleteApiToken, getApiToken, listApiTokens } from "../contracts/api-tokens.js";
import { signIn, signUp } from "../contracts/auth-entry.js";
import { deactivateAccount } from "../contracts/auth.js";
import { getGithubAppStatus, refreshGithubAppInstallation, startGithubAppInstall } from "../contracts/integrations.js";
import {
  exportUserProfileActivity,
  getUserProfileActivity,
  getUserProfileSegregation,
  getUserProfileStats,
  listGroupedUserProfileIssues,
  listMyActivities,
  listUserProfileIssues,
} from "../contracts/profile-activity.js";
import { getUpkeepSettings, setUpkeepEnabled, setUpkeepJobEnabled } from "../contracts/upkeep.js";
import { getMe } from "../contracts/users.js";
import { listWorkspaces } from "../contracts/workspaces.js";
import { CONTRACT_EMAIL, CONTRACT_WORKSPACE, contractClient, contractEnabled, sharedSignedIn } from "./setup.js";

function throwawayEmail(prefix: string): string {
  const stamp = `${Date.now()}-${Math.floor(Math.random() * 1e6)}`;
  return `contract-profile-${prefix}-${stamp}@example.com`;
}

async function seededUser(): Promise<{ client: ApiClient; id: string }> {
  const client = await sharedSignedIn();
  const me = await getMe(client);
  return { client, id: me.id };
}

describe.skipIf(!contractEnabled)("profile area contract", () => {
  it("reads the segregation with identity and per-project counts", async () => {
    const { client, id } = await seededUser();
    const segregation = await getUserProfileSegregation(client, CONTRACT_WORKSPACE, id);
    expect(segregation.user_data.email).toBe(CONTRACT_EMAIL);
    expect(segregation.user_data.display_name.length).toBeGreaterThan(0);
    const seeded = segregation.project_data.find((row) => row.id);
    expect(seeded).toBeDefined();
    for (const row of segregation.project_data) {
      expect(row.created_issues).toBeGreaterThanOrEqual(0);
      expect(row.assigned_issues).toBeGreaterThanOrEqual(0);
      expect(row.completed_issues).toBeGreaterThanOrEqual(0);
      expect(row.pending_issues).toBeGreaterThanOrEqual(0);
    }
  });

  it("reads stats whose assigned total matches the assigned tab", async () => {
    const { client, id } = await seededUser();
    const stats = await getUserProfileStats(client, CONTRACT_WORKSPACE, id);
    expect(stats.assigned_issues).toBeGreaterThan(0);
    expect(stats.created_issues).toBeGreaterThan(0);
    expect(stats.subscribed_issues).toBeGreaterThan(0);
    for (const bucket of stats.state_distribution) {
      expect(bucket.state_count).toBeGreaterThan(0);
    }
    for (const bucket of stats.priority_distribution) {
      expect(bucket.priority_count).toBeGreaterThan(0);
    }
    const assigned = await listUserProfileIssues(client, CONTRACT_WORKSPACE, id, {
      ownership: "assigned",
      perPage: 100,
    });
    expect(assigned.total_results).toBe(stats.assigned_issues);
    expect(assigned.results.length).toBeGreaterThan(0);
  });

  it("filters each profile issue tab by its ownership dimension", async () => {
    const { client, id } = await seededUser();
    const assigned = await listUserProfileIssues(client, CONTRACT_WORKSPACE, id, {
      ownership: "assigned",
      perPage: 100,
    });
    expect(assigned.results.some((row) => row.assignee_ids.includes(id))).toBe(true);
    const created = await listUserProfileIssues(client, CONTRACT_WORKSPACE, id, {
      ownership: "created",
      perPage: 100,
    });
    expect(created.total_results).toBeGreaterThan(0);
    const subscribed = await listUserProfileIssues(client, CONTRACT_WORKSPACE, id, {
      ownership: "subscribed",
      perPage: 100,
    });
    expect(subscribed.total_results).toBeGreaterThan(0);
    for (const row of [...created.results, ...subscribed.results]) {
      expect(row.id).toBeDefined();
    }
  });

  it("groups a profile issue tab", async () => {
    const { client, id } = await seededUser();
    const grouped = await listGroupedUserProfileIssues(client, CONTRACT_WORKSPACE, id, {
      ownership: "assigned",
      groupBy: "priority",
    });
    expect(grouped.grouped_by).toBe("priority");
    const buckets = Object.values(grouped.results);
    expect(buckets.length).toBeGreaterThan(0);
    const total = buckets.reduce((sum, bucket) => sum + bucket.total_results, 0);
    expect(total).toBe(grouped.total_results);
  });

  it("pages the profile activity feed", async () => {
    const { client, id } = await seededUser();
    const first = await getUserProfileActivity(client, CONTRACT_WORKSPACE, id, { perPage: 100 });
    expect(first.results.length).toBeGreaterThan(0);
    expect(first.results.every((row) => row.actor === id)).toBe(true);
    expect(first.count).toBe(first.results.length);
  });

  it("reads the caller's own cross-workspace history", async () => {
    const { client } = await seededUser();
    const mine = await listMyActivities(client, { perPage: 10 });
    expect(mine.results.length).toBeGreaterThan(0);
    expect(mine.total_results).toBeGreaterThanOrEqual(mine.results.length);
  });

  it("exports the day's activity as CSV", async () => {
    const { client, id } = await seededUser();
    const today = new Date().toISOString().slice(0, 10);
    const exported = await exportUserProfileActivity(client, CONTRACT_WORKSPACE, id, today);
    expect(exported.status).toBe(200);
    expect(exported.contentType).toContain("text/csv");
    expect(exported.filename).toBe("workspace-user-activity.csv");
    expect(exported.csv).toContain('"Actor name"');
    expect(exported.csv).toContain('"New value"');
  });

  it("refreshes the upkeep payload when the master pause flips", async () => {
    const { client } = await seededUser();
    const before = await getUpkeepSettings(client);
    const jobBefore = before.jobs.find((job) => job.slug === "contract-upkeep");
    expect(jobBefore).toBeDefined();
    expect(jobBefore?.enabled).toBe(true);
    try {
      const paused = await setUpkeepEnabled(client, false);
      expect(paused.enabled).toBe(false);
      expect(paused.jobs.find((job) => job.slug === "contract-upkeep")?.enabled).toBe(true);
    } finally {
      const restored = await setUpkeepEnabled(client, before.enabled);
      expect(restored.enabled).toBe(before.enabled);
      expect(restored.jobs.find((job) => job.slug === "contract-upkeep")?.enabled).toBe(true);
    }
  });

  it("toggles one upkeep job and restores it", async () => {
    const { client } = await seededUser();
    const before = await getUpkeepSettings(client);
    const job = before.jobs.find((row) => row.slug === "contract-upkeep");
    expect(job).toBeDefined();
    const toggled = await setUpkeepJobEnabled(client, "contract-upkeep", !job?.enabled);
    expect(toggled.jobs.find((row) => row.slug === "contract-upkeep")?.enabled).toBe(!job?.enabled);
    const restored = await setUpkeepJobEnabled(client, "contract-upkeep", job?.enabled ?? true);
    expect(restored.jobs.find((row) => row.slug === "contract-upkeep")?.enabled).toBe(job?.enabled);
  });

  it("refuses an unknown upkeep job server-side", async () => {
    const { client } = await seededUser();
    const failure = await setUpkeepJobEnabled(client, "contract-no-such-job", false).then(
      () => null,
      (error: unknown) => error
    );
    expect(failure).toBeInstanceOf(ApiError);
    expect((failure as ApiError).status).toBe(404);
  });

  it("round-trips a personal access token", async () => {
    const { client } = await seededUser();
    const label = `Contract token ${Date.now()}`;
    const created = await createApiToken(client, {
      label,
      description: "Contract probe",
      expiredAt: null,
    });
    try {
      expect(created.label).toBe(label);
      expect(created.token.length).toBeGreaterThan(0);
      expect(created.is_active).toBe(true);
      const listed = await listApiTokens(client);
      const row = listed.find((token) => token.id === created.id);
      expect(row?.label).toBe(label);
      expect(row).not.toHaveProperty("token");
      const one = await getApiToken(client, created.id);
      expect(one.id).toBe(created.id);
      expect(one).not.toHaveProperty("token");
    } finally {
      await deleteApiToken(client, created.id);
    }
    const gone = await getApiToken(client, created.id).then(
      () => null,
      (error: unknown) => error
    );
    expect(gone).toBeInstanceOf(ApiError);
    expect((gone as ApiError).status).toBe(404);
  });

  it("expires a token on the chosen date", async () => {
    const { client } = await seededUser();
    const created = await createApiToken(client, {
      label: `Contract expiring ${Date.now()}`,
      expiredAt: "2099-01-01T00:00:00Z",
    });
    try {
      expect(created.expired_at).toBe("2099-01-01T00:00:00Z");
      expect(created.is_active).toBe(true);
    } finally {
      await deleteApiToken(client, created.id);
    }
  });

  it("sends an email change code to a free address", async () => {
    // The send is throttled per member, so a fresh throwaway account keeps
    // this probe repeatable instead of consuming the seed user's hourly
    // allowance. The account is deactivated again before the test ends.
    const client = contractClient();
    await signUp(client, {
      email: throwawayEmail("code"),
      password: "OldStrong123!xyz",
      confirmPassword: "OldStrong123!xyz",
    });
    const target = throwawayEmail("target");
    try {
      const sent = await generateEmailChangeCode(client, target);
      expect(sent.message.length).toBeGreaterThan(0);
    } finally {
      await deactivateAccount(client).catch(() => undefined);
    }
  });

  it("refuses an email change with a wrong code and keeps the account email", async () => {
    const { client, id } = await seededUser();
    const me = await getMe(client);
    const failure = await changeEmail(client, throwawayEmail("wrong"), "000000").then(
      () => null,
      (error: unknown) => error
    );
    expect(failure).toBeInstanceOf(ApiError);
    expect((failure as ApiError).status).toBe(400);
    const after = await getMe(client);
    expect(after.id).toBe(id);
    expect(after.email).toBe(me.email);
  });

  it("reads the GitHub App status for the seeded workspace", async () => {
    const { client } = await seededUser();
    const workspaces = await listWorkspaces(client);
    expect(workspaces.some((workspace) => workspace.slug === CONTRACT_WORKSPACE)).toBe(true);
    const status = await getGithubAppStatus(client);
    expect(typeof status.configured).toBe("boolean");
    expect(status.workspaces.some((workspace) => workspace.slug === CONTRACT_WORKSPACE)).toBe(true);
    for (const workspace of status.workspaces) {
      expect(typeof workspace.github_app.connected).toBe("boolean");
    }
  });

  it("refuses an install start while the instance app config is missing", async () => {
    const { client } = await seededUser();
    const status = await getGithubAppStatus(client);
    if (status.configured) return;
    const failure = await startGithubAppInstall(client, CONTRACT_WORKSPACE).then(
      () => null,
      (error: unknown) => error
    );
    expect(failure).toBeInstanceOf(ApiError);
    expect((failure as ApiError).status).toBe(409);
    expect((failure as ApiError).message).toContain("GitHub App");
  });

  it("refuses a refresh when nothing is installed for the workspace", async () => {
    const { client } = await seededUser();
    const status = await getGithubAppStatus(client);
    const workspace = status.workspaces.find((row) => row.slug === CONTRACT_WORKSPACE);
    if (workspace?.github_app.connected) return;
    const failure = await refreshGithubAppInstallation(client, CONTRACT_WORKSPACE).then(
      () => null,
      (error: unknown) => error
    );
    expect(failure).toBeInstanceOf(ApiError);
    expect((failure as ApiError).status).toBe(404);
    expect((failure as ApiError).message).toContain("not installed");
  });

  it("changes a throwaway account's password and then deactivates it", async () => {
    const client = contractClient();
    const email = throwawayEmail("password");
    const created = await signUp(client, {
      email,
      password: "OldStrong123!xyz",
      confirmPassword: "OldStrong123!xyz",
    });
    expect(created.ok).toBe(true);
    try {
      const changed = await changePassword(client, {
        oldPassword: "OldStrong123!xyz",
        newPassword: "NewStrong123!xyz",
      });
      expect(changed.message.length).toBeGreaterThan(0);
      const me = await getMe(client);
      expect(me.email).toBe(email);

      const wrong = await changePassword(client, {
        oldPassword: "OldStrong123!xyz",
        newPassword: "Another123!xyz",
      }).then(
        () => null,
        (error: unknown) => error
      );
      expect(wrong).toBeInstanceOf(ApiError);
      expect((wrong as ApiError).status).toBe(400);

      const fresh = contractClient();
      const signed = await signIn(fresh, { email, password: "NewStrong123!xyz" });
      expect(signed.ok).toBe(true);
      await deactivateAccount(fresh);
    } finally {
      await deactivateAccount(client).catch(() => undefined);
    }
  });
});
