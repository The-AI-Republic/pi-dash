// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { afterEach, describe, expect, it, vi } from "vitest";

const platformHarness = vi.hoisted(() => ({
  shareableUrl: (path: string): string => `https://app.test${path}`,
}));

vi.mock("../../../core/platform/index.js", () => ({
  get platform() {
    return platformHarness;
  },
}));

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { CloudViewAccessField } from "./CloudViewAccessField.js";
import {
  CLOUD_WORKSPACE_VIEW_LAYOUTS,
  cloudViewAccessSlot,
  cloudViewPublishSlot,
  cloudWorkspaceViewLayouts,
  publishedViewPath,
  VIEW_ACCESS_STORE_FAILED,
  VIEW_PUBLISH_FAILED,
  VIEW_UNPUBLISH_FAILED,
} from "./cloudSlots.js";

const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const VIEW_ID = "323e4567-e89b-12d3-a456-426614174002";
const OWNER = "423e4567-e89b-12d3-a456-426614174003";
const WORKSPACE_ID = "523e4567-e89b-12d3-a456-426614174004";

const RECORD = {
  id: VIEW_ID,
  workspace: WORKSPACE_ID,
  project: PROJECT,
  name: "Triage",
  description: "",
  query: {},
  filters: {},
  display_filters: { layout: "list" },
  display_properties: {},
  rich_filters: {},
  access: 1,
  sort_order: 1,
  logo_props: {},
  owned_by: OWNER,
  is_locked: false,
  archived_at: null,
  created_by: OWNER,
  updated_by: OWNER,
  created_at: "2026-01-01T00:00:00Z",
  updated_at: "2026-01-01T00:00:00Z",
};

interface Call {
  method: string;
  url: string;
  body: Record<string, unknown> | null;
}

function transport(respond: (call: Call) => { status: number; body: unknown }): Call[] {
  const calls: Call[] = [];
  createAppClient("https://api.test", (async (url: string, init: { method?: string; body?: string }) => {
    if (url.endsWith("/auth/get-csrf-token/")) {
      return { status: 200, url, headers: { get: () => null }, text: async () => '{"csrf_token":"csrf"}' };
    }
    const call: Call = {
      method: init.method ?? "GET",
      url,
      body: init.body === undefined ? null : (JSON.parse(init.body) as Record<string, unknown>),
    };
    calls.push(call);
    const { status, body } = respond(call);
    return { status, url, headers: { get: () => null }, text: async () => JSON.stringify(body) };
  }) as never);
  return calls;
}

afterEach(() => {
  resetAppClient();
});

describe("cloudViewAccessSlot", () => {
  it("carries the selector and the storing write", () => {
    expect(cloudViewAccessSlot.Field).toBe(CloudViewAccessField);
    expect(typeof cloudViewAccessSlot.storeAccess).toBe("function");
  });

  it("stores a project access choice and resolves the echo", async () => {
    const calls = transport(() => ({ status: 200, body: { ...RECORD, access: 0 } }));
    const echo = await cloudViewAccessSlot.storeAccess({
      workspaceSlug: "acme",
      projectId: PROJECT,
      viewId: VIEW_ID,
      access: 0,
    });
    expect(calls.map((call) => `${call.method} ${call.url}`)).toEqual([
      `PATCH https://api.test/api/workspaces/acme/projects/${PROJECT}/views/${VIEW_ID}/`,
    ]);
    expect(calls[0]?.body).toEqual({ access: 0 });
    expect((echo as typeof RECORD).access).toBe(0);
  });

  it("stores a workspace access choice on the workspace route", async () => {
    const calls = transport(() => ({ status: 200, body: { ...RECORD, project: null, access: 0 } }));
    await cloudViewAccessSlot.storeAccess({ workspaceSlug: "acme", projectId: null, viewId: VIEW_ID, access: 0 });
    expect(calls.map((call) => `${call.method} ${call.url}`)).toEqual([
      `PATCH https://api.test/api/workspaces/acme/views/${VIEW_ID}/`,
    ]);
    expect(calls[0]?.body).toEqual({ access: 0 });
  });

  it("fails loudly when the echo drops the choice", async () => {
    transport(() => ({ status: 200, body: { ...RECORD, access: 1 } }));
    await expect(
      cloudViewAccessSlot.storeAccess({ workspaceSlug: "acme", projectId: PROJECT, viewId: VIEW_ID, access: 0 })
    ).rejects.toThrow(VIEW_ACCESS_STORE_FAILED);
  });
});

describe("cloudViewPublishSlot", () => {
  it("carries the anchor writes and the link derivation", () => {
    expect(typeof cloudViewPublishSlot.publish).toBe("function");
    expect(typeof cloudViewPublishSlot.unpublish).toBe("function");
    expect(typeof cloudViewPublishSlot.publicLink).toBe("function");
  });

  it("publishes under a fresh token the echo confirms", async () => {
    const calls = transport((call) => ({
      status: 200,
      body: { ...RECORD, anchor: (call.body as { anchor: string }).anchor },
    }));
    const result = await cloudViewPublishSlot.publish({ workspaceSlug: "acme", projectId: PROJECT, viewId: VIEW_ID });
    expect(calls.map((call) => `${call.method} ${call.url}`)).toEqual([
      `PATCH https://api.test/api/workspaces/acme/projects/${PROJECT}/views/${VIEW_ID}/`,
    ]);
    const proposed = (calls[0]!.body as { anchor: string }).anchor;
    expect(typeof proposed).toBe("string");
    expect(proposed).not.toBe("");
    expect(result).toEqual({ anchor: proposed });
  });

  it("fails loudly when the echo carries no anchor", async () => {
    transport(() => ({ status: 200, body: { ...RECORD } }));
    await expect(
      cloudViewPublishSlot.publish({ workspaceSlug: "acme", projectId: PROJECT, viewId: VIEW_ID })
    ).rejects.toThrow(VIEW_PUBLISH_FAILED);
  });

  it("unpublishes by clearing the anchor", async () => {
    const calls = transport(() => ({ status: 200, body: { ...RECORD, anchor: null } }));
    await cloudViewPublishSlot.unpublish({ workspaceSlug: "acme", projectId: PROJECT, viewId: VIEW_ID });
    expect(calls.map((call) => `${call.method} ${call.url}`)).toEqual([
      `PATCH https://api.test/api/workspaces/acme/projects/${PROJECT}/views/${VIEW_ID}/`,
    ]);
    expect(calls[0]?.body).toEqual({ anchor: null });
  });

  it("fails loudly when the anchor stays live", async () => {
    transport(() => ({ status: 200, body: { ...RECORD, anchor: "still-live" } }));
    await expect(
      cloudViewPublishSlot.unpublish({ workspaceSlug: "acme", projectId: PROJECT, viewId: VIEW_ID })
    ).rejects.toThrow(VIEW_UNPUBLISH_FAILED);
  });

  it("derives the public link through the active platform", () => {
    expect(cloudViewPublishSlot.publicLink("view-anchor-1")).toBe("https://app.test/views/published/view-anchor-1/");
  });
});

describe("cloudWorkspaceViewLayouts", () => {
  it("offers the surface's known layouts", () => {
    expect(CLOUD_WORKSPACE_VIEW_LAYOUTS).toEqual(["list", "spreadsheet"]);
    expect(cloudWorkspaceViewLayouts.layouts).toEqual(["list", "spreadsheet"]);
  });
});

describe("publishedViewPath", () => {
  it("addresses the anchor's public page", () => {
    expect(publishedViewPath("view-anchor-1")).toBe("/views/published/view-anchor-1/");
  });

  it("escapes anchor characters", () => {
    expect(publishedViewPath("a/b?c")).toBe("/views/published/a%2Fb%3Fc/");
  });
});
