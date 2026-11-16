// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// JSON calls behind the spaces auth card (parity/specs/space.md §5.4):
// the address check, magic-code generation, and the CSRF read the native
// forms post back. Mounts inject their own client through
// createSpaceAuthServices; the default builds a public-safe client over
// same-origin fetch with no session middleware, so the spaces bundle
// never imports workspace-shell auth state. Submits themselves are native
// form posts the browser follows — this module never posts them.
import {
  checkSpaceEmail,
  createClient,
  generateSpaceMagicCode,
  getCsrfToken,
  isApiError,
  type ApiClient,
} from "@pidash/api-client";
import * as React from "react";

import type { SpaceAuthCheckAnswer } from "./spaceAuthMachine.js";

export interface SpaceAuthServices {
  /** Run the address check; throws ApiError carrying the server's code. */
  checkEmail(email: string): Promise<SpaceAuthCheckAnswer>;
  /** Generate a fresh code; the server keeps it, the card needs no key. */
  generateCode(email: string): Promise<void>;
  /** Read the CSRF token the native forms post back. */
  readCsrfToken(): Promise<string>;
}

/** Bind the spaces JSON calls to a caller-owned client. */
export function createSpaceAuthServices(client: ApiClient): SpaceAuthServices {
  return {
    checkEmail: (email) => checkSpaceEmail(client, email),
    generateCode: async (email) => {
      await generateSpaceMagicCode(client, email);
    },
    readCsrfToken: async () => (await getCsrfToken(client)).csrf_token,
  };
}

/**
 * Machine-readable code of a failed call: the server's number when it
 * answered one, otherwise the transport outcome. The failure catalog
 * turns numbers into banners or silence and explains the rest.
 */
export function spaceAuthFailureCode(error: unknown): string {
  if (isApiError(error)) return String(error.code);
  return "network";
}

const FALLBACK_ORIGIN = "http://localhost:8000";

/** Same-origin API base; spaces ships web-only, so the window owns it. */
function defaultOrigin(): string {
  if (typeof globalThis.window === "undefined") return FALLBACK_ORIGIN;
  return globalThis.window.location.origin;
}

/** Default services over a fresh public-safe client. */
export function createDefaultSpaceAuthServices(): SpaceAuthServices {
  return createSpaceAuthServices(createClient({ baseUrl: defaultOrigin() }));
}

export interface SpaceCsrfToken {
  /** The token once read; submits wait for it. */
  token: string | null;
  /** True once the read has failed; the step banners and stays shut. */
  failed: boolean;
}

/** Read the CSRF token when the step renders; late answers are dropped. */
export function useSpaceCsrfToken(services: SpaceAuthServices): SpaceCsrfToken {
  const [token, setToken] = React.useState<string | null>(null);
  const [failed, setFailed] = React.useState(false);
  React.useEffect(() => {
    let live = true;
    services.readCsrfToken().then(
      (value) => {
        if (live) setToken(value);
      },
      () => {
        if (live) setFailed(true);
      }
    );
    return () => {
      live = false;
    };
  }, [services]);
  return { token, failed };
}
