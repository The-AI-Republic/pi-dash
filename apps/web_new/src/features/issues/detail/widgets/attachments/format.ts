// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Attachment presentation rules (parity/specs/issues.md §15, ISS-187–191).
// Written from the spec: the list names files, shows a human size, and
// opens the served asset URL; the upload dropzone enforces the instance
// file-size ceiling, falling back to the server default when the config
// omits it.

import type { IssueAttachment } from "@pidash/api-client";

/** Server default when the instance config carries no ceiling (5 MiB). */
export const FALLBACK_FILE_SIZE_LIMIT = 5 * 1024 * 1024;

/** The file-size ceiling in bytes: instance config first, else 5 MiB. */
export function attachmentSizeLimit(configured: number | null | undefined): number {
  if (configured === null || configured === undefined) return FALLBACK_FILE_SIZE_LIMIT;
  if (!Number.isFinite(configured) || configured <= 0) return FALLBACK_FILE_SIZE_LIMIT;
  return configured;
}

/** Human file size: kilobytes under a mebibyte, otherwise megabytes. */
export function formatAttachmentSize(bytes: number): string {
  const safe = Number.isFinite(bytes) && bytes > 0 ? bytes : 0;
  if (safe < 1024 * 1024) return `${Math.round(safe / 1024)} KB`;
  return `${Math.round(safe / (1024 * 1024))} MB`;
}

/** The ceiling as the rejection notice spells it ("5MB"). */
export function formatSizeLimit(limit: number): string {
  return `${limit / (1024 * 1024)}MB`;
}

/** Stored display name; the upload grant records it under attributes.name. */
export function attachmentName(attachment: IssueAttachment): string {
  const name = attachment.attributes["name"];
  if (typeof name === "string" && name.trim() !== "") return name;
  return "Attachment";
}

/** File name without its extension, for the delete confirmation copy. */
export function attachmentBaseName(name: string): string {
  const dot = name.lastIndexOf(".");
  if (dot <= 0) return name;
  return name.slice(0, dot);
}

/** Lower-case extension without the dot, for the row icon. */
export function attachmentExtension(name: string): string {
  const dot = name.lastIndexOf(".");
  if (dot < 0 || dot === name.length - 1) return "";
  return name.slice(dot + 1).toLowerCase();
}

/** Coarse icon family for one attachment, derived from its extension. */
export type AttachmentKind = "image" | "pdf" | "document" | "sheet" | "archive" | "file";

const IMAGE_EXTENSIONS = new Set(["png", "jpg", "jpeg", "gif", "webp", "svg", "bmp", "avif", "heic"]);
const DOCUMENT_EXTENSIONS = new Set(["doc", "docx", "odt", "rtf", "txt", "md", "csv"]);
const SHEET_EXTENSIONS = new Set(["xls", "xlsx", "ods"]);
const ARCHIVE_EXTENSIONS = new Set(["zip", "gz", "tgz", "tar", "rar", "7z"]);

export function attachmentKind(name: string): AttachmentKind {
  const extension = attachmentExtension(name);
  if (IMAGE_EXTENSIONS.has(extension)) return "image";
  if (extension === "pdf") return "pdf";
  if (SHEET_EXTENSIONS.has(extension)) return "sheet";
  if (DOCUMENT_EXTENSIONS.has(extension)) return "document";
  if (ARCHIVE_EXTENSIONS.has(extension)) return "archive";
  return "file";
}

/** True when the instance ceiling rejects the file. */
export function exceedsSizeLimit(size: number, limit: number): boolean {
  return size > limit;
}

/** Files dropped or chosen beyond the first are never uploaded. */
export function oneFileOf(files: readonly File[]): File | null {
  return files.length === 1 ? (files[0] ?? null) : null;
}

/** Rejection notice for a multi-file drop. */
export const MULTI_FILE_NOTICE = "Only one file can be attached at a time.";
/** Success notice after the three upload legs settle. */
export const ATTACHMENT_UPLOADED_TITLE = "Attachment uploaded";
/** Failure notice when any upload leg is refused. */
export const ATTACHMENT_UPLOAD_FAILED_TITLE = "The file could not be attached.";
/** Success notice after a delete settles. */
export const ATTACHMENT_REMOVED_TITLE = "Attachment removed";
/** Failure notice when a delete is refused. */
export const ATTACHMENT_DELETE_FAILED_TITLE = "Couldn't remove the attachment";

/** Rejection notice carrying the configured ceiling. */
export function oversizeNotice(limit: number): string {
  return `Files can't be larger than ${formatSizeLimit(limit)}.`;
}
