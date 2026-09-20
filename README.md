# LeetCode 刷题插件（leetcode-cn）

在 DBX 插件中心里登录力扣（leetcode.cn）、浏览题库、查看题面、选择语言编写代码并运行 / 提交判题的工作台插件。

当前版本 **0.2.1**，已在本机 DBX 上跑通登录 → 题库 → 做题 → 运行 / 提交全流程。

## 功能

- **免粘贴登录**：插件启动一个受控的 Chrome/Edge 窗口打开力扣官方登录页，你用账号密码 / 手机验证码登录（极验滑块在真实浏览器里可正常通过），插件通过 Chrome DevTools 协议读回已解密的会话 Cookie，自动完成登录——无需手动复制粘贴。
- **题库浏览**：题号 / 标题（中文）/ 通过率 / 难度 / 我的状态；支持按难度、标签筛选，标题/题号搜索，**分页**（可切换每页 15/30/50/100，翻页 / 跳转）。
- **做题页**：点击题目进入独立做题视图，左侧题面、右侧代码编辑器，中间分隔线可**左右拖动**调整宽度，支持**全屏 / 退出全屏**。
- **代码编辑器**：按语言自动**语法高亮**（关键字 / 字符串 / 注释 / 数字），带**行号**、Tab 缩进、回车智能缩进；语言下拉来自题面 `codeSnippets`。
- **运行 / 提交判题**：展示状态、用时、内存、输出、编译错误。
- **会员题兜底**：付费 / 会员题目（接口返回 `isPaidOnly` 或内容为空）会给出明确提示并提供「重试」，不再空白页。
- 收藏、提交历史（需登录）。

## 架构

- **原生后端 sidecar（Rust）**：真正访问 `leetcode.cn` 的接口（题单 / 题面 GraphQL、运行 / 提交判题、登录态校验），并负责启动受控浏览器与 DevTools 抓取。放在后端而非 UI 里，是因为插件工作台是 `default-src 'none'` 的沙箱 iframe，直接 `fetch` 会被 CORS 拦截，且无法内嵌浏览器。
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
| `leetcode/list_problems` | `{ skip, limit, filters }` | `problemsetQuestionList` 拉题单 |
| `leetcode/get_problem` | `{ titleSlug }` | `question` 拉题面 + 代码模板（含 `isPaidOnly`） |
| `leetcode/run_code` | `{ titleSlug, lang, code, inputs? }` | `interpret_solution` 运行样例 |
| `leetcode/submit_code` | `{ titleSlug, lang, code }` | `submit` 提交判题 |
| `leetcode/toggle_favorite` / `leetcode/my_favorites` | `{ titleSlug, favorite }` / — | 收藏（需登录） |
| `leetcode/submission_list` | `{ titleSlug }` | 提交历史（需登录） |

## 构建与本地运行

需要 Rust 工具链（Windows 上需 MSVC Build Tools，含 `link.exe` / Windows SDK）。`dbx-plugin-sdk` 未发布到 crates.io，本地构建需用 dbx 仓库内的路径覆盖：

```bash
cd backend
# 本地开发时用 dbx 仓库里的 SDK 路径（打包/CI 会自行 patch）
mkdir -p .cargo && printf '[patch.crates-io]\ndbx-plugin-sdk = { path = "<dbx>/plugins/sdk/rust/dbx-plugin-sdk" }\n' > .cargo/config.toml
cargo build --release
# 产物：backend/target/release/dbx-leetcode-cn(.exe)
```

打包（`dbx-plugin package`）会为各目标平台产出 `.dbxp`，并把 manifest 里的后端可执行路径改写成平台特定路径（如 `bin/windows-x64/dbx-leetcode-cn.exe`）——这是运行时解析后端所必需的。

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

- 登录必须在插件另开的**受控浏览器窗口**里完成一次（该窗口用独立临时配置，不带平时浏览器的登录态）；这是 DBX 无内嵌浏览器 + 力扣极验风控下的可行折中。
- 插件窗口的「全屏」指铺满插件所在工作台区域，非操作系统全屏（沙箱内无法请求 OS 全屏）。
- 账号密码 / 短信的纯 HTTP 登录、以及读取运行中 Chrome 的 Cookie 文件，在多数现代环境下会被极验 / 文件锁 / App-Bound 加密阻断，仅作兜底。
- 自动化访问 leetcode.cn 受其服务条款约束，可能触发风控；请仅用于个人刷题。

## 许可

Apache-2.0
