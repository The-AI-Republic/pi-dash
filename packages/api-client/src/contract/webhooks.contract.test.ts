// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: workspace webhooks (NEWFRONT-533) against live Django. Every
// test runs in a throwaway workspace, so the seeded workspace keeps an
// empty list and cleanup drops the rows through the workspace cascade. The
// target host must resolve to a public address: the backend refuses
// localhost, private ranges, and its own domains.
import { describe, expect, it } from "vitest";
import type { ApiClient } from "../client.js";
import { ApiError } from "../errors.js";
import {
  allWebhookEvents,
  createWorkspaceWebhook,
  deleteWorkspaceWebhook,
  getWorkspaceWebhook,
  listWorkspaceWebhooks,
  regenerateWorkspaceWebhookSecret,
  updateWorkspaceWebhook,
  webhookUsesAllEvents,
} from "../contracts/webhooks.js";
import { createWorkspace, deleteWorkspace } from "../contracts/workspace-management.js";
import { contractEnabled, sharedSignedIn } from "./setup.js";

function freshSlug(prefix: string): string {
  return `${prefix}-${Date.now().toString(36)}${Math.floor(Math.random() * 1296).toString(36)}`;
}

/** Public, resolvable target; the path keeps rows distinct within a run. */
function targetUrl(): string {
  return `https://example.com/contract-${Date.now().toString(36)}-${Math.floor(Math.random() * 1_000_000)}`;
}

async function withWorkspace(client: ApiClient, prefix: string, body: (slug: string) => Promise<void>): Promise<void> {
  const slug = freshSlug(prefix);
  await createWorkspace(client, { name: `Contract ${slug}`, slug });
  try {
    await body(slug);
  } finally {
    await deleteWorkspace(client, slug).catch(() => 0);
  }
}

async function refusal(work: Promise<unknown>): Promise<ApiError | null> {
  const caught = await work.then(
    () => null,
    (error: unknown) => error
  );
  return caught instanceof ApiError ? caught : null;
}

async function createProbe(client: ApiClient, slug: string): Promise<string> {
  const created = await createWorkspaceWebhook(client, slug, { url: targetUrl(), ...allWebhookEvents() });
  return created.id;
}

describe.skipIf(!contractEnabled)("workspace webhooks contract", () => {
  it("starts a fresh workspace with an empty list", async () => {
    const client = await sharedSignedIn();
    await withWorkspace(client, "ct-whshe", async (slug) => {
      await expect(listWorkspaceWebhooks(client, slug)).resolves.toEqual([]);
    });
  });

  it("creates with the five flags and returns the one-time secret", async () => {
    const client = await sharedSignedIn();
    await withWorkspace(client, "ct-whnew", async (slug) => {
      const url = targetUrl();
      const created = await createWorkspaceWebhook(client, slug, {
        url,
        project: true,
        issue: true,
        cycle: false,
        module: true,
        issue_comment: false,
      });
      expect(created.id).toMatch(/^[0-9a-f-]{36}$/);
      expect(created.url).toBe(url);
      expect(created.is_active).toBe(true);
      expect(created.secret_key).toMatch(/^pi_dash_wh_/);
      expect(created.project).toBe(true);
      expect(created.issue).toBe(true);
      expect(created.cycle).toBe(false);
      expect(created.module).toBe(true);
      expect(created.issue_comment).toBe(false);

      const list = await listWorkspaceWebhooks(client, slug);
      expect(list).toHaveLength(1);
      expect(list[0]?.id).toBe(created.id);
      expect(typeof list[0]?.created_at).toBe("string");
      // The backend still echoes secret_key on reads (NEWFRONT-541); the
      // read contract declares no such field, so the strict parse drops it
      // before any caller can hold the one-time key.
      expect(list[0]).not.toHaveProperty("secret_key");

      const detail = await getWorkspaceWebhook(client, slug, created.id);
      expect(detail).not.toHaveProperty("secret_key");
    });
  });

  it("reads the detail and reports all-events mode only when every flag is set", async () => {
    const client = await sharedSignedIn();
    await withWorkspace(client, "ct-whget", async (slug) => {
      const id = await createProbe(client, slug);
      const detail = await getWorkspaceWebhook(client, slug, id);
      expect(detail.id).toBe(id);
      expect(webhookUsesAllEvents(detail)).toBe(true);

      const partial = await updateWorkspaceWebhook(client, slug, id, { cycle: false });
      expect(partial.cycle).toBe(false);
      expect(webhookUsesAllEvents(partial)).toBe(false);
    });
  });

  it("patches the row switch with the active flag alone", async () => {
    const client = await sharedSignedIn();
    await withWorkspace(client, "ct-whswitch", async (slug) => {
      const id = await createProbe(client, slug);
      const paused = await updateWorkspaceWebhook(client, slug, id, { is_active: false });
      expect(paused.is_active).toBe(false);
      expect(paused.url).toBe((await getWorkspaceWebhook(client, slug, id)).url);
      const resumed = await updateWorkspaceWebhook(client, slug, id, { is_active: true });
      expect(resumed.is_active).toBe(true);
    });
  });

  it("regenerates the secret, invalidating the old key", async () => {
    const client = await sharedSignedIn();
    await withWorkspace(client, "ct-whregen", async (slug) => {
      const created = await createWorkspaceWebhook(client, slug, { url: targetUrl(), ...allWebhookEvents() });
      const rotated = await regenerateWorkspaceWebhookSecret(client, slug, created.id);
      expect(rotated.id).toBe(created.id);
      expect(rotated.secret_key).toMatch(/^pi_dash_wh_/);
      expect(rotated.secret_key).not.toBe(created.secret_key);
    });
  });

  it("deletes a webhook and then 404s its read and rotation", async () => {
    const client = await sharedSignedIn();
    await withWorkspace(client, "ct-whdel", async (slug) => {
      const id = await createProbe(client, slug);
      await expect(deleteWorkspaceWebhook(client, slug, id)).resolves.toBe(204);
      await expect(listWorkspaceWebhooks(client, slug)).resolves.toEqual([]);

      const read = await refusal(getWorkspaceWebhook(client, slug, id));
      expect(read?.status).toBe(404);
      const rotate = await refusal(regenerateWorkspaceWebhookSecret(client, slug, id));
      expect(rotate?.status).toBe(404);
    });
  });

  it("refuses a duplicate target URL with 409", async () => {
    const client = await sharedSignedIn();
    await withWorkspace(client, "ct-whdup", async (slug) => {
      const url = targetUrl();
      await createWorkspaceWebhook(client, slug, { url, ...allWebhookEvents() });
      const error = await refusal(createWorkspaceWebhook(client, slug, { url, ...allWebhookEvents() }));
      expect(error?.status).toBe(409);
      await expect(listWorkspaceWebhooks(client, slug)).resolves.toHaveLength(1);
    });
  });

  it("refuses a private or local target with the field error", async () => {
    const client = await sharedSignedIn();
    await withWorkspace(client, "ct-whurl", async (slug) => {
      const local = await refusal(
        createWorkspaceWebhook(client, slug, { url: "http://127.0.0.1/hook", ...allWebhookEvents() })
      );
      expect(local?.status).toBe(400);
      expect(local?.body).toBeDefined();

      const unsupported = await refusal(
        createWorkspaceWebhook(client, slug, { url: "ftp://example.com/hook", ...allWebhookEvents() })
      );
      expect(unsupported?.status).toBe(400);
      await expect(listWorkspaceWebhooks(client, slug)).resolves.toEqual([]);
    });
  });
});
