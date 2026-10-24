// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Picks the web_new test files a change can affect, by imported name.
// Vitest's own graph (`vitest related`) is file-level: a test that imports one
// hook from an entry file such as features/auth/index.ts or @pidash/kit
// depends on every file that entry re-exports, so one changed component
// selected 600-900 test files. This builds the graph from the sources instead
// (web_new plus @pidash/kit and @pidash/api-client) and follows an import
// only to the files that declare the names it imports, through any chain of
// re-exports. A file is affected when it imports a name declared in a changed
// or affected file; the selection is every affected test file.
// - Type-only imports are skipped: typecheck covers types.
// - A default-less namespace import, a side-effect import or a dynamic
//   import() counts as depending on everything the target exports.
// - An edited re-export (a name added or pointed elsewhere) affects the files
//   that import that name from the edited file; an edited `export *` line
//   affects every file that imports from it.
// - Imports it cannot resolve (npm packages) are not edges.
// What it can miss is a dependency no import statement shows; the 12-hour
// full-suite run on web-new-dev catches that after merge.
const WEB_NEW = "apps/web_new/";
const ALIASES = new Map([
  ["@pidash/kit", "packages/kit/src/index.ts"],
  ["@pidash/api-client", "packages/api-client/src/index.ts"],
  ["@pidash/edition", `${WEB_NEW}src/core/edition/oss.ts`],
  ["@pidash/platform-target", `${WEB_NEW}src/core/platform/web.ts`],
]);
const CODE = /\.[cm]?[jt]sx?$/;
const TEST = /\.test\.[cm]?[jt]sx?$/;

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
    /export\s+(?:declare\s+)?(?:async\s+)?(?:abstract\s+)?(?:const|let|var|function\*?|class|type|interface|enum|namespace)\s+([A-Za-z_$][\w$]*)/g;
  for (const match of text.matchAll(declared)) local.add(match[1]);
  if (/export\s+default\b/.test(text)) local.add("default");
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

// What one module imports, as values: { from, names } where names is a Set of
// imported names, or null for "everything" (namespace, default-plus, side
// effect, dynamic). Type-only imports and re-exports are left out.
function parseImports(text) {
  const imports = [];
  for (const match of text.matchAll(/\bimport\s+(type\s+)?([^;]*?)\s*from\s*["']([^"']+)["']/g)) {
    if (match[1]) continue;
    const clause = match[2];
    const braces = clause.match(/\{([^}]*)\}/);
    const outside = clause
      .replace(/\{[^}]*\}/, "")
      .replace(/,/g, " ")
      .trim();
    const names = new Set(
      braces
        ? parseSpecifiers(braces[1])
            .filter((s) => !s.type)
            .map((s) => s.name)
        : []
    );
    if (outside.startsWith("*")) imports.push({ from: match[3], names: null });
    else {
      if (outside) names.add("default");
      if (names.size > 0) imports.push({ from: match[3], names });
    }
  }
  for (const match of text.matchAll(/(?<!typeof\s)\bimport\s*(?:\(\s*)?["']([^"']+)["']/g))
    imports.push({ from: match[1], names: null });
  return imports;
}

// Resolves a specifier from `from` to a file in `files`: relative paths, the
// vite/vitest aliases, and TypeScript's .js-names-the-.ts-source rule.
function resolveSpecifier(from, spec, files) {
  let base;
  if (ALIASES.has(spec)) return ALIASES.get(spec);
  if (spec.startsWith("@web_new/")) base = `${WEB_NEW}src/${spec.slice("@web_new/".length)}`;
  else if (spec.startsWith(".")) {
    const parts = [];
    for (const part of `${from.slice(0, from.lastIndexOf("/") + 1)}${spec}`.split("/")) {
      if (part === "..") parts.pop();
      else if (part !== ".") parts.push(part);
    }
    base = parts.join("/");
  } else return null;
  const stem = base.replace(/\.[cm]?jsx?$/, "");
  return (
    [base, `${stem}.ts`, `${stem}.tsx`, `${base}.ts`, `${base}.tsx`, `${base}/index.ts`, `${base}/index.tsx`].find(
      (path) => files.has(path)
    ) ?? null
  );
}

// Builds the analysis for one tree. `read(path)` returns a file's text or
// null; `files` is every tracked path under web_new and the two packages.
function createGraph(files, read) {
  const fileSet = new Set(files);
  const texts = new Map();
  const text = (file) => {
    if (!texts.has(file)) texts.set(file, CODE.test(file) ? (read(file) ?? "") : "");
    return texts.get(file);
  };
  const exportMaps = new Map();
  // Public name -> the files that declare it, following re-exports.
  const exportMap = (file, seen = new Set()) => {
    if (exportMaps.has(file)) return exportMaps.get(file);
    const map = new Map();
    if (seen.has(file)) return map;
    seen.add(file);
    const { local, reexports, stars } = parseExports(text(file));
    for (const name of local) map.set(name, new Set([file]));
    for (const { from, specifiers } of reexports) {
      const target = resolveSpecifier(file, from, fileSet);
      if (!target) continue;
      const inner = exportMap(target, seen);
      for (const { name, alias } of specifiers)
        map.set(alias, name === "*" ? everything(target, inner) : (inner.get(name) ?? new Set([target])));
    }
    for (const from of stars) {
      const target = resolveSpecifier(file, from, fileSet);
      if (target) for (const [name, sources] of exportMap(target, seen)) if (!map.has(name)) map.set(name, sources);
    }
    exportMaps.set(file, map);
    return map;
  };
  const everything = (file, map) => new Set([file, ...[...map.values()].flatMap((sources) => [...sources])]);
  // Declaring file -> the files whose imports reach it.
  const dependents = new Map();
  // "file\0name" -> files importing that name straight from that file, and
  // file -> files importing any name straight from it.
  const directImporters = new Map();
  const importersOf = new Map();
  const link = (source, importer) => {
    if (!dependents.has(source)) dependents.set(source, new Set());
    dependents.get(source).add(importer);
  };
  for (const file of files) {
    if (!CODE.test(file)) continue;
    for (const { from, names } of parseImports(text(file))) {
      const target = resolveSpecifier(file, from, fileSet);
      if (!target) continue;
      const map = exportMap(target);
      if (names === null) {
        for (const source of everything(target, map)) link(source, file);
        continue;
      }
      for (const name of names) {
        for (const source of map.get(name) ?? [target]) link(source, file);
        const key = `${target}\u0000${name}`;
        if (!directImporters.has(key)) directImporters.set(key, new Set());
        directImporters.get(key).add(file);
      }
      if (!importersOf.has(target)) importersOf.set(target, new Set());
      importersOf.get(target).add(file);
    }
  }
  return { exportMap, dependents, directImporters, importersOf, text };
}

// The file's own direct exports as name -> where it comes from, to tell which
// names an edit re-pointed or added.
function bindings(text) {
  const map = new Map();
  const { local, reexports, stars } = parseExports(text);
  for (const name of local) map.set(name, "local");
  for (const { from, specifiers } of reexports)
    for (const { name, alias } of specifiers) map.set(alias, `${from}#${name}`);
  for (const from of stars) map.set(`*${from}`, from);
  return map;
}

// The test files under web_new that `changedFiles` can affect. `readBase(path)`
// returns a file's text at the base, or null when it did not exist.
function affectedTests({ changedFiles, read, readBase, files }) {
  const graph = createGraph(files, read);
  const tracked = new Set(files);
  const affected = new Set(changedFiles);
  const queue = [...changedFiles];
  for (const file of changedFiles) {
    if (!CODE.test(file)) continue;
    const before = bindings(readBase(file) ?? "");
    for (const [name, binding] of bindings(graph.text(file)))
      if (before.get(name) !== binding) {
        const importers = name.startsWith("*")
          ? graph.importersOf.get(file)
          : graph.directImporters.get(`${file}\u0000${name}`);
        for (const importer of importers ?? []) queue.push(importer);
      }
  }
  while (queue.length > 0) {
    const file = queue.pop();
    affected.add(file);
    for (const dependent of graph.dependents.get(file) ?? []) if (!affected.has(dependent)) queue.push(dependent);
  }
  return [...affected].filter((file) => file.startsWith(WEB_NEW) && TEST.test(file) && tracked.has(file)).sort();
}

export { affectedTests, bindings, createGraph, parseExports, parseImports, resolveSpecifier };
