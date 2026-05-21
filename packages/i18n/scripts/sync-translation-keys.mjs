#!/usr/bin/env node

import fs from "node:fs";
import path from "node:path";
import ts from "typescript";

const repoRoot = path.resolve(import.meta.dirname, "../../..");
const localesRoot = path.join(repoRoot, "packages/i18n/src/locales");
const sourceRoots = ["apps", "packages"].map((root) => path.join(repoRoot, root));
const existingLocaleFiles = ["core", "translations", "accessibility", "editor", "empty-state"];
const targetLocaleFile = "translations.ts";
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

function readFileIfExists(filePath) {
  return fs.existsSync(filePath) ? fs.readFileSync(filePath, "utf8") : null;
}

function walkFiles(dir, files = []) {
  if (!fs.existsSync(dir)) return files;

  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
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

function collectUsedTranslationKeys() {
  const keys = new Map();
  const files = sourceRoots.flatMap((root) => walkFiles(root));

  for (const filePath of files) {
    const sourceText = fs.readFileSync(filePath, "utf8");
    const sourceFile = ts.createSourceFile(filePath, sourceText, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);

    function visit(node) {
      if (
        ts.isCallExpression(node) &&
        ts.isIdentifier(node.expression) &&
        node.expression.text === "t" &&
        node.arguments.length > 0 &&
        isStringLike(node.arguments[0])
      ) {
        const key = node.arguments[0].text.trim();
        if (key) {
          const location = sourceFile.getLineAndCharacterOfPosition(node.arguments[0].getStart(sourceFile));
          const references = keys.get(key) || [];
          references.push(`${path.relative(repoRoot, filePath)}:${location.line + 1}:${location.character + 1}`);
          keys.set(key, references);
        }
      }

      ts.forEachChild(node, visit);
    }

    visit(sourceFile);
  }

  return keys;
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

function hasPath(object, keyPath) {
  const parts = keyPath.split(".");
  let current = object;

  for (const part of parts) {
    if (!current || typeof current !== "object" || !Object.prototype.hasOwnProperty.call(current, part)) {
      return false;
    }
    current = current[part];
  }

  return true;
}

function hasShapeConflict(object, keyPath) {
  const parts = keyPath.split(".");
  let current = object;

  for (const part of parts.slice(0, -1)) {
    if (!current || typeof current !== "object" || !Object.prototype.hasOwnProperty.call(current, part)) {
      return false;
    }

    current = current[part];
    if (!current || typeof current !== "object") {
      return true;
    }
  }

  const leaf = parts.at(-1);
  return (
    current &&
    typeof current === "object" &&
    Object.prototype.hasOwnProperty.call(current, leaf) &&
    current[leaf] &&
    typeof current[leaf] === "object"
  );
}

function setPath(object, keyPath, value) {
  const parts = keyPath.split(".");
  let current = object;

  for (const part of parts.slice(0, -1)) {
    if (!current[part] || typeof current[part] !== "object") {
      current[part] = {};
    }
    current = current[part];
  }

  const leaf = parts.at(-1);
  if (!Object.prototype.hasOwnProperty.call(current, leaf)) {
    current[leaf] = value;
  }
}

function mergeObjects(...objects) {
  const result = {};

  for (const object of objects) {
    mergeInto(result, object);
  }

  return result;
}

function mergeInto(target, source) {
  for (const [key, value] of Object.entries(source)) {
    if (
      value &&
      typeof value === "object" &&
      !Array.isArray(value) &&
      target[key] &&
      typeof target[key] === "object" &&
      !Array.isArray(target[key])
    ) {
      mergeInto(target[key], value);
    } else {
      target[key] = cloneValue(value);
    }
  }
}

function cloneValue(value) {
  if (!value || typeof value !== "object" || Array.isArray(value)) return value;

  return Object.entries(value).reduce((acc, [key, entryValue]) => {
    acc[key] = cloneValue(entryValue);
    return acc;
  }, {});
}

function formatObject(value, indent = 2) {
  const entries = Object.entries(value);
  if (entries.length === 0) return "{}";

  const pad = " ".repeat(indent);
  const lines = ["{"];

  for (const [key, entryValue] of entries) {
    const property = /^[A-Za-z_$][\w$]*$/.test(key) ? key : JSON.stringify(key);

    if (entryValue && typeof entryValue === "object" && !Array.isArray(entryValue)) {
      lines.push(`${pad}${property}: ${formatObject(entryValue, indent + 2)},`);
    } else {
      lines.push(`${pad}${property}: ${JSON.stringify(entryValue)},`);
    }
  }

  lines.push(`${" ".repeat(indent - 2)}}`);
  return lines.join("\n");
}

function localeFileContent(object) {
  return `/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

export default ${formatObject(object)} as const;
`;
}

function main() {
  const usedKeyReferences = collectUsedTranslationKeys();
  const usedKeys = Array.from(usedKeyReferences.keys());
  const conflictReferences = new Map();
  let totalAdded = 0;
  let totalConflicts = 0;

  for (const language of languages) {
    const languageDir = path.join(localesRoot, language);
    const targetPath = path.join(languageDir, targetLocaleFile);
    fs.mkdirSync(languageDir, { recursive: true });

    const localeObjects = existingLocaleFiles.map((file) => readObjectLiteral(path.join(languageDir, `${file}.ts`)));
    const localeTranslations = mergeObjects(...localeObjects);
    const targetTranslations = readObjectLiteral(targetPath);
    let addedForLanguage = 0;
    let conflictsForLanguage = 0;

    for (const key of usedKeys) {
      if (hasShapeConflict(localeTranslations, key)) {
        conflictsForLanguage += 1;
        conflictReferences.set(key, usedKeyReferences.get(key) || []);
        continue;
      }

      if (!hasPath(localeTranslations, key)) {
        setPath(targetTranslations, key, "");
        setPath(localeTranslations, key, "");
        addedForLanguage += 1;
      }
    }

    if (addedForLanguage > 0) {
      fs.writeFileSync(targetPath, localeFileContent(targetTranslations));
      console.log(`i18n: added ${addedForLanguage} placeholder keys to ${language}/${targetLocaleFile}`);
    }

    totalAdded += addedForLanguage;
    totalConflicts += conflictsForLanguage;
  }

  if (conflictReferences.size > 0) {
    console.error("i18n: skipped keys with shape conflicts:");
    for (const [key, references] of Array.from(conflictReferences.entries()).toSorted(([a], [b]) =>
      a.localeCompare(b)
    )) {
      const referenceText = references.length > 0 ? ` (${references.slice(0, 5).join(", ")})` : "";
      console.error(`i18n:   ${key}${referenceText}`);
    }
  }

  console.log(
    `i18n: scanned ${usedKeys.length} literal translation keys; added ${totalAdded} placeholders; skipped ${totalConflicts} shape conflicts`
  );

  if (conflictReferences.size > 0 && process.env.I18N_SYNC_ALLOW_CONFLICTS !== "1") {
    throw new Error("i18n sync found shape conflicts. Rename the key or set I18N_SYNC_ALLOW_CONFLICTS=1 to override.");
  }
}

main();
