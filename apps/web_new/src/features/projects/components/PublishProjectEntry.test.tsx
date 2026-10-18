// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { PublishProjectEntry } from "./PublishProjectEntry.js";

describe("PublishProjectEntry", () => {
  it("mounts nothing until the first open", () => {
    render(<PublishProjectEntry projectName="Web" open={false} onOpenChange={() => undefined} />);
    expect(screen.queryByRole("heading", { name: "Publish project" })).not.toBeInTheDocument();
  });

  it("loads and shows the dialog once opened", async () => {
    render(<PublishProjectEntry projectName="Web" open onOpenChange={() => undefined} />);
    expect(await screen.findByRole("heading", { name: "Publish project" })).toBeInTheDocument();
    expect(screen.getByTestId("publish-settings-mount")).toBeInTheDocument();
    expect(screen.getByText("Web can go public on a shared board.")).toBeInTheDocument();
  });

  it("forwards close requests from the dialog", async () => {
    const onOpenChange = vi.fn();
    render(<PublishProjectEntry projectName="Web" open onOpenChange={onOpenChange} />);
    (await screen.findByRole("button", { name: "Close dialog" })).click();
    expect(onOpenChange).toHaveBeenCalledTimes(1);
    expect(onOpenChange.mock.calls[0]?.[0]).toBe(false);
  });
});
