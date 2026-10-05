//! A framed connection: the unit clients and the daemon exchange protocol
//! messages over. One connection multiplexes everything (protocol spec §1).

use bytes::Bytes;
use futures_util::{SinkExt, StreamExt};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::codec::{Framed, LengthDelimitedCodec};

use crate::error::IpcError;

type BoxedIo = Box<dyn BoxableIo>;

/// Object-safe umbrella for anything framed-transportable. `Unpin` is a
/// supertrait so the boxed trait object satisfies `Framed`'s bounds.
trait BoxableIo: AsyncRead + AsyncWrite + Send + Unpin {}
impl<T: AsyncRead + AsyncWrite + Send + Unpin> BoxableIo for T {}

pub struct Connection {
    framed: Framed<BoxedIo, LengthDelimitedCodec>,
}

/// Write half of a split connection. Senders serialize through it; the
/// read half belongs to the session loop, so a parked receive never blocks
/// notification delivery.
pub struct ConnectionWriteHalf {
    sink: futures_util::stream::SplitSink<Framed<BoxedIo, LengthDelimitedCodec>, Bytes>,
}

/// Read half of a split connection: owned exclusively by the session loop.
pub struct ConnectionReadHalf {
    stream: futures_util::stream::SplitStream<Framed<BoxedIo, LengthDelimitedCodec>>,
}

fn codec() -> LengthDelimitedCodec {
    LengthDelimitedCodec::builder()
        .little_endian()
        .length_field_length(4)
        .max_frame_length(zamin_protocol::framing::MAX_FRAME_LENGTH)
        .new_codec()
}

impl Connection {
    pub fn new<S>(io: S) -> Self
    where
        S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        Connection {
            framed: Framed::new(Box::new(io), codec()),
        }
    }

    /// Send one frame. Payloads larger than the protocol cap are rejected
    /// before touching the wire.
    pub async fn send(&mut self, payload: Bytes) -> Result<(), IpcError> {
        if payload.len() > zamin_protocol::framing::MAX_FRAME_LENGTH {
            return Err(IpcError::FrameTooLarge);
        }
        self.framed.send(payload).await?;
        Ok(())
    }

    /// Receive the next frame. `None` means the peer closed the connection.
    pub async fn recv(&mut self) -> Result<Option<Bytes>, IpcError> {
        match self.framed.next().await {
            Some(Ok(bytes)) => Ok(Some(bytes.freeze())),
            Some(Err(e)) if e.kind() == std::io::ErrorKind::InvalidData => {
                Err(IpcError::FrameTooLarge)
            }
            Some(Err(e)) => Err(e.into()),
            None => Ok(None),
        }
    }

    /// Split into independently usable halves: many senders behind one
    /// write half, one reader on the read half.
    pub fn split(self) -> (ConnectionWriteHalf, ConnectionReadHalf) {
        let (sink, stream) = self.framed.split();
        (ConnectionWriteHalf { sink }, ConnectionReadHalf { stream })
    }
}

impl ConnectionWriteHalf {
    pub async fn send(&mut self, payload: Bytes) -> Result<(), IpcError> {
        if payload.len() > zamin_protocol::framing::MAX_FRAME_LENGTH {
            return Err(IpcError::FrameTooLarge);
        }
        self.sink.send(payload).await?;
        Ok(())
    }
}

impl ConnectionReadHalf {
    pub async fn recv(&mut self) -> Result<Option<Bytes>, IpcError> {
        match self.stream.next().await {
            Some(Ok(bytes)) => Ok(Some(bytes.freeze())),
            Some(Err(e)) if e.kind() == std::io::ErrorKind::InvalidData => {
                Err(IpcError::FrameTooLarge)
            }
            Some(Err(e)) => Err(e.into()),
            None => Ok(None),
        }
    }
}
