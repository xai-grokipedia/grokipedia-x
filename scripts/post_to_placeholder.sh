 #!/bin/bash
  set -euo pipefail

  SUMMARY_FILE=${1:-summary.json}
  CLI_QUERY=${2:-government}
  PLACEHOLDER_URL=${PLACEHOLDER_URL:-https://example.com/post-edit}

  if [[ ! -f "$SUMMARY_FILE" ]]; then
    echo "Summary file \"$SUMMARY_FILE\" not found."
    exit 1
  fi

  MODEL=$(jq -r '.model' "$SUMMARY_FILE")
  jq -c '.summary[]' "$SUMMARY_FILE" | while IFS= read -r entry; do
    url=$(jq -r '.url' <<<"$entry")
    edit=$(jq -r '.suggested_edit' <<<"$entry")
    original=$(jq -r '.original_text' <<<"$entry")

    echo "Would POST to $PLACEHOLDER_URL for ${url}:"
    jq -n \
      --arg url "$url" \
      --arg edit "$edit" \
      --arg original "$original" \
      --arg model "$MODEL" \
      --arg query "$CLI_QUERY" \
      '{url:$url, suggested_edit:$edit, original_text:$original, source_model:
  $model, source_query:$query}'

    # Uncomment if you ever stand up a real endpoint:
    # curl -X POST "$PLACEHOLDER_URL" \
    #   -H "Content-Type: application/json" \
    #   -d "$(jq -n --arg url "$url" --arg edit "$edit" --arg original
  "$original" \
    #                 --arg model "$MODEL" --arg query "$CLI_QUERY" \

