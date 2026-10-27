// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Stored-embed heal planner (parity/specs/pages.md §6.2, PAGE-040,
// NEWFRONT-932). A stored body whose HTML lost its embed tags still
// carries the embeds in its structured snapshot. The planner diffs that
// snapshot against the mounted document and names the embeds to restore,
// in snapshot order, with the top-level index each one restores. It
// never names prose: the mounted body's own blocks stay exactly where
// they are.

import { PAGE_EMBED_TAG, type PageEmbedAttributes } from "./pageBlocks.js";

/** One embed the snapshot carries that the mounted document lacks. */
export interface PageEmbedHealInsert {
  attributes: PageEmbedAttributes;
  /** The embed's top-level index in the snapshot, for placement. */
  index: number;
}

/** A structured node in either JSON form. */
interface HealNode {
  type?: unknown;
  attrs?: unknown;
  content?: unknown;
}

function nodeOf(value: unknown): HealNode | null {
  if (typeof value !== "object" || value === null) return null;
  return value as HealNode;
}

function nodeList(value: unknown): HealNode[] {
  if (!Array.isArray(value)) return [];
  const out: HealNode[] = [];
  for (const entry of value) {
    const node = nodeOf(entry);
    if (node !== null) out.push(node);
  }
  return out;
}

function attrsOf(value: unknown): Record<string, unknown> {
  if (typeof value !== "object" || value === null) return {};
  return value as Record<string, unknown>;
}

function stringOf(attrs: Record<string, unknown>, name: string): string {
  const value = attrs[name];
  return typeof value === "string" ? value : "";
}

function embedAttributesOf(node: HealNode): PageEmbedAttributes {
  const attrs = attrsOf(node.attrs);
  return {
    id: stringOf(attrs, "id"),
    entity_identifier: stringOf(attrs, "entity_identifier"),
    project_identifier: stringOf(attrs, "project_identifier"),
    workspace_identifier: stringOf(attrs, "workspace_identifier"),
    entity_name: stringOf(attrs, "entity_name"),
  };
}

/** The embed's match key: its work-item identifier, else its stored id. */
function embedKey(attributes: PageEmbedAttributes): string {
  return attributes.entity_identifier !== "" ? attributes.entity_identifier : attributes.id;
}

/** Every top-level snapshot embed, in document order. */
function snapshotEmbeds(snapshot: unknown): PageEmbedHealInsert[] {
  const root = nodeOf(snapshot);
  if (root === null) return [];
  const embeds: PageEmbedHealInsert[] = [];
  const top = nodeList(root.content);
  for (let index = 0; index < top.length; index += 1) {
    const node = top[index] as HealNode;
    if (node.type === PAGE_EMBED_TAG) embeds.push({ attributes: embedAttributesOf(node), index });
  }
  return embeds;
}

/** How many times each embed key occurs anywhere in the mounted document. */
function presentEmbedCounts(document: unknown): Map<string, number> {
  const counts = new Map<string, number>();
  const visit = (node: HealNode): void => {
    if (node.type === PAGE_EMBED_TAG) {
      const key = embedKey(embedAttributesOf(node));
      counts.set(key, (counts.get(key) ?? 0) + 1);
    }
    for (const child of nodeList(node.content)) visit(child);
  };
  const root = nodeOf(document);
  if (root !== null) visit(root);
  return counts;
}

/**
 * The snapshot embeds the mounted document lacks, in snapshot order.
 * Each entry restores at its snapshot index; a snapshot that carries
 * no embeds, or a document that already carries every one, heals
 * nothing. Matching is by work-item identifier, counted per occurrence,
 * so a snapshot that embeds one item twice restores both copies.
 */
export function missingPageEmbedInserts(snapshot: unknown, document: unknown): PageEmbedHealInsert[] {
  const wanted = snapshotEmbeds(snapshot);
  if (wanted.length === 0) return [];
  const present = presentEmbedCounts(document);
  const inserts: PageEmbedHealInsert[] = [];
  for (const embed of wanted) {
    const key = embedKey(embed.attributes);
    const remaining = present.get(key) ?? 0;
    if (remaining > 0) {
      present.set(key, remaining - 1);
      continue;
    }
    inserts.push(embed);
  }
  return inserts;
}
