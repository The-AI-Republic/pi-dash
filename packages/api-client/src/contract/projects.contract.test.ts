// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: projects, states, labels and members against live Django.
import { describe, expect, it } from "vitest";
import { listProjects } from "../contracts/projects.js";
import { listLabels, listMembers, listStates } from "../contracts/reference.js";
import { signIn } from "../contracts/auth.js";
import {
  CONTRACT_EMAIL,
  CONTRACT_PASSWORD,
  CONTRACT_PROJECT_IDENTIFIER,
  CONTRACT_WORKSPACE,
  contractClient,
  contractEnabled,
} from "./setup.js";
import type { ApiClient } from "../client.js";

async function signedIn(): Promise<ApiClient> {
  const client = contractClient();
  const result = await signIn(client, { email: CONTRACT_EMAIL, password: CONTRACT_PASSWORD });
  expect(result.ok).toBe(true);
  return client;
}

async function seededProjectId(client: ApiClient): Promise<string> {
  const projects = await listProjects(client, CONTRACT_WORKSPACE);
  const seeded = projects.find((project) => project.identifier === CONTRACT_PROJECT_IDENTIFIER);
  expect(seeded).toBeDefined();
  return seeded?.id ?? "";
}

describe.skipIf(!contractEnabled)("projects contract", () => {
  it("lists workspace projects", async () => {
    const projects = await listProjects(await signedIn(), CONTRACT_WORKSPACE);
    expect(projects.some((project) => project.identifier === CONTRACT_PROJECT_IDENTIFIER)).toBe(true);
  });
});

describe.skipIf(!contractEnabled)("reference contracts", () => {
  it("lists project states", async () => {
    const client = await signedIn();
    const states = await listStates(client, CONTRACT_WORKSPACE, await seededProjectId(client));
    expect(states.length).toBeGreaterThan(0);
    expect(states.some((state) => state.default)).toBe(true);
  });

  it("lists project labels", async () => {
    const client = await signedIn();
    const labels = await listLabels(client, CONTRACT_WORKSPACE, await seededProjectId(client));
    expect(labels.some((label) => label.name === "contract-bug")).toBe(true);
  });

  it("lists project members", async () => {
    const client = await signedIn();
    const members = await listMembers(client, CONTRACT_WORKSPACE, await seededProjectId(client));
    expect(members.length).toBeGreaterThan(0);
    expect(members.some((member) => member.role === 20)).toBe(true);
  });
});
