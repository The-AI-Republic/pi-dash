// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cycles list address keys (Data layer > Routing). The list carries no
// required keys; the peek key opens one cycle's quick look over the
// list. Anything invalid falls back to absent instead of throwing, so
// shared links never break.

import * as z from "zod/mini";

export const CyclesListSearch = z.object({
  /** Open cycle peek; closing clears it back to the plain list address (CYC-004). */
  peekCycle: z.optional(z.string()),
});
export type CyclesListSearch = z.infer<typeof CyclesListSearch>;

/** Validate raw router search; invalid params fall back to absent. */
export function parseCyclesListSearch(raw: unknown): CyclesListSearch {
  try {
    return CyclesListSearch.parse(raw ?? {});
  } catch {
    return {};
  }
}
