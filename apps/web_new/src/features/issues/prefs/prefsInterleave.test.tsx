// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, render, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { issueKeys } from "../api/keys.js";
import { readCollapsedGroups, useToggleCollapsedGroup } from "../list/collapse.js";
import { useUpdateRichFilters } from "../filters/rich/useRichFilters.js";
import type { RichCondition } from "../filters/rich/expression.js";
import { IssuesLoaderProvider } from "../loaders/loaderContext.js";
import type { IssuesEntity, PrefsRecord } from "./entities.js";
import { entityPrefsKeyPart } from "./prefsQuery.js";
import { selectDisplayPrefs, useEntityPrefs, useUpdateDisplayPrefs, type DisplayPrefs } from "./displayPrefs.js";

vi.mock("../../../shared/shell/index.js", () => ({
  notifyError: vi.fn(),
  notifySuccess: vi.fn(),
}));

const UUID = "123e4567-e89b-12d3-a456-426614174000";
const UUID2 = "223e4567-e89b-12d3-a456-426614174001";
const ENTITY: IssuesEntity = { scope: "project", projectId: UUID2 };
const PREFS_KEY = issueKeys.prefs("acme", ...entityPrefsKeyPart(ENTITY));

type PatchKind = "display" | "collapse" | "rich";

function prefsPayload(record: PrefsRecord) {
  return {
    id: UUID,
    workspace: UUID,
    project: UUID2,
    user: UUID,
    filters: record.filters,
    display_filters: record.display_filters,
    display_properties: record.display_properties,
    rich_filters: record.rich_filters,
    preferences: {},
    sort_order: 1,
    created_by: null,
    updated_by: null,
    created_at: "2026-10-01T00:00:00Z",
    updated_at: "2026-10-01T00:00:00Z",
  };
}

/**
 * A server that merges each PATCH's sent keys wholesale (like the real
 * user-properties endpoints) but holds every response behind a manual gate,
 * so the test decides the order responses land in.
 */
function stubServer() {
  let server: PrefsRecord = {
    filters: {},
    display_filters: { layout: "list", group_by: null, order_by: "sort_order" },
    display_properties: {},
    rich_filters: {},
  };
  const pending: Array<{ kind: PatchKind; release: () => void }> = [];
  let responses = 0;
  createAppClient("https://api.test", (async (url: string, init: { method?: string; body?: string }) => {
    if (url.endsWith("/auth/get-csrf-token/")) {
      return {
        status: 200,
        url,
        headers: { get: () => null },
        text: async () => JSON.stringify({ csrf_token: "csrf" }),
      };
    }
    if (init.method === "PATCH") {
      const sent = JSON.parse(init.body ?? "{}") as Record<string, unknown>;
      const kind: PatchKind =
        sent["filters"] !== undefined ? "collapse" : sent["rich_filters"] !== undefined ? "rich" : "display";
      await new Promise<void>((resolve) => pending.push({ kind, release: resolve }));
      for (const key of ["filters", "display_filters", "display_properties", "rich_filters"] as const) {
        if (sent[key] !== undefined) server[key] = sent[key] as Record<string, unknown>;
      }
      responses += 1;
      return {
        status: 201,
        url,
        headers: { get: () => null },
        text: async () => JSON.stringify(prefsPayload(server)),
      };
    }
    return {
      status: 200,
      url,
      headers: { get: () => null },
      text: async () => JSON.stringify(prefsPayload(server)),
    };
  }) as never);
  return {
    pendingKinds: () => pending.map((entry) => entry.kind),
    release: (kind: PatchKind) => pending.find((entry) => entry.kind === kind)?.release(),
    responseCount: () => responses,
    server: () => server,
  };
}

const RICH_CONDITIONS: readonly RichCondition[] = [
  { key: "c1", property: "priority", operator: "in", negated: false, value: ["high"] },
];

function renderHandle() {
  const stub = stubServer();
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  let update: ((partial: Partial<DisplayPrefs>) => void) | null = null;
  let toggle: ((axis: "group_by" | "sub_group_by", key: string) => void) | null = null;
  let updateRich: ((conditions: readonly RichCondition[]) => void) | null = null;
  function Probe(): React.ReactElement {
    const prefs = useEntityPrefs("acme", ENTITY);
    const writer = useUpdateDisplayPrefs("acme", ENTITY);
    const collapser = useToggleCollapsedGroup("acme", ENTITY);
    const rich = useUpdateRichFilters("acme", ENTITY);
    update = writer.updateDisplayPrefs;
    toggle = collapser.toggleCollapsedGroup;
    updateRich = rich.updateRichFilters;
    const layout = prefs.data ? selectDisplayPrefs(prefs.data).layout : "loading";
    return <p>{`layout:${layout}|display:${writer.isSaving}|collapse:${collapser.isSaving}|rich:${rich.isSaving}`}</p>;
  }
  render(
    <QueryClientProvider client={queryClient}>
      <IssuesLoaderProvider>
        <Probe />
      </IssuesLoaderProvider>
    </QueryClientProvider>
  );
  return {
    ...stub,
    text: () => screen.getByText(/^layout:/).textContent ?? "",
    update: (partial: Partial<DisplayPrefs>) => update?.(partial),
    toggle: (axis: "group_by" | "sub_group_by", key: string) => toggle?.(axis, key),
    updateRich: (conditions: readonly RichCondition[]) => updateRich?.(conditions),
    cache: () => queryClient.getQueryData<PrefsRecord>(PREFS_KEY),
  };
}

function expectCacheMatchesServer(h: ReturnType<typeof renderHandle>) {
  const cache = h.cache();
  expect(cache?.filters).toEqual(h.server().filters);
  expect(cache?.display_filters).toEqual(h.server().display_filters);
  expect(cache?.display_properties).toEqual(h.server().display_properties);
  expect(cache?.rich_filters).toEqual(h.server().rich_filters);
}

describe("prefs mutation interleaves (NEWFRONT-1013)", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  afterEach(() => {
    resetAppClient();
  });

  it("keeps a group toggle when the layout-switch response lands after it", async () => {
    const h = renderHandle();
    await waitFor(() => expect(h.text()).toContain("layout:list"));
    act(() => h.update({ layout: "kanban" }));
    act(() => h.toggle("group_by", "todo"));
    await waitFor(() => expect(h.pendingKinds().sort()).toEqual(["collapse", "display"]));
    // The switch response carries pre-toggle filters; it must not wipe the toggle.
    act(() => h.release("display"));
    await waitFor(() => expect(h.text()).toContain("display:false"));
    expect(readCollapsedGroups(h.cache()?.filters ?? {}).group_by).toEqual(["todo"]);
    act(() => h.release("collapse"));
    await waitFor(() => expect(h.text()).toContain("collapse:false"));
    expect(selectDisplayPrefs(h.cache()!).layout).toBe("kanban");
    expect(readCollapsedGroups(h.cache()?.filters ?? {}).group_by).toEqual(["todo"]);
    expectCacheMatchesServer(h);
  });

  it("keeps a group toggle when its own response lands first", async () => {
    const h = renderHandle();
    await waitFor(() => expect(h.text()).toContain("layout:list"));
    act(() => h.update({ layout: "kanban" }));
    act(() => h.toggle("group_by", "todo"));
    await waitFor(() => expect(h.pendingKinds().sort()).toEqual(["collapse", "display"]));
    act(() => h.release("collapse"));
    await waitFor(() => expect(h.text()).toContain("collapse:false"));
    expect(selectDisplayPrefs(h.cache()!).layout).toBe("kanban");
    expect(readCollapsedGroups(h.cache()?.filters ?? {}).group_by).toEqual(["todo"]);
    act(() => h.release("display"));
    await waitFor(() => expect(h.text()).toContain("display:false"));
    expect(selectDisplayPrefs(h.cache()!).layout).toBe("kanban");
    expect(readCollapsedGroups(h.cache()?.filters ?? {}).group_by).toEqual(["todo"]);
    expectCacheMatchesServer(h);
  });

  it("keeps rich filters when a display response lands after them", async () => {
    const h = renderHandle();
    await waitFor(() => expect(h.text()).toContain("layout:list"));
    act(() => h.update({ layout: "kanban" }));
    act(() => h.updateRich(RICH_CONDITIONS));
    await waitFor(() => expect(h.pendingKinds().sort()).toEqual(["display", "rich"]));
    act(() => h.release("rich"));
    await waitFor(() => expect(h.text()).toContain("rich:false"));
    expect(selectDisplayPrefs(h.cache()!).layout).toBe("kanban");
    act(() => h.release("display"));
    await waitFor(() => expect(h.text()).toContain("display:false"));
    expect(selectDisplayPrefs(h.cache()!).layout).toBe("kanban");
    expect(h.cache()?.rich_filters).toEqual(h.server().rich_filters);
    expect(Object.keys(h.cache()?.rich_filters ?? {}).length).toBeGreaterThan(0);
    expectCacheMatchesServer(h);
  });
});
