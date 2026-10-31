//! PascalCase compatibility document. Unknown fields at every level survive
//! normalization and later saves; diagnostics contain field names, not secrets.
use crate::geometry::{Anchor, Placement};
use serde::Serialize;
use serde_json::{Map, Value, json};
use std::fmt;

#[derive(Clone, Copy)]
pub enum WindowKind {
    Main,
    Overlay,
}
impl WindowKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Main => "MainWindow",
            Self::Overlay => "OverlayWindow",
        }
    }
}

pub const SERVICES: [&str; 11] = [
    "Google",
    "Google2",
    "Ollama",
    "OpenAI",
    "LMStudio",
    "OpenRouter",
    "DeepL",
    "Youdao",
    "Baidu",
    "MTranServer",
    "LibreTranslate",
];
#[derive(Clone, Copy)]
pub struct Screen {
    width: f64,
    height: f64,
}
impl Screen {
    pub fn new(width: f64, height: f64) -> Option<Self> {
        (width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0)
            .then_some(Self { width, height })
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}
impl Bounds {
    pub fn parse(text: &str) -> Option<Self> {
        let values: Vec<f64> = text
            .split(',')
            .map(|part| part.trim().parse())
            .collect::<Result<_, _>>()
            .ok()?;
        if values.len() != 4
            || values.iter().any(|value| !value.is_finite())
            || values[2] <= 0.0
            || values[3] <= 0.0
        {
            return None;
        }
        Some(Self {
            x: values[0],
            y: values[1],
            width: values[2],
            height: values[3],
        })
    }
    pub fn encode(self) -> String {
        format!("{}, {}, {}, {}", self.x, self.y, self.width, self.height)
    }
}
#[derive(Default, Serialize)]
pub struct RepairReport {
    pub fields: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_api_name: Option<String>,
}
impl RepairReport {
    fn record(&mut self, field: &str) {
        if !self.fields.iter().any(|item| item == field) {
            self.fields.push(field.into());
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsError {
    InvalidJson { line: usize, column: usize },
    NotObject,
    InvalidPrompt,
}
impl fmt::Display for SettingsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidJson { line, column } => {
                write!(f, "配置文件格式错误（第 {line} 行，第 {column} 列）。")
            }
            Self::NotObject => f.write_str("配置文件顶层必须是对象。"),
            Self::InvalidPrompt => f.write_str("提示词仅支持 {0}、{{ 和 }}，请检查括号。"),
        }
    }
}
impl std::error::Error for SettingsError {}

pub fn render_prompt(template: &str, language: &str) -> Result<String, SettingsError> {
    let mut chars = template.chars().peekable();
    let mut output = String::with_capacity(template.len() + language.len());
    while let Some(ch) = chars.next() {
        match ch {
            '{' => match chars.next() {
                Some('{') => output.push('{'),
                Some('0') if chars.next() == Some('}') => output.push_str(language),
                _ => return Err(SettingsError::InvalidPrompt),
            },
            '}' => {
                if chars.next() == Some('}') {
                    output.push('}');
                } else {
                    return Err(SettingsError::InvalidPrompt);
                }
            }
            ch => output.push(ch),
        }
    }
    Ok(output)
}

#[derive(Clone, PartialEq)]
pub struct Settings {
    document: Value,
}
fn defaults(screen: Screen) -> Value {
    let mut document: Value =
        serde_json::from_str(include_str!("../defaults.json")).expect("checked source defaults");
    document["WindowBounds"] = json!({
        "MainWindow":Bounds{x:(screen.width-900.0)/2.0,y:((screen.height-600.0)/2.0).max(0.0),width:900.0,height:240.0}.encode(),
        "OverlayWindow":Bounds{x:(screen.width-650.0)/2.0,y:screen.height*5.0/6.0-135.0,width:650.0,height:135.0}.encode()
    });
    let geometry = json!({"CoordinateUnit":"Logical96","Monitor":"","OffsetX":0.0,"OffsetY":0.0,"ManualSize":false});
    document["RustOptions"] = json!({"SchemaVersion":1,"WindowGeometry":{
        "MainWindow":geometry.clone(),"OverlayWindow":geometry
    }});
    document
}
fn fill(actual: &mut Value, default: &Value, path: &str, report: &mut RepairReport) {
    let compatible = match default {
        Value::Object(_) => actual.is_object(),
        Value::Array(_) => actual.is_array(),
        Value::String(_) => actual.is_string(),
        Value::Bool(_) => actual.is_boolean(),
        Value::Number(_) => actual.is_number(),
        Value::Null => actual.is_null() || actual.is_string(),
    };
    if !compatible {
        *actual = default.clone();
        report.record(path);
        return;
    }
    if let (Some(actual), Some(default)) = (actual.as_object_mut(), default.as_object()) {
        for (key, value) in default {
            let path = if path.is_empty() {
                key.clone()
            } else {
                format!("{path}.{key}")
            };
            if let Some(actual) = actual.get_mut(key) {
                fill(actual, value, &path, report);
            } else {
                actual.insert(key.clone(), value.clone());
                report.record(&path);
            }
        }
    }
}
fn integer(
    document: &mut Value,
    path: &str,
    min: i64,
    max: i64,
    default: i64,
    report: &mut RepairReport,
) {
    let value = document.pointer_mut(path).expect("defaulted integer field");
    let number = value
        .as_i64()
        .or_else(|| {
            value
                .as_f64()
                .filter(|number| {
                    number.fract() == 0.0
                        && *number >= i64::MIN as f64
                        && *number <= i64::MAX as f64
                })
                .map(|number| number as i64)
        })
        .unwrap_or(default)
        .clamp(min, max);
    if *value != json!(number) {
        *value = json!(number);
        report.record(&path[1..].replace('/', "."));
    }
}
fn decimal(
    document: &mut Value,
    path: &str,
    min: f64,
    max: f64,
    default: f64,
    report: &mut RepairReport,
) {
    let value = document.pointer_mut(path).expect("defaulted decimal field");
    let number = value
        .as_f64()
        .filter(|number| number.is_finite())
        .unwrap_or(default)
        .clamp(min, max);
    if value.as_f64() != Some(number) {
        *value = json!(number);
        report.record(&path[1..].replace('/', "."));
    }
}
impl Settings {
    pub fn defaults(screen: Screen) -> Self {
        Self {
            document: defaults(screen),
        }
    }
    pub fn parse(text: &str, screen: Screen) -> Result<(Self, RepairReport), SettingsError> {
        let value = serde_json::from_str(text.strip_prefix('\u{feff}').unwrap_or(text)).map_err(
            |error: serde_json::Error| SettingsError::InvalidJson {
                line: error.line(),
                column: error.column(),
            },
        )?;
        Self::normalize(value, screen)
    }
    pub fn normalize(
        mut document: Value,
        screen: Screen,
    ) -> Result<(Self, RepairReport), SettingsError> {
        if !document.is_object() {
            return Err(SettingsError::NotObject);
        }
        let default = defaults(screen);
        let mut report = RepairReport::default();
        fill(&mut document, &default, "", &mut report);
        for (path, min, max, value) in [
            ("/MaxIdleInterval", 50, 50, 50),
            ("/MaxSyncInterval", 0, 10, 3),
            ("/NumContexts", 1, 10, 2),
            ("/DisplaySentences", 0, 6, 1),
            ("/MainWindow/OriginalFontSize", 8, 40, 15),
            ("/MainWindow/TranslatedFontSize", 8, 40, 18),
            ("/OverlayWindow/FontSize", 8, 40, 15),
            ("/OverlayWindow/FontColor", 1, 8, 1),
            ("/OverlayWindow/BackgroundColor", 1, 8, 8),
            ("/OverlayWindow/FontBold", 0, 3, 0),
            ("/OverlayWindow/Opacity", 1, 251, 150),
        ] {
            integer(&mut document, path, min, max, value, &mut report);
        }
        if document["DisplaySentences"].as_u64() > document["NumContexts"].as_u64() {
            document["DisplaySentences"] = document["NumContexts"].clone();
            report.record("DisplaySentences");
        }
        decimal(
            &mut document,
            "/OverlayWindow/FontStroke",
            0.0,
            7.5,
            0.0,
            &mut report,
        );
        if !SERVICES.contains(&document["ApiName"].as_str().unwrap()) {
            report.previous_api_name = Some(document["ApiName"].as_str().unwrap().into());
            document["ApiName"] = json!("Google");
            report.record("ApiName");
        }
        if document["TargetLanguage"]
            .as_str()
            .unwrap()
            .trim()
            .is_empty()
        {
            document["TargetLanguage"] = default["TargetLanguage"].clone();
            report.record("TargetLanguage");
        }
        if render_prompt(document["Prompt"].as_str().unwrap(), "").is_err() {
            document["Prompt"] = default["Prompt"].clone();
            report.record("Prompt");
        }
        for service in SERVICES {
            let path = format!("Configs.{service}");
            let config_default = &default["Configs"][service][0];
            let configs = document["Configs"][service].as_array_mut().unwrap();
            if configs.is_empty() {
                configs.push(config_default.clone());
                report.record(&path);
            }
            for (index, config) in configs.iter_mut().enumerate() {
                fill(
                    config,
                    config_default,
                    &format!("{path}[{index}]"),
                    &mut report,
                );
            }
            let maximum = configs.len() - 1;
            integer(
                &mut document,
                &format!("/ConfigIndices/{service}"),
                0,
                maximum as i64,
                0,
                &mut report,
            );
            for index in 0..=maximum {
                let base = format!("/Configs/{service}/{index}");
                if config_default.get("Temperature").is_some() {
                    decimal(
                        &mut document,
                        &format!("{base}/Temperature"),
                        0.0,
                        2.0,
                        1.0,
                        &mut report,
                    );
                }
                if config_default.get("Keep_alive").is_some() {
                    integer(
                        &mut document,
                        &format!("{base}/Keep_alive"),
                        0,
                        i64::MAX,
                        600,
                        &mut report,
                    );
                }
            }
        }
        for name in ["MainWindow", "OverlayWindow"] {
            if Bounds::parse(document["WindowBounds"][name].as_str().unwrap()).is_none() {
                document["WindowBounds"][name] = default["WindowBounds"][name].clone();
                report.record(&format!("WindowBounds.{name}"));
            }
        }
        Ok((Self { document }, report))
    }
    pub fn json(&self) -> &Value {
        &self.document
    }
    pub fn api_name(&self) -> &str {
        self.document["ApiName"].as_str().unwrap()
    }
    pub fn target_language(&self) -> &str {
        self.document["TargetLanguage"].as_str().unwrap()
    }
    pub fn configs(&self, service: &str) -> Option<&[Value]> {
        self.document["Configs"]
            .get(service)?
            .as_array()
            .map(Vec::as_slice)
    }
    pub fn active_config(&self) -> &Map<String, Value> {
        let service = self.api_name();
        let index = self.document["ConfigIndices"][service].as_u64().unwrap() as usize;
        self.document["Configs"][service][index]
            .as_object()
            .unwrap()
    }
    pub fn edited(
        &self,
        screen: Screen,
        edit: impl FnOnce(&mut Value),
    ) -> Result<(Self, RepairReport), SettingsError> {
        let mut document = self.document.clone();
        edit(&mut document);
        Self::normalize(document, screen)
    }
    pub fn window_bounds(&self, kind: WindowKind) -> Bounds {
        Bounds::parse(self.document["WindowBounds"][kind.name()].as_str().unwrap())
            .expect("normalized window bounds")
    }
    pub fn window_anchor(&self, kind: WindowKind) -> Option<Anchor> {
        let metadata = &self.document["RustOptions"]["WindowGeometry"][kind.name()];
        if metadata["CoordinateUnit"].as_str()? != "Logical96" {
            return None;
        }
        let name = metadata["Monitor"].as_str()?;
        let x = metadata["OffsetX"].as_f64()?;
        let y = metadata["OffsetY"].as_f64()?;
        (!name.is_empty() && x.is_finite() && y.is_finite()).then(|| Anchor {
            monitor: name.into(),
            x,
            y,
        })
    }
    pub fn window_manual_size(&self, kind: WindowKind) -> bool {
        self.document["RustOptions"]["WindowGeometry"][kind.name()]["ManualSize"]
            .as_bool()
            .unwrap_or(false)
    }
    pub fn positioned(
        &self,
        screen: Screen,
        kind: WindowKind,
        placement: &Placement,
        manual: bool,
    ) -> Result<(Self, RepairReport), SettingsError> {
        self.edited(screen, |document| {
            document["WindowBounds"][kind.name()] = placement.bounds.encode().into();
            let metadata = &mut document["RustOptions"]["WindowGeometry"][kind.name()];
            metadata["CoordinateUnit"] = "Logical96".into();
            metadata["Monitor"] = placement.anchor.monitor.clone().into();
            metadata["OffsetX"] = placement.anchor.x.into();
            metadata["OffsetY"] = placement.anchor.y.into();
            metadata["ManualSize"] = manual.into();
        })
    }
    pub fn with_prompt(&self, prompt: &str) -> Result<Self, SettingsError> {
        render_prompt(prompt, "")?;
        let mut result = self.clone();
        result.document["Prompt"] = json!(prompt);
        Ok(result)
    }
    pub fn set_num_contexts(&mut self, count: u8) {
        let count = count.clamp(1, 10);
        self.document["NumContexts"] = json!(count);
        let display = self.document["DisplaySentences"]
            .as_u64()
            .unwrap()
            .min(count as u64);
        self.document["DisplaySentences"] = json!(display);
    }
    pub fn set_display_sentences(&mut self, count: u8) {
        let count = count.min(6);
        self.document["DisplaySentences"] = json!(count);
        let contexts = self.document["NumContexts"]
            .as_u64()
            .unwrap()
            .max(count as u64);
        self.document["NumContexts"] = json!(contexts);
    }
    pub fn add_config(&mut self, service: &str) -> bool {
        if !SERVICES.contains(&service) {
            return false;
        }
        let default: Value =
            serde_json::from_str(include_str!("../defaults.json")).expect("source defaults");
        let index = self.document["ConfigIndices"][service].as_u64().unwrap() as usize + 1;
        let array = self.document["Configs"][service].as_array_mut().unwrap();
        array.insert(index, default["Configs"][service][0].clone());
        self.document["ConfigIndices"][service] = json!(index);
        true
    }
    pub fn remove_config(&mut self, service: &str, index: usize) -> bool {
        if !SERVICES.contains(&service) {
            return false;
        }
        let array = self.document["Configs"][service].as_array_mut().unwrap();
        if array.len() <= 1 || index >= array.len() {
            return false;
        }
        array.remove(index);
        let maximum = array.len() - 1;
        let selected = self.document["ConfigIndices"][service].as_u64().unwrap() as usize;
        let selected = if index < selected {
            selected - 1
        } else {
            selected.min(maximum)
        };
        self.document["ConfigIndices"][service] = json!(selected);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn placement_metadata_keeps_legacy_bounds_unknown_fields_and_other_window_independent() {
        let (original, _) = Settings::normalize(json!({
            "WindowBounds":{"MainWindow":"-1000, 50, 775, 360"},
            "RustOptions":{"FutureOption":[1,2],"WindowGeometry":{"MainWindow":{"FutureField":"keep"}}},
            "Configs":{"OpenAI":[{"ApiKey":"fixture-private-key"}]}
        }),screen()).unwrap();
        assert!(original.window_anchor(WindowKind::Main).is_none());
        assert_eq!(original.window_bounds(WindowKind::Main).x, -1000.0);
        let placement = Placement {
            bounds: Bounds {
                x: -1200.0,
                y: 80.0,
                width: 900.0,
                height: 500.0,
            },
            anchor: Anchor {
                monitor: "left-display".into(),
                x: 40.0,
                y: 80.0,
            },
        };
        let other = original.json()["WindowBounds"]["OverlayWindow"].clone();
        let (updated, _) = original
            .positioned(screen(), WindowKind::Main, &placement, true)
            .unwrap();
        let (loaded, _) =
            Settings::parse(&serde_json::to_string(updated.json()).unwrap(), screen()).unwrap();
        assert_eq!(loaded.window_bounds(WindowKind::Main), placement.bounds);
        assert_eq!(
            loaded.window_anchor(WindowKind::Main),
            Some(placement.anchor)
        );
        assert!(loaded.window_manual_size(WindowKind::Main));
        assert!(!loaded.window_manual_size(WindowKind::Overlay));
        assert_eq!(loaded.json()["WindowBounds"]["OverlayWindow"], other);
        assert_eq!(loaded.json()["RustOptions"]["FutureOption"], json!([1, 2]));
        assert_eq!(
            loaded.json()["RustOptions"]["WindowGeometry"]["MainWindow"]["FutureField"],
            "keep"
        );
        assert_eq!(
            loaded.json()["Configs"]["OpenAI"][0]["ApiKey"],
            "fixture-private-key"
        );
    }
    #[test]
    fn adding_configuration_inserts_defaults_after_active_without_reordering_existing_items() {
        let screen = Screen::new(1920.0, 1080.0).unwrap();
        for service in SERVICES {
            let mut settings = Settings::defaults(screen)
                .edited(screen, |value| {
                    let template = value["Configs"][service][0].clone();
                    value["Configs"][service] = Value::Array(
                        (0..3)
                            .map(|index| {
                                let mut config = template.clone();
                                config["FutureLabel"] = json!(index);
                                config
                            })
                            .collect(),
                    );
                    value["ConfigIndices"][service] = json!(1);
                })
                .unwrap()
                .0;
            let original = settings.configs(service).unwrap().to_vec();
            assert!(settings.add_config(service));
            let configs = settings.configs(service).unwrap();
            assert_eq!(settings.json()["ConfigIndices"][service], 2);
            assert_eq!(configs.len(), 4);
            assert_eq!(configs[0], original[0]);
            assert_eq!(configs[1], original[1]);
            assert_eq!(configs[3], original[2]);
            assert_eq!(
                configs[2],
                Settings::defaults(screen).configs(service).unwrap()[0]
            );
        }
    }
    fn screen() -> Screen {
        Screen::new(1920.0, 1080.0).unwrap()
    }
    #[test]
    fn defaults_match_every_source_contract_field_except_screen_computed_bounds() {
        let contract: Value = serde_json::from_str(include_str!(
            "../../../../docs/handoff/Source-Contract.json"
        ))
        .unwrap();
        let settings = Settings::defaults(screen());
        for (key, value) in contract["defaults"].as_object().unwrap() {
            if key != "WindowBounds" {
                assert_eq!(&settings.json()[key], value, "{key}");
            }
        }
        assert_eq!(
            Bounds::parse(
                settings.json()["WindowBounds"]["OverlayWindow"]
                    .as_str()
                    .unwrap()
            )
            .unwrap()
            .width,
            650.0
        );
    }
    #[test]
    fn repairs_preserve_unknown_nested_fields_arrays_indices_and_secrets() {
        let input = json!({"ApiName":"FutureProvider","FutureTop":{"value":7},"NumContexts":1,"DisplaySentences":6,"MainWindow":{"FutureFlag":true,"OriginalFontSize":999},"Configs":{"OpenAI":[{"ApiKey":"private-token","Future":{"keep":true},"Temperature":4.0},null],"Google":[]},"ConfigIndices":{"OpenAI":999},"WindowBounds":{"OverlayWindow":"NaN, 0, -1, 135"}});
        let (settings, report) = Settings::normalize(input, screen()).unwrap();
        assert_eq!(settings.api_name(), "Google");
        assert_eq!(report.previous_api_name.as_deref(), Some("FutureProvider"));
        assert_eq!(settings.json()["FutureTop"]["value"], 7);
        assert_eq!(settings.json()["MainWindow"]["FutureFlag"], true);
        assert_eq!(settings.json()["MainWindow"]["OriginalFontSize"], 40);
        assert_eq!(settings.json()["DisplaySentences"], 1);
        assert_eq!(settings.configs("OpenAI").unwrap().len(), 2);
        assert_eq!(settings.json()["ConfigIndices"]["OpenAI"], 1);
        assert_eq!(
            settings.json()["Configs"]["OpenAI"][0]["ApiKey"],
            "private-token"
        );
        assert_eq!(
            settings.json()["Configs"]["OpenAI"][0]["Future"]["keep"],
            true
        );
        assert_eq!(settings.json()["Configs"]["OpenAI"][0]["Temperature"], 2.0);
        assert!(
            !serde_json::to_string(&report)
                .unwrap()
                .contains("private-token")
        );
        let encoded = serde_json::to_string(settings.json()).unwrap();
        let (roundtrip, _) = Settings::parse(&encoded, screen()).unwrap();
        assert!(roundtrip == settings);
    }
    #[test]
    fn ui_context_coupling_and_config_deletion_keep_the_original_action_direction() {
        let mut settings = Settings::defaults(screen());
        settings.set_display_sentences(6);
        assert_eq!(settings.json()["NumContexts"], 6);
        settings.set_num_contexts(2);
        assert_eq!(settings.json()["DisplaySentences"], 2);
        assert!(!settings.remove_config("Google", 0));
        assert!(settings.add_config("OpenAI"));
        assert!(settings.add_config("OpenAI"));
        assert_eq!(settings.json()["ConfigIndices"]["OpenAI"], 2);
        assert!(settings.remove_config("OpenAI", 0));
        assert_eq!(settings.json()["ConfigIndices"]["OpenAI"], 1);
        assert_eq!(settings.configs("OpenAI").unwrap().len(), 2);
    }
    #[test]
    fn prompt_format_supports_only_target_language_and_escaped_braces() {
        assert_eq!(
            render_prompt("{{Translate}} to {0}", "中文").unwrap(),
            "{Translate} to 中文"
        );
        for bad in ["{1}", "{0:fmt}", "{", "}", "{0"] {
            assert!(render_prompt(bad, "x").is_err());
        }
        let settings = Settings::defaults(screen());
        assert!(settings.with_prompt("bad {1}").is_err());
        assert_eq!(
            settings.json()["Prompt"],
            serde_json::from_str::<Value>(include_str!("../defaults.json")).unwrap()["Prompt"]
        );
        let (custom, _) = settings
            .edited(screen(), |value| {
                value["TargetLanguage"] = json!("custom-language-code")
            })
            .unwrap();
        assert_eq!(custom.target_language(), "custom-language-code");
    }
    #[test]
    fn corrupt_documents_report_positions_without_echoing_values() {
        let error = Settings::parse("{\"ApiKey\":\"private-token\",broken", screen())
            .err()
            .unwrap();
        assert!(!error.to_string().contains("private-token"));
        assert!(Settings::parse("[]", screen()).is_err());
        assert!(Bounds::parse("0,0,-2,100").is_none());
        assert!(Bounds::parse("NaN,0,100,100").is_none());
    }
}
