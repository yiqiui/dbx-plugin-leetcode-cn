// DOM-level test for the problemset filter bar: every control must actually
// reach the sidecar with the right arguments. jsdom + a stubbed dbxPlugin bridge.
//   npm install jsdom && node dev/ui.test.mjs
import { readFileSync } from "node:fs";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";
import { JSDOM } from "jsdom";

const here = path.dirname(fileURLToPath(import.meta.url));
const html = readFileSync(path.join(here, "..", "ui", "index.html"), "utf8");

const TAGS = Array.from({ length: 14 }, (_, i) => ({
  slug: `tag-${i}`,
  name: `Tag ${i}`,
  translatedName: `标签${i}`,
  count: 3000 - i * 100,
}));
TAGS[0] = { slug: "array", name: "Array", translatedName: "数组", count: 2430 };

const questions = [
  { frontendQuestionId: "1", title: "Two Sum", titleCn: "两数之和", titleSlug: "two-sum", difficulty: "EASY", status: "AC", acRate: 0.52, topicTags: [{ slug: "array", name: "Array" }] },
  { frontendQuestionId: "2", title: "Add Two Numbers", titleCn: "两数相加", titleSlug: "add-two-numbers", difficulty: "MEDIUM", status: "-", acRate: 0.44, topicTags: [] },
];

const calls = [];
let lastListParams = null;

const dom = new JSDOM(html, {
  runScripts: "dangerously",
  url: "http://localhost/",
  pretendToBeVisual: true,
  beforeParse(window) {
    window.addEventListener("error", (event) => calls.push(["window-error", String(event.message)]));
    let resolveReady;
    window.dbxPlugin = {
      ready: new Promise((resolve) => { resolveReady = resolve; }),
      request(channel, payload) {
        const method = payload?.method;
        const params = payload?.params || {};
        calls.push([method, params]);
        if (method === "leetcode/status") return Promise.resolve({ authenticated: true, username: "tester", nickname: "测试者" });
        if (method === "leetcode/topic_tags") return Promise.resolve({ tags: TAGS, total: TAGS.length });
        if (method === "leetcode/list_problems") {
          lastListParams = params;
          return Promise.resolve({ total: params.filters?.tags?.length ? 2430 : 4448, questions });
        }
        return Promise.resolve({});
      },
      onEvent() {},
      onContext() {},
      capabilities: {},
      locale: "zh-CN",
      theme: { appearance: "light", tokens: {} },
      context: {},
    };
    window.__resolveReady = () => resolveReady();
  },
});

const { window } = dom;
const document = window.document;
const settle = async (ms = 40) => { await new Promise((resolve) => setTimeout(resolve, ms)); await new Promise((resolve) => setTimeout(resolve, ms)); };
const results = [];
const check = (name, ok, detail) => {
  results.push({ name, ok });
  console.log(`${ok ? "PASS" : "FAIL"}  ${name}${ok ? "" : `  -> ${detail}`}`);
};
const methodCalls = (method) => calls.filter(([name]) => name === method);
const text = (selector) => [...document.querySelectorAll(selector)].map((node) => node.textContent.trim());

window.__resolveReady();
await settle(120);

// ---- initial render
check("7 official category tabs rendered", document.querySelectorAll("#cats .cat").length === 7, `${document.querySelectorAll("#cats .cat").length}`);
check("tab labels match the site", JSON.stringify(text("#cats .cat")) === JSON.stringify(["全部题目", "算法", "数据库", "Shell", "多线程", "JavaScript", "pandas"]), text("#cats .cat").join("/"));
check("first tab is selected", document.querySelector("#cats .cat")?.getAttribute("aria-selected") === "true", "no selection");
check("tags loaded from the live API", methodCalls("leetcode/topic_tags").length >= 1, "topic_tags never called");
check("tag chips show counts", text("#tags .tag[data-slug]")[0]?.includes("2430"), text("#tags .tag[data-slug]")[0]);
check("tag bar collapses to 12 chips", document.querySelectorAll("#tags .tag[data-slug]").length === 12, `${document.querySelectorAll("#tags .tag[data-slug]").length}`);
check("展开 control present", !!document.querySelector("#tagsMore"), "missing");
check("problem list rendered", document.querySelectorAll("#qbody tr.q").length === 2, `${document.querySelectorAll("#qbody tr.q").length}`);
check("status line summarizes active filters", document.querySelector("#listStatus").textContent.includes("全部题目"), document.querySelector("#listStatus").textContent);

// ---- category tab click
document.querySelectorAll("#cats .cat")[2].click();
await settle(120);
check("clicking 数据库 sends categorySlug=database", lastListParams?.categorySlug === "database", JSON.stringify(lastListParams));
check("clicked tab becomes selected", document.querySelectorAll("#cats .cat")[2].getAttribute("aria-selected") === "true", "not selected");

// ---- tag chip click
const arrayChip = [...document.querySelectorAll("#tags .tag[data-slug]")].find((chip) => chip.dataset.slug === "array");
arrayChip.click();
await settle(120);
check("clicking a tag sends filters.tags", JSON.stringify(lastListParams?.filters?.tags) === '["array"]', JSON.stringify(lastListParams?.filters));
check("selected tag is pressed", document.querySelector('#tags .tag[data-slug="array"]').getAttribute("aria-pressed") === "true", "not pressed");
check("清除标签 appears when a tag is active", !!document.querySelector("#tagsClear"), "missing");

// ---- expand / collapse
document.querySelector("#tagsMore").click();
await settle(60);
check("展开 reveals all tags", document.querySelectorAll("#tags .tag[data-slug]").length === 14, `${document.querySelectorAll("#tags .tag[data-slug]").length}`);
document.querySelector("#tagsMore").click();
await settle(60);
check("收起 collapses again", document.querySelectorAll("#tags .tag[data-slug]").length === 12, `${document.querySelectorAll("#tags .tag[data-slug]").length}`);

// ---- difficulty + status
document.querySelector("#difficulty").value = "HARD";
document.querySelector("#difficulty").dispatchEvent(new window.Event("change", { bubbles: true }));
await settle(120);
check("difficulty reaches the sidecar", lastListParams?.filters?.difficulty === "HARD", JSON.stringify(lastListParams?.filters));
document.querySelector("#status").value = "AC";
document.querySelector("#status").dispatchEvent(new window.Event("change", { bubbles: true }));
await settle(120);
check("作答状态 reaches the sidecar", lastListParams?.filters?.status === "AC", JSON.stringify(lastListParams?.filters));
check("status line reflects difficulty + status", /困难/.test(document.querySelector("#listStatus").textContent), document.querySelector("#listStatus").textContent);

// ---- server-side search (debounced)
const search = document.querySelector("#search");
search.value = "两数";
search.dispatchEvent(new window.Event("input", { bubbles: true }));
await settle(120);
check("search is debounced (not fired immediately)", lastListParams?.searchKeyword !== "两数", JSON.stringify(lastListParams?.searchKeyword));
await settle(700);
check("debounced search sends searchKeyword", lastListParams?.searchKeyword === "两数", JSON.stringify(lastListParams?.searchKeyword));
check("server search skips local re-filtering", document.querySelectorAll("#qbody tr.q").length === 2, `${document.querySelectorAll("#qbody tr.q").length}`);

// ---- reset
document.querySelector("#resetBtn").click();
await settle(150);
check("reset clears category", lastListParams?.categorySlug === "", JSON.stringify(lastListParams?.categorySlug));
check("reset clears tags/difficulty/status/keyword", !lastListParams?.filters?.tags && !lastListParams?.filters?.difficulty && !lastListParams?.filters?.status && !lastListParams?.searchKeyword, JSON.stringify(lastListParams));
check("reset restores selects", document.querySelector("#difficulty").value === "" && document.querySelector("#status").value === "" && document.querySelector("#search").value === "", "inputs not cleared");

// ---- paging still wired
check("pager controls present", !!document.querySelector("#prevBtn") && !!document.querySelector("#nextBtn"), "missing");

const failed = results.filter((item) => !item.ok);
console.log(`\n${results.length - failed.length}/${results.length} checks passed`);
window.close();
process.exit(failed.length ? 1 : 0);
