// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the WebNewDriver keys/voice/registry/desktop/editor methods
// (NEWFRONT-608, AGT-050-052/057/059/060/063). A stub Page stands in for
// Playwright: the assertions are about the selectors each read uses, the
// payloads the stubs serve, and the requests each spy records.
import type { Locator, Page, Route } from "@playwright/test";
import { describe, expect, it, vi } from "vitest";

import { WebNewDriver } from "./web-new";

interface StubLocator {
  count?: () => Promise<number>;
  isVisible?: () => Promise<boolean>;
  isEnabled?: () => Promise<boolean>;
  isDisabled?: () => Promise<boolean>;
  click?: (options?: unknown) => Promise<void>;
  hover?: () => Promise<void>;
  fill?: (value: string) => Promise<void>;
  press?: (key: string) => Promise<void>;
  waitFor?: (options?: unknown) => Promise<unknown>;
  getAttribute?: (name: string) => Promise<string | null>;
  innerText?: () => Promise<string>;
  textContent?: () => Promise<string | null>;
  inputValue?: () => Promise<string>;
  first?: () => StubLocator;
  last?: () => StubLocator;
  nth?: (index: number) => StubLocator;
  getByRole?: (role: string, options?: unknown) => StubLocator;
  filter?: (options?: unknown) => StubLocator;
  locator?: (selector: string) => StubLocator;
  [key: string]: unknown;
}

function locator(overrides: Partial<StubLocator> = {}): Locator & StubLocator {
  const node: StubLocator = {
    count: async () => 1,
    isVisible: async () => true,
    isEnabled: async () => true,
    isDisabled: async () => false,
    click: async () => undefined,
    hover: async () => undefined,
    fill: async () => undefined,
    press: async () => undefined,
    waitFor: async () => undefined,
    getAttribute: async () => null,
    innerText: async () => "",
    textContent: async () => "",
    inputValue: async () => "",
    ...overrides,
  };
  const defaults: StubLocator = {
    first: () => node,
    last: () => node,
    nth: () => node,
    getByRole: () => node,
    filter: () => node,
    locator: () => node,
  };
  for (const [name, value] of Object.entries(defaults)) {
    if (node[name] === undefined) node[name] = value;
  }
  return node as unknown as Locator & StubLocator;
}

function pageOf(overrides: Partial<Record<string, unknown>> = {}): Page {
  return {
    goto: vi.fn(async () => null),
    reload: vi.fn(async () => null),
    waitForLoadState: vi.fn(async () => undefined),
    waitForTimeout: vi.fn(async () => undefined),
    waitForFunction: vi.fn(async () => undefined),
    url: () => "https://app.test/acme/assistant/t1",
    mouse: { down: vi.fn(async () => undefined), up: vi.fn(async () => undefined) },
    context: () => ({ grantPermissions: vi.fn(async () => undefined), clearPermissions: vi.fn(async () => undefined) }),
    addInitScript: vi.fn(async () => undefined),
    on: vi.fn(() => undefined),
    off: vi.fn(() => undefined),
    request: { get: vi.fn(async () => ({ status: () => 200, json: async () => ({}) })) },
    getByRole: vi.fn(() => locator()),
    locator: vi.fn(() => locator()),
    route: vi.fn(async () => undefined),
    unroute: vi.fn(async () => undefined),
    ...overrides,
  } as unknown as Page;
}

function handlerFor(route: ReturnType<typeof vi.fn>, pattern: string) {
  const call = route.mock.calls.find((args) => args[0] === pattern);
  if (call === undefined) throw new Error(`no route installed for ${pattern}`);
  return call[1] as (route: Route) => Promise<void>;
}

function stubRoute(request: Record<string, unknown>) {
  return {
    request: () => ({
      method: () => "POST",
      url: () => "https://app.test/",
      postDataJSON: () => null,
      postDataBuffer: () => null,
      headerValue: async () => null,
      ...request,
    }),
    fallback: vi.fn(async () => undefined),
    fulfill: vi.fn(async (_options: unknown) => undefined),
    abort: vi.fn(async () => undefined),
  };
}

describe("mic reads", () => {
  it("reads the mic label and hides with the control", async () => {
    const mic = locator({ getAttribute: async () => "Hold to dictate" });
    const page = pageOf({ locator: vi.fn(() => mic) });
    const driver = new WebNewDriver(page);
    expect(await driver.assistantMicLabel()).toBe("Hold to dictate");
    expect(page.locator).toHaveBeenCalledWith('[data-testid="assistant-mic"]');
    const hidden = new WebNewDriver(pageOf({ locator: () => locator({ count: async () => 0 }) }));
    expect(await hidden.assistantMicLabel()).toBeNull();
  });

  it("clicks the mic control", async () => {
    const click = vi.fn(async () => undefined);
    const driver = new WebNewDriver(pageOf({ locator: () => locator({ click }) }));
    await driver.assistantClickMic();
    expect(click).toHaveBeenCalledTimes(1);
  });

  it("reads the dictation hint and blanks it when empty or absent", async () => {
    const hint = locator({ innerText: async () => "  Recording — release to transcribe · 0:04  " });
    const driver = new WebNewDriver(pageOf({ locator: () => hint }));
    expect(await driver.assistantDictationHint()).toBe("Recording — release to transcribe · 0:04");
    const blank = new WebNewDriver(pageOf({ locator: () => locator({ innerText: async () => "  " }) }));
    expect(await blank.assistantDictationHint()).toBeNull();
    const absent = new WebNewDriver(pageOf({ locator: () => locator({ count: async () => 0 }) }));
    expect(await absent.assistantDictationHint()).toBeNull();
  });

  it("reads the mic lock and survives a missing control", async () => {
    const locked = new WebNewDriver(pageOf({ locator: () => locator({ isDisabled: async () => true }) }));
    expect(await locked.assistantMicDisabled()).toBe(true);
    const free = new WebNewDriver(pageOf({ locator: () => locator({ isDisabled: async () => false }) }));
    expect(await free.assistantMicDisabled()).toBe(false);
    const missing = new WebNewDriver(
      pageOf({ locator: () => locator({ isDisabled: async () => Promise.reject(new Error("gone")) }) })
    );
    expect(await missing.assistantMicDisabled()).toBe(false);
  });
});

describe("mic gestures", () => {
  function gesturePage() {
    const down = vi.fn(async () => undefined);
    const up = vi.fn(async () => undefined);
    const hover = vi.fn(async () => undefined);
    const waitForTimeout = vi.fn(async () => undefined);
    const page = pageOf({
      mouse: { down, up },
      waitForTimeout,
      locator: () => locator({ hover }),
    });
    return { page, down, up, hover, waitForTimeout };
  }

  it("holds the mic through the mouse", async () => {
    const { page, down, up, hover, waitForTimeout } = gesturePage();
    await new WebNewDriver(page).assistantMicHold(700);
    expect(hover).toHaveBeenCalledTimes(1);
    expect(down).toHaveBeenCalledTimes(1);
    expect(waitForTimeout).toHaveBeenCalledWith(700);
    expect(up).toHaveBeenCalledTimes(1);
  });

  it("presses and releases the mic separately", async () => {
    const { page, down, up, hover } = gesturePage();
    const driver = new WebNewDriver(page);
    await driver.assistantMicDown();
    await driver.assistantMicUp();
    expect(hover).toHaveBeenCalledTimes(1);
    expect(down).toHaveBeenCalledTimes(1);
    expect(up).toHaveBeenCalledTimes(1);
  });

  it("holds past the observation before releasing", async () => {
    const { page, up, waitForTimeout } = gesturePage();
    await new WebNewDriver(page).assistantMicUpAfter(900);
    expect(waitForTimeout).toHaveBeenCalledWith(900);
    expect(up).toHaveBeenCalledTimes(1);
  });

  it("grants and clears microphone permission", async () => {
    const grantPermissions = vi.fn(async () => undefined);
    const clearPermissions = vi.fn(async () => undefined);
    const driver = new WebNewDriver(pageOf({ context: () => ({ grantPermissions, clearPermissions }) }));
    await driver.assistantSetMicrophonePermission("granted");
    expect(grantPermissions).toHaveBeenCalledWith(["microphone"]);
    await driver.assistantSetMicrophonePermission("denied");
    expect(clearPermissions).toHaveBeenCalledTimes(1);
  });

  it("hides capture APIs behind the unsupported simulation", async () => {
    const addInitScript = vi.fn(async () => undefined);
    await new WebNewDriver(pageOf({ addInitScript })).assistantSimulateUnsupportedCapture();
    const script = addInitScript.mock.calls[0]?.[0] as () => void;
    const fakeWindow = {} as Record<string, unknown>;
    const fakeNavigator = { mediaDevices: {} } as Record<string, unknown>;
    vi.stubGlobal("window", fakeWindow);
    vi.stubGlobal("navigator", fakeNavigator);
    try {
      script();
    } finally {
      vi.unstubAllGlobals();
    }
    expect(fakeWindow["MediaRecorder"]).toBeUndefined();
    expect(fakeNavigator["mediaDevices"]).toBeUndefined();
  });

  it("rejects capture as denied behind the denial simulation", async () => {
    const addInitScript = vi.fn(async () => undefined);
    await new WebNewDriver(pageOf({ addInitScript })).assistantSimulateMicDenial();
    const script = addInitScript.mock.calls[0]?.[0] as () => void;
    const mediaDevices = { getUserMedia: async () => ({}) };
    vi.stubGlobal("navigator", { mediaDevices });
    try {
      script();
    } finally {
      vi.unstubAllGlobals();
    }
    await expect(mediaDevices.getUserMedia()).rejects.toMatchObject({ name: "NotAllowedError" });
  });

  it("falls back to assignment when the denial cannot redefine the getter", async () => {
    const addInitScript = vi.fn(async () => undefined);
    await new WebNewDriver(pageOf({ addInitScript })).assistantSimulateMicDenial();
    const script = addInitScript.mock.calls[0]?.[0] as () => void;
    const original = async () => ({});
    const mediaDevices = Object.seal({ getUserMedia: original });
    vi.stubGlobal("navigator", { mediaDevices });
    try {
      script();
    } finally {
      vi.unstubAllGlobals();
    }
    expect(mediaDevices.getUserMedia).not.toBe(original);
    await expect(mediaDevices.getUserMedia()).rejects.toMatchObject({ name: "NotAllowedError" });
  });

  it("ignores the denial simulation without capture APIs", async () => {
    const addInitScript = vi.fn(async () => undefined);
    await new WebNewDriver(pageOf({ addInitScript })).assistantSimulateMicDenial();
    const script = addInitScript.mock.calls[0]?.[0] as () => void;
    vi.stubGlobal("navigator", {});
    try {
      expect(() => script()).not.toThrow();
    } finally {
      vi.unstubAllGlobals();
    }
  });
});

describe("transcribe stubs", () => {
  const PATTERN = "**/api/users/me/ai-assistant/transcribe/";

  it("serves canned text and records the multipart upload", async () => {
    const route = vi.fn(async () => undefined);
    const driver = new WebNewDriver(pageOf({ route }));
    await driver.assistantStubTranscribeText("canned words");
    const handler = handlerFor(route, PATTERN);
    const audio = Buffer.from(
      '------boundary\r\nContent-Disposition: form-data; name="file"; filename="clip.webm"\r\n\r\nbytes'
    );
    const seen = stubRoute({
      postDataBuffer: () => audio,
      headerValue: async () => "multipart/form-data; boundary=----boundary",
    });
    await handler(seen as unknown as Route);
    expect(seen.fulfill).toHaveBeenCalledWith({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({ text: "canned words" }),
    });
    await expect(driver.assistantTranscribeRequests()).resolves.toEqual([
      { contentType: "multipart/form-data; boundary=----boundary", hasFilePart: true, byteLength: audio.length },
    ]);
  });

  it("serves forced failures and records bodiless uploads", async () => {
    const route = vi.fn(async () => undefined);
    const driver = new WebNewDriver(pageOf({ route }));
    await driver.assistantFailTranscribe(500, { error: "transcribe_failed" });
    const handler = handlerFor(route, PATTERN);
    const seen = stubRoute({});
    await handler(seen as unknown as Route);
    expect(seen.fulfill).toHaveBeenCalledWith({
      status: 500,
      contentType: "application/json",
      body: JSON.stringify({ error: "transcribe_failed" }),
    });
    await expect(driver.assistantTranscribeRequests()).resolves.toEqual([
      { contentType: "", hasFilePart: false, byteLength: 0 },
    ]);
  });

  it("lets non-uploads through untouched", async () => {
    const route = vi.fn(async () => undefined);
    const driver = new WebNewDriver(pageOf({ route }));
    await driver.assistantStubTranscribeText("unused");
    const handler = handlerFor(route, PATTERN);
    const seen = stubRoute({ method: () => "GET" });
    await handler(seen as unknown as Route);
    expect(seen.fallback).toHaveBeenCalledTimes(1);
    expect(seen.fulfill).not.toHaveBeenCalled();
    await expect(driver.assistantTranscribeRequests()).resolves.toEqual([]);
  });

  it("removes the transcribe stub", async () => {
    const unroute = vi.fn(async () => undefined);
    await new WebNewDriver(pageOf({ unroute })).assistantClearTranscribeStubs();
    expect(unroute).toHaveBeenCalledWith(PATTERN);
  });
});

describe("settings links and desktop watch", () => {
  it("reads the current hash", async () => {
    const driver = new WebNewDriver(
      pageOf({ url: () => "https://app.test/settings/profile/ai-assistant#voice-dictation" })
    );
    expect(await driver.assistantCurrentHash()).toBe("#voice-dictation");
  });

  it("collects settings links across the assistant surfaces", async () => {
    const threadLinks = locator({
      count: async () => 2,
      nth: (index: number) =>
        locator({ getAttribute: async () => (index === 0 ? "/settings/profile/ai-assistant" : null) }),
    });
    const thread = locator({ locator: () => threadLinks });
    const page = pageOf({
      locator: (selector: string) =>
        selector === '[data-testid="assistant-thread"]' ? thread : locator({ count: async () => 0 }),
    });
    await expect(new WebNewDriver(page).assistantChatSettingsLinks()).resolves.toEqual([
      "/settings/profile/ai-assistant",
      "",
    ]);
  });

  it("reads no settings links off missing surfaces", async () => {
    const driver = new WebNewDriver(pageOf({ locator: () => locator({ count: async () => 0 }) }));
    await expect(driver.assistantChatSettingsLinks()).resolves.toEqual([]);
  });

  it("watches the desktop-gated endpoints only", async () => {
    const handlers: ((request: { method(): string; url(): string }) => void)[] = [];
    const on = vi.fn((_event: string, handler: (request: { method(): string; url(): string }) => void) => {
      handlers.push(handler);
    });
    const off = vi.fn(() => undefined);
    const driver = new WebNewDriver(pageOf({ on, off }));
    await driver.assistantStartDesktopCallWatch();
    const watch = handlers[0]!;
    watch({ method: () => "GET", url: () => "https://app.test/api/users/me/ai-assistant/agent-profile/" });
    watch({ method: () => "POST", url: () => "https://app.test/api/users/me/ai-assistant/agent-token/" });
    watch({ method: () => "GET", url: () => "https://app.test/api/users/me/ai-assistant/config/" });
    expect(await driver.assistantDesktopCallsObserved()).toEqual([
      { method: "GET", url: "https://app.test/api/users/me/ai-assistant/agent-profile/" },
      { method: "POST", url: "https://app.test/api/users/me/ai-assistant/agent-token/" },
    ]);
    await driver.assistantStopDesktopCallWatch();
    expect(off).toHaveBeenCalledWith("request", watch);
  });

  it("restarts the watch cleanly and stops safely without one", async () => {
    const off = vi.fn(() => undefined);
    const driver = new WebNewDriver(pageOf({ on: vi.fn(() => undefined), off }));
    await driver.assistantStopDesktopCallWatch();
    expect(off).not.toHaveBeenCalled();
    await driver.assistantStartDesktopCallWatch();
    await driver.assistantStartDesktopCallWatch();
    expect(off).toHaveBeenCalledTimes(1);
    expect(await driver.assistantDesktopCallsObserved()).toEqual([]);
  });
});

describe("instance and GPT stubs", () => {
  it("patches the nested instance flag", async () => {
    const payload = { config: { has_llm_configured: false }, instance: { id: "i" } };
    const get = vi.fn(async () => ({ status: () => 200, json: async () => payload }));
    const route = vi.fn(async () => undefined);
    const driver = new WebNewDriver(pageOf({ route, request: { get } }));
    await driver.assistantStubInstanceLlm(true);
    expect(get).toHaveBeenCalledWith("/api/instances/");
    const handler = handlerFor(route, "**/api/instances/");
    const seen = stubRoute({ method: () => "GET" });
    await handler(seen as unknown as Route);
    expect(seen.fulfill).toHaveBeenCalledWith({
      status: 200,
      contentType: "application/json",
      json: { config: { has_llm_configured: true }, instance: { id: "i" } },
    });
  });

  it("patches a top-level flag when no nested config exists", async () => {
    const payload = { has_llm_configured: false };
    const get = vi.fn(async () => ({ status: () => 200, json: async () => payload }));
    const route = vi.fn(async () => undefined);
    await new WebNewDriver(pageOf({ route, request: { get } })).assistantStubInstanceLlm(true);
    const handler = handlerFor(route, "**/api/instances/");
    const seen = stubRoute({ method: () => "GET" });
    await handler(seen as unknown as Route);
    expect(seen.fulfill).toHaveBeenCalledWith({
      status: 200,
      contentType: "application/json",
      json: { has_llm_configured: true },
    });
  });

  it("lets instance writes through untouched", async () => {
    const get = vi.fn(async () => ({ status: () => 200, json: async () => ({}) }));
    const route = vi.fn(async () => undefined);
    await new WebNewDriver(pageOf({ route, request: { get } })).assistantStubInstanceLlm(false);
    const handler = handlerFor(route, "**/api/instances/");
    const seen = stubRoute({ method: () => "PATCH" });
    await handler(seen as unknown as Route);
    expect(seen.fallback).toHaveBeenCalledTimes(1);
  });

  it("removes the instance stub", async () => {
    const unroute = vi.fn(async () => undefined);
    await new WebNewDriver(pageOf({ unroute })).assistantClearInstanceStub();
    expect(unroute).toHaveBeenCalledWith("**/api/instances/");
  });

  it("serves a canned GPT answer and records the task body", async () => {
    const route = vi.fn(async () => undefined);
    const driver = new WebNewDriver(pageOf({ route }));
    await driver.assistantStubGptAnswer({ response: "canned", response_html: "<p>canned</p>" });
    const handler = handlerFor(route, "**/api/workspaces/*/projects/*/ai-assistant/");
    const seen = stubRoute({ postDataJSON: () => ({ prompt: "", task: "Summarize" }) });
    await handler(seen as unknown as Route);
    expect(seen.fulfill).toHaveBeenCalledWith({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({
        response: "canned",
        response_html: "<p>canned</p>",
        project_detail: {
          id: "00000000-0000-4000-8000-000000000001",
          identifier: "PAR",
          name: "Parity project",
          cover_image: null,
          cover_image_url: null,
          logo_props: {},
          description: "",
          is_default: false,
        },
        workspace_detail: {
          name: "Parity workspace",
          slug: "parity-workspace",
          id: "00000000-0000-4000-8000-000000000002",
          logo_url: null,
        },
      }),
    });
    await expect(driver.assistantGptRequests()).resolves.toEqual([{ prompt: "", task: "Summarize" }]);
  });

  it("serves forced GPT failures and blanks unshaped bodies", async () => {
    const route = vi.fn(async () => undefined);
    const driver = new WebNewDriver(pageOf({ route }));
    await driver.assistantFailGptAnswer(429, { error: "quota" });
    const handler = handlerFor(route, "**/api/workspaces/*/projects/*/ai-assistant/");
    const failed = stubRoute({ postDataJSON: () => ({ prompt: 5, task: null }) });
    await handler(failed as unknown as Route);
    expect(failed.fulfill).toHaveBeenCalledWith({
      status: 429,
      contentType: "application/json",
      body: JSON.stringify({ error: "quota" }),
    });
    const empty = stubRoute({ postDataJSON: () => null });
    await handler(empty as unknown as Route);
    await expect(driver.assistantGptRequests()).resolves.toEqual([
      { prompt: "", task: "" },
      { prompt: "", task: "" },
    ]);
  });

  it("lets GPT reads through untouched", async () => {
    const route = vi.fn(async () => undefined);
    const driver = new WebNewDriver(pageOf({ route }));
    await driver.assistantStubGptAnswer({ response: "x", response_html: "<p>x</p>" });
    const handler = handlerFor(route, "**/api/workspaces/*/projects/*/ai-assistant/");
    const seen = stubRoute({ method: () => "GET" });
    await handler(seen as unknown as Route);
    expect(seen.fallback).toHaveBeenCalledTimes(1);
    await expect(driver.assistantGptRequests()).resolves.toEqual([]);
  });

  it("removes the GPT stubs", async () => {
    const unroute = vi.fn(async () => undefined);
    await new WebNewDriver(pageOf({ unroute })).assistantClearGptStubs();
    expect(unroute).toHaveBeenCalledWith("**/api/workspaces/*/projects/*/ai-assistant/");
  });
});

describe("issue modal helpers", () => {
  it("opens the create dialog off the issues list", async () => {
    const click = vi.fn(async () => undefined);
    const waitFor = vi.fn(async () => undefined);
    const waitForFunction = vi.fn(async () => undefined);
    const goto = vi.fn(async () => null);
    const page = pageOf({
      goto,
      waitForFunction,
      getByRole: (role: string) => (role === "button" ? locator({ click }) : locator({ waitFor })),
    });
    await new WebNewDriver(page).issueModalOpenCreate("acme", "pid");
    expect(goto).toHaveBeenCalledWith("/acme/projects/pid/issues");
    expect(click).toHaveBeenCalledTimes(1);
    expect(waitFor).toHaveBeenCalledTimes(1);
    expect(waitForFunction).toHaveBeenCalledTimes(1);
  });

  it("reads the AI entry's visibility", async () => {
    const visibleEntry = () => locator({ getByRole: () => locator({}) });
    const shown = new WebNewDriver(pageOf({ getByRole: visibleEntry }));
    expect(await shown.issueModalAiEntryVisible()).toBe(true);
    const hidden = new WebNewDriver(
      pageOf({ getByRole: () => locator({ getByRole: () => locator({ isVisible: async () => false }) }) })
    );
    expect(await hidden.issueModalAiEntryVisible()).toBe(false);
    const absent = new WebNewDriver(
      pageOf({ getByRole: () => locator({ getByRole: () => locator({ count: async () => 0 }) }) })
    );
    expect(await absent.issueModalAiEntryVisible()).toBe(false);
  });

  it("opens the helper and fills its task", async () => {
    const click = vi.fn(async () => undefined);
    const fill = vi.fn(async () => undefined);
    const waitFor = vi.fn(async () => undefined);
    const page = pageOf({
      getByRole: () => locator({ getByRole: () => locator({ click }) }),
      locator: () => locator({ fill, waitFor }),
    });
    const driver = new WebNewDriver(page);
    await driver.issueModalAiOpen();
    expect(click).toHaveBeenCalledTimes(1);
    expect(waitFor).toHaveBeenCalledTimes(1);
    await driver.issueModalAiFillTask("Summarize");
    expect(fill).toHaveBeenCalledWith("Summarize");
  });

  it("drives generate, review, use and close through the panel", async () => {
    const generate = vi.fn(async () => undefined);
    const use = vi.fn(async () => undefined);
    const close = vi.fn(async () => undefined);
    const panel = locator({
      getByRole: (_role: string, options?: unknown) => {
        const name = (options as { name?: unknown })?.name;
        if (name instanceof RegExp) return locator({ click: generate });
        if (name === "Use this response") return locator({ click: use });
        return locator({ click: close });
      },
      locator: (selector: string) =>
        selector === '[data-testid="modal-ai-response"]'
          ? locator({ innerText: async () => "  canned words  " })
          : locator({}),
    });
    const driver = new WebNewDriver(pageOf({ locator: () => panel }));
    await driver.issueModalAiGenerate();
    expect(generate).toHaveBeenCalledTimes(1);
    expect(await driver.issueModalAiResponse()).toBe("canned words");
    expect(await driver.issueModalAiInvalidVisible()).toBe(true);
    await driver.issueModalAiUseResponse();
    expect(use).toHaveBeenCalledTimes(1);
    await driver.issueModalAiClose();
    expect(close).toHaveBeenCalledTimes(1);
  });

  it("blanks the review when empty or absent", async () => {
    const blank = new WebNewDriver(
      pageOf({ locator: () => locator({ locator: () => locator({ innerText: async () => "   " }) }) })
    );
    expect(await blank.issueModalAiResponse()).toBeNull();
    const absent = new WebNewDriver(
      pageOf({ locator: () => locator({ locator: () => locator({ count: async () => 0 }) }) })
    );
    expect(await absent.issueModalAiResponse()).toBeNull();
    expect(await absent.issueModalAiInvalidVisible()).toBe(false);
  });

  it("reads the description editor's text", async () => {
    const editor = () => locator({ locator: () => locator({ textContent: async () => "  hello  " }) });
    expect(await new WebNewDriver(pageOf({ getByRole: editor })).issueModalDescriptionText()).toBe("hello");
    const nullish = () => locator({ locator: () => locator({ textContent: async () => null }) });
    expect(await new WebNewDriver(pageOf({ getByRole: nullish })).issueModalDescriptionText()).toBe("");
    const absent = () => locator({ locator: () => locator({ count: async () => 0 }) });
    expect(await new WebNewDriver(pageOf({ getByRole: absent })).issueModalDescriptionText()).toBeNull();
  });
});

describe("dictation kill switch", () => {
  it("reports the mic only when it is present and visible", async () => {
    const shown = locator({ count: async () => 1, isVisible: async () => true });
    expect(await new WebNewDriver(pageOf({ locator: () => shown })).assistantMicVisible()).toBe(true);
    const hidden = locator({ count: async () => 1, isVisible: async () => false });
    expect(await new WebNewDriver(pageOf({ locator: () => hidden })).assistantMicVisible()).toBe(false);
    const absent = locator({ count: async () => 0 });
    expect(await new WebNewDriver(pageOf({ locator: () => absent })).assistantMicVisible()).toBe(false);
  });

  it("flips document visibility so a refocus refetches the config", async () => {
    const evaluate = vi.fn(async (fn: () => void) => fn());
    const dispatchEvent = vi.fn(() => true);
    vi.stubGlobal("window", { dispatchEvent });
    vi.stubGlobal("document", {});
    try {
      await new WebNewDriver(pageOf({ evaluate })).assistantRefocus();
      expect(evaluate).toHaveBeenCalledTimes(1);
      expect(dispatchEvent).toHaveBeenCalledTimes(2);
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it("counts only speech-config GETs", async () => {
    const listeners: ((request: { url(): string; method(): string }) => void)[] = [];
    const page = pageOf({ on: vi.fn((_event: string, handler: never) => listeners.push(handler)) });
    const driver = new WebNewDriver(page);
    expect(await driver.assistantSpeechConfigReads()).toBe(0);
    expect(page.on).toHaveBeenCalledWith("request", expect.any(Function));
    const emit = listeners[0]!;
    emit({ method: () => "GET", url: () => "https://app.test/api/users/me/ai-assistant/stt-config/" });
    emit({ method: () => "PUT", url: () => "https://app.test/api/users/me/ai-assistant/stt-config/" });
    emit({ method: () => "GET", url: () => "https://app.test/api/users/me/ai-assistant/config/" });
    expect(await driver.assistantSpeechConfigReads()).toBe(1);
    // A second read attaches no second listener.
    expect(page.on).toHaveBeenCalledTimes(1);
  });

  it("serves the flag on the read and refuses writes while DELETE stays open", async () => {
    const route = vi.fn(async () => undefined);
    const request = {
      get: vi.fn(async () => ({
        status: () => 200,
        json: async () => ({
          base_url: "https://stt.test",
          model_name: "whisper",
          has_api_key: true,
          last_verified_at: null,
        }),
      })),
    };
    const driver = new WebNewDriver(pageOf({ route, request }));
    await driver.settingsProfileStubDictationEnabled(false);
    const configHandler = handlerFor(route, "**/api/users/me/ai-assistant/stt-config/");
    const read = stubRoute({ method: () => "GET" });
    await configHandler(read as unknown as Route);
    expect(read.fulfill).toHaveBeenCalledWith({
      status: 200,
      contentType: "application/json",
      json: {
        base_url: "https://stt.test",
        model_name: "whisper",
        has_api_key: true,
        last_verified_at: null,
        enabled: false,
      },
    });
    await driver.settingsProfileStubDictationWritesDisabled();
    const write = stubRoute({ method: () => "PUT" });
    await configHandler(write as unknown as Route);
    expect(write.fulfill).toHaveBeenCalledWith({
      status: 404,
      contentType: "application/json",
      body: JSON.stringify({ error: "dictation_disabled" }),
    });
    const remove = stubRoute({ method: () => "DELETE" });
    await configHandler(remove as unknown as Route);
    expect(remove.fallback).toHaveBeenCalledTimes(1);
    const test = stubRoute({
      method: () => "POST",
      url: () => "https://app.test/api/users/me/ai-assistant/stt-config/test/",
    });
    const testHandler = handlerFor(route, "**/api/users/me/ai-assistant/stt-config/test/");
    await testHandler(test as unknown as Route);
    expect(test.fulfill).toHaveBeenCalledWith({
      status: 404,
      contentType: "application/json",
      body: JSON.stringify({ error: "dictation_disabled" }),
    });
    const transcribe = handlerFor(route, "**/api/users/me/ai-assistant/transcribe/");
    const upload = stubRoute({ method: () => "POST" });
    await transcribe(upload as unknown as Route);
    expect(upload.fulfill).toHaveBeenCalledWith({
      status: 404,
      contentType: "application/json",
      body: JSON.stringify({ error: "dictation_disabled" }),
    });
  });

  it("leaves traffic alone when no stub state is armed and removes the routes", async () => {
    const route = vi.fn(async () => undefined);
    const unroute = vi.fn(async () => undefined);
    const driver = new WebNewDriver(pageOf({ route, unroute }));
    await driver.settingsProfileStubDictationEnabled(true);
    const configHandler = handlerFor(route, "**/api/users/me/ai-assistant/stt-config/");
    const write = stubRoute({ method: () => "PUT" });
    await configHandler(write as unknown as Route);
    expect(write.fallback).toHaveBeenCalledTimes(1);
    await driver.settingsProfileClearDictationStubs();
    expect(unroute).toHaveBeenCalledWith("**/api/users/me/ai-assistant/stt-config/");
    expect(unroute).toHaveBeenCalledWith("**/api/users/me/ai-assistant/stt-config/test/");
    expect(unroute).toHaveBeenCalledWith("**/api/users/me/ai-assistant/transcribe/");
  });
});

describe("dictation API probe", () => {
  it("drives every speech endpoint in the browser and reports its status", async () => {
    const calls: { url: string; method: string }[] = [];
    const fetchMock = vi.fn(async (input: string, init?: { method?: string }) => {
      const method = init?.method ?? "GET";
      calls.push({ url: input, method });
      const status = method === "GET" ? 200 : method === "DELETE" ? 204 : 404;
      return {
        status,
        json: async () => ({ enabled: false }),
      };
    });
    vi.stubGlobal("fetch", fetchMock);
    try {
      const driver = new WebNewDriver(pageOf({ evaluate: async (fn: () => unknown) => fn() }));
      const probe = await driver.assistantDictationApiProbe();
      expect(probe).toEqual({ read: 200, readEnabled: false, save: 404, test: 404, transcribe: 404, remove: 204 });
      expect(calls.map((call) => `${call.method} ${call.url}`)).toEqual([
        "GET /api/users/me/ai-assistant/stt-config/",
        "PUT /api/users/me/ai-assistant/stt-config/",
        "POST /api/users/me/ai-assistant/stt-config/test/",
        "POST /api/users/me/ai-assistant/transcribe/",
        "DELETE /api/users/me/ai-assistant/stt-config/",
      ]);
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it("reports a null flag when the read has no boolean, or no body", async () => {
    let call = 0;
    const fetchMock = vi.fn(async () => {
      call += 1;
      return {
        status: 200,
        json: async () => {
          if (call === 1) return { enabled: "yes" };
          throw new Error("not json");
        },
      };
    });
    vi.stubGlobal("fetch", fetchMock);
    try {
      const driver = new WebNewDriver(pageOf({ evaluate: async (fn: () => unknown) => fn() }));
      expect((await driver.assistantDictationApiProbe()).readEnabled).toBeNull();
      expect((await driver.assistantDictationApiProbe()).readEnabled).toBeNull();
    } finally {
      vi.unstubAllGlobals();
    }
  });
});
