// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project pages (parity/specs/pages.md §11). One zone per leg of the pages
// surface: the collection read, the record read/write pair, the state
// switches (visibility, lock, archive, favourite), duplication and the
// description body's binary read plus three-form write. Version history is
// read-only here — the build has no restore route (§11.1). Nested fields
// (`parent_id`, `sub_pages_count`) stay optional until PDASHOSS01-270 lands;
// the delta is recorded in parity/api-client-contracts.md.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { ApiError, isApiError } from "../errors.js";
import { dateTime, nullableDate, nullableUuid, uuid } from "./fields.js";

/** Mark stored on `logo_props`: one emoji value or one icon name+colour. */
export const PageMark = z.object({
  in_use: z.optional(z.enum(["emoji", "icon"])),
  emoji: z.optional(z.object({ value: z.string() })),
  icon: z.optional(z.object({ name: z.string(), color: z.optional(z.string()) })),
});
export type PageMark = z.infer<typeof PageMark>;

/** Visibility values: 0 open, 1 restricted. */
export const PAGE_ACCESS_PUBLIC = 0;
export const PAGE_ACCESS_PRIVATE = 1;

/**
 * One collection row as the list serializer emits it. `parent` is the
 * container id or null; `parent_id`/`sub_pages_count` arrive with the nested
 * backend (D17) and are optional here so the current API still parses.
 */
export const PageRow = z.object({
  id: uuid(),
  name: z.string(),
  owned_by: uuid(),
  access: z.number(),
  color: z.string(),
  parent: nullableUuid(),
  parent_id: z.optional(nullableUuid()),
  sub_pages_count: z.optional(z.number()),
  is_favorite: z.boolean(),
  is_locked: z.boolean(),
  archived_at: nullableDate(),
  workspace: uuid(),
  created_at: dateTime(),
  updated_at: dateTime(),
  created_by: nullableUuid(),
  updated_by: nullableUuid(),
  view_props: z.record(z.string(), z.unknown()),
  logo_props: PageMark,
  label_ids: z.array(uuid()),
  project_ids: z.array(uuid()),
});
export type PageRow = z.infer<typeof PageRow>;

/**
 * The nested fields a page carries once D17 lands: its parent container and
 * the number of non-archived children the caller can see. `parent` and
 * `parent_id` always agree; both are emitted so clients that do not expand
 * the container can still read the flat id. No child the caller cannot see
 * is ever counted.
 */
export const PageNestedFields = z.object({
  parent: nullableUuid(),
  parent_id: z.optional(nullableUuid()),
  sub_pages_count: z.optional(z.number()),
});
export type PageNestedFields = z.infer<typeof PageNestedFields>;

/** One page in the collection (bare array, not paged). */
export const PageRows = z.array(PageRow);
export type PageRows = z.infer<typeof PageRows>;

/**
 * The detail read: the row plus the body markup, the structured snapshot
 * the canvas heals HTML-only bodies from (PAGE-040), and the work items
 * the page mentions. The record view always sends `issue_ids`; the
 * create response shares this shape but omits that annotation, so it
 * stays optional, and the snapshot stays optional so answers without
 * it (older stubs, reduced echoes) still parse.
 */
export const PageDetail = z.object({
  ...PageRow.shape,
  description_html: z.string(),
  description_json: z.optional(z.record(z.string(), z.unknown())),
  issue_ids: z.optional(z.array(nullableUuid())),
});
export type PageDetail = z.infer<typeof PageDetail>;

/**
 * The record patch echo. Django omits the collection-only annotations on
 * this route, and the shell's older stubs answer a reduced echo, so the
 * shared contract requires only what every answer carries and validates
 * the rest when present.
 */
export const PageUpdate = z.object({
  id: uuid(),
  name: z.string(),
  owned_by: z.optional(uuid()),
  access: z.optional(z.number()),
  color: z.optional(z.string()),
  parent: z.optional(nullableUuid()),
  parent_id: z.optional(nullableUuid()),
  sub_pages_count: z.optional(z.number()),
  is_locked: z.optional(z.boolean()),
  is_favorite: z.optional(z.boolean()),
  archived_at: z.optional(nullableDate()),
  workspace: z.optional(uuid()),
  created_at: z.optional(dateTime()),
  updated_at: z.optional(dateTime()),
  created_by: z.optional(nullableUuid()),
  updated_by: z.optional(nullableUuid()),
  view_props: z.optional(z.record(z.string(), z.unknown())),
  logo_props: z.optional(PageMark),
  description_html: z.optional(z.string()),
  label_ids: z.optional(z.array(uuid())),
  project_ids: z.optional(z.array(uuid())),
  issue_ids: z.optional(z.array(nullableUuid())),
});
export type PageUpdate = z.infer<typeof PageUpdate>;

/** What `POST .../duplicate/` answers: the copy, still without favourites. */
export const PageDuplicateResult = z.object({
  id: uuid(),
  name: z.string(),
  owned_by: uuid(),
  access: z.number(),
  color: z.string(),
  parent: nullableUuid(),
  is_locked: z.boolean(),
  archived_at: nullableDate(),
  workspace: uuid(),
  created_at: dateTime(),
  updated_at: dateTime(),
  created_by: nullableUuid(),
  updated_by: nullableUuid(),
  view_props: z.record(z.string(), z.unknown()),
  logo_props: PageMark,
  project_ids: z.array(uuid()),
  description_html: z.string(),
});
export type PageDuplicateResult = z.infer<typeof PageDuplicateResult>;

/**
 * Query for the collection read; no parameters means top-level rows. Nested
 * pages (D17) add exactly one parameter: `parent` lists that page's children
 * with the same visibility, ordering and favourite-first rule. A search
 * spans every depth.
 */
export interface PageListQuery {
  /** Return the children of this page instead of the top-level rows. */
  parent?: string | undefined;
  /** Name search (the only server-side search field). */
  search?: string | undefined;
  /** Include shelved rows in the collection/children read. */
  includeArchived?: boolean | undefined;
}

const pagesPath = (slug: string, projectId: string): string => `/api/workspaces/${slug}/projects/${projectId}/pages/`;

const pagePath = (slug: string, projectId: string, pageId: string): string => `${pagesPath(slug, projectId)}${pageId}/`;

/** Read the project's page collection. */
export async function listPages(
  client: ApiClient,
  slug: string,
  projectId: string,
  query: PageListQuery = {}
): Promise<PageRows> {
  const path = pagesPath(slug, projectId);
  const body = await client.getJson<unknown>(path, {
    query: {
      parent: query.parent,
      search: query.search,
      include_archived: query.includeArchived === true ? true : undefined,
    },
  });
  return client.parse(PageRows, body, `GET ${path}`);
}

/** Read one page's record and body markup. */
export async function getPage(
  client: ApiClient,
  slug: string,
  projectId: string,
  pageId: string,
  options: { trackVisit?: boolean } = {}
): Promise<PageDetail> {
  const path = pagePath(slug, projectId, pageId);
  const body = await client.getJson<unknown>(path, {
    query: { track_visit: options.trackVisit ?? true },
  });
  return client.parse(PageDetail, body, `GET ${path}`);
}

/** Create a page: the name is required, everything else is optional. */
export interface PageCreateInput {
  name: string;
  access?: number;
  parent?: string | null;
  logo_props?: PageMark;
  description_html?: string;
  description_json?: Record<string, unknown>;
  /** Base64 of the editor's Yjs binary. */
  description_binary?: string;
}

/** Create a page owned by the caller; answers with the created detail. */
export async function createPage(
  client: ApiClient,
  slug: string,
  projectId: string,
  input: PageCreateInput
): Promise<PageDetail> {
  const path = pagesPath(slug, projectId);
  const envelope = await client.sendJson<unknown>("POST", path, { json: input });
  return client.parse(PageDetail, envelope.data, `POST ${path}`);
}

/** Fields the record patch accepts; every one is independent. */
export interface PagePatch {
  name?: string;
  color?: string;
  access?: number;
  parent?: string | null;
  logo_props?: PageMark;
  is_locked?: boolean;
}

/**
 * Patch the record (title, mark, visibility, parent, lock). A locked page
 * answers 400 `Page is locked`; access changes by a non-owner answer 400.
 */
export async function updatePage(
  client: ApiClient,
  slug: string,
  projectId: string,
  pageId: string,
  input: PagePatch
): Promise<PageUpdate> {
  const path = pagePath(slug, projectId, pageId);
  const envelope = await client.sendJson<unknown>("PATCH", path, { json: input });
  return client.parse(PageUpdate, envelope.data, `PATCH ${path}`);
}

/**
 * Delete an archived page. A live page answers 400 "The page should be
 * archived before deleting"; a non-owner, non-admin answers 403.
 */
export async function deletePage(client: ApiClient, slug: string, projectId: string, pageId: string): Promise<void> {
  await client.delete(pagePath(slug, projectId, pageId));
}

/**
 * Copy a page. The copy keeps body, mark and visibility, is owned by the
 * caller, and is detached from any parent; answers with the copy.
 */
export async function duplicatePage(
  client: ApiClient,
  slug: string,
  projectId: string,
  pageId: string
): Promise<PageDuplicateResult> {
  const path = `${pagePath(slug, projectId, pageId)}duplicate/`;
  const envelope = await client.sendJson<unknown>("POST", path, {});
  return client.parse(PageDuplicateResult, envelope.data, `POST ${path}`);
}

/**
 * Thin page lock for the shell and the pages detail: answers 204. The lock
 * freezes record and description writes until released.
 */
export async function lockPage(client: ApiClient, slug: string, projectId: string, pageId: string): Promise<void> {
  await client.post(`${pagePath(slug, projectId, pageId)}lock/`, {});
}

/** Release the page lock; answers 204. */
export async function unlockPage(client: ApiClient, slug: string, projectId: string, pageId: string): Promise<void> {
  await client.delete(`${pagePath(slug, projectId, pageId)}lock/`);
}

/**
 * Switch visibility (0 open, 1 restricted); answers 204. Only the page owner
 * may change it — the server answers 400 for anyone else.
 */
export async function setPageAccess(
  client: ApiClient,
  slug: string,
  projectId: string,
  pageId: string,
  access: number
): Promise<void> {
  await client.post(`${pagePath(slug, projectId, pageId)}access/`, { json: { access } });
}

/** Archive result: the stamp the shelved marker shows. */
export const PageArchiveResult = z.object({
  archived_at: z.string(),
});
export type PageArchiveResult = z.infer<typeof PageArchiveResult>;

/**
 * Archive (shelve) a page; only the owner or an admin may. The stamp comes
 * back as the server's naive datetime string, not ISO 8601.
 */
export async function archivePage(
  client: ApiClient,
  slug: string,
  projectId: string,
  pageId: string
): Promise<PageArchiveResult> {
  const path = `${pagePath(slug, projectId, pageId)}archive/`;
  const envelope = await client.sendJson<unknown>("POST", path, {});
  return client.parse(PageArchiveResult, envelope.data, `POST ${path}`);
}

/** Restore a shelved page; answers 204. */
export async function restorePage(client: ApiClient, slug: string, projectId: string, pageId: string): Promise<void> {
  await client.delete(`${pagePath(slug, projectId, pageId)}archive/`);
}

/** Star a page through its own favourite route; answers 204. */
export async function addPageFavorite(
  client: ApiClient,
  slug: string,
  projectId: string,
  pageId: string
): Promise<void> {
  await client.post(`/api/workspaces/${slug}/projects/${projectId}/favorite-pages/${pageId}/`, {});
}

/** Drop the page's star; answers 204. */
export async function removePageFavorite(
  client: ApiClient,
  slug: string,
  projectId: string,
  pageId: string
): Promise<void> {
  await client.delete(`/api/workspaces/${slug}/projects/${projectId}/favorite-pages/${pageId}/`);
}

/**
 * Move a page into another project (PAGE-017). Cloud editions serve the
 * route; the community build has no route at all (404). The caller gates
 * the affordance on the edition flag.
 */
export interface PageMoveInput {
  new_project_id: string;
}

export async function movePageToProject(
  client: ApiClient,
  slug: string,
  projectId: string,
  pageId: string,
  input: PageMoveInput
): Promise<void> {
  await client.post(`${pagePath(slug, projectId, pageId)}move/`, { json: input });
}

/** One entry in the page's version history (list projection). */
export const PageVersion = z.object({
  id: uuid(),
  workspace: uuid(),
  page: uuid(),
  last_saved_at: dateTime(),
  owned_by: uuid(),
  created_at: dateTime(),
  updated_at: dateTime(),
  created_by: nullableUuid(),
  updated_by: nullableUuid(),
});
export type PageVersion = z.infer<typeof PageVersion>;

export const PageVersionList = z.array(PageVersion);
export type PageVersionList = z.infer<typeof PageVersionList>;

/** One snapshot with every stored form of the body. */
export const PageVersionDetail = z.object({
  id: uuid(),
  workspace: uuid(),
  page: uuid(),
  last_saved_at: dateTime(),
  description_binary: z.nullable(z.string()),
  description_html: z.string(),
  description_json: z.record(z.string(), z.unknown()),
  owned_by: uuid(),
  created_at: dateTime(),
  updated_at: dateTime(),
  created_by: nullableUuid(),
  updated_by: nullableUuid(),
});
export type PageVersionDetail = z.infer<typeof PageVersionDetail>;

/** Read the page's version history, newest first. */
export async function listPageVersions(
  client: ApiClient,
  slug: string,
  projectId: string,
  pageId: string
): Promise<PageVersionList> {
  const path = `${pagePath(slug, projectId, pageId)}versions/`;
  const body = await client.getJson<unknown>(path);
  return client.parse(PageVersionList, body, `GET ${path}`);
}

/** Read one version's snapshot. */
export async function getPageVersion(
  client: ApiClient,
  slug: string,
  projectId: string,
  pageId: string,
  versionId: string
): Promise<PageVersionDetail> {
  const path = `${pagePath(slug, projectId, pageId)}versions/${versionId}/`;
  const body = await client.getJson<unknown>(path);
  return client.parse(PageVersionDetail, body, `GET ${path}`);
}

/**
 * Read the editor binary. The endpoint answers application/octet-stream and
 * an empty body when the page has no binary yet; a transport without
 * arrayBuffer fails closed with a contract error rather than text bytes,
 * which would mangle non-UTF-8 Yjs content.
 */
export async function getPageDescriptionBinary(
  client: ApiClient,
  slug: string,
  projectId: string,
  pageId: string
): Promise<Uint8Array> {
  const path = `${pagePath(slug, projectId, pageId)}description/`;
  const response = await client.get(path);
  if (typeof response.arrayBuffer !== "function") {
    throw new ApiError({
      status: response.status,
      code: "contract",
      message: `GET ${path} carried no readable bytes`,
      body: null,
    });
  }
  return new Uint8Array(await response.arrayBuffer());
}

/** Three-form description write; every form is optional. */
export interface PageDescriptionPatch {
  /** Sanitised Tiptap HTML. */
  description_html?: string;
  /** Editor JSON document. */
  description_json?: Record<string, unknown>;
  /** Base64 of the Yjs binary; the server rejects explicit null. */
  description_binary?: string;
}

/** Acknowledgement of a description write. */
export const PageDescriptionUpdate = z.object({ message: z.string() });
export type PageDescriptionUpdate = z.infer<typeof PageDescriptionUpdate>;

/**
 * Patch the description. A locked page answers 400 with the PAGE_LOCKED
 * code; a shelved page answers 400 with PAGE_ARCHIVED.
 */
export async function updatePageDescription(
  client: ApiClient,
  slug: string,
  projectId: string,
  pageId: string,
  input: PageDescriptionPatch
): Promise<PageDescriptionUpdate> {
  const path = `${pagePath(slug, projectId, pageId)}description/`;
  const envelope = await client.sendJson<unknown>("PATCH", path, { json: input });
  return client.parse(PageDescriptionUpdate, envelope.data, `PATCH ${path}`);
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
