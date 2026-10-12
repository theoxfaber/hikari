# Cross-renderer comparison harness

Reproduces the Satori/resvg figures in `BENCHMARKS.md`.

```sh
npm install satori @resvg/resvg-js linkedom
cargo build --release -p hikari-rs --example html_bench

FONT=crates/hikari-core/assets/DejaVuSans.ttf

# timings
./target/release/examples/html_bench scripts/compare/card.html /tmp/h.png 50
node scripts/compare/run.mjs scripts/compare/card.html /tmp/s.png 50 "$FONT"

# output equivalence
node scripts/compare/compare.mjs scripts/compare/card.html /tmp/h.rgba baseline
```

Two things here are not incidental.

**`conv.mjs` is the port.** Satori takes a JSX element tree, not an HTML
string, so the document has to be parsed and reshaped before either renderer can
use it. Both sides receive the same tree, which is the only way the timings mean
anything. Handing Satori the HTML string directly does not fail — it renders the
markup as literal text, producing `x="NaN"` and a blank white page, and then
"beats" any real renderer on time.

**`compare.mjs` reports a shift as well as a residual.** The two renderers place
this document differently, so a raw pixel difference conflates "different
layout" with "different rasteriser". It searches for the vertical offset that
minimises the difference and reports the residual at that offset, which is the
part that actually compares rasterisation.
