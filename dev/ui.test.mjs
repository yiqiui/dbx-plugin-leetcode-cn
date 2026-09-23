// DOM-level regression test for the problemset UI: every control must actually
// reach the sidecar and re-render. jsdom + a stubbed dbxPlugin bridge.
//   npm install jsdom && node dev/ui.test.mjs
import { readFileSync } from "node:fs";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";
import { JSDOM } from "jsdom";

const here = path.dirname(fileURLToPath(import.meta.url));
const html = readFileSync(path.join(here, "..", "ui", "index.html"), "utf8");

const TAGS = Array.from({ length: 79 }, (_, i) => ({
  slug: `tag-${i}`, name: `Tag ${i}`, translatedName: `标签${i}`, count: 2500 - i * 30,
}));
TAGS[0] = { slug: "array", name: "Array", translatedName: "数组", count: 2430 };

const row = (id, difficulty, acRate, favor) => ({
  frontendQuestionId: String(id), title: `T${id}`, titleCn: `题目${id}`, titleSlug: `slug-${id}`,
  difficulty, acRate, status: id === 1 ? "AC" : "-", paidOnly: false, isFavor: favor, topicTags: [{ slug: "array", name: "Array" }],
});
const PAGE_ROWS = [row(1, "EASY", 0.52, true), row(2, "MEDIUM", 0.44, false), row(3, "MEDIUM", 0.42, false)];
const FULL_ROWS = [row(1, "EASY", 0.52, true), row(2, "MEDIUM", 0.44, false), row(3, "HARD", 0.31, false), row(4, "MEDIUM", 0.9, true)];

const calls = [];
let lastListParams = null;
let lastAllParams = null;

const dom = new JSDOM(html, {
  runScripts: "dangerously",
  url: "http://localhost/",
  pretendToBeVisual: true,
  beforeParse(window) {
    let resolveReady;
    window.dbxPlugin = {
      ready: new Promise((resolve) => { resolveReady = resolve; }),
      request(channel, payload) {
        const method = payload?.method;
        const params = payload?.params || {};
        calls.push([method, params]);
        if (method === "leetcode/status") return Promise.resolve({ authenticated: true, username: "tester", nickname: "测试者", avatar: "https://assets.leetcode.cn/a.png" });
        if (method === "leetcode/topic_tags") return Promise.resolve({ tags: TAGS, total: TAGS.length });
        if (method === "leetcode/list_problems") {
          lastListParams = params;
          const questions = params.limit === 1 ? [FULL_ROWS[2]] : PAGE_ROWS;
          return Promise.resolve({ total: params.filters?.tags?.length ? 2430 : 4448, questions });
        }
        if (method === "leetcode/list_all") { lastAllParams = params; return Promise.resolve({ total: FULL_ROWS.length, questions: FULL_ROWS, truncated: false }); }
        if (method === "leetcode/daily_question") return Promise.resolve({ date: "2026-09-23", question: { titleSlug: "slug-7", questionFrontendId: "7", title: "Reverse", translatedTitle: "整数反转", difficulty: "MEDIUM", acRate: 0.35 } });
        if (method === "leetcode/toggle_favorite") return Promise.resolve({ code: 0 });
        if (method === "leetcode/open_site") return Promise.resolve({ ok: true });
        if (method === "leetcode/get_problem") return Promise.resolve({
          questionId: "7", frontendQuestionId: "7", title: "Reverse", titleCn: "整数反转", titleSlug: "slug-7",
          content: "<p>body</p>", difficulty: "MEDIUM", isPaidOnly: false, sampleTestCase: "1", stats: "{}",
          codeSnippets: [{ lang: "Python3", langSlug: "python3", code: "class Solution: pass" }], topicTags: [],
        });
        return Promise.resolve({});
      },
      onEvent() {}, onContext() {}, capabilities: {}, locale: "zh-CN",
      theme: { appearance: "light", tokens: {} }, context: {},
    };
    window.__resolveReady = () => resolveReady();
  },
});

const { window } = dom;
const document = window.document;
const settle = async (ms = 60) => { for (let i = 0; i < 4; i += 1) await new Promise((resolve) => setTimeout(resolve, ms)); };
const results = [];
const check = (name, ok, detail) => {
  results.push({ name, ok });
  console.log(`${ok ? "PASS" : "FAIL"}  ${name}${ok ? "" : `  -> ${detail}`}`);
};
const called = (method) => calls.some(([name]) => name === method);
const text = (sel) => [...document.querySelectorAll(sel)].map((n) => n.textContent.trim());
const click = (sel, match) => {
  const nodes = [...document.querySelectorAll(sel)];
  const node = match === undefined ? nodes[0]
    : typeof match === "number" ? nodes[match]
    : nodes.find((n) => (match.test ? match.test(n.textContent) : n.textContent.includes(match)));
  if (!node) throw new Error(`no node for ${sel} / ${match}`);
  node.click();
};

window.__resolveReady();
await settle(150);

// ---------- layout
check("left nav rendered", document.querySelectorAll("#nav .nav-item").length >= 6, `${document.querySelectorAll("#nav .nav-item").length}`);
check("nav has 题目/收藏/每日一题", /题目/.test(text("#nav .label").join("/")) && /我的收藏/.test(text("#nav .label").join("/")) && /每日一题/.test(text("#nav .label").join("/")), text("#nav .label").join("/"));
check("official sections present", /探险模式/.test(document.body.textContent) && /LeetBook/.test(document.body.textContent) && /学习计划/.test(document.body.textContent), "missing");
check("7 category tabs", document.querySelectorAll("#cats .cat").length === 7, `${document.querySelectorAll("#cats .cat").length}`);
check("right rail companies rendered", document.querySelectorAll("#companies button").length === 12, `${document.querySelectorAll("#companies button").length}`);

// ---------- header avatar
check("avatar badge rendered (no remote image)", !!document.querySelector("#who .avatar") && !document.querySelector("#who img"), document.querySelector("#who")?.innerHTML);
check("nickname shown", /测试者/.test(document.querySelector("#who").textContent), document.querySelector("#who").textContent);

// ---------- tags: all 79 present, clipped by CSS, toggle outside the clip
check("all tags rendered", document.querySelectorAll("#tags .tag[data-slug]").length === 79, `${document.querySelectorAll("#tags .tag[data-slug]").length}`);
check("tag counts from live API", text("#tags .tag[data-slug]")[0] === "数组2430", text("#tags .tag[data-slug]")[0]);
check("tags collapsed by default", document.querySelector("#tags").classList.contains("clip"), "not clipped");
check("toggle button is outside the clipped scroller", !!document.querySelector("#tagsToggle") && !document.querySelector("#tags #tagsToggle"), "missing/nested");
click("#tagsToggle");
await settle(60);
check("展开 removes the clip", !document.querySelector("#tags").classList.contains("clip"), "still clipped");
check("toggle label switches to 收起", /收起/.test(document.querySelector("#tagsToggle").textContent), document.querySelector("#tagsToggle").textContent);
click("#tagsToggle");
await settle(60);
check("收起 restores the clip", document.querySelector("#tags").classList.contains("clip"), "not clipped again");

// ---------- daily question
check("daily question loaded", called("leetcode/daily_question"), "not called");
check("daily card shows the question", /整数反转/.test(document.querySelector("#dailyBody").textContent), document.querySelector("#dailyBody").textContent);

// ---------- table columns
const headers = text("#qtable thead th");
check("AC column renamed to 通过率", headers.includes("通过率"), headers.join("|"));
check("difficulty shown in Chinese", /简单/.test(document.querySelector("#qbody").textContent) && !/EASY/.test(document.querySelector("#qbody").textContent), document.querySelector("#qbody").textContent.slice(0, 120));
check("every row has a star cell", document.querySelectorAll("#qbody .fav").length === document.querySelectorAll("#qbody tr.q").length, "count mismatch");
check("favorited row shows a filled star", document.querySelector("#qbody .fav.on")?.textContent === "★", document.querySelector("#qbody .fav")?.textContent);
check("tags column rendered per row", /Array/.test(document.querySelector("#qbody").textContent), "no tags");

// ---------- favorite toggle
const starCountBefore = document.querySelectorAll("#qbody .fav.on").length;
click("#qbody .fav:not(.on)");
await settle(120);
check("star click calls toggle_favorite", called("leetcode/toggle_favorite"), "not called");
check("star state flips in the UI", document.querySelectorAll("#qbody .fav.on").length === starCountBefore + 1, `${starCountBefore} -> ${document.querySelectorAll("#qbody .fav.on").length}`);

// ---------- sort
document.querySelector("#sortField").value = "AC_RATE";
document.querySelector("#sortField").dispatchEvent(new window.Event("change", { bubbles: true }));
await settle(200);
check("sorting pulls the full filtered set", called("leetcode/list_all"), "list_all not called");
const rates = [...document.querySelectorAll("#qbody tr.q td:nth-child(4)")].map((td) => parseFloat(td.textContent));
check("rows sorted by 通过率 ascending", rates.every((v, i, a) => i === 0 || a[i - 1] <= v), rates.join(","));
click("#sortDir");
await settle(150);
check("direction toggle reverses the order", /↓/.test(document.querySelector("#sortDir").textContent), document.querySelector("#sortDir").textContent);
const ratesDesc = [...document.querySelectorAll("#qbody tr.q td:nth-child(4)")].map((td) => parseFloat(td.textContent));
check("rows sorted by 通过率 descending", ratesDesc.every((v, i, a) => i === 0 || a[i - 1] >= v), ratesDesc.join(","));
check("status line mentions the active sort", /通过率升序|通过率降序/.test(document.querySelector("#listStatus").textContent), document.querySelector("#listStatus").textContent);

// ---------- only favorites via nav
click("#nav .nav-item", "我的收藏");
await settle(200);
check("我的收藏 filters to favorited rows", [...document.querySelectorAll("#qbody tr.q")].every((tr) => tr.querySelector(".fav.on")), "some rows unfavourited");
check("status line shows 只看收藏", /只看收藏/.test(document.querySelector("#listStatus").textContent), document.querySelector("#listStatus").textContent);
click("#nav .nav-item", "题目");
await settle(200);

// ---------- random question
const listCallsBefore = calls.filter(([m]) => m === "leetcode/list_problems").length;
click("#randomBtn");
await settle(200);
check("随机一题 asks for one row at a random offset", calls.filter(([m]) => m === "leetcode/list_problems").length > listCallsBefore && lastListParams?.limit === 1, JSON.stringify(lastListParams));
check("random pick opens the solving view", !document.querySelector("#solveView").classList.contains("hidden"), "solve view hidden");
click("#backBtn");
await settle(120);

// ---------- company + site links open through the sidecar
click("#companies button", "字节跳动");
await settle(120);
check("company chip opens the official page via sidecar", calls.some(([m, p]) => m === "leetcode/open_site" && p.path === "/company/bytedance/"), "not called");
click('#nav .nav-item.site[data-path="/explore/"]');
await settle(120);
check("探险模式 links out", calls.some(([m, p]) => m === "leetcode/open_site" && p.path === "/explore/"), "not called");

// ---------- reset clears everything (also exits the "full set" sort mode)
click("#resetBtn");
await settle(250);
check("reset clears sort + fav filter + category + tags", !lastListParams?.filters?.tags && !lastListParams?.filters?.status && lastListParams?.categorySlug === ""
  && document.querySelector("#sortField").value === "" && !state0().onlyFavs, JSON.stringify(lastListParams));
function state0() { return { onlyFavs: document.querySelector('#nav .nav-item[data-view="favs"]').getAttribute("aria-current") === "page" }; }

// ---------- filters (paged path, after reset left sort mode off)
click("#cats .cat", "数据库");
await settle(200);
check("category tab sends categorySlug=database", lastListParams?.categorySlug === "database", JSON.stringify(lastListParams));
click("#tags .tag[data-slug='array']");
await settle(200);
check("tag click sends filters.tags", JSON.stringify(lastListParams?.filters?.tags) === '["array"]', JSON.stringify(lastListParams?.filters));
document.querySelector("#status").value = "AC";
document.querySelector("#status").dispatchEvent(new window.Event("change", { bubbles: true }));
await settle(200);
check("作答状态 sends filters.status", lastListParams?.filters?.status === "AC", JSON.stringify(lastListParams?.filters));
check("combined filter summary is shown", /数据库/.test(document.querySelector("#listStatus").textContent) && /数组/.test(document.querySelector("#listStatus").textContent), document.querySelector("#listStatus").textContent);

// ---------- sorting switches to the full-set endpoint instead of paging
document.querySelector("#sortField").value = "FRONTEND_ID";
document.querySelector("#sortField").dispatchEvent(new window.Event("change", { bubbles: true }));
await settle(200);
check("re-enabling sort re-reads the full set", called("leetcode/list_all") && lastAllParams?.filters?.status === "AC", JSON.stringify(lastAllParams));
click("#resetBtn");
await settle(250);

const failed = results.filter((item) => !item.ok);
console.log(`\n${results.length - failed.length}/${results.length} checks passed`);
window.close();
process.exit(failed.length ? 1 : 0);
