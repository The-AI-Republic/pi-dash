// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Pods section (NEWFRONT-542, RUN-012). Every pod of the scope gets a tile
// carrying its name, default marker and runner count; a tile toggles the
// table filter; the tile menu opens the edit and delete dialogs; the last
// entry creates a pod; a failed read replaces the tiles with its message.
import type { Pod } from "@pidash/api-client";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import {
  PODS_CREATE_LABEL,
  PODS_EMPTY,
  PODS_LOAD_FAILED,
  PODS_SECTION_HINT,
  PODS_SECTION_TITLE,
  RunnersPods,
  podRunnerCountLabel,
  podTileLabel,
} from "./RunnersPods.js";

function pod(id: string, overrides: Partial<Pod> = {}): Pod {
  return {
    id,
    name: `WEB_${id}`,
    description: "",
    is_default: false,
    workspace: "ws-1",
    project: "prj-1",
    project_identifier: "WEB",
    created_by: null,
    runner_count: 0,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    ...overrides,
  };
}

function setup(overrides: Partial<React.ComponentProps<typeof RunnersPods>> = {}) {
  const handlers = {
    onToggle: vi.fn(),
    onCreate: vi.fn(),
    onEdit: vi.fn(),
    onDelete: vi.fn(),
  };
  const view = render(
    <RunnersPods
      pods={[pod("alpha", { is_default: true, runner_count: 2 }), pod("beta")]}
      loading={false}
      failed={false}
      selectedPodId={null}
      {...handlers}
      {...overrides}
    />
  );
  return { ...handlers, ...view };
}

describe("pod tile copy", () => {
  it("formats the runner count", () => {
    expect(podRunnerCountLabel(0)).toBe("0 runner(s)");
    expect(podRunnerCountLabel(12)).toBe("12 runner(s)");
  });

  it("names the tile the way the filter control is announced", () => {
    expect(podTileLabel("WEB_alpha")).toBe("Filter runners by pod WEB_alpha");
  });
});

describe("RunnersPods", () => {
  it("renders the section heading, hint and one tile per pod", () => {
    setup();
    expect(screen.getByRole("heading", { name: PODS_SECTION_TITLE })).toBeInTheDocument();
    expect(screen.getByText(PODS_SECTION_HINT)).toBeInTheDocument();
    const alpha = screen.getByRole("button", { name: "Filter runners by pod WEB_alpha" });
    const beta = screen.getByRole("button", { name: "Filter runners by pod WEB_beta" });
    expect(alpha).toHaveTextContent("default");
    expect(alpha).toHaveTextContent("2 runner(s)");
    expect(beta).not.toHaveTextContent("default");
    expect(beta).toHaveTextContent("0 runner(s)");
    expect(screen.queryByText(PODS_EMPTY)).not.toBeInTheDocument();
  });

  it("shows a tile's description when it has one", () => {
    setup({ pods: [pod("alpha", { description: "the big boxes" })] });
    expect(screen.getByRole("button", { name: "Filter runners by pod WEB_alpha" })).toHaveTextContent("the big boxes");
  });

  it("marks the selected tile pressed and toggles on press", () => {
    const { onToggle } = setup({ selectedPodId: "beta" });
    const alpha = screen.getByRole("button", { name: "Filter runners by pod WEB_alpha" });
    const beta = screen.getByRole("button", { name: "Filter runners by pod WEB_beta" });
    expect(alpha).toHaveAttribute("aria-pressed", "false");
    expect(beta).toHaveAttribute("aria-pressed", "true");
    fireEvent.click(alpha);
    expect(onToggle).toHaveBeenCalledWith("alpha");
  });

  it("opens edit and delete from a tile's menu", async () => {
    const { onEdit, onDelete } = setup();
    fireEvent.click(screen.getByRole("button", { name: "Pod actions for WEB_alpha" }));
    fireEvent.click(await screen.findByRole("menuitem", { name: "Edit" }));
    expect(onEdit).toHaveBeenCalledWith(expect.objectContaining({ id: "alpha" }));

    fireEvent.click(screen.getByRole("button", { name: "Pod actions for WEB_beta" }));
    fireEvent.click(await screen.findByRole("menuitem", { name: "Delete" }));
    expect(onDelete).toHaveBeenCalledWith(expect.objectContaining({ id: "beta" }));
  });

  it("offers the create entry and honors its disabled state", () => {
    const { onCreate, rerender } = setup();
    fireEvent.click(screen.getByRole("button", { name: PODS_CREATE_LABEL }));
    expect(onCreate).toHaveBeenCalledTimes(1);
    rerender(
      <RunnersPods
        pods={[]}
        loading={false}
        failed={false}
        selectedPodId={null}
        createDisabled
        onToggle={vi.fn()}
        onCreate={onCreate}
        onEdit={vi.fn()}
        onDelete={vi.fn()}
      />
    );
    expect(screen.getByRole("button", { name: PODS_CREATE_LABEL })).toBeDisabled();
  });

  it("shows the failure message instead of tiles when the read fails", () => {
    setup({ failed: true });
    expect(screen.getByRole("alert")).toHaveTextContent(PODS_LOAD_FAILED);
    expect(screen.queryByRole("button", { name: "Filter runners by pod WEB_alpha" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: PODS_CREATE_LABEL })).not.toBeInTheDocument();
  });

  it("keeps the section quiet while the first read is in flight", () => {
    setup({ pods: undefined, loading: true });
    expect(screen.getByRole("heading", { name: PODS_SECTION_TITLE })).toBeInTheDocument();
    expect(screen.queryByText(PODS_EMPTY)).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: PODS_CREATE_LABEL })).not.toBeInTheDocument();
  });

  it("explains an empty scope once loading settles", async () => {
    const { rerender } = setup({ pods: undefined, loading: true });
    rerender(
      <RunnersPods
        pods={[]}
        loading={false}
        failed={false}
        selectedPodId={null}
        onToggle={vi.fn()}
        onCreate={vi.fn()}
        onEdit={vi.fn()}
        onDelete={vi.fn()}
      />
    );
    await waitFor(() => expect(screen.getByText(PODS_EMPTY)).toBeInTheDocument());
    expect(screen.getByRole("button", { name: PODS_CREATE_LABEL })).toBeEnabled();
  });
});
