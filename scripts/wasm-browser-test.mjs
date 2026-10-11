#!/usr/bin/env node
// Browser test for the WebAssembly bundle.
//
// # Why this exists
//
// Everything else in CI builds the WASM module and measures it. Nothing loaded
// it. Those are different things: a module can compile, be the right size, and
// still fail the moment a browser instantiates it -- a missing export, an
// `import.meta` the bundler rewrote, a function renamed with JavaScript that
// nothing type-checks against it.
//
// The Rust `#[cfg(test)]` calls `render_png` directly, which exercises the
// library but not the `wasm_bindgen` glue, the generated `.d.ts` or the JS shim.
// This runs in a real browser against the real web-target bundle.
//
// # Why a browser and not Node
//
// The committed bundle is built for the **web** target. Running it under Node
// fails at `__wbindgen_add_to_stack_pointer` -- the Node build fetches its
// `.wasm` differently, so loading the web bundle in Node tests the wrong thing
// and reports a failure that is not one. The first version of this script did
// exactly that, and the honest response was to use the environment the artifact
// is actually built for.
//
// # What it does and does not check
//
// Exports present and callable, and a render produces a real PNG. It does *not*
// assert a digest: a browser is not a controlled environment, and a
// determinism failure here would say nothing about this module. That job belongs
// to `cargo run --example determinism` on native targets.
//
// Usage: node wasm-browser-test.mjs [path/to/pkg]

import { existsSync } from "node:fs";
import { fileURLToPath, pathToFileURL } from "node:url";
import { dirname, resolve, join } from "node:path";
import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import { chromium } from "playwright";

const here = dirname(fileURLToPath(import.meta.url));
const pkgDir = resolve(process.argv[2] ?? join(here, "../docs/playground/pkg"));

for (const f of ["hikari_wasm.js", "hikari_wasm_bg.wasm"]) {
  if (!existsSync(join(pkgDir, f))) {
    console.error(`missing ${f} in ${pkgDir}; build the playground bundle first`);
    process.exit(1);
  }
}

// A real origin. `file://` would fail the module fetch that a browser performs
// for a `.wasm` sidecar, which is the very thing under test.
const MIME = { ".wasm": "application/wasm", ".js": "text/javascript" };
const server = createServer(async (req, res) => {
  const name = (req.url ?? "/").split("?")[0].replace(/^\/+/, "") || "hikari_wasm.js";
  const path = join(pkgDir, name);
  if (!path.startsWith(pkgDir)) {
    res.writeHead(403).end();
    return;
  }
  try {
    const body = await readFile(path);
    const ext = name.slice(name.lastIndexOf("."));
    res.writeHead(200, { "content-type": MIME[ext] ?? "application/octet-stream" }).end(body);
  } catch {
    res.writeHead(404).end();
  }
});
await new Promise((r) => server.listen(0, "127.0.0.1", r));
const port = server.address().port;
const base = `http://127.0.0.1:${port}/`;

const browser = await chromium.launch();
const page = await browser.newPage();

let failures = 0;
const check = async (name, fn) => {
  try {
    const detail = await fn();
    console.log(`ok   ${name}${detail ? ` — ${detail}` : ""}`);
  } catch (e) {
    failures += 1;
    console.error(`FAIL ${name} — ${e && e.message ? e.message : e}`);
  }
};

const consoleErrors = [];
page.on("pageerror", (e) => consoleErrors.push(String(e)));

await page.goto(`${base}hikari_wasm.html`).catch(() => {});
await page.setContent(`<script type="module">
  import init, * as m from '${base}hikari_wasm.js';
  window.__ready = init().then(() => { window.__mod = m; });
</script>`);

await check("the module initialises in a browser", async () => {
  await page.evaluate(() => window.__ready);
  const v = await page.evaluate(() => window.__mod.version());
  if (!/^\d+\.\d+\.\d+/.test(v)) throw new Error(`version() returned ${JSON.stringify(v)}`);
  return v;
});

await check("exports are present", async () => {
  // The API the current crate exposes. Checked by name because JavaScript is
  // not type-checked against the Rust side, so a renamed `#[wasm_bindgen]`
  // function would compile, bundle, and fail only here.
  //
  // This check reported `register_font` missing against the bundle on this
  // machine, which turned out to be a stale local build (v0.17.0, gitignored,
  // twelve releases old) rather than a removed export. The check is right and
  // the artifact was wrong, which is the outcome worth having.
  const names = await page.evaluate(() =>
    ["version", "render_png", "render_svg", "register_font"].filter(
      (n) => typeof window.__mod[n] !== "function",
    ),
  );
  if (names.length) {
    const v = await page.evaluate(() => window.__mod.version());
    throw new Error(
      `missing or non-callable: ${names.join(", ")} (bundle reports ${v}; ` +
        `rebuild it if it is older than the crate)`,
    );
  }
  return "version, render_png, render_svg, register_font";
});

await check("render_png produces a real PNG", async () => {
  const result = await page.evaluate(() => {
    const tree = JSON.stringify({
      Container: {
        style: {
          width: 200, height: 120,
          background: { Solid: { r: 15, g: 23, b: 42, a: 255 } },
          padding: 12,
          font_size: 28,
          color: { r: 248, g: 250, b: 252, a: 255 },
        },
        children: [{ Text: { text: "WASM", style: {} } }],
      },
    });
    const bytes = window.__mod.render_png(tree, 200, 120);
    return { len: bytes.length, head: Array.from(bytes.slice(0, 8)) };
  });
  const sig = [137, 80, 78, 71, 13, 10, 26, 10];
  if (result.head.join(",") !== sig.join(",")) {
    throw new Error(`not a PNG: ${result.head.join(" ")}`);
  }
  if (result.len <= 200) throw new Error(`suspiciously small: ${result.len} bytes`);
  return `${result.len} bytes`;
});

await check("render_svg returns SVG", async () => {
  const svg = await page.evaluate(() => {
    const tree = JSON.stringify({
      Container: {
        style: {
          width: 100,
          height: 60,
          background: { Solid: { r: 18, g: 52, b: 86, a: 255 } },
          children: [],
        },
        children: [],
      },
    });
    return window.__mod.render_svg(tree, 100, 60);
  });
  if (typeof svg !== "string" || !svg.includes("<svg")) {
    throw new Error(`render_svg returned ${typeof svg}`);
  }
  return `${svg.length} chars`;
});

await check("malformed input throws rather than corrupting state", async () => {
  const msg = await page.evaluate(() => {
    try {
      window.__mod.render_png("{not json", 10, 10);
      return null;
    } catch (e) {
      return String(e && e.message ? e.message : e);
    }
  });
  if (msg === null) throw new Error("malformed input silently succeeded");
  // And the module still works afterwards, which is the part that matters: a
  // throw that poisons the instance would be worse than the throw itself.
  await page.evaluate(() => window.__mod.version());
  return msg.slice(0, 60);
});

await check("no uncaught page errors", async () => {
  if (consoleErrors.length) throw new Error(consoleErrors.join("; "));
  return "clean";
});

await browser.close();
server.close();

if (failures > 0) {
  console.error(`\n${failures} check(s) failed`);
  process.exit(1);
}
console.log("\nall checks passed");
void pathToFileURL;