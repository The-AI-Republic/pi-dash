#!/usr/bin/env node

import fs from "node:fs";
import path from "node:path";
import ts from "typescript";

const repoRoot = path.resolve(import.meta.dirname, "../../..");
const localesRoot = path.join(repoRoot, "packages/i18n/src/locales");
const sourceRoots = ["apps", "packages"].map((root) => path.join(repoRoot, root));
const targetLocaleFile = "translations.ts";
const fallbackLanguage = "en";
const auxiliaryLocaleFiles = ["core", "accessibility", "editor", "empty-state"];
const languages = [
  "en",
  "fr",
  "es",
  "ja",
  "zh-CN",
  "zh-TW",
  "ru",
  "it",
  "cs",
  "sk",
  "de",
  "ua",
  "pl",
  "ko",
  "pt-BR",
  "id",
  "ro",
  "vi-VN",
  "tr-TR",
];

const ignoredDirs = new Set([
  ".git",
  ".next",
  ".react-router",
  ".turbo",
  "build",
  "coverage",
  "dist",
  "node_modules",
  "storybook-static",
]);
const sourceExtensions = new Set([".ts", ".tsx"]);
const translatablePropertyNames = new Set([
  "i18n_description",
  "i18n_indicator",
  "i18n_key",
  "i18nKey",
  "i18n_label",
  "i18n_message",
  "i18n_name",
  "i18n_placeholder",
  "i18n_title",
  "labelTranslationKey",
  "titleTranslationKey",
  "tooltipTranslationKey",
  "translationKey",
]);

function readFileIfExists(filePath) {
  return fs.existsSync(filePath) ? fs.readFileSync(filePath, "utf8") : null;
}

function walkFiles(dir, files = []) {
  if (!fs.existsSync(dir)) return files;

  const entries = fs
    .readdirSync(dir, { withFileTypes: true })
    .toSorted((left, right) => left.name.localeCompare(right.name));

  for (const entry of entries) {
    if (entry.isDirectory()) {
      if (!ignoredDirs.has(entry.name)) {
        walkFiles(path.join(dir, entry.name), files);
      }
      continue;
    }

    if (!entry.isFile()) continue;

    const filePath = path.join(dir, entry.name);
    if (!sourceExtensions.has(path.extname(filePath))) continue;
    if (filePath.startsWith(localesRoot)) continue;

    files.push(filePath);
  }

  return files;
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

function collectStringLikeExpressions(node, addMessage) {
  const current = unwrapExpression(node);

  if (isStringLike(current)) {
    addMessage(current.text, current);
    return;
  }

  if (ts.isPropertyAssignment(current)) {
    collectStringLikeExpressions(current.initializer, addMessage);
    return;
  }

  if (ts.isObjectLiteralExpression(current)) {
    for (const property of current.properties) {
      if (ts.isPropertyAssignment(property)) {
        collectStringLikeExpressions(property.initializer, addMessage);
      }
    }
    return;
  }

  ts.forEachChild(current, (child) => collectStringLikeExpressions(child, addMessage));
}

function isTranslatableVariableName(name) {
  return /I18N|I18n|i18n/.test(name);
}

function isTranslatablePropertyName(name) {
  return name.startsWith("i18n_") || translatablePropertyNames.has(name);
}

function collectUsedMessages() {
  const messages = new Map();
  const files = sourceRoots.flatMap((root) => walkFiles(root));

  function addMessage(sourceFile, filePath, message, node) {
    if (typeof message !== "string" || message.length === 0) return;

    const location = sourceFile.getLineAndCharacterOfPosition(node.getStart(sourceFile));
    const references = messages.get(message) || [];
    references.push(`${path.relative(repoRoot, filePath)}:${location.line + 1}:${location.character + 1}`);
    messages.set(message, references);
  }

  for (const filePath of files) {
    const sourceText = fs.readFileSync(filePath, "utf8");
    const sourceFile = ts.createSourceFile(filePath, sourceText, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);

    function visit(node) {
      if (
        ts.isCallExpression(node) &&
        ts.isIdentifier(node.expression) &&
        node.expression.text === "t" &&
        node.arguments.length > 0
      ) {
        collectStringLikeExpressions(node.arguments[0], (message, messageNode) =>
          addMessage(sourceFile, filePath, message, messageNode)
        );
      }

      if (ts.isPropertyAssignment(node)) {
        const propertyName = propertyNameToString(node.name);
        if (propertyName && isTranslatablePropertyName(propertyName)) {
          collectStringLikeExpressions(node.initializer, (message, messageNode) =>
            addMessage(sourceFile, filePath, message, messageNode)
          );
        }
      }

      if (
        ts.isVariableDeclaration(node) &&
        ts.isIdentifier(node.name) &&
        isTranslatableVariableName(node.name.text) &&
        node.initializer
      ) {
        collectStringLikeExpressions(node.initializer, (message, messageNode) =>
          addMessage(sourceFile, filePath, message, messageNode)
        );
      }

      if (ts.isFunctionDeclaration(node) && node.name && isTranslatableVariableName(node.name.text)) {
        collectStringLikeExpressions(node, (message, messageNode) =>
          addMessage(sourceFile, filePath, message, messageNode)
        );
      }

      ts.forEachChild(node, visit);
    }

    visit(sourceFile);
  }

  return messages;
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

function flattenObject(object, prefix = "", result = {}) {
  for (const [key, value] of Object.entries(object)) {
    const keyPath = prefix ? `${prefix}.${key}` : key;
    if (typeof value === "string") {
      result[keyPath] = value;
    } else if (value && typeof value === "object" && !Array.isArray(value)) {
      flattenObject(value, keyPath, result);
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

function syncAuxiliaryLocaleFiles(languageDir) {
  for (const file of auxiliaryLocaleFiles) {
    const filePath = path.join(languageDir, `${file}.ts`);
    if (fs.existsSync(filePath)) {
      fs.writeFileSync(filePath, localeFileContent({}));
    }
  }
}

function main() {
  const usedMessageReferences = collectUsedMessages();
  const usedMessages = Array.from(usedMessageReferences.keys()).toSorted((left, right) => left.localeCompare(right));
  let totalPlaceholders = 0;

  for (const language of languages) {
    const languageDir = path.join(localesRoot, language);
    const targetPath = path.join(languageDir, targetLocaleFile);
    fs.mkdirSync(languageDir, { recursive: true });

    const existingTranslations = flattenObject(readObjectLiteral(targetPath));
    const nextTranslations = {};
    let emptyPlaceholders = 0;

    for (const message of usedMessages) {
      if (language === fallbackLanguage) {
        nextTranslations[message] = message;
      } else {
        const existingValue = existingTranslations[message];
        nextTranslations[message] = typeof existingValue === "string" && existingValue.length > 0 ? existingValue : "";
        if (nextTranslations[message] === "") emptyPlaceholders += 1;
      }
    }

    fs.writeFileSync(targetPath, localeFileContent(nextTranslations));
    syncAuxiliaryLocaleFiles(languageDir);
    totalPlaceholders += emptyPlaceholders;

    console.log(
      `i18n: synced ${usedMessages.length} source messages to ${language}/${targetLocaleFile}${
        language === fallbackLanguage ? "" : `; ${emptyPlaceholders} empty placeholders`
      }`
    );
  }

  console.log(
    `i18n: scanned ${usedMessages.length} source messages; wrote ${totalPlaceholders} non-English placeholders`
  );
}

main();
