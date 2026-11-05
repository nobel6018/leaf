#!/usr/bin/env node
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import { defaultLibraryDir } from "./library.js";
import { createServer } from "./server.js";

// leaf MCP 서버 진입점. 보관함 위치는 LEAF_LIBRARY_DIR로 바꿀 수 있다
// @see ../../docs/mcp.md

await createServer(defaultLibraryDir()).connect(new StdioServerTransport());
