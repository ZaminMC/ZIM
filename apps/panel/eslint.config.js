// ESLint flat config. Style guide (docs/development/STYLE-GUIDE.md): strict
// TS, named exports only, hook rules, and import boundaries — the webview
// imports only the protocol client module, the bridge module, and UI code.
import eslint from "@eslint/js";
import tseslint from "typescript-eslint";
import importPlugin from "eslint-plugin-import";
import reactHooks from "eslint-plugin-react-hooks";

export default tseslint.config(
  { ignores: ["dist/", "node_modules/"] },
  eslint.configs.recommended,
  ...tseslint.configs.strictTypeChecked,
  {
    files: ["**/*.{ts,tsx}"],
    languageOptions: {
      parserOptions: {
        projectService: true,
        tsconfigRootDir: import.meta.dirname,
      },
    },
    plugins: {
      import: importPlugin,
      "react-hooks": reactHooks,
    },
    rules: {
      "import/no-default-export": "error",
      "react-hooks/rules-of-hooks": "error",
      "react-hooks/exhaustive-deps": "error",
      // Import boundaries: only the bridge layer talks to @tauri-apps; the
      // webview never touches node builtins or the ws package.
      "no-restricted-imports": [
        "error",
        {
          patterns: [
            {
              group: ["@tauri-apps/**"],
              message:
                "Only src/integration/tauri.ts may import the Tauri API; go through the integration seam.",
            },
            {
              group: ["ws", "node:*", "fs", "path", "net", "child_process"],
              message: "The webview does not import node-only modules.",
            },
          ],
        },
      ],
      "@typescript-eslint/consistent-type-imports": [
        "error",
        { fixStyle: "inline-type-imports" },
      ],
      "@typescript-eslint/no-unused-vars": [
        "error",
        { argsIgnorePattern: "^_", varsIgnorePattern: "^_" },
      ],
      // Numbers in messages and templates are legitimate; booleans are not.
      "@typescript-eslint/restrict-template-expressions": [
        "error",
        { allowNumber: true },
      ],
      // zustand's `() => set(...)` shorthand returns void by design.
      "@typescript-eslint/no-confusing-void-expression": "off",
      // Registry records are keyed by serverId / subscriptionId.
      "@typescript-eslint/no-dynamic-delete": "off",
    },
  },
  {
    // vitest config needs a default export per the tool contract.
    files: ["*.config.ts", "*.config.js"],
    rules: { "import/no-default-export": "off" },
  },
  {
    // The integration seam is the sanctioned importer of the desktop API
    // (its own header states the contract); every other module routes
    // through it.
    files: ["src/integration/tauri.ts"],
    rules: { "no-restricted-imports": "off" },
  },
  {
    files: ["**/*.test.ts", "**/*.test.tsx"],
    rules: {
      "@typescript-eslint/no-explicit-any": "off",
      "@typescript-eslint/no-unsafe-member-access": "off",
      "@typescript-eslint/no-unsafe-assignment": "off",
      "@typescript-eslint/require-await": "off",
      "@typescript-eslint/no-non-null-assertion": "off",
      "@typescript-eslint/non-nullable-type-assertion-style": "off",
    },
  },
);
