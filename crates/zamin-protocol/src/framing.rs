//! Frame codec: `u32` little-endian byte count + payload bytes (protocol
//! spec §1). Pure buffer handling — transports in `zamin-ipc` drive it; there
//! is no I/O here.

/// Hard cap on a single frame. File chunks are kilobytes; this cap exists to
/// reject garbage and hostile input, not to permit giant messages.
pub const MAX_FRAME_LENGTH: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum FrameError {
    #[error("frame of {0} bytes exceeds the {1}-byte limit")]
    TooLarge(usize, usize),
}

/// Append one framed message to `out`.
pub fn encode_frame(payload: &[u8], out: &mut Vec<u8>) {
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(payload);
}

/// Incremental decoder over a byte stream.
#[derive(Debug, Default)]
pub struct FrameDecoder {
    buf: Vec<u8>,
}

impl FrameDecoder {
    pub fn new() -> Self {
        FrameDecoder { buf: Vec::new() }
    }

    /// Buffer received bytes. Cheap; no parsing happens here.
    pub fn push(&mut self, bytes: &[u8]) {
        self.buf.extend_from_slice(bytes);
    }

    /// Pop the next complete frame, if one has arrived. On an oversized
    /// frame the buffer is dropped (the connection is unusable) and the
    /// error names the offending length.
    pub fn next_frame(&mut self) -> Result<Option<Vec<u8>>, FrameError> {
        if self.buf.len() < 4 {
            return Ok(None);
        }
        let len = u32::from_le_bytes([self.buf[0], self.buf[1], self.buf[2], self.buf[3]]) as usize;
        if len > MAX_FRAME_LENGTH {
            self.buf.clear();
            return Err(FrameError::TooLarge(len, MAX_FRAME_LENGTH));
        }
        if self.buf.len() < 4 + len {
            return Ok(None);
        }
        let frame = self.buf[4..4 + len].to_vec();
        self.buf.drain(..4 + len);
        Ok(Some(frame))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_single_frame() {
        let mut wire = Vec::new();
        encode_frame(b"hello", &mut wire);
        let mut decoder = FrameDecoder::new();
        decoder.push(&wire);
        assert_eq!(decoder.next_frame().unwrap(), Some(b"hello".to_vec()));
        assert_eq!(decoder.next_frame().unwrap(), None);
    }

    #[test]
    fn partial_delivery_reassembles() {
        let mut wire = Vec::new();
        encode_frame(b"abcdef", &mut wire);
        let mut decoder = FrameDecoder::new();
        let total = wire.len();
        for (i, chunk) in wire.chunks(2).enumerate() {
            let is_last = (i + 1) * 2 >= total;
            decoder.push(chunk);
            if !is_last {
                // No frame can complete before its final byte arrives.
                assert_eq!(decoder.next_frame().unwrap(), None);
            }
        }
        assert_eq!(decoder.next_frame().unwrap(), Some(b"abcdef".to_vec()));
    }

    #[test]
    fn multiple_frames_in_one_push() {
        let mut wire = Vec::new();
        encode_frame(b"one", &mut wire);
        encode_frame(b"two", &mut wire);
        let mut decoder = FrameDecoder::new();
        decoder.push(&wire);
        assert_eq!(decoder.next_frame().unwrap(), Some(b"one".to_vec()));
        assert_eq!(decoder.next_frame().unwrap(), Some(b"two".to_vec()));
    }

    #[test]
    fn oversized_frame_is_rejected_and_buffer_cleared() {
        let mut wire = Vec::new();
        encode_frame(&[0u8; 8], &mut wire);
        // corrupt the length field
        wire[0] = 0xff;
        wire[1] = 0xff;
        wire[2] = 0xff;
        wire[3] = 0xff;
        let mut decoder = FrameDecoder::new();
        decoder.push(&wire);
        assert!(matches!(
            decoder.next_frame(),
            Err(FrameError::TooLarge(_, _))
        ));
        assert_eq!(decoder.next_frame().unwrap(), None);
    }
}
