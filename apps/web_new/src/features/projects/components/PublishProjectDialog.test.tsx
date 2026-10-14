// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { PublishProjectDialog } from "./PublishProjectDialog.js";

describe("PublishProjectDialog", () => {
  it("opens on the publish heading with the settings mount", () => {
    render(<PublishProjectDialog projectName="Web" open onOpenChange={() => undefined} />);
    expect(screen.getByRole("heading", { name: "Publish project" })).toBeInTheDocument();
    expect(screen.getByTestId("publish-settings-mount")).toBeInTheDocument();
  });

  it("renders nothing while closed", () => {
    render(<PublishProjectDialog projectName="Web" open={false} onOpenChange={() => undefined} />);
    expect(screen.queryByRole("heading", { name: "Publish project" })).not.toBeInTheDocument();
  });

  it("closes through the dialog", async () => {
    const onOpenChange = vi.fn();
    render(<PublishProjectDialog projectName="Web" open onOpenChange={onOpenChange} />);
    screen.getByRole("button", { name: "Close dialog" }).click();
    expect(onOpenChange).toHaveBeenCalledTimes(1);
    expect(onOpenChange.mock.calls[0]?.[0]).toBe(false);
  });
});
