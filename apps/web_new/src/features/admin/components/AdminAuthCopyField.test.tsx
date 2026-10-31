// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Copy field: the labeled URL with its stable id, clipboard write plus
// the informational notice on click, and the optional description and
// help link (static or derived from the live draft).
import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { notifySuccess } from "../../../shared/shell/index.js";
import { AdminAuthCopyField } from "./AdminAuthCopyField.js";

vi.mock("../../../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<Record<string, unknown>>();
  return { ...original, notifySuccess: vi.fn() };
});

const mockedNotifySuccess = vi.mocked(notifySuccess);
const writeText = vi.fn().mockResolvedValue(undefined);

beforeEach(() => {
  vi.clearAllMocks();
  Object.defineProperty(window.navigator, "clipboard", { value: { writeText }, configurable: true });
});

describe("copy field", () => {
  it("renders the label and the shown URL under a stable id", () => {
    render(<AdminAuthCopyField label="Callback URI" url="https://app.example.test/auth/github/callback/" />);
    expect(screen.getByText("Callback URI")).toBeInTheDocument();
    expect(screen.getByText("https://app.example.test/auth/github/callback/")).toBeInTheDocument();
    expect(screen.getByTestId("admin-auth-copy-callback-uri")).toBeInTheDocument();
    expect(screen.queryByRole("link")).not.toBeInTheDocument();
  });

  it("writes the URL to the clipboard with the informational notice on click", () => {
    render(<AdminAuthCopyField label="Origin URL" url="https://app.example.test" />);
    fireEvent.click(screen.getByTestId("admin-auth-copy-origin-url"));
    expect(writeText).toHaveBeenCalledWith("https://app.example.test");
    expect(mockedNotifySuccess).toHaveBeenCalledWith(
      "Copied to clipboard",
      "The Origin URL has been successfully copied to your clipboard"
    );
  });

  it("renders the description when provided", () => {
    render(
      <AdminAuthCopyField
        label="Callback URI"
        url="https://app.example.test/auth/github/callback/"
        description="Paste this into the provider console."
      />
    );
    expect(screen.getByText("Paste this into the provider console.")).toBeInTheDocument();
  });

  it("renders a static help link with text and target", () => {
    render(
      <AdminAuthCopyField
        label="Callback URI"
        url="https://app.example.test/auth/github/callback/"
        helpHref="https://console.example.test/new"
        helpLabel="Open the console"
      />
    );
    const link = screen.getByRole("link", { name: "Open the console" });
    expect(link).toHaveAttribute("href", "https://console.example.test/new");
  });

  it("derives the help link from the live draft", () => {
    render(
      <AdminAuthCopyField
        label="Callback URI"
        url="https://app.example.test/auth/gitea/callback/"
        helpHref={(values) => `${values["GITEA_HOST"]}/user/settings/applications`}
        helpLabel="Open the console"
        fieldValues={{ GITEA_HOST: "https://gitea.example.test" }}
      />
    );
    expect(screen.getByRole("link", { name: "Open the console" })).toHaveAttribute(
      "href",
      "https://gitea.example.test/user/settings/applications"
    );
  });

  it("renders no link without both help text and target", () => {
    const { rerender } = render(
      <AdminAuthCopyField
        label="Callback URI"
        url="https://app.example.test/auth/github/callback/"
        helpHref="https://console.example.test/new"
      />
    );
    expect(screen.queryByRole("link")).not.toBeInTheDocument();
    rerender(
      <AdminAuthCopyField
        label="Callback URI"
        url="https://app.example.test/auth/github/callback/"
        helpLabel="Open the console"
      />
    );
    expect(screen.queryByRole("link")).not.toBeInTheDocument();
  });
});
