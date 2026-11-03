// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render } from "@testing-library/react";
import { expect, it, vi } from "vitest";

const loaded = vi.hoisted(() => ({ dialog: false }));

vi.mock("../../../core/platform/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../core/platform/index.js")>();
  return { ...original, platform: { ...original.platform, kind: "web", agentRuntime: undefined } };
});

vi.mock("./SignOutDialog.js", () => {
  loaded.dialog = true;
  return { RuntimeSignOutDialog: () => <p>sign-out dialog</p> };
});

import { getRuntimeSignOutHooks } from "../../../core/session/index.js";
import { RuntimeSignOutLifecycle } from "./SignOutLifecycle.js";

it("is absent, registers no runtime hooks and never loads the dialog on web", () => {
  const { container } = render(<RuntimeSignOutLifecycle />);
  expect(container).toBeEmptyDOMElement();
  expect(getRuntimeSignOutHooks()).toBeNull();
  expect(loaded.dialog).toBe(false);
});
