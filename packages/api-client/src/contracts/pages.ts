// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Thin page (document) contract for the shell: the command palette's
// contextual edits rename pages. Full page ownership stays in the pages
// area; this is the slice the palette needs to compile and the gate needs
// to prove the rows.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { ApiError, isApiError } from "../errors.js";
import { dateTime, nullableDate, uuid } from "./fields.js";

export const PageUpdate = z.object({
  id: uuid(),
  name: z.string(),
  access: z.optional(z.number()),
  color: z.optional(z.string()),
  is_locked: z.optional(z.boolean()),
  archived_at: z.optional(nullableDate()),
  project_ids: z.optional(z.array(uuid())),
});
export type PageUpdate = z.infer<typeof PageUpdate>;

export interface PagePatch {
  name?: string;
  color?: string;
  is_locked?: boolean;
}

export async function updatePage(
  client: ApiClient,
  slug: string,
  projectId: string,
  pageId: string,
  input: PagePatch
): Promise<PageUpdate> {
  const envelope = await client.sendJson<unknown>(
    "PATCH",
    `/api/workspaces/${slug}/projects/${projectId}/pages/${pageId}/`,
    { json: input }
  );
  return client.parse(PageUpdate, envelope.data, `PATCH page ${pageId}`);
}

/** Page visibility values behind Make public / Make private. */
export const PAGE_ACCESS_PUBLIC = 0;
export const PAGE_ACCESS_PRIVATE = 1;

/**
 * Thin page lock for the shell: the palette's Lock entry. Answers 204.
 * Full page ownership stays in the pages area.
 */
export async function lockPage(client: ApiClient, slug: string, projectId: string, pageId: string): Promise<void> {
  await client.post(`/api/workspaces/${slug}/projects/${projectId}/pages/${pageId}/lock/`, {});
}

/**
 * Thin page unlock for the shell: the palette's Unlock entry. Answers 204.
 * Full page ownership stays in the pages area.
 */
export async function unlockPage(client: ApiClient, slug: string, projectId: string, pageId: string): Promise<void> {
  await client.delete(`/api/workspaces/${slug}/projects/${projectId}/pages/${pageId}/lock/`);
}

/**
 * Thin page visibility for the shell: the palette's Make public / Make
 * private entry. Only the page owner may change access (otherwise 400).
 * Answers 204. Full page ownership stays in the pages area.
 */
export async function setPageAccess(
  client: ApiClient,
  slug: string,
  projectId: string,
  pageId: string,
  access: number
): Promise<void> {
  await client.post(`/api/workspaces/${slug}/projects/${projectId}/pages/${pageId}/access/`, {
    json: { access },
  });
}

export const PageArchiveResult = z.object({
  archived_at: z.string(),
});
export type PageArchiveResult = z.infer<typeof PageArchiveResult>;

/**
 * Thin page archive for the shell: the palette's Archive entry. Only
 * the page owner or a project admin may archive (otherwise 400); the
 * call also drops the page's favorite rows. Answers 200 with the stamp.
 * Full page ownership stays in the pages area.
 */
export async function archivePage(
  client: ApiClient,
  slug: string,
  projectId: string,
  pageId: string
): Promise<PageArchiveResult> {
  const envelope = await client.sendJson<unknown>(
    "POST",
    `/api/workspaces/${slug}/projects/${projectId}/pages/${pageId}/archive/`,
    {}
  );
  return client.parse(PageArchiveResult, envelope.data, `POST page ${pageId} archive`);
}

/**
 * Thin page restore for the shell: the palette's Restore entry.
 * Answers 204. Full page ownership stays in the pages area.
 */
export async function restorePage(client: ApiClient, slug: string, projectId: string, pageId: string): Promise<void> {
  await client.delete(`/api/workspaces/${slug}/projects/${projectId}/pages/${pageId}/archive/`);
}

/**
 * Thin page favorite for the shell. Pages favorite through their own
 * endpoints (backed by user-favorites with entity_type page), not the
 * generic favorites write. Both calls answer 204. Full page ownership
 * stays in the pages area.
 */
export async function addPageFavorite(
  client: ApiClient,
  slug: string,
  projectId: string,
  pageId: string
): Promise<void> {
  await client.post(`/api/workspaces/${slug}/projects/${projectId}/favorite-pages/${pageId}/`, {});
}

export async function removePageFavorite(
  client: ApiClient,
  slug: string,
  projectId: string,
  pageId: string
): Promise<void> {
  await client.delete(`/api/workspaces/${slug}/projects/${projectId}/favorite-pages/${pageId}/`);
}

// ---------------------------------------------------------------------------
// Live operations: the live server's HTTP surface the page area relies on.
// Same-origin /live/* paths (the slot proxy routes them beside /api/*), so
// these ride the caller's client with no base-URL plumbing. The realtime
// socket frames are typed in the collaboration slice, not here.
// ---------------------------------------------------------------------------

/** The liveness probe answer: OK marker, ISO time, server version. */
export const LiveHealthResponse = z.object({
  status: z.string(),
  timestamp: dateTime(),
  version: z.string(),
});
export type LiveHealthResponse = z.infer<typeof LiveHealthResponse>;

/** Probe the live server. Rejects with the transport ApiError while down. */
export async function getLiveHealth(client: ApiClient): Promise<LiveHealthResponse> {
  const path = "/live/health/";
  const body = await client.getJson<unknown>(path);
  return client.parse(LiveHealthResponse, body, `GET ${path}`);
}

/** Conversion source the live server renders from. */
export type ConvertDocumentVariant = "rich" | "document";

export interface ConvertDocumentInput {
  description_html: string;
  variant: ConvertDocumentVariant;
  /** Current Yjs state (base64) to apply the content onto; absent starts fresh. */
  description_binary?: string;
  title?: string;
}

/** The three stored forms the conversion answers. */
export const ConvertDocumentResponse = z.object({
  description_json: z.record(z.string(), z.unknown()),
  description_binary: z.string(),
  description_html: z.string(),
});
export type ConvertDocumentResponse = z.infer<typeof ConvertDocumentResponse>;

/**
 * Convert markup into the three stored forms. Malformed input rejects
 * with the validation shape convertDocumentErrorFrom reads.
 */
export async function convertDocument(
  client: ApiClient,
  input: ConvertDocumentInput
): Promise<ConvertDocumentResponse> {
  const path = "/live/convert-document/";
  const envelope = await client.sendJson<unknown>("POST", path, {
    json: {
      description_html: input.description_html,
      variant: input.variant,
      description_binary: input.description_binary,
      title: input.title,
    },
  });
  return client.parse(ConvertDocumentResponse, envelope.data, `POST ${path}`);
}

/** One rejected field the 400 names. */
export const ConvertDocumentViolation = z.object({
  path: z.string(),
  message: z.string(),
});
export type ConvertDocumentViolation = z.infer<typeof ConvertDocumentViolation>;

/** The conversion refusal body: a message plus, on 400s, the violations. */
export const ConvertDocumentErrorBody = z.object({
  message: z.string(),
  context: z.optional(
    z.object({
      validationErrors: z.optional(z.array(ConvertDocumentViolation)),
    })
  ),
});
export type ConvertDocumentErrorBody = z.infer<typeof ConvertDocumentErrorBody>;

export interface ConvertDocumentFailure {
  message: string;
  violations: ConvertDocumentViolation[];
  status: number;
}

/**
 * Read the conversion failure off a rejected call: the server message,
 * the per-field violations (empty on the 500), and the HTTP status.
 * Null when the failure is not a conversion refusal.
 */
export function convertDocumentErrorFrom(error: unknown): ConvertDocumentFailure | null {
  if (!isApiError(error)) return null;
  const parsed = ConvertDocumentErrorBody.safeParse(error.body);
  if (!parsed.success) return null;
  return {
    message: parsed.data.message,
    violations: parsed.data.context?.validationErrors ?? [],
    status: error.status,
  };
}

export type PagePdfPageSize = "A4" | "A3" | "A2" | "LETTER" | "LEGAL" | "TABLOID";
export type PagePdfPageOrientation = "portrait" | "landscape";

export interface PagePdfExportInput {
  pageId: string;
  workspaceSlug: string;
  projectId?: string;
  title?: string;
  author?: string;
  subject?: string;
  pageSize?: PagePdfPageSize;
  pageOrientation?: PagePdfPageOrientation;
  fileName?: string;
  noAssets?: boolean;
}

export interface PagePdfExport {
  /** Download name from the disposition header; empty when none was sent. */
  filename: string;
  contentType: string;
  bytes: Uint8Array;
}

/**
 * Export a page to printable PDF. The session cookie in the client's
 * jar authenticates the call (401 without one); failures reject with
 * the shape pdfExportErrorFrom maps to a kind.
 */
export async function exportPagePdf(client: ApiClient, input: PagePdfExportInput): Promise<PagePdfExport> {
  const path = "/live/pdf-export/";
  const response = await client.post(path, {
    json: {
      pageId: input.pageId,
      workspaceSlug: input.workspaceSlug,
      projectId: input.projectId,
      title: input.title,
      author: input.author,
      subject: input.subject,
      pageSize: input.pageSize,
      pageOrientation: input.pageOrientation,
      fileName: input.fileName,
      noAssets: input.noAssets,
    },
  });
  const contentType = response.headers.get("content-type") ?? "";
  if (!contentType.includes("application/pdf")) {
    throw new ApiError({
      status: response.status,
      code: "contract",
      message: `POST ${path} did not answer a PDF`,
      body: null,
    });
  }
  if (typeof response.arrayBuffer !== "function") {
    throw new ApiError({
      status: response.status,
      code: "contract",
      message: `POST ${path} carried no readable bytes`,
      body: null,
    });
  }
  const bytes = new Uint8Array(await response.arrayBuffer());
  return {
    filename: pdfFilenameFrom(response.headers.get("content-disposition")),
    contentType,
    bytes,
  };
}

/**
 * Download name off the disposition header: the encoded filename*
 * first, then the quoted filename; empty when neither parses.
 */
function pdfFilenameFrom(disposition: string | null): string {
  if (!disposition) return "";
  const extended = /filename\*\s*=\s*UTF-8''([^;]+)/i.exec(disposition);
  if (extended?.[1]) {
    try {
      return decodeURIComponent(extended[1].trim());
    } catch {
      return "";
    }
  }
  const quoted = /filename\s*=\s*"([^"]*)"/i.exec(disposition);
  return quoted?.[1] ?? "";
}

/** The export refusal body: one human message; the status carries the kind. */
export const PdfExportErrorBody = z.object({
  message: z.string(),
});
export type PdfExportErrorBody = z.infer<typeof PdfExportErrorBody>;

/** Export failure kinds by status: 401 auth, 400 validation, 404/502 page-or-upstream fetch, 504 render timeout, everything else server-side generation. */
export type PdfExportFailureKind = "auth" | "validation" | "fetch" | "timeout" | "generation";

export interface PdfExportFailure {
  kind: PdfExportFailureKind;
  message: string;
  status: number;
}

/**
 * Read the export failure off a rejected call: the kind by status, the
 * server message, and the status. Null when the failure is not an
 * export refusal (transport errors, contract violations, other bodies).
 */
export function pdfExportErrorFrom(error: unknown): PdfExportFailure | null {
  if (!isApiError(error)) return null;
  const parsed = PdfExportErrorBody.safeParse(error.body);
  if (!parsed.success) return null;
  let kind: PdfExportFailureKind = "generation";
  if (error.status === 401) kind = "auth";
  else if (error.status === 400) kind = "validation";
  else if (error.status === 404 || error.status === 502) kind = "fetch";
  else if (error.status === 504) kind = "timeout";
  return { kind, message: parsed.data.message, status: error.status };
}
