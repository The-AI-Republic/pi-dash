// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the space asset lifecycle: the metadata grant, the byte
// leg, the mandatory confirm, the bulk link, restore and the read path.
import { describe, expect, it, vi } from "vitest";
import { createClient } from "../client.js";
import { fakeResponse, recordingTransport } from "../client.test.js";
import {
  SPACE_COMMENT_IMAGE_ENTITY_TYPE,
  completeSpaceAssetUpload,
  linkSpaceAssets,
  requestSpaceAssetUpload,
  restoreSpaceAsset,
  spaceAssetReadPath,
  uploadSpaceAssetBytes,
} from "./space-assets.js";

const ASSET = "123e4567-e89b-12d3-a456-426614174000";
const COMMENT = "223e4567-e89b-12d3-a456-426614174001";

const grant = {
  asset_id: ASSET,
  asset_url: null,
  upload_data: { url: "https://s3.test/bucket", fields: { key: "a/b.png", policy: "p" } },
};

function clientFor(value: unknown, status = 200) {
  const transport = recordingTransport((call) => {
    if (call.url.endsWith("/auth/get-csrf-token/")) {
      return fakeResponse({ body: JSON.stringify({ csrf_token: "csrf" }) });
    }
    if (value === undefined) return fakeResponse({ status, body: "" });
    return fakeResponse({ status, body: JSON.stringify(value) });
  });
  return { client: createClient({ baseUrl: "https://api.test", transport }), transport };
}

describe("space asset contracts", () => {
  it("requests a grant with the comment entity type and file metadata", async () => {
    const { client, transport } = clientFor(grant);
    const parsed = await requestSpaceAssetUpload(client, "anchor123", {
      name: "shot.png",
      type: "image/png",
      size: 2048,
    });
    expect(parsed.asset_id).toBe(ASSET);
    const call = transport.calls.at(-1);
    expect(call?.init.body).toBe(
      JSON.stringify({
        name: "shot.png",
        type: "image/png",
        size: 2048,
        entity_type: SPACE_COMMENT_IMAGE_ENTITY_TYPE,
        entity_identifier: "",
      })
    );
  });

  it("carries the comment id for an edit", async () => {
    const { client, transport } = clientFor(grant);
    await requestSpaceAssetUpload(client, "anchor123", {
      name: "shot.png",
      type: "image/png",
      size: 1,
      entityIdentifier: COMMENT,
    });
    expect(JSON.parse(transport.calls.at(-1)?.init.body ?? "{}").entity_identifier).toBe(COMMENT);
  });

  it("posts bytes to the grant URL with the injected uploader", async () => {
    const upload = vi.fn().mockResolvedValue({ ok: true, status: 204 });
    const form = new FormData();
    form.append("file", new Blob(["x"]), "shot.png");
    const result = await uploadSpaceAssetBytes(grant, form, upload);
    expect(result.ok).toBe(true);
    expect(upload).toHaveBeenCalledWith("https://s3.test/bucket", form);
  });

  it("posts bytes through the global fetch when no uploader is injected", async () => {
    const fetchSpy = vi.spyOn(globalThis, "fetch").mockResolvedValue(new Response(null, { status: 204 }));
    const form = new FormData();
    form.append("file", new Blob(["x"]), "shot.png");
    const result = await uploadSpaceAssetBytes(grant, form);
    expect(result).toEqual({ ok: true, status: 204 });
    expect(fetchSpy).toHaveBeenCalledWith("https://s3.test/bucket", { method: "POST", body: form });
    fetchSpy.mockRestore();
  });

  it("confirms the asset with a PATCH", async () => {
    const { client, transport } = clientFor(undefined, 204);
    await completeSpaceAssetUpload(client, "anchor123", ASSET);
    const call = transport.calls.at(-1);
    expect(call?.init.method).toBe("PATCH");
    expect(call?.url).toBe(`https://api.test/api/public/assets/v2/anchor/anchor123/${ASSET}/`);
  });

  it("links assets to the new comment in one bulk call", async () => {
    const { client, transport } = clientFor(undefined, 204);
    await linkSpaceAssets(client, "anchor123", COMMENT, [ASSET]);
    const call = transport.calls.at(-1);
    expect(call?.url).toBe(`https://api.test/api/public/assets/v2/anchor/anchor123/${COMMENT}/bulk/`);
    expect(call?.init.body).toBe(JSON.stringify({ asset_ids: [ASSET] }));
  });

  it("restores a soft-deleted asset", async () => {
    const { client, transport } = clientFor(undefined, 204);
    await restoreSpaceAsset(client, "anchor123", ASSET);
    const call = transport.calls.at(-1);
    expect(call?.init.method).toBe("POST");
    expect(call?.url).toBe(`https://api.test/api/public/assets/v2/anchor/anchor123/restore/${ASSET}/`);
  });

  it("builds the public read path for an asset image", () => {
    expect(spaceAssetReadPath("anchor123", ASSET)).toBe(`/api/public/assets/v2/anchor/anchor123/${ASSET}/`);
  });
});
