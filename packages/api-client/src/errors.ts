// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Normalized error for every failure the fetch client can produce.

export type ApiErrorCode = "network" | "timeout" | "aborted" | "http" | "parse" | "contract";

export interface ApiErrorFields {
  [field: string]: string[];
}

export interface ApiErrorInit {
  status?: number;
  code: ApiErrorCode | string;
  message: string;
  fields?: ApiErrorFields;
  body?: unknown;
  cause?: unknown;
}

/**
 * Single error type for transport failures, HTTP error statuses, unreadable
 * bodies and contract violations. `code` carries a machine-readable reason:
 * one of the client codes above, or the server's own code (`error_code`,
 * DRF `code`) passed through untouched, in decimal for numeric codes.
 * `body` carries the parsed JSON error body when the server sent one
 * (shape readers like the quota-refusal parser validate it further, and
 * endpoints whose callers branch on a machine code read it from there);
 * null when the failure has no body to read.
 */
export class ApiError extends Error {
  readonly status: number;
  readonly code: ApiErrorCode | string;
  readonly fields: ApiErrorFields | undefined;
  readonly body: unknown;

  constructor(init: ApiErrorInit) {
    super(init.message, init.cause === undefined ? undefined : { cause: init.cause });
    this.name = "ApiError";
    this.status = init.status ?? 0;
    this.code = init.code;
    this.fields = init.fields;
    this.body = init.body ?? null;
  }
}

export function isApiError(error: unknown): error is ApiError {
  return error instanceof ApiError;
}

/** True for failures where retrying later could succeed. */
export function isRetryable(error: unknown): boolean {
  if (!isApiError(error)) return false;
  if (error.code === "network" || error.code === "timeout") return true;
  return error.status === 408 || error.status === 429 || error.status >= 500;
}
