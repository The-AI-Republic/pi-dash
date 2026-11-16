// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { createMemoryStorage, installTestStorage } from "./memoryStorage.js";

describe("createMemoryStorage", () => {
  it("implements the Storage surface", () => {
    const storage = createMemoryStorage();
    expect(storage.length).toBe(0);
    storage.setItem("a", "1");
    storage.setItem("b", "2");
    expect(storage.length).toBe(2);
    expect(storage.getItem("a")).toBe("1");
    expect(storage.getItem("missing")).toBeNull();
    expect(storage.key(0)).toBe("a");
    expect(storage.key(1)).toBe("b");
    expect(storage.key(2)).toBeNull();
    storage.removeItem("a");
    expect(storage.length).toBe(1);
    storage.clear();
    expect(storage.length).toBe(0);
    expect(storage.key(0)).toBeNull();
  });

  it("stringifies values like the browser profile", () => {
    const storage = createMemoryStorage();
    storage.setItem("count", 7 as unknown as string);
    expect(storage.getItem("count")).toBe("7");
  });
});

describe("installTestStorage", () => {
  it("replaces a shadowed or missing global with a fresh store", () => {
    const scope: { document?: unknown; localStorage?: unknown } = { document: {}, localStorage: undefined };
    installTestStorage(scope);
    const first = scope.localStorage as Storage;
    first.setItem("key", "value");
    expect(first.getItem("key")).toBe("value");
    installTestStorage(scope);
    expect(scope.localStorage).not.toBe(first);
    expect((scope.localStorage as Storage).getItem("key")).toBeNull();
  });

  it("leaves Node-env suites without a storage", () => {
    const scope: { document?: unknown; localStorage?: unknown } = {};
    installTestStorage(scope);
    expect(scope.localStorage).toBeUndefined();
  });
});
