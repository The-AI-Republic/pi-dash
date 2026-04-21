# Contributing to Pi Dash

Thank you for your interest in contributing to Pi Dash — an open-source AI agent orchestration platform built for **As Coding (asynchronous vibe coding)**. All kinds of contributions are valuable to us. This guide covers how to get started quickly.

## Submitting an issue

Before submitting a new issue, please search the [issues](https://github.com/The-AI-Republic/pi-dash/issues) tab or [discussions](https://github.com/The-AI-Republic/pi-dash/discussions). An existing thread might already address your question or inform you of workarounds.

When reporting a bug, please provide a minimal reproduction scenario using a repository or [Gist](https://gist.github.com/). A reproducible case lets us investigate without lengthy back-and-forth. Include:

- Steps to reproduce
- Expected vs actual behavior
- Relevant versions (Node.js, Python, Docker, browser, etc.)
- 3rd-party libraries or integrations involved

Without a minimal reproduction, we may not be able to investigate, and the issue might not be resolved.

You can open a new issue [here](https://github.com/The-AI-Republic/pi-dash/issues/new).

### Naming conventions for issues

Use a clear, concise title following this format:

- For bugs: `Bug: [short description]`
- For features: `Feature: [short description]`
- For improvements: `Improvement: [short description]`
- For documentation: `Docs: [short description]`

**Examples:**

- `Bug: Agent task status not updating after completion`
- `Docs: Clarify RAM requirement for local setup`
- `Feature: Support custom agent timeout per task`

## Local development

If you haven't already set up Pi Dash locally, follow the [Installation](./README.md#-installation) steps in README.md first.

Once Pi Dash is running, here's what you need to know for development:

### Project structure

The project is a monorepo with the backend API (Django, in `apps/api`) and frontend in a single repo.

### Development workflow

1. Create a feature branch from `main`
2. Make your changes
3. Run linting and formatting before committing:

```bash
pnpm lint
pnpm format
```

4. Ensure tests pass
5. Submit a pull request

## Missing a feature?

If a feature is missing, you can request one by opening a [new issue](https://github.com/The-AI-Republic/pi-dash/issues/new). If you'd like to implement it yourself, please submit a proposal issue first so we can discuss the approach.

## Coding guidelines

To ensure consistency throughout the source code:

- All features or bug fixes must include tests (unit or integration).
- We lint with [OxLint](https://oxc.rs/docs/guide/usage/linter) using the shared `.oxlintrc.json` and format with [oxfmt](https://oxc.rs/docs/guide/usage/formatter) using `.oxfmtrc.json`.

## Ways to contribute

- Try Pi Dash (cloud or self-hosted) and give feedback
- Add new agent integrations or connectors
- Add or update translations
- Help with open [issues](https://github.com/The-AI-Republic/pi-dash/issues) or [create your own](https://github.com/The-AI-Republic/pi-dash/issues/new/choose)
- Share your thoughts and suggestions with us
- Help create tutorials and blog posts
- Request a feature by submitting a proposal
- Report a bug
- **Improve documentation** — fix incomplete or missing [docs](https://airepublic.com/docs), bad wording, examples or explanations

## Contributing to language support

This guide helps contributors add or update translations in the application.

### Understanding translation structure

#### File organization

Translations are organized by language in the locales directory. Each language has its own folder containing JSON files:

```
packages/i18n/src/locales/
    ├── en/
    │   ├── core.json       # Critical translations
    │   └── translations.json
    ├── fr/
    │   └── translations.json
    └── [language]/
        └── translations.json
```

#### Nested structure

We use nested keys to keep translations organized:

```json
{
  "agent": {
    "status": {
      "running": "Running",
      "completed": "Completed",
      "failed": "Failed"
    }
  },
  "task": {
    "label": "Task",
    "title": {
      "label": "Task title"
    }
  }
}
```

### Translation formatting guide

We use [IntlMessageFormat](https://formatjs.github.io/docs/intl-messageformat/) for dynamic content such as variables and pluralization:

- **Simple variables**

  ```json
  {
    "greeting": "Hello, {name}!"
  }
  ```

- **Pluralization**
  ```json
  {
    "items": "{count, plural, one {task} other {tasks}}"
  }
  ```

### Contributing guidelines

#### Updating existing translations

1. Locate the key in `locales/<language>/translations.json`.
2. Update the value while keeping the key structure intact.
3. Preserve any existing ICU formats (e.g., variables, pluralization).

#### Adding new translation keys

1. When introducing a new key, add it to **all** language files (use English as a placeholder if a translation isn't available yet).
2. Keep the nesting structure consistent across all languages.
3. If the new key requires dynamic content, apply ICU format uniformly across all languages.

### Adding new languages

1. **Update type definitions**

```ts
// packages/i18n/src/types/language.ts
export type TLanguage = "en" | "fr" | "your-lang";
```

2. **Add language configuration**

```ts
// packages/i18n/src/constants/language.ts
export const SUPPORTED_LANGUAGES: ILanguageOption[] = [
  { label: "English", value: "en" },
  { label: "Your Language", value: "your-lang" },
];
```

3. **Create translation files**
   1. Create a new folder under locales (e.g., `locales/your-lang/`).
   2. Add a `translations.json` file inside it.
   3. Copy the structure from an existing translation file and translate all keys.

4. **Update import logic**

```ts
private importLanguageFile(language: TLanguage): Promise<any> {
  switch (language) {
    case "your-lang":
      return import("../locales/your-lang/translations.json");
    // ...
  }
}
```

### Quality checklist

Before submitting your contribution:

- All translation keys exist in every language file
- Nested structures match across all language files
- ICU message formats are correctly implemented
- All languages load without errors in the application
- Dynamic values and pluralization work as expected
- No missing or untranslated keys

## Need help?

Questions, suggestions, and thoughts are most welcome. Reach us on [GitHub Discussions](https://github.com/The-AI-Republic/pi-dash/discussions) or visit [airepublic.com](https://airepublic.com/).
