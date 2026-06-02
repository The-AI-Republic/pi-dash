#!/usr/bin/env node
// oxlint-disable no-await-in-loop -- Translation calls are intentionally sequential to avoid provider rate limits.

import fs from "node:fs";
import path from "node:path";
import process from "node:process";
import { IntlMessageFormat } from "intl-messageformat";
import ts from "typescript";

const repoRoot = path.resolve(import.meta.dirname, "../../..");
const localesRoot = path.join(repoRoot, "packages/i18n/src/locales");
const targetLocaleFile = "translations.ts";
const fallbackLanguage = "en";
const defaultBatchSize = 10;
const defaultRequestTimeoutMs = 180000;
const defaultRetryCount = 2;
const defaultRetryDelayMs = 5000;
const readmeTranslationTargets = new Map([
  ["es", path.join(repoRoot, "packages/i18n/README.es.md")],
  ["zh-CN", path.join(repoRoot, "packages/i18n/README.zh-CN.md")],
]);
const readmeSourcePath = path.join(repoRoot, "packages/i18n/README.md");

const languages = [
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

const languageByValue = new Map(languages.map((language) => [language.value, language]));

function parseArgs(argv) {
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

function configFromArgs() {
  const args = parseArgs(process.argv.slice(2));

  if (args.help === true) {
    console.log(usage());
    process.exit(0);
  }

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
  const requestedLanguages = args.languages || args.language || process.env.I18N_TRANSLATION_LANGUAGES;
  const dryRun = args.dry_run === true || process.env.I18N_TRANSLATION_DRY_RUN === "1";
  const limit = Number(args.limit || process.env.I18N_TRANSLATION_LIMIT || 0);
  const batchSize = Number(args.batch_size || process.env.I18N_TRANSLATION_BATCH_SIZE || defaultBatchSize);
  const requestTimeoutMs = Number(
    args.request_timeout_ms || process.env.I18N_TRANSLATION_REQUEST_TIMEOUT_MS || defaultRequestTimeoutMs
  );
  const retryCount = Number(args.retry_count || process.env.I18N_TRANSLATION_RETRY_COUNT || defaultRetryCount);
  const retryDelayMs = Number(
    args.retry_delay_ms || process.env.I18N_TRANSLATION_RETRY_DELAY_MS || defaultRetryDelayMs
  );
  const skipReadme = args.skip_readme === true || process.env.I18N_TRANSLATION_SKIP_README === "1";
  const continueOnError = args.continue_on_error === true || process.env.I18N_TRANSLATION_CONTINUE_ON_ERROR === "1";

  if (!model && !dryRun) {
    throw new Error("Missing model. Pass --model or set I18N_TRANSLATION_MODEL.");
  }

  if (!apiKey && !dryRun) {
    throw new Error(
      "Missing API key. Pass --api-key, set I18N_TRANSLATION_API_KEY, or set OPENAI_API_KEY/FIREWORKS_API_KEY."
    );
  }

  return {
    provider,
    baseUrl,
    apiKey,
    model,
    languages: resolveLanguages(requestedLanguages),
    dryRun,
    limit: Number.isFinite(limit) && limit > 0 ? limit : 0,
    batchSize: Number.isFinite(batchSize) && batchSize > 0 ? batchSize : defaultBatchSize,
    requestTimeoutMs:
      Number.isFinite(requestTimeoutMs) && requestTimeoutMs > 0 ? requestTimeoutMs : defaultRequestTimeoutMs,
    retryCount: Number.isFinite(retryCount) && retryCount >= 0 ? retryCount : defaultRetryCount,
    retryDelayMs: Number.isFinite(retryDelayMs) && retryDelayMs >= 0 ? retryDelayMs : defaultRetryDelayMs,
    skipReadme,
    continueOnError,
  };
}

function usage() {
  return [
    "Usage:",
    "  pnpm i18n:translate -- --provider openai --model <model> --api-key <key>",
    "  pnpm i18n:translate -- --provider fireworks --model <model> --api-key <key>",
    "",
    "Options:",
    "  --provider openai|fireworks",
    "  --api-key <key>",
    "  --model <model>",
    "  --base-url <openai-compatible-chat-completions-url>",
    "  --languages fr,es,ja",
    "  --limit 100",
    "  --batch-size 10",
    "  --request-timeout-ms 180000",
    "  --retry-count 2",
    "  --retry-delay-ms 5000",
    "  --continue-on-error",
    "  --dry-run",
    "  --skip-readme",
  ].join("\n");
}

function resolveLanguages(value) {
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

function readFileIfExists(filePath) {
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

function readObjectLiteral(filePath) {
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

function collectEmptyEntries(object, result = []) {
  for (const [key, value] of Object.entries(object)) {
    if (value === "") {
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

function localeFileContent(object) {
  return `/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

export default ${formatFlatObject(object)} as const;
`;
}

function chunkArray(items, size) {
  const chunks = [];
  for (let index = 0; index < items.length; index += size) {
    chunks.push(items.slice(index, index + size));
  }
  return chunks;
}

function buildMessages(language, items) {
  return [
    {
      role: "system",
      content: [
        "You are a product UI localization translator.",
        "Translate from English into the requested target language.",
        "Preserve ICU MessageFormat placeholders and plural/select syntax exactly.",
        "Preserve product names, code identifiers, markdown links, HTML tags, and keyboard shortcuts unless natural localization requires surrounding words to change.",
        "Return only valid JSON. The JSON object must map each input id to its translated string.",
      ].join(" "),
    },
    {
      role: "user",
      content: JSON.stringify(
        {
          target_language: language.label,
          target_locale: language.value,
          items: items.map((item) => ({ id: item.id, source: item.source })),
        },
        null,
        2
      ),
    },
  ];
}

async function requestTranslations(config, language, items) {
  const data = await requestChatCompletion(config, buildMessages(language, items));
  const content = data.choices?.[0]?.message?.content;
  if (typeof content !== "string") {
    throw new Error(`Translation response for ${language.value} did not include choices[0].message.content`);
  }

  return parseJsonContent(content);
}

async function requestChatCompletion(config, messages) {
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

  throw new Error("Translation request failed unexpectedly");
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
    const error = new Error(`Translation request failed: HTTP ${response.status} ${body}`);
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

function formatErrorMessage(error) {
  const causeCode = error?.cause?.code ? ` ${error.cause.code}` : "";
  return `${error?.message || String(error)}${causeCode}`.trim();
}

function sleep(ms) {
  return new Promise((resolve) => {
    setTimeout(resolve, ms);
  });
}

function parseJsonContent(content) {
  const trimmed = content.trim();
  const fencedMatch = trimmed.match(/^```(?:json)?\s*([\s\S]*?)\s*```$/);
  const jsonText = fencedMatch ? fencedMatch[1] : trimmed;
  return JSON.parse(jsonText);
}

function validateTranslations(items, translations, language) {
  const valid = {};

  for (const item of items) {
    const translation = translations[item.id];
    if (typeof translation !== "string") {
      console.warn(`i18n: ${language.value} missing translation in response for ${item.key}`);
      continue;
    }
    if (translation.length === 0) {
      console.warn(`i18n: ${language.value} returned an empty translation for ${item.key}`);
      continue;
    }
    try {
      const messageFormat = new IntlMessageFormat(translation, language.value);
      void messageFormat;
    } catch (error) {
      console.warn(`i18n: ${language.value} returned invalid ICU for ${item.key}: ${error.message}`);
      continue;
    }
    if (!hasMatchingIcuArguments(item.source, translation, language.value)) {
      console.warn(`i18n: ${language.value} returned ICU argument mismatch for ${item.key}`);
      continue;
    }
    valid[item.key] = translation;
  }

  return valid;
}

function hasMatchingIcuArguments(source, translation, locale) {
  const sourceArguments = getIcuArgumentSignature(source, fallbackLanguage);
  const translationArguments = getIcuArgumentSignature(translation, locale);

  return JSON.stringify(sourceArguments) === JSON.stringify(translationArguments);
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

async function translateLanguage(config, language) {
  const languageDir = path.join(localesRoot, language.value);
  const targetPath = path.join(languageDir, targetLocaleFile);
  const targetTranslations = readObjectLiteral(targetPath);
  const missingKeys = collectEmptyEntries(targetTranslations);
  const items = missingKeys
    .map((key, index) => ({ id: `message_${index + 1}`, key, source: key }))
    .filter((item) => item.source.length > 0)
    .slice(0, config.limit || undefined);

  if (items.length === 0) {
    console.log(`i18n: ${language.value} has no translatable empty placeholders`);
    return;
  }

  if (config.dryRun) {
    console.log(`i18n: ${language.value} would translate ${items.length} placeholders`);
    return;
  }

  let translatedCount = 0;
  const batches = chunkArray(items, config.batchSize);
  console.log(
    `i18n: ${language.value} translating ${items.length} placeholders in ${batches.length} batches of up to ${config.batchSize}`
  );

  for (const [batchIndex, batch] of batches.entries()) {
    const batchNumber = batchIndex + 1;
    console.log(`i18n: ${language.value} batch ${batchNumber}/${batches.length} requesting ${batch.length} messages`);

    let validTranslations;
    try {
      const translations = await requestTranslations(config, language, batch);
      validTranslations = validateTranslations(batch, translations, language);
    } catch (error) {
      if (!config.continueOnError) {
        throw error;
      }

      console.error(
        `i18n: ${language.value} batch ${batchNumber}/${batches.length} failed: ${formatErrorMessage(error)}`
      );
      continue;
    }

    for (const [key, translation] of Object.entries(validTranslations)) {
      targetTranslations[key] = translation;
      translatedCount += 1;
    }

    const batchTranslatedCount = Object.keys(validTranslations).length;
    if (batchTranslatedCount > 0) {
      fs.writeFileSync(targetPath, localeFileContent(targetTranslations));
    }

    console.log(
      `i18n: ${language.value} batch ${batchNumber}/${batches.length} translated ${batchTranslatedCount}/${batch.length}; ${translatedCount}/${items.length} total`
    );
  }

  console.log(`i18n: ${language.value} translated ${translatedCount}/${items.length} placeholders`);
}

function buildReadmeMessages(language, sourceMarkdown, existingTranslation) {
  return [
    {
      role: "system",
      content: [
        "You are a technical documentation translator.",
        "Translate the English Markdown source into the requested target language.",
        "If an existing translation is provided, improve it against the English source instead of translating blindly.",
        "Preserve Markdown structure, headings, fenced code blocks, inline code, command examples, environment variable names, file paths, package names, and URLs.",
        "Do not translate code blocks or shell commands.",
        "Return only the translated Markdown, with no explanation and no surrounding code fence.",
      ].join(" "),
    },
    {
      role: "user",
      content: [
        `Target locale: ${language.value}`,
        `Target language: ${language.label}`,
        "",
        "English source Markdown:",
        sourceMarkdown,
        "",
        "Existing target Markdown, if any:",
        existingTranslation || "(none)",
      ].join("\n"),
    },
  ];
}

async function translateReadme(config, language) {
  const targetPath = readmeTranslationTargets.get(language.value);
  if (!targetPath) return;

  const sourceMarkdown = fs.readFileSync(readmeSourcePath, "utf8");
  const existingTranslation = readFileIfExists(targetPath) || "";

  if (config.dryRun) {
    console.log(`i18n: ${language.value} would translate packages/i18n/README.md`);
    return;
  }

  const data = await requestChatCompletion(config, buildReadmeMessages(language, sourceMarkdown, existingTranslation));
  const translatedMarkdown = data.choices?.[0]?.message?.content;
  if (typeof translatedMarkdown !== "string" || translatedMarkdown.trim().length === 0) {
    throw new Error(`README translation response for ${language.value} did not include markdown content`);
  }

  fs.writeFileSync(targetPath, `${translatedMarkdown.trim()}\n`);
  console.log(`i18n: ${language.value} updated ${path.relative(repoRoot, targetPath)}`);
}

async function main() {
  const config = configFromArgs();

  console.log(
    `i18n: provider=${config.provider} model=${config.model || "(dry-run)"} languages=${config.languages
      .map((language) => language.value)
      .join(",")} batch_size=${config.batchSize}${config.limit ? ` limit=${config.limit}` : ""}${
      config.dryRun ? " dry_run=true" : ""
    }`
  );

  for (const language of config.languages) {
    await translateLanguage(config, language);
  }

  if (!config.skipReadme) {
    const readmeFailures = [];

    for (const language of config.languages) {
      try {
        await translateReadme(config, language);
      } catch (error) {
        if (readmeTranslationTargets.has(language.value)) {
          readmeFailures.push(language.value);
          console.error(`i18n: ${language.value} README translation failed: ${formatErrorMessage(error)}`);
        } else {
          throw error;
        }
      }
    }

    if (readmeFailures.length > 0) {
      throw new Error(`README translation failed for: ${readmeFailures.join(", ")}`);
    }
  }
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
