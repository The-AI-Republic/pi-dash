// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: assistant connections, provider sign-in, and the cloud
// provider config against live cloud Django. Skipped without
// PIDASH_CONTRACT_CLOUD_BASE_URL: a scratch DEBUG cloud backend with
// provider sign-in offered (so the account row seeds) and no live
// providers behind the config test. Mutating tests clean up after
// themselves; the suite signs in once as a fresh dev-login user.
import { describe, expect, it } from "vitest";
import { ApiError } from "../errors.js";
import {
  CREDENTIAL_LANES,
  createAssistantConnection,
  deleteAssistantConnection,
  disconnectAssistantConnection,
  getLlmConfig,
  getOpenAILoginStatus,
  listAssistantConnections,
  resolveCredentialLane,
  resolveOpenhubAvailable,
  selectAssistantConnection,
  startOpenAILogin,
  testLlmConfig,
  updateConnectionModel,
} from "../contracts/assistant.js";
import type { ApiClient } from "../client.js";
import { cloudContractEnabled, sharedCloudSignedIn } from "./setup.js";

async function signedIn(): Promise<ApiClient> {
  return sharedCloudSignedIn();
}

describe.skipIf(!cloudContractEnabled)("assistant cloud contract", () => {
  it("lists connections with the codex row seeded", async () => {
    const list = await listAssistantConnections(await signedIn());
    expect(Array.isArray(list.codex_model_options)).toBe(true);
    expect(typeof list.codex_model_labels).toBe("object");
    expect(typeof list.has_codex_credential).toBe("boolean");
    expect(typeof list.openai_login_available).toBe("boolean");
    expect(Array.isArray(list.openhub_model_options)).toBe(true);
    expect(typeof list.openhub_wallet_url).toBe("string");
    expect(typeof resolveOpenhubAvailable(list)).toBe("boolean");
    const kinds = list.connections.map((row) => row.kind);
    expect(kinds).toContain("codex");
  });

  it("runs a self-supplied connection through its lifecycle", async () => {
    const client = await signedIn();
    const created = await createAssistantConnection(client, {
      providerKind: "openai_compatible",
      baseUrl: "https://openrouter.ai/api/v1",
      modelName: "meta-llama/llama-3.3-70b-instruct",
      apiKey: "contract-cloud-key",
    });
    try {
      expect(created.kind).toBe("byok");
      expect(created.is_selected).toBe(true);
      expect(created.has_api_key).toBe(true);

      const renamed = await updateConnectionModel(client, created.id, "llama-edited");
      expect(renamed.model_name).toBe("llama-edited");

      const selected = await selectAssistantConnection(client, created.id);
      expect(selected.is_selected).toBe(true);

      const list = await listAssistantConnections(client);
      expect(list.connections.map((row) => row.id)).toContain(created.id);
    } finally {
      await deleteAssistantConnection(client, created.id);
    }
    const after = await listAssistantConnections(client);
    expect(after.connections.map((row) => row.id)).not.toContain(created.id);
  });

  it("refuses to select the unbacked account row", async () => {
    const client = await signedIn();
    const list = await listAssistantConnections(client);
    const codexId = list.connections.find((row) => row.kind === "codex")?.id ?? "";
    expect(codexId).not.toBe("");
    const error = await selectAssistantConnection(client, codexId).catch((e: unknown) => e);
    expect(error).toBeInstanceOf(ApiError);
    expect((error as ApiError).status).toBe(400);
    expect((error as ApiError).message).toBe("codex_not_connected");
  });

  it("refuses to disconnect a self-supplied row", async () => {
    const client = await signedIn();
    const created = await createAssistantConnection(client, {
      providerKind: "openai_compatible",
      baseUrl: "https://openrouter.ai/api/v1",
      modelName: "meta-llama/llama-3.3-70b-instruct",
      apiKey: "contract-cloud-key",
    });
    try {
      const error = await disconnectAssistantConnection(client, created.id).catch((e: unknown) => e);
      expect(error).toBeInstanceOf(ApiError);
      expect((error as ApiError).status).toBe(400);
    } finally {
      await deleteAssistantConnection(client, created.id);
    }
  });

  it("starts provider login and polls it as pending", async () => {
    const client = await signedIn();
    const started = await startOpenAILogin(client);
    expect(started.authorize_url.length).toBeGreaterThan(0);
    expect(started.state.length).toBeGreaterThan(0);
    const status = await getOpenAILoginStatus(client, started.state);
    expect(status.status).toBe("pending");
  });

  it("reads the provider config with a valid lane", async () => {
    const config = await getLlmConfig(await signedIn());
    expect(typeof config.provider_kind).toBe("string");
    expect(typeof config.has_api_key).toBe("boolean");
    expect(CREDENTIAL_LANES).toContain(resolveCredentialLane(config));
  });

  it("tests the active config and reads the verdict shape", async () => {
    const result = await testLlmConfig(await signedIn());
    expect(typeof result.ok).toBe("boolean");
    if (!result.ok) expect(typeof result.error_code).toBe("string");
  });
});
