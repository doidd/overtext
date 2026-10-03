import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "tests/render",
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: 0,
  workers: 2,
  reporter: "list",
  use: { browserName: "chromium", headless: true },
  projects: [1, 1.25, 1.5, 2].map(scale => ({
    name: `scale-${scale * 100}`,
    use: { deviceScaleFactor: scale },
  })),
});
