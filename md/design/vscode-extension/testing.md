# VSCode Extension Integration Testing Guide

## Table of Contents
1. [Overview](#overview)
2. [Testing Types](#testing-types)
3. [Setting Up Integration Tests](#setting-up-integration-tests)
4. [Writing Integration Tests](#writing-integration-tests)
5. [Testing Webviews](#testing-webviews)
6. [Advanced Testing Scenarios](#advanced-testing-scenarios)
7. [Testing Best Practices](#testing-best-practices)
8. [Debugging Tests](#debugging-tests)
9. [Common Patterns](#common-patterns)
10. [Tools and Libraries](#tools-and-libraries)

---

## Overview

VSCode extension testing involves multiple layers, with integration tests being crucial for verifying that your extension works correctly with the VSCode API in a real VSCode environment.

**Why Integration Tests Matter:**
- Unit tests can't verify VSCode API interactions
- Extensions can break due to VSCode API changes
- Manual testing doesn't scale as extensions grow
- Integration tests catch issues that unit tests miss

**Key Principle:** Follow the test pyramid - most tests should be fast unit tests, with a smaller number of integration tests for critical workflows.

---

## Testing Types

### Unit Tests
- Test pure logic in isolation
- No VSCode API required
- Fast and can run in any environment
- Use standard frameworks (Mocha, Jest, etc.)
- Good for: utility functions, data transformations, business logic

### Integration Tests
- Run inside a real VSCode instance (Extension Development Host)
- Have access to full VSCode API
- Test extension behavior with actual VSCode
- Slower but more realistic
- Good for: command execution, UI interactions, API integrations

### End-to-End Tests
- Automate the full VSCode UI using tools like WebdriverIO or Playwright
- Most complex to set up
- Test complete user workflows
- Good for: complex UIs, webviews, full user journeys

---

## Setting Up Integration Tests

### Option 1: Using @vscode/test-cli (Recommended)

The modern approach using the official VSCode test CLI.

**Installation:**
```bash
npm install --save-dev @vscode/test-cli @vscode/test-electron
```

**package.json configuration:**
```json
{
  "scripts": {
    "test": "vscode-test"
  }
}
```

**Create .vscode-test.js or .vscode-test.mjs:**
```javascript
import { defineConfig } from '@vscode/test-cli';

export default defineConfig({
  files: 'out/test/**/*.test.js',
  version: 'stable', // or 'insiders' or specific version like '1.85.0'
  workspaceFolder: './test-workspace',
  mocha: {
    ui: 'tdd',
    timeout: 20000
  }
});
```

**Run tests:**
```bash
npm test
```

### Option 2: Using @vscode/test-electron Directly

For more control over the test runner.

**Installation:**
```bash
npm install --save-dev @vscode/test-electron mocha
```

**Create src/test/runTest.ts:**
```typescript
import * as path from 'path';
import { runTests } from '@vscode/test-electron';

async function main() {
  try {
    // The folder containing the Extension Manifest package.json
    const extensionDevelopmentPath = path.resolve(__dirname, '../../');
    
    // The path to test runner
    const extensionTestsPath = path.resolve(__dirname, './suite/index');
    
    // Optional: specific workspace to open
    const testWorkspace = path.resolve(__dirname, '../../test-fixtures');
    
    // Download VS Code, unzip it and run the integration test
    await runTests({
      extensionDevelopmentPath,
      extensionTestsPath,
      launchArgs: [
        testWorkspace,
        '--disable-extensions' // Disable other extensions during testing
      ]
    });
  } catch (err) {
    console.error('Failed to run tests');
    process.exit(1);
  }
}

main();
```

**Create src/test/suite/index.ts (test runner):**
```typescript
import * as path from 'path';
import * as Mocha from 'mocha';
import { glob } from 'glob';

export function run(): Promise<void> {
  const mocha = new Mocha({
    ui: 'tdd',
    color: true,
    timeout: 20000
  });

  const testsRoot = path.resolve(__dirname, '.');

  return new Promise((resolve, reject) => {
    glob('**/**.test.js', { cwd: testsRoot }).then((files) => {
      // Add files to the test suite
      files.forEach(f => mocha.addFile(path.resolve(testsRoot, f)));

      try {
        // Run the mocha test
        mocha.run(failures => {
          if (failures > 0) {
            reject(new Error(`${failures} tests failed.`));
          } else {
            resolve();
          }
        });
      } catch (err) {
        reject(err);
      }
    }).catch((err) => {
      reject(err);
    });
  });
}
```

### Project Structure

```
your-extension/
├── src/
│   ├── extension.ts
│   └── test/
│       ├── runTest.ts
│       └── suite/
│           ├── index.ts
│           ├── extension.test.ts
│           └── other.test.ts
├── test-fixtures/          # Optional test workspace
│   └── sample-file.txt
├── .vscode/
│   └── launch.json         # Debug configuration
└── package.json
```

---

## Writing Integration Tests

### Basic Test Structure

```typescript
import * as assert from 'assert';
import * as vscode from 'vscode';

suite('Extension Test Suite', () => {
  vscode.window.showInformationMessage('Start all tests.');

  test('Sample test', () => {
    assert.strictEqual(-1, [1, 2, 3].indexOf(5));
    assert.strictEqual(-1, [1, 2, 3].indexOf(0));
  });

  test('Extension should be present', () => {
    assert.ok(vscode.extensions.getExtension('your-publisher.your-extension'));
  });

  test('Should register commands', async () => {
    const commands = await vscode.commands.getCommands(true);
    assert.ok(commands.includes('your-extension.yourCommand'));
  });
});
```

### Testing Commands

```typescript
test('Execute command should work', async () => {
  const result = await vscode.commands.executeCommand('your-extension.yourCommand');
  assert.ok(result);
  assert.strictEqual(result.status, 'success');
});
```

### Testing with Documents and Editors

```typescript
test('Should modify document', async () => {
  // Create a new document
  const doc = await vscode.workspace.openTextDocument({
    content: 'Hello World',
    language: 'plaintext'
  });

  // Open it in an editor
  const editor = await vscode.window.showTextDocument(doc);

  // Execute your command that modifies the document
  await vscode.commands.executeCommand('your-extension.formatDocument');

  // Assert the document was modified
  assert.strictEqual(doc.getText(), 'HELLO WORLD');

  // Clean up
  await vscode.commands.executeCommand('workbench.action.closeActiveEditor');
});
```

### Asynchronous Operations and Waiting

```typescript
function waitForCondition(
  condition: () => boolean,
  timeout: number = 5000,
  message?: string
): Promise<void> {
  return new Promise((resolve, reject) => {
    const startTime = Date.now();
    const interval = setInterval(() => {
      if (condition()) {
        clearInterval(interval);
        resolve();
      } else if (Date.now() - startTime > timeout) {
        clearInterval(interval);
        reject(new Error(message || 'Timeout waiting for condition'));
      }
    }, 50);
  });
}

test('Wait for extension activation', async () => {
  const extension = vscode.extensions.getExtension('your-publisher.your-extension');
  
  if (!extension!.isActive) {
    await extension!.activate();
  }

  await waitForCondition(
    () => extension!.isActive,
    5000,
    'Extension did not activate'
  );

  assert.ok(extension!.isActive);
});
```

### Testing Events

```typescript
test('Should trigger onDidChangeTextDocument', async () => {
  const doc = await vscode.workspace.openTextDocument({
    content: 'Test',
    language: 'plaintext'
  });

  let eventFired = false;
  const disposable = vscode.workspace.onDidChangeTextDocument(e => {
    if (e.document === doc) {
      eventFired = true;
    }
  });

  const editor = await vscode.window.showTextDocument(doc);
  await editor.edit(edit => {
    edit.insert(new vscode.Position(0, 0), 'Hello ');
  });

  await waitForCondition(() => eventFired, 2000);
  assert.ok(eventFired, 'Event should have fired');

  disposable.dispose();
});
```

---

## Testing Webviews

Testing webviews is challenging because they run in an isolated context. There are several approaches:

### Approach 1: Message-Based Testing (Recommended for Integration Tests)

**Extension Side - Add Test Hooks:**

```typescript
class ChatPanel {
  private panel: vscode.WebviewPanel;
  private messageHandlers: Map<string, (message: any) => void> = new Map();

  constructor(extensionUri: vscode.Uri) {
    this.panel = vscode.window.createWebviewPanel(
      'chat',
      'Chat',
      vscode.ViewColumn.One,
      {
        enableScripts: true,
        retainContextWhenHidden: true
      }
    );

    this.panel.webview.onDidReceiveMessage(message => {
      // Handle normal messages
      if (message.type === 'userMessage') {
        this.handleUserMessage(message.text);
      }
      
      // Handle test messages (only in test environment)
      if (process.env.VSCODE_TEST_MODE === 'true') {
        if (message.type === 'test:state') {
          const handler = this.messageHandlers.get('state');
          handler?.(message);
        }
      }
    });
  }

  // Public method for tests to get state
  public requestState(): Promise<any> {
    return new Promise((resolve) => {
      this.messageHandlers.set('state', (message) => {
        resolve(message.data);
        this.messageHandlers.delete('state');
      });
      this.panel.webview.postMessage({ type: 'test:getState' });
    });
  }

  // Method to send messages to webview
  public sendMessage(text: string) {
    this.handleUserMessage(text);
  }

  private handleUserMessage(text: string) {
    // Your normal message handling logic
    // ...
    
    // Send to webview
    this.panel.webview.postMessage({
      type: 'agentResponse',
      text: 'Response to: ' + text
    });
  }
}
```

**Webview Side - Add Test Handlers:**

```typescript
// In your webview HTML/JS
const vscode = acquireVsCodeApi();

let messages = [];

// Handle messages from extension
window.addEventListener('message', event => {
  const message = event.data;
  
  if (message.type === 'agentResponse') {
    messages.push(message);
    updateUI();
  }
  
  // Test-specific handlers
  if (message.type === 'test:getState') {
    vscode.postMessage({
      type: 'test:state',
      data: {
        messages: messages,
        // other state...
      }
    });
  }
});

// Handle user input
function sendMessage(text) {
  vscode.postMessage({
    type: 'userMessage',
    text: text
  });
}
```

**Integration Test:**

```typescript
suite('Chat Webview Tests', () => {
  let chatPanel: ChatPanel;

  setup(async () => {
    // Set test mode
    process.env.VSCODE_TEST_MODE = 'true';
    
    // Create chat panel
    chatPanel = new ChatPanel(extensionUri);
  });

  teardown(async () => {
    // Clean up
    await vscode.commands.executeCommand('workbench.action.closeAllEditors');
    process.env.VSCODE_TEST_MODE = 'false';
  });

  test('Chat state persistence', async () => {
    // Send a message
    chatPanel.sendMessage('Hello');
    
    // Wait for response
    await new Promise(resolve => setTimeout(resolve, 500));
    
    // Get state before closing
    const stateBefore = await chatPanel.requestState();
    assert.strictEqual(stateBefore.messages.length, 1);
    
    // Close and reopen
    await vscode.commands.executeCommand('workbench.action.closePanel');
    await new Promise(resolve => setTimeout(resolve, 100));
    
    // Reopen chat
    chatPanel = new ChatPanel(extensionUri);
    await new Promise(resolve => setTimeout(resolve, 500));
    
    // Verify state persisted
    const stateAfter = await chatPanel.requestState();
    assert.strictEqual(stateAfter.messages.length, 1);
    assert.strictEqual(stateAfter.messages[0].text, 'Response to: Hello');
  });
});
```

### Approach 2: Direct Extension-Side Testing

If your webview logic mostly lives on the extension side, test the handlers directly:

```typescript
test('Handle user message', async () => {
  const chatPanel = new ChatPanel(extensionUri);
  
  // Simulate message from webview by calling the handler directly
  await chatPanel.handleWebviewMessage({
    type: 'userMessage',
    text: 'Test message'
  });
  
  // Verify the extension's state changed
  const messages = chatPanel.getMessages();
  assert.strictEqual(messages.length, 1);
  assert.strictEqual(messages[0].user, 'Test message');
});
```

### Approach 3: Using WebdriverIO for True E2E Webview Testing

For complex webview UIs where you need to test the actual DOM:

**Installation:**
```bash
npm install --save-dev @wdio/cli @wdio/mocha-framework wdio-vscode-service
```

**wdio.conf.ts:**
```typescript
import path from 'path';

export const config = {
  specs: ['./test/e2e/**/*.test.ts'],
  capabilities: [{
    browserName: 'vscode',
    browserVersion: 'stable',
    'wdio:vscodeOptions': {
      extensionPath: path.join(__dirname, '.'),
      userSettings: {
        'window.dialogStyle': 'custom'
      }
    }
  }],
  services: ['vscode'],
  framework: 'mocha',
  mochaOpts: {
    ui: 'bdd',
    timeout: 60000
  }
};
```

**E2E Test:**
```typescript
describe('Chat Webview E2E', () => {
  it('should allow typing and sending messages', async () => {
    const workbench = await browser.getWorkbench();
    
    // Open your chat panel
    await browser.executeWorkbench((vscode) => {
      vscode.commands.executeCommand('your-extension.openChat');
    });
    
    // Wait for webview to appear
    await browser.pause(1000);
    
    // Switch to webview frame
    const webview = await $('iframe.webview');
    await browser.switchToFrame(webview);
    
    // Interact with webview DOM
    const input = await $('input[type="text"]');
    await input.setValue('Hello from E2E test');
    
    const sendButton = await $('button[type="submit"]');
    await sendButton.click();
    
    // Verify response appears
    const messages = await $$('.message');
    expect(messages).toHaveLength(2); // User message + bot response
  });
});
```

---

## Advanced Testing Scenarios

### Testing with Mock Dependencies

```typescript
// Create a mock agent for deterministic testing
class MockAgent {
  async sendMessage(text: string): Promise<string> {
    // Return deterministic responses for testing
    if (text.includes('hello')) {
      return 'Hi there!';
    }
    return 'I received: ' + text;
  }
}

// Inject mock in tests
test('Chat with mock agent', async () => {
  const mockAgent = new MockAgent();
  const chatPanel = new ChatPanel(extensionUri, mockAgent);
  
  chatPanel.sendMessage('hello');
  await waitForCondition(() => chatPanel.getMessages().length > 0);
  
  const messages = chatPanel.getMessages();
  assert.strictEqual(messages[0].response, 'Hi there!');
});
```

### Testing State Serialization

```typescript
test('Serialize and restore webview state', async () => {
  const chatPanel = new ChatPanel(extensionUri);
  
  // Add some state
  chatPanel.sendMessage('First message');
  await new Promise(resolve => setTimeout(resolve, 200));
  
  chatPanel.sendMessage('Second message');
  await new Promise(resolve => setTimeout(resolve, 200));
  
  // Get serialized state
  const state = chatPanel.getSerializedState();
  assert.ok(state);
  assert.ok(state.messages);
  
  // Close panel
  chatPanel.dispose();
  
  // Create new panel with saved state
  const newChatPanel = ChatPanel.restore(extensionUri, state);
  
  // Verify state was restored
  const messages = newChatPanel.getMessages();
  assert.strictEqual(messages.length, 2);
  assert.strictEqual(messages[0].text, 'First message');
});
```

### Testing with File System

```typescript
import * as fs from 'fs/promises';
import * as path from 'path';
import * as os from 'os';

suite('File Operations', () => {
  let tempDir: string;

  setup(async () => {
    // Create temp directory for test files
    tempDir = await fs.mkdtemp(path.join(os.tmpdir(), 'vscode-test-'));
  });

  teardown(async () => {
    // Clean up temp files
    await fs.rm(tempDir, { recursive: true, force: true });
  });

  test('Should read and process files', async () => {
    // Create test file
    const testFile = path.join(tempDir, 'test.txt');
    await fs.writeFile(testFile, 'test content');
    
    // Open file in VSCode
    const doc = await vscode.workspace.openTextDocument(testFile);
    await vscode.window.showTextDocument(doc);
    
    // Execute your command
    await vscode.commands.executeCommand('your-extension.processFile');
    
    // Verify results
    const content = await fs.readFile(testFile, 'utf-8');
    assert.strictEqual(content, 'PROCESSED: test content');
  });
});
```

### Testing Extension Configuration

```typescript
test('Should respect configuration changes', async () => {
  const config = vscode.workspace.getConfiguration('your-extension');
  
  // Set test configuration
  await config.update('someSetting', 'testValue', 
    vscode.ConfigurationTarget.Global);
  
  // Execute command that uses config
  const result = await vscode.commands.executeCommand('your-extension.useConfig');
  
  assert.strictEqual(result.settingValue, 'testValue');
  
  // Clean up
  await config.update('someSetting', undefined, 
    vscode.ConfigurationTarget.Global);
});
```

---

## Testing Best Practices

### 1. Isolation
- Each test should be independent
- Clean up resources in `teardown()`
- Don't rely on test execution order
- Close editors and panels after tests

### 2. Determinism
- Use mock agents or services for predictable behavior
- Avoid timing dependencies where possible
- Use proper wait conditions instead of arbitrary sleeps
- Control randomness (use seeds for random data)

### 3. Speed
- Keep integration tests focused
- Don't test every edge case in integration tests
- Use unit tests for detailed logic testing
- Disable unnecessary extensions with `--disable-extensions`

### 4. Clarity
- Use descriptive test names
- Comment complex setup/teardown logic
- Group related tests in suites
- Keep tests readable and maintainable

### 5. Reliability
- Handle asynchronous operations properly
- Use appropriate timeouts
- Add retry logic for flaky operations
- Log failures for debugging

### Test Helpers

Create reusable test utilities:

```typescript
// test/helpers.ts
export async function createTestDocument(
  content: string, 
  language: string = 'plaintext'
): Promise<vscode.TextDocument> {
  const doc = await vscode.workspace.openTextDocument({
    content,
    language
  });
  return doc;
}

export async function closeAllEditors(): Promise<void> {
  await vscode.commands.executeCommand('workbench.action.closeAllEditors');
}

export function waitForExtensionActivation(
  extensionId: string
): Promise<void> {
  return new Promise((resolve, reject) => {
    const extension = vscode.extensions.getExtension(extensionId);
    if (!extension) {
      reject(new Error(`Extension ${extensionId} not found`));
      return;
    }
    
    if (extension.isActive) {
      resolve();
      return;
    }
    
    extension.activate()
      .then(() => resolve())
      .catch(reject);
  });
}

export class Deferred<T> {
  promise: Promise<T>;
  resolve!: (value: T) => void;
  reject!: (error: Error) => void;

  constructor() {
    this.promise = new Promise((resolve, reject) => {
      this.resolve = resolve;
      this.reject = reject;
    });
  }
}
```

---

## Debugging Tests

### VSCode Launch Configuration

Add to `.vscode/launch.json`:

```json
{
  "version": "0.2.0",
  "configurations": [
    {
      "name": "Extension Tests",
      "type": "extensionHost",
      "request": "launch",
      "runtimeExecutable": "${execPath}",
      "args": [
        "--extensionDevelopmentPath=${workspaceFolder}",
        "--extensionTestsPath=${workspaceFolder}/out/test/suite/index",
        "--disable-extensions"
      ],
      "outFiles": [
        "${workspaceFolder}/out/test/**/*.js"
      ],
      "preLaunchTask": "npm: compile"
    }
  ]
}
```

### Debugging Tips

1. **Set breakpoints** in your test files
2. **Use Debug Console** to inspect variables
3. **Run single tests** by using `.only()`:
   ```typescript
   test.only('This test will run alone', () => {
     // ...
   });
   ```
4. **Use console.log** for quick debugging
5. **Check Extension Development Host output** for extension logs

### Running Specific Tests

```bash
# Run all tests
npm test

# Run tests matching pattern
npm test -- --grep "specific test name"

# Run with more verbose output
npm test -- --reporter spec
```

---

## Common Patterns

### Pattern: Testing Command Registration

```typescript
test('Commands should be registered', async () => {
  const commands = await vscode.commands.getCommands(true);
  const expectedCommands = [
    'your-extension.command1',
    'your-extension.command2',
    'your-extension.command3'
  ];
  
  for (const cmd of expectedCommands) {
    assert.ok(
      commands.includes(cmd),
      `Command ${cmd} should be registered`
    );
  }
});
```

### Pattern: Testing Status Bar Items

```typescript
test('Should show status bar item', async () => {
  // Trigger action that creates status bar item
  await vscode.commands.executeCommand('your-extension.showStatus');
  
  // Status bar items aren't directly testable via API,
  // so test the underlying state
  const extension = vscode.extensions.getExtension('your-publisher.your-extension');
  const statusItem = (extension?.exports as any).statusBarItem;
  
  assert.ok(statusItem);
  assert.strictEqual(statusItem.text, '$(check) Ready');
});
```

### Pattern: Testing Tree Views

```typescript
test('Tree view should show items', async () => {
  // Get your tree data provider
  const extension = vscode.extensions.getExtension('your-publisher.your-extension');
  const treeProvider = (extension?.exports as any).treeDataProvider;
  
  // Get root items
  const items = await treeProvider.getChildren();
  
  assert.ok(items.length > 0);
  assert.strictEqual(items[0].label, 'Expected Item');
});
```

### Pattern: Testing Quick Picks

```typescript
test('Quick pick should show options', async () => {
  // This is tricky - quick picks block execution
  // One approach is to test the logic that generates options
  
  const extension = vscode.extensions.getExtension('your-publisher.your-extension');
  const getQuickPickItems = (extension?.exports as any).getQuickPickItems;
  
  const items = await getQuickPickItems();
  
  assert.strictEqual(items.length, 3);
  assert.strictEqual(items[0].label, 'Option 1');
});
```

---

## Tools and Libraries

### Core Testing Tools

- **@vscode/test-cli**: Official CLI for running tests (recommended)
- **@vscode/test-electron**: Lower-level test runner for Desktop VSCode
- **@vscode/test-web**: Test runner for web extensions
- **Mocha**: Test framework used by VSCode (TDD or BDD style)

### Additional Testing Tools

- **WebdriverIO + wdio-vscode-service**: E2E testing with webview support
- **vscode-extension-tester**: Alternative E2E testing tool by Red Hat
- **Sinon**: Mocking and stubbing library
- **Chai**: Assertion library (alternative to Node's assert)

### Useful Utilities

```typescript
// Helper to wait for promises with timeout
export function withTimeout<T>(
  promise: Promise<T>, 
  timeoutMs: number
): Promise<T> {
  return Promise.race([
    promise,
    new Promise<T>((_, reject) => 
      setTimeout(() => reject(new Error('Timeout')), timeoutMs)
    )
  ]);
}

// Helper to retry flaky operations
export async function retry<T>(
  fn: () => Promise<T>,
  attempts: number = 3,
  delay: number = 100
): Promise<T> {
  for (let i = 0; i < attempts; i++) {
    try {
      return await fn();
    } catch (error) {
      if (i === attempts - 1) throw error;
      await new Promise(resolve => setTimeout(resolve, delay));
    }
  }
  throw new Error('Retry failed');
}
```

---

## Example: Complete Test Suite

Here's a complete example putting it all together:

```typescript
import * as assert from 'assert';
import * as vscode from 'vscode';
import { ChatPanel } from '../../chatPanel';

suite('Chat Extension Test Suite', () => {
  let extensionUri: vscode.Uri;
  let chatPanel: ChatPanel | undefined;

  suiteSetup(async () => {
    // Run once before all tests
    const extension = vscode.extensions.getExtension('your-publisher.your-extension');
    assert.ok(extension);
    
    if (!extension.isActive) {
      await extension.activate();
    }
    
    extensionUri = extension.extensionUri;
  });

  setup(() => {
    // Run before each test
    process.env.VSCODE_TEST_MODE = 'true';
  });

  teardown(async () => {
    // Run after each test
    if (chatPanel) {
      chatPanel.dispose();
      chatPanel = undefined;
    }
    await vscode.commands.executeCommand('workbench.action.closeAllEditors');
    process.env.VSCODE_TEST_MODE = 'false';
  });

  test('Extension should be present', () => {
    assert.ok(vscode.extensions.getExtension('your-publisher.your-extension'));
  });

  test('Chat command should be registered', async () => {
    const commands = await vscode.commands.getCommands(true);
    assert.ok(commands.includes('your-extension.openChat'));
  });

  test('Should create chat panel', async () => {
    chatPanel = new ChatPanel(extensionUri);
    assert.ok(chatPanel);
  });

  test('Should send and receive messages', async function() {
    this.timeout(5000);
    
    chatPanel = new ChatPanel(extensionUri);
    
    // Send message
    chatPanel.sendMessage('Hello');
    
    // Wait for response
    await new Promise(resolve => setTimeout(resolve, 1000));
    
    const state = await chatPanel.requestState();
    assert.ok(state.messages.length > 0);
  });

  test('Should persist state across panel close/reopen', async function() {
    this.timeout(10000);
    
    // Create panel and send message
    chatPanel = new ChatPanel(extensionUri);
    chatPanel.sendMessage('Test message');
    await new Promise(resolve => setTimeout(resolve, 500));
    
    // Get state
    const stateBefore = await chatPanel.requestState();
    const messageCount = stateBefore.messages.length;
    
    // Serialize and dispose
    const serialized = chatPanel.getSerializedState();
    chatPanel.dispose();
    chatPanel = undefined;
    
    // Wait a bit
    await new Promise(resolve => setTimeout(resolve, 200));
    
    // Restore
    chatPanel = ChatPanel.restore(extensionUri, serialized);
    await new Promise(resolve => setTimeout(resolve, 500));
    
    // Verify
    const stateAfter = await chatPanel.requestState();
    assert.strictEqual(stateAfter.messages.length, messageCount);
  });
});
```

---

## Summary

Integration testing for VSCode extensions requires:

1. **Proper setup** using @vscode/test-cli or @vscode/test-electron
2. **Strategic testing** - focus on critical workflows, use unit tests for details
3. **Webview testing** via message-passing or E2E tools like WebdriverIO
4. **Good practices** - isolation, determinism, proper cleanup
5. **Debugging support** with launch configurations

Testing webviews specifically requires creative approaches since they run in isolated contexts. The message-passing pattern works well for integration tests, while WebdriverIO is better for true E2E testing of complex UIs.

Remember: integration tests are slower than unit tests, so use them strategically for testing VSCode API interactions and critical user workflows.
