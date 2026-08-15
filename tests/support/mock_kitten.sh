#!/usr/bin/env bash
# Mock `kitten` binary for integration tests — logs every invocation and,
# for `get-text`, echoes back $MOCK_KITTEN_GET_TEXT so tests can script
# scrapeable screen content without a real Kitty instance.
echo "kitten $*" >> "${MOCK_KITTEN_LOG}"
if [[ "$1" == "@" && "$2" == "get-text" ]]; then
  printf '%s' "${MOCK_KITTEN_GET_TEXT:-}"
fi
