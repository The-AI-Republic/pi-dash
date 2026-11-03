// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Catalog failure lines (written from parity/specs/cloud.md §5,
// CLOUD-030, and §6, CLOUD-024/027). A failed read names its failure:
// the server's own detail when it sent one, a reachability note when
// nothing answered at all. A refused write reports the server detail
// the same way.
import { isApiError } from "@pidash/api-client";

/** One human line for a failed status or catalog read. */
export function catalogFailureMessage(error: unknown): string {
  if (isApiError(error)) {
    if (error.status === 0) {
      return "The apps service could not be reached. Check your connection and try again.";
    }
    return error.message;
  }
  return "The apps catalog did not load. Try again.";
}

/** One human line for a refused install, removal, or connect write. */
export function writeFailureMessage(error: unknown): string {
  if (isApiError(error)) return error.message;
  return "Something went wrong. Try again.";
}
