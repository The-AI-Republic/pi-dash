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
// success shapes from representative payloads.
import { describe, expect, it } from "vitest";
import { ApiError } from "../errors.js";
import {
  AGENT_AVAILABILITY_UNKNOWN,
  checkAgentAvailability,
  createDesktopEnroll,
  deleteDesktopEnroll,
} from "../contracts/desktop-runtime.js";
import { CONTRACT_WORKSPACE, contractEnabled, sharedSignedIn } from "./setup.js";

const DESKTOP_REFUSAL = "This endpoint is available to the Pi Dash desktop app.";

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
