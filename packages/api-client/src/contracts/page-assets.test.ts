// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Stub proofs for the page image pipeline (NEWFRONT-833, spec §11.4).
// Grant, bytes, confirm, link, clone and existence legs each pin their
// path, verb and body; the byte upload is injected so no network is used.
import { describe, expect, it, vi } from "vitest";
import type { ApiClient } from "../client.js";
import {
  PAGE_DESCRIPTION_ENTITY_TYPE,
  attachPageAssets,
  checkPageAsset,
  completePageAsset,
  duplicatePageAsset,
  pageAssetDownloadUrl,
  pageAssetUrl,
  requestPageAsset,
  uploadPageAsset,
} from "./page-assets.js";

const WS = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const PAGE = "123e4567-e89b-12d3-a456-426614174000";
const ASSET = "323e4567-e89b-12d3-a456-426614174002";
const CLONE = "423e4567-e89b-12d3-a456-426614174003";
const WORKSPACE_ASSETS = `/api/assets/v2/workspaces/${WS}`;

interface Call {
  method: string;
  url: string;
  json?: unknown;
}

function stubClient(handler: (call: Call) => unknown = () => null): { client: ApiClient; calls: Call[] } {
  const calls: Call[] = [];
  const record =
    (method: string) =>
    async (url: string, options: { json?: unknown } = {}) => {
      const call = { method, url, json: options.json };
      calls.push(call);
      const value = handler(call);
      return method === "GET" ? value : { data: value, status: 204, url };
    };
  const sendJson = (async (method: string, url: string, options: { json?: unknown } = {}) => {
    const call = { method, url, json: options.json };
    calls.push(call);
    return { data: handler(call), status: 201, url };
  }) as ApiClient["sendJson"];
  const client = {
    parse: ((schema: { parse: (value: unknown) => unknown }, value: unknown) =>
      schema.parse(value)) as ApiClient["parse"],
    getJson: record("GET") as ApiClient["getJson"],
    sendJson,
    post: record("POST") as ApiClient["post"],
    patch: record("PATCH") as ApiClient["patch"],
  } as ApiClient;
  return { client, calls };
}

const GRANT = {
  asset_id: ASSET,
  asset_url: null,
  upload_data: { url: "https://s3.test/upload", fields: { key: "page/image.png", policy: "sig" } },
};

function imageFile(): File {
  return new File([new Uint8Array([9, 8, 7])], "diagram.png", { type: "image/png" });
}

describe("page asset pipeline", () => {
  it("requests a grant under the page description entity type", async () => {
    const { client, calls } = stubClient(() => GRANT);
    const grant = await requestPageAsset(client, WS, PROJECT, PAGE, imageFile());
    expect(calls[0]).toMatchObject({
      method: "POST",
      url: `${WORKSPACE_ASSETS}/projects/${PROJECT}/`,
      json: {
        entity_identifier: PAGE,
        entity_type: PAGE_DESCRIPTION_ENTITY_TYPE,
        name: "diagram.png",
        size: 3,
        type: "image/png",
      },
    });
    expect(grant.asset_id).toBe(ASSET);
    expect(grant.upload_data.fields["key"]).toBe("page/image.png");
  });

  it("parks a pre-creation image with an empty entity id", async () => {
    const { client, calls } = stubClient(() => GRANT);
    await requestPageAsset(client, WS, PROJECT, null, imageFile());
    expect((calls[0]?.json as { entity_identifier: string } | undefined)?.entity_identifier).toBe("");
  });

  it("rejects a grant with no asset id", async () => {
    const { client } = stubClient(() => ({ upload_data: { url: "https://s3.test", fields: {} } }));
    await expect(requestPageAsset(client, WS, PROJECT, PAGE, imageFile())).rejects.toThrow();
  });

  it("confirms the parked asset by id", async () => {
    const { client, calls } = stubClient();
    await completePageAsset(client, WS, PROJECT, ASSET);
    expect(calls[0]).toMatchObject({
      method: "PATCH",
      url: `${WORKSPACE_ASSETS}/projects/${PROJECT}/${ASSET}/`,
    });
  });

  it("uploads the bytes to the signed URL then confirms", async () => {
    const { client, calls } = stubClient(() => GRANT);
    const seen: { url: string; keys: string[]; file: unknown }[] = [];
    const uploaded = await uploadPageAsset(client, WS, PROJECT, PAGE, imageFile(), async (url, body) => {
      seen.push({ url, keys: [...(body as unknown as Map<string, unknown>).keys()], file: body.get("file") });
      return { ok: true, status: 204 };
    });
    expect(seen[0]?.url).toBe("https://s3.test/upload");
    expect(seen[0]?.keys).toEqual(["key", "policy", "file"]);
    expect(seen[0]?.file).toBeInstanceOf(File);
    expect(calls.map((call) => `${call.method} ${call.url}`)).toEqual([
      `POST ${WORKSPACE_ASSETS}/projects/${PROJECT}/`,
      `PATCH ${WORKSPACE_ASSETS}/projects/${PROJECT}/${ASSET}/`,
    ]);
    expect(uploaded).toEqual({ assetId: ASSET, assetUrl: "" });
  });

  it("rejects and skips the confirm when the signed upload fails", async () => {
    const { client, calls } = stubClient(() => GRANT);
    await expect(
      uploadPageAsset(client, WS, PROJECT, PAGE, imageFile(), async () => ({ ok: false, status: 500 }))
    ).rejects.toThrow("rejected");
    expect(calls).toHaveLength(1);
  });

  it("links the embedded images to the page in one bulk call", async () => {
    const { client, calls } = stubClient();
    await attachPageAssets(client, WS, PROJECT, PAGE, [ASSET, CLONE]);
    expect(calls[0]).toMatchObject({
      method: "POST",
      url: `${WORKSPACE_ASSETS}/projects/${PROJECT}/${PAGE}/bulk/`,
      json: { asset_ids: [ASSET, CLONE] },
    });
  });

  it("makes no request for an empty asset list", async () => {
    const { client, calls } = stubClient();
    await attachPageAssets(client, WS, PROJECT, PAGE, []);
    expect(calls).toEqual([]);
  });

  it("clones an image onto the new page", async () => {
    const { client, calls } = stubClient(() => ({ asset_id: CLONE }));
    const clone = await duplicatePageAsset(client, WS, PROJECT, ASSET, PAGE);
    expect(calls[0]).toMatchObject({
      method: "POST",
      url: `${WORKSPACE_ASSETS}/duplicate-assets/${ASSET}/`,
      json: { entity_id: PAGE, entity_type: PAGE_DESCRIPTION_ENTITY_TYPE, project_id: PROJECT },
    });
    expect(clone.asset_id).toBe(CLONE);
  });

  it("reads the asset existence flag", async () => {
    const { client, calls } = stubClient(() => ({ exists: true }));
    const result = await checkPageAsset(client, WS, ASSET);
    expect(calls[0]).toMatchObject({ method: "GET", url: `${WORKSPACE_ASSETS}/check/${ASSET}/` });
    expect(result.exists).toBe(true);
  });

  it("posts the bytes through the real fetch path when no uploader is injected", async () => {
    const { client } = stubClient(() => GRANT);
    const seen: { url: string; method: string; hasBody: boolean }[] = [];
    const fetchMock = vi.fn(async (url: string, init: { method: string; body: FormData }) => {
      seen.push({ url, method: init.method, hasBody: init.body.get("file") instanceof File });
      return new Response(null, { status: 204 });
    });
    vi.stubGlobal("fetch", fetchMock);
    try {
      const uploaded = await uploadPageAsset(client, WS, PROJECT, PAGE, imageFile());
      expect(uploaded.assetId).toBe(ASSET);
    } finally {
      vi.unstubAllGlobals();
    }
    expect(seen).toEqual([{ url: "https://s3.test/upload", method: "POST", hasBody: true }]);
  });

  it("fails closed when the real byte upload answers an error status", async () => {
    const { client, calls } = stubClient(() => GRANT);
    const fetchMock = vi.fn(async () => new Response(null, { status: 503 }));
    vi.stubGlobal("fetch", fetchMock);
    try {
      await expect(uploadPageAsset(client, WS, PROJECT, PAGE, imageFile())).rejects.toMatchObject({
        status: 503,
        code: "http",
      });
    } finally {
      vi.unstubAllGlobals();
    }
    expect(calls.map((call) => call.method)).toEqual(["POST"]);
  });

  it("builds the view and download addresses", () => {
    expect(pageAssetUrl(WS, PROJECT, ASSET)).toBe(`${WORKSPACE_ASSETS}/projects/${PROJECT}/${ASSET}/`);
    expect(pageAssetDownloadUrl(WS, PROJECT, ASSET)).toBe(`${WORKSPACE_ASSETS}/projects/${PROJECT}/download/${ASSET}/`);
  });
});
