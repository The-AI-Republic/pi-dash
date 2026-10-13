// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { act, render } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

const navigate = vi.fn();

// Stand-in for the router: navigation is TanStack's own tested behavior.
// What this suite owns is the expired-redirect decision.
vi.mock("@tanstack/react-router", () => ({
  useRouter: () => ({ state: { location: { href: "/acme/" } }, navigate }),
}));

vi.mock("./toaster.js", () => ({
  notifyError: vi.fn(),
}));

import { notifyError } from "./toaster.js";
import { useSessionStore } from "../../core/session/store.js";
import { useSessionExpiredRedirect } from "./expiredRedirect.js";

function Probe(): React.ReactElement | null {
  useSessionExpiredRedirect();
  return null;
}

afterEach(() => {
  navigate.mockClear();
  vi.mocked(notifyError).mockClear();
  useSessionStore.setState({ status: "unknown", returnUrl: null });
});

describe("useSessionExpiredRedirect", () => {
  it("stays put while the session is not expired", () => {
    render(<Probe />);
    expect(navigate).not.toHaveBeenCalled();
    act(() => {
      useSessionStore.getState().markAuthenticated();
    });
    expect(navigate).not.toHaveBeenCalled();
    expect(notifyError).not.toHaveBeenCalled();
  });

  it("sends expired sessions to sign-in once with the kept return path", () => {
    render(<Probe />);
    act(() => {
      useSessionStore.setState({ status: "expired", returnUrl: "/acme/issues" });
    });
    expect(notifyError).toHaveBeenCalledWith("Session expired", "Sign in again to continue.");
    expect(navigate).toHaveBeenCalledWith({ to: "/", search: { next_path: "/acme/issues" } });
    act(() => {
      useSessionStore.setState({ status: "expired", returnUrl: "/elsewhere" });
    });
    expect(navigate).toHaveBeenCalledTimes(1);
  });

  it("falls back to the current location without a kept return path", () => {
    render(<Probe />);
    act(() => {
      useSessionStore.setState({ status: "expired", returnUrl: null });
    });
    expect(navigate).toHaveBeenCalledWith({ to: "/", search: { next_path: "/acme/" } });
  });
});
