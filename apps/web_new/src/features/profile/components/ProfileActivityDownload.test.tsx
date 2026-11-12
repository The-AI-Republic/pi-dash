// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Today's-activity download (parity/specs/profile.md §3, PROF-007): the
// button shows a working label until the export settles, then returns to
// idle; failures toast and also return to idle.
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { expectContentKeysInert, expectKeyboardReachable, pressEnter, tabTo } from "../../../testSupport/keyboard.js";

vi.mock("../../../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../shared/shell/index.js")>();
  return { ...original, notifyError: vi.fn() };
});

vi.mock("../api/export.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../api/export.js")>();
  return { ...original, downloadProfileActivity: vi.fn(async () => undefined) };
});

import { notifyError } from "../../../shared/shell/index.js";
import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { downloadProfileActivity } from "../api/export.js";
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

  it("ignores a second click dispatched while the export runs", async () => {
    // Both clicks land inside one act scope, so no re-render (and no
    // disabled attribute) sits between them — the pre-render window a
    // rapid double-click hits before the working state lands.
    let release!: () => void;
    const run = vi.fn(
      () =>
        new Promise<void>((resolve) => {
          release = resolve;
        })
    );
    render(<ProfileActivityDownload workspaceSlug="acme" userId="u-1" run={run} />);
    const button = screen.getByTestId("profile-activity-download");
    act(() => {
      fireEvent.click(button);
      fireEvent.click(button);
    });
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

  it("runs the live export when the caller injects no runner", async () => {
    const client = createAppClient("https://api.test", () => new Promise<never>(() => {}));
    render(<ProfileActivityDownload workspaceSlug="acme" userId="u-1" />);
    await userEvent.click(screen.getByTestId("profile-activity-download"));
    await waitFor(() => expect(downloadProfileActivity).toHaveBeenCalledWith(client, "acme", "u-1"));
    await waitFor(() =>
      expect(screen.getByTestId("profile-activity-download")).toHaveTextContent(PROFILE_DOWNLOAD_IDLE_LABEL)
    );
    resetAppClient();
  });

  it("reaches the download by Tab and saves on Enter with no custom bindings", async () => {
    const user = userEvent.setup();
    const run = vi.fn(async () => undefined);
    render(<ProfileActivityDownload workspaceSlug="acme" userId="u-1" run={run} />);
    const button = screen.getByTestId("profile-activity-download");
    expect(expectKeyboardReachable(button.parentElement as HTMLElement)).toContain(button);

    await tabTo(user, button);
    expect(button).toHaveFocus();
    await pressEnter(user);
    await waitFor(() => expect(run).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(button).toBeEnabled());

    await expectContentKeysInert(user, () => button.textContent ?? "");
    expect(run).toHaveBeenCalledTimes(1);
  });
});
