// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Hero breakdown clicks share the cycle's preference record with the
// display and collapse writers: the click PATCHes only rich_filters,
// but its response carries the whole row. When that response lands
// after another writer's success merge, the stale display/collapse
// keys must not clobber the converged cache (NEWFRONT-1015). Each
// PATCH below applies to the stub server on arrival — like the real
// per-key endpoints — while its response waits behind a manual gate,
// so the test controls the order responses land in.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  RouterProvider,
} from "@tanstack/react-router";
import { act, render, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("../../../shared/shell/index.js", () => ({ notifyError: vi.fn(), notifySuccess: vi.fn() }));
vi.mock("../../../shared/shell/toaster.js", () => ({ notifyError: vi.fn() }));

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import {
  entityPrefsKeyPart,
  issueKeys,
  IssuesLoaderProvider,
  selectDisplayPrefs,
  useEntityPrefs,
  useUpdateDisplayPrefs,
  type DisplayPrefs,
  type IssuesEntity,
  type PrefsRecord,
} from "../../issues/index.js";
import { readCollapsedGroups, useToggleCollapsedGroup } from "../../issues/list/collapse.js";
import { CYCLES_CYCLE_ID, CYCLES_PROJECT_ID } from "../testing.js";
import { useApplyBreakdownFilter } from "./heroBreakdown.js";

const WS = "acme";
const MEMBER = "123e4567-e89b-12d3-a456-426614174010";
const ENTITY: IssuesEntity = { scope: "cycle", projectId: CYCLES_PROJECT_ID, cycleId: CYCLES_CYCLE_ID };
const PREFS_KEY = issueKeys.prefs(WS, ...entityPrefsKeyPart(ENTITY));

type WriterKind = "hero" | "display" | "collapse";

interface GatedPatch {
  kind: WriterKind;
  release: () => void;
}

/** Full user-properties row for the stub server's current record. */
function rowPayload(record: PrefsRecord): Record<string, unknown> {
  return {
    id: "123e4567-e89b-12d3-a456-426614174021",
    workspace: "123e4567-e89b-12d3-a456-426614174000",
    project: CYCLES_PROJECT_ID,
    cycle: CYCLES_CYCLE_ID,
    user: "123e4567-e89b-12d3-a456-426614174020",
    filters: record.filters,
    display_filters: record.display_filters,
    display_properties: record.display_properties,
    rich_filters: record.rich_filters,
    created_by: null,
    updated_by: null,
    created_at: "2026-10-01T00:00:00Z",
    updated_at: "2026-10-01T00:00:00Z",
  };
}

function classifyPatch(sent: Record<string, unknown>): WriterKind {
  if (sent["filters"] !== undefined) return "collapse";
  if (sent["display_filters"] !== undefined || sent["display_properties"] !== undefined) return "display";
  return "hero";
}

function renderHarness(): {
  pendingKinds: () => WriterKind[];
  release: (kind: WriterKind) => void;
  text: () => string;
  update: (partial: Partial<DisplayPrefs>) => void;
  toggle: (axis: "group_by" | "sub_group_by", key: string) => void;
  apply: (dimension: "assignee_id", value: string) => Promise<void>;
  cache: () => PrefsRecord | undefined;
  server: () => PrefsRecord;
} {
  let server: PrefsRecord = {
    filters: {},
    display_filters: { layout: "list", group_by: null, order_by: "sort_order" },
    display_properties: {},
    rich_filters: {},
  };
  const gated: GatedPatch[] = [];
  const answer = (status: number, body: unknown, url: string): unknown => ({
    status,
    url,
    headers: { get: () => null },
    text: async () => JSON.stringify(body),
  });
  createAppClient("https://api.test", (async (url: string, init: { method?: string; body?: string }) => {
    if (url.includes("/user-properties/") && (init.method ?? "GET") === "PATCH") {
      const sent = JSON.parse(init.body ?? "{}") as Record<string, unknown>;
      const kind = classifyPatch(sent);
      // The PATCH applies server-side on arrival, but its response —
      // the row as of this PATCH's processing — waits for the test.
      for (const key of ["filters", "display_filters", "display_properties", "rich_filters"] as const) {
        if (sent[key] !== undefined) server[key] = sent[key] as Record<string, unknown>;
      }
      const snapshot = rowPayload(server);
      await new Promise<void>((resolve) => gated.push({ kind, release: resolve }));
      return answer(200, snapshot, url);
    }
    if (url.includes("/user-properties/")) {
      return answer(200, rowPayload(server), url);
    }
    return answer(200, [], url);
  }) as never);

  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  let update: ((partial: Partial<DisplayPrefs>) => void) | null = null;
  let toggle: ((axis: "group_by" | "sub_group_by", key: string) => void) | null = null;
  let apply: ((dimension: "assignee_id", value: string) => Promise<void>) | null = null;
  function Probe(): React.ReactElement {
    const prefs = useEntityPrefs(WS, ENTITY);
    const writer = useUpdateDisplayPrefs(WS, ENTITY);
    const collapser = useToggleCollapsedGroup(WS, ENTITY);
    const hero = useApplyBreakdownFilter({
      workspaceSlug: WS,
      projectId: CYCLES_PROJECT_ID,
      cycleId: CYCLES_CYCLE_ID,
    });
    update = writer.updateDisplayPrefs;
    toggle = collapser.toggleCollapsedGroup;
    apply = hero.apply;
    const layout = prefs.data === undefined ? "loading" : selectDisplayPrefs(prefs.data).layout;
    return <p>{`layout:${layout}|display:${writer.isSaving}|collapse:${collapser.isSaving}|hero:${hero.pending}`}</p>;
  }
  // Both routes mount the probe: the hero click navigates to the
  // detail, and the handles stay live across that navigation.
  const rootRoute = createRootRoute({ component: () => <Probe /> });
  const listRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/$ws/projects/$projectId/cycles",
    component: () => <Probe />,
  });
  const detailRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/$ws/projects/$projectId/cycles/$cycleId",
    component: () => <Probe />,
  });
  const router = createRouter({
    routeTree: rootRoute.addChildren([listRoute, detailRoute]),
    history: createMemoryHistory({ initialEntries: [`/acme/projects/${CYCLES_PROJECT_ID}/cycles`] }),
    defaultPreload: false,
  });
  render(
    <QueryClientProvider client={queryClient}>
      <IssuesLoaderProvider>
        <RouterProvider router={router} />
      </IssuesLoaderProvider>
    </QueryClientProvider>
  );
  return {
    pendingKinds: () => gated.map((entry) => entry.kind),
    release: (kind) => gated.find((entry) => entry.kind === kind)?.release(),
    text: () => screen.getByText(/^layout:/).textContent ?? "",
    update: (partial) => update?.(partial),
    toggle: (axis, key) => toggle?.(axis, key),
    apply: (dimension, value) => apply?.(dimension, value) ?? Promise.resolve(),
    cache: () => queryClient.getQueryData<PrefsRecord>(PREFS_KEY),
    server: () => server,
  };
}

function expectConverged(harness: ReturnType<typeof renderHarness>): void {
  const cache = harness.cache();
  expect(cache?.filters).toEqual(harness.server().filters);
  expect(cache?.display_filters).toEqual(harness.server().display_filters);
  expect(cache?.display_properties).toEqual(harness.server().display_properties);
  expect(cache?.rich_filters).toEqual(harness.server().rich_filters);
}

const HERO_RICH = { assignee_id__in: MEMBER };

describe("hero breakdown prefs interleaves (NEWFRONT-1015)", () => {
  afterEach(() => {
    resetAppClient();
  });

  it("keeps a layout switch when the hero response lands after it", async () => {
    const h = renderHarness();
    await waitFor(() => expect(h.text()).toContain("layout:list"));
    // The hero PATCH applies server-side first, so its response goes
    // stale on the display keys the switch writes next.
    let heroDone!: Promise<void>;
    act(() => {
      heroDone = h.apply("assignee_id", MEMBER);
    });
    await waitFor(() => expect(h.pendingKinds()).toEqual(["hero"]));
    act(() => h.update({ layout: "kanban" }));
    await waitFor(() => expect(h.pendingKinds().sort()).toEqual(["display", "hero"]));
    act(() => h.release("display"));
    await waitFor(() => expect(h.text()).toContain("display:false"));
    expect(selectDisplayPrefs(h.cache()!).layout).toBe("kanban");
    act(() => h.release("hero"));
    await heroDone;
    expect(selectDisplayPrefs(h.cache()!).layout).toBe("kanban");
    expect(h.cache()?.rich_filters).toEqual(HERO_RICH);
    expectConverged(h);
  });

  it("keeps a layout switch when the hero response lands first", async () => {
    const h = renderHarness();
    await waitFor(() => expect(h.text()).toContain("layout:list"));
    let heroDone!: Promise<void>;
    act(() => {
      heroDone = h.apply("assignee_id", MEMBER);
    });
    await waitFor(() => expect(h.pendingKinds()).toEqual(["hero"]));
    act(() => h.update({ layout: "kanban" }));
    await waitFor(() => expect(h.pendingKinds().sort()).toEqual(["display", "hero"]));
    act(() => h.release("hero"));
    await heroDone;
    expect(h.cache()?.rich_filters).toEqual(HERO_RICH);
    act(() => h.release("display"));
    await waitFor(() => expect(h.text()).toContain("display:false"));
    expect(selectDisplayPrefs(h.cache()!).layout).toBe("kanban");
    expect(h.cache()?.rich_filters).toEqual(HERO_RICH);
    expectConverged(h);
  });

  it("keeps a group toggle when the hero response lands after it", async () => {
    const h = renderHarness();
    await waitFor(() => expect(h.text()).toContain("layout:list"));
    let heroDone!: Promise<void>;
    act(() => {
      heroDone = h.apply("assignee_id", MEMBER);
    });
    await waitFor(() => expect(h.pendingKinds()).toEqual(["hero"]));
    act(() => h.toggle("group_by", "todo"));
    await waitFor(() => expect(h.pendingKinds().sort()).toEqual(["collapse", "hero"]));
    act(() => h.release("collapse"));
    await waitFor(() => expect(h.text()).toContain("collapse:false"));
    expect(readCollapsedGroups(h.cache()?.filters ?? {}).group_by).toEqual(["todo"]);
    act(() => h.release("hero"));
    await heroDone;
    expect(readCollapsedGroups(h.cache()?.filters ?? {}).group_by).toEqual(["todo"]);
    expect(h.cache()?.rich_filters).toEqual(HERO_RICH);
    expectConverged(h);
  });

  it("keeps a group toggle when the hero response lands first", async () => {
    const h = renderHarness();
    await waitFor(() => expect(h.text()).toContain("layout:list"));
    let heroDone!: Promise<void>;
    act(() => {
      heroDone = h.apply("assignee_id", MEMBER);
    });
    await waitFor(() => expect(h.pendingKinds()).toEqual(["hero"]));
    act(() => h.toggle("group_by", "todo"));
    await waitFor(() => expect(h.pendingKinds().sort()).toEqual(["collapse", "hero"]));
    act(() => h.release("hero"));
    await heroDone;
    expect(h.cache()?.rich_filters).toEqual(HERO_RICH);
    act(() => h.release("collapse"));
    await waitFor(() => expect(h.text()).toContain("collapse:false"));
    expect(readCollapsedGroups(h.cache()!.filters).group_by).toEqual(["todo"]);
    expect(h.cache()?.rich_filters).toEqual(HERO_RICH);
    expectConverged(h);
  });
});
