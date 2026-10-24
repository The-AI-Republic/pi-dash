// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The detail identifier (ISS-163): one click-to-copy key with its toast.
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { IDENTIFIER_COPIED_TITLE, IDENTIFIER_COPY_FAILED_TITLE, IssueIdentifier } from "./IssueIdentifier.js";

vi.mock("../../../shared/shell/index.js", () => ({
  notifyError: vi.fn(),
  notifySuccess: vi.fn(),
}));

afterEach(() => {
  cleanup();
  vi.mocked(notifyError).mockClear();
  vi.mocked(notifySuccess).mockClear();
  delete (navigator as { clipboard?: unknown }).clipboard;
});

function stubClipboard(write: (text: string) => Promise<void>) {
  const writeText = vi.fn(write);
  Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText } });
  return writeText;
}

describe("IssueIdentifier", () => {
  it("renders the short key as one control", () => {
    render(<IssueIdentifier projectIdentifier="WEB" sequenceId={42} />);
    expect(screen.getByRole("button", { name: "WEB-42" })).toBeInTheDocument();
  });

  it("copies the key with a toast", async () => {
    const writeText = stubClipboard(async () => undefined);
    render(<IssueIdentifier projectIdentifier="WEB" sequenceId={42} />);
    fireEvent.click(screen.getByRole("button", { name: "WEB-42" }));
    await waitFor(() => expect(writeText).toHaveBeenCalledWith("WEB-42"));
    expect(vi.mocked(notifySuccess)).toHaveBeenCalledWith(IDENTIFIER_COPIED_TITLE);
  });

  it("notices a clipboard refusal", async () => {
    stubClipboard(async () => {
      throw new Error("denied");
    });
    render(<IssueIdentifier projectIdentifier="WEB" sequenceId={42} />);
    fireEvent.click(screen.getByRole("button", { name: "WEB-42" }));
    await waitFor(() => expect(vi.mocked(notifyError)).toHaveBeenCalledWith(IDENTIFIER_COPY_FAILED_TITLE));
  });
});
