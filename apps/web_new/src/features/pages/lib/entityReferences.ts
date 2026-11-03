// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Pasted entity-reference parser (parity/specs/pages.md §6.3, §11.5,
// PAGE-043). A bare work-item key or app address resolves to a preview node
// instead of staying a plain link. The parser is pure so paste handling and
// tests agree on exactly which forms are recognised; the resolver searches
// the shared entity-preview contract and the editor stores the normalised
// identifiers.

/** A parsed work-item address or key from pasted text. */
export interface ParsedEntityAddress {
  /** Key as written, e.g. WEB-42. */
  key: string;
  /** Project code from the key, e.g. WEB. */
  projectIdentifier: string;
  /** Sequence number from the key. */
  sequenceId: number;
}

const WORK_ITEM_KEY = /^([A-Za-z][A-Za-z0-9]*)-(\d+)$/;
const BROWSE_PATH = /\/browse\/([A-Za-z][A-Za-z0-9]*-[0-9]+)\/?$/;

/**
 * Read one bare work-item key or an app browse address. Anything carrying
 * prose, a query or a different route stays a normal link. `projectCode`
 * may be supplied when the address is just a sequence key with no prefix.
 */
export function parseEntityAddress(text: string): ParsedEntityAddress | null {
  const trimmed = text.trim();
  if (trimmed === "") return null;
  let candidate = trimmed;
  if (/^https?:\/\//i.test(trimmed)) {
    let url: URL;
    try {
      url = new URL(trimmed);
    } catch {
      return null;
    }
    const match = BROWSE_PATH.exec(url.pathname);
    if (match === null || url.search !== "" || url.hash !== "") return null;
    candidate = match[1] ?? "";
  }
  const match = WORK_ITEM_KEY.exec(candidate);
  if (match === null) return null;
  const projectIdentifier = match[1] ?? "";
  const sequenceId = Number(match[2]);
  if (projectIdentifier === "" || !Number.isSafeInteger(sequenceId) || sequenceId <= 0) return null;
  return { key: candidate.toUpperCase(), projectIdentifier: projectIdentifier.toUpperCase(), sequenceId };
}
