// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: personal API tokens (NEWFRONT-533) against live Django. The
// suite creates its own rows under a contract prefix and deletes them, so
// re-seeding is not required; the list is user-scoped, so rows created by
// earlier runs or other suites never affect the assertions.
import { describe, expect, it } from "vitest";
import type { ApiClient } from "../client.js";
import { ApiError } from "../errors.js";
import { createPersonalApiToken, deletePersonalApiToken, listPersonalApiTokens } from "../contracts/api-tokens.js";
import { contractEnabled, sharedSignedIn } from "./setup.js";

function label(): string {
  return `contract-token-${Date.now().toString(36)}-${Math.floor(Math.random() * 1_000_000)}`;
}

async function refusal(work: Promise<unknown>): Promise<ApiError | null> {
  const caught = await work.then(
    () => null,
    (error: unknown) => error
  );
  return caught instanceof ApiError ? caught : null;
}

async function deleteQueued(client: ApiClient, ids: string[]): Promise<void> {
  for (const id of ids) await deletePersonalApiToken(client, id).catch(() => 0);
}

describe.skipIf(!contractEnabled)("personal API token contract", () => {
  it("creates a never-expiring token, lists it without the secret, and deletes it", async () => {
    const client = await sharedSignedIn();
    const name = label();
    const created = await createPersonalApiToken(client, {
      label: name,
      description: "contract probe",
      expired_at: null,
    });
    try {
      expect(created.label).toBe(name);
      expect(created.description).toBe("contract probe");
      expect(created.expired_at).toBeNull();
      expect(created.is_active).toBe(true);
      expect(created.is_service).toBe(false);
      expect(created.token).toMatch(/^pi_dash_api_/);

      const rows = await listPersonalApiTokens(client);
      const row = rows.find((token) => token.id === created.id);
      expect(row).toBeDefined();
      expect(row?.label).toBe(name);
      expect(row?.expired_at).toBeNull();
      expect(row?.is_active).toBe(true);
      expect("token" in (row ?? {})).toBe(false);
    } finally {
      await deleteQueued(client, [created.id]);
    }
    const after = await listPersonalApiTokens(client);
    expect(after.some((token) => token.id === created.id)).toBe(false);
  });

  it("creates an expiring token and reports it live until the expiry", async () => {
    const client = await sharedSignedIn();
    const future = new Date(Date.now() + 86_400_000).toISOString();
    const created = await createPersonalApiToken(client, { label: label(), description: "", expired_at: future });
    try {
      expect(created.expired_at?.startsWith(future.slice(0, 16))).toBe(true);
      const row = (await listPersonalApiTokens(client)).find((token) => token.id === created.id);
      expect(row?.expired_at?.startsWith(future.slice(0, 16))).toBe(true);
      expect(row?.is_active).toBe(true);
    } finally {
      await deleteQueued(client, [created.id]);
    }
  });

  it("reports a lapsed token as expired", async () => {
    const client = await sharedSignedIn();
    const past = new Date(Date.now() - 86_400_000).toISOString();
    const created = await createPersonalApiToken(client, { label: label(), description: "lapsed", expired_at: past });
    try {
      const row = (await listPersonalApiTokens(client)).find((token) => token.id === created.id);
      expect(row?.is_active).toBe(false);
    } finally {
      await deleteQueued(client, [created.id]);
    }
  });

  it("refuses a label past the column length without creating a row", async () => {
    // The form caps the title at 255. The backend's model column matches,
    // but its create view does not catch the overflow: the request fails
    // as a server error rather than a field 400. The contract pins only
    // that nothing is stored, so a later backend fix to 400 does not need
    // a contract change.
    const client = await sharedSignedIn();
    const before = await listPersonalApiTokens(client);
    const error = await refusal(
      createPersonalApiToken(client, { label: "x".repeat(300), description: "", expired_at: null })
    );
    expect(error).not.toBeNull();
    const after = await listPersonalApiTokens(client);
    expect(after.length).toBe(before.length);
  });

  it("deletes the slashless item address without following a redirect", async () => {
    const client = await sharedSignedIn();
    const created = await createPersonalApiToken(client, { label: label(), description: "", expired_at: null });
    const response = await client.delete(`/api/users/api-tokens/${created.id}`);
    expect(response.status).toBe(204);
    // The server declares the item path with a trailing slash but Django's
    // router answers the slashless DELETE directly (no 301 hop), so the
    // final URL equals the contract's address plus the appended slash.
    expect(new URL(response.url).pathname).toBe(`/api/users/api-tokens/${created.id}/`);
  });

  it("404s deleting an unknown token", async () => {
    const client = await sharedSignedIn();
    const error = await refusal(deletePersonalApiToken(client, "00000000-0000-0000-0000-000000000000"));
    expect(error?.status).toBe(404);
  });
});
