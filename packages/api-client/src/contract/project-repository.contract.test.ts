// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: the project repository binding against live Django. The seed
// stack has no connected git provider, so the two refusal paths are the
// observable ones: bind answers 409 ProviderAccountRequired, and sync on
// an unbound project answers 404. Status and unbind are exercised on the
// seeded project's unbound state; a bound row's shape stays schema-level
// (no provider credentials exist in the contract stack).
import { describe, expect, it } from "vitest";
import { ApiError } from "../errors.js";
import {
  bindProjectRepository,
  getProjectRepository,
  setProjectRepositorySync,
  unbindProjectRepository,
} from "../contracts/project-repository.js";
import { listProjects } from "../contracts/projects.js";
import { CONTRACT_PROJECT_IDENTIFIER, CONTRACT_WORKSPACE, contractEnabled, sharedSignedIn } from "./setup.js";
import type { ApiClient } from "../client.js";

async function signedIn(): Promise<ApiClient> {
  return sharedSignedIn();
}

async function seededProjectId(client: ApiClient): Promise<string> {
  const projects = await listProjects(client, CONTRACT_WORKSPACE);
  const seeded = projects.find((project) => project.identifier === CONTRACT_PROJECT_IDENTIFIER);
  expect(seeded).toBeDefined();
  return seeded?.id ?? "";
}

describe.skipIf(!contractEnabled)("project repository contract", () => {
  it("reads the unbound marker for a project with no binding", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    const status = await getProjectRepository(client, CONTRACT_WORKSPACE, projectId);
    expect(status).toEqual({ bound: false });
  });

  it("refuses a bind with 409 when no provider account is connected", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    const refused = await bindProjectRepository(client, CONTRACT_WORKSPACE, projectId, {
      repo_url: "https://github.com/acme/widgets",
    }).then(
      () => null,
      (caught: unknown) => caught
    );
    expect(refused).toBeInstanceOf(ApiError);
    expect((refused as ApiError).status).toBe(409);
    expect((refused as ApiError).message).toContain("provider account");
  });

  it("refuses an empty repo URL with 400 before touching a provider", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    const refused = await bindProjectRepository(client, CONTRACT_WORKSPACE, projectId, { repo_url: "  " }).then(
      () => null,
      (caught: unknown) => caught
    );
    expect(refused).toBeInstanceOf(ApiError);
    expect((refused as ApiError).status).toBe(400);
  });

  it("refuses the sync switch with 404 while the project is unbound", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    const refused = await setProjectRepositorySync(client, CONTRACT_WORKSPACE, projectId, true).then(
      () => null,
      (caught: unknown) => caught
    );
    expect(refused).toBeInstanceOf(ApiError);
    expect((refused as ApiError).status).toBe(404);
  });

  it("answers the unbound marker on unbind even when nothing was bound", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    await expect(unbindProjectRepository(client, CONTRACT_WORKSPACE, projectId)).resolves.toEqual({ bound: false });
  });
});
