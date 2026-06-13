# @pi-dash/i18n

Shared UI translation package for Pi Dash.

The open-source build **ships English only**. The translation machinery (the
`t()` runtime, the language picker, ICU MessageFormat support) is fully present
— so adding more languages is a matter of dropping in locale files, no code
changes required. See [Adding a language](#adding-a-language) below.

## How translation works

UI code uses the source English text as the message id:

```tsx
const { t } = useTranslation();

return <button>{t("Save changes")}</button>;
```

Each locale is a TypeScript object (not JSON) mapping that id to a translated
string:

```ts
// packages/i18n/src/locales/fr/translations.ts
export default {
  "Save changes": "Enregistrer les modifications",
} as const;
```

Missing or empty target translations fall back to English by returning the
source message id, so untranslated keys always render readable English. ICU
MessageFormat parameters are supported:

```tsx
t("Delete {count, plural, one {# work item} other {# work items}}", { count });
```

## Adding a language

The set of available languages is **derived from the `locales` map** in
[`src/locales/index.ts`](./src/locales/index.ts) — add an entry there and it
appears in the language picker automatically (`SUPPORTED_LANGUAGES` in
`src/constants/language.ts` reads from that map). To add, e.g., German:

1. Create `src/locales/de/` with the four locale files mirroring `en/`'s keys:
   `translations.ts`, `accessibility.ts`, `editor.ts`, `empty-state.ts`. Each is
   `export default { "<english id>": "<translation>" } as const;` — start from
   the English files and translate the values (leave a value empty to fall back
   to English).
2. Register the locale in `src/locales/index.ts`:

   ```ts
   export const locales = {
     en: {
       /* … */
     },
     de: {
       translations: () => import("./de/translations"),
       accessibility: () => import("./de/accessibility"),
       editor: () => import("./de/editor"),
       "empty-state": () => import("./de/empty-state"),
     },
   };
   ```

3. If the code isn't already in `TLanguage`, add it to
   [`src/types/language.ts`](./src/types/language.ts); a display label can be
   added to `LANGUAGE_LABELS` in `src/constants/language.ts` (it falls back to
   the language code if omitted).

That's it — `pnpm --filter @pi-dash/i18n build` and the new language is live.

> **Note:** the upstream multi-language locales and the key-sync / machine-
> translation tooling that maintains them are not part of this open-source
> repository. They live in the Pi Dash Cloud overlay and are layered on top at
> build time. Self-hosters maintain their own locale files as described above.
