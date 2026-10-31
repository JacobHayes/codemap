import js from '@eslint/js';
import globals from 'globals';
import tseslint from 'typescript-eslint';

// A colour is a token; the one literal lives in src/palette.ts for canvases (DESIGN, Tokens).
const hexColour = String.raw`^#[0-9a-fA-F]{3,8}$`;

export default tseslint.config(
  { ignores: ['dist', 'src/generated'] },
  js.configs.recommended,
  tseslint.configs.strictTypeChecked,
  {
    languageOptions: {
      globals: { ...globals.browser },
      parserOptions: { projectService: true, tsconfigRootDir: import.meta.dirname },
    },
    rules: {
      '@typescript-eslint/no-explicit-any': 'error',
      'max-lines-per-function': ['error', { max: 70, skipBlankLines: true, skipComments: true }],
      'no-restricted-syntax': [
        'error',
        {
          selector: `Literal[value=/${hexColour}/]`,
          message: 'Use a design token, or add the colour to src/palette.ts.',
        },
      ],
    },
  },
  { files: ['**/*.js'], extends: [tseslint.configs.disableTypeChecked] },
  { files: ['src/palette.ts'], rules: { 'no-restricted-syntax': 'off' } },
  { files: ['*.config.{js,ts}'], languageOptions: { globals: globals.node } },
);
