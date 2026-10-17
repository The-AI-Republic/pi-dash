// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: the desktop engine endpoints refuse browser sessions
// with the desktop gate. Success shapes stay unit-stubbed — only a
// desktop-marked session passes the gate, and the contract stack
// signs in over the web flow.
import { describe, expect, it } from "vitest";
import { desktopSessionRequiredFrom, getAgentProfile, postAgentToken } from "../contracts/assistant-desktop.js";
import { contractEnabled, sharedSignedIn } from "./setup.js";

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
