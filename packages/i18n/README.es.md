# @pi-dash/i18n

Paquete compartido de traducciones de interfaz para Pi Dash.

## Almacenamiento de traducciones

Las traducciones se encuentran en `packages/i18n/src/locales/<idioma>/`.

Cada locale es un objeto TypeScript, no JSON:

```ts
export default {
  "Save changes": "Guardar cambios",
} as const;
```

El código de la interfaz usa el texto fuente en inglés como identificador del mensaje:

```tsx
const { t } = useTranslation();

return <button>{t("Save changes")}</button>;
```

Las traducciones destino faltantes o vacías recurren al inglés devolviendo el identificador del mensaje fuente. Se admiten parámetros de ICU MessageFormat:

```tsx
t("Delete {count, plural, one {# work item} other {# work items}}", { count });
```

## Agregar texto

Agrega nuevo texto de interfaz envolviéndolo con `t("...")`:

```tsx
t("Create project");
```

Luego ejecuta el comando manual de sincronización desde la raíz del repositorio:

```bash
pnpm i18n:sync
```

Esto escanea las llamadas literales `t("...")`, los literales de cadena dentro de llamadas condicionales `t(...)`, los campos de objeto `i18n_*`, los campos `*TranslationKey` y los mapas locales nombrados `I18N`. Reescribe el archivo `translations.ts` de cada locale como un objeto plano:

```ts
export default {
  "Create project": "",
} as const;
```

Para inglés, el valor es igual a la clave. Para locales que no son inglés, los valores nuevos son marcadores de posición vacíos hasta que se traduzcan.

El comando de sincronización es manual. No se ejecuta durante `pnpm build`.

## Traducir valores faltantes

Después de ejecutar `pnpm i18n:sync`, usa el comando de traducción con LLM para llenar los marcadores de posición vacíos:

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
--batch-size 10
--request-timeout-ms 180000
--retry-count 2
--retry-delay-ms 5000
--continue-on-error
--dry-run
--skip-readme
```

El script registra el progreso de cada lote y escribe los lotes exitosos en disco inmediatamente. Si un proveedor agota el tiempo de espera, vuelve a ejecutar el mismo comando para continuar desde los marcadores de posición vacíos restantes. Usa `--continue-on-error` para omitir lotes fallidos durante una ejecución grande y dejar esos marcadores de posición vacíos para un reintento posterior.

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
I18N_TRANSLATION_CONTINUE_ON_ERROR
I18N_TRANSLATION_SKIP_README
OPENAI_API_KEY
FIREWORKS_API_KEY
```

El traductor solo llena cadenas vacías en los archivos `translations.ts` del locale destino. Usa el identificador del mensaje fuente en inglés como texto fuente y rechaza la salida del modelo que cambie los nombres de argumentos o tipos de argumentos de ICU MessageFormat.

El mismo comando también actualiza los archivos README traducidos usando `README.md` como fuente en inglés. La traducción de README se mantiene actualmente para:

- `README.es.md`
- `README.zh-CN.md`

Usa `--skip-readme` o `I18N_TRANSLATION_SKIP_README=1` para traducir solo los marcadores de posición del locale.

## Flujo de trabajo recomendado

1. Agrega o actualiza código de interfaz con `t("Texto fuente en inglés")`.
2. Ejecuta `pnpm i18n:sync`.
3. Revisa los nuevos marcadores de posición vacíos en `packages/i18n/src/locales/*/translations.ts`.
4. Ejecuta `pnpm i18n:translate -- --provider openai --model "$MODEL"` o traduce manualmente.
5. Revisa el diff antes de hacer commit, especialmente los marcadores de posición ICU como `{count}`, los bloques plurales y los ejemplos de comandos en README traducidos.
6. Ejecuta:

```bash
pnpm --filter @pi-dash/i18n check:format
pnpm --filter @pi-dash/i18n check:types
pnpm --filter @pi-dash/i18n check:lint
```
