import { createInterface, type Interface } from "node:readline";
import { SourceAdapter, AdapterError } from "./adapter.js";
import { Request, errorResponse, successResponse } from "./protocol.js";

// Stdout is reserved for one JSON protocol response per line. Diagnostics must use stderr.
const adapter = new SourceAdapter();
let queue = Promise.resolve();
let input: Interface | undefined;

async function handle(line: string): Promise<void> {
  let requestId = "unknown";
  try {
    const raw: unknown = JSON.parse(line);
    if (raw && typeof raw === "object" && "requestId" in raw) requestId = String(raw.requestId);
    const request = Request.parse(raw);
    const result = await adapter.execute(request);
    process.stdout.write(`${JSON.stringify(successResponse(request.requestId, result))}\n`);
  } catch (error) {
    const response = error instanceof AdapterError
      ? errorResponse(requestId, error.code, error.message, error.retryable)
      : errorResponse(requestId, "INVALID_REQUEST", error instanceof Error ? error.message : "Invalid request");
    process.stdout.write(`${JSON.stringify(response)}\n`);
  }
}

async function shutdown() {
  input?.close();
  await queue;
  await adapter.close();
}

const oneShotRequest = process.argv[2];
if (oneShotRequest) {
  await handle(oneShotRequest);
  await adapter.close();
} else {
  input = createInterface({ input: process.stdin, crlfDelay: Infinity });
  input.on("line", line => {
    queue = queue.then(() => handle(line)).catch(error => console.error(error));
  });
  process.once("SIGINT", () => void shutdown().finally(() => process.exit(0)));
  process.once("SIGTERM", () => void shutdown().finally(() => process.exit(0)));
}
