// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: page live-operations against a live apps/live (NEWFRONT-834).
// Needs PIDASH_CONTRACT_LIVE_URL (a scratch live server whose API_BASE_URL
// points at the contract Django) next to PIDASH_CONTRACT_BASE_URL. One
// sign-in feeds a Django client (probe-page setup) and a live client (the
// operations) through a shared jar, so the export's forwarded cookie
// resolves the probe page. Probe pages go through raw calls (page REST
// belongs to NEWFRONT-833) and are archived and deleted afterwards.
import { describe, expect, it } from "vitest";
import { createClient } from "../client.js";
import type { ApiClient } from "../client.js";
import { signIn } from "../contracts/auth-entry.js";
import {
  archivePage,
  convertDocument,
  convertDocumentErrorFrom,
  exportPagePdf,
  getLiveHealth,
  pdfExportErrorFrom,
} from "../contracts/pages.js";
import { listProjects } from "../contracts/projects.js";
import {
  CONTRACT_BASE_URL,
  CONTRACT_EMAIL,
  CONTRACT_LIVE_URL,
  CONTRACT_PASSWORD,
  CONTRACT_PROJECT_IDENTIFIER,
  CONTRACT_WORKSPACE,
  contractTransport,
  liveContractEnabled,
} from "./setup.js";

interface LivePair {
  django: ApiClient;
  live: ApiClient;
}

let pair: LivePair | null = null;

/** One sign-in per file; both clients share the jar (space-auth precedent). */
async function signedPair(): Promise<LivePair> {
  if (!pair) {
    const jar = new Map<string, string>();
    const django = createClient({ baseUrl: CONTRACT_BASE_URL, transport: contractTransport(jar), validate: true });
    const result = await signIn(django, { email: CONTRACT_EMAIL, password: CONTRACT_PASSWORD });
    if (!result.ok) throw new Error(`contract sign-in failed: ${result.code} ${result.message}`);
    pair = {
      django,
      live: createClient({ baseUrl: CONTRACT_LIVE_URL, transport: contractTransport(jar), validate: true }),
    };
  }
  return pair;
}

async function seededProjectId(client: ApiClient): Promise<string> {
  const projects = await listProjects(client, CONTRACT_WORKSPACE);
  const seeded = projects.find((project) => project.identifier === CONTRACT_PROJECT_IDENTIFIER);
  expect(seeded?.id).toBeDefined();
  return seeded?.id ?? "";
}

const PROBE_NAME = "Contract live export probe";

function pagesPath(projectId: string): string {
  return `/api/workspaces/${CONTRACT_WORKSPACE}/projects/${projectId}/pages/`;
}

/** Remove a probe page: delete refuses live pages, so archive first. */
async function removeProbePage(client: ApiClient, projectId: string, pageId: string): Promise<void> {
  await archivePage(client, CONTRACT_WORKSPACE, projectId, pageId);
  await client.delete(`${pagesPath(projectId)}${pageId}/`);
}

async function sweepProbePages(client: ApiClient, projectId: string): Promise<void> {
  const body = await client.getJson<unknown>(pagesPath(projectId));
  const rows = Array.isArray(body) ? body : [];
  for (const row of rows) {
    const probe = row as { id?: unknown; name?: unknown };
    if (probe.name === PROBE_NAME && typeof probe.id === "string") {
      await removeProbePage(client, projectId, probe.id);
    }
  }
}

describe.skipIf(!liveContractEnabled)("page live health contract", () => {
  it("probes status, timestamp and version", async () => {
    const { live } = await signedPair();
    const health = await getLiveHealth(live);
    expect(health.status).toBe("OK");
    expect(Number.isNaN(Date.parse(health.timestamp))).toBe(false);
    expect(health.version.length).toBeGreaterThan(0);
  });
});

describe.skipIf(!liveContractEnabled)("page live convert contract", () => {
  it("converts markup into the three stored forms", async () => {
    const { live } = await signedPair();
    const marker = `Live834 ${Date.now()}`;
    const converted = await convertDocument(live, {
      description_html: `<p>${marker}</p>`,
      variant: "rich",
      title: marker,
    });
    expect(converted.description_json["type"]).toBe("doc");
    expect(converted.description_binary.length).toBeGreaterThan(0);
    expect(Buffer.from(converted.description_binary, "base64").length).toBeGreaterThan(0);
    expect(converted.description_html).toContain(marker);
  });

  it("applies new content onto a base binary", async () => {
    const { live } = await signedPair();
    const marker = `Live834 base ${Date.now()}`;
    const first = await convertDocument(live, { description_html: `<p>${marker}</p>`, variant: "rich" });
    const second = await convertDocument(live, {
      description_html: `<p>${marker} again</p>`,
      variant: "rich",
      description_binary: first.description_binary,
    });
    expect(second.description_binary.length).toBeGreaterThan(0);
    expect(second.description_html).toContain("again");
  });

  it("refuses malformed input with per-field violations", async () => {
    const { live } = await signedPair();
    const invalid: unknown[] = [
      {},
      { description_html: "", variant: "rich" },
      { description_html: "   ", variant: "rich" },
      { description_html: "no tags here", variant: "rich" },
      { description_html: "<p>x</p>", variant: "bogus" },
      { description_html: "<p>x</p>", variant: "rich", description_binary: "!!!not-base64!!!" },
    ];
    for (const body of invalid) {
      const refused: unknown = await convertDocument(
        live,
        body as unknown as Parameters<typeof convertDocument>[1]
      ).then(
        () => null,
        (error: unknown) => error
      );
      const failure = convertDocumentErrorFrom(refused);
      expect(failure?.status).toBe(400);
      expect(failure?.violations.length).toBeGreaterThan(0);
    }
  });
});

describe.skipIf(!liveContractEnabled)("page live export contract", () => {
  it("exports a persisted page to PDF bytes", async () => {
    const { django, live } = await signedPair();
    const projectId = await seededProjectId(django);
    await sweepProbePages(django, projectId);
    const marker = `Live834 export ${Date.now()}`;
    const created = await django.sendJson<{ id: string }>("POST", pagesPath(projectId), {
      json: { name: PROBE_NAME },
    });
    const pageId = created.data.id;
    try {
      const minted = await convertDocument(live, {
        description_html: `<p>${marker}</p>`,
        variant: "rich",
      });
      const persisted = await django.sendJson<{ message: string }>(
        "PATCH",
        `${pagesPath(projectId)}${pageId}/description/`,
        {
          json: {
            description_binary: minted.description_binary,
            description_html: minted.description_html,
            description_json: minted.description_json,
          },
        }
      );
      expect(persisted.status).toBe(200);
      const exported = await exportPagePdf(live, {
        pageId,
        workspaceSlug: CONTRACT_WORKSPACE,
        projectId,
        title: marker,
        noAssets: true,
      });
      expect(exported.contentType).toContain("application/pdf");
      expect(exported.filename).toBe(`page-${pageId}.pdf`);
      expect(exported.bytes.length).toBeGreaterThan(1000);
      expect(Buffer.from(exported.bytes.subarray(0, 4)).toString("latin1")).toBe("%PDF");
    } finally {
      await removeProbePage(django, projectId, pageId);
    }
  });

  it("maps the unauthenticated, invalid and missing-page failures", async () => {
    const { django, live } = await signedPair();
    const projectId = await seededProjectId(django);
    const anonymous = createClient({
      baseUrl: CONTRACT_LIVE_URL,
      transport: contractTransport(new Map<string, string>()),
      validate: true,
    });
    const unauthenticated: unknown = await exportPagePdf(anonymous, {
      pageId: "00000000-0000-0000-0000-000000000000",
      workspaceSlug: CONTRACT_WORKSPACE,
      projectId,
    }).then(
      () => null,
      (error: unknown) => error
    );
    expect(pdfExportErrorFrom(unauthenticated)).toMatchObject({ kind: "auth", status: 401 });

    const invalid: unknown = await exportPagePdf(live, { pageId: "", workspaceSlug: "" }).then(
      () => null,
      (error: unknown) => error
    );
    expect(pdfExportErrorFrom(invalid)).toMatchObject({ kind: "validation", status: 400 });

    const missing: unknown = await exportPagePdf(live, {
      pageId: "00000000-0000-0000-0000-000000000000",
      workspaceSlug: CONTRACT_WORKSPACE,
      projectId,
    }).then(
      () => null,
      (error: unknown) => error
    );
    const failure = pdfExportErrorFrom(missing);
    expect(failure?.kind).toBe("fetch");
    expect(failure?.status === 404 || failure?.status === 502).toBe(true);
  });
});
