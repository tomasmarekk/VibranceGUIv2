/** Exercises release authorization against mocked GitHub responses without publishing.
 * These tests prevent absence, API errors and an unrelated CI success from being conflated.
 */
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { runInNewContext } from "node:vm";

const source = readFileSync(new URL("./check-release.mjs", import.meta.url), "utf8");
const sha = "a".repeat(40);

async function check({ ref = "refs/heads/main", tagStatus = 404, releaseStatus = 404, ciStatus = 200, runs = [{ head_sha: sha, conclusion: "success" }] } = {}) {
  const requests = [];
  await runInNewContext(`(async () => { ${source}\n})()`, {
    process: { env: { GH_TOKEN: "test-token", GITHUB_REPOSITORY: "owner/repository", GITHUB_SHA: sha, GITHUB_REF: ref, RELEASE_VERSION: "2.0.0" } },
    AbortSignal,
    console: { log() {} },
    fetch: async (url) => {
      requests.push(url);
      const status = url.includes("git/ref/tags/") ? tagStatus : url.includes("releases/tags/") ? releaseStatus : ciStatus;
      return { status, ok: status === 200, json: async () => ({ workflow_runs: runs }) };
    },
  });
  return requests;
}

test("accepts an unused version only after CI succeeds for the same main commit", async () => {
  const requests = await check();
  assert.equal(requests.length, 3);
  assert.match(requests[2], /branch=main&event=push&head_sha=a{40}/);
});

test("rejects a release dispatched from another branch before requesting GitHub", async () => {
  await assert.rejects(check({ ref: "refs/heads/feature" }), /dispatched from main/);
});

test("does not overwrite an existing tag or release", async () => {
  await assert.rejects(check({ tagStatus: 200 }), /tag v2\.0\.0 already exists/);
  await assert.rejects(check({ releaseStatus: 200 }), /release v2\.0\.0 already exists/);
});

test("rejects unrelated, unfinished or failed CI runs", async () => {
  for (const runs of [[], [{ head_sha: "b".repeat(40), conclusion: "success" }], [{ head_sha: sha, conclusion: null }], [{ head_sha: sha, conclusion: "failure" }]]) {
    await assert.rejects(check({ runs }), /must first pass/);
  }
});

test("authentication and service failures never count as a missing tag or successful CI", async () => {
  await assert.rejects(check({ tagStatus: 403 }), /HTTP 403/);
  await assert.rejects(check({ releaseStatus: 500 }), /HTTP 500/);
  await assert.rejects(check({ ciStatus: 404 }), /HTTP 404/);
});
