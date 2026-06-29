#!/usr/bin/env bash
# Build the peacock Flutter-web bundle the way peacock serves it.
#
# peacock serves this bundle at `/app/` (see crates/peacock-server/src/http.rs).
# The bundle is mount-agnostic at runtime — web/index.html rewrites <base href>
# to the document's directory at load time (see
# doc/flutter-iframe-runtime-proposal.md), so CanvasKit/assets resolve correctly
# under any subpath. We still pass `--base-href /app/` as the static default for
# no-JS crawlers and to match the mount, then verify the result.
#
# Usage: web/peacock-web/build_web.sh   (run from anywhere; resolves its own dir)
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$here"

base_href="${PEACOCK_BASE_HREF:-/app/}"

echo "› flutter build web --release --base-href ${base_href}"
flutter build web --release --base-href "${base_href}"

index="build/web/index.html"
echo "› checking ${index}"

# Check 1: the static <base href> matches the intended mount.
if ! grep -q "<base href=\"${base_href}\">" "${index}"; then
  echo "FAIL: <base href> in ${index} is not ${base_href}" >&2
  grep -n '<base' "${index}" >&2 || true
  exit 1
fi

# Check 2: the runtime base-href rewrite is present, so the bundle still works
# if served at a different subpath (e.g. nested under an MCP host iframe).
if ! grep -q 'base.setAttribute' "${index}"; then
  echo "FAIL: runtime base-href rewrite missing from ${index}" >&2
  echo "      (web/index.html must rewrite <base> from window.location.pathname)" >&2
  exit 1
fi

# Check 3: the load-bearing engine files exist next to index.html.
for f in flutter.js flutter_bootstrap.js main.dart.js; do
  if [[ ! -f "build/web/${f}" ]]; then
    echo "FAIL: build/web/${f} is missing" >&2
    exit 1
  fi
done

echo "✓ peacock-web bundle built and verified at build/web (base-href ${base_href})"
