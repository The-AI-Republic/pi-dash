// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Secret key reveal (parity/specs/workspace-settings.md §20, WSET-021):
// masked by default, show/hide round trip, copy with its confirmation,
// and the clipboard-failure notice.
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { SecretKey, SECRET_COPY_LABEL, SECRET_HIDE_LABEL, SECRET_MASK, SECRET_SHOW_LABEL } from "./SecretKey.js";

vi.mock("../../../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../shared/shell/index.js")>();
  return { ...original, notifySuccess: vi.fn(), notifyError: vi.fn() };
});

const { notifyError: shellNotifyError } = await import("../../../shared/shell/index.js");

afterEach(() => {
  cleanup();
  vi.mocked(shellNotifyError).mockClear();
});

describe("SecretKey", () => {
  it("renders the masked dot row, never the key, until shown", () => {
    render(<SecretKey secret="whsec_fixture_abcdefghijklmnopqrstuvwxyz" />);
    expect(screen.getByText(SECRET_MASK)).toBeInTheDocument();
    expect(screen.queryByText("whsec_fixture_abcdefghijklmnopqrstuvwxyz")).not.toBeInTheDocument();
    expect(screen.getByText("Secret key")).toBeInTheDocument();
  });

  it("round-trips show and hide", async () => {
    const user = userEvent.setup();
    render(<SecretKey secret="whsec_fixture_abcdefghijklmnopqrstuvwxyz" />);
    await user.click(screen.getByRole("button", { name: SECRET_SHOW_LABEL }));
    expect(screen.getByText("whsec_fixture_abcdefghijklmnopqrstuvwxyz")).toBeInTheDocument();
    expect(screen.queryByText(SECRET_MASK)).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: SECRET_HIDE_LABEL }));
    expect(screen.getByText(SECRET_MASK)).toBeInTheDocument();
    expect(screen.queryByText("whsec_fixture_abcdefghijklmnopqrstuvwxyz")).not.toBeInTheDocument();
  });

  it("copies the in-clear key and confirms, even while masked", async () => {
    const user = userEvent.setup();
    const writeText = vi.fn(async () => undefined);
    const notifySuccess = vi.fn();
    const notifyError = vi.fn();
    const onCopied = vi.fn();
    render(
      <SecretKey secret="whsec_fixture_key" onCopied={onCopied} copyDeps={{ writeText, notifySuccess, notifyError }} />
    );
    await user.click(screen.getByRole("button", { name: SECRET_COPY_LABEL }));
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith("Secret key copied"));
    expect(writeText).toHaveBeenCalledWith("whsec_fixture_key");
    expect(onCopied).toHaveBeenCalledTimes(1);
    expect(notifyError).not.toHaveBeenCalled();
  });

  it("reports a refused clipboard write and does not confirm", async () => {
    const user = userEvent.setup();
    const writeText = vi.fn(async () => {
      throw new Error("denied");
    });
    const notifySuccess = vi.fn();
    const notifyError = vi.fn();
    const onCopied = vi.fn();
    render(
      <SecretKey secret="whsec_fixture_key" onCopied={onCopied} copyDeps={{ writeText, notifySuccess, notifyError }} />
    );
    await user.click(screen.getByRole("button", { name: SECRET_COPY_LABEL }));
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith("Couldn't copy the secret key", expect.any(String)));
    expect(notifySuccess).not.toHaveBeenCalled();
    expect(onCopied).not.toHaveBeenCalled();
  });

  it("uses the async clipboard when no deps are injected", async () => {
    const user = userEvent.setup();
    const writeText = vi.fn(async () => undefined);
    Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText } });
    render(<SecretKey secret="whsec_fixture_native" />);
    await user.click(screen.getByRole("button", { name: SECRET_COPY_LABEL }));
    await waitFor(() => expect(writeText).toHaveBeenCalledWith("whsec_fixture_native"));
  });

  it("announces a native clipboard refusal through the shell notice", async () => {
    const user = userEvent.setup();
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: {
        writeText: vi.fn(async () => {
          throw new Error("not allowed");
        }),
      },
    });
    render(<SecretKey secret="whsec_fixture_native" />);
    await user.click(screen.getByRole("button", { name: SECRET_COPY_LABEL }));
    await waitFor(() =>
      expect(shellNotifyError).toHaveBeenCalledWith("Couldn't copy the secret key", expect.any(String))
    );
  });
});
