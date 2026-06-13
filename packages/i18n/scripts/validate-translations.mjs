#!/usr/bin/env node
// oxlint-disable no-await-in-loop -- Review calls are intentionally sequential to avoid provider rate limits.
//
// LLM-backed QA pass over EXISTING translations. It never edits locale files:
// it reads every non-empty translation, asks a reviewer model to flag only the
// genuinely WRONG ones (not "could be nicer"), and writes a JSON report that
// `pnpm i18n:translate --validation-report <path>` consumes to apply fixes.
//
// Empty placeholders are ignored here — filling those is `i18n:translate`'s job.

import fs from "node:fs";
import path from "node:path";
import process from "node:process";

import {
  chatContent,
  chunkArray,
  collectFilledEntries,
  defaultBatchSize,
  defaultRequestTimeoutMs,
  defaultRetryCount,
  defaultRetryDelayMs,
  formatErrorMessage,
  localesRoot,
  parseArgs,
  parseJsonContent,
  readObjectLiteral,
  repoRoot,
  resolveLanguages,
  resolveProviderConfig,
  requestChatCompletion,
  targetLocaleFile,
} from "./lib/shared.mjs";
import { buildReportEntry, normalizeIssues } from "./lib/report.mjs";

const defaultReportPath = "packages/i18n/i18n-validation-report.json";

function configFromArgs() {
  const args = parseArgs(process.argv.slice(2));

  if (args.help === true) {
    console.log(usage());
    process.exit(0);
  }

  const dryRun = args.dry_run === true || process.env.I18N_VALIDATION_DRY_RUN === "1";
  const { provider, baseUrl, apiKey, model } = resolveProviderConfig(args, {
    requireModel: !dryRun,
    requireApiKey: !dryRun,
  });

  const requestedLanguages = args.languages || args.language || process.env.I18N_VALIDATION_LANGUAGES;
  const limit = Number(args.limit || process.env.I18N_VALIDATION_LIMIT || 0);
  const batchSize = Number(args.batch_size || process.env.I18N_VALIDATION_BATCH_SIZE || defaultBatchSize);
  const requestTimeoutMs = Number(
    args.request_timeout_ms || process.env.I18N_VALIDATION_REQUEST_TIMEOUT_MS || defaultRequestTimeoutMs
  );
  const retryCount = Number(args.retry_count || process.env.I18N_VALIDATION_RETRY_COUNT || defaultRetryCount);
  const retryDelayMs = Number(args.retry_delay_ms || process.env.I18N_VALIDATION_RETRY_DELAY_MS || defaultRetryDelayMs);
  const continueOnError = args.continue_on_error === true || process.env.I18N_VALIDATION_CONTINUE_ON_ERROR === "1";
  const out = args.out || process.env.I18N_VALIDATION_OUT || defaultReportPath;

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
    continueOnError,
    out,
  };
}

function usage() {
  return [
    "Usage:",
    "  pnpm i18n:validate -- --provider openai --model <model> --api-key <key>",
    "  pnpm i18n:validate -- --provider fireworks --model <model> --api-key <key>",
    "",
    "Reviews EXISTING translations and writes a JSON report of genuinely wrong",
    "ones. It does not modify locale files. Feed the report to:",
    "  pnpm i18n:translate -- --validation-report <out> ...",
    "",
    "Options:",
    "  --provider openai|fireworks",
    "  --api-key <key>",
    "  --model <model>",
    "  --base-url <openai-compatible-chat-completions-url>",
    "  --languages fr,es,ja",
    "  --limit 100               cap reviewed entries per locale",
    "  --batch-size 10",
    "  --request-timeout-ms 180000",
    "  --retry-count 2",
    "  --retry-delay-ms 5000",
    "  --continue-on-error",
    `  --out <path>              report path (default ${defaultReportPath})`,
    "  --dry-run",
  ].join("\n");
}

function buildReviewMessages(language, items) {
  return [
    {
      role: "system",
      content: [
        "You are a meticulous localization QA reviewer.",
        `You review existing ${language.label} (${language.value}) translations of an English product UI.`,
        "For each item you get the English source and the current target-language translation.",
        "Report ONLY translations that are genuinely WRONG, meaning one of:",
        "the meaning is incorrect or reversed; it is a clear mistranslation; English was left untranslated where it should be localized; it is in the wrong language; or an ICU MessageFormat placeholder/plural/select is broken, missing, or altered.",
        "Do NOT report translations that are correct but could be phrased more elegantly. Do NOT report stylistic or tone preferences. When in doubt, do NOT report it — only flag real errors.",
        "For each genuinely wrong translation, provide a concise reason and a corrected suggestion that preserves ICU placeholders and plural/select syntax exactly.",
        "Return only valid JSON: an array of objects { id, reason, suggestion }. Return an empty array if every translation is acceptable.",
      ].join(" "),
    },
    {
      role: "user",
      content: JSON.stringify(
        {
          target_language: language.label,
          target_locale: language.value,
          items: items.map((item) => ({ id: item.id, source: item.source, current: item.current })),
        },
        null,
        2
      ),
    },
  ];
}

async function reviewLanguage(config, language) {
  const targetPath = path.join(localesRoot, language.value, targetLocaleFile);
  const targetTranslations = readObjectLiteral(targetPath);
  const filledKeys = collectFilledEntries(targetTranslations);
  const items = filledKeys
    .filter((key) => key.length > 0)
    .map((key, index) => ({ id: `entry_${index + 1}`, key, source: key, current: targetTranslations[key] }))
    .slice(0, config.limit || undefined);

  if (items.length === 0) {
    console.log(`i18n: ${language.value} has no existing translations to review`);
    return [];
  }

  if (config.dryRun) {
    console.log(`i18n: ${language.value} would review ${items.length} existing translations`);
    return [];
  }

  const byKey = new Map(items.map((item) => [item.id, item]));
  const issues = [];
  const batches = chunkArray(items, config.batchSize);
  console.log(`i18n: ${language.value} reviewing ${items.length} translations in ${batches.length} batches`);

  for (const [batchIndex, batch] of batches.entries()) {
    const batchNumber = batchIndex + 1;

    let response;
    try {
      const data = await requestChatCompletion(config, buildReviewMessages(language, batch));
      response = parseJsonContent(chatContent(data, language.value));
    } catch (error) {
      if (!config.continueOnError) {
        throw error;
      }
      console.error(
        `i18n: ${language.value} review batch ${batchNumber}/${batches.length} failed: ${formatErrorMessage(error)}`
      );
      continue;
    }

    let batchFlagged = 0;
    for (const raw of normalizeIssues(response)) {
      const item = raw && byKey.get(raw.id);
      if (!item) continue;

      // buildReportEntry keeps a suggestion only when it is a real,
      // placeholder-safe change; otherwise it blanks it so the downstream
      // apply pass treats it as "no proposal".
      issues.push(buildReportEntry(language.value, item, raw));
      batchFlagged += 1;
    }

    console.log(
      `i18n: ${language.value} review batch ${batchNumber}/${batches.length} flagged ${batchFlagged}/${batch.length}`
    );
  }

  console.log(`i18n: ${language.value} flagged ${issues.length}/${items.length} translations`);
  return issues;
}

function writeReport(config, issues) {
  const absolute = path.isAbsolute(config.out) ? config.out : path.join(repoRoot, config.out);
  fs.mkdirSync(path.dirname(absolute), { recursive: true });

  const report = {
    generatedBy: "i18n:validate",
    model: config.model,
    languages: config.languages.map((language) => language.value),
    issueCount: issues.length,
    issues,
  };

  fs.writeFileSync(absolute, `${JSON.stringify(report, null, 2)}\n`);
  console.log(`i18n: wrote ${issues.length} issues to ${path.relative(repoRoot, absolute)}`);
}

async function main() {
  const config = configFromArgs();

  console.log(
    `i18n: validate provider=${config.provider} model=${config.model || "(dry-run)"} languages=${config.languages
      .map((language) => language.value)
      .join(",")} batch_size=${config.batchSize}${config.limit ? ` limit=${config.limit}` : ""}${
      config.dryRun ? " dry_run=true" : ""
    }`
  );

  const allIssues = [];
  for (const language of config.languages) {
    const issues = await reviewLanguage(config, language);
    allIssues.push(...issues);
  }

  if (config.dryRun) {
    console.log("i18n: dry run — no report written");
    return;
  }

  writeReport(config, allIssues);

  if (allIssues.length === 0) {
    console.log("i18n: no wrong translations found; report has an empty issue list");
  } else {
    console.log(
      `i18n: next step — apply with: pnpm i18n:translate -- --validation-report ${config.out} --skip-empty --provider ${config.provider} --model <model> --api-key <key>`
    );
  }
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
