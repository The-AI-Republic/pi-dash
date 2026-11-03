// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The detail identifier (ISS-163): one click-to-copy key with its toast.
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { edition } from "../../../core/edition/index.js";
import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { ISSUE_TYPES_FLAG } from "../edition/flags.js";
import { registerEditionIssuesProviders, resetEditionIssuesProviders } from "../edition/providers.js";
import { IDENTIFIER_COPIED_TITLE, IDENTIFIER_COPY_FAILED_TITLE, IssueIdentifier } from "./IssueIdentifier.js";

vi.mock("../../../shared/shell/index.js", () => ({
  notifyError: vi.fn(),
  notifySuccess: vi.fn(),
}));

afterEach(() => {
  cleanup();
  resetEditionIssuesProviders();
  delete edition.flags[ISSUE_TYPES_FLAG];
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

  it("mounts the cloud type badge beside the key only when the edition supplies one (ISS-235)", () => {
    registerEditionIssuesProviders({
      issueTypeDetail: { badge: ({ editable }) => <span data-testid="type-probe">{editable ? "edit" : "read"}</span> },
    });
    render(
      <IssueIdentifier
        projectIdentifier="WEB"
        sequenceId={42}
        workspaceSlug="acme"
        projectId="p"
        issueId="i"
        editable
      />
    );
    expect(screen.queryByTestId("detail-type-badge")).toBeNull();
    cleanup();
    edition.flags[ISSUE_TYPES_FLAG] = true;
    render(
      <IssueIdentifier
        projectIdentifier="WEB"
        sequenceId={42}
        workspaceSlug="acme"
        projectId="p"
        issueId="i"
        editable
      />
    );
    expect(screen.getByTestId("detail-type-badge")).toContainElement(screen.getByTestId("type-probe"));
    expect(screen.getByTestId("type-probe")).toHaveTextContent("edit");
    expect(screen.getByRole("button", { name: "WEB-42" })).toBeInTheDocument();
  });

  it("keeps the key alone when the caller passes no item to read", () => {
    edition.flags[ISSUE_TYPES_FLAG] = true;
    registerEditionIssuesProviders({ issueTypeDetail: { badge: () => <span data-testid="type-probe">Bug</span> } });
    render(<IssueIdentifier projectIdentifier="WEB" sequenceId={42} />);
    expect(screen.queryByTestId("detail-type-badge")).toBeNull();
    expect(screen.getByRole("button", { name: "WEB-42" })).toBeInTheDocument();
  });
});
