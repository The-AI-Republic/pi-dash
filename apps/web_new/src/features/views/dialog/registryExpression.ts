// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Read the dialog's filter expression (parity/specs/views.md §5,
// VIEW-013). The dialog hosts a temporary builder instance that reports
// its live conditions into a registry; submitting serializes whatever it
// holds (an untouched builder reads as the empty expression).

import { serializeRichExpression, type FieldConfigRegistry, type RichExpression } from "../../issues/index.js";

/** The builder's live conditions as the stored expression tree. */
export function readRegistryExpression(registry: FieldConfigRegistry): RichExpression {
  // Every field entry holds the same full conditions array; one read is
  // the whole expression. No mounted field means no conditions at all.
  const first = registry.activeProperties()[0];
  if (first === undefined) return {};
  // The key comes from the registry's own listing, so its entry exists.
  return serializeRichExpression(registry.read(first)!);
}
