// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { PublishViewDialog } from "./PublishViewDialog.js";

function handlers(): {
  onPublish: ReturnType<typeof vi.fn>;
  onUnpublish: ReturnType<typeof vi.fn>;
  onCopyLink: ReturnType<typeof vi.fn>;
  onClose: ReturnType<typeof vi.fn>;
} {
  return { onPublish: vi.fn(), onUnpublish: vi.fn(), onCopyLink: vi.fn(), onClose: vi.fn() };
}

describe("PublishViewDialog", () => {
  it("offers publishing while the view has no link", async () => {
    const user = userEvent.setup();
    const calls = handlers();
    render(<PublishViewDialog open={true} viewName="Triage" link={null} {...calls} />);

    expect(screen.getByRole("heading", { name: "Publish view" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Publish" }));
    expect(calls.onPublish).toHaveBeenCalledTimes(1);
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(calls.onClose).toHaveBeenCalledTimes(1);
    expect(calls.onUnpublish).not.toHaveBeenCalled();
  });

  it("shows the link with copy and unpublish while published", async () => {
    const user = userEvent.setup();
    const calls = handlers();
    render(<PublishViewDialog open={true} viewName="Triage" link="https://public.test/v/abc" {...calls} />);

    expect(screen.getByRole("heading", { name: "Published view" })).toBeInTheDocument();
    expect(screen.getByTestId("publish-view-link")).toHaveTextContent("https://public.test/v/abc");
    await user.click(screen.getByRole("button", { name: "Copy link" }));
    expect(calls.onCopyLink).toHaveBeenCalledWith("https://public.test/v/abc");
    await user.click(screen.getByRole("button", { name: "Unpublish" }));
    expect(calls.onUnpublish).toHaveBeenCalledTimes(1);
    expect(calls.onPublish).not.toHaveBeenCalled();
  });

  it("pins the actions while a write flies", async () => {
    const user = userEvent.setup();
    const calls = handlers();
    render(
      <PublishViewDialog open={true} viewName="Triage" link="https://public.test/v/abc" pending={true} {...calls} />
    );

    expect(screen.getByRole("button", { name: "Unpublishing…" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Copy link" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Cancel" })).toBeDisabled();
    await user.keyboard("{Escape}");
    expect(calls.onClose).toHaveBeenCalledTimes(1);
    expect(calls.onUnpublish).not.toHaveBeenCalled();
  });

  it("pins the publish action while publishing", () => {
    const calls = handlers();
    render(<PublishViewDialog open={true} viewName="Triage" link={null} pending={true} {...calls} />);

    expect(screen.getByRole("button", { name: "Publishing…" })).toBeDisabled();
  });

  it("renders nothing while closed", () => {
    const calls = handlers();
    render(<PublishViewDialog open={false} viewName="Triage" link={null} {...calls} />);
    expect(screen.queryByRole("heading", { name: "Publish view" })).not.toBeInTheDocument();
  });
});
