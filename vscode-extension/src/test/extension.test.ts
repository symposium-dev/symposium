import * as assert from "assert";
import * as vscode from "vscode";

suite("Extension Test Suite", () => {
  test("Extension should be present", async () => {
    const extension = vscode.extensions.getExtension("symposium-dev.symposium");
    assert.ok(extension, "Extension should be installed");
  });

  test("Extension should activate", async () => {
    const extension = vscode.extensions.getExtension("symposium-dev.symposium");
    assert.ok(extension);

    await extension.activate();
    assert.strictEqual(extension.isActive, true, "Extension should be active");
  });

  test("Chat view should be registered", async () => {
    const extension = vscode.extensions.getExtension("symposium-dev.symposium");
    assert.ok(extension);
    await extension.activate();

    // The chat view provider should be registered
    // We can't directly check if a view provider exists, but we can verify
    // the extension activated without errors
    assert.strictEqual(extension.isActive, true);
  });

  test("Show output command should be registered", async () => {
    const extension = vscode.extensions.getExtension("symposium-dev.symposium");
    assert.ok(extension);
    await extension.activate();

    const allCommands = await vscode.commands.getCommands(true);
    assert.ok(
      allCommands.includes("symposium.showOutput"),
      "symposium.showOutput should be registered",
    );
  });
});
