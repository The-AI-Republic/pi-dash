// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { act, render } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

const navigate = vi.fn();
const routerHarness = vi.hoisted(() => ({ href: "/acme/" }));

// Stand-in for the router: navigation is TanStack's own tested behavior.
// What this suite owns is the expired-redirect decision.
vi.mock("@tanstack/react-router", () => ({
  useRouter: () => ({ state: { location: { href: routerHarness.href } }, navigate }),
}));

vi.mock("./toaster.js", () => ({
  notifyError: vi.fn(),
}));

import { notifyError } from "./toaster.js";
import { useSessionStore } from "../../core/session/store.js";
import { bouncePathname, isBounceExempt, useSessionExpiredRedirect } from "./expiredRedirect.js";

function Probe(): React.ReactElement | null {
  useSessionExpiredRedirect();
  return null;
}

afterEach(() => {
  navigate.mockClear();
  vi.mocked(notifyError).mockClear();
  routerHarness.href = "/acme/";
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

  it("leaves public and auth pages put without even a toast", () => {
    for (const href of [
      "/",
      "/home",
      "/home/",
      "/docs?x=1",
      "/downloads#frag",
      "/pricing/",
      "/login",
      "/login/",
      "/sign-in?next_path=%2Fdocs",
      "/signin",
      "/sign-up",
      "/accounts/sign-up/",
      "/accounts/forgot-password",
      "/accounts/reset-password/",
    ]) {
      routerHarness.href = href;
      const { unmount } = render(<Probe />);
      act(() => {
        useSessionStore.setState({ status: "expired", returnUrl: "/acme/issues" });
      });
      expect(navigate).not.toHaveBeenCalled();
      expect(notifyError).not.toHaveBeenCalled();
      unmount();
      navigate.mockClear();
      vi.mocked(notifyError).mockClear();
      useSessionStore.setState({ status: "unknown", returnUrl: null });
    }
  });

  it("bounces again past an exempt page once the expiry re-fires", () => {
    routerHarness.href = "/home";
    render(<Probe />);
    act(() => {
      useSessionStore.setState({ status: "expired", returnUrl: "/home" });
    });
    expect(navigate).not.toHaveBeenCalled();
    routerHarness.href = "/acme/issues";
    act(() => {
      // A fetch past the exempt page rejects the session again.
      useSessionStore.setState({ status: "expired", returnUrl: "/acme/issues" });
    });
    expect(navigate).toHaveBeenCalledWith({ to: "/", search: { next_path: "/acme/issues" } });
  });
});

describe("bounce address rules", () => {
  it("strips query, fragment and trailing slashes to the root form", () => {
    expect(bouncePathname("/acme/issues?x=1#frag")).toBe("/acme/issues");
    expect(bouncePathname("/login/")).toBe("/login");
    expect(bouncePathname("/")).toBe("/");
    expect(bouncePathname("///")).toBe("/");
  });

  it("exempts only the listed public and auth addresses", () => {
    for (const href of ["/", "/home", "/docs", "/downloads", "/pricing", "/login", "/sign-in", "/signin"]) {
      expect(isBounceExempt(href)).toBe(true);
    }
    expect(isBounceExempt("/sign-up?next_path=%2Fdocs")).toBe(true);
    expect(isBounceExempt("/accounts/forgot-password")).toBe(true);
    expect(isBounceExempt("/acme/issues")).toBe(false);
    expect(isBounceExempt("/login-help")).toBe(false);
    expect(isBounceExempt("/accounts/set-password")).toBe(false);
  });
});
