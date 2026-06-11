# @pi-dash/i18n

Pi Dash 共享的 UI 翻译包。

## 翻译存储

翻译文件位于 `packages/i18n/src/locales/<language>/`。

每个 locale 都是一个 TypeScript 对象，不是 JSON：

```ts
export default {
  "Save changes": "Enregistrer les modifications",
} as const;
```

UI 代码使用源英文文本作为消息 ID：

```tsx
const { t } = useTranslation();

return <button>{t("Save changes")}</button>;
```

缺失或空的目标翻译会回退到英文，即返回源消息 ID。支持 ICU MessageFormat 参数：

```tsx
t("Delete {count, plural, one {# work item} other {# work items}}", { count });
```

## 添加文案

添加新的 UI 文案时，用 `t("...")` 包裹它：

```tsx
t("Create project");
```

然后在仓库根目录运行手动同步命令：

```bash
pnpm i18n:sync
```

该命令会扫描字面量 `t("...")` 调用、条件 `t(...)` 中的字符串字面量、`i18n_*` 对象字段、`*TranslationKey` 字段以及 `I18N` 命名的本地映射。它会将每个 locale 的 `translations.ts` 重写为扁平对象：

```ts
export default {
  "Create project": "",
} as const;
```

对于英文 locale，值与键相同。对于非英文 locale，新值在翻译前为空占位符。

同步命令是手动命令。它不会在 `pnpm build` 期间运行。它会在退出前格式化生成的 locale 文件。

## 翻译缺失值

运行 `pnpm i18n:sync` 后，使用 LLM 翻译命令填充空占位符：

```bash
pnpm i18n:translate -- --provider openai --model "$MODEL" --api-key "$OPENAI_API_KEY"
```

如果省略 `--languages`，会翻译所有受支持的非英文语言。

翻译指定语言：

```bash
pnpm i18n:translate -- --provider openai --model "$MODEL" --api-key "$OPENAI_API_KEY" --languages fr,es,ja
```

使用 Fireworks：

```bash
pnpm i18n:translate -- --provider fireworks --model "$MODEL" --api-key "$FIREWORKS_API_KEY"
```

支持的选项：

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

脚本会记录每个批次的进度，并立即将成功的批次写入磁盘。如果提供者超时，重新运行相同的命令即可从剩余的空占位符继续。使用 `--continue-on-error` 可以在大规模运行中跳过失败的批次，将那些占位符留空以便稍后重试。

环境变量：

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

生成的 locale 文件和翻译后的 README 文件会在命令退出前进行格式化。

翻译器只会填充目标 locale `translations.ts` 文件中的空字符串。它使用源英文消息 ID 作为源文本，并拒绝会改变 ICU MessageFormat 参数名称或参数类型的模型输出。

同一个命令也会使用 `README.md` 作为英文源文档来更新已翻译的 README。当前维护的 README 翻译包括：

- `README.es.md`
- `README.zh-CN.md`

使用 `--skip-readme` 或 `I18N_TRANSLATION_SKIP_README=1` 可以只翻译 locale 占位符。

## 推荐流程

1. 使用 `t("Source English copy")` 添加或更新 UI 代码。
2. 运行 `pnpm i18n:sync`。
3. 检查 `packages/i18n/src/locales/*/translations.ts` 中新增的空占位符。
4. 运行 `pnpm i18n:translate -- --provider openai --model "$MODEL"`，或手动翻译。
5. 提交前检查 diff，特别是 `{count}` 这样的 ICU 占位符、plural 块以及已翻译 README 中的命令示例。
6. 运行：

```bash
pnpm --filter @pi-dash/i18n check:format
pnpm --filter @pi-dash/i18n check:types
pnpm --filter @pi-dash/i18n check:lint
```
