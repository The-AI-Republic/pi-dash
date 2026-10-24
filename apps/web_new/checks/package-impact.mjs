// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Narrows test:changed when @pidash/kit or @pidash/api-client source changes.
// web_new imports both packages through their entry file (src/index.ts),
// which re-exports everything, so vitest's file-level import graph makes every
// test that touches a package depend on every file in it: one changed contract
// selected ~900 test files. Instead this finds the public names the change can
// affect (the changed files, plus package files that import them, mapped
// through the entry file's re-exports) and the web_new files that import one
// of those names as a value. test:changed passes those files to `vitest
// related` in place of the package files. Type-only imports are skipped:
// typecheck covers them. A namespace or default import of a package counts as
// using every name. Package tests run through the package's own test script.
const PACKAGES = [
  { name: "@pidash/kit", src: "packages/kit/src/" },
  { name: "@pidash/api-client", src: "packages/api-client/src/" },
];
const CODE = /\.[cm]?[jt]sx?$/;
const CONSUMER_ROOTS = ["apps/web_new/src/", "apps/web_new/e2e/"];

function packageOf(file) {
  return PACKAGES.find((pkg) => file.startsWith(pkg.src)) ?? null;
}

function entryOf(pkg) {
  return `${pkg.src}index.ts`;
}

// "a", "type a", "a as b", "type a as b" -> { name, alias, type }.
function parseSpecifiers(list) {
  return list
    .split(",")
    .map((part) => part.trim())
    .filter(Boolean)
    .map((part) => {
      const type = /^type\s+/.test(part);
      const [name, alias = name] = part.replace(/^type\s+/, "").split(/\s+as\s+/);
      return { name: name.trim(), alias: alias.trim(), type };
    });
}

// What one module exports: names it declares itself, names it re-exports from
// another module, and whole modules it re-exports with `export *`.
function parseExports(text) {
  const local = new Set();
  const reexports = [];
  const stars = [];
  const declared =
    /export\s+(?:declare\s+)?(?:default\s+)?(?:async\s+)?(?:abstract\s+)?(?:const|let|var|function\*?|class|type|interface|enum|namespace)\s+([A-Za-z_$][\w$]*)/g;
  for (const match of text.matchAll(declared)) local.add(match[1]);
  for (const match of text.matchAll(/export\s+(?:type\s+)?\{([^}]*)\}(\s*from\s*["']([^"']+)["'])?/g)) {
    const specifiers = parseSpecifiers(match[1]);
    if (match[3]) reexports.push({ from: match[3], specifiers });
    else for (const specifier of specifiers) local.add(specifier.alias);
  }
  for (const match of text.matchAll(/export\s*\*\s*(?:as\s+([A-Za-z_$][\w$]*)\s+)?from\s*["']([^"']+)["']/g)) {
    if (match[1]) reexports.push({ from: match[2], specifiers: [{ name: "*", alias: match[1], type: false }] });
    else stars.push(match[2]);
  }
  return { local, reexports, stars };
}

// Relative module specifiers a file imports or re-exports.
function relativeImports(text) {
  const specs = new Set();
  const patterns = [/\bfrom\s*["'](\.{1,2}\/[^"']+)["']/g, /\bimport\s*\(?\s*["'](\.{1,2}\/[^"']+)["']/g];
  for (const pattern of patterns) for (const match of text.matchAll(pattern)) specs.add(match[1]);
  return [...specs];
}

// Resolves "./x" or "./x.js" from `from` to a file in `files` (TypeScript's
// resolution for these packages: the .js suffix names the .ts source).
function resolveModule(from, spec, files) {
  const dir = from.slice(0, from.lastIndexOf("/") + 1);
  const parts = [];
  for (const part of `${dir}${spec}`.split("/")) {
    if (part === "..") parts.pop();
    else if (part !== ".") parts.push(part);
  }
  const base = parts.join("/").replace(/\.jsx?$/, "");
  return (
    [`${base}.ts`, `${base}.tsx`, base, `${base}/index.ts`, `${base}/index.tsx`].find((path) => files.has(path)) ?? null
  );
}

// Public name -> the package file that defines it, following the entry file's
// re-exports (and any nested barrels) down to the declaring file.
function publicSources(pkg, read, files) {
  const sources = new Map();
  const walk = (file, seen) => {
    const map = new Map();
    if (seen.has(file)) return map;
    seen.add(file);
    const { local, reexports, stars } = parseExports(read(file) ?? "");
    for (const name of local) map.set(name, file);
    for (const { from, specifiers } of reexports) {
      const target = resolveModule(file, from, files);
      if (!target) continue;
      const inner = walk(target, seen);
      for (const { name, alias } of specifiers) map.set(alias, name === "*" ? target : (inner.get(name) ?? target));
    }
    for (const from of stars) {
      const target = resolveModule(file, from, files);
      if (target) for (const [name, source] of walk(target, seen)) if (!map.has(name)) map.set(name, source);
    }
    return map;
  };
  for (const [name, source] of walk(entryOf(pkg), new Set())) sources.set(name, source);
  return sources;
}

// The entry file's own direct exports as name -> "from#name", to tell which
// names an edit to the entry file added or re-pointed.
function entryBindings(text) {
  const bindings = new Map();
  const { local, reexports, stars } = parseExports(text);
  for (const name of local) bindings.set(name, "local");
  for (const { from, specifiers } of reexports)
    for (const { name, alias } of specifiers) bindings.set(alias, `${from}#${name}`);
  for (const from of stars) bindings.set(`*${from}`, from);
  return bindings;
}

// Package files the changed ones can affect: themselves plus every package
// file that imports them, directly or through others. Barrels (index files)
// are not followed: they are how every name reaches every test.
function affectedPackageFiles(pkg, changed, read, files) {
  const importers = new Map();
  for (const file of files) {
    if (!file.startsWith(pkg.src) || !CODE.test(file) || /\/index\.tsx?$/.test(file)) continue;
    for (const spec of relativeImports(read(file) ?? "")) {
      const target = resolveModule(file, spec, files);
      if (!target) continue;
      if (!importers.has(target)) importers.set(target, new Set());
      importers.get(target).add(file);
    }
  }
  const affected = new Set(changed);
  const queue = [...changed];
  while (queue.length > 0) {
    for (const importer of importers.get(queue.pop()) ?? []) {
      if (affected.has(importer)) continue;
      affected.add(importer);
      queue.push(importer);
    }
  }
  return affected;
}

// Whether a file imports one of `names` from `pkg` as a value (or the whole
// package through a default or namespace import).
function importsAny(text, pkg, names) {
  const escaped = pkg.name.replace(/[/\\^$.*+?()[\]{}|-]/g, "\\$&");
  const statement = new RegExp(`(import|export)\\s+(type\\s+)?([^;]*?)\\s*from\\s*["']${escaped}["']`, "g");
  for (const match of text.matchAll(statement)) {
    if (match[2]) continue;
    const clause = match[3];
    // A default or namespace import (or `export *`) can reach any name.
    if (
      clause
        .replace(/\{[^}]*\}/, "")
        .replace(/,/g, " ")
        .trim()
    )
      return true;
    // Otherwise the clause is just `{ ... }`.
    const specifiers = parseSpecifiers(clause.trim().slice(1, -1));
    if (specifiers.some((specifier) => !specifier.type && names.has(specifier.name))) return true;
  }
  return false;
}

// The whole analysis. `changedFiles` are repo-relative; `read(path)` returns a
// file's text or null; `files` lists every tracked file; `readBase(path)`
// returns a file's text at the base, or null.
function packageImpact({ changedFiles, read, files, readBase }) {
  const fileSet = new Set(files);
  const packageFiles = changedFiles.filter((file) => packageOf(file));
  const names = new Map();
  for (const pkg of PACKAGES) {
    const changed = packageFiles.filter((file) => file.startsWith(pkg.src) && CODE.test(file));
    if (changed.length === 0) continue;
    const pkgNames = new Set();
    if (changed.includes(entryOf(pkg))) {
      const before = entryBindings(readBase(entryOf(pkg)) ?? "");
      for (const [name, binding] of entryBindings(read(entryOf(pkg)) ?? ""))
        if (before.get(name) !== binding && !name.startsWith("*")) pkgNames.add(name);
    }
    const affected = affectedPackageFiles(pkg, changed, read, fileSet);
    for (const [name, source] of publicSources(pkg, read, fileSet)) if (affected.has(source)) pkgNames.add(name);
    names.set(pkg, pkgNames);
  }
  const consumers = files.filter(
    (file) =>
      CONSUMER_ROOTS.some((root) => file.startsWith(root)) &&
      CODE.test(file) &&
      [...names].some(([pkg, pkgNames]) => pkgNames.size > 0 && importsAny(read(file) ?? "", pkg, pkgNames))
  );
  return {
    packageFiles,
    names: [...names].flatMap(([pkg, pkgNames]) => [...pkgNames].sort().map((name) => `${pkg.name}:${name}`)),
    consumers,
  };
}

export {
  affectedPackageFiles,
  entryBindings,
  importsAny,
  packageImpact,
  parseExports,
  publicSources,
  relativeImports,
  resolveModule,
};
