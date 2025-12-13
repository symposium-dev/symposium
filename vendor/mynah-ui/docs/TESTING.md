# Testing MynahUI
To ensure both visual and logical correctness in MynahUI, we have implemented two types of tests:

## 1. Unit tests (Jest)
We use the Jest library for unit testing. These test individual components and smaller parts of the whole system. You can run the unit tests using `npm run tests:unit`. We currently have tests for:
* The main MynahUI (in `src/__test__/main.spec.ts`)
* And the components of MynahUI (in `src/components/__test__`)

The `tests:unit` script also generates a `/coverage` folder in the root of the workspace, which includes an Istanbul coverage report. The current coverage at the time of writing is only around 51%, so there is definitely room for improvement.

## 2. E2E tests (Playwright)
Testing the full system from start to finish, simulating real user scenarios. We use Playwright for E2E testing, specifically visual regression testing with snapshots.

Currently, (at the time of writing) there are **58** tests, running on both *Chromium* and *Webkit*, so **116** tests in total.

All of these tests are defined in `ui-tests/__test__/main.spec.ts`, and their specific flows can be found under `ui-tests/__test__/flows/`.

### Adding a new E2E test
Adding a new test here is as simple as adding a new `test()` call in the `main.spec.ts`, e.g.:

```typescript
test('should do something', async ({ page }) => {
   await testSomething(page);
});
```

A new test flow should then be added to `ui-tests/__test__/flows/`. A basic flow looks something like this:

```typescript
export const testSomething = async (page: Page, skipScreenshots?: boolean): Promise<void> => {
  if (skipScreenshots !== true) {
    expect(await page.screenshot()).toMatchSnapshot();
  }
};
```

### Running E2E tests
Running the E2E tests has to be done in a Dockerized environment,  otherwise generated snapshots might slightly differ based on the environment (e.g. OS) in which they were generated. This means that having Docker desktop installed is a requirement for running the E2E tests.

Simply run all the e2e tests through the command `npm run tests:e2e`. Under the hood, this will run:
`npm run docker:clean && npm run docker:build && npm run docker:run`.

To specifically run only the chromium or webkit tests, you can use the `npm run tests:e2e:chromium` and `npm run tests:e2e:webkit` commands.

### Extracting results & updating snapshots
To extract the results from the Docker container, you should run `npm run docker:extract`. This will add a results folder to `ui-tests/__results__`. This folder contains the following:

```python
__results__:

  __reports__: # Reports of failed tests
    ...
    junit.xml # JUnit report of the test results, to be used by the GitHub test reporter
    .last-run.json # JSON formatted overview of the final status and an array of failed test IDs
    
  __snapshots__ # All snapshots that were used to run the tests
    chromium: # All the chromium snapshots, with subfolders for each test
    webkit: # All the webkit snapshots, with subfolders for each test
```

In case of any failed tests, the __reports__ directory will contain a subfolder for each of these failed tests, e.g.:
```python
main-Open-MynahUI-Feedback-form-should-cancel-feedback-form-chromium:
  snapshot-actual.png # the real output
  snapshot-diff.png # the differing pixels between actual and expected
  snapshot-expected.png # the expected output (= the provided golden snapshot)
  error-context.md # some context on why the test failed
  trace.zip # the traces, usable on the Playwright Trace Viewer website
```

The `error-context.md` will provide some context about why the test failed. Additionally, the `trace.zip` can be used on https://trace.playwright.dev/ to visually watch back the test’s execution and see what went wrong.

In case you now want any golden snapshot to be updated, you should take the `snapshot-actual.png` from this folder and replace the one in `ui-tests/__snapshots__/*THE_TEST'S_FOLDER*` with the new (actual) one. This process is slightly tedious on purpose, with the goal of encouraging extra carefulness when updating the golden snapshots.