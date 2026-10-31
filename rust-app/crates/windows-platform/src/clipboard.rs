//! Copy only on an explicit UI action. Prepare UTF-16 before opening/emptying
//! the clipboard; retain local memory ownership until SetClipboardData succeeds.
use std::{io, mem::size_of};
use windows::Win32::{
    Foundation::{GlobalFree, HANDLE, HGLOBAL, HWND},
    System::{
        DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData},
        Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock},
        Ole::CF_UNICODETEXT,
    },
};
struct TextMemory(Option<HGLOBAL>);
impl TextMemory {
    fn new(text: &str) -> io::Result<Self> {
        // CF_UNICODETEXT is NUL-terminated. Reject embedded NUL rather than
        // claiming that a silently truncated copy preserved the whole text.
        if text.contains('\0') {
            return Err(io::Error::other("文本含有无法复制的空字符。"));
        }
        let count = text
            .encode_utf16()
            .count()
            .checked_add(1)
            .ok_or_else(|| io::Error::other("文本过长。"))?;
        let bytes = count
            .checked_mul(size_of::<u16>())
            .ok_or_else(|| io::Error::other("文本过长。"))?;
        // SAFETY: bytes includes the terminator and is checked against overflow.
        let memory = Self(Some(
            unsafe { GlobalAlloc(GMEM_MOVEABLE, bytes) }
                .map_err(|_| io::Error::other("无法准备复制内容。"))?,
        ));
        let handle = memory.0.unwrap();
        let pointer = unsafe { GlobalLock(handle) } as *mut u16;
        if pointer.is_null() {
            return Err(io::Error::other("无法准备复制内容。"));
        }
        // SAFETY: GlobalLock succeeded, allocation contains count u16s, and
        // this function alone owns the block. No fallible work occurs locked.
        unsafe {
            let target = std::slice::from_raw_parts_mut(pointer, count);
            for (target, value) in target.iter_mut().zip(text.encode_utf16().chain(Some(0))) {
                *target = value;
            }
            // A false result after the final unlock can mean success (lock
            // count reached zero). This fresh block has exactly one lock.
            let _ = GlobalUnlock(handle);
        }
        Ok(memory)
    }
}
impl Drop for TextMemory {
    fn drop(&mut self) {
        if let Some(handle) = self.0.take() {
            unsafe {
                let _ = GlobalFree(Some(handle));
            }
        }
    }
}
struct Open;
impl Drop for Open {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseClipboard();
        }
    }
}
pub fn write_text(owner: usize, text: &str) -> io::Result<()> {
    if owner == 0 {
        return Err(io::Error::other("字幕窗口尚未就绪。"));
    }
    let mut memory = TextMemory::new(text)?;
    // SAFETY: UI caller supplies its live main HWND. Clipboard lifetime is
    // scoped and this call never pumps messages, retries, or reads old content.
    unsafe { OpenClipboard(Some(HWND(owner as *mut _))) }
        .map_err(|_| io::Error::other("剪贴板正被占用，请稍后重试。"))?;
    let _open = Open;
    unsafe {
        EmptyClipboard().map_err(|_| io::Error::other("无法复制，请稍后重试。"))?;
        SetClipboardData(CF_UNICODETEXT.0 as u32, Some(HANDLE(memory.0.unwrap().0)))
            .map_err(|_| io::Error::other("无法复制，请稍后重试。"))?;
    }
    memory.0.take(); // Windows now owns the HGLOBAL; never free it locally.
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn utf16_block_preserves_whitespace_unicode_and_terminator_without_touching_clipboard() {
        let text = "  中文 😀, \"quoted\"\nline two\r\n  ";
        let memory = TextMemory::new(text).unwrap();
        let handle = memory.0.unwrap();
        let pointer = unsafe { GlobalLock(handle) } as *const u16;
        assert!(!pointer.is_null());
        let expected: Vec<_> = text.encode_utf16().chain(Some(0)).collect();
        assert_eq!(
            unsafe { std::slice::from_raw_parts(pointer, expected.len()) },
            expected
        );
        unsafe {
            let _ = GlobalUnlock(handle);
        }
        assert!(TextMemory::new("a\0b").is_err());
        assert!(write_text(0, "public fixture").is_err());
    }
}
