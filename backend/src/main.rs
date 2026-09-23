use std::collections::HashMap;
use std::sync::Mutex;

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use base64::Engine as _;
use dbx_plugin_sdk::{PluginEmitter, PluginError, PluginHandler, PluginMetadata, PluginServer, RequestContext};
use serde_json::{json, Value};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

const BASE: &str = "https://leetcode.cn";
const GRAPHQL: &str = "https://leetcode.cn/graphql/";

const LIST_QUERY: &str = r#"query problemsetQuestionList($categorySlug: String, $limit: Int, $skip: Int, $filters: QuestionListFilterInput) {
  problemsetQuestionList(categorySlug: $categorySlug, limit: $limit, skip: $skip, filters: $filters) {
    total
    questions {
      acRate difficulty frontendQuestionId title titleCn titleSlug paidOnly status
      topicTags { name slug nameTranslated }
    }
  }
}"#;

/// Server-side keyword search. The public V1 list has no search field, and the
/// V2 endpoint is login-gated, so this runs with the session cookies and the
/// caller reports a clear "please sign in" error when it is rejected.
const LIST_V2_QUERY: &str = r#"query problemsetQuestionListV2($categorySlug: String, $limit: Int, $skip: Int, $searchKeyword: String) {
  problemsetQuestionListV2(categorySlug: $categorySlug, limit: $limit, skip: $skip, searchKeyword: $searchKeyword) {
    totalLength
    hasMore
    questions {
      acRate difficulty questionFrontendId title translatedTitle titleSlug paidOnly status
      topicTags { name slug }
    }
  }
}"#;

/// Every topic with the ids of its questions; `questionIds.len()` reproduces the
/// counts the official problemset shows next to each tag (数组 2430, ...).
const TOPIC_TAGS_QUERY: &str = r#"query topicTags {
  questionTopicTags {
    edges {
      node { name slug translatedName questionIds }
    }
  }
}"#;

const DETAIL_QUERY: &str = r#"query questionData($titleSlug: String!) {
  question(titleSlug: $titleSlug) {
    questionId questionFrontendId title titleSlug content difficulty isPaidOnly
    translatedTitle translatedContent sampleTestCase stats
    codeSnippets { lang langSlug code }
    topicTags { name slug }
  }
}"#;

#[derive(Default)]
struct Session {
    cookies: HashMap<String, String>,
    csrf: String,
    username: String,
    realname: String,
}

impl Session {
    fn cookie_header(&self) -> String {
        self.cookies
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join("; ")
    }
    fn absorb_set_cookie(&mut self, header: &str) {
        let pair = header.split(';').next().unwrap_or("").trim();
        if let Some((k, v)) = pair.split_once('=') {
            let k = k.trim();
            if !k.is_empty() {
                self.cookies.insert(k.to_string(), v.trim().to_string());
                if k.eq_ignore_ascii_case("csrftoken") {
                    self.csrf = v.trim().to_string();
                }
            }
        }
    }
}

#[derive(Default)]
struct LeetCodePlugin {
    session: Mutex<Session>,
    capture: Mutex<Option<Capture>>,
}

/// 受控浏览器登录会话的跨调用状态（start 与 poll 是两个独立的后端请求）。
struct Capture {
    child: std::process::Child,
    port: u16,
    profile: std::path::PathBuf,
    started: Instant,
    ready: bool,
}

fn err(msg: impl Into<String>) -> PluginError {
    PluginError::new(-32000, msg)
}

/// The V2 search endpoint names its fields differently
/// (`questionFrontendId` / `translatedTitle`); map them onto the V1 shape so the
/// UI keeps a single code path for both listings.
fn normalize_v2_list(v2: Value) -> Value {
    let rename = |obj: &mut Value, from: &str, to: &str| {
        let Some(map) = obj.as_object_mut() else { return };
        if let Some(value) = map.remove(from) {
            map.insert(to.to_string(), value);
        }
    };
    let questions: Vec<Value> = v2
        .get("questions")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|mut question| {
            rename(&mut question, "questionFrontendId", "frontendQuestionId");
            rename(&mut question, "translatedTitle", "titleCn");
            question
        })
        .collect();
    json!({
        "total": v2.get("totalLength").cloned().unwrap_or_else(|| Value::from(questions.len())),
        "hasMore": v2.get("hasMore").cloned().unwrap_or(Value::Bool(false)),
        "questions": questions,
    })
}

/// 访问 leetcode.cn 的共享 HTTP 客户端：尊重系统代理环境变量
/// (HTTP_PROXY/HTTPS_PROXY/ALL_PROXY/NO_PROXY)，并带连接/读取超时。
/// 修复「通过代理访问外网时，插件后端连不上 leetcode.cn、登录检测超时」的问题。
fn remote_agent() -> &'static ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        ureq::AgentBuilder::new()
            .try_proxy_from_env(true)
            .timeout_connect(Duration::from_secs(10))
            .timeout(Duration::from_secs(25))
            .build()
    })
}

/// 访问本机 Chrome DevTools 的客户端：不走代理（避免代理劫持 127.0.0.1），短超时。
fn local_agent() -> &'static ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(2))
            .timeout(Duration::from_secs(5))
            .build()
    })
}

fn truncate(s: &str) -> String {
    let t = s.trim();
    if t.chars().count() <= 200 {
        t.to_string()
    } else {
        let head: String = t.chars().take(200).collect();
        format!("{head}…")
    }
}

// ==================== 本机浏览器 Cookie 读取 ====================
// Firefox: cookies.sqlite 明文，直接读（先复制副本避免锁）。
// Chrome/Edge: %LOCALAPPDATA%\<Browser>\User Data\{Default,<Profile>}\Network\Cookies，
//   值为 v10/v11 前缀 + AES-256-GCM 密文；密钥在 User Data\Local State 的
//   os_crypt.encrypted_key（DPAPI 保护，需 CryptUnprotectData）。

fn local_appdata() -> Option<std::path::PathBuf> {
    std::env::var("LOCALAPPDATA").ok().map(std::path::PathBuf::from)
}

/// DPAPI 解密（用户级）。返回明文。
#[cfg(windows)]
fn dpapi_decrypt(data: &[u8]) -> Result<Vec<u8>, String> {
    use windows_sys::Win32::Security::Cryptography::CRYPT_INTEGER_BLOB;
    use windows_sys::Win32::Foundation::LocalFree;
    unsafe {
        let in_blob = CRYPT_INTEGER_BLOB {
            cbData: data.len() as u32,
            pbData: data.as_ptr() as *mut u8,
        };
        let mut out_blob = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
        let ok = windows_sys::Win32::Security::Cryptography::CryptUnprotectData(
            &in_blob,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            0,
            &mut out_blob,
        );
        if ok == 0 {
            return Err("CryptUnprotectData failed".to_string());
        }
        let plain = std::slice::from_raw_parts(out_blob.pbData, out_blob.cbData as usize).to_vec();
        if !out_blob.pbData.is_null() {
            LocalFree(out_blob.pbData as usize as *mut core::ffi::c_void);
        }
        Ok(plain)
    }
}

/// Chrome/Edge：从 Local State 提取 os_crypt.encrypted_key → DPAPI 解出 32 字节 AES key。
#[cfg(windows)]
fn chrome_master_key(user_data_dir: &std::path::Path) -> Result<Vec<u8>, String> {
    let local_state = std::fs::read_to_string(user_data_dir.join("Local State"))
        .map_err(|e| format!("read Local State: {e}"))?;
    let parsed: Value = serde_json::from_str(&local_state).map_err(|e| format!("parse Local State: {e}"))?;
    let encoded = parsed
        .pointer("/os_crypt/encrypted_key")
        .and_then(Value::as_str)
        .ok_or("no os_crypt.encrypted_key")?;
    let raw = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|e| format!("base64 key: {e}"))?;
    if raw.len() < 5 || &raw[..5] != b"DPAPI" {
        return Err("unexpected encrypted_key prefix".to_string());
    }
    dpapi_decrypt(&raw[5..])
}

/// 解密 Chrome/Edge 的单个 cookie 值（v10/v11 前缀 + AES-256-GCM，nonce=前12字节）。
#[cfg(windows)]
fn decrypt_chromium_value(encrypted: &[u8], key: &[u8]) -> Result<String, String> {
    if encrypted.is_empty() {
        return Ok(String::new());
    }
    // 旧版 DPAPI 直接加密（无前缀）
    if encrypted.len() > 5 && &encrypted[..5] != b"v10" && &encrypted[..5] != b"v11" {
        if let Ok(plain) = dpapi_decrypt(encrypted) {
            return Ok(String::from_utf8_lossy(&plain).to_string());
        }
        return Err("unrecognized cookie format".to_string());
    }
    if encrypted.len() < 5 + 12 + 16 {
        return Err("cookie ciphertext too short".to_string());
    }
    let nonce_bytes = &encrypted[5..17];
    let cipher = &encrypted[17..];
    let gcm = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    let payload = Payload { msg: cipher, aad: &[] };
    let plain = gcm
        .decrypt(Nonce::from_slice(nonce_bytes), payload)
        .map_err(|_| "AES-GCM decrypt failed".to_string())?;
    // Chrome 明文可能有 32 字节 SHA256(domain) 前缀（v20 App-bound encryption 之后）；
    // 去掉尾部 NUL 后按 UTF-8 输出即可。
    Ok(String::from_utf8_lossy(&plain).trim_end_matches('\0').to_string())
}

/// 非 Windows：仅支持 v10/v11 前缀的 AES-256-GCM 解密（无 DPAPI 回退）。
#[cfg(not(windows))]
fn decrypt_chromium_value(encrypted: &[u8], key: &[u8]) -> Result<String, String> {
    if encrypted.is_empty() {
        return Ok(String::new());
    }
    if encrypted.len() < 5 + 12 + 16 || (&encrypted[..3] != b"v10" && &encrypted[..3] != b"v11") {
        return Err("unsupported cookie encryption on this platform".to_string());
    }
    let nonce_bytes = &encrypted[5..17];
    let cipher = &encrypted[17..];
    let gcm = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    let payload = Payload { msg: cipher, aad: &[] };
    let plain = gcm.decrypt(Nonce::from_slice(nonce_bytes), payload).map_err(|_| "AES-GCM decrypt failed".to_string())?;
    Ok(String::from_utf8_lossy(&plain).trim_end_matches(' ').to_string())
}

/// 非 Windows：无法取 Chrome/Edge 主密钥（依赖系统 keychain/libsecret，超出本插件范围）。
#[cfg(not(windows))]
fn chrome_master_key(_user_data_dir: &std::path::Path) -> Result<Vec<u8>, String> {
    Err("Chrome/Edge cookie auto-read is only supported on Windows; use Firefox or paste cookies".to_string())
}

/// 复制到一个临时文件再打开（浏览器运行时锁库）。先用 CreateFileW 全共享模式读；
/// 失败则退回 std::fs::copy（浏览器关闭时可成功）。
fn copy_to_temp(src: &std::path::Path, tag: &str) -> Result<std::path::PathBuf, String> {
    let dst = std::env::temp_dir().join(format!("dbx-lc-{}-{}.sqlite", tag, std::process::id()));
    #[cfg(windows)]
    {
        if copy_shared_read(src, &dst).is_err() {
            std::fs::copy(src, &dst).map_err(|e| format!("copy {}: {e}", src.display()))?;
        }
    }
    #[cfg(not(windows))]
    {
        std::fs::copy(src, &dst).map_err(|e| format!("copy {}: {e}", src.display()))?;
    }
    Ok(dst)
}

/// 以 FILE_SHARE_READ|WRITE|DELETE 打开源文件并复制（绕过浏览器运行时对 Cookies 的锁定）。
#[cfg(windows)]
fn copy_shared_read(src: &std::path::Path, dst: &std::path::Path) -> Result<(), String> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, ReadFile, FILE_GENERIC_READ, FILE_SHARE_DELETE, FILE_SHARE_READ,
        FILE_SHARE_WRITE, OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL,
    };
    unsafe {
        let wide: Vec<u16> = src.as_os_str().to_string_lossy().encode_utf16().chain(std::iter::once(0)).collect();
        let handle = CreateFileW(wide.as_ptr(), FILE_GENERIC_READ, FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE, std::ptr::null(), OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, std::ptr::null_mut());
        if handle == windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE {
            return Err("CreateFileW failed".to_string());
        }
        let result = (|| -> std::io::Result<()> {
            let mut out = std::fs::File::create(dst)?;
            let mut buf = vec![0u8; 1 << 16];
            loop {
                let mut read = 0u32;
                if ReadFile(handle, buf.as_mut_ptr(), buf.len() as u32, &mut read, std::ptr::null_mut()) == 0 {
                    return Err(std::io::Error::last_os_error());
                }
                if read == 0 { break; }
                use std::io::Write;
                out.write_all(&buf[..read as usize])?;
            }
            Ok(())
        })();
        CloseHandle(handle);
        result.map_err(|e| format!("shared read: {e}"))
    }
}

fn query_cookie_db(path: &std::path::Path, is_firefox: bool) -> Result<Vec<(String, String)>, String> {
    let conn = rusqlite::Connection::open(path).map_err(|e| format!("open sqlite: {e}"))?;
    let mut stmt = if is_firefox {
        conn.prepare("SELECT name, value FROM moz_cookies WHERE host LIKE '%leetcode.cn'")
            .map_err(|e| format!("prepare: {e}"))?
    } else {
        conn.prepare("SELECT name, encrypted_value FROM cookies WHERE host_key LIKE '%leetcode.cn'")
            .map_err(|e| format!("prepare: {e}"))?
    };
    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))
        })
        .map_err(|e| format!("query: {e}"))?;
    let mut out = Vec::new();
    for row in rows {
        let (name, value) = row.map_err(|e| format!("row: {e}"))?;
        let text = String::from_utf8_lossy(&value).to_string();
        out.push((name, text));
    }
    Ok(out)
}

/// 从所有常见浏览器读取 leetcode.cn 的 LEETCODE_SESSION / csrftoken。
fn read_browser_leetcode_cookies() -> Result<Value, PluginError> {
    let mut attempts: Vec<String> = Vec::new();
    let mut found: Vec<(String, String, String)> = Vec::new(); // (browser, session, csrf)

    let profile_dirs: Vec<(String, std::path::PathBuf, bool)> = {
        let lad = match local_appdata() {
            Some(p) => p,
            None => return Err(err("no LOCALAPPDATA")),
        };
        let mut v = Vec::new();
        // Chrome / Edge（可加更多 Chromium 系）
        for (label, dir, is_chromium) in [
            ("edge", lad.join(r"Microsoft\Edge\User Data"), true),
            ("chrome", lad.join(r"Google\Chrome\User Data"), true),
        ] {
            if dir.join("Local State").exists() {
                if let Ok(entries) = std::fs::read_dir(&dir) {
                    for entry in entries.flatten() {
                        let name = entry.file_name().to_string_lossy().to_string();
                        if name == "Default" || name.starts_with("Profile ") {
                            v.push((label.to_string(), entry.path(), is_chromium));
                        }
                    }
                }
            }
        }
        // Firefox（%APPDATA%\Mozilla\Firefox\Profiles\*.default*）
        if let Ok(roaming) = std::env::var("APPDATA") {
            let profiles = std::path::Path::new(&roaming).join(r"Mozilla\Firefox\Profiles");
            if let Ok(entries) = std::fs::read_dir(&profiles) {
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if name.contains("default") && entry.path().join("cookies.sqlite").exists() {
                        v.push(("firefox".to_string(), entry.path(), false));
                    }
                }
            }
        }
        v
    };

    for (label, profile_dir, is_chromium) in &profile_dirs {
        let cookie_path = if *is_chromium {
            profile_dir.join("Network").join("Cookies")
        } else {
            profile_dir.join("cookies.sqlite")
        };
        if !cookie_path.exists() {
            continue;
        }
        let tag = format!("{label}-{}", profile_dir.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default());
        let temp = match copy_to_temp(&cookie_path, &tag) {
            Ok(t) => t,
            Err(e) => { attempts.push(format!("{tag}: {e}")); continue; }
        };
        let result: Result<Vec<(String, String)>, String> = if !*is_chromium {
            query_cookie_db(&temp, true)
        } else {
            let key = match chrome_master_key(profile_dir.parent().unwrap_or(profile_dir)) {
                Ok(k) => k,
                Err(e) => { attempts.push(format!("{tag}: key: {e}")); let _ = std::fs::remove_file(&temp); continue; }
            };
            query_cookie_db_raw(&temp)
                .map(|raw_rows| {
                    raw_rows
                        .into_iter()
                        .map(|(name, enc)| (name, decrypt_chromium_value(&enc, &key).unwrap_or_default()))
                        .collect::<Vec<_>>()
                })
        };
        let _ = std::fs::remove_file(&temp);
        match result {
            Ok(rows) => {
                let mut session = String::new();
                let mut csrf = String::new();
                for (name, value) in rows {
                    if name == "LEETCODE_SESSION" && !value.is_empty() { session = value.clone(); }
                    if name == "csrftoken" && !value.is_empty() { csrf = value; }
                }
                if !session.is_empty() {
                    found.push((format!("{label}({})", profile_dir.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()), session, csrf));
                }
            }
            Err(e) => attempts.push(format!("{tag}: {e}")),
        }
    }

    if found.is_empty() {
        return Err(err(format!(
            "未能自动读取 leetcode.cn Cookie。新版 Chrome/Edge 运行时锁定 Cookies 且启用 App-Bound 加密（无法读取属预期）。可选：1) 完全退出 Chrome/Edge 后重试一键登录；2) 在 Firefox 登录 leetcode.cn 后一键登录；3) 手动粘贴 LEETCODE_SESSION / csrftoken。详情：{}",
            if attempts.is_empty() { "无可用浏览器配置".to_string() } else { attempts.join("；") }
        )));
    }
    // 优先返回第一个（edge → chrome → firefox 的顺序由 profile_dirs 顺序决定）
    let (browser, session, csrf) = found.remove(0);
    Ok(json!({ "ok": true, "browser": browser, "session": session, "csrf": csrf, "others": found.len() }))
}

/// Chrome/Edge：拿原始 encrypted_value 字节。
fn query_cookie_db_raw(path: &std::path::Path) -> Result<Vec<(String, Vec<u8>)>, String> {
    let conn = rusqlite::Connection::open(path).map_err(|e| format!("open sqlite: {e}"))?;
    let mut stmt = conn
        .prepare("SELECT name, encrypted_value FROM cookies WHERE host_key LIKE '%leetcode.cn'")
        .map_err(|e| format!("prepare: {e}"))?;
    let rows = stmt
        .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?)))
        .map_err(|e| format!("query: {e}"))?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(|e| format!("row: {e}"))?);
    }
    Ok(out)
}

impl LeetCodePlugin {
    fn request(
        &self,
        method: &str,
        url: &str,
        json_body: Option<Value>,
        form_body: Option<Vec<(String, String)>>,
        with_csrf: bool,
    ) -> Result<(u16, String), PluginError> {
        let (cookies, csrf) = {
            let s = self.session.lock().map_err(|_| err("session poisoned"))?;
            (s.cookie_header(), s.csrf.clone())
        };
        let mut req = remote_agent().request(method, url);
        req = req
            .set("User-Agent", "Mozilla/5.0 (DBX LeetCode Plugin)")
            .set("Accept", "application/json, text/plain, */*");
        if !cookies.is_empty() {
            req = req.set("Cookie", &cookies);
        }
        if with_csrf {
            req = req.set("X-CSRFToken", &csrf).set("Referer", &format!("{BASE}/"));
        }
        let response = if let Some(body) = json_body {
            req.set("Content-Type", "application/json").send_json(body)
        } else if let Some(form) = form_body {
            req.set("Content-Type", "application/x-www-form-urlencoded")
                .send_form(&form.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect::<Vec<_>>())
        } else {
            req.call()
        };
        let response = response.map_err(|e| err(format!("leetcode.cn request failed: {e}")))?;
        let status = response.status();
        let set_cookies: Vec<String> = response.all("set-cookie").iter().map(|v| v.to_string()).collect();
        {
            let mut s = self.session.lock().map_err(|_| err("session poisoned"))?;
            for value in &set_cookies {
                s.absorb_set_cookie(value);
            }
        }
        let body = response
            .into_string()
            .map_err(|e| err(format!("leetcode.cn read failed: {e}")))?;
        Ok((status, body))
    }

    fn request_json(
        &self,
        method: &str,
        url: &str,
        json_body: Option<Value>,
        form_body: Option<Vec<(String, String)>>,
        with_csrf: bool,
    ) -> Result<(u16, Value), PluginError> {
        let (status, text) = self.request(method, url, json_body, form_body, with_csrf)?;
        let value = serde_json::from_str(&text)
            .map_err(|e| err(format!("leetcode.cn 返回非 JSON（{e}）：{}", truncate(&text))))?;
        Ok((status, value))
    }

    fn graphql(&self, query: &str, variables: Value) -> Result<Value, PluginError> {
        let (_, body) = self.request_json("POST", GRAPHQL, Some(json!({ "query": query, "variables": variables })), None, true)?;
        if let Some(errors) = body.get("errors") {
            if !errors.is_null() {
                return Err(err(format!("GraphQL error: {errors}")));
            }
        }
        Ok(body.get("data").cloned().unwrap_or(Value::Null))
    }

    /// Prime the CSRF cookie by loading the login page, then return the current token.
    fn prime_csrf(&self) -> Result<String, PluginError> {
        self.request("GET", &format!("{BASE}/accounts/login/"), None, None, false)?;
        Ok(self.session.lock().map_err(|_| err("session poisoned"))?.csrf.clone())
    }

    fn finish_login(&self, _fallback: &str) -> Result<Value, PluginError> {
        match self.current_username() {
            Ok(username) => {
                let nickname = self.session.lock().map(|s| s.realname.clone()).unwrap_or_default();
                Ok(json!({ "ok": true, "username": username, "nickname": nickname }))
            }
            Err(_) => Err(err(
                "登录未成功：leetcode.cn 通常需要极验滑块/风控校验，无法自动完成账号密码或短信登录。请改用「Cookie 登录」（从浏览器复制 LEETCODE_SESSION 与 csrftoken）。",
            )),
        }
    }

    fn login(&self, login: &str, password: &str) -> Result<Value, PluginError> {
        let csrf = self.prime_csrf()?;
        let (status, _body) = self.request(
            "POST",
            &format!("{BASE}/accounts/login/?next=/"),
            None,
            Some(vec![
                ("csrfmiddlewaretoken".to_string(), csrf),
                ("identity".to_string(), login.to_string()),
                ("password".to_string(), password.to_string()),
                ("next".to_string(), "/".to_string()),
            ]),
            true,
        )?;
        if status >= 400 {
            return Err(err(format!("登录请求失败 HTTP {status}")));
        }
        self.finish_login(login)
    }

    /// Reliable auth path: inject the browser's leetcode.cn session cookies and verify.
    fn login_cookie(&self, session: &str, csrf: &str) -> Result<Value, PluginError> {
        let session = session.trim();
        let csrf = csrf.trim();
        if session.is_empty() {
            return Err(err("请粘贴 LEETCODE_SESSION 的值"));
        }
        {
            let mut s = self.session.lock().map_err(|_| err("session poisoned"))?;
            s.cookies.insert("LEETCODE_SESSION".to_string(), session.to_string());
            s.cookies.insert("csrftoken".to_string(), csrf.to_string());
            s.csrf = csrf.to_string();
        }
        match self.current_username() {
            Ok(username) => {
                let nickname = self.session.lock().map(|s| s.realname.clone()).unwrap_or_default();
                Ok(json!({ "ok": true, "username": username, "nickname": nickname }))
            }
            Err(_) => Err(err("Cookie 无效或已过期（未检测到登录态）。请在已登录的浏览器里重新复制 LEETCODE_SESSION / csrftoken。")),
        }
    }

    /// Request an SMS verification code. NOTE: leetcode.cn gates this behind a geetest slider
    /// captcha; without a captcha token this will usually fail. The raw response is surfaced so
    /// the exact endpoint / required fields can be calibrated against the live site.
    fn send_code(&self, target: &str) -> Result<Value, PluginError> {
        let _csrf = self.prime_csrf()?;
        let (status, body) = self.request(
            "POST",
            &format!("{BASE}/accounts/login/send_sms/"),
            Some(json!({ "phone": target, "type": "phone" })),
            None,
            true,
        )?;
        if status != 200 {
            return Err(err(format!("发送验证码失败 HTTP {status}: {}", truncate(&body))));
        }
        Ok(json!({ "ok": true, "raw": truncate(&body) }))
    }

    /// Log in with a phone/email + SMS verification code. Field names mirror the password flow;
    /// calibrate against leetcode.cn's live login form if it rejects the request.
    fn login_code(&self, target: &str, code: &str) -> Result<Value, PluginError> {
        let csrf = self.prime_csrf()?;
        let (status, _body) = self.request(
            "POST",
            &format!("{BASE}/accounts/login/?next=/"),
            None,
            Some(vec![
                ("csrfmiddlewaretoken".to_string(), csrf),
                ("identity".to_string(), target.to_string()),
                ("verification_code".to_string(), code.to_string()),
                ("type".to_string(), "phone".to_string()),
                ("next".to_string(), "/".to_string()),
            ]),
            true,
        )?;
        if status >= 400 {
            return Err(err(format!("验证码登录请求失败 HTTP {status}")));
        }
        self.finish_login(target)
    }

    fn current_username(&self) -> Result<String, PluginError> {
        let data = self.graphql(
            "query globalData { userStatus { username realName isSignedIn } }",
            json!({}),
        )?;
        let status = data.get("userStatus").cloned().unwrap_or(Value::Null);
        if status.get("isSignedIn").and_then(Value::as_bool).unwrap_or(false) {
            let username = status.get("username").and_then(Value::as_str).unwrap_or_default().to_string();
            let realname = status.get("realName").and_then(Value::as_str).unwrap_or_default().to_string();
            if let Ok(mut s) = self.session.lock() {
                s.username = username.clone();
                s.realname = realname;
            }
            Ok(username)
        } else {
            Err(err("not signed in"))
        }
    }

    /// One page of the problemset.
    ///
    /// `category_slug` drives the official tabs ("" = 全部题目, plus `shell`,
    /// `concurrency`, `javascript`, `pandas`, …). A non-empty `search_keyword`
    /// switches to the V2 endpoint, because the public V1 list has no search
    /// field; V2 is login-gated, so a rejection is reported as a sign-in hint.
    fn list_problems(
        &self,
        skip: i64,
        limit: i64,
        filters: Value,
        category_slug: &str,
        search_keyword: &str,
    ) -> Result<Value, PluginError> {
        let keyword = search_keyword.trim();
        if !keyword.is_empty() {
            return match self.graphql(
                LIST_V2_QUERY,
                json!({ "categorySlug": category_slug, "skip": skip, "limit": limit, "searchKeyword": keyword }),
            ) {
                Ok(data) => {
                    let v2 = data
                        .get("problemsetQuestionListV2")
                        .cloned()
                        .unwrap_or_else(|| json!({ "totalLength": 0, "questions": [] }));
                    Ok(normalize_v2_list(v2))
                }
                Err(e) => Err(err(format!("搜索需要登录力扣账号：{}", e.message))),
            };
        }
        let data = self.graphql(
            LIST_QUERY,
            json!({ "categorySlug": category_slug, "skip": skip, "limit": limit, "filters": filters }),
        )?;
        let list = data
            .get("problemsetQuestionList")
            .cloned()
            .unwrap_or_else(|| json!({ "total": 0, "questions": [] }));
        Ok(list)
    }

    /// The official tag bar: every topic with its question count, most frequent
    /// first. Counts come from `questionIds.len()`, so they match leetcode.cn
    /// exactly instead of being hardcoded.
    fn topic_tags(&self) -> Result<Value, PluginError> {
        let data = self.graphql(TOPIC_TAGS_QUERY, json!({}))?;
        let edges = data
            .pointer("/questionTopicTags/edges")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut tags: Vec<Value> = Vec::with_capacity(edges.len());
        for edge in edges {
            let node = match edge.get("node") {
                Some(node) if node.is_object() => node,
                _ => continue,
            };
            let slug = node.get("slug").and_then(Value::as_str).unwrap_or_default();
            if slug.is_empty() {
                continue;
            }
            let count = node
                .get("questionIds")
                .and_then(Value::as_array)
                .map(Vec::len)
                .unwrap_or(0);
            tags.push(json!({
                "slug": slug,
                "name": node.get("name").cloned().unwrap_or(Value::Null),
                "translatedName": node.get("translatedName").cloned().unwrap_or(Value::Null),
                "count": count,
            }));
        }
        tags.sort_by_key(|tag| std::cmp::Reverse(tag.get("count").and_then(Value::as_i64).unwrap_or(0)));
        Ok(json!({ "tags": tags, "total": tags.len() }))
    }

    fn get_problem(&self, title_slug: &str) -> Result<Value, PluginError> {
        let data = self.graphql(DETAIL_QUERY, json!({ "titleSlug": title_slug }))?;
        let q = data.get("question").cloned().ok_or_else(|| err("question not found"))?;
        let pick = |keys: &[&str]| -> String {
            for k in keys {
                if let Some(s) = q.get(*k).and_then(Value::as_str) {
                    if !s.trim().is_empty() { return s.to_string(); }
                }
            }
            String::new()
        };
        let mut out = serde_json::Map::new();
        out.insert("questionId".into(), q.get("questionId").cloned().unwrap_or(Value::Null));
        out.insert("frontendQuestionId".into(), q.get("questionFrontendId").cloned().unwrap_or(Value::Null));
        out.insert("title".into(), q.get("title").cloned().unwrap_or(Value::Null));
        out.insert("titleCn".into(), Value::String(pick(&["translatedTitle", "title"])));
        out.insert("titleSlug".into(), q.get("titleSlug").cloned().unwrap_or(Value::Null));
        out.insert("content".into(), Value::String(pick(&["translatedContent", "content"])));
        out.insert("difficulty".into(), q.get("difficulty").cloned().unwrap_or(Value::Null));
        out.insert("isPaidOnly".into(), q.get("isPaidOnly").cloned().unwrap_or(Value::Bool(false)));
        out.insert("sampleTestCase".into(), q.get("sampleTestCase").cloned().unwrap_or(Value::Null));
        out.insert("stats".into(), q.get("stats").cloned().unwrap_or(Value::Null));
        out.insert("codeSnippets".into(), q.get("codeSnippets").cloned().unwrap_or_else(|| Value::Array(vec![])));
        out.insert("topicTags".into(), q.get("topicTags").cloned().unwrap_or_else(|| Value::Array(vec![])));
        Ok(Value::Object(out))
    }

    /// Toggle a question's favorite state. NOTE: login-gated, endpoint/field names are best-effort
    /// and NOT verified against the live site (requires a signed-in session).
    fn toggle_favorite(&self, title_slug: &str, favorite: bool) -> Result<Value, PluginError> {
        let (_status, body) = self.request_json(
            "POST",
            &format!("{BASE}/problems/{title_slug}/favorite/"),
            Some(json!({ "favorite": favorite })),
            None,
            true,
        )?;
        Ok(body)
    }

    /// List the signed-in user's favorite lists. NOTE: favorite-list GraphQL field names could not
    /// be validated against leetcode.cn (introspection disabled); left best-effort and unverified.
    fn my_favorites(&self) -> Result<Value, PluginError> {
        let data = self.graphql(
            "query { favQuestionList: userFavouriteQuestionList { id name } }",
            json!({}),
        )?;
        Ok(data)
    }

    /// Submission history for a question. Field names validated against leetcode.cn's schema
    /// (`submissionList(questionSlug, limit){ submissions { id status lang runtime memory timestamp } }`);
    /// a signed-in session is required at runtime (server returns an error otherwise).
    fn submission_list(&self, title_slug: &str) -> Result<Value, PluginError> {
        let data = self.graphql(
            "query submissionList($questionSlug: String!, $limit: Int) { submissionList(questionSlug: $questionSlug, limit: $limit) { submissions { id status lang runtime memory timestamp } } }",
            json!({ "questionSlug": title_slug, "limit": 20 }),
        )?;
        Ok(data.get("submissionList").cloned().unwrap_or(Value::Null))
    }

    fn judge(&self, title_slug: &str, lang: &str, code: &str, inputs: &str, submit: bool) -> Result<Value, PluginError> {
        let detail = self.get_problem(title_slug)?;
        let question_id = detail.get("questionId").and_then(Value::as_str).unwrap_or_default().to_string();
        let data_input = if inputs.is_empty() {
            detail.get("sampleTestCase").and_then(Value::as_str).unwrap_or("").to_string()
        } else {
            inputs.to_string()
        };
        let payload = json!({
            "data_input": data_input,
            "question_id": question_id,
            "lang": lang,
            "codedetail": code,
            "judge_type": "general"
        });
        let path = if submit { "submit" } else { "interpret_solution" };
        let url = format!("{BASE}/problems/{title_slug}/{path}/");
        let (status, body) = self.request_json("POST", &url, Some(payload), None, true)?;
        if status != 200 {
            return Err(err(format!("judge request failed: HTTP {status}: {body}")));
        }
        let id = body
            .get(if submit { "submission_id" } else { "interpret_id" })
            .and_then(|v| v.as_str().map(|s| s.to_string()).or_else(|| v.as_i64().map(|n| n.to_string())))
            .ok_or_else(|| err(format!("no judge id in response: {body}")))?;
        // Poll for the verdict.
        let check_url = format!("{BASE}/problems/{title_slug}/check/?submissionId={id}&format=json");
        for _ in 0..60 {
            let (_, check) = self.request_json("GET", &check_url, None, None, false)?;
            let finished = check.get("state").and_then(Value::as_str).map(|s| s == "Finished").unwrap_or(false);
            if finished {
                return Ok(self.normalize_result(&check));
            }
            std::thread::sleep(std::time::Duration::from_millis(1000));
        }
        Err(err("判题超时（轮询未完成）"))
    }

    fn normalize_result(&self, check: &Value) -> Value {
        let status = check.get("status_code").and_then(Value::as_i64).unwrap_or(-1);
        let status_label = match status {
            10 => "Accepted",
            11 => "Wrong Answer",
            13 => "Runtime Error",
            14 => "Time Limit Exceeded",
            15 => "Memory Limit Exceeded",
            16 => "Output Limit Exceeded",
            20 => "Compile Error",
            _ => "Pending / Unknown",
        };
        json!({
            "status": status_label,
            "statusLabel": status_label,
            "statusCode": status,
            "runtime": check.get("status_runtime").and_then(Value::as_str),
            "memory": check.get("status_memory").and_then(Value::as_str),
            "expectedOutput": check.get("expected_output"),
            "codeOutput": check.get("code_output"),
            "compileError": check.get("compile_error"),
            "error": check.get("error"),
            "message": check.get("message"),
        })
    }

    /// 第一步：启动受控 Chrome/Edge 打开力扣官方登录页，保存会话状态后立即返回。
    /// 拆成 start + poll 两个快速请求，避免被 DBX 主机的 30s 单请求超时打断（登录需要用户手动完成，远超 30s）。
    fn browser_login_start(&self) -> Result<Value, PluginError> {
        self.cancel_capture();
        let exe = find_chromium().ok_or_else(|| {
            err("未找到 Chrome / Edge。请安装其一，或改用下方『手动粘贴 Cookie』。")
        })?;
        let port = free_port().ok_or_else(|| err("无法分配本地调试端口"))?;
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let profile = std::env::temp_dir().join(format!("dbx-leetcode-cdp-{}-{}", std::process::id(), stamp));
        let _ = std::fs::create_dir_all(&profile);
        let url = format!("{BASE}/accounts/login/?next=/");
        cdp_log(&format!("browser_login_start exe={} port={} profile={}", exe.display(), port, profile.display()));
        let child = std::process::Command::new(&exe)
            .arg(format!("--remote-debugging-port={port}"))
            .arg(format!("--user-data-dir={}", profile.display()))
            .arg("--no-first-run")
            .arg("--no-default-browser-check")
            .arg("--class=DBXLeetCodeLogin")
            .arg(format!("--app={url}"))
            .spawn()
            .map_err(|e| {
                cdp_log(&format!("spawn failed: {e}"));
                let _ = std::fs::remove_dir_all(&profile);
                err(format!("启动浏览器失败：{e}"))
            })?;
        let pid = child.id();
        *self.capture.lock().map_err(|_| err("capture poisoned"))? =
            Some(Capture { child, port, profile, started: Instant::now(), ready: false });
        cdp_log(&format!("browser spawned pid={pid}"));
        Ok(json!({ "ok": true, "pid": pid }))
    }

    /// 第二步：前端每隔几秒调用一次。快速检查受控浏览器里是否已完成登录；成功或超时则收尾。
    fn browser_login_poll(&self) -> Result<Value, PluginError> {
        let mut guard = self.capture.lock().map_err(|_| err("capture poisoned"))?;
        let cap = match guard.as_mut() {
            Some(c) => c,
            None => return Err(err("没有正在进行的浏览器登录，请重新点击『用浏览器登录并自动导入』")),
        };
        let base_http = format!("http://127.0.0.1:{}", cap.port);
        // 受控浏览器是否已被用户关闭
        match cap.child.try_wait() {
            Ok(Some(_status)) => {
                cdp_log("poll: controlled browser exited");
                self.teardown_capture(cap);
                *guard = None;
                return Ok(json!({ "status": "error", "reason": "browser-closed", "message": "受控浏览器窗口被关闭了。请重新点击『用浏览器登录并自动导入』。" }));
            }
            Ok(None) => {}
            Err(_) => {}
        }
        // 调试端口是否就绪
        if local_agent().get(&format!("{base_http}/json/version")).call().is_err() {
            if !cap.ready && cap.started.elapsed() > Duration::from_secs(25) {
                cdp_log("poll: devtools never came up");
                self.teardown_capture(cap);
                *guard = None;
                return Ok(json!({ "status": "error", "reason": "devtools-never-ready", "message": "受控浏览器的调试端口一直没能就绪（可能被安全软件或企业策略拦截）。请改用『手动粘贴 Cookie』。" }));
            }
            cdp_log("poll: devtools not ready");
            return Ok(json!({ "status": "pending", "reason": "devtools-not-ready" }));
        }
        cap.ready = true;
        match self.read_leetcode_cookies_via_cdp(&base_http) {
            Some((session, csrf)) => {
                cdp_log(&format!("poll: session len={} verifying", session.len()));
                match self.login_cookie(&session, &csrf) {
                    Ok(mut r) => {
                        r["source"] = json!("devtools");
                        cdp_log(&format!("poll: login verified user={}", r.get("username").and_then(Value::as_str).unwrap_or("")));
                        self.teardown_capture(cap);
                        *guard = None;
                        return Ok(r);
                    }
                    Err(e) => cdp_log(&format!("poll: verify failed (not logged in yet): {:?}", e)),
                }
            }
            None => cdp_log("poll: no leetcode session cookie yet"),
        }
        if cap.started.elapsed() > Duration::from_secs(300) {
            cdp_log("poll: timeout (300s), cleaning up");
            self.teardown_capture(cap);
            *guard = None;
            return Ok(json!({ "status": "timeout" }));
        }
        Ok(json!({ "status": "pending" }))
    }

    /// 收尾：结束受控浏览器（含其子进程树）、清理临时配置目录、清空会话状态。
    fn teardown_capture(&self, cap: &mut Capture) {
        let pid = cap.child.id().to_string();
        #[cfg(windows)]
        {
            let _ = std::process::Command::new("taskkill").args(["/F", "/T", "/PID", &pid]).output();
        }
        let _ = cap.child.kill();
        let _ = cap.child.wait();
        let _ = std::fs::remove_dir_all(&cap.profile);
    }

    fn cancel_capture(&self) {
        if let Ok(mut g) = self.capture.lock() {
            if let Some(cap) = g.as_mut() {
                self.teardown_capture(cap);
            }
            *g = None;
        }
    }

    fn read_leetcode_cookies_via_cdp(&self, base_http: &str) -> Option<(String, String)> {
        let targets: Value = local_agent().get(&format!("{base_http}/json")).call().ok()?.into_json().ok()?;
        let arr = targets.as_array()?;
        let ws_url = arr
            .iter()
            .filter(|t| t.get("type").and_then(Value::as_str) == Some("page"))
            .find_map(|t| t.get("webSocketDebuggerUrl").and_then(Value::as_str).map(|s| s.to_string()))?;
        let (mut ws, _) = tungstenite::connect(&ws_url).ok()?;
        use tungstenite::Message;
        let cmd = json!({ "id": 1, "method": "Network.getAllCookies" }).to_string();
        ws.send(Message::Text(cmd)).ok()?;
        let mut session = String::new();
        let mut csrf = String::new();
        for _ in 0..50 {
            let msg = match ws.read() {
                Ok(m) => m,
                Err(_) => break,
            };
            let text = match msg {
                Message::Text(t) => t,
                _ => continue,
            };
            let v: Value = match serde_json::from_str(&text) {
                Ok(v) => v,
                Err(_) => continue,
            };
            if v.get("id").and_then(Value::as_i64) != Some(1) {
                continue;
            }
            if let Some(cookies) = v.pointer("/result/cookies").and_then(Value::as_array) {
                for c in cookies {
                    let name = c.get("name").and_then(Value::as_str).unwrap_or("");
                    let domain = c.get("domain").and_then(Value::as_str).unwrap_or("");
                    let value = c.get("value").and_then(Value::as_str).unwrap_or("");
                    if domain.contains("leetcode.cn") {
                        if name == "LEETCODE_SESSION" && !value.is_empty() {
                            session = value.to_string();
                        }
                        if name == "csrftoken" && !value.is_empty() {
                            csrf = value.to_string();
                        }
                    }
                }
            }
            break;
        }
        let _ = ws.close(None);
        if session.is_empty() {
            None
        } else {
            Some((session, csrf))
        }
    }
}

/// 打开系统默认浏览器访问指定 leetcode.cn 页面（登录页 / 个人页）。
/// 沙箱化工作台 iframe 受 CSP `default-src 'none'` 限制，无法内嵌外部网页，
/// 也就无法像 IDEA(JetBrains) 插件那样在窗口里直接完成极验滑块。
/// 折中方案：跳到真实系统浏览器完成登录（极验在真实浏览器里可正常通过），
/// 再回到插件用「一键导入浏览器 Cookie」或「粘贴 Cookie」把登录态同步进来。
fn open_system_browser(url: &str) -> Result<Value, PluginError> {
    let spawned = if cfg!(windows) {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .spawn()
            .is_ok()
    } else if cfg!(target_os = "macos") {
        std::process::Command::new("open").arg(url).spawn().is_ok()
    } else {
        std::process::Command::new("xdg-open").arg(url).spawn().is_ok()
    };
    if spawned {
        Ok(json!({ "ok": true, "url": url }))
    } else {
        Err(err("无法打开系统浏览器，请手动访问 https://leetcode.cn 登录后粘贴 Cookie。"))
    }
}

/// 取一个当前空闲的本地端口，用于 Chrome DevTools 调试。
fn free_port() -> Option<u16> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").ok()?;
    let port = listener.local_addr().ok()?.port();
    drop(listener);
    Some(port)
}

/// 诊断日志：写入 %TEMP%/dbx-leetcode-cdp.log，便于在 DBX 后端日志关闭时排查登录抓取流程。
fn cdp_log(msg: &str) {
    let path = std::env::temp_dir().join("dbx-leetcode-cdp.log");
    let line = format!("[{:?}] {msg}\n", std::time::SystemTime::now());
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        use std::io::Write;
        let _ = f.write_all(line.as_bytes());
    }
}

/// 定位本机 Chrome / Edge 可执行文件（Windows 优先，兼容 macOS / Linux）。
fn find_chromium() -> Option<std::path::PathBuf> {
    let mut cands: Vec<std::path::PathBuf> = Vec::new();
    #[cfg(windows)]
    {
        for key in ["ProgramFiles", "ProgramFiles(x86)", "ProgramW6432", "LOCALAPPDATA"] {
            if let Ok(root) = std::env::var(key) {
                let p = std::path::Path::new(&root);
                cands.push(p.join(r"Google\Chrome\Application\chrome.exe"));
                cands.push(p.join(r"Microsoft\Edge\Application\msedge.exe"));
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        cands.push(std::path::PathBuf::from("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"));
        cands.push(std::path::PathBuf::from("/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge"));
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        for name in ["google-chrome", "google-chrome-stable", "chromium", "chromium-browser", "microsoft-edge"] {
            if let Ok(out) = std::process::Command::new("which").arg(name).output() {
                if out.status.success() {
                    let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
                    if !path.is_empty() {
                        cands.push(std::path::PathBuf::from(path));
                    }
                }
            }
        }
    }
    cands.into_iter().find(|c| c.exists())
}

impl PluginHandler for LeetCodePlugin {
    fn handle(&self, _context: RequestContext, method: &str, params: Value, _emitter: &PluginEmitter) -> Result<Value, PluginError> {
        let get_str = |key: &str| params.get(key).and_then(Value::as_str).unwrap_or("").to_string();
        match method {
            "leetcode/status" => {
                let username = self.current_username().unwrap_or_default();
                let nickname = self.session.lock().map(|s| s.realname.clone()).unwrap_or_default();
                Ok(json!({ "authenticated": !username.is_empty(), "username": username, "nickname": nickname }))
            }
            "leetcode/login" => self.login(&get_str("login"), &get_str("password")),
            "leetcode/send_code" => self.send_code(&get_str("target")),
            "leetcode/login_code" => self.login_code(&get_str("target"), &get_str("code")),
            "leetcode/login_cookie" => self.login_cookie(&get_str("session"), &get_str("csrf")),
            "leetcode/read_browser_cookies" => read_browser_leetcode_cookies(),
            "leetcode/open_login_browser" => open_system_browser(&format!("{BASE}/accounts/login/?next=/")),
            "leetcode/browser_login_start" => self.browser_login_start(),
            "leetcode/browser_login_poll" => self.browser_login_poll(),
            "leetcode/browser_login_cancel" => {
                self.cancel_capture();
                Ok(json!({ "ok": true }))
            }
            "leetcode/login_auto" => {
                let cookies = read_browser_leetcode_cookies()?;
                let session = cookies.get("session").and_then(Value::as_str).unwrap_or_default().to_string();
                let csrf = cookies.get("csrf").and_then(Value::as_str).unwrap_or_default().to_string();
                let browser = cookies.get("browser").and_then(Value::as_str).unwrap_or("browser").to_string();
                match self.login_cookie(&session, &csrf) {
                    Ok(mut result) => {
                        result["browser"] = Value::String(browser);
                        Ok(result)
                    }
                    Err(e) => Err(e),
                }
            }
            "leetcode/logout" => {
                let mut s = self.session.lock().map_err(|_| err("session poisoned"))?;
                s.cookies.clear();
                s.csrf.clear();
                s.username.clear();
                Ok(json!({ "ok": true }))
            }
            "leetcode/list_problems" => {
                let skip = params.get("skip").and_then(Value::as_i64).unwrap_or(0);
                let limit = params.get("limit").and_then(Value::as_i64).unwrap_or(30);
                let filters = params.get("filters").cloned().unwrap_or_else(|| json!({}));
                let category_slug = params.get("categorySlug").and_then(Value::as_str).unwrap_or("");
                let search_keyword = params.get("searchKeyword").and_then(Value::as_str).unwrap_or("");
                self.list_problems(skip, limit, filters, category_slug, search_keyword)
            }
            "leetcode/topic_tags" => self.topic_tags(),
            "leetcode/get_problem" => self.get_problem(&get_str("titleSlug")),
            "leetcode/toggle_favorite" => self.toggle_favorite(&get_str("titleSlug"), params.get("favorite").and_then(Value::as_bool).unwrap_or(true)),
            "leetcode/my_favorites" => self.my_favorites(),
            "leetcode/submission_list" => self.submission_list(&get_str("titleSlug")),
            "leetcode/run_code" => {
                let inputs = params.get("inputs").and_then(Value::as_str).unwrap_or("").to_string();
                self.judge(&get_str("titleSlug"), &get_str("lang"), &get_str("code"), &inputs, false)
            }
            "leetcode/submit_code" => self.judge(&get_str("titleSlug"), &get_str("lang"), &get_str("code"), "", true),
            other => Err(PluginError::new(-32601, format!("unknown method: {other}"))),
        }
    }
}

fn main() -> std::io::Result<()> {
    let metadata = PluginMetadata::new("com.yiqiui.leetcode-cn", env!("CARGO_PKG_VERSION")).with_capability("workbench");
    PluginServer::new(metadata, LeetCodePlugin::default()).serve()
}
