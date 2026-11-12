// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: attachments upload legs, list, served-URL redirect and
// delete against live Django. The byte leg is stubbed — no object
// store runs in the contract env — while grant, complete, list,
// redirect and delete all hit the server.
import { describe, expect, it } from "vitest";
import { createClient } from "../client.js";
import { signIn } from "../contracts/auth-entry.js";
import { createIssue, deleteIssue } from "../contracts/issue-actions.js";
import {
  completeAttachmentUpload,
  deleteIssueAttachment,
  listIssueAttachments,
  requestAttachmentUpload,
  uploadIssueAttachment,
} from "../contracts/issue-attachments.js";
import { listIssues } from "../contracts/issues.js";
import { listProjects } from "../contracts/projects.js";
import {
  CONTRACT_BASE_URL,
  CONTRACT_EMAIL,
  CONTRACT_PASSWORD,
  CONTRACT_PROJECT_IDENTIFIER,
  CONTRACT_WORKSPACE,
  contractEnabled,
  contractTransport,
  sharedSignedIn,
} from "./setup.js";
import type { ApiClient } from "../client.js";

const PROBE_NAME = "Contract attachment probe";

async function seededProjectId(client: ApiClient): Promise<string> {
  const projects = await listProjects(client, CONTRACT_WORKSPACE);
  const seeded = projects.find((project) => project.identifier === CONTRACT_PROJECT_IDENTIFIER);
  expect(seeded).toBeDefined();
  return seeded?.id ?? "";
}

async function freshProbe(client: ApiClient, projectId: string): Promise<string> {
  const page = await listIssues(client, CONTRACT_WORKSPACE, projectId);
  for (const row of page.results.filter((entry) => entry.name === PROBE_NAME)) {
    await deleteIssue(client, CONTRACT_WORKSPACE, projectId, row.id);
  }
  const probe = await createIssue(client, CONTRACT_WORKSPACE, projectId, { name: PROBE_NAME });
  return probe.id;
}

describe.skipIf(!contractEnabled)("issue attachments contract", () => {
  it("grants, completes, lists, serves and deletes an attachment", async () => {
    const client = await sharedSignedIn();
    const projectId = await seededProjectId(client);
    const probeId = await freshProbe(client, projectId);

    const grant = await requestAttachmentUpload(client, CONTRACT_WORKSPACE, projectId, probeId, {
      name: "contract-note.png",
      type: "image/png",
      size: 1024,
    });
    expect(grant.asset_id).toMatch(/^[0-9a-f-]{36}$/);
    expect(grant.upload_data.url.length).toBeGreaterThan(0);
    expect(grant.upload_data.fields["Content-Type"]).toBe("image/png");
    expect(grant.attachment.is_uploaded).toBe(false);

    // Unfinished uploads stay out of the list.
    const pending = await listIssueAttachments(client, CONTRACT_WORKSPACE, projectId, probeId);
    expect(pending.map((row) => row.id)).not.toContain(grant.asset_id);

    await completeAttachmentUpload(client, CONTRACT_WORKSPACE, projectId, probeId, grant.asset_id);
    const listed = await listIssueAttachments(client, CONTRACT_WORKSPACE, projectId, probeId);
    const row = listed.find((entry) => entry.id === grant.asset_id);
    expect(row?.is_uploaded).toBe(true);
    expect(row?.attributes["name"]).toBe("contract-note.png");
    expect(row?.asset_url).toContain(`/attachments/${grant.asset_id}/`);

    // The served URL answers a redirect to the object store. A signed-in
    // jar of our own carries the session past the permission check.
    const jar = new Map<string, string>();
    const authed = createClient({ baseUrl: CONTRACT_BASE_URL, transport: contractTransport(jar) });
    const signedIn = await signIn(authed, { email: CONTRACT_EMAIL, password: CONTRACT_PASSWORD });
    expect(signedIn.ok).toBe(true);
    const cookie = [...jar.entries()].map(([name, value]) => `${name}=${value}`).join("; ");
    const served = await globalThis.fetch(`${CONTRACT_BASE_URL}${row?.asset_url ?? ""}`, {
      headers: { Cookie: cookie },
      redirect: "manual",
    });
    expect(served.status).toBe(302);
    expect(served.headers.get("location")?.length ?? 0).toBeGreaterThan(0);

    await deleteIssueAttachment(client, CONTRACT_WORKSPACE, projectId, probeId, grant.asset_id);
    const cleared = await listIssueAttachments(client, CONTRACT_WORKSPACE, projectId, probeId);
    expect(cleared.map((entry) => entry.id)).not.toContain(grant.asset_id);

    await deleteIssue(client, CONTRACT_WORKSPACE, projectId, probeId);
  });

  it("uploads one file through the three-leg helper with stubbed bytes", async () => {
    const client = await sharedSignedIn();
    const projectId = await seededProjectId(client);
    const probeId = await freshProbe(client, projectId);

    const file = new File(["contract-bytes"], "contract-helper.png", { type: "image/png" });
    const attachment = await uploadIssueAttachment(client, CONTRACT_WORKSPACE, projectId, probeId, file, () =>
      Promise.resolve({ ok: true, status: 200 })
    );
    expect(attachment.attributes["name"]).toBe("contract-helper.png");

    const listed = await listIssueAttachments(client, CONTRACT_WORKSPACE, projectId, probeId);
    expect(listed.map((row) => row.id)).toContain(attachment.id);

    await deleteIssueAttachment(client, CONTRACT_WORKSPACE, projectId, probeId, attachment.id);
    await deleteIssue(client, CONTRACT_WORKSPACE, projectId, probeId);
  });

  it("rejects a MIME type outside the allowlist with 400", async () => {
    const client = await sharedSignedIn();
    const projectId = await seededProjectId(client);
    const probeId = await freshProbe(client, projectId);

    await expect(
      requestAttachmentUpload(client, CONTRACT_WORKSPACE, projectId, probeId, {
        name: "contract-evil.bin",
        type: "application/x-unknown-contract",
        size: 16,
      })
    ).rejects.toMatchObject({ status: 400 });

    await deleteIssue(client, CONTRACT_WORKSPACE, projectId, probeId);
  });
});
