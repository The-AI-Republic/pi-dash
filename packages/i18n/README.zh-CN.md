# @pi-dash/i18n

Pi Dash 共享的 UI 翻译包。

## 翻译存储

翻译文件位于 `packages/i18n/src/locales/<language>/`。

每个 locale 都是一个 TypeScript 对象，不是 JSON：

```ts
export default {
  common: {
    save: "Save",
  },
} as const;
```

UI 代码通过点路径 key 读取翻译：

```tsx
const { t } = useTranslation();

return <button>{t("common.save")}</button>;
```

`t("...")` 不会在运行时创建 key。缺失的 key 会先回退到英文，再回退到 key 字符串本身。空字符串会被视为缺失值，因此未翻译的占位符不会渲染成空白 UI。

## 添加 Key

添加新的 UI 文案时，用 `t("...")` 包裹它：

```tsx
t("new_feature.title");
```

然后在仓库根目录运行手动同步命令：

```bash
pnpm i18n:sync
```

该命令会扫描字面量 `t("...")` 调用，并把缺失的 key 作为空字符串占位符添加到每个 locale 的 `translations.ts`。

如果新 key 与现有对象路径或字符串路径冲突，同步命令会打印冲突 key 和调用位置，然后以错误退出。请重命名 key 来避免冲突。只有在你明确想跳过这些 key 时，才使用 `I18N_SYNC_ALLOW_CONFLICTS=1`。

同步命令是手动命令。它不会在 `pnpm build` 期间运行。

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
--batch-size 30
--request-timeout-ms 180000
--retry-count 2
--retry-delay-ms 2000
--dry-run
--skip-readme
```

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
I18N_TRANSLATION_SKIP_README
OPENAI_API_KEY
FIREWORKS_API_KEY
```

翻译器只会填充目标 locale `translations.ts` 文件中的空字符串。它使用合并后的英文 locale 作为源文本，并拒绝会改变 ICU MessageFormat 参数名称或参数类型的模型输出。

同一个命令也会使用 `README.md` 作为英文源文档来更新已翻译的 README。当前维护的 README 翻译包括：

- `README.es.md`
- `README.zh-CN.md`

使用 `--skip-readme` 或 `I18N_TRANSLATION_SKIP_README=1` 可以只翻译 locale 占位符。

## 推荐流程

1. 使用 `t("some.key")` 添加或更新 UI 代码。
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
