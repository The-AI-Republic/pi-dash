// Platform release version check. A platform release (a `platform-v<semver>`
// tag, or a manual "Release" run of build-branch.yml with a `v<semver>`
// version) must match the root package.json version, which RELEASING.md bumps
// in the release commit. Ordinary PRs to main do not bump the version, so this
// runs at release time only. Dependency-free.
// Usage: node .github/scripts/check-release-version.mjs <tag-or-version> [package.json]
import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

/** Return null when `tag` names `pkgVersion`, else an error message. */
export function releaseVersionError(tag, pkgVersion) {
  if (!tag) return "no release tag or version given";
  if (!pkgVersion) return "package.json has no version";
  const version = tag.replace(/^platform-/, "").replace(/^v/, "");
  if (version === pkgVersion) return null;
  return `release ${tag} is version ${version}, but package.json says ${pkgVersion}; bump package.json in the release commit (see RELEASING.md)`;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const [tag, pkgPath = join(dirname(fileURLToPath(import.meta.url)), "..", "..", "package.json")] =
    process.argv.slice(2);
  const { version } = JSON.parse(readFileSync(pkgPath, "utf8"));
  const error = releaseVersionError(tag, version);
  if (error) {
    console.error(`::error::${error}`);
    process.exit(1);
  }
  console.log(`OK: release ${tag} matches package.json version ${version}`);
}
