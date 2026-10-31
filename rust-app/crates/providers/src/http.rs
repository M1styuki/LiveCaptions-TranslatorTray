use std::{fmt, time::Duration};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranslateError {
    pub service: &'static str,
    pub status: Option<u16>,
    pub code: Option<String>,
    pub reason: &'static str,
}

impl fmt::Display for TranslateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.status {
            Some(status) => write!(
                formatter,
                "{}：{}（HTTP {}）",
                self.service, self.reason, status
            ),
            None => write!(formatter, "{}：{}", self.service, self.reason),
        }?;
        if let Some(code) = &self.code {
            write!(formatter, "（服务代码 {code}）")?;
        }
        Ok(())
    }
}
impl std::error::Error for TranslateError {}

pub(crate) fn client() -> Result<reqwest::Client, TranslateError> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .build()
        .map_err(|_| TranslateError {
            service: "翻译",
            status: None,
            code: None,
            reason: "无法初始化网络连接",
        })
}

pub(crate) fn network_error(service: &'static str, error: reqwest::Error) -> TranslateError {
    TranslateError {
        service,
        status: error.status().map(|status| status.as_u16()),
        code: None,
        reason: if error.is_timeout() {
            "请求超时，请检查网络"
        } else if error.is_connect() {
            "无法连接服务，请检查网络"
        } else {
            "请求失败或响应格式不正确"
        },
    }
}
