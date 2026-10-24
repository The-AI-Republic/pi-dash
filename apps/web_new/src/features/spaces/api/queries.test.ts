// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { createClient, type Transport } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import {
  readSpaceViewer,
  resolveLegacySpaceAnchor,
  spaceInstanceQueryOptions,
  spaceIssuesQueryOptions,
  spaceMetaQueryOptions,
  spaceSettingsQueryOptions,
  spaceStatesQueryOptions,
  spaceViewerQueryOptions,
} from "./queries.js";

function transportOf(status: number, body: unknown): Transport {
  return async (url) => ({
    status,
    url,
    headers: { get: () => "application/json" },
    text: () => Promise.resolve(JSON.stringify(body)),
  });
}

function clientWith(transport: Transport) {
  return createClient({ baseUrl: "https://api.test", transport });
}

const UUID = "11111111-1111-4111-8111-111111111111";

describe("space query options", () => {
  it("probes the instance once with no retries", () => {
    const options = spaceInstanceQueryOptions(clientWith(transportOf(200, {})));
    expect(options.queryKey).toEqual(["spaces", "instance"]);
    expect(options.retry).toBe(false);
    expect(options.staleTime).toBe(Number.POSITIVE_INFINITY);
  });

  it("gates settings on a valid anchor and keys every read by anchor", () => {
    const client = clientWith(transportOf(200, {}));
    const valid = spaceSettingsQueryOptions(client, "anchor-1");
    expect(valid.queryKey).toEqual(["spaces", "anchor", "anchor-1", "settings"]);
    expect(valid.enabled).toBe(true);
    expect(spaceSettingsQueryOptions(client, "bad anchor").enabled).toBe(false);
    expect(spaceIssuesQueryOptions(client, "anchor-1").queryKey).toEqual(["spaces", "anchor", "anchor-1", "issues"]);
    expect(spaceStatesQueryOptions(client, "anchor-1").queryKey).toEqual(["spaces", "anchor", "anchor-1", "states"]);
  });

  it("keys board metadata by anchor and never requests a malformed one", async () => {
    const meta = { name: "Public board", description: "Team roadmap" };
    const valid = spaceMetaQueryOptions(clientWith(transportOf(200, meta)), "anchor-1");
    expect(valid.queryKey).toEqual(["spaces", "anchor", "anchor-1", "meta"]);
    expect(valid.enabled).toBe(true);
    expect(valid.retry).toBe(false);
    await expect(valid.queryFn!({} as never)).resolves.toMatchObject(meta);
    expect(spaceMetaQueryOptions(clientWith(transportOf(200, meta)), "bad anchor").enabled).toBe(false);
  });

  it("reads the viewer as anonymous on 401 and on any other failure", async () => {
    await expect(readSpaceViewer(clientWith(transportOf(401, { detail: "no session" })))).resolves.toBeNull();
    await expect(readSpaceViewer(clientWith(transportOf(500, { detail: "down" })))).resolves.toBeNull();
  });

  it("keys the viewer read under the public root and resolves anonymously", async () => {
    const options = spaceViewerQueryOptions(clientWith(transportOf(401, { detail: "no session" })));
    expect(options.queryKey).toEqual(["spaces", "viewer"]);
    expect(options.retry).toBe(false);
    expect(options.staleTime).toBe(60_000);
    await expect(options.queryFn!({} as never)).resolves.toBeNull();
  });

  it("returns the signed-in viewer", async () => {
    const me = {
      id: UUID,
      email: "ada@example.com",
      username: "ada",
      first_name: "Ada",
      last_name: "Lovelace",
      display_name: "Ada Lovelace",
      avatar: "",
      avatar_url: null,
      cover_image: null,
      cover_image_url: null,
      date_joined: "2026-01-01T00:00:00Z",
      user_timezone: "UTC",
      is_active: true,
      is_bot: false,
      is_email_verified: true,
      is_password_autoset: false,
      last_login_medium: "password",
      last_login_time: null,
    };
    await expect(readSpaceViewer(clientWith(transportOf(200, me)))).resolves.toMatchObject({
      email: "ada@example.com",
    });
  });
});

describe("resolveLegacySpaceAnchor", () => {
  const settings = {
    id: UUID,
    project_details: null,
    workspace_detail: { name: "Acme", slug: "acme", id: UUID, logo_url: null },
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    deleted_at: null,
    created_by: null,
    updated_by: null,
    workspace: UUID,
    project: UUID,
    entity_identifier: "PROJ",
    entity_name: "project",
    anchor: "anchor-1",
    is_comments_enabled: false,
    is_reactions_enabled: false,
    is_votes_enabled: false,
    view_props: {},
    is_disabled: false,
    intake: null,
  };

  it("returns a valid project anchor", async () => {
    await expect(resolveLegacySpaceAnchor(clientWith(transportOf(200, settings)), "acme", UUID)).resolves.toBe(
      "anchor-1"
    );
  });

  it("refuses a non-project entity", async () => {
    await expect(
      resolveLegacySpaceAnchor(clientWith(transportOf(200, { ...settings, entity_name: "page" })), "acme", UUID)
    ).resolves.toBeNull();
  });

  it("refuses a missing or malformed anchor", async () => {
    for (const anchor of [null, "", "bad anchor", "a/b"]) {
      await expect(
        resolveLegacySpaceAnchor(clientWith(transportOf(200, { ...settings, anchor })), "acme", UUID)
      ).resolves.toBeNull();
    }
  });

  it("turns every server refusal into a generic miss", async () => {
    await expect(resolveLegacySpaceAnchor(clientWith(transportOf(404, {})), "acme", UUID)).resolves.toBeNull();
    await expect(resolveLegacySpaceAnchor(clientWith(transportOf(500, {})), "acme", UUID)).resolves.toBeNull();
  });
});
