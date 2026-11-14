// Runs only the fixture maker for sshee-app's model tests, never as part of `npm test`:
//   npx vitest run --config crates/sshee-app/tests/fixtures/gen/vitest.config.ts

import { fileURLToPath } from "node:url";
import { defineConfig } from "vitest/config";

export default defineConfig({
  root: fileURLToPath(new URL("../../../../../", import.meta.url)),
  test: {
    include: ["crates/sshee-app/tests/fixtures/gen/*.gen.ts"],
    environment: "node",
    testTimeout: 120_000,
  },
});
