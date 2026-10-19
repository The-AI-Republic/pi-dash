// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { VirtualList } from "./VirtualList";

const items = Array.from({ length: 1000 }, (_, index) => `Issue ${index + 1}`);

// jsdom reports zero layout sizes, so the virtualizer would compute an empty
// range. The stub gives the scroll region a viewport and every row its
// estimate; everything else delegates to the original getters.
const originalHeight = Object.getOwnPropertyDescriptor(HTMLElement.prototype, "offsetHeight");
const originalWidth = Object.getOwnPropertyDescriptor(HTMLElement.prototype, "offsetWidth");

function sizeOf(element: HTMLElement): number {
  if (element.hasAttribute("data-index")) return 28;
  if (element.getAttribute("role") === "region") return 224;
  return 0;
}

beforeEach(() => {
  Object.defineProperty(HTMLElement.prototype, "offsetHeight", {
    configurable: true,
    get(this: HTMLElement) {
      return sizeOf(this) || originalHeight?.get?.call(this) || 0;
    },
  });
  Object.defineProperty(HTMLElement.prototype, "offsetWidth", {
    configurable: true,
    get(this: HTMLElement) {
      return this.hasAttribute("data-index") || this.getAttribute("role") === "region"
        ? 300
        : (originalWidth?.get?.call(this) ?? 0);
    },
  });
});

afterEach(() => {
  if (originalHeight) Object.defineProperty(HTMLElement.prototype, "offsetHeight", originalHeight);
  if (originalWidth) Object.defineProperty(HTMLElement.prototype, "offsetWidth", originalWidth);
});

function setup() {
  return render(
    <VirtualList
      label="Issues"
      items={items}
      estimateSize={28}
      overscan={2}
      getKey={(item) => item}
      renderRow={(item) => <div>{item}</div>}
      className="h-56"
      footer={<div data-testid="footer">More rows</div>}
    />
  );
}

describe("VirtualList", () => {
  it("mounts only the visible window with stable keys", async () => {
    setup();
    expect(screen.getByRole("region", { name: "Issues" })).toBeInTheDocument();
    // Measurement resolves on the next animation frame, so rows appear async.
    expect(await screen.findByText("Issue 1")).toBeInTheDocument();
    expect(screen.queryByText("Issue 1000")).not.toBeInTheDocument();
    const first = screen.getByText("Issue 1").closest("[data-index]");
    expect(first).toHaveAttribute("data-index", "0");
  });

  it("renders later rows after scrolling", async () => {
    setup();
    await screen.findByText("Issue 1");
    const region = screen.getByRole("region", { name: "Issues" });
    region.scrollTop = 28 * 500;
    fireEvent.scroll(region);
    expect(await screen.findByText("Issue 501")).toBeInTheDocument();
    expect(screen.queryByText("Issue 1")).not.toBeInTheDocument();
  });

  it("keeps the footer inside the scroll region after the virtual rows", async () => {
    setup();
    const region = screen.getByRole("region", { name: "Issues" });
    const footer = screen.getByTestId("footer");
    expect(region).toContainElement(footer);
    expect(footer).toHaveTextContent("More rows");
    await screen.findByText("Issue 1");
    const lastRow = screen.getByText("Issue 1").closest("[data-index]")?.parentElement;
    expect(lastRow?.nextElementSibling).toBe(footer);
  });

  it("moves following rows down when an expanded row measures taller", async () => {
    Object.defineProperty(HTMLElement.prototype, "offsetHeight", {
      configurable: true,
      get(this: HTMLElement) {
        if (this.getAttribute("data-index") === "0") return 96;
        if (this.hasAttribute("data-index")) return 28;
        if (this.getAttribute("role") === "region") return 224;
        return originalHeight?.get?.call(this) || 0;
      },
    });
    setup();
    await screen.findByText("Issue 1");
    const second = screen.getByText("Issue 2").closest<HTMLElement>("[data-index]");
    await vi.waitFor(() => expect(second?.style.transform).toBe("translateY(96px)"));
  });
});
