import { nodeResolve } from "@rollup/plugin-node-resolve";

export default {
  input: "cm-entry.mjs",
  output: {
    file: "codemirror.bundle.js",
    format: "es",
    // Left unminified on purpose: a checked-in artifact should stay greppable.
    generatedCode: { preset: "es2015" },
  },
  plugins: [nodeResolve()],
};
