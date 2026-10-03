//! Key-less translation providers, tried in order:
//! 1. Google's Chrome page-translation endpoint: native batching, one request per capture.
//! 2. Google's `gtx` web endpoint: newline-joined batches.
//! 3. MyMemory: one request per text, small daily quota.

use std::time::Duration;

use serde_json::{json, Value};

use crate::settings::{Provider, Settings};

const GOOGLE_HTML_URL: &str = "https://translate-pa.googleapis.com/v1/translateHtml";
/// Public key shipped in Chromium for its built-in page translation.
const GOOGLE_HTML_KEY: &str = "AIzaSyATBXajvzQLTDHEQbcpq0Ihe0vWDHmO520";
const GOOGLE_FREE_URL: &str = "https://translate.googleapis.com/translate_a/single";
const MYMEMORY_URL: &str = "https://api.mymemory.translated.net/get";
/// Google's free endpoints reject much larger bodies.
const GOOGLE_BATCH_CHARS: usize = 4500;
/// MyMemory limit is 500 bytes per query.
const MYMEMORY_CHUNK_BYTES: usize = 450;
const MYMEMORY_CONCURRENCY: usize = 6;

/// Client for the free endpoints: never follows redirects (Google answers rate
/// limiting with a 302 to a captcha page) and never waits indefinitely.
pub fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(15))
        .build()
        .expect("static reqwest config")
}

/// Translates every text into `target`, preserving order and count.
pub async fn translate_all(client: &reqwest::Client, texts: &[String], target: &str) -> Result<Vec<String>, String> {
    if texts.is_empty() {
        return Ok(Vec::new());
    }
    let mut errors = Vec::new();
    match google_html_batched(client, texts, target).await {
        Ok(out) => return Ok(out),
        Err(e) => errors.push(e),
    }
    match google_batched(client, texts, target).await {
        Ok(out) => return Ok(out),
        Err(e) => errors.push(e),
    }
    eprintln!("google endpoints failed, falling back to MyMemory: {}", errors.join("; "));
    mymemory_all(client, texts, target).await.map_err(|e| {
        errors.push(e);
        errors.join("; ")
    })
}

async fn google_html_batched(client: &reqwest::Client, texts: &[String], target: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::with_capacity(texts.len());
    for batch in batches(texts, GOOGLE_BATCH_CHARS) {
        let escaped: Vec<String> = batch.iter().map(|t| escape_html(t)).collect();
        let response = client
            .post(GOOGLE_HTML_URL)
            .header("Content-Type", "application/json+protobuf")
            .header("X-Goog-API-Key", GOOGLE_HTML_KEY)
            .body(json!([[escaped, "auto", target], "te_lib"]).to_string())
            .send()
            .await
            .map_err(|e| format!("Google (Chrome): {e}"))?;
        if !response.status().is_success() {
            return Err(format!("Google (Chrome): HTTP {}", response.status()));
        }
        let body: Value = response.json().await.map_err(|e| format!("Google (Chrome): {e}"))?;
        let translated = parse_google_html(&body, batch.len()).ok_or("Google (Chrome): unexpected response")?;
        out.extend(translated);
    }
    Ok(out)
}

/// Response shape: `[["dịch 1", "dịch 2", ...], ["en", "ja", ...]]`, HTML-escaped.
fn parse_google_html(body: &Value, expected: usize) -> Option<Vec<String>> {
    let items = body.get(0)?.as_array()?;
    if items.len() != expected {
        return None;
    }
    items.iter().map(|v| v.as_str().map(unescape_html)).collect()
}

fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn unescape_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('&') {
        out.push_str(&rest[..start]);
        rest = &rest[start..];
        let decoded = rest.find(';').filter(|&end| end <= 10).and_then(|end| {
            let c = match &rest[1..end] {
                "amp" => '&',
                "lt" => '<',
                "gt" => '>',
                "quot" => '"',
                "apos" => '\'',
                "nbsp" => '\u{a0}',
                e => {
                    let n = e.strip_prefix("#x").or_else(|| e.strip_prefix("#X")).map_or_else(
                        || e.strip_prefix('#')?.parse().ok(),
                        |hex| u32::from_str_radix(hex, 16).ok(),
                    )?;
                    char::from_u32(n)?
                }
            };
            Some((c, end))
        });
        match decoded {
            Some((c, end)) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Packs texts into newline-separated batches; Google keeps line breaks, so the
/// output splits back 1:1. Falls back to one request per text if it does not.
async fn google_batched(client: &reqwest::Client, texts: &[String], target: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::with_capacity(texts.len());
    for batch in batches(texts, GOOGLE_BATCH_CHARS) {
        let joined = batch.iter().map(|t| t.replace('\n', " ")).collect::<Vec<_>>().join("\n");
        let translated = google(client, &joined, target).await?;
        let parts: Vec<&str> = translated.split('\n').collect();
        if parts.len() == batch.len() {
            out.extend(parts.into_iter().map(|p| p.trim().to_owned()));
        } else {
            for text in batch {
                out.push(google(client, text, target).await?);
            }
        }
    }
    Ok(out)
}

fn batches(texts: &[String], max_chars: usize) -> Vec<&[String]> {
    let mut result = Vec::new();
    let (mut start, mut size) = (0, 0);
    for (i, text) in texts.iter().enumerate() {
        let len = text.chars().count() + 1;
        if i > start && size + len > max_chars {
            result.push(&texts[start..i]);
            (start, size) = (i, 0);
        }
        size += len;
    }
    result.push(&texts[start..]);
    result
}

async fn google(client: &reqwest::Client, text: &str, target: &str) -> Result<String, String> {
    let response = client
        .post(GOOGLE_FREE_URL)
        .query(&[("client", "gtx"), ("sl", "auto"), ("tl", target), ("dt", "t")])
        .form(&[("q", text)])
        .send()
        .await
        .map_err(|e| format!("Google: {e}"))?;
    match response.status() {
        s if s.is_redirection() || s.as_u16() == 429 => return Err(crate::i18n::current("rateLimit").into()),
        s if !s.is_success() => return Err(format!("Google: HTTP {s}")),
        _ => {}
    }
    let body: Value = response.json().await.map_err(|e| format!("Google: {e}"))?;
    parse_google(&body).ok_or_else(|| "Google: unexpected response".into())
}

/// Response shape: `[[["dịch", "source", ...], ...], null, "en", ...]`.
fn parse_google(body: &Value) -> Option<String> {
    body.get(0)?
        .as_array()?
        .iter()
        .map(|seg| seg.get(0).and_then(Value::as_str))
        .collect()
}

async fn mymemory_all(client: &reqwest::Client, texts: &[String], target: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::with_capacity(texts.len());
    for group in texts.chunks(MYMEMORY_CONCURRENCY) {
        let tasks: Vec<_> = group
            .iter()
            .map(|text| {
                let (client, text, target) = (client.clone(), text.clone(), target.to_owned());
                tauri::async_runtime::spawn(async move {
                    let mut parts = Vec::new();
                    for chunk in chunks(&text, MYMEMORY_CHUNK_BYTES) {
                        parts.push(mymemory(&client, chunk, &target).await?);
                    }
                    Ok::<_, String>(parts.join(" "))
                })
            })
            .collect();
        for task in tasks {
            out.push(task.await.map_err(|e| e.to_string())??);
        }
    }
    Ok(out)
}

/// Splits on whitespace so each piece stays within `max_bytes` (single long words
/// are kept whole).
fn chunks(text: &str, max_bytes: usize) -> Vec<&str> {
    let mut result = Vec::new();
    let mut rest = text.trim();
    while rest.len() > max_bytes {
        let mut cut = max_bytes;
        while !rest.is_char_boundary(cut) {
            cut -= 1;
        }
        let cut = rest[..cut].rfind(char::is_whitespace).filter(|&i| i > 0).unwrap_or(cut);
        result.push(rest[..cut].trim_end());
        rest = rest[cut..].trim_start();
    }
    if !rest.is_empty() {
        result.push(rest);
    }
    result
}

async fn mymemory(client: &reqwest::Client, text: &str, target: &str) -> Result<String, String> {
    let langpair = format!("autodetect|{target}");
    let body: Value = client
        .get(MYMEMORY_URL)
        .query(&[("q", text), ("langpair", langpair.as_str())])
        .send()
        .await
        .map_err(|e| format!("MyMemory: {e}"))?
        .json()
        .await
        .map_err(|e| format!("MyMemory: {e}"))?;
    let details = body["responseDetails"].as_str().unwrap_or("request failed");
    // Text already in the target language.
    if details.contains("DISTINCT LANGUAGES") {
        return Ok(text.to_owned());
    }
    let status = &body["responseStatus"];
    let ok = status.as_i64().or_else(|| status.as_str()?.parse().ok()) == Some(200);
    if !ok || body["quotaFinished"].as_bool() == Some(true) {
        return Err(format!("MyMemory: {details}"));
    }
    body["responseData"]["translatedText"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| "MyMemory: unexpected response".into())
}


/// Smaller batch (max 1500 chars or 10 items) prevents LLMs like Gemini from
/// hitting token truncation or generating malformed unescaped JSON.
const LLM_BATCH_CHARS: usize = 1500;
const LLM_MAX_ITEMS_PER_BATCH: usize = 10;
const LLM_TIMEOUT: Duration = Duration::from_secs(90);

/// Dispatches on the configured provider. A failing user-chosen provider is an error:
/// silently re-sending the text to a different third party would be a privacy surprise.
pub async fn translate(
    client: &reqwest::Client,
    settings: &Settings,
    api_key: Option<&str>,
    texts: &[String],
) -> Result<Vec<String>, String> {
    match settings.provider {
        Provider::Free => translate_all(client, texts, &settings.target_lang).await,
        Provider::Openai => openai_batched(client, settings, api_key, texts).await,
    }
}

async fn openai_batched(
    client: &reqwest::Client,
    settings: &Settings,
    api_key: Option<&str>,
    texts: &[String],
) -> Result<Vec<String>, String> {
    let mut out = Vec::with_capacity(texts.len());
    for batch in split_llm_batches(texts, LLM_BATCH_CHARS, LLM_MAX_ITEMS_PER_BATCH) {
        let reply = chat(client, settings, api_key, batch).await?;
        out.extend(parse_json_array(&reply, batch.len())?);
    }
    Ok(out)
}

fn split_llm_batches(texts: &[String], max_chars: usize, max_items: usize) -> Vec<&[String]> {
    let mut result = Vec::new();
    let (mut start, mut size) = (0, 0);
    for (i, text) in texts.iter().enumerate() {
        let len = text.chars().count() + 1;
        if i > start && (size + len > max_chars || i - start >= max_items) {
            result.push(&texts[start..i]);
            (start, size) = (i, 0);
        }
        size += len;
    }
    if start < texts.len() {
        result.push(&texts[start..]);
    }
    result
}

fn system_prompt(language: &str) -> String {
    format!(
        "You are an accurate translator. You receive a JSON array of strings extracted from a screen capture. \
         Translate every string into {language}. \
         CRITICAL RULES: \
         1. Keep numbers, technical terms, URLs, file paths, and code identifiers unchanged. \
         2. Your response MUST be ONLY a valid JSON array of strings with EXACTLY the same number of items. \
         3. Do NOT output markdown code fences, notes, or explanations. \
         4. Escape any inner quotes with backslashes so the output is strictly valid JSON."
    )
}

async fn chat(
    client: &reqwest::Client,
    settings: &Settings,
    api_key: Option<&str>,
    batch: &[String],
) -> Result<String, String> {
    let url = format!("{}/chat/completions", settings.base_url);
    let mut request = client.post(&url).timeout(LLM_TIMEOUT).json(&json!({
        "model": settings.model,
        "temperature": 0,
        "messages": [
            { "role": "system", "content": system_prompt(settings.language_name()) },
            { "role": "user", "content": serde_json::to_string(batch).map_err(|e| e.to_string())? },
        ],
    }));
    if let Some(key) = api_key.filter(|k| !k.is_empty()) {
        request = request.bearer_auth(key);
    }
    let response = request.send().await.map_err(|e| format!("{}: {e}", settings.base_url))?;
    let status = response.status();
    let text = response.text().await.map_err(|e| format!("{}: {e}", settings.base_url))?;
    if !status.is_success() {
        let hint = if status.as_u16() == 404 { crate::i18n::current("modelHint") } else { "" };
        return Err(format!("HTTP {status}: {}{hint}", error_detail(&text)));
    }
    let body: Value = serde_json::from_str(&text).map_err(|_| format!("{}: not an OpenAI-style JSON response", settings.base_url))?;
    body["choices"][0]["message"]["content"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| "response has no message content".into())
}

/// Providers disagree on error shape: `{"error":{"message"}}`, `{"error":"…"}` or `[{"error":…}]`
/// (Gemini); anything else (an HTML proxy page) is shown truncated.
fn error_detail(body: &str) -> String {
    let json: Option<Value> = serde_json::from_str(body).ok();
    let err = json.as_ref().map(|v| v.get(0).unwrap_or(v)).map(|v| &v["error"]);
    err.and_then(|e| e["message"].as_str().or_else(|| e.as_str()))
        .map(str::to_owned)
        .unwrap_or_else(|| body.chars().take(200).collect::<String>().trim().to_owned())
}

/// Sends one tiny real request so a wrong URL, model or key is caught when saving
/// instead of on the first capture. No-op for key-less providers.
pub async fn verify(client: &reqwest::Client, settings: &Settings, api_key: Option<&str>) -> Result<(), String> {
    if settings.provider != Provider::Openai {
        return Ok(());
    }
    let probe = ["Hello".to_owned()];
    let reply = chat(client, settings, api_key, &probe).await?;
    parse_json_array(&reply, 1).map(|_| ())
}

/// Extracts the JSON string array from a model reply, tolerating code fences and
/// surrounding prose, and checks the count so results can never shift between blocks.
fn parse_json_array(reply: &str, expected: usize) -> Result<Vec<String>, String> {
    let trimmed = reply.trim();
    // Find outermost `[` and `]`
    let (start, end) = trimmed
        .find('[')
        .zip(trimmed.rfind(']'))
        .filter(|(s, e)| s < e)
        .ok_or_else(|| format!("model did not return a JSON array: {}", trimmed.chars().take(200).collect::<String>()))?;

    let raw_json = &trimmed[start..=end];
    // Attempt 1: Standard JSON parse
    if let Ok(items) = serde_json::from_str::<Vec<Value>>(raw_json) {
        if items.len() == expected {
            return items
                .into_iter()
                .map(|v| v.as_str().map(str::to_owned).ok_or_else(|| "model returned a non-string item".to_string()))
                .collect();
        }
    }

    // Attempt 2: Lenient extraction if raw parse failed (e.g. unescaped newlines or quotes inside strings)
    // Parse line-by-line / regex or fallback to cleaned json
    let cleaned = sanitize_json_array(raw_json);
    let items: Vec<Value> = serde_json::from_str(&cleaned).map_err(|e| {
        format!("model returned invalid JSON ({e}): {}", raw_json.chars().take(300).collect::<String>())
    })?;

    if items.len() != expected {
        return Err(format!("model returned {} items for {expected} texts", items.len()));
    }
    items
        .into_iter()
        .map(|v| v.as_str().map(str::to_owned).ok_or_else(|| "model returned a non-string item".to_string()))
        .collect()
}

/// Sanitizes common LLM JSON glitches like literal unescaped newlines or trailing commas
fn sanitize_json_array(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut in_str = false;
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                out.push(c);
                if let Some(next) = chars.next() {
                    out.push(next);
                }
            }
            '"' => {
                in_str = !in_str;
                out.push(c);
            }
            '\n' | '\r' if in_str => {
                // Replace literal unescaped newline inside JSON string with space
                out.push(' ');
            }
            ',' => {
                // Look ahead: if followed only by whitespace and `]`, drop trailing comma
                let mut rest = chars.clone();
                for nc in rest.by_ref() {
                    if nc == ']' {
                        // skip comma
                        break;
                    } else if !nc.is_whitespace() {
                        out.push(',');
                        break;
                    }
                }
            }
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joins_google_segments() {
        let body = serde_json::json!([[["Xin chào. ", "Hello. ", null], ["Thế giới", "World", null]], null, "en"]);
        assert_eq!(parse_google(&body).as_deref(), Some("Xin chào. Thế giới"));
        assert_eq!(parse_google(&serde_json::json!({"error": 1})), None);
    }

    #[test]
    fn chrome_endpoint_response_is_unescaped_and_count_checked() {
        let body = json!([["Nếu a &lt; b và c &gt; d thì &quot;ok&quot; &#39;x&#x27; &amp;", "\u{3c}b\u{3e}Hồ sơ"], ["en", "en"]]);
        assert_eq!(
            parse_google_html(&body, 2).unwrap(),
            ["Nếu a < b và c > d thì \"ok\" 'x' &", "<b>Hồ sơ"]
        );
        assert_eq!(parse_google_html(&body, 3), None);
    }

    #[test]
    fn html_escape_round_trips_and_leaves_bare_ampersands() {
        let text = "a < b && c > d; AT&T &copy";
        assert_eq!(unescape_html(&escape_html(text)), text);
        assert_eq!(unescape_html("R&D; x & y"), "R&D; x & y");
    }

    #[test]
    fn batches_respect_char_budget_and_keep_every_text() {
        let texts: Vec<String> = ["aaaa", "bbbb", "cc", "dddddddd", "e"].map(String::from).into();
        let b = batches(&texts, 10);
        assert_eq!(b.iter().map(|s| s.len()).collect::<Vec<_>>(), [2, 1, 1, 1]);
        assert_eq!(b.concat(), texts);
        // A single oversized text still forms its own batch.
        assert_eq!(batches(&texts[3..4], 3).len(), 1);
    }

    #[test]
    fn chunks_split_on_whitespace_within_byte_limit() {
        assert_eq!(chunks("xin chào thế giới", 12), ["xin chào", "thế giới"]);
        assert_eq!(chunks("short", 10), ["short"]);
        assert_eq!(chunks("abcdefghijkl", 5), ["abcde", "fghij", "kl"]);
    }

    #[test]
    fn llm_reply_parsing_tolerates_fences_and_rejects_count_or_type_mismatch() {
        let fenced = "Here you go:\n```json\n[\"xin chào\", \"[tag] 1\"]\n```";
        assert_eq!(parse_json_array(fenced, 2).unwrap(), ["xin chào", "[tag] 1"]);
        assert!(parse_json_array("[\"a\"]", 2).unwrap_err().contains("1 items for 2"));
        assert!(parse_json_array("[1, 2]", 2).unwrap_err().contains("non-string"));
        assert!(parse_json_array("sorry, I can't", 1).is_err());
    }
}
