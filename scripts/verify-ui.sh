#!/usr/bin/env bash
# J7 (issue #44): an analyst opens the `ui://peacock` report runtime in a REAL
# browser, hovers a bar of the anofox chart and reads its value, then clicks a
# category to drill — a fresh render from absolute params. Everything is real:
# headless Chrome (driven by rodney), peacock, escurel over the Northwind
# Parquet (`peacock-demo` boots both). No mocks.
#
# The runtime is plain HTML (not the Flutter canvas), so DOM hover/click events
# reach the chart marks — anofox tags each mark `.gg-mark` with
# data-x / data-series / data-fvalue.
#
#   scripts/verify-ui.sh            # builds + runs peacock-demo, then drives it
#   RODNEY=rodney scripts/verify-ui.sh   # use an installed rodney instead of uvx
#   PEACOCK_UI_PORT=8791            # the demo's port
#
# Exit 0 = every assertion held. Screenshots: target/verify-ui/*.png.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PORT="${PEACOCK_UI_PORT:-8791}"
BASE="http://127.0.0.1:${PORT}"
REPORT="northwind-monthly-revenue"
OUT="${ROOT}/target/verify-ui"
read -r -a RODNEY <<<"${RODNEY:-uvx rodney}"
mkdir -p "$OUT"

DEMO_PID=""
cleanup() {
  "${RODNEY[@]}" stop >/dev/null 2>&1 || true
  if [[ -n "$DEMO_PID" ]]; then
    kill -TERM -- "-$DEMO_PID" 2>/dev/null || true
    wait "$DEMO_PID" 2>/dev/null || true
  fi
}
trap cleanup EXIT

step() { printf '\n== %s\n' "$*"; }
fail() { printf 'FAIL: %s\n' "$*" >&2; exit 1; }

step "build + start peacock-demo (real escurel + peacock) on :${PORT}"
# Through `cargo run`, not the binary: with a downloaded (dynamic) libduckdb
# (CI's DUCKDB_DOWNLOAD_LIB) cargo puts the library on the loader path.
(cd "$ROOT" && cargo build -q -p peacock-demo)
# In its own process group, so cleanup stops cargo AND the demo it spawned.
setsid bash -c 'cd "$1" && PEACOCK_DEMO_PORT="$2" PEACOCK_DEMO_FLUTTER=0 exec cargo run -q -p peacock-demo' \
  _ "$ROOT" "$PORT" >"${OUT}/demo.log" 2>&1 &
DEMO_PID=$!
for _ in $(seq 1 240); do
  curl -fsS "${BASE}/healthz" >/dev/null 2>&1 && break
  kill -0 "$DEMO_PID" 2>/dev/null || { cat "${OUT}/demo.log"; fail "peacock-demo exited"; }
  sleep 0.5
done
curl -fsS "${BASE}/healthz" >/dev/null || fail "peacock-demo never became healthy"

step "open the report runtime in headless Chrome"
"${RODNEY[@]}" start
"${RODNEY[@]}" open "${BASE}/ui/${REPORT}"
"${RODNEY[@]}" wait '.chart svg .gg-mark[data-series="Beverages"]'
"${RODNEY[@]}" assert 'document.querySelectorAll(".chart svg .gg-mark").length > 10' \
  -m "the interactive anofox chart is inline (not just a PNG)"
"${RODNEY[@]}" assert \
  '[...document.querySelectorAll(".chart .gg-mark")].some(m => m.dataset.series === "Seafood")' \
  -m "all five categories are drawn"
"${RODNEY[@]}" screenshot -w 1100 -h 900 "${OUT}/1-report.png"

step "hover a Beverages bar → its value in the tooltip"
MARK='.chart .gg-mark[data-series="Beverages"]'
"${RODNEY[@]}" hover "$MARK"
"${RODNEY[@]}" assert '!document.getElementById("tip").hidden' -m "a tooltip appears on hover"
"${RODNEY[@]}" assert \
  "document.getElementById('tip').textContent === (() => { const d = document.querySelector('${MARK}').dataset; return d.series + ' · ' + d.x + ': ' + d.fvalue; })()" \
  -m "the tooltip names the series, the month and the value of that bar"
"${RODNEY[@]}" screenshot -w 1100 -h 900 "${OUT}/2-hover.png"

step "click it → drill to Beverages (fresh render, absolute params)"
"${RODNEY[@]}" click "$MARK"
"${RODNEY[@]}" wait '.chip.on[data-cat="Beverages"]'
"${RODNEY[@]}" assert \
  '![...document.querySelectorAll(".chart .gg-mark")].some(m => m.dataset.series === "Seafood")' \
  -m "only Beverages is drawn after the drill"
"${RODNEY[@]}" assert \
  '[...document.querySelectorAll(".chart .gg-mark")].some(m => m.dataset.series === "Beverages")' \
  -m "the drilled chart still shows Beverages"
"${RODNEY[@]}" assert \
  '[...document.querySelectorAll("table td")].every(td => td.textContent !== "Seafood")' \
  -m "the table is drilled too"
"${RODNEY[@]}" screenshot -w 1100 -h 900 "${OUT}/3-drilled.png"

step "back to all categories"
"${RODNEY[@]}" click '.chip[data-cat="ALL"]'
"${RODNEY[@]}" wait '.chip.on[data-cat="ALL"]'
"${RODNEY[@]}" assert \
  '[...document.querySelectorAll(".chart .gg-mark")].some(m => m.dataset.series === "Seafood")' \
  -m "the un-drilled chart shows every category again"

printf '\nverify-ui: OK — screenshots in %s\n' "$OUT"
