// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Issues beforeLoad denial tolerance (AUTH-024 seam). The project gate
// owns 403/404/409 from the issues read; anything else still throws to
// the load error. Only the tolerance is tested here — the list itself
// belongs to the issues area.
import { QueryClient } from "@tanstack/react-query";
import { ApiError, createClient } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import { issuesQueryOptions } from "../../../../../../features/issues/index.js";
import { Route } from "./index.js";

const PARAMS = { ws: "acme", projectId: "223e4567-e89b-12d3-a456-426614174001" };

function failingClient(status: number) {
  return createClient({
    baseUrl: "https://api.test",
    transport: async (url) => ({
      status,
      url,
      headers: { get: () => null },
      text: async () => "{}",
    }),
  });
}

async function load(apiClient: ReturnType<typeof failingClient>): Promise<{ ok: unknown } | { thrown: unknown }> {
  const beforeLoad = Route.options.beforeLoad;
  if (!beforeLoad) throw new Error("missing beforeLoad");
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return beforeLoad({ context: { queryClient, apiClient }, params: PARAMS, search: {} } as never).then(
    (value: unknown) => ({ ok: value }),
    (error: unknown) => ({ thrown: error })
  );
}

describe("issues denial tolerance", () => {
  it("loads through when the issues read succeeds", async () => {
    const apiClient = failingClient(200);
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    queryClient.setQueryData(issuesQueryOptions(apiClient, PARAMS.ws, PARAMS.projectId, {} as never).queryKey, {
      grouped_by: null,
    } as never);
    const beforeLoad = Route.options.beforeLoad;
    if (!beforeLoad) throw new Error("missing beforeLoad");
    await expect(
      beforeLoad({ context: { queryClient, apiClient }, params: PARAMS, search: {} } as never)
    ).resolves.toBeUndefined();
  });

  it.each([403, 404, 409])("lets a %s through to the project gate", async (status) => {
    await expect(load(failingClient(status))).resolves.toEqual({ ok: undefined });
  });

  it("throws anything else to the load error", async () => {
    const outcome = await load(failingClient(500));
    expect("thrown" in outcome && outcome.thrown).toBeInstanceOf(ApiError);
  });

  it("throws transport failures to the load error", async () => {
    const offline = createClient({
      baseUrl: "https://api.test",
      transport: () => {
        throw new Error("offline");
      },
    });
    const outcome = await load(offline);
    expect("thrown" in outcome && outcome.thrown).toBeInstanceOf(Error);
  });
});
