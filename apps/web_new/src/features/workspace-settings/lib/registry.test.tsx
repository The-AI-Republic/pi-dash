// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Page registry seam (WSET-001/036): a slice registers its body under a
// section key; an unmount removes only its own registration.
import { act, renderHook } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it } from "vitest";

import {
  getWorkspaceSettingsPage,
  getWorkspaceWebhookDetailPage,
  registerWorkspaceSettingsPage,
  registerWorkspaceWebhookDetailPage,
  resetWorkspaceSettingsPages,
  useWorkspaceSettingsPage,
  useWorkspaceWebhookDetailPage,
} from "./registry.js";

function Page(): React.ReactElement {
  return <p>page</p>;
}
function Detail(): React.ReactElement {
  return <p>detail</p>;
}

afterEach(() => {
  resetWorkspaceSettingsPages();
});

describe("workspace settings page registry", () => {
  it("starts empty", () => {
    expect(getWorkspaceSettingsPage("members")).toBeNull();
    expect(getWorkspaceWebhookDetailPage()).toBeNull();
  });

  it("registers per section and unregisters only its own page", () => {
    const unregister = registerWorkspaceSettingsPage("members", Page);
    expect(getWorkspaceSettingsPage("members")).toBe(Page);
    expect(getWorkspaceSettingsPage("billing")).toBeNull();
    unregister();
    expect(getWorkspaceSettingsPage("members")).toBeNull();
  });

  it("keeps a later registration when an earlier owner unregisters", () => {
    const first = registerWorkspaceSettingsPage("members", Page);
    const second = registerWorkspaceSettingsPage("members", Detail);
    first();
    expect(getWorkspaceSettingsPage("members")).toBe(Detail);
    second();
    expect(getWorkspaceSettingsPage("members")).toBeNull();
  });

  it("registers the webhook detail body and ignores stale unmounts", () => {
    const first = registerWorkspaceWebhookDetailPage(Detail);
    const second = registerWorkspaceWebhookDetailPage(Page);
    first();
    expect(getWorkspaceWebhookDetailPage()).toBe(Page);
    second();
    expect(getWorkspaceWebhookDetailPage()).toBeNull();
  });

  it("re-renders a consumer when its page registers and unregisters", () => {
    const { result } = renderHook(() => useWorkspaceSettingsPage("webhooks"));
    expect(result.current).toBeNull();
    let unregister = (): void => undefined;
    act(() => {
      unregister = registerWorkspaceSettingsPage("webhooks", Page);
    });
    expect(result.current).toBe(Page);
    act(() => {
      unregister();
    });
    expect(result.current).toBeNull();
  });

  it("re-renders a detail consumer when its body registers", () => {
    const { result } = renderHook(() => useWorkspaceWebhookDetailPage());
    expect(result.current).toBeNull();
    act(() => {
      registerWorkspaceWebhookDetailPage(Detail);
    });
    expect(result.current).toBe(Detail);
  });

  it("clears every registration for a fresh test", () => {
    registerWorkspaceSettingsPage("members", Page);
    registerWorkspaceWebhookDetailPage(Detail);
    resetWorkspaceSettingsPages();
    expect(getWorkspaceSettingsPage("members")).toBeNull();
    expect(getWorkspaceWebhookDetailPage()).toBeNull();
  });
});
