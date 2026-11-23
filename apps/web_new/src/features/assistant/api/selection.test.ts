// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Per-surface selection reads and writes (NEWPIDASH-394): the surface
// the composer writes (desktop build vs browser) and the client-bound
// actions with their endpoint slots.
import { describe, expect, it, vi } from "vitest";

const platformMock = vi.hoisted(() => ({ desktop: false }));
vi.mock("../../../core/platform/index.js", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../../core/platform/index.js")>();
  // Read the flag lazily: the factory runs once at import time, but the
  // tests flip the build per case.
  const platform = new Proxy(actual.platform, {
    get: (target, property, receiver) =>
      property === "agentRuntime" && platformMock.desktop ? {} : Reflect.get(target, property, receiver),
  });
  return { ...actual, platform };
});

import { afterEach } from "vitest";

import type { ApiClient } from "@pidash/api-client";

import { composerSurface, modelSelectionActions } from "./selection.js";

function stubClient(impl: Partial<ApiClient>): ApiClient {
  return {
    parse: ((schema: { parse: (value: unknown) => unknown }, value: unknown) =>
      schema.parse(value)) as ApiClient["parse"],
    ...impl,
  } as ApiClient;
}

afterEach(() => {
  platformMock.desktop = false;
});

describe("composerSurface", () => {
  it("writes web_assistant in the browser build", () => {
    expect(composerSurface()).toBe("web_assistant");
  });

  it("writes desktop_assistant in the desktop build", () => {
    platformMock.desktop = true;
    expect(composerSurface()).toBe("desktop_assistant");
  });
});

describe("modelSelectionActions", () => {
  it("saves through the surface PUT slot", async () => {
    const calls: Array<{ method: string; url: string; json: unknown }> = [];
    const client = stubClient({
      sendJson: (async (method: string, url: string, options: { json?: unknown }) => {
        calls.push({ method, url, json: options.json });
        return {
          data: {
            saved: null,
            effective: { model: "gpt-6", connection_id: "223e4567-e89b-12d3-a456-426614174002" },
            fallback_reason: null,
          },
          status: 200,
          url,
        };
      }) as ApiClient["sendJson"],
    });
    const state = await modelSelectionActions(client).save("web_assistant", {
      model: "gpt-6",
      connectionId: "223e4567-e89b-12d3-a456-426614174002",
    });
    expect(calls).toEqual([
      {
        method: "PUT",
        url: "/api/users/me/ai-assistant/model-selection/web_assistant/",
        json: { model: "gpt-6", connection_id: "223e4567-e89b-12d3-a456-426614174002" },
      },
    ]);
    expect(state.effective).toEqual({ model: "gpt-6", connection_id: "223e4567-e89b-12d3-a456-426614174002" });
  });

  it("clears through the surface DELETE slot", async () => {
    const urls: string[] = [];
    const client = stubClient({
      delete: (async (url: string) => {
        urls.push(url);
        return { status: 204, url, headers: { get: () => null }, text: async () => "" };
      }) as ApiClient["delete"],
    });
    await modelSelectionActions(client).clear("agent");
    expect(urls).toEqual(["/api/users/me/ai-assistant/model-selection/agent/"]);
  });
});
