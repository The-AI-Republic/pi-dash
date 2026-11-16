// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Label hierarchy (parity/specs/issues.md §19, ISS-229): the flat label
// list derives a tree at most two levels deep through parent links,
// sorted by the ordering value. Labels whose parent is missing, self,
// or itself a child render at top level so a bad link never strands a
// row. The settings list and the shared picker both read this tree.

import type { Label } from "@pidash/api-client";

/** One top-level label with its children, both sorted by sort_order. */
export interface LabelGroup {
  label: Label;
  children: Label[];
}

/** Ascending ordering-value comparison for label rows. */
export function compareLabelOrder(a: Pick<Label, "sort_order">, b: Pick<Label, "sort_order">): number {
  return a.sort_order - b.sort_order;
}

/**
 * Derive the two-level tree from the flat list. Groups and children
 * sort by the ordering value; orphan, self-parented, and grandchild
 * rows render at top level.
 */
export function selectLabelTree(labels: readonly Label[]): LabelGroup[] {
  const byId = new Map<string, Label>();
  for (const label of labels) byId.set(label.id, label);
  const topIds = new Set<string>();
  for (const label of labels) {
    const parent = label.parent;
    if (parent === null || parent === label.id || !byId.has(parent)) topIds.add(label.id);
  }
  const top: Label[] = [];
  const kids = new Map<string, Label[]>();
  for (const label of labels) {
    const parent = label.parent;
    if (parent !== null && parent !== label.id && topIds.has(parent)) {
      const list = kids.get(parent) ?? [];
      list.push(label);
      kids.set(parent, list);
    } else {
      top.push(label);
    }
  }
  top.sort(compareLabelOrder);
  return top.map((label) => ({ label, children: (kids.get(label.id) ?? []).sort(compareLabelOrder) }));
}

/**
 * Narrow the tree to a search query (the shared picker): a group stays
 * when its own name matches or any child does, keeping only matching
 * children in the latter case. A blank query returns the whole tree.
 */
export function filterLabelTree(tree: readonly LabelGroup[], query: string): LabelGroup[] {
  const needle = query.trim().toLowerCase();
  if (needle === "") return [...tree];
  const out: LabelGroup[] = [];
  for (const group of tree) {
    if (group.label.name.toLowerCase().includes(needle)) {
      out.push(group);
      continue;
    }
    const children = group.children.filter((child) => child.name.toLowerCase().includes(needle));
    if (children.length > 0) out.push({ label: group.label, children });
  }
  return out;
}

/**
 * Exact case-insensitive name lookup for picker deduplication: typing
 * an existing name selects it instead of creating a duplicate. Blank
 * queries match nothing.
 */
export function findLabelByName(labels: readonly Label[], name: string): Label | null {
  const needle = name.trim().toLowerCase();
  if (needle === "") return null;
  return labels.find((label) => label.name.toLowerCase() === needle) ?? null;
}
