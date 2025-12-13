module.exports = {
    env: {
        browser: true,
        es2021: true,
        node: true,
    },
    extends: ['standard-with-typescript'],
    parser: '@typescript-eslint/parser',
    parserOptions: {
        tsconfigRootDir: __dirname,
        ecmaVersion: 'latest',
        sourceType: 'module',
        project: ['./tsconfig.json', './ui-tests/tsconfig.json'],
    },
    plugins: ['@typescript-eslint', 'prettier'],
    rules: {
        'no-case-declarations': 'off',
        '@typescript-eslint/no-floating-promises': 'off',
        '@typescript-eslint/semi': [2, 'always'],
        'comma-dangle': [2, 'only-multiline'],
        'array-bracket-spacing': [2, 'always'],
        'no-useless-call': 'off',
        '@typescript-eslint/member-delimiter-style': [
            'error',
            {
                multiline: {
                    delimiter: 'semi',
                    requireLast: true,
                },
                singleline: {
                    delimiter: 'semi',
                    requireLast: false,
                },
            },
        ],
    },
};
