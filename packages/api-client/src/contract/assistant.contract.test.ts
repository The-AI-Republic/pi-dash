// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: assistant thread listing and the model-config check against
// live Django.
import { describe, expect, it } from "vitest";
import { createAssistantThread, getLlmConfig, listAssistantThreads } from "../contracts/assistant.js";
import { CONTRACT_WORKSPACE, contractEnabled, sharedSignedIn } from "./setup.js";

describe.skipIf(!contractEnabled)("assistant contract", () => {
  it("creates a thread and lists it back newest-first", async () => {
    const client = await sharedSignedIn();
    const title = `Contract thread ${Date.now()}`;
    const created = await createAssistantThread(client, CONTRACT_WORKSPACE, title);
    expect(created.title).toBe(title);
    expect(created.has_active_turn).toBe(false);
    const threads = await listAssistantThreads(client, CONTRACT_WORKSPACE);
    expect(threads.map((thread) => thread.id)).toContain(created.id);
  });

  it("reads the model config, blank until configured", async () => {
    const client = await sharedSignedIn();
    const config = await getLlmConfig(client);
    expect(typeof config.provider_kind).toBe("string");
    expect(typeof config.model_name).toBe("string");
    expect(typeof config.has_api_key).toBe("boolean");
  });
});
