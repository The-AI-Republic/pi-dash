// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { PREVIEW_CARD_DELAY_MS, PreviewCard } from "./PreviewCard";

describe("PreviewCard", () => {
  it("opens on hover after the configured delay", async () => {
    const user = userEvent.setup();
    render(
      <PreviewCard delay={0} trigger={<a href="/acme/browse/WEB-1/">WEB-1</a>}>
        <p>Card body</p>
      </PreviewCard>
    );
    expect(screen.queryByTestId("preview-card")).not.toBeInTheDocument();
    await user.hover(screen.getByRole("link", { name: "WEB-1" }));
    expect(await screen.findByTestId("preview-card")).toHaveTextContent("Card body");
  });

  it("closes when the pointer leaves the reference and the card", async () => {
    const user = userEvent.setup();
    render(
      <div>
        <PreviewCard delay={0} trigger={<button type="button">Hover me</button>}>
          <p>Card body</p>
        </PreviewCard>
        <button type="button">Elsewhere</button>
      </div>
    );
    await user.hover(screen.getByRole("button", { name: "Hover me" }));
    await screen.findByTestId("preview-card");
    await user.hover(screen.getByRole("button", { name: "Elsewhere" }));
    await waitFor(() => expect(screen.queryByTestId("preview-card")).not.toBeInTheDocument(), { timeout: 1000 });
  });

  it("uses a short default delay so pass-through hovers do not flash a card", () => {
    expect(PREVIEW_CARD_DELAY_MS).toBeGreaterThan(0);
    expect(PREVIEW_CARD_DELAY_MS).toBeLessThanOrEqual(500);
  });

  it("waits out a configured delay before showing and forwards the trigger's own handlers", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });
      const onPointerEnter = vi.fn();
      const onPointerLeave = vi.fn();
      render(
        <PreviewCard
          delay={200}
          trigger={
            <button type="button" onPointerEnter={onPointerEnter} onPointerLeave={onPointerLeave}>
              Hover me
            </button>
          }
        >
          <p>Card body</p>
        </PreviewCard>
      );
      await user.hover(screen.getByRole("button", { name: "Hover me" }));
      expect(onPointerEnter).toHaveBeenCalledTimes(1);
      expect(screen.queryByTestId("preview-card")).not.toBeInTheDocument();
      vi.advanceTimersByTime(200);
      expect(await screen.findByTestId("preview-card")).toHaveTextContent("Card body");
      await user.unhover(screen.getByRole("button", { name: "Hover me" }));
      expect(onPointerLeave).toHaveBeenCalledTimes(1);
      expect(await screen.findByTestId("preview-card")).toBeInTheDocument();
      await waitFor(() => expect(screen.queryByTestId("preview-card")).not.toBeInTheDocument(), { timeout: 1000 });
    } finally {
      vi.useRealTimers();
    }
  });

  it("stays open while the pointer is on the card and closes when it leaves", async () => {
    const user = userEvent.setup();
    render(
      <PreviewCard delay={0} trigger={<a href="/acme/browse/WEB-1/">WEB-1</a>}>
        <p>Card body</p>
      </PreviewCard>
    );
    await user.hover(screen.getByRole("link", { name: "WEB-1" }));
    const card = await screen.findByTestId("preview-card");
    await user.hover(card);
    await new Promise((resolve) => setTimeout(resolve, 150));
    expect(screen.getByTestId("preview-card")).toBeInTheDocument();
    await user.unhover(card);
    await waitFor(() => expect(screen.queryByTestId("preview-card")).not.toBeInTheDocument(), { timeout: 1000 });
  });

  it("opens on keyboard focus, closes on blur, and places the card on the requested side", async () => {
    render(
      <PreviewCard delay={0} side="right" align="end" trigger={<button type="button">Focus me</button>}>
        <p>Card body</p>
      </PreviewCard>
    );
    const trigger = screen.getByRole("button", { name: "Focus me" });
    trigger.focus();
    const card = await screen.findByTestId("preview-card");
    expect(card.style.top).toBe("0px");
    expect(card.style.left).toBe("6px");
    expect(card.style.transform).toBe("translate(0, -100%)");
    trigger.blur();
    await waitFor(() => expect(screen.queryByTestId("preview-card")).not.toBeInTheDocument());
  });

  it("places the card for every side and alignment", async () => {
    const rect = { top: 10, right: 110, bottom: 30, left: 20, width: 90, height: 20, x: 20, y: 10, toJSON: () => ({}) };
    for (const [side, align, top, left, transform] of [
      ["bottom", "center", 36, 65, "translate(-50%, 0)"],
      ["top", "end", 4, 110, "translate(-100%, -100%)"],
      ["left", "start", 10, 14, "translate(-100%, 0)"],
      ["left", "center", 20, 14, "translate(-100%, -50%)"],
      ["right", "start", 10, 116, "translate(0, 0)"],
      ["right", "center", 20, 116, "translate(0, -50%)"],
      ["right", "end", 30, 116, "translate(0, -100%)"],
    ] as const) {
      const { unmount } = render(
        <PreviewCard
          delay={0}
          side={side}
          align={align}
          trigger={
            <button type="button" style={{ position: "fixed", top: 10, left: 20 }}>
              Ref
            </button>
          }
        >
          <p>Card body</p>
        </PreviewCard>
      );
      const trigger = screen.getByRole("button", { name: "Ref" });
      trigger.getBoundingClientRect = () => rect as DOMRect;
      trigger.focus();
      const card = await screen.findByTestId("preview-card");
      expect(card.style.top).toBe(`${top}px`);
      expect(card.style.left).toBe(`${left}px`);
      expect(card.style.transform).toBe(transform);
      unmount();
    }
  });

  it("forwards every trigger handler the caller set, including focus handlers", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      const onFocus = vi.fn();
      const onBlur = vi.fn();
      render(
        <PreviewCard
          delay={0}
          trigger={
            <button type="button" onPointerEnter={vi.fn()} onPointerLeave={vi.fn()} onFocus={onFocus} onBlur={onBlur}>
              Ref
            </button>
          }
        >
          <p>Card body</p>
        </PreviewCard>
      );
      const trigger = screen.getByRole("button", { name: "Ref" });
      trigger.focus();
      expect(onFocus).toHaveBeenCalledTimes(1);
      expect(await screen.findByTestId("preview-card")).toBeInTheDocument();
      trigger.blur();
      expect(onBlur).toHaveBeenCalledTimes(1);
      await waitFor(() => expect(screen.queryByTestId("preview-card")).not.toBeInTheDocument());
    } finally {
      vi.useRealTimers();
    }
  });

  it("keeps the card while the reference blurs into it", async () => {
    const user = userEvent.setup();
    render(
      <PreviewCard delay={0} trigger={<button type="button">Ref</button>}>
        <button type="button">Inside</button>
      </PreviewCard>
    );
    const trigger = screen.getByRole("button", { name: "Ref" });
    await user.hover(trigger);
    trigger.focus();
    const inside = await screen.findByRole("button", { name: "Inside" });
    inside.focus();
    expect(screen.getByTestId("preview-card")).toBeInTheDocument();
  });
});
