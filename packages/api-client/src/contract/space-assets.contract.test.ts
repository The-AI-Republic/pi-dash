// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: the public asset lifecycle against live Django
// (NEWFRONT-752). The seeded bucket is a dummy (signing is local crypto),
// so the byte leg is not attempted: this proves the grant shape, the
// mandatory confirm (204), the read route refusing an unconfirmed asset,
// the bulk link, and restore. Assets are parked and never surfaced.
import { describe, expect, it } from "vitest";
import { ApiError } from "../errors.js";
import { listSpaceBoardIssues } from "../contracts/space-issues.js";
import {
  SPACE_COMMENT_IMAGE_ENTITY_TYPE,
  completeSpaceAssetUpload,
  linkSpaceAssets,
  requestSpaceAssetUpload,
  restoreSpaceAsset,
  spaceAssetReadPath,
} from "../contracts/space-assets.js";
import { createSpaceComment } from "../contracts/space-comments.js";
import { CONTRACT_BASE_URL, CONTRACT_BOARD_ANCHOR, contractClient, contractEnabled, sharedSignedIn } from "./setup.js";
import type { ApiClient } from "../client.js";

async function anyIssueId(client: ApiClient): Promise<string> {
  const page = await listSpaceBoardIssues(client, CONTRACT_BOARD_ANCHOR, {
    groupBy: "state_id",
    perPage: 50,
    cursor: "50:0:0",
  });
  const row = Object.values(page.results).flatMap((bucket) => bucket.results)[0];
  expect(row).toBeDefined();
  return row?.id ?? "";
}

/** Read the asset URL without following the redirect to storage. */
async function assetStatus(anchor: string, assetId: string): Promise<number> {
  const response = await globalThis.fetch(`${CONTRACT_BASE_URL}${spaceAssetReadPath(anchor, assetId)}`, {
    redirect: "manual",
  });
  return response.status;
}

describe.skipIf(!contractEnabled)("space asset contract", () => {
  it("grants, confirms and links an asset to a comment", async () => {
    const client = await sharedSignedIn();
    const issueId = await anyIssueId(client);

    const grant = await requestSpaceAssetUpload(client, CONTRACT_BOARD_ANCHOR, {
      name: `contract-${Date.now().toString(36)}.png`,
      type: "image/png",
      size: 64,
    });
    expect(grant.asset_id).toBeTruthy();
    expect(grant.upload_data.url.length).toBeGreaterThan(0);
    expect(grant.upload_data.fields).toBeTypeOf("object");

    // Unconfirmed assets are invisible to the read route.
    expect(await assetStatus(CONTRACT_BOARD_ANCHOR, grant.asset_id)).toBe(404);

    await completeSpaceAssetUpload(client, CONTRACT_BOARD_ANCHOR, grant.asset_id);
    // Confirmed: the read route now redirects to storage instead of 404.
    expect(await assetStatus(CONTRACT_BOARD_ANCHOR, grant.asset_id)).toBe(302);

    const comment = await createSpaceComment(client, CONTRACT_BOARD_ANCHOR, issueId, {
      commentHtml: "<p>Contract space asset probe</p>",
    });
    await linkSpaceAssets(client, CONTRACT_BOARD_ANCHOR, comment.id, [grant.asset_id]);

    // A soft-deleted asset can be restored.
    await client.delete(`/api/public/assets/v2/anchor/${CONTRACT_BOARD_ANCHOR}/${grant.asset_id}/`);
    await restoreSpaceAsset(client, CONTRACT_BOARD_ANCHOR, grant.asset_id);

    await client.delete(`/api/public/anchor/${CONTRACT_BOARD_ANCHOR}/issues/${issueId}/comments/${comment.id}/`);
  });

  it("rejects an unsupported MIME type with the server's error envelope", async () => {
    const client = await sharedSignedIn();
    const error = await requestSpaceAssetUpload(client, CONTRACT_BOARD_ANCHOR, {
      name: "probe.pdf",
      type: "application/pdf",
      size: 64,
    }).catch((caught: unknown) => caught);
    expect(error).toBeInstanceOf(ApiError);
    expect((error as ApiError).status).toBe(400);
    expect((error as ApiError).message.length).toBeGreaterThan(0);
    expect(SPACE_COMMENT_IMAGE_ENTITY_TYPE).toBe("COMMENT_DESCRIPTION");
  });

  it("refuses an anonymous asset grant", async () => {
    const error = await requestSpaceAssetUpload(contractClient(), CONTRACT_BOARD_ANCHOR, {
      name: "probe.png",
      type: "image/png",
      size: 64,
    }).catch((caught: unknown) => caught);
    expect(error).toBeInstanceOf(ApiError);
    expect((error as ApiError).status).toBe(401);
  });
});
