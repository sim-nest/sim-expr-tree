# Server-backed desktop and phone session

This recipe composes `ExpressionTreeSurfaceCodec`, the authoritative
`ExpressionTreeServer` site, a real `RemoteTransport`, isolated desktop and
phone session hosts, and the existing generic browser Scene interpreter. The
checked Rust specimen creates a directory and cell, edits and calculates the
cell, requests its explanation, collapses and reopens the directory, reconnects
the phone, and verifies a separate read-only browser cannot gain write
authority.

The specimen is deliberately stricter than a happy-path demo. It proves that a
collapsed directory carries no descendant faces or receipt text, automatic
progress and its bounded receipt are visible, a stale phone commit reports
`stale-revision`, reconnect refreshes server authority without widening it, and
an unrelated server session remains isolated. The generic browser gate also
proves that the service worker caches shell assets but never session APIs or
tree data.

Run the authoritative flow and regenerate its deterministic Scene fixtures:

```sh
SIM_UPDATE_EXPR_TREE_WEB_FIXTURES=1 \
  cargo test -p sim-lib-expr-tree-server \
  recipe_server_backed_web_session_runs_desktop_phone_and_failure_paths
cargo test -p sim-lib-expr-tree-server \
  recipe_server_backed_web_session_runs_desktop_phone_and_failure_paths
node ../sim-web/crates/sim-web-shell/web/tests/e2e.test.mjs \
  crates/sim-lib-expr-tree-server/tests/fixtures/web-session/desktop.json \
  crates/sim-lib-expr-tree-server/tests/fixtures/web-session/phone.json
```

The committed fixture data comes from the final authoritative revision after
the source becomes `42`. Session ids are canonicalized to
`expr-tree/session/fixture`; the deterministic server clock supplies the
remaining receipt and timestamp fields.

Capture the review images through the generic `scene-fixture.html` interpreter.
Run the HTTP server from the directory containing the sibling repositories:

```sh
python3 -m http.server 8765 --bind 127.0.0.1
google-chrome --headless --no-sandbox --disable-gpu --hide-scrollbars \
  --run-all-compositor-stages-before-draw --virtual-time-budget=2000 \
  --window-size=1440,900 \
  --screenshot=sim-expr-tree/recipes/03-server/web-session/screenshots/desktop.png \
  'http://127.0.0.1:8765/sim-web/crates/sim-web-shell/web/tests/scene-fixture.html?scene=/sim-expr-tree/crates/sim-lib-expr-tree-server/tests/fixtures/web-session/desktop.json'
google-chrome --headless --no-sandbox --disable-gpu --hide-scrollbars \
  --run-all-compositor-stages-before-draw --virtual-time-budget=2000 \
  --window-size=390,844 \
  --screenshot=sim-expr-tree/recipes/03-server/web-session/screenshots/phone.png \
  'http://127.0.0.1:8765/sim-web/crates/sim-web-shell/web/tests/scene-fixture.html?scene=/sim-expr-tree/crates/sim-lib-expr-tree-server/tests/fixtures/web-session/phone.json'
```

Two consecutive captures produced the byte-identical digests recorded in
`screenshots/SHA256SUMS`.

The browser page imports `interpreter/scene.js` from `sim-web`; this repository
contains no product JavaScript and no fork of the Scene interpreter.
