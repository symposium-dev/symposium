#!/usr/bin/env node

const scenarioArg = process.argv.find((value) =>
  value.startsWith("--startup-scenario="),
);
const scenario = scenarioArg?.split("=")[1] ?? "hang";

const holdOpen = () => {
  setInterval(() => {
    // Keep the process alive so startup timeout behavior can be tested.
  }, 60_000);
};

const writeScenarioBanner = () => {
  process.stderr.write(`startup-scenario=${scenario}\n`);
};

const parseNdjsonInput = (onMessage) => {
  let buffer = "";
  process.stdin.setEncoding("utf8");
  process.stdin.on("data", (chunk) => {
    buffer += chunk;
    while (true) {
      const separatorIndex = buffer.indexOf("\n");
      if (separatorIndex < 0) {
        return;
      }

      const line = buffer.slice(0, separatorIndex).trim();
      buffer = buffer.slice(separatorIndex + 1);

      if (line.length === 0) {
        continue;
      }

      try {
        const message = JSON.parse(line);
        onMessage(message);
      } catch {
        // Ignore malformed lines from tests that do not need protocol handling.
      }
    }
  });
};

writeScenarioBanner();

switch (scenario) {
  case "exit":
    process.stderr.write("simulated startup exit\n");
    process.exit(23);
    break;

  case "hang":
    process.stderr.write("simulated startup hang\n");
    holdOpen();
    break;

  case "close":
    process.stderr.write("simulated startup stdout close\n");
    process.stdout.end();
    holdOpen();
    break;

  case "acp-error":
    process.stderr.write("simulated initialize error response\n");
    parseNdjsonInput((message) => {
      if (
        message &&
        typeof message === "object" &&
        message.method === "initialize" &&
        "id" in message
      ) {
        const response = {
          jsonrpc:
            typeof message.jsonrpc === "string" ? message.jsonrpc : "2.0",
          id: message.id,
          error: {
            code: -32001,
            message: "simulated acp initialize error",
          },
        };
        process.stdout.write(`${JSON.stringify(response)}\n`);
      }
    });
    holdOpen();
    break;

  default:
    process.stderr.write(`unknown startup scenario: ${scenario}\n`);
    process.exit(64);
}
