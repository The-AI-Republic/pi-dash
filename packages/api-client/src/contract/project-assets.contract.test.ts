// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: the user-asset upload grant and status confirmation against
// live Django. No object storage exists in the contract stack, so the
// byte leg is stub-proven in the unit suite; here the grant parses, the
// confirm answers 204, and the server rejects an entity type it does not
// accept.
import { describe, expect, it } from "vitest";
import { ApiError } from "../errors.js";
import { SignedUserAsset, USER_ASSET_TYPES } from "../contracts/assets.js";
import { contractEnabled, sharedSignedIn } from "./setup.js";
import type { ApiClient } from "../client.js";

async function signedIn(): Promise<ApiClient> {
  return sharedSignedIn();
}

describe.skipIf(!contractEnabled)("project emblem asset contract", () => {
  it.each(USER_ASSET_TYPES)("grants %s and confirms the upload", async (entityType) => {
    const client = await signedIn();
    const grantPath = "/api/assets/v2/user-assets/";
    const envelope = await client.sendJson<unknown>("POST", grantPath, {
      json: {
        entity_identifier: "",
        entity_type: entityType,
        name: "emblem.png",
        size: 2048,
        type: "image/png",
      },
    });
    const grant = client.parse(SignedUserAsset, envelope.data, `POST ${grantPath}`);
    expect(grant.asset_id.length).toBeGreaterThan(0);
    expect(grant.asset_url).toBe(`/api/assets/v2/static/${grant.asset_id}/`);
    expect(grant.upload_data.url.length).toBeGreaterThan(0);
    expect(Object.keys(grant.upload_data.fields).length).toBeGreaterThan(0);

    const confirmed = await client.patch(`/api/assets/v2/user-assets/${grant.asset_id}/`);
    expect(confirmed.status).toBe(204);
    await client.delete(`/api/assets/v2/user-assets/${grant.asset_id}/`);
  });

  it("rejects an entity type the user-asset route does not serve", async () => {
    const client = await signedIn();
    const refused = await client
      .sendJson<unknown>("POST", "/api/assets/v2/user-assets/", {
        json: {
          entity_identifier: "",
          entity_type: "PROJECT_COVER",
          name: "emblem.png",
          size: 2048,
          type: "image/png",
        },
      })
      .then(
        () => null,
        (caught: unknown) => caught
      );
    expect(refused).toBeInstanceOf(ApiError);
    expect((refused as ApiError).status).toBe(400);
  });

  it("rejects a non-image mime type with 400", async () => {
    const client = await signedIn();
    const refused = await client
      .sendJson<unknown>("POST", "/api/assets/v2/user-assets/", {
        json: {
          entity_identifier: "",
          entity_type: "USER_COVER",
          name: "emblem.txt",
          size: 2048,
          type: "text/plain",
        },
      })
      .then(
        () => null,
        (caught: unknown) => caught
      );
    expect(refused).toBeInstanceOf(ApiError);
    expect((refused as ApiError).status).toBe(400);
  });
});
