use super::*;

pub(super) fn run(
    owner: &Rc<std::cell::RefCell<Controller>>,
    api: &crate::ApiWindow,
    window: &mut VisualTestContext,
) {
    let model = |service: &str, index: usize| {
        owner.borrow().test_settings().json()["Configs"][service][index]["ModelName"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    let mut append = |field: &'static str, suffix: &str| {
        redraw(window, 950., 600.);
        let position = center(window, field);
        window.simulate_click(position, Modifiers::none());
        window.simulate_keystrokes("end");
        window.simulate_input(suffix);
    };
    let openai_second = model("OpenAI", 1);
    let lmstudio_second = model("LMStudio", 1);
    api.invoke_section_changed(3);
    append("模型名称", "-typed");
    assert_eq!(model("OpenAI", 1), format!("{openai_second}-typed"));
    api.invoke_section_changed(2);
    append("lm-model-select", "-edited");
    assert_eq!(model("LMStudio", 1), format!("{lmstudio_second}-edited"));
    assert_eq!(model("OpenAI", 1), format!("{openai_second}-typed"));
    api.invoke_section_changed(3);
    api.invoke_config_action(0);
    let openai_first = model("OpenAI", 0);
    append("模型名称", "-first");
    assert_eq!(model("OpenAI", 0), format!("{openai_first}-first"));
    api.invoke_config_action(1);
    append("模型名称", "-second");
    assert_eq!(model("OpenAI", 1), format!("{openai_second}-typed-second"));
    assert_eq!(model("OpenAI", 0), format!("{openai_first}-first"));
    assert_eq!(owner.borrow().test_settings().api_name(), "Google");
    println!(
        "API actual virtual input: service/config switching refreshes edited values and preserves unrelated configurations PASS"
    );
}
