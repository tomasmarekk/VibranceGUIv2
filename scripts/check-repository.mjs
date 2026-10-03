/** Repository release invariants shared by local verification and GitHub Actions.
 * The private reference tree must remain outside Git and all published artifacts.
 */
import { readFileSync } from "node:fs";
import { spawnSync } from "node:child_process";

const semver = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-((?:0|[1-9]\d*|\d*[A-Za-z-][0-9A-Za-z-]*)(?:\.(?:0|[1-9]\d*|\d*[A-Za-z-][0-9A-Za-z-]*))*))?(?:\+([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?$/;

const fail = (message) => { throw new Error(message); };
const json = (file) => JSON.parse(readFileSync(file, "utf8"));
const requested = process.argv[2];
if (process.argv.length > 3) fail("expected at most one release version argument");

const packageJson = json("package.json");
const packageLock = json("package-lock.json");
const cargo = readFileSync("src-tauri/Cargo.toml", "utf8");
const packageSection = cargo.split(/^\[package\]\s*$/m)[1]?.split(/^\[/m)[0];
const cargoVersion = packageSection?.match(/^version\s*=\s*"([^"]+)"\s*$/m)?.[1];
const cargoLockPackage = readFileSync("Cargo.lock", "utf8").split(/^\[\[package\]\]\s*$/m)
  .find((section) => /^name\s*=\s*"vibrance-gui-v2"\s*$/m.test(section));
const cargoLockVersion = cargoLockPackage?.match(/^version\s*=\s*"([^"]+)"\s*$/m)?.[1];
const versions = {
  "package.json": packageJson.version,
  "package-lock.json": packageLock.version,
  "package-lock.json root package": packageLock.packages?.[""]?.version,
  "src-tauri/Cargo.toml": cargoVersion,
  "Cargo.lock application package": cargoLockVersion,
  "src-tauri/tauri.conf.json": json("src-tauri/tauri.conf.json").version,
};

for (const [file, version] of Object.entries(versions)) {
  if (typeof version !== "string" || !semver.test(version)) fail(`invalid SemVer in ${file}`);
  if (version !== packageJson.version) fail(`version mismatch: ${file} is ${version}, expected ${packageJson.version}`);
}
if (requested !== undefined && (!semver.test(requested) || requested !== packageJson.version)) {
  fail(`release version must exactly equal the checked-in version ${packageJson.version}`);
}

const tracked = spawnSync("git", ["ls-files", "-z"], { encoding: "utf8" });
if (tracked.status !== 0) fail("could not inspect tracked files");
const forbidden = tracked.stdout.split("\0").filter((file) =>
  /(^|\/)reference-source(\/|$)/i.test(file) || /(^|\/)vibranceDLL\.dll$/i.test(file),
);
if (forbidden.length > 0) fail(`reference material is tracked: ${forbidden.join(", ")}`);

const ignored = spawnSync("git", ["check-ignore", "--no-index", "--quiet", "--", "reference-source/.release-exclusion-check"]);
if (ignored.status !== 0) fail("reference-source must be excluded by .gitignore");

console.log(`Repository guard passed; synchronized version ${packageJson.version}; reference sources excluded.`);
