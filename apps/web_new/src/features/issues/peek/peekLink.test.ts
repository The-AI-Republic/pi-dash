// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  copy: vi.fn(),
}));

vi.mock("../../../shared/shell/shareLink.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../shared/shell/shareLink.js")>()),
  copyWorkItemLink: mocks.copy,
}));

import { copyCardLink, CARD_COPY_COPIED, CARD_COPY_FAILED } from "./peekLink.js";

beforeEach(() => {
  vi.clearAllMocks();
  mocks.copy.mockResolvedValue(true);
});

describe("copyCardLink", () => {
  it("copies the item path and passes both notices", async () => {
    await copyCardLink("acme", "WEB-7");
    expect(mocks.copy).toHaveBeenCalledWith(
      "/acme/browse/WEB-7/",
      expect.objectContaining({ copied: CARD_COPY_COPIED, failed: CARD_COPY_FAILED })
    );
  });
});
