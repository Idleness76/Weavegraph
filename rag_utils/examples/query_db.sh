#!/usr/bin/env bash
set -euo pipefail

echo "🔍 Querying rust_book_chunks.sqlite database..."
echo

cargo run -p rag_utils --example query_chunks --quiet

echo
echo "✅ Query completed!"