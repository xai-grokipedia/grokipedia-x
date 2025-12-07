#!/bin/bash
  set -euo pipefail

  queries=("laptop class" "drone wars" "cloud seeding" "technology" "healthcare")

  for q in "${queries[@]}"; do
    echo "=== Running pipeline for \"$q\" ==="
    cargo run --release -- "$q"
    ./scripts/post_to_placeholder.sh summary.json "$q"
  done

