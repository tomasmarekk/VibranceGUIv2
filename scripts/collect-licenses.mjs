/** Collects the license files distributed with the installed dependency versions.
 * The inventory accompanies release binaries and intentionally excludes local references.
 */
import { existsSync, readFileSync, readdirSync, mkdirSync, writeFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { spawnSync } from "node:child_process";

const sections = ["VibranceGUIv2 third-party dependency notices\n\nThis inventory includes build and platform dependencies; inclusion does not imply every dependency is shipped in every binary.\n"];

function collect(label, directory, declaredLicense, source) {
  const files = readdirSync(directory, { withFileTypes: true }).filter((entry) =>
    entry.isFile() && /^(licen[sc]e|copying|notice)([-._].*)?$/i.test(entry.name),
  ).map((entry) => join(directory, entry.name));
  for (const folder of ["licenses", "LICENSES"]) {
    const path = join(directory, folder);
    if (!existsSync(path)) continue;
    for (const entry of readdirSync(path, { withFileTypes: true })) {
      if (entry.isFile()) files.push(join(path, entry.name));
    }
  }
  sections.push(`\n${"=".repeat(72)}\n${label}\nDeclared license: ${declaredLicense ?? "not declared"}\nSource: ${source ?? "see package registry"}\n`);
  for (const file of [...new Set(files)].sort()) {
    sections.push(`\n--- ${file.slice(directory.length + 1).replaceAll("\\", "/")} ---\n${readFileSync(file, "utf8")}\n`);
  }
  if (files.length === 0) sections.push("\nNo standalone license file was included in this installed package. Refer to its declared license and upstream source.\n");
}

const lock = JSON.parse(readFileSync("package-lock.json", "utf8"));
for (const path of Object.keys(lock.packages).sort()) {
  if (!path.startsWith("node_modules/")) continue;
  const manifest = join(path, "package.json");
  if (!existsSync(manifest)) continue;
  const pkg = JSON.parse(readFileSync(manifest, "utf8"));
  collect(`npm: ${pkg.name}@${pkg.version}`, path, pkg.license, `https://www.npmjs.com/package/${pkg.name}/v/${pkg.version}`);
}

const metadata = spawnSync("cargo", ["metadata", "--locked", "--format-version", "1"], { encoding: "utf8", maxBuffer: 32 * 1024 * 1024 });
if (metadata.status !== 0) throw new Error(`cargo metadata failed: ${metadata.stderr}`);
const graph = JSON.parse(metadata.stdout);
for (const pkg of graph.packages.filter((pkg) => !graph.workspace_members.includes(pkg.id)).sort((a, b) => a.id.localeCompare(b.id))) {
  collect(`Rust: ${pkg.name}@${pkg.version}`, dirname(pkg.manifest_path), pkg.license, pkg.repository);
}

mkdirSync("artifacts", { recursive: true });
writeFileSync("artifacts/THIRD-PARTY-LICENSES.txt", sections.join(""), "utf8");
console.log("Collected dependency notices into artifacts/THIRD-PARTY-LICENSES.txt");
