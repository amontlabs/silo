"""The traffic-shape markers one release of an agent program carries, read from its package.

usage: python3 -I markers.py <program> <unpacked package dir>   (prints one JSON object)

A marker is a string the program needs to talk to its model in a given shape: a beta it can ask
for, a path, a header it sends or reads, an item or event type of the wire format. Finding one in
the package says the release can send or read it, not that it does on every call; what it does
send is what contract/ measures. Claude Code and Codex ship one native binary each, Gemini CLI a
JavaScript bundle; all are read as bytes.
"""
import json, os, re, sys

prog, root = sys.argv[1], sys.argv[2]

def blobs():
    for d, _, fs in os.walk(root):
        for f in fs:
            p = os.path.join(d, f)
            if os.path.islink(p) or os.path.getsize(p) < 200_000:
                continue
            if prog == "gemini-cli" and not p.endswith((".js", ".cjs", ".mjs")):
                continue
            if prog == "codex" and "/bin/" not in p:
                continue
            yield open(p, "rb").read()

def grab(rx, data, group=0):
    return {m.group(group).decode("latin-1") for m in re.finditer(rx, data)}

# Codex is Rust: its strings sit end to end with no separator, so open patterns run into their
# neighbours. Its markers are a fixed list, each looked for as it is spelled on the wire.
CODEX_WORDS = """
/responses /v1/responses /backend-api/codex /backend-api/codex/responses /responses/compact
/backend-api/wham/usage /v1/models /models response.create response.created response.completed
response.output_item.added response.output_item.done response.output_text.delta
response.function_call_arguments.delta response.custom_tool_call_input.delta
response.reasoning_summary_text.delta response.reasoning_text.delta response.failed
response.incomplete response.in_progress response.metadata codex.rate_limits
function_call function_call_output custom_tool_call custom_tool_call_output local_shell_call
local_shell_call_output web_search_call tool_search_call tool_search_output image_generation_call
additional_tools compaction encrypted_content prompt_cache_key previous_response_id
client_metadata parallel_tool_calls service_tier store include reasoning.encrypted_content
apply_patch exec_command write_stdin shell_command spawn_agent update_plan view_image
OpenAI-Beta chatgpt-account-id session_id conversation_id originator x-codex-turn-state
x-codex-turn-metadata x-codex-beta-features x-openai-subagent x-openai-memgen-request
x-codex-window-id x-client-request-id x-openai-request-id permessage-deflate
""".split()
CODEX_RX = {
    "websocket_beta": rb"responses_websockets=[0-9]{4}-[0-9]{2}-[0-9]{2}",
    "responses_beta": rb"responses=(?:experimental|v[0-9]+)",
}

GEMINI_WORDS = """
functionCall functionResponse thoughtSignature thought executableCode codeExecutionResult
inlineData fileData cachedContent cachedContentTokenCount thoughtsTokenCount toolUsePromptTokenCount
usageMetadata promptFeedback groundingMetadata urlContextMetadata systemInstruction toolConfig
generationConfig thinkingConfig thinkingBudget thinkingLevel includeThoughts responseJsonSchema
parametersJsonSchema functionDeclarations googleSearch urlContext session_id user_prompt_id
""".split()

WIRE_TYPES = {"text", "image", "document", "thinking", "redacted_thinking", "compaction", "ping", "error",
              "message", "tool", "custom", "auto", "any", "none", "ephemeral", "enabled", "disabled", "adaptive"}

def markers(data):
    out = {}
    if prog == "claude-code":
        out["betas"] = grab(rb'"([a-z0-9]+(?:-[a-z0-9]+)*-20[0-9]{2}-[0-9]{2}-[0-9]{2})"', data, 1)
        out["paths"] = grab(rb'"(/v1/[a-z_]+(?:/[a-z_]+)*)', data, 1)
        out["headers"] = grab(rb'"((?:anthropic|x-claude-code|x-app|x-anthropic)-[a-z0-9-]+|x-app)"', data, 1)
        types = grab(rb'type:"([a-z][a-z_]*)"', data, 1)
        out["block_and_event_types"] = {t for t in types if not t.startswith("tengu_") and (
            t.endswith(("_delta", "_use", "_result", "_start", "_stop", "_reference", "_upload")) or t in WIRE_TYPES)}
        out["hosts"] = grab(rb"https://((?:[a-z0-9-]+\.)*(?:anthropic\.com|claude\.ai|claude\.com))", data, 1)
    elif prog == "codex":
        out["words"] = {w for w in CODEX_WORDS if w.encode() in data}
        for k, rx in CODEX_RX.items():
            out[k] = grab(rx, data)
        out["hosts"] = {h for h in ("api.openai.com", "chatgpt.com", "auth.openai.com", "ab.chatgpt.com") if h.encode() in data}
    elif prog == "gemini-cli":
        out["words"] = {w for w in GEMINI_WORDS if (b'"' + w.encode() + b'"' in data or b"." + w.encode() in data or w.encode() + b":" in data)}
        out["methods"] = grab(rb"\b(v1internal|v1beta|v1alpha|streamGenerateContent|generateContent|countTokens|embedContent|loadCodeAssist|onboardUser|retrieveUserQuota|listExperiments|fetchAdminControls|recordCodeAssistMetrics|setCodeAssistGlobalUserSetting|getCodeAssistGlobalUserSetting)\b", data, 1)
        out["tools"] = grab(rb'[A-Z_]*TOOL_NAME[A-Z_]* = "([a-z_]+)"', data, 1)
        out["hosts"] = grab(rb"https://([a-z0-9-]+(?:\.[a-z0-9-]+)*\.googleapis\.com)", data, 1)
        out["headers"] = grab(rb"[\"'`]((?:x-goog|x-gemini)-[a-z0-9-]+)[\"'`]", data, 1)
    return out

acc = {}
for b in blobs():
    for k, v in markers(b).items():
        acc.setdefault(k, set()).update(v)
print(json.dumps({k: sorted(v) for k, v in acc.items()}, sort_keys=True))
