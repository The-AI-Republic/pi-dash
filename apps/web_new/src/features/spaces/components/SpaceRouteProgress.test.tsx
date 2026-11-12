// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { act, render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import {
  SPACE_PROGRESS_DELAY_MS,
  SpaceProgressProvider,
  SpaceRouteProgress,
  useSpaceProgress,
} from "./SpaceRouteProgress.js";

function Lease({ on }: { on: boolean }): React.ReactElement | null {
  useSpaceProgress(on);
  return null;
}

function renderProgress(ui: React.ReactElement): void {
  render(
    <SpaceProgressProvider>
      <SpaceRouteProgress delayMs={100} />
      {ui}
    </SpaceProgressProvider>
  );
}

afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
});

describe("SpaceRouteProgress", () => {
  it("renders nothing outside the provider", () => {
    render(<SpaceRouteProgress />);
    expect(screen.queryByTestId("space-route-progress")).not.toBeInTheDocument();
  });

  it("renders nothing while idle", () => {
    vi.useFakeTimers();
    renderProgress(<Lease on={false} />);
    act(() => {
      vi.advanceTimersByTime(1000);
    });
    expect(screen.queryByTestId("space-route-progress")).not.toBeInTheDocument();
  });

  it("shows the bar once a slow transition passes the delay", () => {
    vi.useFakeTimers();
    renderProgress(<Lease on />);
    expect(screen.queryByTestId("space-route-progress")).not.toBeInTheDocument();
    act(() => {
      vi.advanceTimersByTime(100);
    });
    expect(screen.getByRole("progressbar", { name: "Loading" })).toBeInTheDocument();
  });

  it("uses the default delay without an explicit one", () => {
    vi.useFakeTimers();
    render(
      <SpaceProgressProvider>
        <SpaceRouteProgress />
        <Lease on />
      </SpaceProgressProvider>
    );
    act(() => {
      vi.advanceTimersByTime(SPACE_PROGRESS_DELAY_MS - 1);
    });
    expect(screen.queryByTestId("space-route-progress")).not.toBeInTheDocument();
    act(() => {
      vi.advanceTimersByTime(1);
    });
    expect(screen.getByTestId("space-route-progress")).toBeInTheDocument();
  });

  it("never shows an instant transition", () => {
    vi.useFakeTimers();
    const { rerender } = render(
      <SpaceProgressProvider>
        <SpaceRouteProgress delayMs={100} />
        <Lease on />
      </SpaceProgressProvider>
    );
    act(() => {
      vi.advanceTimersByTime(50);
    });
    rerender(
      <SpaceProgressProvider>
        <SpaceRouteProgress delayMs={100} />
        <Lease on={false} />
      </SpaceProgressProvider>
    );
    act(() => {
      vi.advanceTimersByTime(1000);
    });
    expect(screen.queryByTestId("space-route-progress")).not.toBeInTheDocument();
  });

  it("removes the bar when the last lease settles", () => {
    vi.useFakeTimers();
    const { rerender } = render(
      <SpaceProgressProvider>
        <SpaceRouteProgress delayMs={100} />
        <Lease on />
      </SpaceProgressProvider>
    );
    act(() => {
      vi.advanceTimersByTime(100);
    });
    expect(screen.getByTestId("space-route-progress")).toBeInTheDocument();
    rerender(
      <SpaceProgressProvider>
        <SpaceRouteProgress delayMs={100} />
        <Lease on={false} />
      </SpaceProgressProvider>
    );
    expect(screen.queryByTestId("space-route-progress")).not.toBeInTheDocument();
  });

  it("stays up until every lease settles", () => {
    vi.useFakeTimers();
    const both = (
      <SpaceProgressProvider>
        <SpaceRouteProgress delayMs={100} />
        <Lease on />
        <Lease on />
      </SpaceProgressProvider>
    );
    const { rerender } = render(both);
    act(() => {
      vi.advanceTimersByTime(100);
    });
    expect(screen.getByTestId("space-route-progress")).toBeInTheDocument();
    rerender(
      <SpaceProgressProvider>
        <SpaceRouteProgress delayMs={100} />
        <Lease on />
        <Lease on={false} />
      </SpaceProgressProvider>
    );
    // One lease released: the bar stays, and its timer restarts without a flash.
    act(() => {
      vi.advanceTimersByTime(1000);
    });
    expect(screen.getByTestId("space-route-progress")).toBeInTheDocument();
    rerender(
      <SpaceProgressProvider>
        <SpaceRouteProgress delayMs={100} />
        <Lease on={false} />
        <Lease on={false} />
      </SpaceProgressProvider>
    );
    expect(screen.queryByTestId("space-route-progress")).not.toBeInTheDocument();
  });

  it("is a no-op outside the provider", () => {
    render(<Lease on />);
    expect(screen.queryByTestId("space-route-progress")).not.toBeInTheDocument();
  });
});
