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
--dry-run
--skip-readme
```

The script logs progress for each batch and writes successful batches to disk immediately. If a provider times out,
rerun the same command to continue from the remaining empty placeholders. Use `--continue-on-error` to skip failed
batches during a large run and leave those placeholders empty for a later retry.

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

The same command also updates translated README files from `README.md` as the English source. README translation is currently maintained for:

- `README.es.md`
- `README.zh-CN.md`

Use `--skip-readme` or `I18N_TRANSLATION_SKIP_README=1` to translate locale placeholders only.

## Recommended Workflow

1. Add or update UI code with `t("Source English copy")`.
2. Run `pnpm i18n:sync`.
3. Review new empty placeholders in `packages/i18n/src/locales/*/translations.ts`.
4. Run `pnpm i18n:translate -- --provider openai --model "$MODEL"` or translate manually.
5. Review the diff before committing, especially ICU placeholders such as `{count}`, plural blocks, and translated README command examples.
6. Run:

```bash
pnpm --filter @pi-dash/i18n check:format
pnpm --filter @pi-dash/i18n check:types
pnpm --filter @pi-dash/i18n check:lint
```
