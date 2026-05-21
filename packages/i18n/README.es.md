# @pi-dash/i18n

Paquete compartido de traducciones de interfaz para Pi Dash.

## Almacenamiento de traducciones

Las traducciones viven en `packages/i18n/src/locales/<language>/`.

Cada locale es un objeto TypeScript, no JSON:

```ts
export default {
  common: {
    save: "Save",
  },
} as const;
```

El código de UI lee traducciones con claves de ruta separadas por puntos:

```tsx
const { t } = useTranslation();

return <button>{t("common.save")}</button>;
```

`t("...")` no crea claves en tiempo de ejecución. Las claves faltantes recurren al inglés y luego a la propia cadena de la clave. Los valores de cadena vacía se tratan como faltantes, por lo que los placeholders sin traducir no muestran UI en blanco.

## Agregar Claves

Agrega nuevo texto de UI envolviéndolo con `t("...")`:

```tsx
t("new_feature.title");
```

Luego ejecuta el comando manual de sincronización desde la raíz del repositorio:

```bash
pnpm i18n:sync
```

Esto escanea las llamadas literales `t("...")` y agrega las claves faltantes como placeholders de cadena vacía en el `translations.ts` de cada locale.

El comando de sincronización es manual. No se ejecuta durante `pnpm build`.

## Traducir Valores Faltantes

Después de ejecutar `pnpm i18n:sync`, usa el comando de traducción con LLM para completar los placeholders vacíos:

```bash
pnpm i18n:translate -- --provider openai --model "$MODEL" --api-key "$OPENAI_API_KEY"
```

Si se omite `--languages`, se traducen todos los idiomas compatibles que no sean inglés.

Traducir idiomas seleccionados:

```bash
pnpm i18n:translate -- --provider openai --model "$MODEL" --api-key "$OPENAI_API_KEY" --languages fr,es,ja
```

Usar Fireworks:

```bash
pnpm i18n:translate -- --provider fireworks --model "$MODEL" --api-key "$FIREWORKS_API_KEY"
```

Opciones compatibles:

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

Variables de entorno:

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

El traductor solo completa cadenas vacías en los archivos `translations.ts` del locale destino. Usa el locale inglés fusionado como texto fuente y pide al modelo preservar los placeholders de ICU MessageFormat.

El mismo comando también actualiza los README traducidos usando `README.md` como fuente inglés. La traducción de README se mantiene actualmente para:

- `README.es.md`
- `README.zh-CN.md`

Usa `--skip-readme` o `I18N_TRANSLATION_SKIP_README=1` para traducir solo los placeholders de locale.

## Flujo Recomendado

1. Agrega o actualiza código de UI con `t("some.key")`.
2. Ejecuta `pnpm i18n:sync`.
3. Revisa los nuevos placeholders vacíos en `packages/i18n/src/locales/*/translations.ts`.
4. Ejecuta `pnpm i18n:translate -- --provider openai --model "$MODEL"` o traduce manualmente.
5. Revisa el diff antes de hacer commit, especialmente los placeholders ICU como `{count}`, los bloques plurales y los ejemplos de comandos en README traducidos.
6. Ejecuta:

```bash
pnpm --filter @pi-dash/i18n check:format
pnpm --filter @pi-dash/i18n check:types
pnpm --filter @pi-dash/i18n check:lint
```
