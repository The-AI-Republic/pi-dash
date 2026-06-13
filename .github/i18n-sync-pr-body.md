Automated by the **i18n key-sync** workflow after a merge to `main`.

`pnpm i18n:sync` regenerated the translation **key set** — it adds new message
ids as **empty placeholders** and removes orphaned ones. It does **not**
translate: non-English locales are blank (or fall back to English) until filled.

### Next step

Fill in the translations, review them, and push to this branch:

```bash
pnpm i18n:translate   # needs I18N_TRANSLATION_API_KEY + I18N_TRANSLATION_MODEL
```

Then merge.

> Heads-up: machine translation can miss established terminology (e.g. a product
> term that already has a fixed translation in some locales). Give the
> non-English values a quick look before merging.
