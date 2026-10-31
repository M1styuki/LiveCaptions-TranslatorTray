//! Small, bounded host/worker wire protocol. No captions, settings, or secrets.

mod message;

pub use message::{Empty, EngineState, Envelope, Message, Ready, WindowState};

use std::io::{self, Read, Write};

pub const PROTOCOL_VERSION: u32 = 1;
pub const MAX_MESSAGE_BYTES: usize = 64 * 1024;
pub const CHANNEL_CAPACITY: usize = 64;

/// Returns `None` only for EOF between frames. A partial frame is an error.
pub fn read_frame(reader: &mut impl Read) -> io::Result<Option<Envelope>> {
    let mut header = [0; 4];
    loop {
        match reader.read(&mut header[..1]) {
            Ok(0) => return Ok(None),
            Ok(_) => break,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }
    reader.read_exact(&mut header[1..])?;
    let length = u32::from_le_bytes(header) as usize;
    if length == 0 || length > MAX_MESSAGE_BYTES {
        return Err(invalid_data("invalid IPC frame length"));
    }
    let mut payload = vec![0; length];
    reader.read_exact(&mut payload)?;
    let message: Envelope = serde_json::from_slice(&payload).map_err(invalid_data)?;
    if message.protocol_version != PROTOCOL_VERSION {
        return Err(invalid_data("unsupported IPC protocol version"));
    }
    Ok(Some(message))
}

pub fn write_frame(writer: &mut impl Write, envelope: &Envelope) -> io::Result<()> {
    if envelope.protocol_version != PROTOCOL_VERSION {
        return Err(invalid_data("unsupported IPC protocol version"));
    }
    let payload = serde_json::to_vec(envelope).map_err(invalid_data)?;
    if payload.is_empty() || payload.len() > MAX_MESSAGE_BYTES {
        return Err(invalid_data("IPC message exceeds frame limit"));
    }
    writer.write_all(&(payload.len() as u32).to_le_bytes())?;
    writer.write_all(&payload)?;
    writer.flush()
}

fn invalid_data(error: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    struct Fragmented<T>(T);

    impl<T: Read> Read for Fragmented<T> {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            let count = buffer.len().min(2);
            self.0.read(&mut buffer[..count])
        }
    }

    impl<T: Write> Write for Fragmented<T> {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            self.0.write(&buffer[..buffer.len().min(3)])
        }

        fn flush(&mut self) -> io::Result<()> {
            self.0.flush()
        }
    }

    fn request() -> Envelope {
        Envelope::new(42, Message::ShowSettings(WindowState::default()))
    }

    #[test]
    fn partial_io_preserves_multiple_frames_and_clean_eof() {
        let mut writer = Fragmented(Vec::new());
        write_frame(&mut writer, &request()).unwrap();
        write_frame(
            &mut writer,
            &Envelope::new(43, Message::StopEngine(Empty {})),
        )
        .unwrap();
        let mut reader = Fragmented(Cursor::new(writer.0));
        assert_eq!(read_frame(&mut reader).unwrap(), Some(request()));
        assert_eq!(read_frame(&mut reader).unwrap().unwrap().request_id, 43);
        assert!(read_frame(&mut reader).unwrap().is_none());
    }

    #[test]
    fn truncated_header_and_body_are_errors() {
        for frame in [vec![1, 0], vec![5, 0, 0, 0, b'{']] {
            assert_eq!(
                read_frame(&mut Cursor::new(frame)).unwrap_err().kind(),
                io::ErrorKind::UnexpectedEof
            );
        }
    }

    #[test]
    fn zero_and_oversized_lengths_are_rejected_before_allocation() {
        for length in [0_u32, MAX_MESSAGE_BYTES as u32 + 1, u32::MAX] {
            assert_eq!(
                read_frame(&mut Cursor::new(length.to_le_bytes()))
                    .unwrap_err()
                    .kind(),
                io::ErrorKind::InvalidData
            );
        }
    }

    #[test]
    fn malformed_json_and_unknown_version_are_rejected() {
        let mut frame = Vec::from(1_u32.to_le_bytes());
        frame.push(b'{');
        assert!(read_frame(&mut Cursor::new(frame)).is_err());

        let mut envelope = request();
        envelope.protocol_version = 2;
        let payload = serde_json::to_vec(&envelope).unwrap();
        let mut frame = Vec::from((payload.len() as u32).to_le_bytes());
        frame.extend(payload);
        assert_eq!(
            read_frame(&mut Cursor::new(frame)).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[test]
    fn every_message_has_all_four_envelope_fields() {
        let value = serde_json::to_value(Envelope::new(4, Message::Exit(Empty {}))).unwrap();
        assert_eq!(value["protocol_version"], 1);
        assert_eq!(value["request_id"], 4);
        assert_eq!(value["kind"], "Exit");
        assert_eq!(value["payload"], serde_json::json!({}));
    }
}
