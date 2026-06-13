// oxlint-disable no-await-in-loop -- LLM calls are intentionally sequential to avoid provider rate limits.
//
// Shared infrastructure for the i18n LLM-backed scripts (translate-missing,
// validate-translations). Holds the bits both commands need: CLI arg parsing,
// provider/key resolution, locale-file read/write, the chat-completion HTTP
// client with retry/backoff, and ICU MessageFormat argument checking.

import fs from "node:fs";
import path from "node:path";
import process from "node:process";
import { spawnSync } from "node:child_process";
import { IntlMessageFormat } from "intl-messageformat";
import ts from "typescript";

export const repoRoot = path.resolve(import.meta.dirname, "../../../..");
export const localesRoot = path.join(repoRoot, "packages/i18n/src/locales");
export const targetLocaleFile = "translations.ts";
export const fallbackLanguage = "en";

export const defaultBatchSize = 10;
export const defaultRequestTimeoutMs = 180000;
export const defaultRetryCount = 2;
export const defaultRetryDelayMs = 5000;

export const languages = [
  { value: "en", label: "English" },
  { value: "fr", label: "French" },
  { value: "es", label: "Spanish" },
  { value: "ja", label: "Japanese" },
  { value: "zh-CN", label: "Simplified Chinese" },
  { value: "zh-TW", label: "Traditional Chinese" },
  { value: "ru", label: "Russian" },
  { value: "it", label: "Italian" },
  { value: "cs", label: "Czech" },
  { value: "sk", label: "Slovak" },
  { value: "de", label: "German" },
  { value: "ua", label: "Ukrainian" },
  { value: "pl", label: "Polish" },
  { value: "ko", label: "Korean" },
  { value: "pt-BR", label: "Brazilian Portuguese" },
  { value: "id", label: "Indonesian" },
  { value: "ro", label: "Romanian" },
  { value: "vi-VN", label: "Vietnamese" },
  { value: "tr-TR", label: "Turkish" },
];

export const languageByValue = new Map(languages.map((language) => [language.value, language]));

export function parseArgs(argv) {
  const args = {};

  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (!arg.startsWith("--")) continue;

    const [rawKey, inlineValue] = arg.slice(2).split("=", 2);
    const key = rawKey.replaceAll("-", "_");

    if (inlineValue !== undefined) {
      args[key] = inlineValue;
      continue;
    }

    const next = argv[i + 1];
    if (!next || next.startsWith("--")) {
      args[key] = true;
    } else {
      args[key] = next;
      i += 1;
    }
  }

  return args;
}

// Provider/base-url/api-key/model resolution. Both commands share the same LLM
// credentials, so they read the same I18N_TRANSLATION_* env vars (with explicit
// --flags taking precedence). `requireModel`/`requireApiKey` let a dry-run skip
// the hard requirement.
export function resolveProviderConfig(args, { requireModel = true, requireApiKey = true } = {}) {
  const provider = String(args.provider || process.env.I18N_TRANSLATION_PROVIDER || "openai").toLowerCase();
  const baseUrl =
    args.base_url ||
    process.env.I18N_TRANSLATION_BASE_URL ||
    (provider === "fireworks"
      ? "https://api.fireworks.ai/inference/v1/chat/completions"
      : "https://api.openai.com/v1/chat/completions");
  const apiKey =
    args.api_key ||
    process.env.I18N_TRANSLATION_API_KEY ||
    (provider === "fireworks" ? process.env.FIREWORKS_API_KEY : process.env.OPENAI_API_KEY);
  const model = args.model || process.env.I18N_TRANSLATION_MODEL;

  if (requireModel && !model) {
    throw new Error("Missing model. Pass --model or set I18N_TRANSLATION_MODEL.");
  }
  if (requireApiKey && !apiKey) {
    throw new Error(
      "Missing API key. Pass --api-key, set I18N_TRANSLATION_API_KEY, or set OPENAI_API_KEY/FIREWORKS_API_KEY."
    );
  }

  return { provider, baseUrl, apiKey, model };
}

export function resolveLanguages(value) {
  if (!value) {
    return languages.filter((language) => language.value !== fallbackLanguage);
  }

  const selected = String(value)
    .split(",")
    .map((language) => language.trim())
    .filter(Boolean);

  for (const language of selected) {
    if (!languageByValue.has(language)) {
      throw new Error(
        `Unsupported language "${language}". Supported: ${languages.map((item) => item.value).join(", ")}`
      );
    }
  }

  return selected
    .map((language) => languageByValue.get(language))
    .filter((language) => language.value !== fallbackLanguage);
}

export function readFileIfExists(filePath) {
  return fs.existsSync(filePath) ? fs.readFileSync(filePath, "utf8") : null;
}

function isStringLike(node) {
  return ts.isStringLiteral(node) || ts.isNoSubstitutionTemplateLiteral(node);
}

function unwrapExpression(expression) {
  let current = expression;
  while (ts.isAsExpression(current) || ts.isSatisfiesExpression?.(current) || ts.isParenthesizedExpression(current)) {
    current = current.expression;
  }
  return current;
}

function propertyNameToString(name) {
  if (ts.isIdentifier(name) || ts.isStringLiteral(name) || ts.isNumericLiteral(name)) {
    return name.text;
  }
  return null;
}

export function readObjectLiteral(filePath) {
  const sourceText = readFileIfExists(filePath);
  if (!sourceText) return {};

  const sourceFile = ts.createSourceFile(filePath, sourceText, ts.ScriptTarget.Latest, true, ts.ScriptKind.TS);

  for (const statement of sourceFile.statements) {
    if (!ts.isExportAssignment(statement)) continue;

    const expression = unwrapExpression(statement.expression);
    if (ts.isObjectLiteralExpression(expression)) {
      return objectLiteralToObject(expression);
    }
  }

  return {};
}

function objectLiteralToObject(objectLiteral) {
  const result = {};

  for (const property of objectLiteral.properties) {
    if (!ts.isPropertyAssignment(property)) continue;

    const key = propertyNameToString(property.name);
    if (!key) continue;

    const value = unwrapExpression(property.initializer);
    if (ts.isObjectLiteralExpression(value)) {
      result[key] = objectLiteralToObject(value);
    } else if (isStringLike(value)) {
      result[key] = value.text;
    }
  }

  return result;
}

export function collectEmptyEntries(object, result = []) {
  for (const [key, value] of Object.entries(object)) {
    if (value === "") {
      result.push(key);
    }
  }

  return result;
}

// Non-empty `value !== ""` entries: the existing translations that
// validate-translations reviews (empty placeholders are translate's job).
export function collectFilledEntries(object, result = []) {
  for (const [key, value] of Object.entries(object)) {
    if (typeof value === "string" && value.length > 0) {
      result.push(key);
    }
  }

  return result;
}

function formatFlatObject(object) {
  const entries = Object.entries(object);
  if (entries.length === 0) return "{}";

  return ["{", ...entries.map(([key, value]) => `  ${JSON.stringify(key)}: ${JSON.stringify(value)},`), "}"].join("\n");
}

export function localeFileContent(object) {
  return `/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

export default ${formatFlatObject(object)} as const;
`;
}

export function formatGeneratedFiles(filePaths) {
  const uniquePaths = Array.from(new Set(filePaths)).filter((filePath) => fs.existsSync(filePath));
  if (uniquePaths.length === 0) return;

  const relativePaths = uniquePaths.map((filePath) => path.relative(repoRoot, filePath));
  const result = spawnSync("pnpm", ["exec", "oxfmt", ...relativePaths], {
    cwd: repoRoot,
    stdio: "inherit",
  });

  if (result.error) {
    throw result.error;
  }

  if (result.status !== 0) {
    throw new Error("Failed to format generated i18n files with oxfmt.");
  }

  console.log(`i18n: formatted ${uniquePaths.length} generated files`);
}

export function chunkArray(items, size) {
  const chunks = [];
  for (let index = 0; index < items.length; index += size) {
    chunks.push(items.slice(index, index + size));
  }
  return chunks;
}

export async function requestChatCompletion(config, messages) {
  let delayMs = config.retryDelayMs;

  for (let attempt = 0; attempt <= config.retryCount; attempt += 1) {
    try {
      return await requestChatCompletionOnce(config, messages);
    } catch (error) {
      const hasAttemptsRemaining = attempt < config.retryCount;
      if (!hasAttemptsRemaining || !isRetryableError(error)) {
        throw error;
      }

      console.warn(`i18n: request failed (${formatErrorMessage(error)}), retrying ${attempt + 1}/${config.retryCount}`);
      await sleep(delayMs);
      delayMs *= 2;
    }
  }

  throw new Error("Chat completion request failed unexpectedly");
}

async function requestChatCompletionOnce(config, messages) {
  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), config.requestTimeoutMs);
  timeout.unref?.();

  const response = await fetch(config.baseUrl, {
    method: "POST",
    signal: controller.signal,
    headers: {
      Authorization: `Bearer ${config.apiKey}`,
      "Content-Type": "application/json",
    },
    body: JSON.stringify({
      model: config.model,
      temperature: 0,
      messages,
    }),
  }).finally(() => {
    clearTimeout(timeout);
  });

  const body = await response.text();
  if (!response.ok) {
    const error = new Error(`Chat completion request failed: HTTP ${response.status} ${body}`);
    error.status = response.status;
    error.retryable = response.status === 429 || response.status >= 500;
    throw error;
  }

  return JSON.parse(body);
}

function isRetryableError(error) {
  if (error?.retryable === true) return true;
  if (error?.name === "AbortError" || error?.name === "TimeoutError") return true;

  const code = error?.code || error?.cause?.code;
  if (typeof code === "string" && code.startsWith("UND_ERR_")) return true;

  return error instanceof TypeError && error.message === "fetch failed";
}

export function formatErrorMessage(error) {
  const causeCode = error?.cause?.code ? ` ${error.cause.code}` : "";
  return `${error?.message || String(error)}${causeCode}`.trim();
}

export function sleep(ms) {
  return new Promise((resolve) => {
    setTimeout(resolve, ms);
  });
}

// Pull the message content out of a chat-completion response.
export function chatContent(data, context) {
  const content = data.choices?.[0]?.message?.content;
  if (typeof content !== "string") {
    throw new Error(`Response for ${context} did not include choices[0].message.content`);
  }
  return content;
}

export function parseJsonContent(content) {
  const trimmed = content.trim();
  const fencedMatch = trimmed.match(/^```(?:json)?\s*([\s\S]*?)\s*```$/);
  const jsonText = fencedMatch ? fencedMatch[1] : trimmed;
  return JSON.parse(jsonText);
}

// True when `translation` carries the same ICU argument signature (names +
// node types) as the English `source` — i.e. no placeholder was dropped,
// added, or retyped. Also returns false when either side is not valid ICU.
export function hasMatchingIcuArguments(source, translation, locale) {
  let sourceArguments;
  let translationArguments;
  try {
    sourceArguments = getIcuArgumentSignature(source, fallbackLanguage);
    translationArguments = getIcuArgumentSignature(translation, locale);
  } catch {
    return false;
  }

  return JSON.stringify(sourceArguments) === JSON.stringify(translationArguments);
}

// True when `translation` parses as valid ICU MessageFormat for `locale`.
export function isValidIcu(translation, locale) {
  try {
    void new IntlMessageFormat(translation, locale);
    return true;
  } catch {
    return false;
  }
}

function getIcuArgumentSignature(message, locale) {
  const messageFormat = new IntlMessageFormat(message, locale);
  const argumentsByName = new Map();

  collectIcuArguments(messageFormat.getAst(), argumentsByName);

  return Array.from(argumentsByName.entries())
    .map(([name, types]) => [name, Array.from(types).toSorted()])
    .toSorted(([left], [right]) => left.localeCompare(right));
}

function collectIcuArguments(ast, result) {
  for (const node of ast) {
    if (typeof node.value === "string" && [1, 2, 3, 4, 5, 6].includes(node.type)) {
      const types = result.get(node.value) || new Set();
      types.add(node.type);
      result.set(node.value, types);
    }

    if (node.options) {
      for (const option of Object.values(node.options)) {
        collectIcuArguments(option.value, result);
      }
    }
  }
}
