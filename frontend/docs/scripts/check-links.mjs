#!/usr/bin/env node
// Internal-link audit for the docs. Replicates fumadocs-core getSlugs():
// route-group folders matching /^\(.+\)$/ are stripped from the URL; real
// folder names are kept; an `index` file maps to its folder.
// Fails (exit 1) if any in-page `/docs/...` link has no corresponding page.
import { readdirSync, readFileSync, statSync } from "node:fs";
import { basename, dirname, extname, join, relative } from "node:path";

const ROOT = new URL("../content/docs", import.meta.url).pathname;
const GROUP = /^\(.+\)$/;

function walk(dir) {
  const out = [];
  for (const e of readdirSync(dir)) {
    const p = join(dir, e);
    if (statSync(p).isDirectory()) out.push(...walk(p));
    else if (extname(p) === ".mdx") out.push(p);
  }
  return out;
}

function slugFor(file) {
  const rel = relative(ROOT, file);
  const dir = dirname(rel);
  const name = basename(rel, extname(rel));
  const segs = [];
  if (dir !== ".")
    for (const s of dir.split("/")) if (s && !GROUP.test(s)) segs.push(s);
  if (name !== "index") segs.push(name);
  return "/docs" + (segs.length ? "/" + segs.join("/") : "");
}

const files = walk(ROOT);
const pages = new Set(files.map(slugFor));

const linkRe = /(?:\]\(|href=["'])(\/docs[^)"'#\s]*)/g;
let broken = 0;
for (const f of files) {
  const text = readFileSync(f, "utf8");
  for (const m of text.matchAll(linkRe)) {
    const target = m[1].replace(/\/$/, "");
    if (!pages.has(target)) {
      console.error(`BROKEN  ${relative(ROOT, f)}  ->  ${target}`);
      broken++;
    }
  }
}

console.log(
  `\n${files.length} pages, ${pages.size} routes; ${broken} broken internal link(s).`,
);
process.exit(broken ? 1 : 0);
