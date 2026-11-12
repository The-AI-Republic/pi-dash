// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public board runtime. A visitor has no workspace session, so this client
// shares the app origin and transport but deliberately omits the session
// middleware: a 401 here means "anonymous", never "session expired".

import { QueryClient } from "@tanstack/react-query";
import { createClient, type ApiClient, type Transport } from "@pidash/api-client";

import { platform, toApiTransport } from "../../../core/platform/index.js";

function publicBaseUrl(): string {
  if (platform.apiOrigin !== undefined) return platform.apiOrigin;
  if (typeof globalThis.window !== "undefined" && globalThis.window.location?.origin) {
    return globalThis.window.location.origin;
  }
  return "http://localhost:8000";
}

export interface SpaceRuntime {
  readonly client: ApiClient;
  readonly queryClient: QueryClient;
}

/** Build one isolated public runtime; nothing here touches the workspace cache. */
export function createSpaceRuntime(baseUrl: string = publicBaseUrl(), transport?: Transport): SpaceRuntime {
  const client = createClient({
    baseUrl,
    transport: transport ?? toApiTransport(platform),
  });
  return {
    client,
    queryClient: new QueryClient({
      defaultOptions: {
        queries: {
          staleTime: 30_000,
          refetchOnWindowFocus: false,
          refetchOnReconnect: false,
          retry: false,
        },
      },
    }),
  };
}

export const spaceRuntime = createSpaceRuntime();
