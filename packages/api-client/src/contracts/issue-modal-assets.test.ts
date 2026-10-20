// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract tests for the create/edit modal's editor helpers: the
// mention search and the description asset lifecycle (grant, bytes,
// complete, link). Each call is driven through a recording transport so
// the method, path and body are pinned.
import { describe, expect, it, vi } from "vitest";

import { createClient } from "../client.js";
import { ApiError } from "../errors.js";
import { fakeResponse, recordingTransport } from "../client.test.js";
import { searchMentionPeople } from "./entity-search.js";
import {
  DRAFT_DESCRIPTION_ENTITY_TYPE,
  ISSUE_DESCRIPTION_ENTITY_TYPE,
  attachDescriptionAssets,
  requestDescriptionAsset,
  uploadDescriptionAsset,
} from "./issue-description-assets.js";

const SLUG = "acme";
const PROJECT = "123e4567-e89b-12d3-a456-426614174000";

function json(value: unknown, status = 200) {
  return recordingTransport((call) =>
    call.url.endsWith("/auth/get-csrf-token/")
      ? fakeResponse({ body: JSON.stringify({ csrf_token: "csrf" }) })
      : fakeResponse({ status, body: JSON.stringify(value) })
  );
}

function clientFor(value: unknown, status = 200) {
  return createClient({ baseUrl: "https://api.test", transport: json(value, status) });
}

function file(name = "shot.png"): File {
  return new File(["bytes"], name, { type: "image/png" });
}

describe("mention search", () => {
  it("asks for people in the editor's project and returns their names", async () => {
    const transport = json({
      user_mention: [
        { member__id: "u-1", member__display_name: "Ada", member__avatar_url: null },
        { member__id: "u-2", member__display_name: "Grace", member__avatar_url: "https://cdn.test/g.png" },
      ],
      issue: [{ ignored: true }],
    });
    const client = createClient({ baseUrl: "https://api.test", transport });

    const people = await searchMentionPeople(client, SLUG, { query: "ad", projectId: PROJECT, count: 5 });

    expect(people.map((person) => person.member__display_name)).toEqual(["Ada", "Grace"]);
    const call = transport.calls.find((entry) => entry.url.includes("/entity-search/"));
    expect(call?.init.method).toBe("GET");
    expect(call?.url).toContain(`/api/workspaces/${SLUG}/entity-search/`);
    expect(call?.url).toContain("query=ad");
    expect(call?.url).toContain("query_type=user_mention");
    expect(call?.url).toContain(`project_id=${PROJECT}`);
    expect(call?.url).toContain("count=5");
  });

  it("reports no people when the envelope omits the section", async () => {
    const client = clientFor({});
    await expect(searchMentionPeople(client, SLUG, { query: "x" })).resolves.toEqual([]);
  });
});

describe("description assets", () => {
  const grant = {
    asset_id: "asset-1",
    asset_url: "/api/assets/v2/workspaces/acme/projects/p-1/asset-1/",
    upload_data: { url: "https://s3.test/upload", fields: { key: "k" } },
  };

  it("parks the file under the given entity type and row", async () => {
    const transport = json(grant);
    const client = createClient({ baseUrl: "https://api.test", transport });

    const result = await requestDescriptionAsset(
      client,
      SLUG,
      PROJECT,
      { entityType: DRAFT_DESCRIPTION_ENTITY_TYPE, entityId: "draft-1" },
      file()
    );

    expect(result.asset_id).toBe("asset-1");
    const post = transport.calls.find(
      (call) => call.init.method === "POST" && call.url.includes(`/projects/${PROJECT}/`)
    );
    expect(post?.url).toBe(`https://api.test/api/assets/v2/workspaces/${SLUG}/projects/${PROJECT}/`);
    expect(JSON.parse(post?.init.body ?? "{}")).toEqual({
      entity_identifier: "draft-1",
      entity_type: "DRAFT_ISSUE_DESCRIPTION",
      name: "shot.png",
      size: 5,
      type: "image/png",
    });
  });

  it("uploads bytes to the signed URL and confirms the asset", async () => {
    const transport = recordingTransport((call) => {
      if (call.url.endsWith("/auth/get-csrf-token/")) {
        return fakeResponse({ body: JSON.stringify({ csrf_token: "csrf" }) });
      }
      if (call.init.method === "POST") return fakeResponse({ status: 201, body: JSON.stringify(grant) });
      return fakeResponse({ status: 204, body: "" });
    });
    const client = createClient({ baseUrl: "https://api.test", transport });
    const uploadBytes = vi.fn(async () => ({ ok: true, status: 204 }));

    const uploaded = await uploadDescriptionAsset(
      client,
      SLUG,
      PROJECT,
      { entityType: ISSUE_DESCRIPTION_ENTITY_TYPE, entityId: "" },
      file(),
      uploadBytes
    );

    expect(uploaded).toEqual({
      assetId: "asset-1",
      assetUrl: "/api/assets/v2/workspaces/acme/projects/p-1/asset-1/",
    });
    expect(uploadBytes).toHaveBeenCalledTimes(1);
    const [url, payload] = uploadBytes.mock.calls[0] as unknown as [string, FormData];
    expect(url).toBe("https://s3.test/upload");
    expect(payload.get("key")).toBe("k");
    expect((payload.get("file") as File).name).toBe("shot.png");
    const confirm = transport.calls.find((call) => call.init.method === "PATCH");
    expect(confirm?.url).toBe(`https://api.test/api/assets/v2/workspaces/${SLUG}/projects/${PROJECT}/asset-1/`);
  });

  it("rejects when the byte leg is refused and never confirms", async () => {
    const transport = json(grant);
    const client = createClient({ baseUrl: "https://api.test", transport });

    await expect(
      uploadDescriptionAsset(
        client,
        SLUG,
        PROJECT,
        { entityType: ISSUE_DESCRIPTION_ENTITY_TYPE, entityId: "" },
        file(),
        async () => ({ ok: false, status: 500 })
      )
    ).rejects.toBeInstanceOf(ApiError);
    expect(transport.calls.some((call) => call.init.method === "PATCH")).toBe(false);
  });

  it("links parked assets to the work item and skips an empty list", async () => {
    const transport = json({});
    const client = createClient({ baseUrl: "https://api.test", transport });

    await attachDescriptionAssets(client, SLUG, PROJECT, "issue-1", []);
    expect(transport.calls.some((call) => call.url.includes("/bulk/"))).toBe(false);

    await attachDescriptionAssets(client, SLUG, PROJECT, "issue-1", ["a-1", "a-2"]);
    const bulk = transport.calls.find((call) => call.url.includes("/bulk/"));
    expect(bulk?.url).toBe(`https://api.test/api/assets/v2/workspaces/${SLUG}/projects/${PROJECT}/issue-1/bulk/`);
    expect(JSON.parse(bulk?.init.body ?? "{}")).toEqual({ asset_ids: ["a-1", "a-2"] });
  });
});
