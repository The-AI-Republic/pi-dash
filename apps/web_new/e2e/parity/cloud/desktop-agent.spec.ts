// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-440): desktop agent model reasons, row
// CLOUD-040 (the desktop engine explains cloud-only model requirements),
// green on the cloud-edition old app first. The same file must go green
// on apps/web_new once the cloud profile area lands there.
//
// Observed behavior notes (cloud oracle): the desktop engine runs on the
// hosted lane only, and the profile endpoint classifies each user into
// exactly one reason — a missing configuration, a self-supplied key (the
// desktop never receives stored keys), or the other gateway/session
// states. The endpoint is desktop-session-gated: a browser session is
// refused with the desktop-session reason rather than a bare denial, and
// the credential endpoint answers with the same code the profile would
// show. The instance kill switch surfaces on the project options the web
// app reads. The scratch stack has no model gateway and no desktop OIDC
// client, so gateway-backed codes (revoked session, stale scopes,
// unavailable gateway, unconnected app) and the per-reason fix messages
// — which render in the desktop shell — are out of oracle reach; the
// desktop-session halves below run through a token minted by the same
// issuer the views verify. Same precedent as the DESK oracle rows and
// CLOUD-018/019: the oracle pins the web-observable contract here, and
// the message rendering runs in the desktop build.
import { test, expect } from "../fixtures";
import {
  cloudAgentProfileRaw,
  cloudAgentTokenRaw,
  cloudAssistantCreateByokRaw,
  cloudCsrfRaw,
  cloudDesktopAccessToken,
  cloudDevLogin,
  cloudProjectRaw,
  uniqueEmail,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

function executorOptions(payload: Record<string, unknown>): { kind: string; available: boolean; reason: string }[] {
  const options = payload["agent_executor_options"];
  if (!Array.isArray(options)) return [];
  return options.map((entry) => {
    const row = entry as Record<string, unknown>;
    return {
      kind: typeof row["kind"] === "string" ? row["kind"] : "",
      available: row["available"] === true,
      reason: typeof row["reason_code"] === "string" ? row["reason_code"] : "",
    };
  });
}

test(
  specTitle(["CLOUD-040"], "browser sessions are refused with the desktop-session reason"),
  { tag: specTags(["CLOUD-040"]) },
  async () => {
    const session = await cloudDevLogin(uniqueEmail("parity-440-agent"), "Agent Refused");
    const profile = await cloudAgentProfileRaw(session.cookie);
    expect(profile.status).toBe(403);
    expect(profile.payload["error"]).toBe("desktop_session_required");
    const token = await cloudAgentTokenRaw(session.cookie);
    expect(token.status).toBe(403);
    expect(token.payload["error"]).toBe("desktop_session_required");
  }
);

test(
  specTitle(["CLOUD-040"], "desktop sessions classify missing and self-supplied lanes distinctly"),
  { tag: specTags(["CLOUD-040"]) },
  async () => {
    const email = uniqueEmail("parity-440-agent");
    const session = await cloudDevLogin(email, "Agent Classify");
    const desktopCookie = `pidash_access=${await cloudDesktopAccessToken(email)}`;

    const missing = await cloudAgentProfileRaw(desktopCookie);
    expect(missing.status).toBe(200);
    expect(missing.payload["available"]).toBe(false);
    expect(missing.payload["reason_code"]).toBe("llm_config_missing");
    expect(typeof missing.payload["managed_runner_enabled"]).toBe("boolean");

    const created = await cloudAssistantCreateByokRaw(session.cookie, {
      provider_kind: "openai_compatible",
      base_url: "https://openrouter.ai/api/v1",
      model_name: "meta-llama/llama-3.3-70b-instruct",
      api_key: "parity-440-agent-key",
    });
    expect(created.status).toBe(201);

    const byok = await cloudAgentProfileRaw(desktopCookie);
    expect(byok.status).toBe(200);
    expect(byok.payload["available"]).toBe(false);
    expect(byok.payload["lane"]).toBe("byok");
    expect(byok.payload["reason_code"]).toBe("byok_not_supported_on_desktop");

    // The credential endpoint answers with the same code the profile showed.
    const token = await cloudAgentTokenRaw(desktopCookie);
    expect(token.status).toBe(409);
    expect(token.payload["error"]).toBe("byok_not_supported_on_desktop");
  }
);

test(
  specTitle(["CLOUD-040"], "enablement-off surfaces on the project options web sessions read"),
  { tag: specTags(["CLOUD-040"]) },
  async ({ seed }) => {
    const session = await cloudDevLogin(seed.email);
    const project = await cloudProjectRaw(seed.workspaceSlug, seed.projectId, session.cookie);
    expect(project.status).toBe(200);
    const managed = executorOptions(project.payload).find((entry) => entry.kind === "managed_runner");
    expect(managed).toBeDefined();
    expect(managed?.available).toBe(false);
    expect(managed?.reason).toBe("managed_runner_disabled");
  }
);

test(
  specTitle(["CLOUD-040"], "cloud CSRF path answers for the desktop shell"),
  { tag: specTags(["CLOUD-040"]) },
  async () => {
    const session = await cloudDevLogin(uniqueEmail("parity-440-agent"), "Agent Csrf");
    const csrf = await cloudCsrfRaw(session.cookie);
    expect(csrf.status).toBe(200);
    expect(typeof csrf.payload["csrf_token"]).toBe("string");
    expect((csrf.payload["csrf_token"] as string).length).toBeGreaterThan(0);
  }
);
