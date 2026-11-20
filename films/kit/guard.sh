#!/bin/sh
# Fjord's Show me creator may call only its preview tool (and ToolSearch, which Claude Code puts deferred MCP tools behind).
# Claude Code runs this before every tool call with the call as JSON on stdin; every other tool is denied with a reason,
# which reaches what a deny list can't name: the user's own MCP servers and plugins, and tools added later. plutil parses
# the JSON for real (a name inside the call's input can't pass for the tool's); what it can't read is denied.
name=$(/usr/bin/plutil -extract tool_name raw -o - - 2>/dev/null)
case "$name" in
  mcp__fjord__preview|ToolSearch) exit 0 ;;
esac
printf '%s\n' '{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny","permissionDecisionReason":"Fjord explainers may only call mcp__fjord__preview. Finish with the code in a ```js block."}}'
