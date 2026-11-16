// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: home endpoints (dashboard, preferences, quick links,
// recent visits, tour flag) against live Django.
import { beforeAll, describe, expect, it } from "vitest";
import { signIn } from "../contracts/auth-entry.js";
import {
  createQuickLink,
  deleteQuickLink,
  getQuickLink,
  getWorkspaceDashboard,
  listHomePreferences,
  listQuickLinks,
  listRecentVisits,
  updateHomePreference,
  updateQuickLink,
} from "../contracts/home.js";
import { completeTour } from "../contracts/tour.js";
import { CONTRACT_EMAIL, CONTRACT_PASSWORD, CONTRACT_WORKSPACE, contractClient, contractEnabled } from "./setup.js";
import type { ApiClient } from "../client.js";

async function signedIn(): Promise<ApiClient> {
  const client = contractClient();
  const result = await signIn(client, { email: CONTRACT_EMAIL, password: CONTRACT_PASSWORD });
  expect(result.ok).toBe(true);
  return client;
}

// One session per file: the scratch stack throttles anonymous hits
// (CSRF fetch + sign-in POST) to 30/minute per IP, so per-test sign-ins
// would 429 each other when files run together.
let client: ApiClient;

beforeAll(async () => {
  client = await signedIn();
});

describe.skipIf(!contractEnabled)("home dashboard contract", () => {
  it("reads the workspace dashboard stats", async () => {
    const dashboard = await getWorkspaceDashboard(client, CONTRACT_WORKSPACE);
    expect(dashboard.assigned_issues_count).toBeGreaterThanOrEqual(0);
    expect(dashboard.pending_issues_count).toBeGreaterThanOrEqual(0);
    expect(dashboard.completed_issues_count).toBeGreaterThanOrEqual(0);
    expect(Array.isArray(dashboard.issue_activities)).toBe(true);
    expect(Array.isArray(dashboard.state_distribution)).toBe(true);
  });
});

describe.skipIf(!contractEnabled)("home preferences contract", () => {
  it("lists preferences and flips one flag back", async () => {
    const preferences = await listHomePreferences(client, CONTRACT_WORKSPACE);
    const recents = preferences.find((preference) => preference.key === "recents");
    expect(recents).toBeDefined();
    expect(preferences.some((preference) => preference.key === "quick_links")).toBe(true);

    const wasEnabled = recents?.is_enabled ?? false;
    const updated = await updateHomePreference(client, CONTRACT_WORKSPACE, "recents", {
      is_enabled: !wasEnabled,
    });
    expect(updated.key).toBe("recents");
    expect(updated.is_enabled).toBe(!wasEnabled);

    const restored = await updateHomePreference(client, CONTRACT_WORKSPACE, "recents", {
      is_enabled: wasEnabled,
    });
    expect(restored.is_enabled).toBe(wasEnabled);
  });
});

describe.skipIf(!contractEnabled)("quick links contract", () => {
  it("creates, reads, updates, lists and deletes a link", async () => {
    const created = await createQuickLink(client, CONTRACT_WORKSPACE, {
      title: "Contract link",
      url: "https://contract.example.com/docs",
    });
    expect(created.title).toBe("Contract link");
    expect(created.url).toBe("https://contract.example.com/docs");

    const read = await getQuickLink(client, CONTRACT_WORKSPACE, created.id);
    expect(read.url).toBe(created.url);

    const updated = await updateQuickLink(client, CONTRACT_WORKSPACE, created.id, {
      title: "Contract link renamed",
    });
    expect(updated.title).toBe("Contract link renamed");

    const listed = await listQuickLinks(client, CONTRACT_WORKSPACE);
    expect(listed.some((link) => link.id === created.id)).toBe(true);

    await expect(deleteQuickLink(client, CONTRACT_WORKSPACE, created.id)).resolves.toBe(204);
    const afterDelete = await listQuickLinks(client, CONTRACT_WORKSPACE);
    expect(afterDelete.some((link) => link.id === created.id)).toBe(false);
  });
});

describe.skipIf(!contractEnabled)("recent visits contract", () => {
  it("lists visits, optionally narrowed by entity", async () => {
    const visits = await listRecentVisits(client, CONTRACT_WORKSPACE);
    expect(Array.isArray(visits)).toBe(true);
    const issues = await listRecentVisits(client, CONTRACT_WORKSPACE, "issue");
    expect(issues.every((visit) => visit.entity_name === "issue")).toBe(true);
  });
});

describe.skipIf(!contractEnabled)("tour flag contract", () => {
  it("flips the tour flag and restores it", async () => {
    await expect(completeTour(client, { is_tour_completed: true })).resolves.toEqual({
      message: "Updated successfully",
    });
    await expect(completeTour(client, { is_tour_completed: false })).resolves.toEqual({
      message: "Updated successfully",
    });
  });
});
