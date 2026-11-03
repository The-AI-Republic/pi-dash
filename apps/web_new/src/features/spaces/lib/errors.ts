// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Failure classification for the public board. Only an explicit 404 is the
// anchor-not-found state; transport failures, 5xx and unparseable payloads
// are board errors (parity/specs/space.md §3.1, SPC-003/SPC-004).

import { ApiError, isApiError } from "@pidash/api-client";

export type SpaceFailureKind = "not-found" | "error";

export function classifySpaceFailure(error: unknown): SpaceFailureKind {
  return isApiError(error) && error.status === 404 ? "not-found" : "error";
}

export function spaceErrorStatus(error: unknown): number | null {
  return error instanceof ApiError ? error.status : null;
}
