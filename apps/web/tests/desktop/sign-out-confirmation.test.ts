/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  confirm: vi.fn(),
  signOut: vi.fn(),
}));

vi.mock("@pi-dash/constants", () => ({ API_BASE_URL: "http://localhost:18002", EUserPermissions: {} }));
vi.mock("@/services/agent-runtime", () => ({ confirmSignOut: mocks.confirm }));
vi.mock("@/services/auth.service", () => ({
  AuthService: class {
    signOut = mocks.signOut;
  },
}));
vi.mock("@/services/user.service", () => ({ UserService: vi.fn() }));
vi.mock("@/pi-dash-web/store/user/permission.store", () => ({ UserPermissionStore: vi.fn() }));
vi.mock("@/store/user/profile.store", () => ({ ProfileStore: vi.fn() }));
vi.mock("@/store/user/settings.store", () => ({ UserSettingsStore: vi.fn() }));

import { UserStore } from "@/store/user";

const root = { resetOnSignOut: vi.fn() };
const store = () => new UserStore(root as unknown as ConstructorParameters<typeof UserStore>[0]);

beforeEach(() => {
  vi.resetAllMocks();
  mocks.signOut.mockResolvedValue(undefined);
});

describe("sign-out confirmation", () => {
  it("signs out with the user's choice once they confirm", async () => {
    mocks.confirm.mockResolvedValue({ deleteChatHistory: true });
    await store().signOut();
    expect(mocks.signOut).toHaveBeenCalledWith("http://localhost:18002", { deleteChatHistory: true });
    expect(root.resetOnSignOut).toHaveBeenCalledOnce();
  });

  it("stays signed in when the user cancels", async () => {
    mocks.confirm.mockResolvedValue(null);
    await store().signOut();
    expect(mocks.signOut).not.toHaveBeenCalled();
    expect(root.resetOnSignOut).not.toHaveBeenCalled();
  });

  it("does not ask, and deletes nothing, when sign-out follows another confirmed action", async () => {
    await store().signOut({ skipConfirmation: true });
    expect(mocks.confirm).not.toHaveBeenCalled();
    expect(mocks.signOut).toHaveBeenCalledWith("http://localhost:18002", undefined);
    expect(root.resetOnSignOut).toHaveBeenCalledOnce();
  });
});
