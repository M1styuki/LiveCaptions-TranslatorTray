//! Fixed-revision wire protocols. One engine owns this client and its in-memory
//! OpenAI template cache; authorization belongs to each individual request.
use crate::{TranslateError, compatibility, http, language, signature};
use captions_core::{
    settings::{SERVICES, Settings, render_prompt},
    text::Context,
};
use reqwest::{Client, RequestBuilder, Url};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::HashMap, sync::Mutex, time::Duration};

pub fn is_llm(service: &str) -> bool {
    matches!(service, "OpenAI" | "OpenRouter" | "Ollama" | "LMStudio")
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Model {
    pub id: String,
    pub display_name: String,
}

pub struct Translator {
    client: Client,
    templates: Mutex<HashMap<[u8; 32], usize>>,
    #[cfg(any(test, feature = "test-support"))]
    endpoints: HashMap<&'static str, String>,
    #[cfg(test)]
    fixed_salt: Option<String>,
    #[cfg(test)]
    timeout: Duration,
}
impl Translator {
    pub fn new() -> Result<Self, TranslateError> {
        Ok(Self {
            client: http::client()?,
            templates: Mutex::default(),
            #[cfg(any(test, feature = "test-support"))]
            endpoints: HashMap::new(),
            #[cfg(test)]
            fixed_salt: None,
            #[cfg(test)]
            timeout: Duration::from_secs(8),
        })
    }
    #[cfg(any(test, feature = "test-support"))]
    pub fn local_fixture(service: &str, endpoint: String) -> Self {
        let url = Url::parse(&endpoint).expect("fixture URL");
        assert_eq!(url.scheme(), "http");
        assert_eq!(url.host_str(), Some("127.0.0.1"));
        let service = service_name(service).expect("fixture service");
        Self {
            client: Client::builder()
                .no_proxy()
                .timeout(Duration::from_secs(8))
                .build()
                .unwrap(),
            templates: Mutex::default(),
            endpoints: HashMap::from([(service, endpoint)]),
            #[cfg(test)]
            fixed_salt: Some("fixture-salt-001".into()),
            #[cfg(test)]
            timeout: Duration::from_secs(8),
        }
    }
    fn timeout(&self) -> Duration {
        #[cfg(test)]
        {
            self.timeout
        }
        #[cfg(not(test))]
        {
            Duration::from_secs(8)
        }
    }
    fn url(
        &self,
        service: &'static str,
        configured: &str,
        suffix: &str,
    ) -> Result<Url, TranslateError> {
        #[cfg(any(test, feature = "test-support"))]
        let configured = self
            .endpoints
            .get(service)
            .map_or(configured, String::as_str);
        let mut url =
            Url::parse(configured.trim()).map_err(|_| error(service, "服务地址格式不正确"))?;
        if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
            return Err(error(service, "服务地址必须以 http:// 或 https:// 开头"));
        }
        if !suffix.is_empty() {
            let path = format!("{}{suffix}", url.path().trim_end_matches('/'));
            url.set_path(&path);
        }
        Ok(url)
    }
    fn salt(&self, service: &'static str) -> Result<String, TranslateError> {
        #[cfg(test)]
        if let Some(salt) = &self.fixed_salt {
            return Ok(salt.clone());
        }
        signature::salt(service)
    }
    /// Returns the adapter's raw translated body. Marker selection and unified
    /// thinking/notice cleanup occur afterwards in the cancellable worker job.
    pub async fn translate(
        &self,
        settings: &Settings,
        text: &str,
        contexts: &[Context],
    ) -> Result<String, TranslateError> {
        self.translate_inner(settings, text, contexts)
            .await
            .map_err(|mut error| {
                if let Some(code) = &error.code {
                    let config = settings.active_config();
                    let echoes_credential = ["ApiKey", "AppSecret", "AppKey", "AppId"]
                        .iter()
                        .filter_map(|field| config.get(*field).and_then(Value::as_str))
                        .any(|secret| !secret.is_empty() && code.contains(secret));
                    if echoes_credential
                        || (settings.api_name() == "Google2" && code.contains(self.google2_key()))
                    {
                        error.code = None;
                    }
                }
                error
            })
    }
    fn google2_key(&self) -> &'static str {
        #[cfg(any(test, feature = "test-support"))]
        if self.endpoints.contains_key("Google2") {
            return "fixture-google2-key";
        }
        compatibility::GOOGLE2_KEY
    }
    async fn translate_inner(
        &self,
        settings: &Settings,
        text: &str,
        contexts: &[Context],
    ) -> Result<String, TranslateError> {
        let service =
            service_name(settings.api_name()).ok_or_else(|| error("翻译", "未知翻译接口"))?;
        let config = settings.active_config();
        let field = |name: &str| config.get(name).and_then(Value::as_str).unwrap_or("");
        let target = language::target(service, settings.target_language());
        if service == "Google" {
            let endpoint = self.url(service, "https://clients5.google.com/translate_a/t", "")?;
            let request = self.client.get(endpoint).query(&[
                ("client", "dict-chrome-ex"),
                ("sl", "auto"),
                ("tl", target),
                ("q", text),
            ]);
            let (status, value) = self.send(service, request).await?;
            check_status(service, status, &value)?;
            return parse_translation(service, status, &value);
        }
        if is_llm(service) {
            return self.llm(settings, text, contexts).await;
        }
        let request = match service {
            "Google2" => {
                let key = self.google2_key();
                self.client.get(self.url(service, "https://dictionaryextension-pa.googleapis.com/v1/dictionaryExtensionData", "")?)
                    .header("x-referer", "chrome-extension://mgijmajocgfcbeboacabfgobmjgjcoja")
                    .query(&[("language", target), ("key", key), ("term", text), ("strategy", "2")])
            }
            "DeepL" => self.client.post(self.url(service, field("ApiUrl"), "")?)
                .header("Authorization", format!("DeepL-Auth-Key {}", field("ApiKey")))
                .json(&json!({"text":[text], "target_lang":target})),
            "MTranServer" => self.client.post(self.url(service, field("ApiUrl"), "")?)
                .bearer_auth(field("ApiKey"))
                .json(&json!({"text":text, "to":target, "from":field("SourceLanguage")})),
            "LibreTranslate" => self.client.post(self.url(service, field("ApiUrl"), "")?)
                .json(&json!({"q":text, "target":target, "source":"auto", "format":"text", "api_key":field("ApiKey")})),
            "Youdao" | "Baidu" => {
                let salt = self.salt(service)?;
                let (key_field, form_field) = if service == "Youdao" { ("AppKey", "appKey") } else { ("AppId", "appid") };
                let sign = signature::md5(service, &format!("{}{text}{salt}{}", field(key_field), field("AppSecret")))?;
                self.client.post(self.url(service, field("ApiUrl"), "")?)
                    .form(&[("q", text), ("from", "auto"), ("to", target), (form_field, field(key_field)), ("salt", salt.as_str()), ("sign", sign.as_str())])
            }
            _ => unreachable!("supported services are exhaustive"),
        };
        let (status, value) = self.send(service, request).await?;
        check_status(service, status, &value)?;
        parse_translation(service, status, &value)
    }
    async fn send(
        &self,
        service: &'static str,
        request: RequestBuilder,
    ) -> Result<(u16, Value), TranslateError> {
        let response = request
            .timeout(self.timeout())
            .send()
            .await
            .map_err(|e| http::network_error(service, e))?;
        read_response(service, response).await
    }
    async fn llm(
        &self,
        settings: &Settings,
        text: &str,
        contexts: &[Context],
    ) -> Result<String, TranslateError> {
        let service = service_name(settings.api_name()).unwrap();
        let config = settings.active_config();
        let field = |name: &str| config.get(name).and_then(Value::as_str).unwrap_or("");
        let (base, suffix) = match service {
            "OpenRouter" => ("https://openrouter.ai/api/v1/chat/completions", ""),
            "Ollama" => (field("ApiUrl"), "/api/chat"),
            "LMStudio" => (field("ApiUrl"), "/chat"),
            _ => (field("ApiUrl"), ""),
        };
        let url = self.url(service, base, suffix)?;
        let prompt = render_prompt(
            settings.json()["Prompt"].as_str().unwrap(),
            settings.target_language(),
        )
        .map_err(|_| error(service, "提示词格式不正确"))?;
        let aware = settings.json()["ContextAware"].as_bool().unwrap();
        let contexts = if aware { contexts } else { &[] };
        let mut messages = vec![json!({"role":"system", "content":prompt})];
        for context in contexts {
            messages.push(json!({"role":"user", "content":format!("🔤 {} 🔤", context.source)}));
            messages.push(json!({"role":"assistant", "content":context.translation}));
        }
        messages.push(json!({"role":"user", "content":format!("🔤 {text} 🔤")}));
        let mut common = json!({"model":field("ModelName"), "messages":messages, "temperature":config["Temperature"], "max_tokens":128, "stream":false, "keep_alive":600});
        if matches!(service, "OpenAI" | "OpenRouter") {
            common["max_tokens"] = json!(512);
        }
        if service == "LMStudio" {
            let mut input: Vec<_> = contexts
                .iter()
                .map(|context| format!("🔤 {} 🔤 → {}", context.source, context.translation))
                .collect();
            input.push(format!("🔤 {text} 🔤"));
            common = json!({"model":field("ModelName"), "system_prompt":prompt, "input":input.join("\n"), "temperature":config["Temperature"]});
        } else if service == "Ollama" {
            common["keep_alive"] = config["Keep_alive"].clone();
            common["think"] = json!(false);
        } else if service == "OpenRouter" {
            common["reasoning"] = json!({"exclude":true,"enabled":false});
        }
        if service == "OpenAI" {
            let mut digest = Sha256::new();
            for part in [
                service.as_bytes(),
                url.as_str().as_bytes(),
                serde_json::to_vec(config).unwrap().as_slice(),
                settings.json()["ConfigIndices"][service]
                    .to_string()
                    .as_bytes(),
            ] {
                digest.update(part);
                digest.update([0]);
            }
            let key: [u8; 32] = digest.finalize().into();
            return tokio::time::timeout(
                Duration::from_secs(64),
                self.openai(url, field("ApiKey"), common, key),
            )
            .await
            .map_err(|_| error(service, "兼容请求总时长超过64秒"))?;
        }
        let mut request = self.client.post(url).json(&common);
        if service == "OpenRouter" {
            request = request.bearer_auth(field("ApiKey"));
        }
        let (status, value) = self.send(service, request).await?;
        check_status(service, status, &value)?;
        parse_translation(service, status, &value)
    }
    async fn openai(
        &self,
        url: Url,
        api_key: &str,
        common: Value,
        key: [u8; 32],
    ) -> Result<String, TranslateError> {
        let cached = self.templates.lock().unwrap().get(&key).copied();
        let mut order: Vec<usize> = cached.into_iter().collect();
        order.extend((0..8).filter(|index| Some(*index) != cached));
        let mut last = error("OpenAI", "服务不接受兼容参数");
        for index in order {
            let mut body = common.clone();
            add_template(&mut body, index);
            let (status, value) = self
                .send(
                    "OpenAI",
                    self.client
                        .post(url.clone())
                        .bearer_auth(api_key)
                        .json(&body),
                )
                .await?;
            if matches!(status, 400 | 422) {
                last = response_error("OpenAI", status, &value);
                continue;
            }
            check_status("OpenAI", status, &value)?;
            let result = parse_translation("OpenAI", status, &value)?;
            self.templates.lock().unwrap().insert(key, index);
            return Ok(result);
        }
        self.templates.lock().unwrap().remove(&key);
        Err(last)
    }
    pub async fn models(&self, service: &str, base: &str) -> Result<Vec<Model>, TranslateError> {
        let service = service_name(service).ok_or_else(|| error("模型列表", "未知接口"))?;
        let suffix = match service {
            "LMStudio" => "/models",
            "Ollama" => "/api/tags",
            _ => return Err(error(service, "此接口没有模型列表入口")),
        };
        let url = self.url(service, base, suffix)?;
        let response = self
            .client
            .get(url)
            .timeout(Duration::from_secs(10))
            .send()
            .await
            .map_err(|e| http::network_error(service, e))?;
        let (status, value) = read_response(service, response).await?;
        check_status(service, status, &value)?;
        let rows = value["models"]
            .as_array()
            .ok_or_else(|| error(service, "模型列表响应格式不正确"))?;
        Ok(rows
            .iter()
            .filter_map(|row| {
                let id = if service == "LMStudio" {
                    if row["type"].as_str() != Some("llm") {
                        return None;
                    }
                    row["key"].as_str()?
                } else {
                    row["name"].as_str()?
                };
                if id.trim().is_empty() {
                    return None;
                }
                Some(Model {
                    id: id.into(),
                    display_name: row["display_name"]
                        .as_str()
                        .filter(|name| !name.is_empty())
                        .unwrap_or(id)
                        .into(),
                })
            })
            .collect())
    }
}
fn service_name(name: &str) -> Option<&'static str> {
    SERVICES.into_iter().find(|service| *service == name)
}
fn error(service: &'static str, reason: &'static str) -> TranslateError {
    TranslateError {
        service,
        status: None,
        code: None,
        reason,
    }
}
fn safe_code(value: &Value) -> Option<String> {
    let code = match value {
        Value::String(code) => code.clone(),
        Value::Number(code) => code.to_string(),
        _ => return None,
    };
    (!code.is_empty()
        && code.len() <= 40
        && code
            .bytes()
            .all(|ch| ch.is_ascii_alphanumeric() || b"_-.:".contains(&ch)))
    .then_some(code)
}
fn response_error(service: &'static str, status: u16, value: &Value) -> TranslateError {
    let code = value
        .get("errorCode")
        .or_else(|| value.get("error_code"))
        .or_else(|| value.pointer("/error/code"))
        .and_then(safe_code);
    TranslateError {
        service,
        status: Some(status),
        code,
        reason: match status {
            401 | 403 => "认证失败，请检查接口凭据",
            429 => "请求过于频繁，请稍后重试",
            500..=599 => "服务暂时不可用",
            _ => "服务返回错误",
        },
    }
}
fn check_status(service: &'static str, status: u16, value: &Value) -> Result<(), TranslateError> {
    if !(200..300).contains(&status) {
        return Err(response_error(service, status, value));
    }
    let code = value.get("errorCode").or_else(|| value.get("error_code"));
    if code.is_some_and(|code| {
        code.as_str()
            .map_or_else(|| code.as_i64() != Some(0), |code| code != "0")
    }) || value.get("error").is_some_and(|error| !error.is_null())
    {
        return Err(response_error(service, status, value));
    }
    if service == "Youdao" && code.is_none() {
        return Err(error(service, "响应缺少服务状态码"));
    }
    Ok(())
}
async fn read_response(
    service: &'static str,
    response: reqwest::Response,
) -> Result<(u16, Value), TranslateError> {
    let status = response.status().as_u16();
    let bytes = response
        .bytes()
        .await
        .map_err(|e| http::network_error(service, e))?;
    let value = match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        Err(_) if !(200..300).contains(&status) => Value::Null,
        Err(_) => return Err(error(service, "响应不是有效JSON")),
    };
    Ok((status, value))
}
fn parse_translation(
    service: &'static str,
    status: u16,
    value: &Value,
) -> Result<String, TranslateError> {
    if matches!(service, "OpenAI" | "OpenRouter")
        && value
            .pointer("/choices/0/finish_reason")
            .and_then(Value::as_str)
            == Some("length")
    {
        return Err(TranslateError {
            service,
            status: Some(status),
            code: None,
            reason: "译文达到输出长度上限，结果不完整",
        });
    }
    let content = match service {
        "Google" => value.pointer("/0/0"),
        "Google2" => value.pointer("/translateResponse/translateText"),
        "DeepL" => value.pointer("/translations/0/text"),
        "Youdao" => value.pointer("/translation/0"),
        "Baidu" => value.pointer("/trans_result/0/dst"),
        "MTranServer" => value.get("result"),
        "LibreTranslate" => value.get("translatedText"),
        "OpenAI" | "OpenRouter" => value.pointer("/choices/0/message/content"),
        "Ollama" => value.pointer("/message/content"),
        "LMStudio" => value["output"]
            .as_array()
            .and_then(|rows| {
                rows.iter()
                    .find(|row| row["type"].as_str() == Some("message"))
            })
            .and_then(|row| row.get("content")),
        _ => None,
    };
    content
        .and_then(Value::as_str)
        .filter(|text| !text.trim().is_empty())
        .map(str::to_owned)
        .ok_or(TranslateError {
            service,
            status: Some(status),
            code: None,
            reason: "响应缺少非空翻译结果",
        })
}
fn add_template(value: &mut Value, index: usize) {
    match index {
        0 => {
            value["think"] = json!(false);
            value["enable_thinking"] = json!(false);
            value["reasoning_effort"] = json!("low");
            value["reasoning"] = json!({"exclude":true,"enabled":false,"effort":"low"});
            value["thinking"] = json!({"type":"disabled"});
        }
        1 => value["enable_thinking"] = json!(false),
        2 => value["thinking"] = json!({"type":"disabled"}),
        3 => value["think"] = json!(false),
        4 => value["reasoning"] = json!({"exclude":true,"enabled":false}),
        5 => value["reasoning"] = json!({"effort":"low"}),
        6 => value["reasoning_effort"] = json!("low"),
        7 => {}
        _ => unreachable!(),
    }
}

#[cfg(test)]
mod tests;
