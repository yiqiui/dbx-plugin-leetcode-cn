// Drives the built sidecar over stdio exactly like the DBX host does and checks
// the new problemset filters against the live leetcode.cn API.
//   node dev/smoke.mjs [path/to/dbx-leetcode-cn.exe]
import { spawn } from "node:child_process";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const exe = process.argv[2] || path.join(root, "backend", "target", "release", process.platform === "win32" ? "dbx-leetcode-cn.exe" : "dbx-leetcode-cn");

const child = spawn(exe, [], { stdio: ["pipe", "pipe", "inherit"] });
let buffer = "";
const pending = new Map();
child.stdout.on("data", (chunk) => {
  buffer += chunk.toString("utf8");
  let index;
  while ((index = buffer.indexOf("\n")) >= 0) {
    const line = buffer.slice(0, index).trim();
    buffer = buffer.slice(index + 1);
    if (!line) continue;
    const message = JSON.parse(line);
    if (message.id !== undefined && pending.has(message.id)) {
      const { resolve, reject } = pending.get(message.id);
      pending.delete(message.id);
      message.error ? reject(new Error(message.error.message)) : resolve(message.result);
    }
  }
});

let id = 1;
const call = (method, params, ms = 45000) => new Promise((resolve, reject) => {
  const rid = id++;
  const timer = setTimeout(() => { pending.delete(rid); reject(new Error(`${method} timed out`)); }, ms);
  pending.set(rid, { resolve: (v) => { clearTimeout(timer); resolve(v); }, reject: (e) => { clearTimeout(timer); reject(e); } });
  child.stdin.write(JSON.stringify({ jsonrpc: "2.0", id: rid, method, params }) + "\n");
});

const checks = [];
const check = (name, ok, detail) => {
  checks.push({ name, ok });
  console.log(`${ok ? "PASS" : "FAIL"}  ${name}${ok ? "" : `  -> ${detail}`}`);
};

try {
  const hello = await call("plugin/initialize", { host: { protocolVersions: [1] }, plugin: { id: "com.yiqiui.leetcode-cn", version: "0.3.0" }, permissions: [] });
  check("handshake v1", hello.protocolVersion === 1, JSON.stringify(hello));

  const tags = await call("leetcode/topic_tags", {});
  check("topic_tags returns the full tag set", (tags.tags || []).length >= 70, `got ${tags.tags?.length}`);
  const top = tags.tags?.[0];
  check("top tag is 数组 with a live count", top?.slug === "array" && top.count > 2000, JSON.stringify(top));
  check("counts are sorted descending", (tags.tags || []).every((t, i, a) => i === 0 || a[i - 1].count >= t.count), "order");
  console.log(`      tags=${tags.total} top5=${(tags.tags || []).slice(0, 5).map((t) => `${t.translatedName || t.name}:${t.count}`).join(" ")}`);

  const byTag = await call("leetcode/list_problems", { skip: 0, limit: 5, filters: { tags: ["array"] } });
  check("filter by tag 数组", byTag.total === (tags.tags.find((t) => t.slug === "array") || {}).count, `list=${byTag.total} tag=${(tags.tags.find((t) => t.slug === "array") || {}).count}`);
  check("tagged rows carry topicTags", (byTag.questions?.[0]?.topicTags || []).length > 0, JSON.stringify(byTag.questions?.[0]?.topicTags));

  const multi = await call("leetcode/list_problems", { skip: 0, limit: 5, filters: { tags: ["array", "dynamic-programming"] } });
  check("multi-tag filter is accepted", typeof multi.total === "number" && multi.total > 0, JSON.stringify(multi).slice(0, 120));

  const easy = await call("leetcode/list_problems", { skip: 0, limit: 5, filters: { difficulty: "EASY" } });
  check("difficulty filter", easy.total > 500 && easy.questions?.[0]?.difficulty === "EASY", `total=${easy.total} first=${easy.questions?.[0]?.difficulty}`);

  const db = await call("leetcode/list_problems", { skip: 0, limit: 5, categorySlug: "database", filters: {} });
  check("category tab 数据库", db.total > 100 && db.total < 500, `total=${db.total}`);
  const shell = await call("leetcode/list_problems", { skip: 0, limit: 5, categorySlug: "shell", filters: {} });
  check("category tab Shell", shell.total === 4, `total=${shell.total}`);
  const conc = await call("leetcode/list_problems", { skip: 0, limit: 5, categorySlug: "concurrency", filters: {} });
  check("category tab 多线程", conc.total > 0 && conc.total < 30, `total=${conc.total}`);

  const combined = await call("leetcode/list_problems", { skip: 0, limit: 5, categorySlug: "algorithms", filters: { tags: ["binary-search"], difficulty: "MEDIUM" } });
  check("category + tag + difficulty combine", combined.total > 0, `total=${combined.total}`);

  const paged = await call("leetcode/list_problems", { skip: 5, limit: 3, filters: {} });
  check("paging still works", (paged.questions || []).length === 3 && paged.questions[0].frontendQuestionId !== "1", JSON.stringify(paged.questions?.map((q) => q.frontendQuestionId)));

  try {
    await call("leetcode/list_problems", { skip: 0, limit: 5, searchKeyword: "two sum", filters: {} });
    console.log("      note: server search succeeded without a session (site allowed it)");
  } catch (error) {
    check("anonymous search reports a sign-in hint", /登录|login|sign/i.test(String(error.message)), error.message);
  }
} catch (error) {
  check("smoke run completed", false, error.message);
} finally {
  child.stdin.end();
  child.kill();
}

const failed = checks.filter((item) => !item.ok);
console.log(`\n${checks.length - failed.length}/${checks.length} checks passed`);
process.exit(failed.length ? 1 : 0);
