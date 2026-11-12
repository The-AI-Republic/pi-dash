// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Intake-state option shaping (parity/specs/issues.md §17/§21,
// ISS-217). The project's intake states shape through the shared
// state options; the trigger falls back to the first listed state,
// never the default flag. The intake epic owns the triage screens
// and actions; this only reads.
import type { StateList } from "@pidash/api-client";

import { selectStateOptions, type StateOption } from "./states.js";

/** Trigger fallback: the first intake state, or null when there is none. */
export function selectIntakeFallback(states: StateList): StateOption | null {
  return selectStateOptions(states)[0] ?? null;
}
