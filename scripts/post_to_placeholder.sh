#!/bin/bash
set -euo pipefail

SUMMARY_FILE=${1:-summary.json}
CLI_QUERY=${2:-government}
# Allow overriding the auth cookies/headers, but default to the captured browser values.
GROK_COOKIES=${GROK_COOKIES:-'sso-rw=eyJ0eXAiOiJKV1QiLCJhbGciOiJIUzI1NiJ9.eyJzZXNzaW9uX2lkIjoiNDNjMDA1YmYtNTFhYy00ZTM5LWE3ODAtZWM4MWQ0MWFhZWIyIn0.GPG9Y_6wpLv_-3jLbINtUcClk5TP36-k9pm0LyBQeYY; sso=eyJ0eXAiOiJKV1QiLCJhbGciOiJIUzI1NiJ9.eyJzZXNzaW9uX2lkIjoiNDNjMDA1YmYtNTFhYy00ZTM5LWE3ODAtZWM4MWQ0MWFhZWIyIn0.GPG9Y_6wpLv_-3jLbINtUcClk5TP36-k9pm0LyBQeYY; grokipedia-affinity=806d9e8b4ecd3dfbc1505db2137f44e7|ed66aa1bc0dc796acaf359230d461129'}
# Optional override for the Next.js action token if it ever changes.
NEXT_ACTION=${NEXT_ACTION:-'7f8d65b61f382e396aeec028ddad7bba5849630fab'}

if [[ ! -f "$SUMMARY_FILE" ]]; then
  echo "Summary file \"$SUMMARY_FILE\" not found."
  exit 1
fi

MODEL=$(jq -r '.model' "$SUMMARY_FILE")
jq -c '.summary[]' "$SUMMARY_FILE" | while IFS= read -r entry; do
  url=$(jq -r '.url' <<<"$entry")
  edit=$(jq -r '.suggested_edit' <<<"$entry")
  original=$(jq -r '.original_text' <<<"$entry")

  # Derive slug and human-readable section title from the Grokipedia URL.
  slug=${url##*/page/}
  section_title=${slug//_/ }
  page_url="https://grokipedia.com/page/$slug"

  # Build the Next.js router state header for this slug (must stay percent-encoded).
  next_router_state_tree=$(python - "$slug" <<'PY'
import json, sys, urllib.parse as u
slug = sys.argv[1]
state = ["", {"children":["page", {"children":[["slug", slug, "d"], {"children":["__PAGE__", {}, None, None]}, None, None]}, None, None]}, None, None, True]
print(u.quote(json.dumps(state, separators=(",", ":"))))
PY
)

  payload=$(jq -nc \
    --arg slug "$slug" \
    --arg summary "$edit" \
    --arg original "$original" \
    --arg proposed "$edit" \
    --arg section "$section_title" \
    --arg start "$section_title" \
    '[
      {
        slug: $slug,
        type: "EDIT_REQUEST_TYPE_UPDATE_INFORMATION",
        summary: $summary,
        originalContent: $original,
        proposedContent: $proposed,
        sectionTitle: $section,
        editStartHeader: $start,
        editEndHeader: $section,
        supportingEvidence: null
      }
    ]')

  echo "POSTing edit request for ${slug} (query: ${CLI_QUERY})..."
  curl "$page_url" \
    -H 'accept: text/x-component' \
    -H 'accept-language: en-US,en;q=0.9' \
    -H 'baggage: sentry-environment=production,sentry-public_key=5f2258f71198ee26a355127af230c3a6,sentry-trace_id=6b4b9d95a307e2a1b4ba76f9342c2c47,sentry-org_id=4508179396558848,sentry-sampled=false,sentry-sample_rand=0.21192165782558137,sentry-sample_rate=0' \
    -H 'content-type: text/plain;charset=UTF-8' \
    -b "$GROK_COOKIES" \
    -H 'dnt: 1' \
    -H "next-action: $NEXT_ACTION" \
    -H "next-router-state-tree: $next_router_state_tree" \
    -H 'origin: https://grokipedia.com' \
    -H 'priority: u=1, i' \
    -H "referer: $page_url" \
    -H 'sec-ch-ua: "Chromium";v="142", "Google Chrome";v="142", "Not_A Brand";v="99"' \
    -H 'sec-ch-ua-mobile: ?0' \
    -H 'sec-ch-ua-platform: "macOS"' \
    -H 'sec-fetch-dest: empty' \
    -H 'sec-fetch-mode: cors' \
    -H 'sec-fetch-site: same-origin' \
    -H 'sentry-trace: 6b4b9d95a307e2a1b4ba76f9342c2c47-a30ee06bf30f10d4-0' \
    -H 'user-agent: Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/142.0.0.0 Safari/537.36' \
    --data-raw "$payload"
done
