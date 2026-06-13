# @pi-dash/i18n

Shared UI translation package for Pi Dash.

## Translation Storage

Translations live under `packages/i18n/src/locales/<language>/`.

Each locale is a TypeScript object, not JSON:

```ts
export default {
  "Save changes": "Enregistrer les modifications",
} as const;
```

UI code uses the source English text as the message id:

```tsx
const { t } = useTranslation();

return <button>{t("Save changes")}</button>;
```

Missing or empty target translations fall back to English by returning the source message id. ICU MessageFormat parameters are supported:

```tsx
t("Delete {count, plural, one {# work item} other {# work items}}", { count });
```

## Adding Copy

Add new UI copy by wrapping it with `t("...")`:

```tsx
t("Create project");
```

Then run the manual sync command from the repo root:

```bash
pnpm i18n:sync
```

This scans literal `t("...")` calls, string literals inside conditional `t(...)` calls, `i18n_*` object fields, `*TranslationKey` fields, and `I18N`-named local maps. It rewrites each locale's `translations.ts` as a flat object:

```ts
export default {
  "Create project": "",
} as const;
```

For English, the value is the same as the key. For non-English locales, new values are empty placeholders until translated.

The sync command is manual. It does not run during `pnpm build`.
It formats generated locale files before exiting.

## Translating Missing Values

After running `pnpm i18n:sync`, use the LLM translation command to fill empty placeholders:

```bash
pnpm i18n:translate -- --provider openai --model "$MODEL" --api-key "$OPENAI_API_KEY"
```

If `--languages` is omitted, all supported non-English languages are translated.

Translate selected languages:

```bash
pnpm i18n:translate -- --provider openai --model "$MODEL" --api-key "$OPENAI_API_KEY" --languages fr,es,ja
```

Use Fireworks:

```bash
pnpm i18n:translate -- --provider fireworks --model "$MODEL" --api-key "$FIREWORKS_API_KEY"
```

Supported options:

```bash
--provider openai|fireworks
--api-key <key>
--model <model>
--base-url <openai-compatible-chat-completions-url>
--languages fr,es,ja
--limit 100
--batch-size 10
--request-timeout-ms 180000
--retry-count 2
--retry-delay-ms 5000
--continue-on-error
--validation-report [path]   consume an i18n:validate report (default packages/i18n/i18n-validation-report.json)
--skip-empty                 skip empty-placeholder filling (apply validation report only)
--dry-run
--skip-readme
```

The script logs progress for each batch and writes successful batches to disk immediately. If a provider times out,
rerun the same command to continue from the remaining empty placeholders. Use `--continue-on-error` to skip failed
batches during a large run and leave those placeholders empty for a later retry.
Generated locale files and translated README files are formatted before the command exits.

Environment variables:

```bash
I18N_TRANSLATION_PROVIDER
I18N_TRANSLATION_API_KEY
I18N_TRANSLATION_MODEL
I18N_TRANSLATION_BASE_URL
I18N_TRANSLATION_LANGUAGES
I18N_TRANSLATION_LIMIT
I18N_TRANSLATION_BATCH_SIZE
I18N_TRANSLATION_REQUEST_TIMEOUT_MS
I18N_TRANSLATION_RETRY_COUNT
I18N_TRANSLATION_RETRY_DELAY_MS
I18N_TRANSLATION_CONTINUE_ON_ERROR
I18N_TRANSLATION_SKIP_README
OPENAI_API_KEY
FIREWORKS_API_KEY
```

The translator only fills empty strings in target locale `translations.ts` files. It uses the source English message id as the source text and rejects model output that changes ICU MessageFormat argument names or argument types.

This package README is developer-facing and kept English-only — the translator does not generate per-locale copies of it.

## Validating Existing Translations

`i18n:translate` only fills _empty_ placeholders — it never re-checks translations that already have a value, so a wrong
or stale existing translation is kept silently. `i18n:validate` is a manual, LLM-backed QA pass over those existing
values. It is deliberately **not** wired into any automation.

```bash
pnpm i18n:validate -- --provider openai --model "$MODEL" --api-key "$OPENAI_API_KEY"
```

What it does:

- Reviews every **non-empty** translation (empty placeholders are `i18n:translate`'s job).
- Flags **only genuinely wrong** translations — wrong meaning, mistranslation, untranslated English, wrong language, or
  broken/missing ICU placeholders. It is prompted to **skip** translations that are merely "could be phrased better";
  stylistic preferences are not reported.
- **Never edits locale files.** It writes a JSON report (default `packages/i18n/i18n-validation-report.json`) where each
  issue records `{ locale, key, current, reason, suggestion }`.

Then feed the report back to `i18n:translate` to apply fixes. The translator does **not** blindly accept the
suggestions: for every flagged entry it asks the model to independently judge whether the current value is actually
wrong, and only writes a placeholder-safe correction when the verdict is "incorrect". Use `--skip-empty` to apply
corrections only (without also filling empty placeholders):

```bash
pnpm i18n:translate -- --provider openai --model "$MODEL" --api-key "$OPENAI_API_KEY" \
  --validation-report packages/i18n/i18n-validation-report.json --skip-empty
```

A translation edited since the report was generated is skipped (never silently reverted). Options mirror
`i18n:translate` (`--languages`, `--limit`, `--batch-size`, `--out <path>`, `--dry-run`, …) and the same provider
environment variables apply (`I18N_TRANSLATION_*`, `OPENAI_API_KEY`, `FIREWORKS_API_KEY`). Validation-only overrides use
the `I18N_VALIDATION_*` prefix (e.g. `I18N_VALIDATION_OUT`, `I18N_VALIDATION_LIMIT`).

## Recommended Workflow

1. Add or update UI code with `t("Source English copy")`.
2. Run `pnpm i18n:sync`.
3. Review new empty placeholders in `packages/i18n/src/locales/*/translations.ts`.
4. Run `pnpm i18n:translate -- --provider openai --model "$MODEL"` or translate manually.
5. _(Optional, manual)_ Run `pnpm i18n:validate -- --provider openai --model "$MODEL"` to QA existing translations, then
   apply with `pnpm i18n:translate -- --validation-report packages/i18n/i18n-validation-report.json --skip-empty …`.
6. Review the diff before committing, especially ICU placeholders such as `{count}`, plural blocks, and translated README command examples.
7. Run:

```bash
pnpm --filter @pi-dash/i18n check:format
pnpm --filter @pi-dash/i18n check:types
pnpm --filter @pi-dash/i18n check:lint
```
