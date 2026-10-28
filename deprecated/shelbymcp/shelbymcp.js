#!/usr/bin/env node

// The last release under the old name (ADR-0012 D6): it says where the server went, on stderr
// so stdout stays the MCP channel, and runs it.
import process from "node:process";
import { launch } from "advisaur-mcp/bin/advisaur-mcp.js";

process.stderr.write(
  "shelbymcp is now advisaur-mcp: change your MCP config to `npx -y advisaur-mcp`. " +
    "This name gets no more updates.\n",
);
process.exitCode = launch(process.argv.slice(2));
