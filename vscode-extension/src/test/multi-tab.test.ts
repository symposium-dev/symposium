import * as assert from "assert";
import * as vscode from "vscode";
import { logger } from "../extension";
import { LogEvent } from "../logger";

suite("Multi-Tab Tests", () => {
  test("Should handle conversations across multiple tabs", async function () {
    // This test needs time for multiple agents and conversations
    this.timeout(40000);

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

    // Create first tab
    console.log("Creating tab 1...");
    await vscode.commands.executeCommand(
      "symposium.test.simulateNewTab",
      "tab-1",
    );
    await new Promise((resolve) => setTimeout(resolve, 3000));

    // Create second tab
    console.log("Creating tab 2...");
    await vscode.commands.executeCommand(
      "symposium.test.simulateNewTab",
      "tab-2",
    );
    await new Promise((resolve) => setTimeout(resolve, 1000));

    // Verify both tabs exist
    let tabs = (await vscode.commands.executeCommand(
      "symposium.test.getTabs",
    )) as string[];
    assert.ok(tabs.includes("tab-1"), "Tab 1 should exist");
    assert.ok(tabs.includes("tab-2"), "Tab 2 should exist");

    // Start capturing responses for both tabs
    await vscode.commands.executeCommand(
      "symposium.test.startCapturingResponses",
      "tab-1",
    );
    await vscode.commands.executeCommand(
      "symposium.test.startCapturingResponses",
      "tab-2",
    );

    // Send prompt to tab 1
    console.log("Sending prompt to tab 1...");
    await vscode.commands.executeCommand(
      "symposium.test.sendPrompt",
      "tab-1",
      "What is your name?",
    );
    await new Promise((resolve) => setTimeout(resolve, 2000));

    // Send prompt to tab 2
    console.log("Sending prompt to tab 2...");
    await vscode.commands.executeCommand(
      "symposium.test.sendPrompt",
      "tab-2",
      "Tell me about yourself.",
    );
    await new Promise((resolve) => setTimeout(resolve, 2000));

    // Send another prompt to tab 1
    console.log("Sending second prompt to tab 1...");
    await vscode.commands.executeCommand(
      "symposium.test.sendPrompt",
      "tab-1",
      "How are you?",
    );
    await new Promise((resolve) => setTimeout(resolve, 2000));

    // Get responses
    const response1 = (await vscode.commands.executeCommand(
      "symposium.test.getResponse",
      "tab-1",
    )) as string;
    const response2 = (await vscode.commands.executeCommand(
      "symposium.test.getResponse",
      "tab-2",
    )) as string;

    console.log(`\nTab 1 response: ${response1}`);
    console.log(`Tab 2 response: ${response2}`);

    // Stop capturing
    await vscode.commands.executeCommand(
      "symposium.test.stopCapturingResponses",
      "tab-1",
    );
    await vscode.commands.executeCommand(
      "symposium.test.stopCapturingResponses",
      "tab-2",
    );

    // Clean up
    logDisposable.dispose();

    // Verify both tabs got responses
    assert.ok(response1.length > 0, "Tab 1 should receive responses");
    assert.ok(response2.length > 0, "Tab 2 should receive responses");

    // Verify responses are different (different conversations)
    assert.notStrictEqual(
      response1,
      response2,
      "Each tab should have its own conversation",
    );

    // Verify log events
    const sessionsCreated = logEvents.filter(
      (e) => e.category === "agent" && e.message === "Agent session created",
    );
    const promptsReceived = logEvents.filter(
      (e) => e.category === "agent" && e.message === "Received prompt",
    );
    const promptsSent = logEvents.filter(
      (e) => e.category === "agent" && e.message === "Sending prompt to agent",
    );

    assert.strictEqual(
      sessionsCreated.length,
      2,
      "Should create two agent sessions (one per tab)",
    );
    assert.strictEqual(
      promptsReceived.length,
      3,
      "Should receive three prompts total",
    );
    assert.strictEqual(
      promptsSent.length,
      3,
      "Should send three prompts to agent",
    );

    // Verify the sessions are different
    const sessionIds = sessionsCreated.map((e) => e.data.agentSessionId);
    assert.notStrictEqual(
      sessionIds[0],
      sessionIds[1],
      "Each tab should have its own agent session",
    );

    console.log(`\nMulti-tab test summary:`);
    console.log(`- Tab 1 response length: ${response1.length} characters`);
    console.log(`- Tab 2 response length: ${response2.length} characters`);
    console.log(`- Sessions created: ${sessionsCreated.length}`);
    console.log(`- Total prompts: ${promptsReceived.length}`);
    console.log(`- Session IDs are different: ${sessionIds[0] !== sessionIds[1]}`);
  });
});
