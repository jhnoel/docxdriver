#!/usr/bin/env bash
set -o pipefail

if (($# == 0)); then
  echo "usage: $(basename "$0") [--log FILE] STRING [agent options...]" >&2
  exit 2
fi

if ! command -v jq >/dev/null 2>&1; then
  echo "error: jq is required" >&2
  exit 127
fi

agent_log=""
if [[ ${1:-} == "--log" ]]; then
  if (($# < 3)); then
    echo "error: --log requires a file and a prompt" >&2
    exit 2
  fi
  agent_log=$2
  shift 2
  mkdir -p "$(dirname "$agent_log")"
else
  agent_log_dir=${TMPDIR:-/tmp}/docxdriver-agent-logs
  mkdir -p "$agent_log_dir"
  agent_log=$(mktemp "$agent_log_dir/agent.XXXXXX")
fi

echo "agent log: $agent_log" >&2

# stream-json keeps the command observable while omitting the extremely noisy
# token-by-token deltas produced by --stream-partial-output.
agent -p --force --output-format stream-json "$@" |
  tee "$agent_log" |
  jq --unbuffered -r '
    def text_content:
      [.message.content[]? | select(.type == "text") | .text] | join("");

    def tool_name:
      (.tool_call // {})
      | keys_unsorted[0]? // "tool";

    def tool_description:
      [.. | objects | .description? | select(type == "string" and length > 0)][0]?;

    if .type == "assistant" then
      (text_content | select(length > 0))
    elif .type == "tool_call" and .subtype == "started" then
      "[tool] \(tool_description // tool_name)"
    elif .type == "tool_call" and .subtype == "completed"
         and ([.. | objects | .error? | select(. != null)] | length) > 0 then
      "[tool error] \([.. | objects | .error? | select(. != null)][0])"
    elif .type == "error" then
      "[error] \(.message // .error // .)"
    else
      empty
    end
  '
