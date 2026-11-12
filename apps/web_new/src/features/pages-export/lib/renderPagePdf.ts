// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Page-export document renderer (parity/specs/exports.md §0.5/§5, EXP-019):
// the only module that touches the PDF renderer, always through a lazy
// import() so the heavy dependency stays out of the main bundle (Stack).
// The product typeface embeds as a subset; every text style uses it.
import * as React from "react";

import { fetchBytes } from "../../../core/api/index.js";
import { platform } from "../../../core/platform/index.js";
import type { PageExportSize } from "./pageExportModel.js";

/** Page-size names the document renderer accepts. */
export type PagePdfSize = "A4" | "A3" | "A2" | "LETTER" | "LEGAL" | "TABLOID";

/** Map the dialog's size labels to the renderer's size names. */
export function pagePdfSize(size: PageExportSize): PagePdfSize {
  if (size === "Letter") return "LETTER";
  if (size === "Legal") return "LEGAL";
  if (size === "Tabloid") return "TABLOID";
  return size;
}

/** The embedded product typeface family. */
export const PAGE_PDF_FONT_FAMILY = "Inter";

/** Loads the Inter TTF bytes (the production loader fetches the asset). */
export type LoadInterFontBytes = () => Promise<Uint8Array>;

/** Fetch the Inter TTF through its lazily imported asset URL. */
export async function loadInterFontBytes(): Promise<Uint8Array> {
  const asset = await import("@expo-google-fonts/inter/400Regular/Inter_400Regular.ttf?url");
  return fetchBytes(asset.default, platform.fetch);
}

let interRegistered = false;

/** True when the bytes start with the marker at the offset. */
function bytesMatchAt(bytes: Uint8Array, offset: number, marker: readonly number[]): boolean {
  if (bytes.length < offset + marker.length) return false;
  return marker.every((code, index) => bytes[offset + index] === code);
}

/** Detect the raster format from its magic bytes (null when unknown). */
function sniffImageMime(bytes: Uint8Array): string | null {
  if (bytesMatchAt(bytes, 0, [0x89, 0x50, 0x4e, 0x47])) return "image/png";
  if (bytesMatchAt(bytes, 0, [0xff, 0xd8, 0xff])) return "image/jpeg";
  if (bytesMatchAt(bytes, 0, [0x47, 0x49, 0x46, 0x38])) return "image/gif";
  if (bytesMatchAt(bytes, 0, [0x52, 0x49, 0x46, 0x46]) && bytesMatchAt(bytes, 8, [0x57, 0x45, 0x42, 0x50])) {
    return "image/webp";
  }
  return null;
}

/** Base64 without Buffer, so the browser and Node share this path. */
function bytesToBase64(bytes: Uint8Array): string {
  let binary = "";
  const stride = 0x8000;
  for (let at = 0; at < bytes.length; at += stride) {
    binary += String.fromCharCode(...bytes.subarray(at, at + stride));
  }
  return btoa(binary);
}

/** Picture bytes as an embeddable data URI, or null when unrecognized. */
export function toExportImageDataUri(bytes: Uint8Array): string | null {
  const mime = sniffImageMime(bytes);
  if (mime === null) return null;
  return `data:${mime};base64,${bytesToBase64(bytes)}`;
}

/** Typeface bytes as a data URI: the renderer's byte-source form. */
function toFontDataUrl(bytes: Uint8Array): string {
  return `data:font/ttf;base64,${bytesToBase64(bytes)}`;
}

export interface RenderPagePdfInput {
  title: string;
  paragraphs: readonly string[];
  imageBytes: readonly Uint8Array[];
  size: PageExportSize;
}

export interface RenderPagePdfDeps {
  /** Inter TTF bytes; the production loader fetches them when absent. */
  fontBytes?: Uint8Array | undefined;
  loadFontBytes?: LoadInterFontBytes | undefined;
}

/** Render the title, paragraphs and pictures into document bytes. */
export async function renderPagePdf(input: RenderPagePdfInput, deps: RenderPagePdfDeps = {}): Promise<Uint8Array> {
  const fontBytes = deps.fontBytes ?? (await (deps.loadFontBytes ?? loadInterFontBytes)());
  const renderer = await import("@react-pdf/renderer");
  if (!interRegistered) {
    renderer.Font.register({ family: PAGE_PDF_FONT_FAMILY, src: toFontDataUrl(fontBytes) });
    interRegistered = true;
  }
  const dataUris: string[] = [];
  for (const bytes of input.imageBytes) {
    const uri = toExportImageDataUri(bytes);
    if (uri !== null) dataUris.push(uri);
  }
  const body: React.ReactNode[] = [
    React.createElement(
      renderer.Text,
      { key: "title", style: { fontFamily: PAGE_PDF_FONT_FAMILY, fontSize: 20, marginBottom: 12 } },
      input.title
    ),
    ...input.paragraphs.map((paragraph, index) =>
      React.createElement(
        renderer.Text,
        { key: `paragraph-${index}`, style: { fontFamily: PAGE_PDF_FONT_FAMILY, fontSize: 11, marginBottom: 8 } },
        paragraph
      )
    ),
    ...dataUris.map((uri, index) =>
      React.createElement(renderer.Image, { key: `image-${index}`, src: uri, style: { width: 400, marginBottom: 8 } })
    ),
  ];
  const document = React.createElement(
    renderer.Document,
    null,
    React.createElement(
      renderer.Page,
      {
        size: pagePdfSize(input.size),
        style: { fontFamily: PAGE_PDF_FONT_FAMILY, backgroundColor: "white", padding: 32 },
      },
      ...body
    )
  );
  const blob = await renderer.pdf(document).toBlob();
  return new Uint8Array(await blob.arrayBuffer());
}
