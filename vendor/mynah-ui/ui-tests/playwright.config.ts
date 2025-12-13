import { defineConfig } from '@playwright/test';
import { DEFAULT_VIEWPORT } from './__test__/helpers';

const isHeadless = process.env.HEADLESS !== 'false';
const updateSnapshots = process.env.UPDATE_SNAPSHOTS === 'true';

export default defineConfig({
  testDir: './__test__',
  testMatch: [ '**/?(*.)+(spec|test).[t]s' ],
  testIgnore: [ '/node_modules/', 'dist', 'src' ],
  timeout: 30000,
  snapshotDir: './__snapshots__',
  outputDir: './__results__/__reports__',
  snapshotPathTemplate: '{snapshotDir}{/projectName}/{testName}/{arg}{ext}',
  fullyParallel: true,
  use: {
    headless: isHeadless,
    trace: 'retain-on-failure', // Capture trace only on failure
    viewport: DEFAULT_VIEWPORT, // Enforce the default viewport
  },
  expect: {
    toMatchSnapshot: {
      maxDiffPixelRatio: 0.01,
    },
  },
  projects: [
    {
      name: 'chromium',
      use: {
        browserName: 'chromium',
        launchOptions: {
          args: [
            '--disable-features=VizDisplayCompositor',
            '--disable-dev-shm-usage',
            '--no-sandbox',
          ],
        },
      }
    },
    { name: 'webkit', use: { browserName: 'webkit' } },
  ],
  updateSnapshots: updateSnapshots ? 'all' : 'missing',
  reporter: [
    [ 'list' ],
    [ 'junit', { outputFile: './__results__/__reports__/junit.xml' } ],
  ],
});
