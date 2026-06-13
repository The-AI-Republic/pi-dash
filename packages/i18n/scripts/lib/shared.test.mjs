import { afterEach, beforeEach, describe, expect, it } from "vitest";

import {
  chatContent,
  chunkArray,
  collectEmptyEntries,
  collectFilledEntries,
  hasMatchingIcuArguments,
  isValidIcu,
  localeFileContent,
  parseArgs,
  parseJsonContent,
  resolveLanguages,
  resolveProviderConfig,
} from "./shared.mjs";

describe("parseArgs", () => {
  it("parses --key value pairs", () => {
    expect(parseArgs(["--provider", "openai", "--model", "gpt"])).toEqual({ provider: "openai", model: "gpt" });
  });

  it("parses --key=value form", () => {
    expect(parseArgs(["--base-url=https://x"])).toEqual({ base_url: "https://x" });
  });

  it("treats a trailing flag with no value as boolean true", () => {
    expect(parseArgs(["--dry-run"])).toEqual({ dry_run: true });
    expect(parseArgs(["--dry-run", "--model", "gpt"])).toEqual({ dry_run: true, model: "gpt" });
  });

  it("kebab-cases keys to snake_case", () => {
    expect(parseArgs(["--validation-report", "r.json"])).toEqual({ validation_report: "r.json" });
  });

  it("ignores non-flag tokens", () => {
    expect(parseArgs(["positional", "--model", "gpt"])).toEqual({ model: "gpt" });
  });
});

describe("collectEmptyEntries / collectFilledEntries", () => {
  const obj = { a: "", b: "x", c: "", d: "y" };

  it("collectEmptyEntries returns only empty-valued keys", () => {
    expect(collectEmptyEntries(obj)).toEqual(["a", "c"]);
  });

  it("collectFilledEntries returns only non-empty string keys", () => {
    expect(collectFilledEntries(obj)).toEqual(["b", "d"]);
  });

  it("the two are complementary and disjoint", () => {
    const empty = new Set(collectEmptyEntries(obj));
    const filled = new Set(collectFilledEntries(obj));
    for (const key of empty) expect(filled.has(key)).toBe(false);
    expect(empty.size + filled.size).toBe(Object.keys(obj).length);
  });
});

describe("chunkArray", () => {
  it("splits into chunks of the given size", () => {
    expect(chunkArray([1, 2, 3, 4, 5], 2)).toEqual([[1, 2], [3, 4], [5]]);
  });

  it("returns [] for an empty input", () => {
    expect(chunkArray([], 3)).toEqual([]);
  });
});

describe("isValidIcu", () => {
  it("accepts plain and placeholder strings", () => {
    expect(isValidIcu("Bonjour", "fr")).toBe(true);
    expect(isValidIcu("Salut {name}", "fr")).toBe(true);
  });

  it("rejects malformed ICU", () => {
    expect(isValidIcu("Salut {name", "fr")).toBe(false);
  });
});

describe("hasMatchingIcuArguments", () => {
  it("is true when placeholders match", () => {
    expect(hasMatchingIcuArguments("Hi {name}", "Salut {name}", "fr")).toBe(true);
  });

  it("is false when a placeholder is dropped or renamed", () => {
    expect(hasMatchingIcuArguments("Hi {name}", "Salut", "fr")).toBe(false);
    expect(hasMatchingIcuArguments("Hi {name}", "Salut {nom}", "fr")).toBe(false);
  });

  it("matches plural/select argument signatures", () => {
    const source = "{count, plural, one {# item} other {# items}}";
    const ok = "{count, plural, one {# article} other {# articles}}";
    expect(hasMatchingIcuArguments(source, ok, "fr")).toBe(true);
  });

  it("is false (not throwing) when either side is invalid ICU", () => {
    expect(hasMatchingIcuArguments("Hi {name}", "Salut {name", "fr")).toBe(false);
  });
});

describe("parseJsonContent", () => {
  it("parses bare JSON", () => {
    expect(parseJsonContent('{"a":1}')).toEqual({ a: 1 });
  });

  it("strips a ```json fence", () => {
    expect(parseJsonContent('```json\n{"a":1}\n```')).toEqual({ a: 1 });
  });

  it("strips a bare ``` fence", () => {
    expect(parseJsonContent("```\n[1,2]\n```")).toEqual([1, 2]);
  });
});

describe("chatContent", () => {
  it("extracts choices[0].message.content", () => {
    expect(chatContent({ choices: [{ message: { content: "hi" } }] }, "fr")).toBe("hi");
  });

  it("throws when content is absent", () => {
    expect(() => chatContent({ choices: [] }, "fr")).toThrow(/fr/);
  });
});

describe("localeFileContent", () => {
  it("emits a default-exported as-const object with the license header", () => {
    const out = localeFileContent({ Hello: "Bonjour" });
    expect(out).toContain("export default");
    expect(out).toContain("as const;");
    expect(out).toContain('"Hello": "Bonjour"');
    expect(out).toContain("SPDX-License-Identifier: AGPL-3.0-only");
  });

  it("emits {} for an empty object", () => {
    expect(localeFileContent({})).toContain("export default {} as const;");
  });
});

describe("resolveLanguages", () => {
  it("returns all non-English locales when unset", () => {
    const langs = resolveLanguages(undefined);
    expect(langs.length).toBeGreaterThan(0);
    expect(langs.some((l) => l.value === "en")).toBe(false);
  });

  it("resolves a comma list and drops English", () => {
    expect(resolveLanguages("fr,es,en").map((l) => l.value)).toEqual(["fr", "es"]);
  });

  it("throws on an unsupported locale", () => {
    expect(() => resolveLanguages("fr,xx")).toThrow(/Unsupported language "xx"/);
  });
});

describe("resolveProviderConfig", () => {
  const saved = {};
  const envKeys = [
    "I18N_TRANSLATION_PROVIDER",
    "I18N_TRANSLATION_BASE_URL",
    "I18N_TRANSLATION_API_KEY",
    "I18N_TRANSLATION_MODEL",
    "OPENAI_API_KEY",
    "FIREWORKS_API_KEY",
  ];

  beforeEach(() => {
    for (const key of envKeys) {
      saved[key] = process.env[key];
      delete process.env[key];
    }
  });

  afterEach(() => {
    for (const key of envKeys) {
      if (saved[key] === undefined) delete process.env[key];
      else process.env[key] = saved[key];
    }
  });

  it("defaults to the openai chat-completions endpoint", () => {
    const cfg = resolveProviderConfig({ api_key: "k", model: "m" });
    expect(cfg).toMatchObject({
      provider: "openai",
      baseUrl: "https://api.openai.com/v1/chat/completions",
      apiKey: "k",
      model: "m",
    });
  });

  it("uses the fireworks endpoint when provider=fireworks", () => {
    const cfg = resolveProviderConfig({ provider: "fireworks", api_key: "k", model: "m" });
    expect(cfg.baseUrl).toContain("fireworks.ai");
  });

  it("reads env fallbacks", () => {
    process.env.I18N_TRANSLATION_API_KEY = "envkey";
    process.env.I18N_TRANSLATION_MODEL = "envmodel";
    const cfg = resolveProviderConfig({});
    expect(cfg.apiKey).toBe("envkey");
    expect(cfg.model).toBe("envmodel");
  });

  it("throws when the model is required but missing", () => {
    expect(() => resolveProviderConfig({ api_key: "k" })).toThrow(/Missing model/);
  });

  it("throws when the api key is required but missing", () => {
    expect(() => resolveProviderConfig({ model: "m" })).toThrow(/Missing API key/);
  });

  it("skips the requirements for a dry run", () => {
    const cfg = resolveProviderConfig({}, { requireModel: false, requireApiKey: false });
    expect(cfg.model).toBeUndefined();
    expect(cfg.apiKey).toBeUndefined();
  });
});
