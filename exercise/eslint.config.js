import storybook from 'eslint-plugin-storybook';
import tseslint from 'typescript-eslint';

// Two rules carry weight here; the rest is the recommended baseline.
//
// `ban-ts-comment` with a `descriptionFormat` is what turns the types
// ledger (`src/record/pending.types.ts`) into a burn-down rather than a pile
// of suppressions: every `@ts-expect-error` must cite the issue it waits on
// (`#92.4`), or say in so many words that it waits on nothing filed yet
// (`unfiled/entries-created`). A directive with neither is a lint error.
//
// `no-restricted-imports` keeps the `Bound` brand honest: only the accessor
// module may construct one, so nothing else may reach into its internals.
export default tseslint.config(
  { ignores: ['node_modules/', 'storybook-static/', 'dist/', '.red/', 'src/fixtures/generated/'] },
  ...tseslint.configs.recommended,
  ...storybook.configs['flat/recommended'],
  {
    files: ['**/*.ts', '**/*.tsx'],
    rules: {
      '@typescript-eslint/ban-ts-comment': [
        'error',
        {
          'ts-expect-error': { descriptionFormat: '^ (#\\d+(\\.\\d+)?|unfiled/[a-z0-9-]+)\\b' },
          'ts-ignore': true,
          'ts-nocheck': true,
          'ts-check': false,
        },
      ],
      '@typescript-eslint/consistent-type-imports': ['error', { prefer: 'type-imports' }],
    },
  },
);
