// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Picks the web_new test files a change can affect, by imported name.
// Vitest's own graph (`vitest related`) is file-level: a test that imports one
// hook from an entry file such as features/auth/index.ts or @pidash/kit
// depends on every file that entry re-exports, so one changed component
// selected 600-900 test files. This builds the graph from the sources instead
// (web_new plus @pidash/kit and @pidash/api-client, parsed with TypeScript)
// and follows an import only to the files that declare the names it imports,
// through any chain of re-exports. A file is affected when it imports a name
// declared in a changed or affected file; the selection is every affected
// test file.
// - Type-only imports and exports are skipped: typecheck covers types.
// - A namespace import, a side-effect import or a dynamic import() depends on
//   everything the target exports.
// - A module that runs code at load time (a top-level statement such as a
//   registration call) affects every file that loads it, directly or through
//   re-exports, whatever names they import.
// - An edited re-export (a name added, removed or pointed elsewhere) affects
//   the files importing that name from the edited file or from any entry file
//   that passes it on; an edited `export *` line affects all of their
//   importers.
// - Imports it cannot resolve (npm packages) are not edges.
// What it can miss is a dependency no import shows; the 12-hour full-suite
// run on main catches that after merge.
import ts from "typescript";

const WEB_NEW = "apps/web_new/";
const SOURCE_ROOTS = [WEB_NEW, "packages/kit/src/", "packages/api-client/src/"];
const TEST_SETUP = `${WEB_NEW}src/test-setup.ts`;
const CODE = /\.[cm]?[jt]sx?$/;
const TEST = /\.test\.[cm]?[jt]sx?$/;

// The vite/vitest aliases, with the edition module vitest.config.ts resolves
// (core/edition/oss.ts unless PIDASH_EDITION_MODULE points elsewhere).
function aliasesFor(editionModule = `${WEB_NEW}src/core/edition/oss.ts`) {
  return new Map([
    ["@pidash/kit", "packages/kit/src/index.ts"],
    ["@pidash/api-client", "packages/api-client/src/index.ts"],
    ["@pidash/edition", editionModule],
    ["@pidash/platform-target", `${WEB_NEW}src/core/platform/web.ts`],
  ]);
}

function scriptKind(file) {
  if (/\.tsx$/.test(file)) return ts.ScriptKind.TSX;
  if (/\.jsx$/.test(file)) return ts.ScriptKind.JSX;
  if (/\.[cm]?js$/.test(file)) return ts.ScriptKind.JS;
  return ts.ScriptKind.TS;
}

function hasModifier(node, kind) {
  return ts.getModifiers(node)?.some((modifier) => modifier.kind === kind) ?? false;
}

function bindingNames(name, out) {
  if (ts.isIdentifier(name)) out.push(name.text);
  else for (const element of name.elements) if (!ts.isOmittedExpression(element)) bindingNames(element.name, out);
  return out;
}

// Top-level statements that run nothing observable when the module loads.
const NO_EFFECT = new Set([
  ts.SyntaxKind.ImportDeclaration,
  ts.SyntaxKind.ImportEqualsDeclaration,
  ts.SyntaxKind.ExportDeclaration,
  ts.SyntaxKind.ExportAssignment,
  ts.SyntaxKind.FunctionDeclaration,
  ts.SyntaxKind.ClassDeclaration,
  ts.SyntaxKind.InterfaceDeclaration,
  ts.SyntaxKind.TypeAliasDeclaration,
  ts.SyntaxKind.EnumDeclaration,
  ts.SyntaxKind.ModuleDeclaration,
  ts.SyntaxKind.VariableStatement,
  ts.SyntaxKind.EmptyStatement,
]);

// One module's value imports and exports:
// - imports: { from, names } with names a Set, or null for everything;
// - local: names it declares itself; reexports: { from, specifiers: [{ name,
//   alias }] } (name "*" for `export * as alias`); stars: `export *` targets;
// - effects: whether it runs a top-level statement when loaded.
function parseModule(text, file = "module.ts") {
  const source = ts.createSourceFile(file, text, ts.ScriptTarget.Latest, false, scriptKind(file));
  const result = { imports: [], local: new Set(), reexports: [], stars: [], effects: false };
  for (const statement of source.statements) {
    if (ts.isImportDeclaration(statement)) {
      const from = statement.moduleSpecifier.text;
      const clause = statement.importClause;
      if (!clause) result.imports.push({ from, names: null });
      else if (!clause.isTypeOnly) {
        const named = clause.namedBindings;
        if (named && ts.isNamespaceImport(named)) result.imports.push({ from, names: null });
        else {
          const names = new Set(clause.name ? ["default"] : []);
          for (const element of named?.elements ?? [])
            if (!element.isTypeOnly) names.add((element.propertyName ?? element.name).text);
          if (names.size > 0) result.imports.push({ from, names });
        }
      }
    } else if (ts.isExportDeclaration(statement)) {
      if (statement.isTypeOnly) continue;
      const clause = statement.exportClause;
      const from = statement.moduleSpecifier?.text;
      if (from === undefined) {
        for (const element of clause.elements) if (!element.isTypeOnly) result.local.add(element.name.text);
      } else if (!clause) result.stars.push(from);
      else if (ts.isNamespaceExport(clause))
        result.reexports.push({ from, specifiers: [{ name: "*", alias: clause.name.text }] });
      else {
        const specifiers = clause.elements
          .filter((element) => !element.isTypeOnly)
          .map((element) => ({ name: (element.propertyName ?? element.name).text, alias: element.name.text }));
        if (specifiers.length > 0) result.reexports.push({ from, specifiers });
      }
    } else if (ts.isExportAssignment(statement)) result.local.add("default");
    else if (hasModifier(statement, ts.SyntaxKind.ExportKeyword)) {
      if (hasModifier(statement, ts.SyntaxKind.DefaultKeyword)) result.local.add("default");
      else if (ts.isVariableStatement(statement))
        for (const declaration of statement.declarationList.declarations)
          for (const name of bindingNames(declaration.name, [])) result.local.add(name);
      else result.local.add(statement.name.text);
    } else if (
      !NO_EFFECT.has(statement.kind) &&
      !(ts.isExpressionStatement(statement) && ts.isStringLiteral(statement.expression))
    )
      result.effects = true;
  }
  const visit = (node) => {
    if (
      ts.isCallExpression(node) &&
      node.expression.kind === ts.SyntaxKind.ImportKeyword &&
      node.arguments.length > 0 &&
      ts.isStringLiteralLike(node.arguments[0])
    )
      result.imports.push({ from: node.arguments[0].text, names: null });
    ts.forEachChild(node, visit);
  };
  visit(source);
  return result;
}

// Resolves a specifier from `from` to a file in `files`: relative paths, the
// aliases, and TypeScript's .js-names-the-.ts-source rule.
function resolveSpecifier(from, spec, files, aliases = aliasesFor()) {
  let base;
  if (aliases.has(spec)) return aliases.get(spec);
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

// A module's direct exports as name -> where it comes from, to tell which
// names an edit added, removed or re-pointed.
function bindings(module) {
  const map = new Map();
  for (const name of module.local) map.set(name, "local");
  for (const { from, specifiers } of module.reexports)
    for (const { name, alias } of specifiers) map.set(alias, `${from}#${name}`);
  for (const from of module.stars) map.set(`*${from}`, from);
  return map;
}

function add(map, key, value) {
  if (!map.has(key)) map.set(key, new Set());
  map.get(key).add(value);
}

// Builds the analysis for one tree. `read(path)` returns a file's text or
// null; `files` is every tracked path under the source roots.
function createGraph(files, read, aliases = aliasesFor()) {
  const fileSet = new Set(files);
  const modules = new Map();
  const moduleOf = (file) => {
    if (!modules.has(file)) modules.set(file, parseModule((CODE.test(file) ? read(file) : null) ?? "", file));
    return modules.get(file);
  };
  const resolve = (from, spec) => resolveSpecifier(from, spec, fileSet, aliases);
  const everything = (file, map) => new Set([file, ...[...map.values()].flatMap((sources) => [...sources])]);
  const exportMaps = new Map();
  // Public name -> the files that declare it, following re-exports. A map
  // computed while a re-export cycle was still open is not cached.
  const walk = (file, open) => {
    if (exportMaps.has(file)) return { map: exportMaps.get(file), complete: true };
    if (open.has(file)) return { map: new Map(), complete: false };
    open.add(file);
    let complete = true;
    const map = new Map();
    const { local, reexports, stars } = moduleOf(file);
    for (const name of local) map.set(name, new Set([file]));
    for (const { from, specifiers } of reexports) {
      const target = resolve(file, from);
      if (!target) continue;
      const inner = walk(target, open);
      complete &&= inner.complete;
      for (const { name, alias } of specifiers)
        map.set(alias, name === "*" ? everything(target, inner.map) : (inner.map.get(name) ?? new Set([target])));
    }
    for (const from of stars) {
      const target = resolve(file, from);
      if (!target) continue;
      const inner = walk(target, open);
      complete &&= inner.complete;
      for (const [name, sources] of inner.map) if (!map.has(name)) map.set(name, sources);
    }
    open.delete(file);
    if (complete) exportMaps.set(file, map);
    return { map, complete };
  };
  const exportMap = (file) => walk(file, new Set()).map;
  // Declaring file -> files whose imports reach it by name.
  const dependents = new Map();
  // "file\0name" -> files importing that name straight from that file ("*"
  // for namespace, side-effect and dynamic imports).
  const directImporters = new Map();
  // File -> files that load it (any value import or re-export of it).
  const loaders = new Map();
  // File -> [{ by, specifiers }] re-exporting from it (specifiers null for
  // `export *`).
  const reexporters = new Map();
  for (const file of files) {
    if (!CODE.test(file)) continue;
    const module = moduleOf(file);
    for (const { from, names } of module.imports) {
      const target = resolve(file, from);
      if (!target) continue;
      add(loaders, target, file);
      const map = exportMap(target);
      for (const source of names === null
        ? everything(target, map)
        : [...names].flatMap((name) => [...(map.get(name) ?? [target])]))
        add(dependents, source, file);
      for (const name of names ?? ["*"]) add(directImporters, `${target}\u0000${name}`, file);
    }
    const passOn = [...module.reexports, ...module.stars.map((from) => ({ from, specifiers: null }))];
    for (const { from, specifiers } of passOn) {
      const target = resolve(file, from);
      if (!target) continue;
      add(loaders, target, file);
      if (!reexporters.has(target)) reexporters.set(target, []);
      reexporters.get(target).push({ by: file, specifiers });
    }
  }
  // Files importing `name` (null: any name) from `file`, or the same binding
  // from any entry file that passes it on.
  const importersOf = (file, name, seen = new Set()) => {
    if (seen.has(`${file}\u0000${name}`)) return [];
    seen.add(`${file}\u0000${name}`);
    const found = [...(directImporters.get(`${file}\u0000*`) ?? [])];
    for (const [key, set] of directImporters)
      if (key.startsWith(`${file}\u0000`) && (name === null || key === `${file}\u0000${name}`)) found.push(...set);
    for (const { by, specifiers } of reexporters.get(file) ?? []) {
      if (specifiers === null) found.push(...importersOf(by, name, seen));
      for (const specifier of specifiers ?? [])
        if (name === null || specifier.name === name || specifier.name === "*")
          found.push(...importersOf(by, name === null ? null : specifier.alias, seen));
    }
    return found;
  };
  return { moduleOf, exportMap, dependents, loaders, importersOf };
}

// The test files under web_new that `changedFiles` can affect, and whether the
// change reaches src/test-setup.ts (which every test loads). `readBase(path)`
// returns a file's text at the base, or null when it did not exist.
function affectedTests({ changedFiles, read, readBase, files, aliases }) {
  const graph = createGraph(files, read, aliases);
  const tracked = new Set(files);
  const affected = new Set();
  const queue = [];
  const reach = (file) => {
    if (affected.has(file)) return;
    affected.add(file);
    queue.push(file);
  };
  // A module that runs code at load time affects everything that loads it.
  const loadedBy = (file) => {
    const stack = [file];
    const seen = new Set(stack);
    while (stack.length > 0)
      for (const loader of graph.loaders.get(stack.pop()) ?? [])
        if (!seen.has(loader)) {
          seen.add(loader);
          stack.push(loader);
          reach(loader);
        }
  };
  for (const file of changedFiles) {
    reach(file);
    if (!CODE.test(file)) continue;
    const before = parseModule(readBase(file) ?? "", file);
    const after = graph.moduleOf(file);
    if (before.effects) loadedBy(file);
    const was = bindings(before);
    const now = bindings(after);
    for (const name of new Set([...was.keys(), ...now.keys()]))
      if (was.get(name) !== now.get(name))
        for (const importer of graph.importersOf(file, name.startsWith("*") ? null : name)) reach(importer);
  }
  while (queue.length > 0) {
    const file = queue.pop();
    if (graph.moduleOf(file).effects) loadedBy(file);
    for (const dependent of graph.dependents.get(file) ?? []) reach(dependent);
  }
  return {
    tests: [...affected].filter((file) => file.startsWith(WEB_NEW) && TEST.test(file) && tracked.has(file)).sort(),
    setupReached: affected.has(TEST_SETUP),
  };
}

export { SOURCE_ROOTS, affectedTests, aliasesFor, bindings, createGraph, parseModule, resolveSpecifier };
