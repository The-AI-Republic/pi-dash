// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Desktop profile/token contracts and the gate reader against a
// stub client (no backend — a web session never passes the gate).
import { describe, expect, it } from "vitest";
import type { ApiClient } from "../client.js";
import { ApiError } from "../errors.js";
import { desktopSessionRequiredFrom, getAgentProfile, postAgentToken } from "./assistant-desktop.js";

function stubClient(impl: {
  getJson?: ApiClient["getJson"];
  sendJson?: ApiClient["sendJson"];
  post?: ApiClient["post"];
  delete?: ApiClient["delete"];
}): ApiClient {
  return {
    parse: ((schema: { parse: (value: unknown) => unknown }, value: unknown) =>
      schema.parse(value)) as ApiClient["parse"],
    ...impl,
  } as ApiClient;
}

describe("desktop profile and token", () => {
  it("reads the engine profile", async () => {
    const calls: string[] = [];
    const client = stubClient({
      getJson: (async (url: string) => {
        calls.push(url);
        return {
          managed_runner_enabled: true,
          lane: "openhub",
          base_url: "https://gateway.example.com",
          model: "runner",
          available: true,
          reason_code: "",
          graceful_stop_seconds: 30,
        };
      }) as ApiClient["getJson"],
    });
    const profile = await getAgentProfile(client);
    expect(calls).toEqual(["/api/users/me/ai-assistant/agent-profile/"]);
    expect(profile.available).toBe(true);
  });

  it("mints the token with an empty-body post", async () => {
    const calls: { url?: string; json?: unknown; headers: Record<string, string> | undefined }[] = [];
    const client = stubClient({
      sendJson: (async (
        _method: string,
        url: string,
        options?: { json?: unknown; headers?: Record<string, string> }
      ) => {
        calls.push({ url, json: options?.json, headers: options?.headers });
        return { data: { token: "tok", expires_at: "2026-10-16T15:32:05+00:00" }, status: 200, url };
      }) as ApiClient["sendJson"],
    });
    const token = await postAgentToken(client);
    expect(calls[0]?.url).toBe("/api/users/me/ai-assistant/agent-token/");
    expect(calls[0]?.json).toEqual({});
    expect(calls[0]?.headers).toBeUndefined();
    expect(token.token).toBe("tok");
  });

  it("sends a supplied CSRF token instead of reading the default path", async () => {
    const calls: { headers: Record<string, string> | undefined }[] = [];
    const client = stubClient({
      sendJson: (async (_method: string, url: string, options?: { headers?: Record<string, string> }) => {
        calls.push({ headers: options?.headers });
        return { data: { token: "tok", expires_at: "2026-10-16T15:32:05+00:00" }, status: 200, url };
      }) as ApiClient["sendJson"],
    });
    await postAgentToken(client, { csrfToken: "cloud-csrf" });
    expect(calls[0]?.headers).toEqual({ "X-CSRFTOKEN": "cloud-csrf" });
  });
});

describe("desktopSessionRequiredFrom", () => {
  const refusal = (status: number, body: unknown) => new ApiError({ status, code: "http", message: "x", body });

  it("reads the browser-session refusal", () => {
    expect(
      desktopSessionRequiredFrom(refusal(403, { error: "desktop_session_required", detail: "desktop app" }))
    ).toEqual({ detail: "desktop app" });
    expect(desktopSessionRequiredFrom(refusal(403, { error: "desktop_session_required" }))).toEqual({
      detail: "",
    });
  });

  it("returns null for anything else", () => {
    expect(desktopSessionRequiredFrom(refusal(403, { error: "role_not_allowed" }))).toBeNull();
    expect(desktopSessionRequiredFrom(refusal(403, { nope: true }))).toBeNull();
    expect(desktopSessionRequiredFrom(refusal(401, { error: "desktop_session_required" }))).toBeNull();
    expect(desktopSessionRequiredFrom(new Error("plain"))).toBeNull();
  });
});
