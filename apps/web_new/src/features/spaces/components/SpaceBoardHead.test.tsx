// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { createClient, type Transport } from "@pidash/api-client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

const runtime = vi.hoisted(() => ({ client: null as unknown, queryClient: null as unknown }));
vi.mock("../api/client.js", () => ({ spaceRuntime: runtime }));

import { SPACE_HEAD_DESCRIPTION_FALLBACK, SPACE_HEAD_TITLE_FALLBACK } from "../lib/boardHead.js";
import { SpaceBoardHead } from "./SpaceBoardHead.js";

function reply(status: number, body: unknown): Awaited<ReturnType<Transport>> {
  return {
    status,
    url: "",
    headers: { get: () => "application/json" },
    text: () => Promise.resolve(JSON.stringify(body)),
  };
}

function renderHead(anchor: string, transport: Transport): void {
  runtime.client = createClient({ baseUrl: "https://api.test", transport });
  runtime.queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={runtime.queryClient as QueryClient}>
      <SpaceBoardHead anchor={anchor} />
    </QueryClientProvider>
  );
}

function metaContent(selector: string): string | null {
  return document.head.querySelector(selector)?.getAttribute("content") ?? null;
}

afterEach(() => {
  document.title = "";
  for (const tag of [...document.head.querySelectorAll("meta")]) tag.remove();
  vi.restoreAllMocks();
});

describe("SpaceBoardHead", () => {
  it("claims the board tags without any image tag", async () => {
    const seen: string[] = [];
    renderHead("anchor-1", async (url) => {
      seen.push(url);
      return reply(200, { name: "Public board", description: "Team roadmap", cover_image: "https://cdn.test/c.png" });
    });
    await waitFor(() => expect(document.title).toBe("Public board"));
    expect(metaContent('meta[name="description"]')).toBe("Team roadmap");
    expect(metaContent('meta[name="robots"]')).toBe("noindex, nofollow");
    expect(metaContent('meta[property="og:title"]')).toBe("Public board");
    expect(metaContent('meta[property="og:description"]')).toBe("Team roadmap");
    expect(metaContent('meta[property="og:type"]')).toBe("website");
    expect(metaContent('meta[name="twitter:card"]')).toBe("summary");
    expect(metaContent('meta[name="twitter:title"]')).toBe("Public board");
    expect(metaContent('meta[name="twitter:description"]')).toBe("Team roadmap");
    expect(document.head.querySelector('meta[property="og:image"]')).toBeNull();
    expect(document.head.querySelector('meta[name="twitter:image"]')).toBeNull();
    expect(seen).toEqual(["https://api.test/api/public/anchor/anchor-1/meta/"]);
  });

  it("falls back to the generic tags on unknown anchors and server errors", async () => {
    renderHead("anchor-9", async () => reply(404, { detail: "unknown" }));
    await waitFor(() => expect(document.title).toBe(SPACE_HEAD_TITLE_FALLBACK));
    expect(metaContent('meta[name="description"]')).toBe(SPACE_HEAD_DESCRIPTION_FALLBACK);
    expect(metaContent('meta[name="robots"]')).toBe("noindex, nofollow");
    expect(metaContent('meta[property="og:title"]')).toBe(SPACE_HEAD_TITLE_FALLBACK);
    expect(document.head.querySelector('meta[property="og:image"]')).toBeNull();
    expect(document.head.querySelector('meta[name="twitter:image"]')).toBeNull();
  });

  it("never requests a malformed anchor and serves the fallback", async () => {
    const seen: string[] = [];
    renderHead("bad anchor", async (url) => {
      seen.push(url);
      return reply(200, {});
    });
    await waitFor(() => expect(document.title).toBe(SPACE_HEAD_TITLE_FALLBACK));
    expect(seen).toEqual([]);
  });
});
