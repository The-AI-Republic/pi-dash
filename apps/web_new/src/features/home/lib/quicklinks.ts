// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Saved-link form rules (SHELL-011–012; written from parity/specs/shell.md
// §2). A link needs an address with an optional display name; a bare
// address gains its scheme automatically, and only web addresses are
// accepted. Validation reports one inline message at a time.

import type { QuickLink } from "@pidash/api-client";

export interface QuickLinkForm {
  address: string;
  name: string;
}

export interface NormalizedQuickLink {
  url: string;
  title: string;
}

export type QuickLinkValidation = { ok: true; value: NormalizedQuickLink } | { ok: false; error: string };

const SCHEME_PATTERN = /^[a-zA-Z][a-zA-Z0-9+.-]*:/;

/** Trim the address, defaulting a missing scheme to https. */
export function normalizeQuickLinkAddress(raw: string): string {
  const trimmed = raw.trim();
  if (trimmed === "" || SCHEME_PATTERN.test(trimmed)) return trimmed;
  return `https://${trimmed}`;
}

export function validateQuickLinkForm(form: QuickLinkForm): QuickLinkValidation {
  const address = form.address.trim();
  if (address === "") return { ok: false, error: "Enter the link address." };
  const url = normalizeQuickLinkAddress(address);
  let parsed: URL;
  try {
    parsed = new URL(url);
  } catch {
    return { ok: false, error: "Enter a valid link address." };
  }
  if (parsed.protocol !== "http:" && parsed.protocol !== "https:") {
    return { ok: false, error: "Enter a valid link address." };
  }
  return { ok: true, value: { url, title: form.name.trim() } };
}

/** The row label: the display name when set, otherwise the raw address. */
export function quickLinkDisplayName(link: Pick<QuickLink, "title" | "url">): string {
  const title = link.title?.trim() ?? "";
  return title === "" ? link.url : title;
}
