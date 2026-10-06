/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

type Choice = { deleteChatHistory: boolean } | null;
type Prompt = (resolve: (choice: Choice) => void) => void;

const mocks = vi.hoisted(() => ({ prompt: null as Prompt | null }));

vi.mock("@pi-dash/i18n", () => ({ useTranslation: () => ({ t: (text: string) => text }) }));
vi.mock("@/services/agent-runtime", () => ({
  registerSignOutPrompt: (prompt: Prompt | null) => {
    mocks.prompt = prompt;
  },
}));

import { DesktopSignOutDialog } from "../../core/components/desktop-sign-out-dialog";

afterEach(cleanup);

/** Open the dialog the way `confirmSignOut` does and return what it resolves. */
async function ask() {
  let answer: Promise<Choice> = Promise.resolve(null);
  await act(async () => {
    answer = new Promise((resolve) => mocks.prompt!(resolve));
  });
  return answer;
}

describe("desktop sign-out confirmation", () => {
  it("stays closed until a sign-out asks", () => {
    render(<DesktopSignOutDialog />);
    expect(screen.queryByText("Are you sure you want to sign out?")).toBeNull();
  });

  it("signs out without deleting history by default", async () => {
    render(<DesktopSignOutDialog />);
    const answer = ask();
    await screen.findByText("Are you sure you want to sign out?");
    expect(screen.getByRole<HTMLInputElement>("checkbox", { name: "Delete chat history" }).checked).toBe(false);
    fireEvent.click(screen.getByRole("button", { name: "Sign out" }));
    await expect(answer).resolves.toEqual({ deleteChatHistory: false });
  });

  it("deletes history only when the box is ticked, and starts unticked next time", async () => {
    render(<DesktopSignOutDialog />);
    const first = ask();
    fireEvent.click(await screen.findByRole("checkbox", { name: "Delete chat history" }));
    fireEvent.click(screen.getByRole("button", { name: "Sign out" }));
    await expect(first).resolves.toEqual({ deleteChatHistory: true });

    const second = ask();
    expect((await screen.findByRole<HTMLInputElement>("checkbox", { name: "Delete chat history" })).checked).toBe(
      false
    );
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    await expect(second).resolves.toBeNull();
  });

  it("cancels a pending request when it unmounts", async () => {
    const { unmount } = render(<DesktopSignOutDialog />);
    const answer = ask();
    unmount();
    await expect(answer).resolves.toBeNull();
    expect(mocks.prompt).toBeNull();
  });
});
