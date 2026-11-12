// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: the desktop engine endpoints refuse browser sessions
// with the desktop gate. Success legs ride the seeded desktop session
// (NEWFRONT-585): the profile shape parses live, and the CE token mint
// refuses 409. Shapes stay stub-covered in unit tests too.
import { describe, expect, it } from "vitest";
import { ApiError } from "../errors.js";
import { desktopSessionRequiredFrom, getAgentProfile, postAgentToken } from "../contracts/assistant-desktop.js";
import { contractEnabled, sharedDesktopSignedIn, sharedSignedIn } from "./setup.js";

describe.skipIf(!contractEnabled)("assistant desktop contract", () => {
  it("refuses the profile read from a browser session", async () => {
    const client = await sharedSignedIn();
    const refused: unknown = await getAgentProfile(client).then(
      () => null,
      (error: unknown) => error
    );
    const gate = desktopSessionRequiredFrom(refused);
    expect(gate).not.toBeNull();
    expect(gate?.detail).toContain("desktop");
  });

  it("refuses the token mint from a browser session", async () => {
    const client = await sharedSignedIn();
    const refused: unknown = await postAgentToken(client).then(
      () => null,
      (error: unknown) => error
    );
    expect(desktopSessionRequiredFrom(refused)).not.toBeNull();
  });
});

describe.skipIf(!contractEnabled)("assistant desktop success contract", () => {
  it("reads the agent profile without a credential", async () => {
    const client = await sharedDesktopSignedIn();
    const profile = await getAgentProfile(client);
    expect(typeof profile.managed_runner_enabled).toBe("boolean");
    expect(typeof profile.lane).toBe("string");
    expect(typeof profile.base_url).toBe("string");
    expect(typeof profile.model).toBe("string");
    expect(typeof profile.available).toBe("boolean");
    expect(typeof profile.reason_code).toBe("string");
    expect(typeof profile.graceful_stop_seconds).toBe("number");
    if (profile.available) expect(profile.reason_code).toBe("");
    else expect(profile.reason_code.length).toBeGreaterThan(0);
    // The credential travels by its own endpoint, never in this pollable body.
    expect("token" in profile).toBe(false);
  });

  it("refuses the agent token where the build has no lane", async () => {
    // CE has no lane the desktop may use: the mint answers 409 carrying
    // the refusal code, never a blank token that fails later inside a run.
    const client = await sharedDesktopSignedIn();
    const failure = await postAgentToken(client).catch((error: unknown) => error);
    expect(failure).toBeInstanceOf(ApiError);
    expect((failure as ApiError).status).toBe(409);
  });
});
