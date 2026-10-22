#!/usr/bin/env node
// ---------------------------------------------------------------------------
// The performance gate.
//
// Rationale: the render benchmarks are documented to run on a local machine,
// and CI originally had a bench-gate job that needed a self-hosted runner which
// does not exist. It was removed, and with it any automatic check that a merge
// had not silently made the renderer slower -- the exact class of defect that
// the workload benchmarks exist to catch (the 300x image-layout bug was found
// in the first run of a benchmark, months after it was introduced).
//
// This script closes that hole with honest limits rather than pretending VM
// timings are as repeatable as an M2 desk machine:
//
//   - It compares criterion medians against a committed baseline file.
//   - Two benchmark passes run per job, and the better (lower) median of the
//     two wins per key -- a shared runner has transient noisy intervals and
//     best-of-two removes most of them.
//   - Keys whose baseline median is under --floor (default 20 microseconds)
//     are reported but never gate. A 2x move on a 4-microsecond layout is
//     not measurable on a VM and has never been the risk.
//   - A key only fails the job if the ratio persists across both passes.
//   - Default threshold is --threshold 2.0. This gate exists to catch
//     catastrophic regressions (the class that every previous incident was),
//     not a five-percent drift, which needs controlled hardware and is what
//     the published benchmark runs are for.
//
// Modes:
//   --mode compare  compare against .github/perf-baseline.json (default)
//   --mode write    write a fresh baseline from the passes and exit 0
//
// Exit codes: 0 pass (or rebaseline), 1 regression, 2 usage error.
// ---------------------------------------------------------------------------

import fs from "node:fs";
import path from "node:path";

/** @param {string} name */
const flag = (name) => {
  const i = process.argv.indexOf(`--${name}`);
  return i >= 0 && i + 1 < process.argv.length ? process.argv[i + 1] : undefined;
};

const mode = flag("mode") ?? "compare";
const first = flag("first");
const second = flag("second");
const baselinePath = flag("baseline") ?? ".github/perf-baseline.json";
const threshold = Number(flag("threshold") ?? 2.0);
const floorNs = Number(flag("floor") ?? 20_000);
const outPath = flag("out");

if (!first || !second) usage("--first and --second (criterion roots of the two passes)");

/** @returns {Record<string, number>} key -> median point estimate, ns */
function collect(criterionRoot) {
  if (!fs.existsSync(criterionRoot)) return {};
  const out = {};
  for (const file of walk(criterionRoot)) {
    if (!/[/\\](new|base)[/\\]estimates\.json$/.test(file)) continue;
    const key = path.relative(criterionRoot, file)
      .replace(/[\\/](new|base)[\\/]estimates\.json$/, "")
      .replaceAll(path.sep, "/");
    const data = JSON.parse(fs.readFileSync(file, "utf8"));
    const ns = data?.median?.point_estimate;
    if (typeof ns === "number" && Number.isFinite(ns) && ns > 0) {
      out[key] = ns;
    }
  }
  return out;
}

/** @param {string} dir @returns {string[]} */
function walk(dir) {
  const files = [];
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const p = path.join(dir, entry.name);
    if (entry.isDirectory()) files.push(...walk(p));
    else files.push(p);
  }
  return files;
}

const a = collect(first);
const b = collect(second);
/** Best of the two passes, per key. */
const best = Object.fromEntries(
  [...new Set([...Object.keys(a), ...Object.keys(b)])]
    .map((key) => [key, Math.min(a[key] ?? Infinity, b[key] ?? Infinity)]),
);

if (outPath) fs.writeFileSync(outPath, JSON.stringify({ a, b, best }, null, 2));
else if (Object.keys(best).length === 0) {
  console.error("no criterion estimates found under", first, "or", second);
  process.exit(2);
}

/** Pretty ns -> 0.15 ms formatting for the log. */
function fmt(ns) {
  if (ns >= 1e6) return `${(ns / 1e6).toFixed(2)} ms`;
  if (ns >= 1e3) return `${(ns / 1e3).toFixed(1)} us`;
  return `${ns.toFixed(0)} ns`;
}

const summaryLines = [];
const add = (line) => { summaryLines.push(line); console.log(line); };

if (mode === "write") {
  fs.mkdirSync(path.dirname(baselinePath), { recursive: true });
  const meta = {
    _meta: {
      note: "Committed by the perf workflow; edit by hand only diversely down",
      generatedBy: process.env.GITHUB_RUN_ID ?? "local",
      generatedAt: new Date().toISOString(),
      threshold: String(threshold),
      floorNs: String(floorNs),
    },
  };
  fs.writeFileSync(
    baselinePath,
    JSON.stringify({ ...meta, ...best }, null, 2) + "\n",
  );
  add(`baseline written: ${Object.keys(best).length} keys -> ${baselinePath}`);
  process.exit(0);
}

if (!fs.existsSync(baselinePath)) {
  // Bootstrap: the very first run has nothing to compare against. Write the
  // baseline, say so, and pass, so the gate turns on automatically rather
  // than requiring a manual unblocking step someone will forget.
  fs.mkdirSync(path.dirname(baselinePath), { recursive: true });
  fs.writeFileSync(baselinePath, JSON.stringify({ ...best }, null, 2) + "\n");
  add(`::notice::no baseline found — wrote a fresh one with ${Object.keys(best).length} keys. From the next run on, regressions will fail this job.`);
  process.exit(0);
}

const baseline = JSON.parse(fs.readFileSync(baselinePath, "utf8"));
const floorKeys = new Set(
  Object.entries(baseline)
    .filter(([key, ns]) => key !== "_meta" && Number(ns) < floorNs)
    .map(([key]) => key),
);

const regressions = [];
const improvements = [];
const newKeys = [];
const skipped = [];
let compared = 0;

for (const [key, now] of Object.entries(best).sort()) {
  const base = baseline[key];
  if (typeof base !== "number" || base <= 0) {
    newKeys.push([key, now]);
    continue;
  }
  compared++;
  const ratio = now / base;
  if (ratio >= threshold && !floorKeys.has(key)) {
    regressions.push([key, base, now, ratio]);
  } else if (ratio <= 1 / threshold) {
    improvements.push([key, base, now, ratio]);
  }
}

for (const key of floorKeys) {
  if (best[key] !== undefined) skipped.push(key);
}

add("");
add(`perf gate: ${compared} keys compared (threshold ${threshold}x, floor ${fmt(floorNs)})`);
add("");
add("| key | baseline | this run | ratio |");
add("|---|---:|---:|---:|");
for (const [key, now] of newKeys) add(`| ${key} (new) | — | ${fmt(now)} | |`);
for (const [key, base, now, ratio] of regressions) {
  add(`| ${key} | ${fmt(base)} | ${fmt(now)} | **${ratio.toFixed(2)}x** |`);
}
for (const [key, base, now, ratio] of improvements) {
  add(`| ${key} | ${fmt(base)} | ${fmt(now)} | ${ratio.toFixed(2)}x faster |`);
}
add("");
if (skipped.length) {
  add(`under floor (not gated): ${skipped.join(", ")}`);
  add("");
}

if (regressions.length) {
  console.error(`::error::${regressions.length} regression(s) at or above ${threshold}x, persisting across both benchmark passes: ${regressions.map(([k, , , r]) => `${k} ${r.toFixed(2)}x`).join(", ")}`);
  process.exit(1);
}

if (summaryLines) {
  const stepSummary = process.env.GITHUB_STEP_SUMMARY;
  if (stepSummary) fs.appendFileSync(stepSummary, summaryLines.join("\n") + "\n");
}
process.exit(0);
