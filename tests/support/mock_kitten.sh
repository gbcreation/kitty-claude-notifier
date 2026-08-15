#!/usr/bin/env bash
# Mock `kitten` binary for integration tests. Logs every invocation and
# scripts responses for the two commands that read something back:
# - `get-text`: echoes $MOCK_KITTEN_GET_TEXT
# - `ls`: returns minimal valid JSON with $MOCK_KITTEN_LS_TITLE as the
#   tab's title (defaults to empty), matching the shape get_tab_info parses
echo "kitten $*" >> "${MOCK_KITTEN_LOG}"
if [[ "$1" == "@" && "$2" == "get-text" ]]; then
  printf '%s' "${MOCK_KITTEN_GET_TEXT:-}"
elif [[ "$1" == "@" && "$2" == "ls" ]]; then
  printf '[{"tabs":[{"title":"%s"}]}]' "${MOCK_KITTEN_LS_TITLE:-}"
fi
