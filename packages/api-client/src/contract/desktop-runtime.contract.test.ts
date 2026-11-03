// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: desktop enrollment endpoints against live Django. The
// profile read and token mint are covered by
// assistant-desktop.contract.test.ts (NEWFRONT-577, merged first).
// Enrollment is desktop-only, and the open-source build mints no
// desktop session over HTTP (the session marker the gate reads is
// stamped by the desktop flow itself; the cloud overlay stamps an
// OIDC claim instead). So this suite proves the refusal a browser
// session gets, while the stub tests in contracts.test.ts prove the
// success shapes from representative payloads. The live success legs
// below ride the seeded desktop session (NEWFRONT-585).
import { describe, expect, it } from "vitest";
import { ApiError } from "../errors.js";
import {
  AGENT_AVAILABILITY_UNKNOWN,
  checkAgentAvailability,
  createDesktopEnroll,
  deleteDesktopEnroll,
} from "../contracts/desktop-runtime.js";
import { CONTRACT_WORKSPACE, contractEnabled, sharedDesktopSignedIn, sharedSignedIn } from "./setup.js";

const DESKTOP_REFUSAL = "This endpoint is available to the Pi Dash desktop app.";
const HOST_LABEL = "contract-desktop";

async function refusal(call: Promise<unknown>): Promise<ApiError> {
  const error = await call.catch((e: unknown) => e);
  expect(error).toBeInstanceOf(ApiError);
  return error as ApiError;
}

describe.skipIf(!contractEnabled)("desktop runtime contract", () => {
  it("refuses desktop enrollment for a browser session", async () => {
    const client = await sharedSignedIn();
    // Permission precedes validation: even a well-formed body is refused.
    const error = await refusal(
      createDesktopEnroll(client, { workspaceSlug: CONTRACT_WORKSPACE, hostLabel: "contract-probe" })
    );
    expect(error.status).toBe(403);
    expect(error.message).toBe(DESKTOP_REFUSAL);
  });

  it("refuses enrollment deletion for a browser session", async () => {
    const client = await sharedSignedIn();
    const error = await refusal(deleteDesktopEnroll(client, "contract-probe"));
    expect(error.status).toBe(403);
    expect(error.message).toBe(DESKTOP_REFUSAL);
  });

  it("reports availability from the refusal without throwing", async () => {
    const client = await sharedSignedIn();
    await expect(checkAgentAvailability(client)).resolves.toEqual({
      available: false,
      reasonCode: AGENT_AVAILABILITY_UNKNOWN,
    });
  });
});

describe.skipIf(!contractEnabled)("desktop enrollment success contract", () => {
  it("enrolls a machine, rotates on re-enroll, and removes it", async () => {
    const client = await sharedDesktopSignedIn();
    const first = await createDesktopEnroll(client, {
      workspaceSlug: CONTRACT_WORKSPACE,
      hostLabel: HOST_LABEL,
    });
    expect(first.machine_token.length).toBeGreaterThan(0);
    expect(first.workspace_slug).toBe(CONTRACT_WORKSPACE);
    expect(typeof first.managed_runner_enabled).toBe("boolean");
    expect(typeof first.graceful_stop_seconds).toBe("number");
    const second = await createDesktopEnroll(client, {
      workspaceSlug: CONTRACT_WORKSPACE,
      hostLabel: HOST_LABEL,
    });
    expect(second.dev_machine_id).toBe(first.dev_machine_id);
    expect(second.machine_token).not.toBe(first.machine_token);
    await expect(deleteDesktopEnroll(client, HOST_LABEL)).resolves.toBeUndefined();
    // Removing twice stays quiet: sign-out teardown never trips on a rerun.
    await expect(deleteDesktopEnroll(client, HOST_LABEL)).resolves.toBeUndefined();
  });

  it("refuses enrollment in a workspace you are not in", async () => {
    const client = await sharedDesktopSignedIn();
    const error = await refusal(
      createDesktopEnroll(client, { workspaceSlug: "someone-elses", hostLabel: HOST_LABEL })
    );
    expect(error.status).toBe(404);
  });
});
