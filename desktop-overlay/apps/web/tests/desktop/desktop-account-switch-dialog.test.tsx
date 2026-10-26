/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

type Request = { daemonEmail: string; desktopEmail: string; activeRuns: number };
type Prompt = (request: Request, resolve: (confirmed: boolean) => void) => void;

const mocks = vi.hoisted(() => ({ prompt: null as Prompt | null }));

vi.mock("@pi-dash/i18n", () => ({
  useTranslation: () => ({
    t: (text: string, params?: Record<string, string | number>) =>
      text.replace(/\{(\w+)\}/g, (_match, key: string) => String(params?.[key] ?? "")),
  }),
}));
vi.mock("@/services/desktop-account-sync", () => ({
  registerAccountSwitchPrompt: (prompt: Prompt | null) => {
    mocks.prompt = prompt;
  },
}));

import { DesktopAccountSwitchDialog } from "../../core/components/desktop-account-switch-dialog";

afterEach(cleanup);

async function ask() {
  let answer: Promise<boolean> = Promise.resolve(false);
  await act(async () => {
    answer = new Promise((resolve) =>
      mocks.prompt!({ daemonEmail: "b@example.test", desktopEmail: "a@example.test", activeRuns: 2 }, resolve)
    );
  });
  return answer;
}

describe("desktop account switch confirmation", () => {
  it("says whose runs stop, and switches on confirm", async () => {
    render(<DesktopAccountSwitchDialog />);
    const answer = ask();
    expect(
      await screen.findByText(
        "Pi Dash on this computer is signed in as b@example.test with 2 runs in progress. Switch it to a@example.test? Those runs will be stopped."
      )
    ).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Switch" }));
    await expect(answer).resolves.toBe(true);
  });

  it("declines on cancel, and when it unmounts", async () => {
    const { unmount } = render(<DesktopAccountSwitchDialog />);
    const first = ask();
    fireEvent.click(await screen.findByRole("button", { name: "Cancel" }));
    await expect(first).resolves.toBe(false);
    const second = ask();
    unmount();
    await expect(second).resolves.toBe(false);
  });
});
