// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the space board-resolution contracts: every function
// parses a representative payload through a stub transport and requests
// the anchor-scoped path. Live-server proof lives in
// contract/space-board.contract.test.ts.
import { describe, expect, it } from "vitest";
import { createClient } from "../client.js";
import { fakeResponse, recordingTransport } from "../client.test.js";
import { getSpaceBoardMeta, getSpaceBoardSettings, resolveSpaceBoardAnchor } from "./space-board.js";

const UUID = "123e4567-e89b-12d3-a456-426614174000";
const UUID2 = "223e4567-e89b-12d3-a456-426614174001";
const UUID3 = "323e4567-e89b-12d3-a456-426614174002";
const DT = "2026-10-20T12:00:00.000Z";

const settings = {
  id: UUID,
  project_details: {
    id: UUID2,
    identifier: "CT",
    name: "Contract",
    cover_image: null,
    cover_image_url: null,
    logo_props: {},
    description: "Board",
    is_default: true,
  },
  workspace_detail: { name: "Acme", slug: "acme", id: UUID3, logo_url: null },
  created_at: DT,
  updated_at: DT,
  deleted_at: null,
  created_by: null,
  updated_by: null,
  workspace: UUID3,
  project: UUID2,
  entity_identifier: UUID2,
  entity_name: "project",
  anchor: "anchor123",
  is_comments_enabled: true,
  is_reactions_enabled: false,
  is_votes_enabled: true,
  view_props: { list: true, kanban: true },
  is_activity_enabled: false,
  is_disabled: false,
  intake: null,
};

function clientFor(value: unknown) {
  const transport = recordingTransport((call) =>
    call.url.endsWith("/auth/get-csrf-token/")
      ? fakeResponse({ body: JSON.stringify({ csrf_token: "csrf" }) })
      : fakeResponse({ body: JSON.stringify(value) })
  );
  return { client: createClient({ baseUrl: "https://api.test", transport }), transport };
}

describe("space board contracts", () => {
  it("reads settings with the publish payload, layouts and engagement flags", async () => {
    const { client, transport } = clientFor(settings);
    const parsed = await getSpaceBoardSettings(client, "anchor123");
    expect(parsed.project_details?.name).toBe("Contract");
    expect(parsed.view_props).toEqual({ list: true, kanban: true });
    expect(parsed.is_comments_enabled).toBe(true);
    expect(transport.calls.at(-1)?.url).toBe("https://api.test/api/public/anchor/anchor123/settings/");
  });

  it("accepts a settings payload whose project details are null", async () => {
    const { client } = clientFor({ ...settings, project_details: null, anchor: null });
    const parsed = await getSpaceBoardSettings(client, "anchor123");
    expect(parsed.project_details).toBeNull();
    expect(parsed.anchor).toBeNull();
  });

  it("settles a missing project_details key to null (SPC-010 branch 4)", async () => {
    const { project_details: _dropped, ...withoutDetails } = settings;
    expect("project_details" in withoutDetails).toBe(false);
    const { client } = clientFor(withoutDetails);
    const parsed = await getSpaceBoardSettings(client, "anchor123");
    expect(parsed.project_details).toBeNull();
    expect(parsed.anchor).toBe("anchor123");
  });

  it("settles malformed project details to null instead of failing the read", async () => {
    const malformed = [
      { ...settings, project_details: { ...settings.project_details, logo_props: "not-a-map" } },
      { ...settings, project_details: { ...settings.project_details, name: undefined } },
      { ...settings, project_details: "not-an-object" },
    ];
    for (const payload of malformed) {
      const { client } = clientFor(payload);
      const parsed = await getSpaceBoardSettings(client, "anchor123");
      expect(parsed.project_details).toBeNull();
    }
  });

  it("reads board metadata with optional fields", async () => {
    const { client, transport } = clientFor({
      id: UUID2,
      identifier: "CT",
      name: "Contract",
      description: "Board",
      cover_image: null,
      icon_prop: null,
      emoji: null,
    });
    const parsed = await getSpaceBoardMeta(client, "anchor123");
    expect(parsed.name).toBe("Contract");
    expect(transport.calls.at(-1)?.url).toBe("https://api.test/api/public/anchor/anchor123/meta/");
  });

  it("resolves a legacy address to the board anchor", async () => {
    const { client, transport } = clientFor(settings);
    const parsed = await resolveSpaceBoardAnchor(client, "acme", UUID2);
    expect(parsed.anchor).toBe("anchor123");
    expect(transport.calls.at(-1)?.url).toBe(`https://api.test/api/public/workspaces/acme/projects/${UUID2}/anchor/`);
  });

  it("rejects a payload without the engagement flags", async () => {
    const { client } = clientFor({ ...settings, is_comments_enabled: undefined });
    await expect(getSpaceBoardSettings(client, "anchor123")).rejects.toThrow();
  });
});
