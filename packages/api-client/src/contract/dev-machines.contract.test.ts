// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: dev-machine list/rotate/revoke/delete plus the
// create-runner request and status poll against live Django. The
// delete test consumes a seed probe; re-run
// contract/seed-contracts.py before re-running this suite. The 202
// creation path needs a live daemon, so the live tests prove the
// offline rejection and the unknown-request read while the unit
// tests pin the accepted and reported shapes.
import { describe, expect, it } from "vitest";
import type { ApiClient } from "../client.js";
import { ApiError } from "../errors.js";
import {
  createRunnerOnMachine,
  deleteDevMachine,
  getCreateRunnerStatus,
  listDevMachines,
  revokeDevMachine,
  rotateDevMachine,
} from "../contracts/dev-machines.js";
import { listWorkspaces } from "../contracts/workspaces.js";
import { CONTRACT_PROJECT_IDENTIFIER, CONTRACT_WORKSPACE, contractEnabled, sharedSignedIn } from "./setup.js";

async function seededWorkspaceId(client: ApiClient): Promise<string> {
  const workspaces = await listWorkspaces(client);
  const seeded = workspaces.find((workspace) => workspace.slug === CONTRACT_WORKSPACE);
  expect(seeded).toBeDefined();
  return seeded?.id ?? "";
}

describe.skipIf(!contractEnabled)("dev-machines contract", () => {
  it("lists workspace machines with their counts", async () => {
    const client = await sharedSignedIn();
    const machines = await listDevMachines(client, await seededWorkspaceId(client));
    const keeper = machines.find((machine) => machine.label === "contract-machine");
    expect(keeper).toBeDefined();
    expect(keeper?.runner_count).toBeGreaterThanOrEqual(1);
    expect(keeper?.control_online).toBe(false);
    expect(keeper?.revoked_at).toBeNull();
  });

  it("rotates a machine token and keeps the row", async () => {
    const client = await sharedSignedIn();
    const workspaceId = await seededWorkspaceId(client);
    const machines = await listDevMachines(client, workspaceId);
    const probe = machines.find((machine) => machine.label === "contract-machine-rotate");
    expect(probe).toBeDefined();
    const rotated = await rotateDevMachine(client, probe?.id ?? "", workspaceId);
    expect(rotated.id).toBe(probe?.id);
    expect(rotated.revoked_at).toBeNull();
  });

  it("revokes a machine and keeps the row", async () => {
    const client = await sharedSignedIn();
    const workspaceId = await seededWorkspaceId(client);
    const machines = await listDevMachines(client, workspaceId);
    const probe = machines.find((machine) => machine.label === "contract-machine-revoke");
    expect(probe).toBeDefined();
    const revoked = await revokeDevMachine(client, probe?.id ?? "", workspaceId);
    expect(revoked.revoked_at).not.toBeNull();
  });

  it("deletes a machine and drops the row", async () => {
    const client = await sharedSignedIn();
    const workspaceId = await seededWorkspaceId(client);
    const machines = await listDevMachines(client, workspaceId);
    const probe = machines.find((machine) => machine.label === "contract-machine-delete");
    expect(probe).toBeDefined();
    const status = await deleteDevMachine(client, probe?.id ?? "", workspaceId);
    expect(status).toBe(204);
    const after = await listDevMachines(client, workspaceId);
    expect(after.some((machine) => machine.id === probe?.id)).toBe(false);
  });

  it("refuses creation on an offline machine", async () => {
    const client = await sharedSignedIn();
    const workspaceId = await seededWorkspaceId(client);
    const machines = await listDevMachines(client, workspaceId);
    const keeper = machines.find((machine) => machine.label === "contract-machine");
    expect(keeper).toBeDefined();
    const failure = await createRunnerOnMachine(client, keeper?.id ?? "", {
      workspace: workspaceId,
      project: CONTRACT_PROJECT_IDENTIFIER,
    }).catch((error: unknown) => error);
    expect(failure).toBeInstanceOf(ApiError);
    expect((failure as ApiError).status).toBe(409);
    expect((failure as ApiError).message).toBe("machine_offline");
  });

  it("reports unknown creation requests as not-found", async () => {
    const client = await sharedSignedIn();
    const workspaceId = await seededWorkspaceId(client);
    const machines = await listDevMachines(client, workspaceId);
    const keeper = machines.find((machine) => machine.label === "contract-machine");
    expect(keeper).toBeDefined();
    const failure = await getCreateRunnerStatus(
      client,
      keeper?.id ?? "",
      "00000000-0000-0000-0000-000000000000",
      workspaceId
    ).catch((error: unknown) => error);
    expect(failure).toBeInstanceOf(ApiError);
    expect((failure as ApiError).status).toBe(404);
  });
});
