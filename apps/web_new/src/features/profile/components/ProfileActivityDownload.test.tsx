// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Today's-activity download (parity/specs/profile.md §3, PROF-007): the
// button shows a working label until the export settles, then returns to
// idle; failures toast and also return to idle.
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

vi.mock("../../../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../shared/shell/index.js")>();
  return { ...original, notifyError: vi.fn() };
});

import { notifyError } from "../../../shared/shell/index.js";
import {
  PROFILE_DOWNLOAD_IDLE_LABEL,
  PROFILE_DOWNLOAD_WORKING_LABEL,
  ProfileActivityDownload,
} from "./ProfileActivityDownload.js";

describe("ProfileActivityDownload", () => {
  it("shows the working label while the export runs, then returns to idle", async () => {
    let release!: () => void;
    const run = vi.fn(
      () =>
        new Promise<void>((resolve) => {
          release = resolve;
        })
    );
    render(<ProfileActivityDownload workspaceSlug="acme" userId="u-1" run={run} />);
    const button = screen.getByTestId("profile-activity-download");
    expect(button).toHaveTextContent(PROFILE_DOWNLOAD_IDLE_LABEL);
    expect(PROFILE_DOWNLOAD_IDLE_LABEL).toBe("Download today's activity");
    await userEvent.click(button);
    await waitFor(() => expect(button).toHaveTextContent(PROFILE_DOWNLOAD_WORKING_LABEL));
    expect(button).toBeDisabled();
    expect(PROFILE_DOWNLOAD_WORKING_LABEL).toBe("Downloading");
    await userEvent.click(button);
    expect(run).toHaveBeenCalledTimes(1);
    release();
    await waitFor(() => expect(button).toHaveTextContent(PROFILE_DOWNLOAD_IDLE_LABEL));
    expect(button).toBeEnabled();
  });

  it("toasts a failure and returns to idle", async () => {
    const run = vi.fn(async () => {
      throw new Error("export broke");
    });
    render(<ProfileActivityDownload workspaceSlug="acme" userId="u-1" run={run} />);
    await userEvent.click(screen.getByTestId("profile-activity-download"));
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith("Could not download activity", "export broke"));
    expect(screen.getByTestId("profile-activity-download")).toHaveTextContent(PROFILE_DOWNLOAD_IDLE_LABEL);
  });

  it("falls back to a generic message for a non-error rejection", async () => {
    const run = vi.fn(async () => {
      throw "nope";
    });
    render(<ProfileActivityDownload workspaceSlug="acme" userId="u-1" run={run} />);
    await userEvent.click(screen.getByTestId("profile-activity-download"));
    await waitFor(() =>
      expect(notifyError).toHaveBeenCalledWith("Could not download activity", "Try again in a moment.")
    );
  });
});
