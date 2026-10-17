// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: the pending approvals queue against live Django. A live
// decide test is blocked on NEWFRONT-297 (every decide POST 500s
// server-side); the decide response shape is the same serializer the
// list test proves, and the decide function itself is pinned by unit
// tests. NEWFRONT-189 tracks the server rejecting accept_for_session.
import { describe, expect, it } from "vitest";
import type { ApiClient } from "../client.js";
import { listApprovals } from "../contracts/approvals.js";
import { listProjects } from "../contracts/projects.js";
import { CONTRACT_PROJECT_IDENTIFIER, CONTRACT_WORKSPACE, contractEnabled, sharedSignedIn } from "./setup.js";

async function seededProjectId(client: ApiClient): Promise<string> {
  const projects = await listProjects(client, CONTRACT_WORKSPACE);
  const seeded = projects.find((project) => project.identifier === CONTRACT_PROJECT_IDENTIFIER);
  expect(seeded).toBeDefined();
  return seeded?.id ?? "";
}

describe.skipIf(!contractEnabled)("approvals contract", () => {
  it("lists the caller's pending approvals", async () => {
    const client = await sharedSignedIn();
    const approvals = await listApprovals(client);
    const probe = approvals.find((approval) => approval.reason === "contract approval probe");
    expect(probe).toBeDefined();
    expect(probe?.kind).toBe("command_execution");
    expect(probe?.status).toBe("pending");
    expect(probe?.decision_source).toBe("");
    expect(probe?.decided_at).toBeNull();
  });

  it("scopes the queue to one project", async () => {
    const client = await sharedSignedIn();
    const approvals = await listApprovals(client, await seededProjectId(client));
    expect(approvals.some((approval) => approval.reason === "contract approval probe")).toBe(true);
  });
});
