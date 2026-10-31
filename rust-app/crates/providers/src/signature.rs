//! The legacy providers require MD5 of the unencoded UTF-8 form input. Use
//! Windows CNG, rather than shipping another crypto implementation/runtime.
use crate::TranslateError;
use windows::{Win32::Security::Cryptography::*, core::PCWSTR};

fn error(service: &'static str) -> TranslateError {
    TranslateError {
        service,
        status: None,
        code: None,
        reason: "无法生成接口签名",
    }
}
pub(crate) fn salt(service: &'static str) -> Result<String, TranslateError> {
    let mut bytes = [0u8; 16];
    let status = unsafe { BCryptGenRandom(None, &mut bytes, BCRYPT_USE_SYSTEM_PREFERRED_RNG) };
    if status.0 < 0 {
        return Err(error(service));
    }
    Ok(hex(&bytes))
}
pub(crate) fn md5(service: &'static str, input: &str) -> Result<String, TranslateError> {
    let mut algorithm = BCRYPT_ALG_HANDLE::default();
    let status = unsafe {
        BCryptOpenAlgorithmProvider(
            &mut algorithm,
            BCRYPT_MD5_ALGORITHM,
            PCWSTR::null(),
            BCRYPT_OPEN_ALGORITHM_PROVIDER_FLAGS(0),
        )
    };
    if status.0 < 0 {
        return Err(error(service));
    }
    let mut digest = [0u8; 16];
    let status = unsafe { BCryptHash(algorithm, None, input.as_bytes(), &mut digest) };
    unsafe {
        let _ = BCryptCloseAlgorithmProvider(algorithm, 0);
    }
    if status.0 < 0 {
        return Err(error(service));
    }
    Ok(hex(&digest))
}
fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(&mut output, "{byte:02x}").unwrap();
    }
    output
}
