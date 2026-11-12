// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// A read-only card names its estimate (EST-022): guests, archived items
// and completed cycles mount the strip disabled, and the chip still reads
// the stored value while pressing it opens no menu. The card renders the
// real strip here, so this pins the shared read-only clause end to end.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { IssueListItem } from "@pidash/api-client";
import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";

vi.mock("../../../shared/shell/narrowViewport.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../shared/shell/narrowViewport.js")>()),
  useNarrowViewport: () => false,
}));

import { KanbanCard } from "./KanbanCard.js";

const SLUG = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const ISSUE = "123e4567-e89b-12d3-a456-426614174000";
const STATE = "323e4567-e89b-12d3-a456-426614174002";
const SYSTEM = "423e4567-e89b-12d3-a456-426614174003";
const POINT = "523e4567-e89b-12d3-a456-426614174004";

function issue(overrides: Partial<IssueListItem> = {}): IssueListItem {
  return {
    id: ISSUE,
    name: "Fix login",
    state_id: STATE,
    sort_order: 1,
    completed_at: null,
    priority: "none",
    sequence_id: 42,
    project_id: PROJECT,
    parent_id: null,
    cycle_id: null,
    module_ids: [],
    label_ids: [],
    assignee_ids: [],
    sub_issues_count: null,
    attachment_count: null,
    link_count: null,
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-02-03T04:05:06Z",
    created_by: null,
    updated_by: null,
    is_draft: false,
    archived_at: null,
    ...overrides,
  };
}

function answer(url: string, body: unknown) {
  return { status: 200, url, headers: { get: () => null }, text: async () => JSON.stringify(body) };
}

function stubEstimates(type: "points" | "time", value: string): void {
  createAppClient("https://api.test", (async (url: string) => {
    if (url.includes(`/projects/${PROJECT}/estimates/`)) {
      return answer(url, [
        {
          id: SYSTEM,
          workspace: PROJECT,
          project: PROJECT,
          name: "Sizes",
          description: "",
          type,
          last_used: true,
          points: [{ id: POINT, key: 2, value, description: "" }],
          created_at: "2026-01-01T00:00:00Z",
          updated_at: "2026-01-01T00:00:00Z",
          created_by: null,
          updated_by: null,
        },
      ]);
    }
    return answer(url, []);
  }) as never);
}

function renderCard(props: Partial<React.ComponentProps<typeof KanbanCard>> = {}) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={queryClient}>
      <KanbanCard
        workspaceSlug={SLUG}
        projectId={PROJECT}
        projectIdentifier="WEB"
        issue={issue({ estimate_point: POINT })}
        groupKey={STATE}
        laneKey={null}
        properties={{}}
        flags={{ hideCycle: false, hideModule: false }}
        readOnly
        peeked={false}
        estimateEnabled
        states={[]}
        stateNames={new Map()}
        assigneeNames={new Map()}
        labelNames={new Map()}
        cycleNames={new Map()}
        moduleNames={new Map()}
        openCycleIds={new Set()}
        timeZone={null}
        href="/acme/browse/WEB-42"
        onOpen={vi.fn()}
        onCopyLink={vi.fn()}
        {...props}
      />
    </QueryClientProvider>
  );
}

afterEach(() => resetAppClient());

describe("KanbanCard read-only estimate (EST-022)", () => {
  it("shows a guest the stored value on a disabled chip that opens nothing", async () => {
    stubEstimates("points", "3");
    renderCard();
    const chip = await screen.findByTestId("row-estimate-trigger");
    expect(await screen.findByText("3")).toBeInTheDocument();
    expect(chip).toBeDisabled();
    expect(chip).toHaveTextContent("3");
    fireEvent.pointerEnter(chip);
    fireEvent.click(chip);
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
  });

  it("formats a time point and names a point from a superseded system", async () => {
    stubEstimates("time", "90");
    const first = renderCard();
    expect(await screen.findByText("1h 30m")).toBeInTheDocument();
    first.unmount();

    stubEstimates("points", "3");
    renderCard({ issue: issue({ estimate_point: POINT, archived_at: "2026-06-01" }) });
    expect(await screen.findByText("3")).toBeInTheDocument();
    expect(screen.getByTestId("row-estimate-trigger")).toBeDisabled();
  });

  it("keeps the empty placeholder on an unestimated card without reading estimates", () => {
    const transport = vi.fn(async (url: string) => answer(url, []));
    createAppClient("https://api.test", transport as never);
    renderCard({ issue: issue({ estimate_point: null }) });
    expect(screen.getByTestId("row-estimate-trigger")).toHaveTextContent("None");
    expect(transport).not.toHaveBeenCalled();
  });
});
