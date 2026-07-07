use crate::error::{AppError, AppResult};
use crate::nexus::ValidateResult;
use crate::state::AppState;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

const EMAILNATOR: &str = "https://www.emailnator.com";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TempAccount {
    pub email: String,
    pub username: String,
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmailMessage {
    pub id: String,
    pub from: String,
    pub subject: String,
    pub time: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationResult {
    pub found: bool,
    pub link: Option<String>,
    pub subject: Option<String>,
    pub code: Option<String>,
}

impl TempAccount {
    pub fn generate(email: &str) -> Self {
        let username = random_string(10);
        let password = random_password();
        TempAccount {
            email: email.to_string(),
            username,
            password,
        }
    }
}

fn random_string(len: usize) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let uuid = uuid::Uuid::new_v4().to_string();
    let seed = format!("{now}{uuid}");
    let mut hasher = DefaultHasher::new();
    seed.hash(&mut hasher);
    let mut h = hasher.finish();
    const CHARS: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut out = String::with_capacity(len);
    for _ in 0..len {
        h = h
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        out.push(CHARS[(h >> 33) as usize % CHARS.len()] as char);
    }
    out
}

/// Generate a password that always satisfies Nexus requirements:
/// 12+ chars, at least one uppercase, one lowercase, and one digit.
fn random_password() -> String {
    let base = random_string(14);
    // Guarantee the requirements are met by appending a known pattern.
    // We take the first 11 random chars, then append "aA1" (lower/upper/digit).
    let mut pwd = String::with_capacity(14);
    pwd.push_str(&base[..11]);
    pwd.push_str("aA1");
    pwd
}

struct Session {
    client: reqwest::Client,
    xsrf_token: String,
    cookie_header: String,
}

impl Session {
    fn user_agent() -> &'static str {
        "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 \
         (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36"
    }

    async fn fresh() -> AppResult<Self> {
        let client = reqwest::Client::builder()
            .user_agent(Self::user_agent())
            .build()?;

        let resp = client.get(EMAILNATOR).send().await?;
        let cookies = extract_set_cookies(&resp);

        let xsrf = cookies
            .iter()
            .find(|c| c.starts_with("XSRF-TOKEN="))
            .and_then(|c| {
                let raw = c.strip_prefix("XSRF-TOKEN=").unwrap_or("");
                let decoded = urlencoding_helper(raw);
                if decoded.is_empty() { None } else { Some(decoded) }
            })
            .unwrap_or_default();

        if xsrf.is_empty() {
            return Err(AppError::msg("could not obtain emailnator XSRF token"));
        }

        let cookie_header = cookies
            .iter()
            .filter(|c| c.starts_with("XSRF-TOKEN=") || c.starts_with("gmailnator_session="))
            .cloned()
            .collect::<Vec<_>>()
            .join("; ");

        Ok(Self {
            client,
            xsrf_token: xsrf,
            cookie_header,
        })
    }

    fn auth_headers(&self) -> Vec<(&'static str, String)> {
        vec![
            ("X-XSRF-TOKEN", self.xsrf_token.clone()),
            ("Cookie", self.cookie_header.clone()),
        ]
    }

    fn update_from_response(&mut self, resp: &reqwest::Response) {
        let new_cookies = extract_set_cookies(resp);
        for c in &new_cookies {
            if c.starts_with("XSRF-TOKEN=") {
                let raw = c.strip_prefix("XSRF-TOKEN=").unwrap_or("");
                let decoded = urlencoding_helper(raw);
                if !decoded.is_empty() {
                    self.xsrf_token = decoded;
                }
            }
        }
        let mut parts: Vec<String> = self.cookie_header.split("; ").map(|s| s.to_string()).collect();
        for nc in &new_cookies {
            let name = nc.split('=').next().unwrap_or("");
            if name.is_empty() {
                continue;
            }
            parts.retain(|p| !p.starts_with(&format!("{name}=")));
            if nc.starts_with("XSRF-TOKEN=") || nc.starts_with("gmailnator_session=") {
                parts.push(nc.clone());
            }
        }
        self.cookie_header = parts.join("; ");
    }

    async fn generate_email(&mut self) -> AppResult<String> {
        let body = serde_json::json!({ "email": ["dotGmail"] });
        let req = self
            .client
            .post(format!("{EMAILNATOR}/generate-email"))
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/plain, */*")
            .header("X-Requested-With", "XMLHttpRequest")
            .header("Origin", EMAILNATOR)
            .header("Referer", format!("{EMAILNATOR}/"));

        let mut req = req;
        for (k, v) in &self.auth_headers() {
            req = req.header(*k, v);
        }

        let resp = req.json(&body).send().await?;
        self.update_from_response(&resp);

        let v: serde_json::Value = resp.json().await?;
        let emails = v
            .get("email")
            .and_then(|e| e.as_array())
            .ok_or_else(|| AppError::msg("emailnator: unexpected generate-email response"))?;
        emails
            .first()
            .and_then(|e| e.as_str())
            .map(String::from)
            .ok_or_else(|| AppError::msg("emailnator: no email returned"))
    }

    async fn message_list(&mut self, email: &str) -> AppResult<Vec<EmailMessage>> {
        let body = serde_json::json!({ "email": email });
        let mut req = self
            .client
            .post(format!("{EMAILNATOR}/message-list"))
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/plain, */*")
            .header("X-Requested-With", "XMLHttpRequest")
            .header("Origin", EMAILNATOR)
            .header("Referer", format!("{EMAILNATOR}/"));

        for (k, v) in &self.auth_headers() {
            req = req.header(*k, v);
        }

        let resp = req.json(&body).send().await?;
        self.update_from_response(&resp);

        let status = resp.status();
        if !status.is_success() {
            return Err(AppError::msg(format!(
                "emailnator message-list returned {status}"
            )));
        }

        let v: serde_json::Value = resp.json().await?;
        let arr = v
            .get("messageData")
            .and_then(|d| d.as_array())
            .cloned()
            .unwrap_or_default();

        let mut msgs = Vec::new();
        for item in arr {
            let id = item
                .get("messageID")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            msgs.push(EmailMessage {
                from: item
                    .get("from")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string(),
                subject: item
                    .get("subject")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string(),
                time: item
                    .get("time")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string(),
                id,
            });
        }
        Ok(msgs)
    }

    async fn fetch_message(&mut self, email: &str, message_id: &str) -> AppResult<String> {
        let body = serde_json::json!({
            "email": email,
            "messageID": message_id,
        });
        let mut req = self
            .client
            .post(format!("{EMAILNATOR}/message-list"))
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/plain, */*")
            .header("X-Requested-With", "XMLHttpRequest")
            .header("Origin", EMAILNATOR)
            .header("Referer", format!("{EMAILNATOR}/"));

        for (k, v) in &self.auth_headers() {
            req = req.header(*k, v);
        }

        let resp = req.json(&body).send().await?;
        self.update_from_response(&resp);

        let _status = resp.status();
        let resp_text = resp.text().await?;

        // emailnator sometimes returns raw HTML, sometimes JSON with a "data" key.
        if resp_text.trim_start().starts_with('<') {
            // Raw HTML body — use directly.
            return Ok(resp_text);
        }

        // Try JSON with a "data" field.
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&resp_text) {
            if let Some(data) = v.get("data").and_then(|d| d.as_str()) {
                return Ok(data.to_string());
            }
            if let Some(data) = v.get("data") {
                let raw = data.to_string();
                if !raw.is_empty() && raw != "null" && raw != "\"\"" {
                    return Ok(raw);
                }
            }
            if let Some(msg) = v.get("message").and_then(|s| s.as_str()) {
                if msg != "Server Error" {
                    return Ok(msg.to_string());
                }
            }
        }

        Err(AppError::msg(format!(
            "emailnator: could not fetch body for msg {message_id}"
        )))
    }
}

fn extract_set_cookies(resp: &reqwest::Response) -> Vec<String> {
    resp.headers()
        .get_all("set-cookie")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|h| h.split(';'))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn urlencoding_helper(input: &str) -> String {
    input
        .replace("%3D", "=")
        .replace("%2F", "/")
        .replace("%2B", "+")
        .replace("%3A", ":")
        .replace("%40", "@")
        .replace("%2E", ".")
        .replace("%25", "%")
}

fn extract_verification_info(body: &str) -> (Option<String>, Option<String>) {
    if body.is_empty() {
        return (None, None);
    }
    let body_lower = body.to_lowercase();
    let link = extract_http_link(&body_lower);
    let code = extract_four_digit_code(body);
    (link, code)
}

fn extract_http_link(body_lower: &str) -> Option<String> {
    for pattern in &["nexusmods.com", "href="] {
        if let Some(idx) = body_lower.find(pattern) {
            let start = if *pattern == "href=" {
                idx + 5
            } else {
                idx.saturating_sub(8)
            };
            let slice = &body_lower[start..];
            let link_start = if let Some(p) = slice.find("http") {
                start + p
            } else {
                start
            };
            let slice2 = &body_lower[link_start..];
            if let Some(end) = slice2
                .find(|c: char| c == '"' || c == '\'' || c == '>' || c.is_whitespace())
            {
                let link = &slice2[..end]
                    .trim_end_matches(|c| c == '"' || c == '\'' || c == '>' || c == ' ');
                if link.starts_with("https://") || link.starts_with("http://") {
                    return Some(link.to_string());
                }
            }
        }
    }
    None
}

fn extract_four_digit_code(body: &str) -> Option<String> {
    // Strip the emailnator subject-header wrapper so we only scan the real
    // email content.  The wrapper looks like:
    //  <div id="subject-header">…<hr/></div></div>
    let real_body = if let Some(hr_end) = body.find("<hr") {
        // Cut after the closing </div> that follows <hr>
        if let Some(cut) = body[hr_end..].find("</div></div>") {
            &body[hr_end + cut + 12..]
        } else {
            body
        }
    } else {
        body
    };

    // First pass: look for a 4-digit code near "code", "verification", or
    // "enter the code" — these are strong signals.
    let lower = real_body.to_lowercase();
    let keywords = ["verification code", "enter the code", "your code"];
    for kw in &keywords {
        if let Some(pos) = lower.find(kw) {
            let window = &real_body[pos.saturating_sub(50)..];
            if let Some(code) = scan_for_4_digit(window) {
                return Some(code);
            }
        }
    }

    // Second pass: scan the whole body for any isolated 4-digit number.
    scan_for_4_digit(real_body)
}

fn scan_for_4_digit(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    while i + 4 <= len {
        if bytes[i].is_ascii_digit()
            && bytes[i + 1].is_ascii_digit()
            && bytes[i + 2].is_ascii_digit()
            && bytes[i + 3].is_ascii_digit()
        {
            // Ensure the 4 digits are isolated (not part of a longer number)
            let before_ok = i == 0 || !bytes[i - 1].is_ascii_digit();
            let after_ok = i + 4 >= len || !bytes[i + 4].is_ascii_digit();
            if before_ok && after_ok {
                let code = &s[i..i + 4];
                eprintln!("scan_for_4_digit found candidate: {code} at offset {i}");
                return Some(code.to_string());
            }
            i += 4;
        } else {
            i += 1;
        }
    }
    None
}

pub async fn create_temp_account() -> AppResult<TempAccount> {
    let mut session = Session::fresh().await?;
    let email = session.generate_email().await?;
    Ok(TempAccount::generate(&email))
}

pub async fn poll_verification(email: &str, timeout_secs: u64) -> AppResult<VerificationResult> {
    let deadline = std::time::Instant::now() + Duration::from_secs(timeout_secs);
    let poll_interval = Duration::from_secs(5);

    loop {
        if std::time::Instant::now() > deadline {
            return Ok(VerificationResult {
                found: false,
                link: None,
                subject: None,
                code: None,
            });
        }

        let result = try_poll(email).await;
        match result {
            Ok(v) => return Ok(v),
            Err(e) => {
                eprintln!("emailnator poll error: {e}, retrying in 5s...");
                tokio::time::sleep(poll_interval).await;
            }
        }
    }
}

async fn try_poll(email: &str) -> AppResult<VerificationResult> {
    let mut session = Session::fresh().await?;
    let messages = session.message_list(email).await?;

    eprintln!("emailnator: {} total messages for {}", messages.len(), email);

    for msg in &messages {
        eprintln!(
            "emailnator msg: from=\"{}\" subject=\"{}\" id={}",
            msg.from, msg.subject, msg.id
        );
    }

    for msg in &messages {
        let subject_lower = msg.subject.to_lowercase();
        let from_lower = msg.from.to_lowercase();

        if from_lower.contains("nexus")
            || subject_lower.contains("nexus")
            || subject_lower.contains("verification code")
            || subject_lower.contains("confirm email")
        {
            let body = match session.fetch_message(email, &msg.id).await {
                Ok(b) => b,
                Err(e) => {
                    eprintln!(
                        "emailnator fetch_message FAILED for id={}: {e}",
                        msg.id
                    );
                    String::new()
                }
            };
            let body_len = body.len();
            let body_preview: String = body.chars().take(300).collect();
            eprintln!("emailnator body len={body_len}, preview:\n{body_preview}");
            let (link, code) = extract_verification_info(&body);
            eprintln!("emailnator extract: link={link:?} code={code:?}");

            if link.is_some() || code.is_some() {
                return Ok(VerificationResult {
                    found: true,
                    link,
                    code,
                    subject: Some(msg.subject.clone()),
                });
            }

            return Ok(VerificationResult {
                found: true,
                link: None,
                code: None,
                subject: Some(msg.subject.clone()),
            });
        }
    }

    Err(AppError::msg("no verification email yet"))
}

// ---------------------------------------------------------------------------
// Fully‑automated Nexus registration via an embedded WebView + emailnator
// ---------------------------------------------------------------------------

fn emit_log(app: &AppHandle, step: &str, detail: &str, is_error: bool) {
    let level = if is_error { "error" } else { "info" };
    let _ = app.emit("auto-register-log", serde_json::json!({
        "step": step,
        "detail": detail,
        "level": level,
        "ts": chrono::Utc::now().to_rfc3339(),
    }));
}

/// Use the native HTMLInputElement value setter — works around React controlled inputs.
const NATIVE_VALUE_SETTER: &str = r#"
function __sv(el,v){var d=Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value');d.set.call(el,v);el.dispatchEvent(new Event('input',{bubbles:true}));el.dispatchEvent(new Event('change',{bubbles:true}));}
"#;

/// Inject JS to extract an API key from the page.  Sets ``location.hash``
/// to ``#scav_key=ENCODED_KEY`` so Rust can read it from the URL without
/// navigating away from the page.
async fn try_extract_api_key(window: &tauri::WebviewWindow) -> Option<String> {
    let js = r#"(function(){
var els=document.querySelectorAll('input[readonly], [class*="key"], pre, code, table td:last-child');
for(var i=0;i<els.length;i++){
var t=(els[i].value||els[i].textContent||'').trim().replace(/[\n\r\s]/g,'');
if(t.length>20&&t.length<200){
window.location.hash='#scav_key='+encodeURIComponent(t);
return;
}
}
})()"#;
    window.eval(js).ok();

    // Poll briefly for the hash
    for _ in 0..8 {
        if let Ok(url) = window.url() {
            let us = url.as_str();
            if let Some(idx) = us.find("#scav_key=") {
                let encoded = &us[idx + 10..];
                if let Ok(decoded) = urlencoding::decode(encoded) {
                    let key = decoded.into_owned();
                    if key.len() > 20 && key.len() < 200 {
                        // Clear the hash so we don't re-detect it
                        window.eval("window.location.hash=''").ok();
                        return Some(key);
                    }
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    None
}

pub async fn register_and_authorize(
    app_handle: &AppHandle,
    state: &AppState,
) -> AppResult<ValidateResult> {
    // ---------------------------------------------------------------
    // 1. Create temp account with a disposable email
    // ---------------------------------------------------------------
    emit_log(app_handle, "email", "Generating temporary email…", false);
    let mut session = Session::fresh().await?;
    let email = session.generate_email().await?;
    let account = TempAccount::generate(&email);
    emit_log(app_handle, "email", &format!("Email: {email}"), false);

    // ---------------------------------------------------------------
    // 2. (SSO websocket removed — replaced with direct API key extraction below)
    // ---------------------------------------------------------------

    // ---------------------------------------------------------------
    // 3. Open a visible WebView pointing at the Nexus registration page
    // ---------------------------------------------------------------
    emit_log(app_handle, "browser", "Opening Nexus registration page…", false);
    if let Some(old) = app_handle.get_webview_window("nexus_auto") {
        let _ = old.close();
    }
    let window = WebviewWindowBuilder::new(
        app_handle,
        "nexus_auto",
        WebviewUrl::External("https://users.nexusmods.com/register".parse().unwrap()),
    )
    .visible(true)
    .title("Creating Nexus Mods account…")
    .build()
    .map_err(|e| AppError::msg(format!("cannot create WebView: {e}")))?;

    // Wait for the page to load its DOM
    emit_log(app_handle, "browser", "Waiting for page to load…", false);
    tokio::time::sleep(Duration::from_secs(4)).await;

    // ---------------------------------------------------------------
    // 4. Fill the email field and wait for Turnstile, then submit
    // ---------------------------------------------------------------
    emit_log(app_handle, "register", "Filling email and waiting for Turnstile…", false);

    let register_js = format!(
        "{NATIVE_VALUE_SETTER}",
    ) + &r#"(function(){
console.log('scav-autoreg: script running');
var e=document.querySelector('input[type="email"],input[name="email"]');
if(e){
__sv(e,'__EMAIL__');
console.log('scav-autoreg: email filled');
}else{
console.log('scav-autoreg: no email input found');
}
var a=0;
function chk(){
var t=document.querySelector('[name="cf-turnstile-response"]');
if(t){
console.log('scav-autoreg: turnstile val length='+t.value.length);
if(t.value&&t.value.length>10){
console.log('scav-autoreg: turnstile ok, clicking submit');
// Click the submit button instead of form.submit() to trigger event handlers
var btn=document.querySelector('button[type="submit"],input[type="submit"],button:not([type])');
if(btn){
console.log('scav-autoreg: clicking '+btn.textContent);
btn.click();
}else{
console.log('scav-autoreg: no submit button, trying form.submit()');
var f=document.querySelector('form');
if(f)f.submit();
}
return;
}
}
a++;
if(a<60){
setTimeout(chk,2000);
}else{
console.log('scav-autoreg: turnstile timed out after '+a+' attempts');
}
}
setTimeout(chk,1500);
})()"#.replace("__EMAIL__", &email);
    if let Err(e) = window.eval(&register_js) {
        emit_log(app_handle, "register", &format!("JS eval failed: {e}"), true);
    }

    // ---------------------------------------------------------------
    // 5. Poll until we land on the verify-code page
    // ---------------------------------------------------------------
    emit_log(app_handle, "verify", "Waiting to reach verification page…", false);
    if let Err(e) = wait_for_url_contains(&window, "/register/verify", 120).await {
        emit_log(app_handle, "verify", &format!("{e}"), true);
        return Err(e);
    }
    emit_log(app_handle, "verify", "On verification page!", false);

    // ---------------------------------------------------------------
    // 6. Fetch the 4-digit code from emailnator
    // ---------------------------------------------------------------
    emit_log(app_handle, "email", &format!("Polling {email} for verification code…"), false);
    let code = poll_for_verification_code(app_handle, &email, 120).await?;
    emit_log(app_handle, "email", &format!("Got code: {code}"), false);

    // ---------------------------------------------------------------
    // 7. Fill the code on the verify page and submit.
    //    Nexus uses 4 separate single-digit inputs (code1..code4) + AJAX.
    // ---------------------------------------------------------------
    emit_log(app_handle, "verify", "Filling verification code…", false);
    let code_js = format!("{NATIVE_VALUE_SETTER}") + &r#"(function(){
var digits='__CODE__'.split('');
var allFilled=true;
for(var i=0;i<4;i++){
var inp=document.getElementById('code'+(i+1));
if(inp){
__sv(inp,digits[i]);
inp.dispatchEvent(new Event('keyup',{bubbles:true}));
}else{
allFilled=false;
}
}
console.log('scav-autoreg: code digits filled, allFilled='+allFilled);
// Wait for the form to assemble the hidden "code" field and enable the button
setTimeout(function(){
var btn=document.querySelector('#verify_form input[type="submit"],#verify_form button[type="submit"]');
if(btn&&!btn.disabled){
console.log('scav-autoreg: clicking verify button');
btn.click();
}else if(btn){
console.log('scav-autoreg: btn still disabled, trying later');
setTimeout(function(){btn.click();},2000);
}else{
console.log('scav-autoreg: no verify button found');
}
},1500);
})()"#.replace("__CODE__", &code);
    window.eval(&code_js).ok();

    // Wait a moment and check the URL to detect error pages
    tokio::time::sleep(Duration::from_secs(3)).await;
    let current_url = window.url().map(|u| u.to_string()).unwrap_or_default();
    emit_log(app_handle, "verify", &format!("After code submit, URL: {current_url}"), false);
    if current_url.contains("oops") || current_url.contains("something-went-wrong") {
        emit_log(app_handle, "verify", "Nexus returned an error page after code submission!", true);
        return Err(AppError::msg("Nexus returned an error after submitting the verification code. The code may be wrong or the page expired."));
    }

    // ---------------------------------------------------------------
    // 8. Wait for the "create account" / sign-up step to appear
    // ---------------------------------------------------------------
    emit_log(app_handle, "register", "Waiting for username/password step…", false);
    wait_for_url_contains(&window, "/auth/", 60).await?;
    emit_log(app_handle, "register", "On username/password page!", false);

    // ---------------------------------------------------------------
    // 9. Fill username, password, confirm, check TOS, and submit.
    //    Nexus requires: 12+ chars, upper+lower+digit, TOS checkbox checked.
    // ---------------------------------------------------------------
    emit_log(app_handle, "register", "Filling credentials…", false);

    // Give the page a moment to settle after Turbolinks navigation
    tokio::time::sleep(Duration::from_secs(2)).await;

    let cred_js = format!("{NATIVE_VALUE_SETTER}") + &r#"(function(){
console.log('scav-autoreg: cred script running, URL='+window.location.href);
// Try several selectors for the username field
var un=document.getElementById('user_name')
      ||document.querySelector('[name="user[name]"]')
      ||document.querySelector('#new_user input[autocomplete="name"]');
console.log('scav-autoreg: username field found='+!!un);
if(un)__sv(un,'__USER__');

// Password fields
var pw=document.getElementById('password')
      ||document.querySelector('[name="user[password]"]');
console.log('scav-autoreg: password field found='+!!pw);
if(pw)__sv(pw,'__PW__');

var pc=document.getElementById('password_confirmation')
      ||document.querySelector('[name="user[password_confirmation]"]');
console.log('scav-autoreg: confirm pw field found='+!!pc);
if(pc)__sv(pc,'__PW__');

// Check the TOS checkbox (required)
var tos=document.getElementById('tsAndCs');
console.log('scav-autoreg: TOS checkbox found='+!!tos);
if(tos&&!tos.checked){
tos.checked=true;
tos.dispatchEvent(new Event('change',{bubbles:true}));
tos.dispatchEvent(new Event('click',{bubbles:true}));
}

// Wait for submit button to become enabled
function trySubmit(attempts){
var btn=document.getElementById('submitRegistrationFormButton')
     ||document.querySelector('#new_user input[type="submit"]')
     ||document.querySelector('input[type="submit"]')
     ||document.querySelector('button[type="submit"]');
console.log('scav-autoreg: submit btn found='+!!btn+' disabled='+(btn?btn.disabled:'?'));
if(btn&&!btn.disabled){
console.log('scav-autoreg: clicking submit');
btn.click();
}else if(attempts<30){
setTimeout(function(){trySubmit(attempts+1)},800);
}else{
console.log('scav-autoreg: giving up, forced click');
if(btn)btn.click();
}
}
// Trigger input events to make password strength checker run
if(pw){pw.dispatchEvent(new Event('keyup',{bubbles:true}));pw.dispatchEvent(new Event('blur',{bubbles:true}));}
if(pc){pc.dispatchEvent(new Event('keyup',{bubbles:true}));pc.dispatchEvent(new Event('blur',{bubbles:true}));}
setTimeout(function(){trySubmit(0)},1200);
})()"#
        .replace("__USER__", &account.username)
        .replace("__PW__", &account.password);
    window.eval(&cred_js).ok();

    // ---------------------------------------------------------------
    // 10. Wait for the registration to complete
    // ---------------------------------------------------------------
    emit_log(app_handle, "register", "Waiting for registration to complete…", false);
    tokio::time::sleep(Duration::from_secs(4)).await;
    let post_reg_url = window.url().map(|u| u.to_string()).unwrap_or_default();
    emit_log(app_handle, "register", &format!("URL after creds: {post_reg_url}"), false);
    if post_reg_url.contains("oops") || post_reg_url.contains("something-went-wrong") {
        emit_log(app_handle, "register", "Nexus returned an error page after username/password!", true);
        return Err(AppError::msg("Nexus error after setting username/password"));
    }

    // ---------------------------------------------------------------
    // 11. Extract API key directly from the account page.
    // ---------------------------------------------------------------
    emit_log(app_handle, "key", "Navigating to API key page…", false);
    window
        .eval("window.location.href='https://www.nexusmods.com/users/myaccount?tab=api'")
        .ok();
    tokio::time::sleep(Duration::from_secs(4)).await;

    // Try extracting a key that might already be visible
    emit_log(app_handle, "key", "Looking for existing API key…", false);
    let mut api_key = try_extract_api_key(&window).await;
    if api_key.is_some() {
        emit_log(app_handle, "key", "Found existing API key", false);
    } else {
        // Click the last "Request API Key" button
        emit_log(app_handle, "key", "Clicking Request API Key…", false);
        window.eval(r#"
var btns=document.querySelectorAll('button, a');
var keyBtns=[];
for(var i=0;i<btns.length;i++){
var txt=btns[i].textContent.toLowerCase();
if((txt.includes('request')||txt.includes('generate')||txt.includes('create'))&&txt.includes('key'))
keyBtns.push(btns[i]);
}
if(keyBtns.length>0)keyBtns[keyBtns.length-1].click();
"#).ok();

        // Poll for the key to appear
        emit_log(app_handle, "key", "Waiting for API key to appear…", false);
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        loop {
            if std::time::Instant::now() > deadline {
                return Err(AppError::msg("Timed out waiting for API key"));
            }
            if let Some(k) = try_extract_api_key(&window).await {
                api_key = Some(k);
                emit_log(app_handle, "key", "API key extracted!", false);
                break;
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }
    let api_key = api_key.ok_or_else(|| AppError::msg("Failed to extract API key"))?;
    emit_log(app_handle, "key", &format!("API key: {:.8}…", &api_key), false);

    // ---------------------------------------------------------------
    // 12. Close the WebView
    // ---------------------------------------------------------------
    window.close().ok();
    emit_log(app_handle, "done", "WebView closed", false);

    // ---------------------------------------------------------------
    // 13. Save the key and settings. Skip Nexus validation — the key was
    //     just generated on their website so we know it's valid.
    // ---------------------------------------------------------------
    emit_log(app_handle, "done", "Saving API key and settings…", false);
    {
        let mut s = state.settings();
        s.nexus_api_key = Some(api_key.clone());
        s.nexus_user = Some(account.username.clone());
        s.temp_email = Some(account.email.clone());
        s.temp_username = Some(account.username.clone());
        s.temp_password = Some(account.password.clone());
        state.save_settings(s)?;
    }
    // Validate in the background (non-fatal if it fails)
    let (is_premium, nexus_user) = match crate::nexus::validate(state, &api_key).await {
        Ok(r) if r.valid => (r.is_premium, r.name),
        _ => (false, None),
    };
    if is_premium || nexus_user.is_some() {
        let mut s = state.settings();
        s.is_premium = is_premium;
        s.nexus_user = nexus_user;
        state.save_settings(s)?;
    }
    emit_log(
        app_handle,
        "done",
        &format!("Account ready! Signed in as {}", account.username),
        false,
    );
    Ok(ValidateResult {
        valid: true,
        user_id: None,
        name: Some(account.username.clone()),
        is_premium,
        email: Some(account.email.clone()),
    })
}

/// Poll `window.url()` until it contains `fragment` or the deadline elapses.
async fn wait_for_url_contains(
    window: &tauri::WebviewWindow,
    fragment: &str,
    timeout_secs: u64,
) -> AppResult<()> {
    let deadline = std::time::Instant::now() + Duration::from_secs(timeout_secs);
    loop {
        if std::time::Instant::now() > deadline {
            return Err(AppError::msg(format!(
                "timed out waiting for URL containing '{fragment}'"
            )));
        }
        if let Ok(url) = window.url() {
            if url.as_str().contains(fragment) {
                return Ok(());
            }
        }
        tokio::time::sleep(Duration::from_millis(800)).await;
    }
}

/// Poll emailnator until a 4‑digit verification code arrives or time runs out.
async fn poll_for_verification_code(
    app: &AppHandle,
    email: &str,
    timeout_secs: u64,
) -> AppResult<String> {
    let deadline = std::time::Instant::now() + Duration::from_secs(timeout_secs);
    let mut attempts = 0u32;
    loop {
        if std::time::Instant::now() > deadline {
            return Err(AppError::msg("timed out waiting for verification code"));
        }
        attempts += 1;
        match try_poll(email).await {
            Ok(v) if v.code.is_some() => return Ok(v.code.unwrap()),
            Ok(v) => {
                eprintln!("emailnator poll #{attempts}: found={} subject={:?}", v.found, v.subject);
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
            Err(e) => {
                let msg = format!("emailnator poll #{attempts}: {e}");
                eprintln!("{msg}");
                emit_log(app, "email", &msg, true);
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
    }
}

