use super::*;
use captions_core::settings::Screen;
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::Instant,
};

const SOURCE: &str = "Public 世界? a=1&b=two / 😀";
struct Captured {
    method: String,
    url: Url,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}
enum Reply {
    Json(u16, String),
    SlowBody(Duration),
    WaitForDisconnect,
}
struct Server {
    url: String,
    requests: Arc<Mutex<Vec<Captured>>>,
    stop: Arc<AtomicBool>,
    disconnected: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}
impl Server {
    fn start(replies: Vec<Reply>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let capture = requests.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let shutdown = stop.clone();
        let disconnected = Arc::new(AtomicBool::new(false));
        let disconnect = disconnected.clone();
        let join = thread::spawn(move || {
            for reply in replies {
                let deadline = Instant::now() + Duration::from_secs(5);
                let mut stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            if shutdown.load(Ordering::Acquire) || Instant::now() >= deadline {
                                return;
                            }
                            thread::sleep(Duration::from_millis(1));
                        }
                        Err(e) => panic!("fixture accept: {e}"),
                    }
                };
                // On Windows accepted sockets can inherit the listener's
                // nonblocking mode. Only the accept loop should be nonblocking.
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                capture.lock().unwrap().push(read_request(&mut stream));
                match reply {
                    Reply::Json(status, body) => {
                        let _ = write!(
                            stream,
                            "HTTP/1.1 {status} Fixture\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{body}",
                            body.len()
                        );
                    }
                    Reply::SlowBody(delay) => {
                        let _ = write!(
                            stream,
                            "HTTP/1.1 200 OK\r\nContent-Length: 200\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n"
                        );
                        let _ = stream.flush();
                        thread::sleep(delay);
                    }
                    Reply::WaitForDisconnect => {
                        let mut byte = [0];
                        match stream.read(&mut byte) {
                            Ok(0) => disconnect.store(true, Ordering::Release),
                            Err(e)
                                if matches!(
                                    e.kind(),
                                    std::io::ErrorKind::ConnectionReset
                                        | std::io::ErrorKind::ConnectionAborted
                                ) =>
                            {
                                disconnect.store(true, Ordering::Release)
                            }
                            _ => {}
                        }
                    }
                }
            }
        });
        Self {
            url,
            requests,
            stop,
            disconnected,
            join: Some(join),
        }
    }
    fn client(&self, service: &str) -> Translator {
        Translator::local_fixture(service, format!("{}/wire?keep=a/b", self.url))
    }
    fn count(&self) -> usize {
        self.requests.lock().unwrap().len()
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(join) = self.join.take()
            && let Err(error) = join.join()
            && !thread::panicking()
        {
            std::panic::resume_unwind(error);
        }
    }
}
fn read_request(stream: &mut TcpStream) -> Captured {
    let mut bytes = Vec::new();
    while !bytes.ends_with(b"\r\n\r\n") {
        let mut byte = [0];
        stream.read_exact(&mut byte).unwrap();
        bytes.push(byte[0]);
        assert!(bytes.len() < 65536);
    }
    let header = String::from_utf8(bytes).unwrap();
    let mut lines = header.lines();
    let mut first = lines.next().unwrap().split_whitespace();
    let method = first.next().unwrap().into();
    let url = Url::parse(&format!("http://localhost{}", first.next().unwrap())).unwrap();
    let headers: HashMap<_, _> = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(key, value)| (key.to_ascii_lowercase(), value.trim().to_owned()))
        .collect();
    let mut body = vec![
        0;
        headers
            .get("content-length")
            .map_or(0, |value| value.parse().unwrap())
    ];
    stream.read_exact(&mut body).unwrap();
    Captured {
        method,
        url,
        headers,
        body,
    }
}
fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}
fn settings(service: &str) -> Settings {
    let screen = Screen::new(1920.0, 1080.0).unwrap();
    Settings::defaults(screen)
        .edited(screen, |value| {
            value["ApiName"] = json!(service);
            value["TargetLanguage"] = json!("zh-TW");
            value["Prompt"] = json!("Translate to {0}. {{Exact}}");
            value["ContextAware"] = json!(true);
            let config = &mut value["Configs"][service][0];
            if is_llm(service) {
                config["ModelName"] = json!("fixture-model");
                config["Temperature"] = json!(0.7);
            }
            for name in ["ApiKey", "AppKey", "AppId"] {
                if config.get(name).is_some() {
                    config[name] = json!("fixture-id");
                }
            }
            if config.get("AppSecret").is_some() {
                config["AppSecret"] = json!("fixture-secret");
            }
            if config.get("Keep_alive").is_some() {
                config["Keep_alive"] = json!(42);
            }
        })
        .unwrap()
        .0
}
fn success(service: &str) -> String {
    match service {
        "Google"=>json!([["你好"]]),
        "Google2"=>json!({"translateResponse":{"translateText":"你好"}}),
        "DeepL"=>json!({"translations":[{"text":"你好"}]}),
        "Youdao"=>json!({"errorCode":"0","translation":["你好"]}),
        "Baidu"=>json!({"trans_result":[{"dst":"你好"}]}),
        "MTranServer"=>json!({"result":"你好"}),
        "LibreTranslate"=>json!({"translatedText":"你好"}),
        "OpenAI" | "OpenRouter"=>json!({"choices":[{"message":{"content":"你好"}}]}),
        "Ollama"=>json!({"message":{"content":"你好"}}),
        "LMStudio"=>json!({"output":[{"type":"reasoning","content":"ignore"},{"type":"message","content":"你好"},{"type":"message","content":"later"}]}),
        _=>unreachable!(),
    }.to_string()
}
fn contexts() -> Vec<Context> {
    vec![
        Context {
            source: "Old source 1".into(),
            translation: "旧译文1".into(),
        },
        Context {
            source: "Old source 2".into(),
            translation: "旧译文2".into(),
        },
    ]
}

#[test]
fn all_eleven_wire_requests_match_fixed_contract_and_do_not_share_headers() {
    runtime().block_on(async {
        for service in SERVICES {
            let server=Server::start(vec![Reply::Json(200,success(service))]);
            let client=server.client(service);
            let settings=settings(service);
            assert_eq!(client.translate(&settings,SOURCE,&contexts()).await.unwrap(),"你好","{service}");
            let capture=server.requests.lock().unwrap();
            assert_eq!(capture.len(),1,"{service}");
            let request=&capture[0];
            let query:HashMap<_,_>=request.url.query_pairs().into_owned().collect();
            assert_eq!(query["keep"],"a/b","query slash must survive");
            assert_eq!(request.method,if matches!(service,"Google"|"Google2") {"GET"} else {"POST"});
            assert_eq!(request.url.path(), match service { "LMStudio"=>"/wire/chat", "Ollama"=>"/wire/api/chat", _=>"/wire" });
            let authorization=request.headers.get("authorization").map(String::as_str);
            match service {
                "DeepL"=>assert_eq!(authorization,Some("DeepL-Auth-Key fixture-id")),
                "OpenAI"|"OpenRouter"|"MTranServer"=>assert_eq!(authorization,Some("Bearer fixture-id")),
                _=>assert_eq!(authorization,None,"{service}"),
            }
            if service=="Google" { assert_eq!(query["q"],SOURCE); assert_eq!(query["tl"],"zh-TW"); assert_eq!(query["sl"],"auto"); assert_eq!(query["client"],"dict-chrome-ex"); }
            else if service=="Google2" { assert_eq!(query["term"],SOURCE); assert_eq!(query["language"],"zh-TW"); assert_eq!(query["strategy"],"2"); assert_eq!(query["key"],"fixture-google2-key"); assert_eq!(request.headers["x-referer"],"chrome-extension://mgijmajocgfcbeboacabfgobmjgjcoja"); }
            else if matches!(service,"Youdao"|"Baidu") {
                assert!(request.headers["content-type"].starts_with("application/x-www-form-urlencoded"));
                let form:HashMap<_,_>=Url::parse(&format!("http://fixture/?{}",String::from_utf8(request.body.clone()).unwrap())).unwrap().query_pairs().into_owned().collect();
                assert_eq!(form["q"],SOURCE); assert_eq!(form["salt"],"fixture-salt-001"); assert_eq!(form["from"],"auto");
                assert_eq!(form["to"],if service=="Youdao" {"zh-CHT"} else {"cht"});
                assert_eq!(form[if service=="Youdao" {"appKey"} else {"appid"}],"fixture-id");
                // Independently generated with .NET MD5.HashData over UTF-8.
                assert_eq!(form["sign"],"e0d9cddc506956f644ec75cde31d9504");
            } else {
                let body:Value=serde_json::from_slice(&request.body).unwrap();
                assert!(request.headers["content-type"].starts_with("application/json"));
                match service {
                    "DeepL"=>assert_eq!(body,json!({"text":[SOURCE],"target_lang":"ZH-HANT"})),
                    "MTranServer"=>assert_eq!(body,json!({"text":SOURCE,"to":"zh","from":"en"})),
                    "LibreTranslate"=>assert_eq!(body,json!({"q":SOURCE,"target":"zh","source":"auto","format":"text","api_key":"fixture-id"})),
                    "LMStudio"=>assert_eq!(body,json!({"model":"fixture-model","system_prompt":"Translate to zh-TW. {Exact}","input":format!("🔤 Old source 1 🔤 → 旧译文1\n🔤 Old source 2 🔤 → 旧译文2\n🔤 {SOURCE} 🔤"),"temperature":0.7})),
                    _=> {
                        let mut expected=json!({"model":"fixture-model","temperature":0.7,"messages":[{"role":"system","content":"Translate to zh-TW. {Exact}"},{"role":"user","content":"🔤 Old source 1 🔤"},{"role":"assistant","content":"旧译文1"},{"role":"user","content":"🔤 Old source 2 🔤"},{"role":"assistant","content":"旧译文2"},{"role":"user","content":format!("🔤 {SOURCE} 🔤")}],"max_tokens":128,"stream":false,"keep_alive":600});
                        match service { "OpenAI"=>{expected["max_tokens"]=json!(512);add_template(&mut expected,0);}, "OpenRouter"=>{expected["max_tokens"]=json!(512);expected["reasoning"]=json!({"exclude":true,"enabled":false});}, "Ollama"=>{expected["think"]=json!(false);expected["keep_alive"]=json!(42);}, _=>unreachable!() }
                        assert_eq!(body,expected,"{service}");
                    }
                }
            }
        }
        // Reuse the very same client for authenticated and unauthenticated
        // services. A fresh client per service above alone cannot prove this.
        let server=Server::start(vec![Reply::Json(200,success("DeepL")),Reply::Json(200,success("LibreTranslate")),Reply::Json(200,success("Google2")),Reply::Json(200,success("Ollama"))]);
        let mut client=server.client("DeepL");
        for service in ["LibreTranslate","Google2","Ollama"] { client.endpoints.insert(service,server.url.clone()); }
        for service in ["DeepL","LibreTranslate","Google2","Ollama"] { client.translate(&settings(service),SOURCE,&[]).await.unwrap(); }
        let rows=server.requests.lock().unwrap();
        for row in &rows[1..] { assert!(!row.headers.contains_key("authorization")); }
        assert!(rows[2].headers.contains_key("x-referer"));
        assert!(!rows[3].headers.contains_key("x-referer"));
    });
}

#[test]
fn every_adapter_rejects_invalid_missing_empty_and_service_error_responses() {
    runtime().block_on(async {
        for service in SERVICES {
            for (status,body) in [(200,"{}"),(200,"[]"),(200,"not json"),(200,"{\"errorCode\":\"207\"}"),(401,"{\"error\":{\"code\":\"auth_failed\",\"message\":\"fixture-secret must not escape\"}}"),(429,"quota"),(503,"unavailable")] {
                let server=Server::start(vec![Reply::Json(status,body.into())]);
                let result=server.client(service).translate(&settings(service),SOURCE,&[]).await;
                let error=result.expect_err(service);
                assert_eq!(error.service,service);
                assert!(!error.to_string().contains("fixture-secret"));
                assert_eq!(server.count(),1,"{service} must not retry status {status}");
            }
            let empty=success(service).replace("你好","");
            let server=Server::start(vec![Reply::Json(200,empty)]);
            assert!(server.client(service).translate(&settings(service),SOURCE,&[]).await.is_err());
        }
    });
}

#[test]
fn all_eleven_complete_body_timeouts_and_cancellation_close_the_actual_connection() {
    runtime().block_on(async {
        for service in SERVICES {
            let server = Server::start(vec![Reply::SlowBody(Duration::from_millis(350))]);
            let mut client = server.client(service);
            client.timeout = Duration::from_millis(120);
            let started = Instant::now();
            let error = client
                .translate(&settings(service), SOURCE, &[])
                .await
                .unwrap_err();
            assert!(error.reason.contains("超时"), "{service}: {error}");
            assert!(started.elapsed() < Duration::from_secs(2));
            drop(server);
            let server = Server::start(vec![Reply::WaitForDisconnect]);
            let client = server.client(service);
            let job =
                tokio::spawn(
                    async move { client.translate(&settings(service), SOURCE, &[]).await },
                );
            let deadline = Instant::now() + Duration::from_secs(2);
            while server.count() == 0 {
                assert!(Instant::now() < deadline);
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
            job.abort();
            assert!(job.await.unwrap_err().is_cancelled());
            while !server.disconnected.load(Ordering::Acquire) {
                assert!(
                    Instant::now() < deadline,
                    "{service} did not close cancelled connection"
                );
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        }
    });
}

#[test]
fn languages_preserve_all_contract_entries_and_unknown_custom_codes() {
    let contract: Value = serde_json::from_str(include_str!(
        "../../../../../docs/handoff/Source-Contract.json"
    ))
    .unwrap();
    // Explicit independent expectations cover all 13 UI codes for each mapping.
    for (code, deepl, youdao, baidu, simple) in [
        ("zh-CN", "ZH-HANS", "zh-CHS", "zh", "zh"),
        ("zh-TW", "ZH-HANT", "zh-CHT", "cht", "zh"),
        ("en-US", "EN-US", "en", "en", "en"),
        ("en-GB", "EN-GB", "en", "en", "en"),
        ("ja-JP", "JA", "ja", "jp", "ja"),
        ("ko-KR", "KO", "ko", "kor", "ko"),
        ("fr-FR", "FR", "fr", "fra", "fr"),
        ("th-TH", "TH", "th", "th", "th"),
        ("ru-RU", "RU", "ru", "ru", "ru"),
        ("es-ES", "ES", "es", "spa", "es"),
        ("pt-BR", "PT-BR", "pt", "pt", "pt"),
        ("tr-TR", "TR", "tr", "tr", "tr"),
        ("ar-SA", "AR", "ar", "ara", "ar"),
    ] {
        for service in SERVICES {
            assert_eq!(
                language::target(service, code),
                match service {
                    "DeepL" => deepl,
                    "Youdao" => youdao,
                    "Baidu" => baidu,
                    "MTranServer" | "LibreTranslate" => simple,
                    _ => code,
                }
            );
        }
    }
    for (group, map) in contract["languageMappings"].as_object().unwrap() {
        let service = match group.as_str() {
            "TranslateAPIConfig" => "Google",
            "DeepLConfig" => "DeepL",
            "YoudaoConfig" => "Youdao",
            "BaiduConfig" => "Baidu",
            "MTranServerConfig" => "MTranServer",
            "LibreTranslateConfig" => "LibreTranslate",
            _ => panic!("unknown contract mapping"),
        };
        for (code, expected) in map.as_object().unwrap() {
            assert_eq!(language::target(service, code), expected.as_str().unwrap());
        }
    }
    for service in SERVICES {
        assert_eq!(
            language::target(service, "custom/language"),
            "custom/language"
        );
    }
    assert_eq!(
        signature::md5("Youdao", "").unwrap(),
        "d41d8cd98f00b204e9800998ecf8427e"
    );
    assert_ne!(
        signature::salt("Baidu").unwrap(),
        signature::salt("Baidu").unwrap()
    );
}

#[test]
fn openai_tries_each_template_once_caches_success_and_scopes_by_active_configuration() {
    runtime().block_on(async {
        let mut replies: Vec<_> = (0..7).map(|_| Reply::Json(422, "{}".into())).collect();
        replies.extend([
            Reply::Json(200, success("OpenAI")),
            Reply::Json(200, success("OpenAI")),
            Reply::Json(200, success("OpenAI")),
        ]);
        let server = Server::start(replies);
        let client = server.client("OpenAI");
        let settings = settings("OpenAI");
        for _ in 0..2 {
            assert_eq!(
                client.translate(&settings, SOURCE, &[]).await.unwrap(),
                "你好"
            );
        }
        let screen = Screen::new(1920.0, 1080.0).unwrap();
        let changed = settings
            .edited(screen, |v| {
                let mut second = v["Configs"]["OpenAI"][0].clone();
                second["ApiKey"] = json!("second-fixture-key");
                v["Configs"]["OpenAI"].as_array_mut().unwrap().push(second);
                v["ConfigIndices"]["OpenAI"] = json!(1);
            })
            .unwrap()
            .0;
        client.translate(&changed, SOURCE, &[]).await.unwrap();
        let rows = server.requests.lock().unwrap();
        assert_eq!(rows.len(), 10);
        for (index, row) in rows[..8].iter().enumerate() {
            let body: Value = serde_json::from_slice(&row.body).unwrap();
            let keys = body.as_object().unwrap();
            let extras: Vec<_> = keys
                .keys()
                .filter(|key| {
                    ![
                        "model",
                        "messages",
                        "temperature",
                        "max_tokens",
                        "stream",
                        "keep_alive",
                    ]
                    .contains(&key.as_str())
                })
                .map(String::as_str)
                .collect();
            assert_eq!(
                extras,
                match index {
                    0 => vec![
                        "enable_thinking",
                        "reasoning",
                        "reasoning_effort",
                        "think",
                        "thinking"
                    ],
                    1 => vec!["enable_thinking"],
                    2 => vec!["thinking"],
                    3 => vec!["think"],
                    4 | 5 => vec!["reasoning"],
                    6 => vec!["reasoning_effort"],
                    _ => vec![],
                }
            );
        }
        let cached: Value = serde_json::from_slice(&rows[8].body).unwrap();
        assert!(cached.get("think").is_none());
        let fresh: Value = serde_json::from_slice(&rows[9].body).unwrap();
        assert_eq!(fresh["think"], false);
        assert_eq!(
            rows[9].headers["authorization"],
            "Bearer second-fixture-key"
        );
    });
}

#[test]
fn openai_cached_template_failure_retries_other_templates_once_and_never_retries_auth() {
    runtime().block_on(async {
        let server = Server::start(vec![
            Reply::Json(400, "{}".into()),
            Reply::Json(200, success("OpenAI")),
            Reply::Json(422, "{}".into()),
            Reply::Json(200, success("OpenAI")),
            Reply::Json(401, "{}".into()),
        ]);
        let client = server.client("OpenAI");
        let settings = settings("OpenAI");
        client.translate(&settings, SOURCE, &[]).await.unwrap();
        client.translate(&settings, SOURCE, &[]).await.unwrap();
        assert_eq!(
            client
                .translate(&settings, SOURCE, &[])
                .await
                .unwrap_err()
                .status,
            Some(401)
        );
        let rows = server.requests.lock().unwrap();
        assert_eq!(rows.len(), 5);
        let body: Vec<Value> = rows
            .iter()
            .map(|row| serde_json::from_slice(&row.body).unwrap())
            .collect();
        assert_eq!(body[1].get("think"), None);
        assert_eq!(body[2].get("think"), None);
        assert_eq!(body[3]["think"], false);
    });
}

#[test]
fn llm_context_off_omits_history_and_models_keep_ids_and_filter_embedding_entries() {
    runtime().block_on(async {
        let screen=Screen::new(1920.0,1080.0).unwrap();
        for service in ["OpenAI","OpenRouter","Ollama","LMStudio"] {
            let server=Server::start(vec![Reply::Json(200,success(service))]);
            let settings=settings(service).edited(screen,|v|v["ContextAware"]=json!(false)).unwrap().0;
            server.client(service).translate(&settings,SOURCE,&contexts()).await.unwrap();
            let rows=server.requests.lock().unwrap();let body:Value=serde_json::from_slice(&rows[0].body).unwrap();
            if service=="LMStudio" { assert_eq!(body["input"],format!("🔤 {SOURCE} 🔤")); } else { assert_eq!(body["messages"].as_array().unwrap().len(),2); }
        }
        let server=Server::start(vec![Reply::Json(200,json!({"models":[{"type":"embedding","key":"exclude"},{"type":"llm","key":"id-1","display_name":"Pretty Name"},{"type":"llm","key":"id-2"},{"type":"llm","key":""}]}).to_string()),Reply::Json(200,json!({"models":[{"name":"local:latest"},{"name":""}]}).to_string())]);
        let mut client=server.client("LMStudio");client.endpoints.insert("Ollama",server.url.clone());
        assert_eq!(client.models("LMStudio","unused").await.unwrap(),vec![Model{id:"id-1".into(),display_name:"Pretty Name".into()},Model{id:"id-2".into(),display_name:"id-2".into()}]);
        assert_eq!(client.models("Ollama","unused").await.unwrap(),vec![Model{id:"local:latest".into(),display_name:"local:latest".into()}]);
        let rows=server.requests.lock().unwrap();assert_eq!(rows[0].method,"GET");assert_eq!(rows[0].url.path(),"/wire/models");assert_eq!(rows[1].url.path(),"/api/tags");
    });
}

#[test]
fn production_translation_and_model_deadlines_include_response_body() {
    runtime().block_on(async {
        let server = Server::start(vec![Reply::SlowBody(Duration::from_millis(8300))]);
        let client = server.client("Google");
        assert_eq!(client.timeout(), Duration::from_secs(8));
        let started = Instant::now();
        assert!(
            client
                .translate(&settings("Google"), SOURCE, &[])
                .await
                .unwrap_err()
                .reason
                .contains("超时")
        );
        assert!(
            started.elapsed() >= Duration::from_secs(7)
                && started.elapsed() < Duration::from_secs(10)
        );
        drop(server);
        let server = Server::start(vec![Reply::SlowBody(Duration::from_millis(10300))]);
        let client = server.client("LMStudio");
        let started = Instant::now();
        assert!(
            client
                .models("LMStudio", "unused")
                .await
                .unwrap_err()
                .reason
                .contains("超时")
        );
        assert!(
            started.elapsed() >= Duration::from_secs(9)
                && started.elapsed() < Duration::from_secs(12)
        );
    });
}

#[test]
fn urls_preserve_complete_chat_addresses_and_openai_rejection_stops_at_eight() {
    let client = Translator::new().unwrap();
    assert_eq!(
        client
            .url(
                "OpenAI",
                " https://example.invalid/a/chat/completions?q=a//b ",
                ""
            )
            .unwrap()
            .as_str(),
        "https://example.invalid/a/chat/completions?q=a//b"
    );
    assert_eq!(
        client
            .url("LMStudio", "http://localhost:1234/api/v1/?q=a/b", "/chat")
            .unwrap()
            .as_str(),
        "http://localhost:1234/api/v1/chat?q=a/b"
    );
    for invalid in [
        "file:///secret",
        "ftp://example.invalid",
        "missing scheme",
        "",
    ] {
        assert!(client.url("OpenAI", invalid, "").is_err());
    }
    runtime().block_on(async {
        let server = Server::start((0..8).map(|_| Reply::Json(422, "{}".into())).collect());
        let error = server
            .client("OpenAI")
            .translate(&settings("OpenAI"), SOURCE, &[])
            .await
            .unwrap_err();
        assert_eq!(error.status, Some(422));
        assert_eq!(server.count(), 8);
    });
}

#[test]
#[ignore = "Actual public Google/Google2 service calls; run explicitly, no UI Automation"]
fn real_public_multi_provider_google_adapters() {
    const TEXT: &str = "Hello, world. This is a public translation test.";
    let client = Translator::new().unwrap();
    let screen = Screen::new(1920.0, 1080.0).unwrap();
    let mut evidence = Vec::new();
    let mut google_passed = false;
    runtime().block_on(async {
        for service in ["Google","Google2"] {
            let settings=Settings::defaults(screen).edited(screen,|value|value["ApiName"]=json!(service)).unwrap().0;
            let started=Instant::now();
            let result=client.translate(&settings,TEXT,&[]).await;
            evidence.push(match result {
                Ok(text)=> { let passed=text.chars().any(|ch|matches!(ch as u32,0x3400..=0x9fff)); if service=="Google" {google_passed=passed;} json!({"service":service,"status":if passed {"PASS"} else {"UNEXPECTED_RESPONSE"},"scope":"actual HTTP only, public fixture; no real captions or GUI","source":TEXT,"target":"zh-CN","translation":text,"latency_ms":started.elapsed().as_millis()}) },
                Err(error)=>json!({"service":service,"status":"BLOCKED_EXTERNAL","scope":"actual HTTP only, public fixture; no real captions or GUI","error":error.to_string(),"latency_ms":started.elapsed().as_millis()}),
            });
        }
    });
    if let Some(path) = std::env::var_os("LIVE_CAPTIONS_SERVICE_EVIDENCE") {
        let file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(path)
            .unwrap();
        serde_json::to_writer_pretty(file, &evidence).unwrap();
    }
    assert!(
        google_passed,
        "Google actual call did not return Chinese; inspect scoped service evidence"
    );
}

#[test]
fn service_codes_never_echo_active_credentials_but_normal_error_codes_remain_visible() {
    runtime().block_on(async {
        for (service, secret) in [
            ("OpenAI", "fixture-id"),
            ("DeepL", "fixture-id"),
            ("Youdao", "fixture-secret"),
            ("Baidu", "fixture-id"),
            ("Google2", "fixture-google2-key"),
        ] {
            for code in [secret.to_owned(), format!("prefix-{secret}-suffix")] {
                let server = Server::start(vec![Reply::Json(
                    401,
                    json!({"error":{"code":code}}).to_string(),
                )]);
                let error = server
                    .client(service)
                    .translate(&settings(service), SOURCE, &[])
                    .await
                    .unwrap_err();
                assert_eq!(error.code, None);
                assert!(!error.to_string().contains(secret));
                assert_eq!(error.status, Some(401));
            }
        }
        let server = Server::start(vec![Reply::Json(
            401,
            json!({"error":{"code":"auth_failed"}}).to_string(),
        )]);
        let error = server
            .client("OpenAI")
            .translate(&settings("OpenAI"), SOURCE, &[])
            .await
            .unwrap_err();
        assert_eq!(error.code.as_deref(), Some("auth_failed"));
    });
}

#[test]
fn truncated_chat_completions_are_rejected_without_retrying() {
    runtime().block_on(async {
        for service in ["OpenAI", "OpenRouter"] {
            let response = json!({"choices":[{
                "message":{"content":"这是一段尚未完成的译文"},
                "finish_reason":"length"
            }]});
            let server = Server::start(vec![Reply::Json(200, response.to_string())]);
            let error = server
                .client(service)
                .translate(&settings(service), SOURCE, &[])
                .await
                .unwrap_err();
            assert_eq!(error.reason, "译文达到输出长度上限，结果不完整");
            assert_eq!(server.count(), 1);
            let requests = server.requests.lock().unwrap();
            let body: Value = serde_json::from_slice(&requests[0].body).unwrap();
            assert_eq!(body["max_tokens"], 512);
        }
    });
}

#[test]
#[ignore = "Explicit real Google/OpenAI calls with public text and an authorized local config"]
fn real_configured_services_public_samples() {
    let config_path =
        std::env::var_os("LIVE_CAPTIONS_TEST_CONFIG").expect("authorized config path");
    let evidence_path = std::env::var_os("LIVE_CAPTIONS_SERVICE_EVIDENCE").expect("evidence path");
    let screen = Screen::new(1920.0, 1080.0).unwrap();
    let config = std::fs::read_to_string(config_path).expect("read authorized config");
    let original = Settings::parse(&config, screen)
        .expect("valid configured settings")
        .0;
    let contexts = vec![
        Context {
            source: "Alex borrowed my camera yesterday.".into(),
            translation: "亚历克斯昨天借了我的相机。".into(),
        },
        Context {
            source: "He promised to return it before lunch.".into(),
            translation: "他答应午饭前归还。".into(),
        },
    ];
    let samples = [
        ("negation", "Please turn the music down, not off.", false),
        ("context", "Did he bring it back?", true),
        (
            "long",
            "Before we leave for the airport tomorrow morning, please check that every window is closed, put the spare keys in the small drawer beside the kitchen door, leave a note for the neighbour explaining when the plants need water, make sure the cat has enough food and fresh water, unplug the coffee machine, pack the blue folder containing our tickets and hotel reservation, charge both phones, set an alarm for half past five, and send me a message when everything is ready so that we can leave together without rushing or forgetting anything important.",
            false,
        ),
    ];
    let mut evidence = Vec::new();
    runtime().block_on(async {
        let client = Translator::new().unwrap();
        for service in ["Google", "OpenAI"] {
            let settings = original.edited(screen, |root| root["ApiName"] = json!(service)).unwrap().0;
            for (kind, source, with_context) in samples {
                let supplied = if with_context {contexts.as_slice()} else {&[]};
                let mut history = captions_core::text::Contexts::default();
                for (index, context) in supplied.iter().enumerate() {
                    history.upsert(index as u64, 1, context.clone());
                }
                let input = if service == "Google" {
                    history.non_llm_input_before(10, source, 2, with_context)
                } else {source.into()};
                let started = Instant::now();
                let result = client.translate(&settings, &input, supplied).await;
                let result = match result {
                    Ok(raw) => {
                        let current = if service == "Google" && with_context {
                            captions_core::text::extract_marked(&raw).unwrap_or(&raw)
                        } else {&raw};
                        json!({"status":"PASS_HTTP", "translation":captions_core::text::clean_translation(current)})
                    }
                    Err(error) => json!({"status":"FAILED", "error":error.to_string()}),
                };
                evidence.push(json!({"service":service,"sample":kind,"source":source,
                    "target":settings.target_language(),"context_count":supplied.len(),
                    "model":if service == "OpenAI" {settings.active_config()["ModelName"].clone()} else {Value::Null},
                    "latency_ms":started.elapsed().as_millis(),"result":result}));
            }
        }
    });
    let report = json!({"scope":"Real HTTP with public samples, current adapters and configured model; no desktop/ASR or original executable comparison", "samples":evidence});
    let file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(evidence_path)
        .unwrap();
    serde_json::to_writer_pretty(file, &report).unwrap();
    assert!(
        evidence
            .iter()
            .all(|row| row["result"]["status"] == "PASS_HTTP"
                && row["result"]["translation"]
                    .as_str()
                    .is_some_and(|text| !text.trim().is_empty())),
        "inspect public sample evidence for actual service failures"
    );
}
