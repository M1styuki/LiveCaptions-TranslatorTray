//! Real controller and isolated data with GPUI’s headless platform.
use crate::{MainWindow, controller::Controller, data::Data, ui_bridge::Bridge};
use captions_core::settings::Screen;
use gpui::{Modifiers, MouseButton, TestAppContext, VisualTestContext, point, px, size};
use std::{
    fs,
    rc::Rc,
    time::{Duration, Instant},
};

mod api_interaction;
mod caption_interaction;
mod caption_toggle;
mod capture_cutoff;
mod deletion;
mod main_geometry;
mod overlay_geometry;
mod startup;
fn until(
    controller: &mut Controller,
    jobs: &TestAppContext,
    condition: impl Fn(&Controller) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !condition(controller) {
        assert!(Instant::now() < deadline, "controller transition timed out");
        jobs.executor().run_until_parked();
        std::thread::sleep(Duration::from_millis(10));
        controller.wakeup();
    }
}
fn until_owner(
    owner: &Rc<std::cell::RefCell<Controller>>,
    jobs: &TestAppContext,
    condition: impl Fn() -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !condition() {
        assert!(Instant::now() < deadline, "API editor transition timed out");
        jobs.executor().run_until_parked();
        std::thread::sleep(Duration::from_millis(5));
        owner.borrow_mut().wakeup();
    }
}
fn model_server() -> (
    String,
    std::sync::Arc<std::sync::atomic::AtomicUsize>,
    std::thread::JoinHandle<bool>,
) {
    use std::io::{Read, Write};
    use std::sync::atomic::{AtomicUsize, Ordering};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}/api/v1", listener.local_addr().unwrap());
    let stage = std::sync::Arc::new(AtomicUsize::new(0));
    let counter = stage.clone();
    let thread = std::thread::spawn(move || {
        for index in 1..=3 {
            // The UI gallery intentionally waits for finite page animations
            // between the first model request and the next request.
            let deadline = Instant::now() + Duration::from_secs(30);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(
                            Instant::now() < deadline,
                            "model fixture received no request"
                        );
                        std::thread::sleep(Duration::from_millis(2));
                    }
                    Err(error) => panic!("{error}"),
                }
            };
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut header = Vec::new();
            while !header.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                stream.read_exact(&mut byte).unwrap();
                header.push(byte[0]);
            }
            assert!(
                String::from_utf8(header)
                    .unwrap()
                    .starts_with("GET /api/v1/models ")
            );
            if index == 3 {
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: 999\r\nConnection: close\r\n\r\n"
                )
                .unwrap();
                stream.flush().unwrap();
                counter.store(index, Ordering::Release);
                let mut byte = [0];
                return matches!(stream.read(&mut byte), Ok(0));
            }
            let (status, body) = if index == 1 {
                (
                    200,
                    r#"{"models":[{"type":"embedding","key":"exclude"},{"type":"llm","key":"actual-model-id","display_name":"Friendly model name"}]}"#,
                )
            } else {
                (503, r#"{"error":{"code":"unavailable"}}"#)
            };
            write!(stream,"HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
            counter.store(index, Ordering::Release);
        }
        false
    });
    (url, stage, thread)
}
fn redraw(window: &mut VisualTestContext, width: f32, height: f32) {
    window.simulate_resize(size(px(width), px(height)));
    window.run_until_parked();
    window.update(|window, cx| {
        let _ = window.draw(cx);
    });
}

fn center(window: &mut VisualTestContext, selector: &'static str) -> gpui::Point<gpui::Pixels> {
    window
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("missing UI element {selector}"))
        .center()
}

#[test]
fn actual_controller_saves_last_edit_and_keeps_recovery_open_after_write_failure() {
    let jobs = TestAppContext::single();
    jobs.update(|cx| {
        gpui_component::init(cx);
        crate::ui::runtime::start(cx);
    });
    let ui = MainWindow::new().unwrap();
    let parent = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../scratch/controller-fixtures");
    fs::create_dir_all(&parent).unwrap();
    let parent = parent.canonicalize().unwrap();
    let root = parent.join(windows_platform::desktop::instance_id().unwrap());
    fs::create_dir(&root).unwrap();
    let directory = root.join("data");
    fs::create_dir(&directory).unwrap();
    let screen = Screen::new(1920.0, 1080.0).unwrap();
    let data = Data::start(directory.clone(), screen, ui.as_weak()).unwrap();
    let (bridge, _host_inbox) = Bridge::fixture();
    let mut controller = Controller::new(bridge, ui.as_weak(), data, screen);
    controller.test_show_settings();
    until(&mut controller, &jobs, |_| ui.get_data_state() == 2);
    // The headless views use the same rendering and input paths as the desktop.
    controller.test_show_settings();
    let owner = Rc::new(std::cell::RefCell::new(controller));
    Controller::open_api(&owner);
    let api = owner.borrow().test_api();
    let mut main_view = VisualTestContext::from_window(ui.window().test_handle(), &jobs);
    let mut api_view = VisualTestContext::from_window(api.window().test_handle(), &jobs);
    assert_eq!(owner.borrow().test_windows(), 2);
    for (index, service) in [
        "Ollama",
        "LMStudio",
        "OpenAI",
        "OpenRouter",
        "DeepL",
        "Youdao",
        "MTranServer",
        "Baidu",
        "LibreTranslate",
    ]
    .iter()
    .enumerate()
    {
        api.invoke_section_changed(index as i32 + 1);
        assert_eq!(api.get_service(), *service);
        assert_eq!(api.get_config_count(), 1);
        api.invoke_config_action(2);
        assert_eq!(api.get_config_count(), 2);
        assert_eq!(api.get_config_index(), 1);
        let marker = format!("fixture-{service}");
        let field = if providers::is_llm(service) {
            "ModelName"
        } else if *service == "Youdao" {
            "AppKey"
        } else if *service == "Baidu" {
            "AppId"
        } else {
            "ApiKey"
        };
        api.invoke_field_edited(field.into(), marker.clone().into());
        api.invoke_config_action(0);
        assert_eq!(api.get_config_index(), 0);
        api.invoke_config_action(1);
        assert_eq!(
            owner.borrow().test_settings().json()["Configs"][*service][1][field],
            marker
        );
        api.invoke_config_action(2);
        assert_eq!(api.get_config_count(), 3);
        api.invoke_config_action(3);
        assert_eq!(api.get_config_count(), 2);
    }
    assert_eq!(owner.borrow().test_settings().api_name(), "Google");
    assert_eq!(api.invoke_section_count(), 9);
    let selected_section = api.get_section();
    api.invoke_editor_field_edited(1, "ModelName".into(), "visible-card-edit".into());
    assert_eq!(api.get_section(), selected_section);
    assert_eq!(
        owner.borrow().test_settings().json()["Configs"]["Ollama"][1]["ModelName"],
        "visible-card-edit"
    );
    assert_eq!(
        api.get_editors().row_data(0).unwrap().model_name,
        "visible-card-edit"
    );
    api.invoke_editor_action(1, 0, 2);
    assert_eq!(api.get_editors().row_data(0).unwrap().config_count, 3);
    api.invoke_editor_action(1, 0, 3);
    assert_eq!(api.get_editors().row_data(0).unwrap().config_count, 2);
    assert_eq!(api.get_section(), selected_section);
    assert_eq!(owner.borrow().test_settings().api_name(), "Google");
    owner.borrow_mut().setting_edited("DisplaySentences", "5");
    assert_eq!(ui.get_num_contexts(), 5);
    owner.borrow_mut().setting_edited("NumContexts", "2");
    assert_eq!(ui.get_display_sentences(), 2);
    owner
        .borrow_mut()
        .setting_edited("TargetLanguage", "custom-code");
    assert_eq!(ui.get_selected_language(), "custom-code");
    owner.borrow_mut().setting_edited("ApiName", "Google2");
    assert_eq!(ui.get_selected_service(), "Google2");
    owner.borrow_mut().setting_edited("ApiName", "Google");
    api_interaction::run(&owner, &api, &mut api_view);
    let (model_url, model_stage, model_thread) = model_server();
    api.invoke_section_changed(2);
    api.set_api_url(model_url.clone().into());
    api.invoke_field_edited("ApiUrl".into(), model_url.into());
    api.set_temperature_text("0.65".into());
    api.invoke_field_edited("Temperature".into(), "0.65".into());
    api.invoke_temperature_stepped(1);
    assert_eq!(api.get_temperature_text(), "0.75");
    api.invoke_temperature_stepped(-1);
    let settings = owner.borrow().test_settings();
    let selected = settings.json()["ConfigIndices"]["LMStudio"]
        .as_u64()
        .unwrap() as usize;
    assert_eq!(
        settings.json()["Configs"]["LMStudio"][selected]["Temperature"],
        0.65
    );
    api.invoke_field_edited("Temperature".into(), "NaN".into());
    assert!(api.get_message().contains("仍保留上一次有效值"));
    assert_eq!(
        owner.borrow().test_settings().json()["Configs"]["LMStudio"][selected]["Temperature"],
        0.65
    );
    api.invoke_field_edited("Temperature".into(), "0.65".into());
    api.invoke_field_edited("ModelName".into(), "handwritten-model".into());
    api.invoke_models_requested();
    until_owner(&owner, &jobs, || !api.get_models_loading());
    assert_eq!(api.get_model_name(), "handwritten-model");
    assert_eq!(
        api.get_model_labels().row_count(),
        1,
        "{}",
        api.get_message()
    );
    assert_eq!(
        api.get_model_labels().row_data(0).unwrap(),
        "Friendly model name"
    );
    let section = api.get_section();
    let service = api.get_service();
    for dark in [false, true] {
        ui.set_frame_dark(dark);
        api.set_frame_dark(dark);
        redraw(&mut main_view, 900., 600.);
        for (index, provider) in [
            "Prompt",
            "Ollama",
            "LMStudio",
            "OpenAI",
            "OpenRouter",
            "DeepL",
            "Youdao",
            "MTranServer",
            "Baidu",
            "LibreTranslate",
        ]
        .into_iter()
        .enumerate()
        {
            api.set_section(index as i32);
            api.set_service(provider.into());
            if index > 0 {
                api.set_active_editor(api.get_editors().row_data(index - 1).unwrap());
            }
            for (width, height) in [(950., 600.), (760., 360.)] {
                redraw(&mut api_view, width, height);
            }
        }
    }
    api.set_section(section);
    api.set_service(service);
    api.set_active_editor(api.get_editors().row_data(section as usize - 1).unwrap());
    redraw(&mut api_view, 950., 500.);
    api.invoke_model_selected(0);
    assert_eq!(api.get_model_name(), "actual-model-id");
    api.invoke_models_requested();
    until_owner(&owner, &jobs, || !api.get_models_loading());
    assert_eq!(api.get_model_name(), "actual-model-id");
    assert!(api.get_message().contains("现有模型名称已保留"));
    api.invoke_models_requested();
    until_owner(&owner, &jobs, || {
        model_stage.load(std::sync::atomic::Ordering::Acquire) == 3
    });
    let cancel_started = Instant::now();
    api.invoke_section_changed(3);
    assert!(cancel_started.elapsed() < Duration::from_secs(2));
    assert!(
        model_thread.join().unwrap(),
        "changing section must close the actual pending model request"
    );
    assert_eq!(api.get_model_name(), "fixture-OpenAI-typed-second");
    assert!(!api.get_models_loading());
    ui.hide().unwrap();
    owner.borrow_mut().main_closed();
    assert_eq!(owner.borrow().test_windows(), 1);
    assert!(!owner.borrow().test_data_stopped());
    owner.borrow_mut().test_show_settings();
    assert_eq!(owner.borrow().test_windows(), 2);
    api.invoke_window_action(1);
    assert_eq!(owner.borrow().test_windows(), 1);
    let mut controller = Rc::try_unwrap(owner).ok().unwrap().into_inner();
    controller.prompt_changed("The last accepted edit to {0}");
    controller.test_queue_style(windows_platform::overlay::Settings {
        font_size: 40,
        ..Default::default()
    });
    controller.main_closed();
    controller.prompt_changed("Rejected edit after Shutdown was queued to {0}");
    until(&mut controller, &jobs, |controller| {
        controller.test_data_stopped()
    });
    let read_prompt = || {
        let value: serde_json::Value =
            serde_json::from_slice(&fs::read(directory.join("setting.json")).unwrap()).unwrap();
        value["Prompt"].as_str().unwrap().to_owned()
    };
    assert_eq!(read_prompt(), "The last accepted edit to {0}");
    let value: serde_json::Value =
        serde_json::from_slice(&fs::read(directory.join("setting.json")).unwrap()).unwrap();
    assert_eq!(value["OverlayWindow"]["FontSize"], 40);
    drop(controller);

    let data = Data::start(directory.clone(), screen, ui.as_weak()).unwrap();
    let (bridge, _second_host_inbox) = Bridge::fixture();
    let mut controller = Controller::new(bridge, ui.as_weak(), data, screen);
    ui.set_data_state(0);
    until(&mut controller, &jobs, |_| ui.get_data_state() == 2);
    let settings_path = directory.join("setting.json");
    let preserved = directory.join("fixture-original.json");
    fs::rename(&settings_path, &preserved).unwrap();
    fs::create_dir(&settings_path).unwrap();
    controller.prompt_changed("Retain this failed edit to {0}");
    controller.main_closed();
    until(&mut controller, &jobs, |_| {
        ui.get_data_message().starts_with("保存尚未完成")
    });
    assert!(!controller.test_data_stopped());
    assert!(ui.get_retry_save());
    assert!(
        ui.window().is_visible(),
        "recovery stays available in the virtual window"
    );
    fs::remove_dir(&settings_path).unwrap();
    fs::rename(&preserved, &settings_path).unwrap();
    controller.data_action(4, "");
    until(&mut controller, &jobs, |_| {
        ui.get_data_message() == "设置已保存。"
    });
    assert!(!ui.get_retry_save());
    controller.main_closed();
    until(&mut controller, &jobs, |controller| {
        controller.test_data_stopped()
    });
    assert_eq!(read_prompt(), "Retain this failed edit to {0}");
    drop(controller);

    // A genuine SQLite exclusive lock delays the actor's preceding history
    // query for its five-second busy timeout. The final JSON edit must survive
    // the queued save, not just an immediate failure/retry path.
    let data = Data::start(directory.clone(), screen, ui.as_weak()).unwrap();
    let (bridge, _slow_host_inbox) = Bridge::fixture();
    let mut controller = Controller::new(bridge, ui.as_weak(), data, screen);
    ui.set_data_state(0);
    until(&mut controller, &jobs, |_| ui.get_data_state() == 2);
    let lock = rusqlite::Connection::open(directory.join("translation_history.db")).unwrap();
    lock.execute_batch("BEGIN EXCLUSIVE;").unwrap();
    let waiting = Instant::now();
    controller.test_show_history();
    controller.prompt_changed("Queued behind a five second database lock to {0}");
    controller.main_closed();
    until(&mut controller, &jobs, |controller| {
        controller.test_data_stopped()
    });
    assert!(waiting.elapsed() >= Duration::from_secs(5));
    assert_eq!(
        read_prompt(),
        "Queued behind a five second database lock to {0}"
    );
    lock.execute_batch("ROLLBACK;").unwrap();
    drop(lock);
    drop(controller);

    // Closing settings before storage readiness cancels a pending start
    // without launching capture or initializing an unused directory.
    let empty = root.join("empty-data");
    fs::create_dir(&empty).unwrap();
    let data = Data::start(empty.clone(), screen, ui.as_weak()).unwrap();
    let (bridge, _first_use_host_inbox) = Bridge::fixture();
    let mut controller = Controller::new(bridge, ui.as_weak(), data, screen);
    ui.set_data_state(0);
    controller.test_show_settings();
    controller.test_wait_for_first_use();
    controller.main_closed();
    until(&mut controller, &jobs, |controller| {
        controller.test_data_stopped()
    });
    assert!(!empty.join("setting.json").exists());
    assert!(!empty.join("translation_history.db").exists());
    drop(controller);
    // The actual controller -> HTTP transport -> SQLite -> GPUI state path,
    // with a local protocol server and a virtual window (no UIA/desktop).
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let endpoint = format!("http://{}/translate", listener.local_addr().unwrap());
    let server = std::thread::spawn(move || {
        use std::io::{Read, Write};
        let mut queries = Vec::new();
        for response in [
            "未包含标记的整段译文",
            "<think>public\nfixture</think>当前句译文",
            "",
            "🔤恢复后的译文🔤",
        ] {
            let deadline = Instant::now() + Duration::from_secs(8);
            let (mut stream, _) = loop {
                match listener.accept() {
                    Ok(stream) => break stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(
                            Instant::now() < deadline,
                            "local fixture received no request"
                        );
                        std::thread::sleep(Duration::from_millis(2));
                    }
                    Err(error) => panic!("{error}"),
                }
            };
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut header = Vec::new();
            while !header.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                stream.read_exact(&mut byte).unwrap();
                header.push(byte[0]);
                assert!(header.len() < 16384);
            }
            let header = String::from_utf8(header).unwrap();
            let path = header
                .lines()
                .next()
                .unwrap()
                .split_whitespace()
                .nth(1)
                .unwrap();
            let url = reqwest::Url::parse(&format!("http://localhost{path}")).unwrap();
            queries.push(
                url.query_pairs()
                    .find(|(key, _)| key == "q")
                    .unwrap()
                    .1
                    .into_owned(),
            );
            let body = serde_json::to_vec(&serde_json::json!([[response]])).unwrap();
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).unwrap();
            stream.write_all(&body).unwrap();
        }
        queries
    });
    let data = Data::start(directory.clone(), screen, ui.as_weak()).unwrap();
    let (bridge, _network_host_inbox) = Bridge::fixture();
    let mut controller = Controller::new(bridge, ui.as_weak(), data, screen);
    ui.set_data_state(0);
    until(&mut controller, &jobs, |_| ui.get_data_state() == 2);
    controller.test_start_network_fixture(
        providers::Translator::local_fixture("Google", endpoint),
        "Google",
    );
    controller.test_capture("This is a public local protocol fixture.");
    controller.test_capture("");
    until(&mut controller, &jobs, |_| {
        ui.get_translated_caption() == "当前句译文"
    });
    controller.test_show_history();
    until(&mut controller, &jobs, |_| {
        ui.get_history_rows().row_count() == 1
    });
    let row = ui.get_history_rows().row_data(0).unwrap();
    assert_eq!(row.translation, "当前句译文");
    assert_eq!(row.source, "This is a public local protocol fixture.");
    controller.test_capture(
        "This is a public local protocol fixture. A second public sentence is complete.",
    );
    until(&mut controller, &jobs, |_| {
        ui.get_caption_notice().contains("翻译失败")
    });
    assert_eq!(ui.get_translated_caption(), "当前句译文");
    assert!(
        controller
            .test_overlay_content()
            .notice
            .contains("翻译失败")
    );
    assert!(
        controller
            .test_overlay_content()
            .translation
            .ends_with("当前句译文")
    );
    controller.test_capture("This is a public local protocol fixture. A second public sentence is complete. The third public sentence completes recovery.");
    until(&mut controller, &jobs, |_| {
        ui.get_translated_caption() == "恢复后的译文"
    });
    assert!(ui.get_caption_notice().is_empty());
    assert!(controller.test_overlay_content().notice.is_empty());
    let queries = server.join().unwrap();
    assert_eq!(queries.len(), 4);
    assert!(queries[0].contains("🔤 This is a public local protocol fixture. 🔤"));
    assert_eq!(queries[1], "This is a public local protocol fixture.");
    // Exercise both LLM protocol families through the actual Flow/Engine,
    // rather than passing hand-built context directly to an adapter only.
    for service in ["OpenAI", "LMStudio"] {
        controller.test_stop_engine();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            use std::io::{Read, Write};
            let mut requests = Vec::new();
            for index in 0..2 {
                let deadline = Instant::now() + Duration::from_secs(8);
                let mut stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            assert!(Instant::now() < deadline, "LLM fixture received no request");
                            std::thread::sleep(Duration::from_millis(2));
                        }
                        Err(error) => panic!("{error}"),
                    }
                };
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut header = Vec::new();
                while !header.ends_with(b"\r\n\r\n") {
                    let mut byte = [0];
                    stream.read_exact(&mut byte).unwrap();
                    header.push(byte[0]);
                    assert!(header.len() < 16384);
                }
                let header = String::from_utf8(header).unwrap();
                let length: usize = header
                    .lines()
                    .find_map(|line| {
                        line.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .map(|value| value.trim().parse().unwrap())
                    })
                    .unwrap();
                let mut input = vec![0; length];
                stream.read_exact(&mut input).unwrap();
                requests.push(serde_json::from_slice::<serde_json::Value>(&input).unwrap());
                let content = if index == 0 {
                    "模型第一句"
                } else {
                    "<think>fixture</think>模型第二句"
                };
                let body = if service == "OpenAI" {
                    serde_json::json!({"choices":[{"message":{"content":content}}]})
                } else {
                    serde_json::json!({"output":[{"type":"message","content":content}]})
                };
                let body = serde_json::to_vec(&body).unwrap();
                write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",body.len()).unwrap();
                stream.write_all(&body).unwrap();
            }
            requests
        });
        controller.setting_edited("DisplaySentences", "1");
        controller.test_start_network_fixture(
            providers::Translator::local_fixture(service, endpoint),
            service,
        );
        controller.test_capture("The first public caption for the model.");
        controller.test_capture("");
        until(&mut controller, &jobs, |_| {
            ui.get_translated_caption() == "模型第一句"
        });
        controller.adjust_caption_font(0, 1);
        assert_eq!(ui.get_translated_caption(), "模型第一句");
        controller.test_capture(
            "The first public caption for the model. The next public caption carries context.",
        );
        assert_eq!(ui.get_translated_caption(), "模型第一句");
        assert!(
            controller
                .test_overlay_content()
                .translation
                .ends_with("模型第一句")
        );
        controller.test_capture("");
        until(&mut controller, &jobs, |_| {
            ui.get_translated_caption() == "模型第二句"
        });
        let overlay = controller.test_overlay_content();
        assert_eq!(overlay.translation, "模型第一句。模型第二句");
        assert_eq!(overlay.previous_translation_units, 6);
        let requests = server.join().unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[1]["model"], "fixture-model");
        if service == "OpenAI" {
            let messages = requests[1]["messages"].as_array().unwrap();
            assert_eq!(messages.len(), 4);
            assert_eq!(
                messages[1],
                serde_json::json!({"role":"user","content":"🔤 The first public caption for the model. 🔤"})
            );
            assert_eq!(
                messages[2],
                serde_json::json!({"role":"assistant","content":"模型第一句"})
            );
            assert_eq!(
                messages[3],
                // The fixed source keeps the space after the previous EOS.
                serde_json::json!({"role":"user","content":"🔤  The next public caption carries context. 🔤"})
            );
        } else {
            assert_eq!(
                requests[1]["input"],
                "🔤 The first public caption for the model. 🔤 → 模型第一句\n🔤  The next public caption carries context. 🔤"
            );
            assert!(requests[1].get("messages").is_none());
        }
    }
    controller.history_requested(1);
    until(&mut controller, &jobs, |_| {
        ui.get_history_rows().row_count() == 6
    });
    assert_eq!(
        ui.get_history_rows().row_data(0).unwrap().translation,
        "模型第二句"
    );
    controller.test_caption_fault("公开测试连接故障");
    assert_eq!(ui.get_caption_notice(), "系统字幕已停止：公开测试连接故障");
    assert!(ui.get_source_caption().is_empty());
    assert!(ui.get_translated_caption().is_empty());
    assert_eq!(ui.get_history_rows().row_count(), 6);
    controller.test_stop_engine();
    assert!(ui.get_caption_notice().is_empty());
    controller.main_closed();
    until(&mut controller, &jobs, |controller| {
        controller.test_data_stopped()
    });
    drop(controller);
    history_fixture(&root, screen, &ui, &jobs, &mut main_view);
    caption_interaction::run(&root, screen, &ui, &jobs, &mut main_view);
    file_lifecycle_fixture(&root, screen, &ui, &jobs);
    deletion::run(&root, screen, &ui, &jobs, &mut main_view);
    main_geometry::run(&root, screen, &ui, &jobs);
    overlay_geometry::run(&root, screen, &ui, &jobs);
    startup::run(&root, screen, &ui, &jobs);
    caption_toggle::run(&root, screen, &ui, &jobs);
    capture_cutoff::run(&root, screen, &ui, &jobs);
    original_number_input(&ui, &mut main_view);
    translation_dropdown(&ui, &mut main_view);
    interval_slider(&ui, &mut main_view);
    let actual = root.canonicalize().unwrap();
    assert_eq!(actual.parent(), Some(parent.as_path()));
    fs::remove_dir_all(actual).unwrap();
}

#[test]
fn settings_layout_across_window_sizes() {
    let jobs = TestAppContext::single();
    jobs.update(|cx| {
        gpui_component::init(cx);
        crate::ui::runtime::start(cx);
    });
    let ui = MainWindow::new().unwrap();
    ui.set_data_state(2);
    ui.set_page(1);
    ui.show().unwrap();
    let mut window = VisualTestContext::from_window(ui.window().test_handle(), &jobs);
    ui.set_context_aware(false);
    redraw(&mut window, 900., 600.);
    assert!(window.debug_bounds("悬浮字幕前文句数").is_some());
    assert!(window.debug_bounds("上下文句数").is_none());
    ui.set_context_aware(true);
    for width in [480., 800., 900., 1280.] {
        redraw(&mut window, width, 600.);
        let contexts = window.debug_bounds("上下文句数").unwrap();
        let sentences = window.debug_bounds("悬浮字幕前文句数").unwrap();
        assert!(contexts.top() > sentences.bottom());
        assert_eq!(contexts.right(), sentences.right());
        let service = window.debug_bounds("service").unwrap();
        let language = window.debug_bounds("target-language").unwrap();
        let slider = window.debug_bounds("interval-slider").unwrap();
        assert!(service.bottom() < language.top());
        assert_eq!(service.right(), language.right());
        assert_eq!(slider.right(), language.right());
    }
    redraw(&mut window, 900., 360.);
    let field = window.debug_bounds("悬浮字幕前文句数").unwrap();
    let service = window.debug_bounds("service").unwrap();
    let position = point(field.left() - px(30.), service.center().y);
    window.simulate_mouse_move(position, None, Modifiers::none());
    window.simulate_event(gpui::ScrollWheelEvent {
        position,
        delta: gpui::ScrollDelta::Pixels(point(px(0.), px(-120.))),
        ..Default::default()
    });
    redraw(&mut window, 900., 360.);
    let visible = window.debug_bounds("悬浮字幕前文句数").unwrap();
    assert!(
        visible.top() < field.top(),
        "no page scroll at {position:?}: {field:?} -> {visible:?}"
    );
    assert!(visible.center().y < px(360.) && visible.center().y > px(30.));
    window.simulate_mouse_move(visible.center(), None, Modifiers::none());
    window.simulate_event(gpui::ScrollWheelEvent {
        position: visible.center(),
        delta: gpui::ScrollDelta::Pixels(point(px(0.), px(60.))),
        ..Default::default()
    });
    redraw(&mut window, 900., 360.);
    let after = window.debug_bounds("悬浮字幕前文句数").unwrap();
    assert!(
        after.top() > visible.top(),
        "no scroll over input: {visible:?} -> {after:?}"
    );
    ui.set_page(0);
    redraw(&mut window, 480., 240.);
    let source = window.debug_bounds("source").unwrap();
    let translation = window.debug_bounds("translation").unwrap();
    assert!(source.top() >= px(48.));
    assert!(translation.top() - source.bottom() >= px(12.));
}

fn original_number_input(ui: &MainWindow, window: &mut VisualTestContext) {
    ui.set_context_aware(true);
    ui.set_data_state(2);
    ui.set_data_message(String::new());
    ui.set_retry_save(false);
    ui.set_page(1);
    ui.set_num_contexts(2);
    ui.set_display_sentences(1);
    redraw(window, 900., 600.);
    let position = center(window, "contexts-1");
    window.simulate_mouse_move(position, None, Modifiers::none());
    window.simulate_mouse_down(position, MouseButton::Left, Modifiers::none());
    assert_eq!(
        ui.get_num_contexts(),
        3,
        "press increments once immediately"
    );
    window.executor().advance_clock(Duration::from_millis(550));
    assert_eq!(ui.get_num_contexts(), 4, "held button begins repeating");
    window.executor().advance_clock(Duration::from_millis(70));
    assert_eq!(ui.get_num_contexts(), 5, "held button continues repeating");
    window.simulate_mouse_up(position, MouseButton::Left, Modifiers::none());
    window.executor().advance_clock(Duration::from_millis(70));
    assert_eq!(
        ui.get_num_contexts(),
        5,
        "release stops without an extra step"
    );
    let position = center(window, "sentences-1");
    window.simulate_click(position, Modifiers::none());
    assert_eq!(ui.get_display_sentences(), 2);
}

fn history_fixture(
    root: &std::path::Path,
    screen: Screen,
    ui: &MainWindow,
    jobs: &TestAppContext,
    window: &mut VisualTestContext,
) {
    let directory = root.join("history-data");
    fs::create_dir(&directory).unwrap();
    ui.set_data_state(0);
    ui.set_history_query("".into());
    let data = Data::start(directory.clone(), screen, ui.as_weak()).unwrap();
    let (bridge, _host) = Bridge::fixture();
    let mut controller = Controller::new(bridge, ui.as_weak(), data, screen);
    until(&mut controller, jobs, |_| ui.get_data_state() == 2);
    let mut fixture = rusqlite::Connection::open(directory.join("translation_history.db")).unwrap();
    fixture.busy_timeout(Duration::from_secs(3)).unwrap();
    let transaction = fixture.transaction().unwrap();
    for index in 1..=10000 {
        let source = if index <= 31 {
            format!("thirty-one {index:05}")
        } else {
            format!("public history {index:05}")
        };
        let translation = if index == 2 {
            "中文 😀, \"quoted\"\nline two\r\nline three"
        } else if index >= 9991 {
            "needle"
        } else {
            "公开测试译文"
        };
        transaction.execute("INSERT INTO TranslationHistory(Timestamp,SourceText,TranslatedText,TargetLanguage,ApiUsed) VALUES('1700000000',?1,?2,'zh-CN','Fixture')",rusqlite::params![source,translation]).unwrap();
    }
    transaction.commit().unwrap();
    controller.test_show_history();
    let owner = Rc::new(std::cell::RefCell::new(controller));
    Controller::bind_history(&owner);
    Controller::bind_files(&owner);
    owner.borrow_mut().test_hidden_file_dialogs();
    let settled = || owner.borrow().test_history_idle();
    until_owner(&owner, jobs, settled);
    assert_eq!(ui.get_history_total(), "10000");
    assert_eq!(ui.get_history_pages(), 334);
    assert_eq!(ui.get_history_rows().row_count(), 30);
    assert_eq!(
        ui.get_history_rows().row_data(0).unwrap().source,
        "public history 10000"
    );
    let row = ui.get_history_rows().row_data(0).unwrap();
    assert_eq!(row.stamp.len(), 16);
    assert_eq!(row.stamp_full.len(), 19);
    for dark in [false, true] {
        ui.set_frame_dark(dark);
        redraw(window, 800., 420.);
    }
    ui.invoke_history_requested(3);
    until_owner(&owner, jobs, settled);
    assert_eq!(
        ui.get_history_rows().row_data(0).unwrap().source,
        "public history 09940"
    );
    ui.set_history_query("needle".into());
    ui.invoke_history_requested(4);
    until_owner(&owner, jobs, settled);
    assert_eq!(ui.get_history_total(), "10000", "typing is only a draft");
    assert_eq!(
        ui.get_history_rows().row_data(0).unwrap().source,
        "public history 09910"
    );
    ui.invoke_history_action(0, 0);
    until_owner(&owner, jobs, settled);
    assert_eq!(ui.get_history_total(), "10");
    assert_eq!(ui.get_history_page(), 1);
    // Actual UI callback + real native STA/hidden HWND, with an injected path
    // in place of visible Shell interaction. Export must ignore this filter.
    ui.invoke_file_requested(3);
    assert_eq!(owner.borrow().test_windows(), 2);
    ui.invoke_file_requested(3);
    assert_eq!(owner.borrow().test_windows(), 2, "one file dialog only");
    owner.borrow_mut().test_select_file(Ok(None));
    owner.borrow_mut().wakeup();
    assert!(!ui.get_file_busy());
    assert!(!ui.get_export_busy());
    assert_eq!(owner.borrow().test_windows(), 1);
    let csv_evidence = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../evidence/functional")
        .join(format!(
            "csv-controller-{}",
            windows_platform::desktop::instance_id().unwrap()
        ));
    fs::create_dir_all(&csv_evidence).unwrap();
    let csv_target = csv_evidence.join("history.csv");
    assert!(!csv_target.exists(), "cancel created no export");
    ui.invoke_file_requested(3);
    owner
        .borrow_mut()
        .test_select_file(Ok(Some(csv_target.clone())));
    owner.borrow_mut().wakeup();
    until_owner(&owner, jobs, || !ui.get_export_busy());
    assert!(
        ui.get_export_message()
            .starts_with("已导出全部 10000 条历史")
    );
    assert!(
        fs::read(&csv_target)
            .unwrap()
            .starts_with(&[0xef, 0xbb, 0xbf])
    );
    fixture
        .backup(rusqlite::MAIN_DB, csv_evidence.join("fixture.db"), None)
        .unwrap();
    fs::write(csv_evidence.join("scope.json"),br#"{"scope":"real worker storage export, virtual UI, injected native selection; no visible dialog acceptance","expected_rows":10000,"search_rows":10,"page_size":30}"#).unwrap();
    println!("CSV controller evidence: {}", csv_evidence.display());
    for name in ["old-setting.JSON", "old-translation_history.db"] {
        let protected = root.join(name);
        fs::write(&protected, b"unique old fixture bytes").unwrap();
        ui.invoke_file_requested(3);
        owner
            .borrow_mut()
            .test_select_file(Ok(Some(protected.clone())));
        owner.borrow_mut().wakeup();
        assert!(
            ui.get_export_message()
                .contains("配置和数据库文件不会被覆盖")
        );
        assert_eq!(fs::read(protected).unwrap(), b"unique old fixture bytes");
        assert!(!ui.get_export_busy());
    }
    ui.invoke_file_requested(3);
    owner
        .borrow_mut()
        .test_select_file(Err("受控文件选择失败".into()));
    owner.borrow_mut().wakeup();
    assert!(ui.get_export_message().contains("受控文件选择失败"));
    assert!(!ui.get_export_busy());
    ui.set_history_query("unsubmitted text".into());
    ui.invoke_history_requested(1);
    until_owner(&owner, jobs, settled);
    assert_eq!(ui.get_history_total(), "10");
    ui.invoke_history_action(1, 0);
    until_owner(&owner, jobs, settled);
    assert_eq!(ui.get_history_query(), "");
    assert_eq!(ui.get_history_page(), 4);
    assert_eq!(ui.get_history_total(), "10000");
    for (size, pages) in [(20, 500), (50, 200), (100, 100), (30, 334)] {
        ui.invoke_history_action(3, size);
        until_owner(&owner, jobs, settled);
        assert_eq!(ui.get_history_rows().row_count(), size as usize);
        assert_eq!(ui.get_history_pages(), pages);
        assert_eq!(ui.get_history_page(), 1);
    }
    ui.set_history_query("thirty-one".into());
    ui.invoke_history_action(0, 0);
    until_owner(&owner, jobs, settled);
    assert_eq!(ui.get_history_total(), "31");
    assert_eq!(ui.get_history_pages(), 2);
    ui.invoke_history_requested(2);
    until_owner(&owner, jobs, settled);
    assert_eq!(ui.get_history_rows().row_count(), 1);
    assert_eq!(
        ui.get_history_rows().row_data(0).unwrap().source,
        "thirty-one 00001"
    );
    ui.set_history_query("%".into());
    ui.invoke_history_action(0, 0);
    until_owner(&owner, jobs, settled);
    assert_eq!(
        ui.get_history_total(),
        "10000",
        "LIKE wildcards stay supported"
    );
    ui.set_history_query("' OR 1=1 --".into());
    ui.invoke_history_action(0, 0);
    until_owner(&owner, jobs, settled);
    assert_eq!(
        ui.get_history_total(),
        "0",
        "query text is a bound parameter"
    );
    assert_eq!((ui.get_history_page(), ui.get_history_pages()), (1, 1));
    ui.invoke_history_action(1, 0);
    until_owner(&owner, jobs, settled);
    // Do not service the event loop between clicks: only first and final view
    // may be queried, even when the database completes the first immediately.
    let before = owner.borrow().test_history_request();
    for page in 1..=100 {
        ui.invoke_history_requested(page);
    }
    assert_eq!(owner.borrow().test_history_request(), before + 1);
    until_owner(&owner, jobs, settled);
    assert_eq!(owner.borrow().test_history_request(), before + 2);
    assert_eq!(ui.get_history_page(), 100);
    assert_eq!(
        ui.get_history_rows().row_data(0).unwrap().source,
        "public history 07030"
    );
    ui.invoke_history_requested(334);
    ui.invoke_page_changed(1);
    until_owner(&owner, jobs, settled);
    let hidden_request = owner.borrow().test_history_request();
    let hidden_source = ui.get_history_rows().row_data(0).unwrap().source;
    owner.borrow_mut().test_start_network_fixture(
        providers::Translator::local_fixture("Google", "http://127.0.0.1:1".into()),
        "Google",
    );
    ui.set_log_only(true);
    owner.borrow_mut().pause_changed(true);
    owner
        .borrow_mut()
        .test_capture("A new public caption while history is hidden.");
    let count = || {
        fixture
            .query_row("SELECT COUNT(*) FROM TranslationHistory", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap()
    };
    until_owner(&owner, jobs, || count() == 10001);
    for _ in 0..10 {
        owner.borrow_mut().wakeup();
    }
    assert_eq!(owner.borrow().test_history_request(), hidden_request);
    assert_eq!(
        ui.get_history_rows().row_data(0).unwrap().source,
        hidden_source,
        "late hidden result is discarded"
    );
    ui.invoke_page_changed(2);
    ui.invoke_history_requested(1);
    until_owner(&owner, jobs, settled);
    assert_eq!(ui.get_history_total(), "10001");
    assert_eq!(
        ui.get_history_rows().row_data(0).unwrap().service,
        "LogOnly"
    );
    owner.borrow_mut().test_capture("A new public caption while history is hidden. A second public caption while history is visible.");
    until_owner(&owner, jobs, || {
        ui.get_history_total() == "10002" && settled()
    });
    assert_eq!(
        ui.get_history_rows().row_data(0).unwrap().translation,
        "N/A"
    );
    let visible_request = owner.borrow().test_history_request();
    for _ in 0..100 {
        owner.borrow_mut().wakeup();
    }
    assert_eq!(
        owner.borrow().test_history_request(),
        visible_request,
        "no polling while unchanged"
    );
    ui.invoke_history_requested(334);
    until_owner(&owner, jobs, settled);
    ui.set_history_query("needle".into());
    ui.invoke_history_action(0, 0);
    until_owner(&owner, jobs, settled);
    // Simulate the data set shrinking using only this isolated fixture DB.
    // This is not the application's Delete feature or its acceptance test.
    fixture
        .execute("DELETE FROM TranslationHistory WHERE Id > 31", [])
        .unwrap();
    ui.invoke_history_action(1, 0);
    until_owner(&owner, jobs, settled);
    assert_eq!(ui.get_history_total(), "31");
    assert_eq!(
        ui.get_history_page(),
        2,
        "restored pre-search page is clamped"
    );
    fixture.execute("INSERT INTO TranslationHistory(Timestamp,SourceText) VALUES('1700000000','manual fixture refresh')",[]).unwrap();
    ui.invoke_history_action(2, 0);
    until_owner(&owner, jobs, settled);
    assert_eq!(ui.get_history_total(), "32");
    drop(fixture);
    let mut controller = Rc::try_unwrap(owner).ok().unwrap().into_inner();
    controller.test_stop_engine();
    ui.set_log_only(false);
    controller.main_closed();
    until(&mut controller, jobs, |value| value.test_data_stopped());
    println!(
        "History UI: isolated 10000 rows; capacities, draft/applied search, restore/clamp, parameterization, bounded latest view, visible-only durable-change refresh PASS (headless GPUI)"
    );
}

fn file_lifecycle_fixture(
    root: &std::path::Path,
    screen: Screen,
    ui: &MainWindow,
    jobs: &TestAppContext,
) {
    let empty = root.join("cancelled-file-data");
    fs::create_dir(&empty).unwrap();
    ui.set_data_state(0);
    let data = Data::start(empty.clone(), screen, ui.as_weak()).unwrap();
    let (bridge, _host) = Bridge::fixture();
    let mut controller = Controller::new(bridge, ui.as_weak(), data, screen);
    controller.test_show_settings();
    until(&mut controller, jobs, |_| ui.get_data_state() == 2);
    assert_eq!(
        controller.test_pending_start_and_engine(),
        (false, protocol::EngineState::Disabled)
    );
    controller.test_hidden_file_dialogs();
    let owner = Rc::new(std::cell::RefCell::new(controller));
    Controller::bind_files(&owner);
    ui.invoke_file_requested(0);
    assert_eq!(
        owner.borrow().test_windows(),
        1,
        "removed import cannot open a dialog"
    );
    assert!(!ui.get_file_busy());
    ui.hide().unwrap();
    owner.borrow_mut().main_closed();
    until_owner(&owner, jobs, || owner.borrow().test_data_stopped());
    assert!(empty.join("setting.json").exists());
    assert!(empty.join("translation_history.db").exists());
    drop(owner);

    let legacy = root.join("legacy-picker-source");
    fs::create_dir(&legacy).unwrap();
    let mut settings = captions_core::settings::Settings::defaults(screen);
    settings = settings.with_prompt("Imported fixture {0}").unwrap();
    let original_config = serde_json::to_vec(settings.json()).unwrap();
    fs::write(legacy.join("setting.json"), &original_config).unwrap();
    let source_db = legacy.join("translation_history.db");
    let fixture = rusqlite::Connection::open(&source_db).unwrap();
    fixture.execute_batch(storage::history::SCHEMA).unwrap();
    fixture.execute("INSERT INTO TranslationHistory(Timestamp,SourceText,TranslatedText) VALUES('1700000000','public import fixture','公开导入夹具')",[]).unwrap();
    drop(fixture);
    let original_db = fs::read(&source_db).unwrap();
    // The ordinary importer is gone, but an already-interrupted migration must
    // still be recoverable when its manifest was not written before shutdown.
    let interrupted = root.join("interrupted-existing-recovery");
    fs::create_dir(&interrupted).unwrap();
    fs::create_dir(storage::migrate::stage_path(&interrupted).unwrap()).unwrap();
    ui.set_data_state(0);
    let data = Data::start(interrupted, screen, ui.as_weak()).unwrap();
    let (bridge, _recovery_host) = Bridge::fixture();
    let mut controller = Controller::new(bridge, ui.as_weak(), data, screen);
    until(&mut controller, jobs, |_| ui.get_data_state() == 3);
    controller.test_show_settings();
    controller.test_hidden_file_dialogs();
    let recovery_owner = Rc::new(std::cell::RefCell::new(controller));
    Controller::bind_files(&recovery_owner);
    ui.invoke_file_requested(2);
    assert!(ui.get_file_busy());
    recovery_owner
        .borrow_mut()
        .test_select_file(Ok(Some(legacy.clone())));
    recovery_owner.borrow_mut().wakeup();
    until_owner(&recovery_owner, jobs, || ui.get_data_state() == 2);
    assert_eq!(
        recovery_owner.borrow().test_settings().json()["Prompt"],
        "Imported fixture {0}"
    );
    recovery_owner.borrow_mut().main_closed();
    until_owner(&recovery_owner, jobs, || {
        recovery_owner.borrow().test_data_stopped()
    });
    drop(recovery_owner);
    let directory = root.join("selected-file-data");
    fs::create_dir(&directory).unwrap();
    fs::write(directory.join("setting.json"), &original_config).unwrap();
    fs::write(directory.join("translation_history.db"), &original_db).unwrap();
    ui.set_data_state(0);
    let data = Data::start(directory.clone(), screen, ui.as_weak()).unwrap();
    let (bridge, _import_host) = Bridge::fixture();
    let mut controller = Controller::new(bridge, ui.as_weak(), data, screen);
    until(&mut controller, jobs, |_| ui.get_data_state() == 2);
    controller.test_show_settings();
    controller.test_hidden_file_dialogs();
    let owner = Rc::new(std::cell::RefCell::new(controller));
    Controller::bind_files(&owner);
    assert_eq!(
        owner.borrow().test_settings().json()["Prompt"],
        "Imported fixture {0}"
    );
    assert_eq!(
        fs::read(legacy.join("setting.json")).unwrap(),
        original_config
    );
    assert_eq!(fs::read(&source_db).unwrap(), original_db);
    // AppExit posts cancellation and keeps pumping until the native STA has
    // released its windows. Only then may storage/worker shutdown complete.
    ui.invoke_file_requested(3);
    assert!(ui.get_file_busy());
    let started = Instant::now();
    owner.borrow_mut().test_host_exit();
    assert!(started.elapsed() < Duration::from_secs(2));
    assert!(
        ui.get_file_busy(),
        "Exit must keep the job until its completion event is processed"
    );
    assert!(!owner.borrow().test_data_stopped());
    until_owner(&owner, jobs, || owner.borrow().test_data_stopped());
    assert!(!ui.get_file_busy());
    drop(owner);

    let settings_path = directory.join("setting.json");
    fs::write(&settings_path, b"invalid fixture JSON").unwrap();
    let replacement = root.join("replacement-fixture.json");
    settings = settings
        .with_prompt("Recovered picker fixture {0}")
        .unwrap();
    let replacement_bytes = serde_json::to_vec(settings.json()).unwrap();
    fs::write(&replacement, &replacement_bytes).unwrap();
    ui.set_data_state(0);
    let data = Data::start(directory.clone(), screen, ui.as_weak()).unwrap();
    let (bridge, _recover_host) = Bridge::fixture();
    let mut controller = Controller::new(bridge, ui.as_weak(), data, screen);
    until(&mut controller, jobs, |_| ui.get_data_state() == 4);
    controller.test_show_settings();
    controller.test_hidden_file_dialogs();
    let owner = Rc::new(std::cell::RefCell::new(controller));
    Controller::bind_files(&owner);
    ui.invoke_file_requested(1);
    owner
        .borrow_mut()
        .test_select_file(Ok(Some(replacement.clone())));
    owner.borrow_mut().wakeup();
    until_owner(&owner, jobs, || ui.get_data_state() == 2);
    assert_eq!(
        owner.borrow().test_settings().json()["Prompt"],
        "Recovered picker fixture {0}"
    );
    assert_eq!(fs::read(replacement).unwrap(), replacement_bytes);
    let lock = rusqlite::Connection::open(directory.join("translation_history.db")).unwrap();
    lock.execute_batch("BEGIN EXCLUSIVE").unwrap();
    let target = root.join("preserved-on-error.CSV");
    fs::write(&target, b"old complete CSV fixture").unwrap();
    let preserved_bytes = fs::read(&target).unwrap();
    ui.invoke_file_requested(3);
    owner
        .borrow_mut()
        .test_select_file(Ok(Some(target.clone())));
    owner.borrow_mut().wakeup();
    assert!(ui.get_export_busy());
    owner.borrow_mut().test_host_exit();
    assert!(
        !owner.borrow().test_data_stopped(),
        "export acknowledgement precedes shutdown"
    );
    until_owner(&owner, jobs, || {
        ui.get_export_message().starts_with("历史导出未完成")
    });
    assert!(ui.window().is_visible(), "failed export remains reviewable");
    assert!(!owner.borrow().test_data_stopped());
    assert_eq!(fs::read(&target).unwrap(), preserved_bytes);
    assert!(
        !fs::read_dir(root).unwrap().any(|entry| entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".preserved-on-error.CSV.")),
        "no half file after failed export"
    );
    lock.execute_batch("ROLLBACK").unwrap();
    drop(lock);
    owner
        .borrow_mut()
        .prompt_changed("Unrelated setting after export failure {0}");
    until_owner(&owner, jobs, || ui.get_data_message() == "设置已保存。");
    assert!(
        ui.get_export_message().starts_with("历史导出未完成"),
        "an unrelated settings acknowledgement cannot hide the export failure"
    );
    // Retry chooses a CSV target again; uppercase suffix is accepted.
    ui.invoke_file_requested(3);
    owner
        .borrow_mut()
        .test_select_file(Ok(Some(target.clone())));
    owner.borrow_mut().wakeup();
    until_owner(&owner, jobs, || !ui.get_export_busy());
    assert!(ui.get_export_message().starts_with("已导出全部 1 条历史"));
    assert!(fs::read(target).unwrap().starts_with(&[0xef, 0xbb, 0xbf]));
    owner.borrow_mut().main_closed();
    until_owner(&owner, jobs, || owner.borrow().test_data_stopped());
    println!(
        "File controller: removed import cannot open picker; existing settings/history, config recovery, dialog AppExit, protected JSON/DB, failed export while exiting preserves target and reopens UI PASS"
    );
}

fn translation_dropdown(ui: &MainWindow, window: &mut VisualTestContext) {
    ui.set_selected_service("Google".into());
    redraw(window, 900., 600.);
    let button = window.debug_bounds("service").unwrap();
    window.simulate_click(button.center(), Modifiers::none());
    redraw(window, 900., 600.);
    let popup = window.debug_bounds("service-popup").unwrap();
    assert_eq!(button.size.width, popup.size.width);
    let option = center(window, "service-option-1");
    window.simulate_click(option, Modifiers::none());
    assert_eq!(ui.get_selected_service(), "Google2");
    redraw(window, 900., 600.);
    assert!(window.debug_bounds("service-popup").is_none());
}

fn interval_slider(ui: &MainWindow, window: &mut VisualTestContext) {
    redraw(window, 900., 600.);
    let track = window.debug_bounds("interval-slider").unwrap();
    let position = point(
        track.left() + px(10.) + (track.size.width - px(20.)) * 0.7,
        track.center().y,
    );
    window.simulate_click(position, Modifiers::none());
    assert_eq!(ui.get_max_sync_interval(), 7);
}
