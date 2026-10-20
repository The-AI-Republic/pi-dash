// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
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
    await waitFor(() => expect(screen.queryByTestId("preview-card")).not.toBeInTheDocument());
  });

  it("uses a short default delay so pass-through hovers do not flash a card", () => {
    expect(PREVIEW_CARD_DELAY_MS).toBeGreaterThan(0);
    expect(PREVIEW_CARD_DELAY_MS).toBeLessThanOrEqual(500);
  });
});
