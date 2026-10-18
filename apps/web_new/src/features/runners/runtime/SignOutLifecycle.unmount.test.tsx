// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import { expect, it, vi } from "vitest";

const gate = vi.hoisted(() => {
  let release: () => void = () => undefined;
  const promise = new Promise<void>((resolve) => {
    release = resolve;
  });
  return { promise, release: () => release() };
});

vi.mock("../../../core/platform/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../core/platform/index.js")>();
  return { ...original, platform: { ...original.platform, kind: "desktop", agentRuntime: {} } };
});

vi.mock("./SignOutDialog.js", async () => {
  await gate.promise;
  return { RuntimeSignOutDialog: () => <p>sign-out dialog</p> };
});

import { RuntimeSignOutLifecycle } from "./SignOutLifecycle.js";

it("drops a dialog chunk that arrives after unmount", async () => {
  const first = render(<RuntimeSignOutLifecycle />);
  first.unmount();
  gate.release();
  await new Promise((resolve) => setTimeout(resolve, 0));
  expect(screen.queryByText("sign-out dialog")).not.toBeInTheDocument();
  render(<RuntimeSignOutLifecycle />);
  expect(await screen.findByText("sign-out dialog")).toBeVisible();
});
