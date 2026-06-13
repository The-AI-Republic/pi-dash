#!/usr/bin/env node
// oxlint-disable no-await-in-loop -- Translation calls are intentionally sequential to avoid provider rate limits.

import fs from "node:fs";
import path from "node:path";
import process from "node:process";

import {
  chatContent,
  chunkArray,
  collectEmptyEntries,
  defaultBatchSize,
  defaultRequestTimeoutMs,
  defaultRetryCount,
  defaultRetryDelayMs,
  formatErrorMessage,
  formatGeneratedFiles,
  hasMatchingIcuArguments,
  isValidIcu,
  localeFileContent,
  localesRoot,
  parseArgs,
  parseJsonContent,
  readFileIfExists,
  readObjectLiteral,
  repoRoot,
  resolveLanguages,
  resolveProviderConfig,
  requestChatCompletion,
  targetLocaleFile,
} from "./lib/shared.mjs";
import { groupIssuesByLocale, normalizeIssues, pendingEvaluations, resolveCorrection } from "./lib/report.mjs";

// The i18n package README is developer-facing and intentionally English-only;
// no per-locale translations are generated for it.
const readmeTranslationTargets = new Map();
const readmeSourcePath = path.join(repoRoot, "packages/i18n/README.md");

function configFromArgs() {
  const args = parseArgs(process.argv.slice(2));

  if (args.help === true) {
    console.log(usage());
    process.exit(0);
  }

  const dryRun = args.dry_run === true || process.env.I18N_TRANSLATION_DRY_RUN === "1";
  const { provider, baseUrl, apiKey, model } = resolveProviderConfig(args, {
    requireModel: !dryRun,
    requireApiKey: !dryRun,
  });

  const requestedLanguages = args.languages || args.language || process.env.I18N_TRANSLATION_LANGUAGES;
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
  // Optional: a report produced by `pnpm i18n:validate`. When present, translate
  // also re-evaluates each flagged existing translation and applies the fix if
  // the LLM confirms it. `--no-validation-report` / skip-empty opt-outs below.
  const validationReport =
    args.validation_report === true
      ? "packages/i18n/i18n-validation-report.json"
      : args.validation_report || process.env.I18N_VALIDATION_REPORT || "";
  const skipEmpty = args.skip_empty === true || process.env.I18N_TRANSLATION_SKIP_EMPTY === "1";

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
    validationReport,
    skipEmpty,
  };
}

function usage() {
  return [
    "Usage:",
    "  pnpm i18n:translate -- --provider openai --model <model> --api-key <key>",
    "  pnpm i18n:translate -- --provider fireworks --model <model> --api-key <key>",
    "",
    "Fills empty translation placeholders. With --validation-report it also",
    "re-evaluates and applies fixes flagged by `pnpm i18n:validate`.",
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
    "  --validation-report [path]   consume an i18n:validate report (default path if no value)",
    "  --skip-empty                 skip empty-placeholder filling (apply validation report only)",
    "  --dry-run",
    "  --skip-readme",
  ].join("\n");
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
  return parseJsonContent(chatContent(data, language.value));
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
    if (!isValidIcu(translation, language.value)) {
      console.warn(`i18n: ${language.value} returned invalid ICU for ${item.key}`);
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

async function translateLanguage(config, language, writtenFiles) {
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
      writtenFiles.add(targetPath);
    }

    console.log(
      `i18n: ${language.value} batch ${batchNumber}/${batches.length} translated ${batchTranslatedCount}/${batch.length}; ${translatedCount}/${items.length} total`
    );
  }

  console.log(`i18n: ${language.value} translated ${translatedCount}/${items.length} placeholders`);
}

// ---------------------------------------------------------------------------
// Validation-report consumption
//
// A report from `pnpm i18n:validate` lists existing translations a reviewer LLM
// flagged as wrong, each with a `suggestion`. We do NOT trust the suggestion
// blindly: for every flagged entry we ask the LLM to independently judge whether
// the current value is actually wrong and, if so, return the best correction.
// Only an explicit "incorrect" verdict with a placeholder-safe replacement is
// written back. Anything else leaves the existing translation untouched.
// ---------------------------------------------------------------------------

function loadValidationReport(reportPath) {
  const absolute = path.isAbsolute(reportPath) ? reportPath : path.join(repoRoot, reportPath);
  const raw = readFileIfExists(absolute);
  if (raw === null) {
    throw new Error(`Validation report not found: ${reportPath}`);
  }

  let parsed;
  try {
    parsed = JSON.parse(raw);
  } catch (error) {
    throw new Error(`Validation report is not valid JSON (${reportPath}): ${formatErrorMessage(error)}`, {
      cause: error,
    });
  }

  if (!Array.isArray(parsed) && !Array.isArray(parsed?.issues)) {
    throw new Error(`Validation report must be an array of issues or { issues: [...] } (${reportPath})`);
  }

  return groupIssuesByLocale(normalizeIssues(parsed));
}

function buildEvaluationMessages(language, items) {
  return [
    {
      role: "system",
      content: [
        "You are a senior localization reviewer making a final accept/reject decision.",
        "For each item you are given the English source, the CURRENT target-language translation, and a PROPOSED replacement with a reason.",
        "Judge independently. Do NOT accept the proposal just because it exists.",
        "Mark verdict 'incorrect' ONLY when the current translation is genuinely wrong: wrong meaning, mistranslation, untranslated English left in, wrong language, or broken/missing ICU placeholders.",
        "If the current translation is acceptable and conveys the right meaning, mark verdict 'acceptable' even if the proposal is slightly more elegant — do not chase marginal stylistic improvements.",
        "When verdict is 'incorrect', return your own best correction in 'final' (you may adopt or improve the proposal). Preserve ICU MessageFormat placeholders and plural/select syntax exactly.",
        "Return only valid JSON: an object mapping each input id to { verdict: 'incorrect' | 'acceptable', final: string }.",
      ].join(" "),
    },
    {
      role: "user",
      content: JSON.stringify(
        {
          target_language: language.label,
          target_locale: language.value,
          items: items.map((item) => ({
            id: item.id,
            source: item.source,
            current: item.current,
            proposed: item.suggestion,
            reason: item.reason,
          })),
        },
        null,
        2
      ),
    },
  ];
}

function warnSkippedDriftedIssue(issue) {
  console.warn(`i18n: ${issue.locale} ${JSON.stringify(issue.key)} changed since validation; skipping`);
}

async function applyValidationForLanguage(config, language, issues, writtenFiles) {
  const targetPath = path.join(localesRoot, language.value, targetLocaleFile);
  const targetTranslations = readObjectLiteral(targetPath);
  const items = pendingEvaluations(issues, targetTranslations, warnSkippedDriftedIssue).slice(
    0,
    config.limit || undefined
  );

  if (items.length === 0) {
    console.log(`i18n: ${language.value} has no applicable validation entries`);
    return 0;
  }

  if (config.dryRun) {
    console.log(`i18n: ${language.value} would re-evaluate ${items.length} flagged translations`);
    return 0;
  }

  let appliedCount = 0;
  const batches = chunkArray(items, config.batchSize);
  console.log(`i18n: ${language.value} evaluating ${items.length} flagged translations in ${batches.length} batches`);

  for (const [batchIndex, batch] of batches.entries()) {
    const batchNumber = batchIndex + 1;

    let verdicts;
    try {
      const data = await requestChatCompletion(config, buildEvaluationMessages(language, batch));
      verdicts = parseJsonContent(chatContent(data, language.value));
    } catch (error) {
      if (!config.continueOnError) {
        throw error;
      }
      console.error(
        `i18n: ${language.value} evaluation batch ${batchNumber}/${batches.length} failed: ${formatErrorMessage(error)}`
      );
      continue;
    }

    let batchApplied = 0;
    for (const item of batch) {
      const final = resolveCorrection(verdicts?.[item.id], item, language.value);
      if (final === null) continue;

      targetTranslations[item.key] = final;
      appliedCount += 1;
      batchApplied += 1;
    }

    if (batchApplied > 0) {
      fs.writeFileSync(targetPath, localeFileContent(targetTranslations));
      writtenFiles.add(targetPath);
    }

    console.log(
      `i18n: ${language.value} evaluation batch ${batchNumber}/${batches.length} applied ${batchApplied}/${batch.length}`
    );
  }

  console.log(`i18n: ${language.value} applied ${appliedCount}/${items.length} validation corrections`);
  return appliedCount;
}

async function applyValidationReport(config, writtenFiles) {
  const byLocale = loadValidationReport(config.validationReport);
  const totalIssues = Array.from(byLocale.values()).reduce((sum, issues) => sum + issues.length, 0);
  if (totalIssues === 0) {
    console.log(`i18n: validation report ${config.validationReport} has no actionable issues`);
    return;
  }

  console.log(`i18n: applying validation report ${config.validationReport} (${totalIssues} flagged across locales)`);

  let appliedTotal = 0;
  for (const language of config.languages) {
    const issues = byLocale.get(language.value);
    if (!issues || issues.length === 0) continue;
    appliedTotal += await applyValidationForLanguage(config, language, issues, writtenFiles);
  }

  console.log(`i18n: validation report applied ${appliedTotal} corrections total`);
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

async function translateReadme(config, language, writtenFiles) {
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
  writtenFiles.add(targetPath);
  console.log(`i18n: ${language.value} updated ${path.relative(repoRoot, targetPath)}`);
}

async function main() {
  const config = configFromArgs();
  const writtenFiles = new Set();

  console.log(
    `i18n: provider=${config.provider} model=${config.model || "(dry-run)"} languages=${config.languages
      .map((language) => language.value)
      .join(",")} batch_size=${config.batchSize}${config.limit ? ` limit=${config.limit}` : ""}${
      config.dryRun ? " dry_run=true" : ""
    }${config.validationReport ? ` validation_report=${config.validationReport}` : ""}`
  );

  try {
    if (!config.skipEmpty) {
      for (const language of config.languages) {
        await translateLanguage(config, language, writtenFiles);
      }
    }

    if (config.validationReport) {
      await applyValidationReport(config, writtenFiles);
    }

    if (!config.skipReadme) {
      const readmeFailures = [];

      for (const language of config.languages) {
        try {
          await translateReadme(config, language, writtenFiles);
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
  } catch (error) {
    if (!config.dryRun) {
      try {
        formatGeneratedFiles(writtenFiles);
      } catch (formatError) {
        console.error(`i18n: generated file formatting failed: ${formatErrorMessage(formatError)}`);
      }
    }

    throw error;
  }

  if (!config.dryRun) {
    formatGeneratedFiles(writtenFiles);
  }
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
