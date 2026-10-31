pub fn target<'a>(service: &str, code: &'a str) -> &'a str {
    const CODES: [&str; 13] = [
        "zh-CN", "zh-TW", "en-US", "en-GB", "ja-JP", "ko-KR", "fr-FR", "th-TH", "ru-RU", "es-ES",
        "pt-BR", "tr-TR", "ar-SA",
    ];
    let Some(index) = CODES.iter().position(|value| *value == code) else {
        return code;
    };
    match service {
        "DeepL" => [
            "ZH-HANS", "ZH-HANT", "EN-US", "EN-GB", "JA", "KO", "FR", "TH", "RU", "ES", "PT-BR",
            "TR", "AR",
        ][index],
        "Youdao" => [
            "zh-CHS", "zh-CHT", "en", "en", "ja", "ko", "fr", "th", "ru", "es", "pt", "tr", "ar",
        ][index],
        "Baidu" => [
            "zh", "cht", "en", "en", "jp", "kor", "fra", "th", "ru", "spa", "pt", "tr", "ara",
        ][index],
        "MTranServer" | "LibreTranslate" => [
            "zh", "zh", "en", "en", "ja", "ko", "fr", "th", "ru", "es", "pt", "tr", "ar",
        ][index],
        _ => code,
    }
}
