// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Page-export registration load (PAGE-074): the chunk load settles either way
// and a failed load is consumed rather than escaping as an unhandled rejection.
import { describe, expect, it, vi } from "vitest";

import { loadPageExportRegistration } from "./loadPageExportRegistration.js";

describe("loadPageExportRegistration", () => {
  it("reports a loaded registration chunk", async () => {
    const load = vi.fn(async () => ({ registerPageExport: () => undefined }));
    await expect(loadPageExportRegistration(load)).resolves.toBe(true);
    expect(load).toHaveBeenCalledTimes(1);
  });

  it("consumes a failed chunk load and reports it did not register", async () => {
    const failure = new Error('Closing rpc while "fetch" was pending');
    const load = vi.fn(async () => {
      throw failure;
    });
    await expect(loadPageExportRegistration(load)).resolves.toBe(false);
    expect(load).toHaveBeenCalledTimes(1);
  });
});
