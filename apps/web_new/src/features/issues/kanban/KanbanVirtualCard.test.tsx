// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Virtualized board card shell (parity/specs/issues.md §5, ISS-043):
// content near the viewport, a marked placeholder past the overscan,
// and content while forced or without an observer.
import { act, render, renderHook, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import {
  KanbanVirtualCard,
  useNearViewport,
  VIRTUAL_CARD_HEIGHT_PX,
  VIRTUAL_OVERSCAN_PX,
} from "./KanbanVirtualCard.js";

const ISSUE = "123e4567-e89b-12d3-a456-426614174000";
const COLUMN = "323e4567-e89b-12d3-a456-426614174002";

type ObserverCallback = (entries: Array<{ isIntersecting: boolean }>) => void;

const seen = vi.hoisted(() => ({ callbacks: [] as ObserverCallback[], options: [] as object[] }));

function installObserver(): void {
  seen.callbacks = [];
  seen.options = [];
  vi.stubGlobal(
    "IntersectionObserver",
    class {
      callback: ObserverCallback;
      constructor(callback: ObserverCallback, options?: object) {
        this.callback = callback;
        seen.callbacks.push(callback);
        seen.options.push(options ?? {});
      }
      observe(): void {}
      disconnect(): void {}
    }
  );
}

afterEach(() => {
  vi.unstubAllGlobals();
});

function shell(forceContent = false): React.ReactElement {
  return (
    <KanbanVirtualCard issueId={ISSUE} groupKey={COLUMN} laneKey={null} forceContent={forceContent}>
      <article data-testid="kanban-card">Fix login</article>
    </KanbanVirtualCard>
  );
}

describe("KanbanVirtualCard", () => {
  it("renders content without an observer", () => {
    render(shell());
    expect(screen.getByText("Fix login")).toBeVisible();
    expect(VIRTUAL_OVERSCAN_PX).toBe(800);
  });

  it("mounts content once the shell nears the viewport", () => {
    installObserver();
    render(shell());
    expect(screen.queryByText("Fix login")).toBeNull();
    const placeholder = screen.getByTestId("kanban-card");
    expect(placeholder).toHaveAttribute("data-issue-id", ISSUE);
    expect(placeholder).toHaveAttribute("data-group-key", COLUMN);
    expect(placeholder).toHaveAttribute("data-virtualized", "true");
    expect(placeholder).toHaveStyle({ height: `${VIRTUAL_CARD_HEIGHT_PX}px` });
    act(() => seen.callbacks[0]?.([{ isIntersecting: true }]));
    expect(screen.getByText("Fix login")).toBeVisible();
  });

  it("unmounts content past the overscan", () => {
    installObserver();
    render(shell());
    act(() => seen.callbacks[0]?.([{ isIntersecting: true }]));
    expect(screen.getByText("Fix login")).toBeVisible();
    act(() => seen.callbacks[0]?.([{ isIntersecting: false }]));
    expect(screen.queryByText("Fix login")).toBeNull();
    expect(screen.getByTestId("kanban-card")).toHaveAttribute("data-virtualized", "true");
  });

  it("keeps content while forced even past the overscan", () => {
    installObserver();
    render(shell(true));
    act(() => seen.callbacks[0]?.([{ isIntersecting: false }]));
    expect(screen.getByText("Fix login")).toBeVisible();
  });

  it("reads near when the ref never attaches", () => {
    installObserver();
    const { result } = renderHook(() => useNearViewport<HTMLDivElement>());
    expect(result.current.near).toBe(true);
  });
});
