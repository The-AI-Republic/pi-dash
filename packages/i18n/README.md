# @pi-dash/i18n

Shared UI translation package for Pi Dash.

## Translation Storage

Translations live under `packages/i18n/src/locales/<language>/`.

Each locale is a TypeScript object, not JSON:

```ts
export default {
  common: {
    save: "Save",
  },
} as const;
```

UI code reads translations with dot-path keys:

```tsx
const { t } = useTranslation();

return <button>{t("common.save")}</button>;
```

`t("...")` does not create keys at runtime. Missing keys fall back to English, then to the key string itself. Empty string values are treated as missing, so untranslated placeholders do not render blank UI.

## Adding Keys

Add new UI copy by wrapping it with `t("...")`:

```tsx
t("new_feature.title");
```

Then run the manual sync command from the repo root:

```bash
pnpm i18n:sync
```

This scans literal `t("...")` calls and adds missing keys as empty-string placeholders to each locale's `translations.ts`.

If a new key conflicts with an existing object or string path, the sync command prints the conflicting key and call sites, then exits with an error. Rename the key to avoid the conflict. Use `I18N_SYNC_ALLOW_CONFLICTS=1` only when you intentionally want to skip those keys.

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
--batch-size 30
--request-timeout-ms 180000
--retry-count 2
--retry-delay-ms 2000
--dry-run
--skip-readme
```

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
I18N_TRANSLATION_SKIP_README
OPENAI_API_KEY
FIREWORKS_API_KEY
```

The translator only fills empty strings in target locale `translations.ts` files. It uses the merged English locale as source text and rejects model output that changes ICU MessageFormat argument names or argument types.

The same command also updates translated README files from `README.md` as the English source. README translation is currently maintained for:

- `README.es.md`
- `README.zh-CN.md`

Use `--skip-readme` or `I18N_TRANSLATION_SKIP_README=1` to translate locale placeholders only.

## Recommended Workflow

1. Add or update UI code with `t("some.key")`.
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
