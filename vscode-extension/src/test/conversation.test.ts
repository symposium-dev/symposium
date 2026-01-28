import * as assert from "assert";
import * as vscode from "vscode";
import { logger } from "../extension";
import { LogEvent } from "../logger";

suite("Conversation Tests", () => {
  test("Should have conversation with ElizACP", async function () {
    // This test needs time for agent spawning and response
    this.timeout(30000);

    // Capture log events
    const logEvents: LogEvent[] = [];
    const logDisposable = logger.onLog((event) => {
      logEvents.push(event);
    });

    // Activate the extension
    const extension = vscode.extensions.getExtension("symposium-dev.symposium");
    assert.ok(extension);
    await extension.activate();

    // Show the chat view
    await vscode.commands.executeCommand("symposium.chatView.focus");
    await new Promise((resolve) => setTimeout(resolve, 1000));

    // Create a tab
    console.log("Creating test tab...");
    await vscode.commands.executeCommand(
      "symposium.test.simulateNewTab",
      "test-tab-conversation",
    );

    // Wait for agent to spawn and session to be created
    await new Promise((resolve) => setTimeout(resolve, 3000));

    // Verify tab exists
    let tabs = (await vscode.commands.executeCommand(
      "symposium.test.getTabs",
    )) as string[];
    assert.ok(tabs.includes("test-tab-conversation"), "Tab should exist");

    // Start capturing agent responses
    await vscode.commands.executeCommand(
      "symposium.test.startCapturingResponses",
      "test-tab-conversation",
    );

    // Send a prompt to ElizACP
    console.log("Sending prompt to ElizACP...");
    await vscode.commands.executeCommand(
      "symposium.test.sendPrompt",
      "test-tab-conversation",
      "Hello, how are you?",
    );

    // Wait for response (ElizACP should respond quickly)
    await new Promise((resolve) => setTimeout(resolve, 2000));

    // Get the response
    const response = (await vscode.commands.executeCommand(
      "symposium.test.getResponse",
      "test-tab-conversation",
    )) as string;

    console.log(`ElizACP response: ${response}`);

    // Stop capturing
    await vscode.commands.executeCommand(
      "symposium.test.stopCapturingResponses",
      "test-tab-conversation",
    );

    // Clean up
    logDisposable.dispose();

    // Verify we got a response
    assert.ok(response.length > 0, "Should receive a response from ElizACP");
    assert.ok(
      response.toLowerCase().includes("eliza") ||
        response.toLowerCase().includes("hello") ||
        response.toLowerCase().includes("hi") ||
        response.toLowerCase().includes("how") ||
        response.toLowerCase().includes("what") ||
        response.toLowerCase().includes("you") ||
        response.toLowerCase().includes("thank"),
      "Response should be relevant to the prompt",
    );

    // Verify log events
    const agentSpawned = logEvents.filter(
      (e) => e.category === "agent" && e.message === "Spawning new agent actor",
    );
    const agentReused = logEvents.filter(
      (e) =>
        e.category === "agent" && e.message === "Reusing existing agent actor",
    );
    const promptReceived = logEvents.filter(
      (e) => e.category === "agent" && e.message === "Received prompt",
    );
    const promptSent = logEvents.filter(
      (e) => e.category === "agent" && e.message === "Sending prompt to agent",
    );

    // Agent might be spawned or reused depending on test order
    assert.ok(
      agentSpawned.length + agentReused.length >= 1,
      "Should spawn or reuse an agent",
    );
    assert.strictEqual(promptReceived.length, 1, "Should receive one prompt");
    assert.strictEqual(promptSent.length, 1, "Should send one prompt to agent");

    console.log(`\nConversation test summary:`);
    console.log(`- Response length: ${response.length} characters`);
    console.log(`- Agent spawned: ${agentSpawned.length}`);
    console.log(`- Prompt received: ${promptReceived.length}`);
    console.log(`- Prompt sent to agent: ${promptSent.length}`);
  });
});
