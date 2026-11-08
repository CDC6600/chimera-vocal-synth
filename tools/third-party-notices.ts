/**
 * THIRD-PARTY-LICENSES.txt for the plugin's download: the licence of everything compiled into it or
 * shown by its page. Built from what the build really uses, so it can't go stale: the plugin crate's
 * normal dependencies from `cargo metadata`, the page's from package.json, and what `clap-wrapper`
 * embeds. Fails if a dependency has no licence at all, or only a GPL one, which an MIT plugin can't
 * ship (nih-plug's own VST3 export is why this check exists).
 *
 * Usage: bun tools/third-party-notices.ts > target/bundled/THIRD-PARTY-LICENSES.txt
 */
import { execSync } from "node:child_process";
import { existsSync, readFileSync, readdirSync } from "node:fs";
import { dirname, join } from "node:path";

const TARGET = "aarch64-apple-darwin";

interface Pkg {
  id: string;
  name: string;
  version: string;
  license: string | null;
  repository: string | null;
  authors: string[];
  manifest_path: string;
  source: string | null;
}
interface Node {
  id: string;
  deps: { pkg: string; dep_kinds: { kind: string | null }[] }[];
}

/** For the crates that declare MIT but ship no licence file. */
const MIT = `Permission is hereby granted, free of charge, to any person obtaining a copy of this software and
associated documentation files (the "Software"), to deal in the Software without restriction, including
without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the
following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial
portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT
LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO
EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER
IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE
USE OR OTHER DEALINGS IN THE SOFTWARE.`;

/** Licence texts seen, so the many identical ones are printed once. */
const seen = new Map<string, string>();
const out: string[] = [];

function section(title: string, details: string[], texts: { name: string; text: string }[]): void {
  out.push(`${"=".repeat(78)}\n${title}\n${details.filter(Boolean).join("\n")}`);
  for (const { name, text } of texts) {
    const key = text.replace(/\s+/g, " ").trim();
    const first = seen.get(key);
    if (first) out.push(`${name}: the same text as for ${first}, above.`);
    else {
      seen.set(key, title);
      out.push(`--- ${name}\n${text.trim()}`);
    }
  }
  out.push("");
}

const licenceFiles = (dir: string) =>
  readdirSync(dir)
    .filter((f) => /^(licen[cs]e|copying|notice|unlicense)/i.test(f))
    .sort()
    .map((name) => ({ name, text: readFileSync(join(dir, name), "utf8") }));

// ---- Rust
const meta = JSON.parse(execSync(`cargo metadata --format-version 1 --locked --filter-platform ${TARGET}`, { maxBuffer: 1 << 29 }).toString()) as { packages: Pkg[]; resolve: { nodes: Node[] } };
const packages = new Map(meta.packages.map((p) => [p.id, p]));
const nodes = new Map(meta.resolve.nodes.map((n) => [n.id, n]));
const root = meta.packages.find((p) => p.name === "chimera-plugin")!;
const crates = new Set<string>();
const walk = (id: string) => {
  for (const d of nodes.get(id)!.deps) {
    if (!d.dep_kinds.some((k) => k.kind === null) || crates.has(d.pkg)) continue; // not what ships: build and dev dependencies
    crates.add(d.pkg);
    walk(d.pkg);
  }
};
walk(root.id);

out.push(
  "CHIMERA is MIT licensed (see LICENSE in the source, https://github.com/jamesondh/chimera-vocal-synth).",
  "It includes the following third-party software and data, under their own licences.",
  "",
);

const missing: string[] = [];
const copyleft: string[] = [];
for (const p of [...crates].map((id) => packages.get(id)!).sort((a, b) => a.name.localeCompare(b.name))) {
  if (!p.source) continue; // this repository
  const dir = dirname(p.manifest_path);
  let texts = licenceFiles(dir);
  // several crates of a git repository keep their licence at its root
  if (!texts.length && p.source.startsWith("git+")) {
    for (let up = dirname(dir), i = 0; i < 3 && !texts.length; up = dirname(up), i++) texts = licenceFiles(up);
  }
  if (p.license && p.license.split(/ OR |\//).every((l) => /\bA?GPL/.test(l))) copyleft.push(`${p.name} ${p.version} (${p.license})`);
  if (!texts.length && p.license && /\bMIT\b/.test(p.license)) texts = [{ name: "the MIT licence, as the crate declares (it ships no licence file)", text: MIT }];
  if (!texts.length && !p.license) missing.push(`${p.name} ${p.version}`);
  section(`${p.name} ${p.version}`, [p.license ? `Licence: ${p.license}` : "", p.repository ?? "", p.authors.length ? `Authors: ${p.authors.join(", ")}` : ""], texts);
}

// ---- what clap-wrapper compiles from its own sources
const wrapper = meta.packages.find((p) => p.name === "clap-wrapper")!;
const external = join(dirname(wrapper.manifest_path), "external");
for (const [title, path] of [
  ["clap-wrapper (C++)", "clap-wrapper/LICENSE"],
  ["CLAP SDK", "clap/LICENSE"],
  ["ghc::filesystem", "filesystem/LICENSE"],
  ["VST 3 SDK, Steinberg Media Technologies GmbH", "vst3sdk/LICENSE.txt"],
  ["AudioUnitSDK, Apple Inc.", "AudioUnitSDK/LICENSE.txt"],
] as const) {
  section(title, [], [{ name: "LICENSE", text: readFileSync(join(external, path), "utf8") }]);
}
const fmt = readFileSync(join(external, "clap-wrapper/libs/fmt/fmt/format.h"), "utf8");
section("{fmt}", [], [{ name: "licence, from fmt/format.h", text: fmt.slice(fmt.indexOf("Copyright"), fmt.indexOf("*/")) }]);

// ---- the page's JavaScript
const js = new Set<string>();
const walkJs = (name: string) => {
  if (js.has(name)) return;
  const manifest = join("node_modules", name, "package.json");
  if (!existsSync(manifest)) return;
  js.add(name);
  for (const d of Object.keys((JSON.parse(readFileSync(manifest, "utf8")) as { dependencies?: Record<string, string> }).dependencies ?? {})) walkJs(d);
};
for (const d of Object.keys((JSON.parse(readFileSync("package.json", "utf8")) as { dependencies: Record<string, string> }).dependencies)) walkJs(d);
for (const name of [...js].sort()) {
  const dir = join("node_modules", name);
  const m = JSON.parse(readFileSync(join(dir, "package.json"), "utf8")) as { version: string; license?: string };
  const texts = licenceFiles(dir);
  if (!texts.length) missing.push(`${name} (npm)`);
  section(`${name} ${m.version} (JavaScript)`, [m.license ? `Licence: ${m.license}` : ""], texts);
}

// ---- data
section("CMU Pronouncing Dictionary", [], [{ name: "licence", text: readFileSync("public/data/CMUDICT-LICENSE.txt", "utf8") }]);

if (missing.length) console.error(`no licence found for: ${missing.join(", ")}`);
if (copyleft.length) console.error(`GPL only, which can't ship in an MIT plugin: ${copyleft.join(", ")}`);
if (missing.length || copyleft.length) process.exit(1);
console.log(out.join("\n"));
