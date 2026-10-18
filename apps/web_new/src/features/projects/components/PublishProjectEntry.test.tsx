// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { PublishProjectEntry } from "./PublishProjectEntry.js";

// The lazy chunk pulls the dialog and its primitives on first open; the
// first import can outlast the default 1 s find timeout when the suite
// runs the whole tree.
const OPEN_TIMEOUT = { timeout: 10_000 };

describe("PublishProjectEntry", () => {
  it("mounts nothing until the first open", () => {
    render(<PublishProjectEntry projectName="Web" open={false} onOpenChange={() => undefined} />);
    expect(screen.queryByRole("heading", { name: "Publish project" })).not.toBeInTheDocument();
  });

  it("loads and shows the dialog once opened", async () => {
    render(<PublishProjectEntry projectName="Web" open onOpenChange={() => undefined} />);
    expect(await screen.findByRole("heading", { name: "Publish project" }, OPEN_TIMEOUT)).toBeInTheDocument();
    expect(screen.getByTestId("publish-settings-mount")).toBeInTheDocument();
    expect(screen.getByText("Web can go public on a shared board.")).toBeInTheDocument();
  });

  it("forwards close requests from the dialog", async () => {
    const onOpenChange = vi.fn();
    render(<PublishProjectEntry projectName="Web" open onOpenChange={onOpenChange} />);
    (await screen.findByRole("button", { name: "Close dialog" }, OPEN_TIMEOUT)).click();
    expect(onOpenChange).toHaveBeenCalledTimes(1);
    expect(onOpenChange.mock.calls[0]?.[0]).toBe(false);
  });
});
