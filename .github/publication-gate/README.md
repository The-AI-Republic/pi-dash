# Publication gate

Nothing reaches the public repository or a public registry without passing
this gate. It covers release tags, GitHub Releases, runner archives and
installers, the self-host assets and the Docker images.

## The flow

1. **Stage.** A release starts from `workflow_dispatch` on `Release`
   (runner) or `Branch Build CE` (platform), with the full SHA of a commit
   that is already on `main`. Code reaches `main` only through reviewed pull
   requests, so the candidate is always built from a gated ref. The run
   refuses a SHA that is not on `main` and a tag that already exists.
2. **Build.** Every artifact is built from that SHA. Docker images are built
   into OCI archives; nothing is pushed.
3. **Finish.** Every transformation happens before the scan: the install
   wrappers are pinned to the release, `docker-compose.yml` is pinned to the
   image tag, and the release title and notes are written to files.
   `gate.py build-manifest` records the SHA-256 of every file.
4. **Scan and test.** The `publication-scan` job checks the manifest, runs a
   self-test that plants a synthetic leak and fails unless the scanner
   rejects it, then scans the candidate (opening archives, compressed blobs
   and image layers) and the source tree of the SHA. It looks for credential
   shapes and for every marker in the `PUBLICATION_DENYLIST` secret. The
   runner workflow also runs the Linux binary and checks its version and
   checksums. The job's output is the digest of the approved manifest.
5. **Publish.** The `publish` job runs in the `release-publisher`
   environment, after a required reviewer approves it. It refuses to run
   unless the downloaded candidate matches the approved manifest digest file
   for file, then checks that the live tag ruleset still enforces the gate,
   then creates the release tag at the scanned SHA and publishes exactly the
   scanned files and image digests.

No workflow starts on a tag push, a ref creation or a release event, so
creating a release tag, by the publisher or anyone else, publishes nothing.

## What enforces it

| Promise | Enforced by |
| --- | --- |
| Ordinary identities cannot create, move or delete a release tag | The tag ruleset in `ruleset.json` (tags `pidash-v*`, `platform-v*`, `v*`), whose only bypass actor is the publisher GitHub App |
| Publish credentials exist only in the publisher | Registry, cloud and App credentials are secrets of the `release-publisher` environment; no repository-level publish secrets |
| An unscanned ref or artifact cannot be published | The publisher needs the scan job and verifies the approved manifest digest before any publishing step |
| Workflows cannot drift from the above | `gate.py check-policy`, run on every pull request by `publication-gate.yml` using the base branch's checker |

`gate.py check-policy` fails a pull request that adds a tag or release
trigger, reads a publish secret outside the publisher, grants write
permissions outside it, runs a publishing command (`git push`, `gh release`,
`skopeo copy`, `docker push`, ...) outside it, or weakens the ruleset.

## One-time setup (repository admin)

These are repository settings, not code, so this change does not apply them.
Until they are in place, the publisher job fails closed: it has no
credentials and the live ruleset check fails.

1. **Publisher GitHub App.** Create an App owned by the organization with
   repository permissions *Contents: read and write* and *Administration:
   read* (to read the ruleset), install it on this repository only, and note
   its App ID.
2. **Environments.**
   - `release-publisher`: required reviewers (the release owners), *Prevent
     self-review* on, deployment branches limited to `main`. Variable
     `PUBLISHER_APP_ID`; secrets `PUBLISHER_APP_PRIVATE_KEY`,
     `DOCKERHUB_USERNAME`, `DOCKERHUB_TOKEN`, `AWS_ACCESS_KEY_ID`,
     `AWS_SECRET_ACCESS_KEY`.
   - `publication-scan`: deployment branches limited to `main`. Secret
     `PUBLICATION_DENYLIST`: base64 of a text file with one literal marker per
     line (names and strings that must never appear in public output). The
     list stays out of this repository so the markers stay private. The scan
     refuses to run with an empty list.
3. **Retire the old credentials.** Delete the repository-level secrets
   `DOCKERHUB_USERNAME`, `DOCKERHUB_TOKEN`, `AWS_ACCESS_KEY_ID` and
   `AWS_SECRET_ACCESS_KEY`, and rotate the Docker Hub token and AWS key that
   they held, since earlier workflows exposed them to any job.
4. **Tag ruleset.** Substitute the App ID and create the ruleset:

   ```bash
   sed "s/\"PUBLISHER_APP_ID\"/<app-id>/" .github/publication-gate/ruleset.json \
     | gh api -X POST repos/The-AI-Republic/pi-dash/rulesets --input -
   ```

   Then confirm an ordinary push is refused:
   `git push origin <any-sha>:refs/tags/v0.0.0-gate-check` must fail.
5. **Make the policy check required** on `main` (`publication gate policy`).

## Scanner allowlist

`allowlist.txt` lists `<rule> <path-glob>` pairs the generic credential rules
skip, each with a reason. It never applies to the denylist. Vendored
third-party trees inside images (`node_modules`, `site-packages`, system
library paths) are skipped by the generic rules for the same reason, and are
still checked against the denylist.

## Running it locally

```bash
python3 -m pip install pyyaml
python3 -m unittest discover -s .github/publication-gate/tests
python3 .github/publication-gate/gate.py check-policy
python3 .github/publication-gate/gate.py scan <dir-or-file>...
```
