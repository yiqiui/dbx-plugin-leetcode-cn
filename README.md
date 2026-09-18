# LeetCode 刷题插件（leetcode-cn）

在 DBX 插件中心里登录力扣（leetcode.cn）、浏览题库、查看题面、选择语言编写代码并运行 / 提交判题的工作台插件。

> ⚠️ 这是一个**功能骨架 / 起步实现**，不是已验证可用的成品。见下方「实现状态与未验证项」。

## 架构

- **原生后端 sidecar（Rust）**：真正访问 `leetcode.cn` 的接口（登录、题单 GraphQL、题面、运行 / 提交）。放在后端而不是插件 UI 里，是因为插件工作台是 `default-src 'none'` 的沙箱 iframe，直接 `fetch` leetcode.cn 会被 CORS 拦截；sidecar 走服务端 HTTP，无 CORS，并可管理登录 Cookie。
- **工作台 UI（`ui/index.html`）**：沙箱 iframe，纯内联 HTML/CSS/JS（受 CSP 限制，不加载任何外部脚本 / CDN）。通过 `window.dbxPlugin.request("backend.invoke", { method, params })` 调用 sidecar。

UI 覆盖：登录 → 题单列表（题号 / 标题 / 通过率 / 难度 / 我的状态，可搜索、按难度筛选、加载更多）→ 题面详情 → 语言下拉（来自题面 `codeSnippets`）+ 代码编辑器 → 运行 / 提交，展示判题结果（状态、用时、内存、输出、编译错误）。

## sidecar 方法（`backend.invoke` 的 method）

| method | 参数 | 说明 |
|---|---|---|
| `leetcode/status` | — | 是否已登录 + 用户名 |
| `leetcode/login` | `{ login, password }` | 力扣账号密码登录（`/accounts/login/` 表单 + CSRF），成功后保存会话 Cookie |
| `leetcode/logout` | — | 清空会话 |
| `leetcode/list_problems` | `{ skip, limit, filters }` | GraphQL `questionList` 拉题单 |
| `leetcode/get_problem` | `{ titleSlug }` | GraphQL `question` 拉题面 + 代码模板 |
| `leetcode/run_code` | `{ titleSlug, lang, code, inputs? }` | `interpret_solution` 运行样例 |
| `leetcode/submit_code` | `{ titleSlug, lang, code }` | `submit` 提交判题 |

## 构建与本地运行

需要 Rust 工具链（Windows 上需 MSVC Build Tools，含 `link.exe` / Windows SDK）。

```bash
# 1) 构建后端（native 项目必须在目标平台构建）
cd backend
cargo build --release
# 产物：backend/target/release/dbx-leetcode-cn(.exe)，按 dbx-plugin.toml 约定放到 bin/ 由打包步骤处理

# 2) 用插件 CLI 起本地开发宿主（含后端），在浏览器里先验证 UI + RPC
npx --yes @dbx-app/plugin-cli dev --path .. --port 5190

# 3) 打包并在 DBX 插件中心「本地安装」（开启“允许安装未签名开发包”）
npx --yes @dbx-app/plugin-cli package ..
```

## 实现状态与未验证项（重要）

后端 sidecar **已在本机用 Rust（`D:\workspace\kaifa\rust`）+ MSVC Build Tools 编译通过**（`target/release/dbx-leetcode-cn.exe`，启动无崩溃）。仍未做的是**连真实 leetcode.cn 跑通接口**（该环境没有可用的力扣账号 / 外网）：

- ✅ 结构完整：`manifest.json`、`dbx-plugin.toml`、`ui/index.html`、`backend/{Cargo.toml,src/main.rs}` 均按 DBX 插件 SDK 真实 API 编写（`PluginHandler.handle` + `window.dbxPlugin.request("backend.invoke", …)`）。
- ✅ **已编译通过**：装好 MSVC 链接器后 `cargo build --release` 成功；修掉了 `ureq` 的 `Response` 头部读取（改用 `response.all("set-cookie")`）。
- ⚠️ **接口形状需实测**：`leetcode.cn` 用的是非官方 GraphQL + 表单登录 + `interpret_solution` / `submit` 接口，字段与鉴权细节会随版本变化；登录可能触发验证码 / 风控。这些需在你的环境里对照真实响应逐个校准。
- ⚠️ **合规**：账号密码登录属自动化访问，受 leetcode.cn 服务条款约束，可能触发风控；更稳妥的鉴权是复用浏览器 Cookie（可后续加）。
- ❌ **未做的完整特性**：标签 / 公司 / 面试清单等筛选、题目收藏、提交历史、讨论区、调试面板、Markdown 富文本渲染增强、多页缓存等，属于「功能齐全」的后续迭代。

## 下一步建议

1. ~~在有 MSVC 的机器上 `cargo build --release` 修掉编译期问题~~（已完成，sidecar 编译通过）；
2. 用你自己的账号跑通 `login` → `list_problems` → `get_problem` → `run_code` → `submit_code`，按真实响应微调 GraphQL 查询与判题轮询字段；
3. 再逐项补齐上面「未做的完整特性」。

## 上架 t8y2/dbx-store（发布流程）

`t8y2/dbx-store` 是官方**目录仓库**（只放审核后的目录元数据、发布者身份、公钥、吊销、生成的 catalog JSON），不放源码、不放 `.dbxp`。上架不是提 PR，而是“提交→人工审核→官方签名→发布”：

1. 把本插件放到**独立仓库**（GitHub 只执行仓库根 `.github/workflows/`；把 `github/plugin-release.yml` 移到根 `.github/workflows/plugin-release.yml`）。
2. 在该仓库发布一个 **GitHub Release** → 复用工作流会调用 `dbx-plugin package` 为各平台构建 `.dbxp` 并生成产物元数据（URL、SHA-256、target、signingKeyId）。
3. 在 `t8y2/dbx-store` 开一个 **Plugin submission Issue**，附上源码 tag + `release-candidates.json` 的 URL。
4. 官方**人工审核**通过后，由维护者用官方 key 签名并把最终 `.dbxp` 发布到 store 存储、更新 catalog → 用户在 dbx 插件中心可见。

> 作者拿不到官方签名 key；且官方目录会人工审核——一个 LeetCode 刷题插件对“数据库客户端”的定位是否被接受，取决于审核。给自己用走下面的本地安装即可。

## 本地安装（自用，无需上架）

1. `dbx-plugin package .` 产出未签名 `.dbxp`（native 插件需在对应平台构建）；
2. dbx 插件中心开启“允许安装未签名开发包”，本地安装该 `.dbxp`；
3. 用 **Cookie 登录**（粘贴浏览器 `LEETCODE_SESSION` / `csrftoken`）绕过极验验证码。
