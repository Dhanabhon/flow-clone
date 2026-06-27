export default [
  {
    ignores: [
      "dist/**",
      "node_modules/**",
      "src/**",
      "src-tauri/**",
      "*.d.ts",
      "*.tsbuildinfo",
    ],
  },
  {
    files: ["*.js"],
    languageOptions: {
      ecmaVersion: "latest",
      sourceType: "module",
    },
  },
];
