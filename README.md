# LeetCode 刷题插件（leetcode-cn）

在 DBX 插件中心里登录力扣（leetcode.cn）、浏览题库、查看题面、选择语言编写代码并运行 / 提交判题的工作台插件。

当前版本 **0.3.11**。已在本机 DBX 上跑通：登录（含真实头像）→ 分类 / 标签 / 难度 / 作答状态筛选 → 排序 → 随机一题 → 收藏与我的收藏 → 每日一题 → 学习计划 → 做题 → 运行 / 提交判题（含结果卡片与提交历史表格）→ 格式化（JavaScript / TypeScript）。

## 功能

- **免粘贴登录**：插件启动一个受控的 Chrome/Edge 窗口打开力扣官方登录页，你用账号密码 / 手机验证码登录（极验滑块在真实浏览器里可正常通过），插件通过 Chrome DevTools 协议读回已解密的会话 Cookie，自动完成登录——无需手动复制粘贴。
- **题库浏览**：题号 / 标题（中文）/ 标签 / **通过率** / **难度（中文）** / 我的状态；**官方分类页签**（全部题目 / 算法 / 数据库 / Shell / 多线程 / JavaScript / pandas）、**79 个标签芯片带实时题数**（数组 2430、字符串 966…计数直接取力扣接口，可多选、可展开收起）、难度与**作答状态**（已解答 / 未尝试 / 尝试过）筛选、**服务端关键词搜索**，以及**分页**（每页 15/30/50/100，翻页 / 跳转）。
- **排序**：默认 / 题号 / 难度 / 通过率 / 出题频率 / 竞赛分，配 ↑↓ 方向切换。选择排序后会一次性拉取当前筛选的**完整结果集**（按筛选组合缓存，上限 1500 题）再本地排序，避免"只排当前页"的假排序；状态行会标注是否被截断。
- **随机一题**：在**当前筛选与分类范围内**随机取一题直接打开，而不是只在第一页里随机。
- **收藏星标**：每行末尾一颗 ⭐/☆，点击即收藏或取消。初值来自列表接口的 `isFavor`（零额外请求）；写入用 `addQuestionToDefaultFavoriteV2` / `removeQuestionFromFavoriteV2`（文档逐字取自力扣前端 bundle，取消时先解析所属收藏夹 slug）。接口返回 `ok:false` 时状态行显示真实原因，且不会把星星点亮。
- **我的收藏**：左侧导航项，直接读 `favoriteQuestionList`，展示的是账号下完整的收藏集合（不受当前分类 / 筛选影响），同样支持翻页。
- **每日一题**：右侧栏与左侧导航都会显示当日题目（日期 / 难度 / 中文标题），点击直接进入做题页。
- **学习计划（插件内）**：左侧「学习计划」直接渲染 `studyPlanV2Catalogs` 分类与 `studyPlansV2ByCatalog` 题单（名称 / 题数 / 是否 Plus），点「打开题单」才进浏览器。接口文档逐字取自力扣前端 bundle。
- **探险模式（插件内说明面板）**：关卡列表由力扣按会话下发、没有可稳定调用的公开查询，因此插件内展示分类概览并提供入口，而不是留白。
- **LeetBook 暂时隐藏**：`leetbooksByIds` / `leetbookBooks` 对未登录会话返回「字段不存在」，无法稳定取书列表，故导航中先不显示该项。
- **热门企业题库**：右侧企业芯片通过 sidecar 打开对应的 leetcode.cn 页面（企业排行无公开接口）。
- **登录态显示**：顶部展示当前昵称、用户名与**力扣真实头像**（由 sidecar 取回字节转 data URL，失败时回退首字母徽章）。
- **做题页**：点击题目进入独立做题视图，左侧题面、右侧代码编辑器，中间分隔线可**左右拖动**调整宽度，支持**全屏 / 退出全屏**。
- **代码编辑器**：按语言自动**语法高亮**（关键字 / 字符串 / 注释 / 数字），带**行号**、Tab 缩进、回车智能缩进；语言下拉来自题面 `codeSnippets`。
- **运行 / 提交判题**：与官网同款的判定卡片（状态 + 执行用时 / 内存消耗 / 通过用例 + 输入 / 输出 / 预期结果，编译与运行错误单独成块）；提交历史是官网那张表（# / 状态 / 语言 / 执行用时 / 消耗内存 / 备注，中文状态 + 相对时间）。
- **格式化**：工具栏「格式化」按钮，JavaScript 走 prettier 的 babel 解析器、TypeScript 走 typescript 解析器；其余语言只做行尾空白与换行规范化并给出提示（见「已知限制」）。
- **会员题兜底**：付费 / 会员题目（接口返回 `isPaidOnly` 或内容为空）会给出明确提示并提供「重试」，不再空白页。
- 收藏、提交历史（需登录）。

## 架构

- **原生后端 sidecar（Rust）**：真正访问 `leetcode.cn` 的接口（题单 / 题面 GraphQL、收藏、每日一题、学习计划、运行 / 提交判题、登录态与头像取字节），并负责启动受控浏览器与 DevTools 抓取。放在后端而非 UI 里，是因为工作台 iframe 的 CSP 为 `default-src 'none'`（仅放开 `script-src`/`style-src`/`img-src data: blob:`），直接 `fetch` 会被拦，也无法内嵌浏览器；头像因此由后端取字节转 `data:` URL 交给界面渲染。
- **工作台 UI（`ui/index.html`）**：沙箱 iframe，纯内联 HTML/CSS/JS（受 CSP 限制，不加载任何外部脚本 / CDN；语法高亮为自实现，无 highlight.js/CodeMirror）。通过 `window.dbxPlugin.request("backend.invoke", { method, params, timeoutMs })` 调用 sidecar。

### 关于登录方式（重要）

DBX 插件只有 `host.workbench` / `host.binary` / `host.network` 三种能力，**没有内嵌浏览器 / webview**，工作台又是 CSP 锁死的单一 iframe——因此无法像 IntelliJ 的 LeetCode 插件那样在窗口内用 JCEF 内嵌浏览器直接完成极验登录。同时 `leetcode.cn` 的账号密码 / 短信接口受极验滑块与风控保护，纯 HTTP 提交无法通过。

所以采用「受控浏览器 + DevTools 取回」这一免粘贴方案：

1. `browser_login_start`：以独立临时配置启动 Chrome/Edge，`--app` 打开官方登录页（无边框窗口，便于和你平时的浏览器区分）。
2. 你在该窗口用账号密码 / 验证码登录（极验可正常通过）。
3. `browser_login_poll`：通过 DevTools 的 `Network.getAllCookies` 读取该浏览器**内存中已解密**的 `LEETCODE_SESSION`（绕开运行期 Cookie 文件锁 `os error 32` 与 App-Bound 加密），校验登录态后收尾。

> 登录拆成 `start` + 轮询 `poll` 两个快速请求，是为了规避 DBX 主机对单个后端请求的 **30s 超时**（`PLUGIN_REQUEST_TIMEOUT`）——人工登录远超 30s。

## sidecar 方法（`backend.invoke` 的 method）

| method | 参数 | 说明 |
|---|---|---|
| `leetcode/status` | — | 是否已登录 + 用户名 + 昵称 |
| `leetcode/browser_login_start` | — | 启动受控浏览器打开官方登录页（立即返回） |
| `leetcode/browser_login_poll` | — | 轮询登录结果（pending / 成功 / timeout / error） |
| `leetcode/browser_login_cancel` | — | 取消并收尾受控浏览器 |
| `leetcode/open_login_browser` | — | 用系统默认浏览器打开登录页（辅助） |
| `leetcode/login_cookie` | `{ session, csrf }` | 直接注入 `LEETCODE_SESSION` 校验登录（备用） |
| `leetcode/login` | `{ login, password }` | 账号密码登录（通常被极验拦截，兜底） |
| `leetcode/send_code` / `leetcode/login_code` | `{ target }` / `{ target, code }` | 短信验证码登录（通常被极验拦截，兜底） |
| `leetcode/read_browser_cookies` / `leetcode/login_auto` | — | 读取本机浏览器 Cookie 文件（现代 Chrome/Edge 会失败，备用） |
| `leetcode/logout` | — | 清空会话 |
| `leetcode/list_problems` | `{ skip, limit, filters, categorySlug?, searchKeyword? }` | `filters` 支持 `{ tags: [slug], difficulty, status }`；`categorySlug` 对应官方页签（注意数据库是单数 `database`）；`searchKeyword` 走 V2 接口，**需登录**。返回行含 `isFavor` |
| `leetcode/list_all` | `{ filters, categorySlug?, maxQuestions? }` | 逐页（每页 100）取回**完整筛选结果**供本地排序；返回 `{ total, questions, truncated }` |
| `leetcode/topic_tags` | — | `questionTopicTags`，返回全部标签及各自题数（按题数降序） |
| `leetcode/daily_question` | — | `todayRecord`（注意它返回**数组**，后端已取首元素） |
| `leetcode/open_site` | `{ path }` | 用系统浏览器打开指定 leetcode.cn 路径；只接受站内绝对路径，拒绝 `//host` 与完整 URL |
| `leetcode/get_problem` | `{ titleSlug }` | `question` 拉题面 + 代码模板（含 `isPaidOnly`） |
| `leetcode/run_code` | `{ titleSlug, lang, code, inputs? }` | `interpret_solution` 运行样例 |
| `leetcode/submit_code` | `{ titleSlug, lang, code }` | `submit` 提交判题 |
| `leetcode/toggle_favorite` / `leetcode/my_favorites` | `{ titleSlug, favorite }` / — | 收藏（需登录） |
| `leetcode/submission_list` | `{ titleSlug }` | 提交历史（需登录） |

### 判题接口契约（0.3.11 校正）

两处都从站点自己的编辑器 bundle（`chunks/pages/problems/[slug]`）核对，不是猜测：

- 请求体是 **JSON**，代码字段必须叫 **`typed_code`**；发 `codedetail` 会被回 `{"error":"解答提交 POST 数据丢失，请刷新此页面。"}`。运行带 `data_input`，提交不带（它跑全量用例），提交另带可为 `null` 的 `study_plan_slug` / `favorite_slug`。
- 结果轮询：运行 `/submissions/detail/<id>/check/`，提交 `/submissions/detail/<id>/v2/check/`。
- `state` 只有 `PENDING / SUCCESS / FAILURE / REVOKED`（**没有** `Finished`）；输出在 `std_output`（运行还有 `std_output_list`），另有 `compare_result`、`total_correct`、`total_testcases`、`last_testcase`、`full_compile_error`、`full_runtime_error`。
- `submissionList` 的 `offset` 与 `limit` 都是非空参数，漏 `offset` 服务端只回「发生未知错误，请联系管理员」。

## 构建与本地运行

需要 Rust 工具链（Windows 上需 MSVC Build Tools，含 `link.exe` / Windows SDK）。`dbx-plugin-sdk` 未发布到 crates.io，本地构建需用 dbx 仓库内的路径覆盖：

```bash
cd backend
# 本地开发时用 dbx 仓库里的 SDK 路径（打包/CI 会自行 patch）
mkdir -p .cargo && printf '[patch.crates-io]\ndbx-plugin-sdk = { path = "<dbx>/plugins/sdk/rust/dbx-plugin-sdk" }\n' > .cargo/config.toml
cargo build --release
# 产物：backend/target/release/dbx-leetcode-cn(.exe)
```

打包（`dbx-plugin package`）会为各目标平台产出 `.dbxp`，并把 manifest 里的后端可执行路径改写成平台特定路径（如 `bin/windows-x64/dbx-leetcode-cn.exe`）——这是运行时解析后端所必需的。本地执行 `dbx-plugin package .` 时同样要 SDK 路径，用环境变量提供：`DBX_PLUGIN_SDK_ROOT=<dbx 仓库根目录> dbx-plugin package .`。

冒烟测试（打真实 leetcode.cn 接口，无需登录的部分）：

```bash
node dev/smoke.mjs        # 握手 + 标签计数 + 标签/难度/状态/分类筛选 + 分页 + 每日一题 + 全量取数 + open_site 入参校验 + 未登录搜索的报错路径
npm install jsdom         # 仅首次
node dev/ui.test.mjs      # DOM 级交互回归：导航折叠、页签、标签展开收起、排序方向与顺序、收藏星标与失败处理、随机一题、学习计划/探险面板、头像 data URL、筛选组合、重置
```

当前状态：`dev/smoke.mjs` 24/24（真实接口）、`dev/ui.test.mjs` 57/57（DOM 交互）。两套都在 CI 之外可独立重复执行。

## 本地安装（自用，无需上架）

1. `dbx-plugin package .` 产出未签名 `.dbxp`（native 插件需在对应平台构建）。
2. 在 DBX 插件中心「设置 → 开发者选项」开启「允许安装未签名开发包」，再本地安装该 `.dbxp`。
3. 打开「LeetCode 刷题」工作台，点「用浏览器登录并自动导入」，在弹出的专用窗口登录即可。

## 上架 t8y2/dbx-store（发布流程）

`t8y2/dbx-store` 是官方**目录仓库**（只放审核后的目录元数据、发布者身份、公钥、生成的 catalog），不放源码、不放 `.dbxp`。提交用**单个 PR**：

1. 源码在本独立仓库，发布一个 **GitHub Release**；仓库的 Release 工作流复用 `t8y2/dbx/.github/workflows/plugin-release-reusable.yml` 为各平台构建**未签名** `.dbxp` 并生成 `release-candidates.json`（含各 target 的 URL / SHA-256 / size）。
2. Fork `t8y2/dbx-store`，向 `main` 开 PR，包含：
   - 首次：`publishers/<publisher-id>.json`；
   - 每次：`candidates/<plugin-id>.json`（`sha256`/`size` 精确锁定未签名字节，URL 指向你自己的 GitHub Release，HTTPS、不可变）。
3. CI 会保持红色并提示「open candidate(s) awaiting DBX Store signing」——这是**预期**，用于阻止未签名内容被合并。
4. 维护者审核后在该 PR 上运行受保护的 **Sign plugin PR candidates** 工作流：校验候选字节、用官方 key 签名、把签名产物发布到 R2、回写 `plugins/<id>.json` 与 `catalog/index.json`、删除 `candidates/`。CI 转绿后由维护者合并。

> 作者拿不到官方签名 key。候选包的 `manifest.json` 必须与候选元数据里的 `id`/`version`/`publisher` 一致，否则签名会拒绝。

## 已知限制

- **收藏与头像依赖登录会话**：两者的接口都需要已登录的 `LEETCODE_SESSION`；未登录时收藏会被力扣拒绝（界面显示原因），头像回退为首字母徽章。
- **排序会先拉全量**：选择任一排序后会按每页 100 逐页取回当前筛选结果（上限 1500 题，状态行标注「仅在前 1500 题内排序」），首次切换有数秒延迟；结果按筛选组合缓存，之后再排序即时无请求。
- **服务端关键词搜索需要登录**：公开的 V1 列表接口没有搜索字段，只能走登录态的 V2 接口；未登录时界面会给出明确提示，而不是静默返回空列表。
- **头像经 sidecar 中转**：工作台 CSP 允许 `img-src data:` 但禁止站外图片源，因此由 sidecar 取回头像字节、以 data URL 交给界面渲染；取不到时回退为首字母徽章。
- **LeetBook 入口已隐藏**：其书列表接口对匿名会话不可用（`leetbooksByIds` 在 Query 上不存在），登录后是否可用尚未实测；恢复前需先确认接口。
- **企业题库与题单/书本的深层页面跳系统浏览器**：这些页面没有稳定的公开列表接口，插件内复刻不可靠。
- **格式化只覆盖 JavaScript / TypeScript**：prettier 没有 Go / Python / Java / C++ / Rust 等语言的解析器，这些语言点「格式化」只做行尾空白与换行规范化——刻意不按括号深度重排，因为 Python 这类缩进敏感语言一旦重排就会改坏代码。要 Go 的官方格式需另接本机 `gofmt`。prettier 的 standalone 与 babel / typescript 解析器由 `dev/inline-prettier.mjs` 内联进 `ui/index.html`（官方打包流程不装项目依赖，且沙箱不放行同源脚本外链，所以只能内联提交），代价是包体积 +约 390 KB。
- **没有「击败 xx%」**：官网那个百分比来自单独的分布接口，判题响应里没有对应字段，不做估算。
- 登录必须在插件另开的**受控浏览器窗口**里完成一次（该窗口用独立临时配置，不带平时浏览器的登录态）；这是 DBX 无内嵌浏览器 + 力扣极验风控下的可行折中。
- 插件窗口的「全屏」指铺满插件所在工作台区域，非操作系统全屏（沙箱内无法请求 OS 全屏）。
- 账号密码 / 短信的纯 HTTP 登录、以及读取运行中 Chrome 的 Cookie 文件，在多数现代环境下会被极验 / 文件锁 / App-Bound 加密阻断，仅作兜底。
- 自动化访问 leetcode.cn 受其服务条款约束，可能触发风控；请仅用于个人刷题。

## 许可

Apache-2.0
