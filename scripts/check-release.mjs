/** Refuses to publish an existing version or a commit without a successful main CI run.
 * HTTP errors are distinguished from absence so network/authentication failures fail closed.
 */
const { GH_TOKEN: token, GITHUB_REPOSITORY: repository, GITHUB_SHA: sha, GITHUB_REF: ref, RELEASE_VERSION: version } = process.env;
if (!token || !repository || !sha || !version) throw new Error("release environment is incomplete");
if (ref !== "refs/heads/main") throw new Error("releases must be dispatched from main");
if (!/^[a-f0-9]{40}$/i.test(sha)) throw new Error("release target must be a full commit SHA");
const headers = { Authorization: `Bearer ${token}`, Accept: "application/vnd.github+json", "X-GitHub-Api-Version": "2022-11-28" };

async function request(path, allowMissing = false) {
  const response = await fetch(`https://api.github.com/repos/${repository}/${path}`, { headers, signal: AbortSignal.timeout(30_000) });
  if (allowMissing && response.status === 404) return null;
  if (!response.ok) throw new Error(`GitHub preflight request failed with HTTP ${response.status}`);
  return response.json();
}

const tag = `v${version}`;
if (await request(`git/ref/tags/${encodeURIComponent(tag)}`, true)) throw new Error(`tag ${tag} already exists`);
if (await request(`releases/tags/${encodeURIComponent(tag)}`, true)) throw new Error(`release ${tag} already exists`);
const runs = await request(`actions/workflows/ci.yml/runs?branch=main&event=push&head_sha=${sha}&per_page=100`);
if (!runs.workflow_runs.some((run) => run.head_sha === sha && run.conclusion === "success")) {
  throw new Error("the selected commit must first pass the Windows CI workflow on main");
}
console.log(`Release preflight passed for ${tag} at ${sha}.`);
