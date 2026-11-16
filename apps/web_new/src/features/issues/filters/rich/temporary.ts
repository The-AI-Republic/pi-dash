// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Temporary filter instances (parity/specs/issues.md §9, ISS-105). A
// builder mounted with `temporary` gets a client id prefixed TEMP-
// instead of an entity, edits its expression in memory only, and reports
// its live field configurations to a registry for as long as it is
// mounted. The registry is per field, so a surface that remounts a
// builder can read what the previous instance had before it disposed its
// entry.

import * as React from "react";

import type { RichCondition } from "./expression.js";

/** Id prefix every ephemeral builder instance gets. */
export const TEMPORARY_PREFIX = "TEMP-";

let sequence = 0;

/** Allocate a client-only instance id; never sent to the server. */
export function temporaryInstanceId(): string {
  sequence += 1;
  return `${TEMPORARY_PREFIX}${sequence}`;
}

/** True for ids this module minted (or any caller passed with the prefix). */
export function isTemporaryId(id: string): boolean {
  return id.startsWith(TEMPORARY_PREFIX);
}

/**
 * Per-field configuration registry. Each mounted builder owns one entry
 * keyed by its field property; a later instance of the same field reads
 * the last live entry. Unmounting removes only the entry it added, so a
 * remount inside one render pass never drops the other field's config.
 */
export class FieldConfigRegistry {
  private readonly entries = new Map<string, readonly RichCondition[]>();

  register(property: string, conditions: readonly RichCondition[]): void {
    this.entries.set(property, conditions);
  }

  read(property: string): readonly RichCondition[] | undefined {
    return this.entries.get(property);
  }

  dispose(property: string): void {
    this.entries.delete(property);
  }

  /** Field properties currently mounted. */
  activeProperties(): string[] {
    return [...this.entries.keys()];
  }
}

/** Default registry shared by temporary instances in one client session. */
export const temporaryRegistry = new FieldConfigRegistry();

/**
 * Register one temporary builder's conditions for each field it holds
 * while it is mounted. Every edit refreshes the held entries; fields the
 * edit dropped are released at once, and unmount releases the rest.
 */
export function useTemporaryFieldConfigs(
  conditions: readonly RichCondition[],
  registry: FieldConfigRegistry = temporaryRegistry
): void {
  // Each instance reports into the registry its host names.
  const properties = React.useMemo(
    () => [
      ...new Set(conditions.filter((condition) => condition.property !== "").map((condition) => condition.property)),
    ],
    [conditions]
  );
  const held = React.useRef<string[]>([]);
  React.useEffect(() => {
    for (const property of held.current) {
      if (!properties.includes(property)) registry.dispose(property);
    }
    for (const property of properties) registry.register(property, conditions);
    held.current = properties;
  }, [properties, conditions, registry]);
  React.useEffect(
    () => () => {
      for (const property of held.current) registry.dispose(property);
      held.current = [];
    },
    [registry]
  );
}
