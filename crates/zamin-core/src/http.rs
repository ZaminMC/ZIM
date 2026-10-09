//! One retry law for the idempotent metadata GETs. A connection that
//! dies **before a single response byte existed** is transient: the
//! loopback mock suites have documented the Windows flavor twice (the
//! pre-accept reset that surfaces on the listening socket, the phantom
//! empty read), and real networks surface the same class hourly. The
//! clients retry such deaths — bounded, once — and everything else
//! stays exactly as typed as before:
//!
//! - a status that arrived (even 5xx) never retries — the server spoke;
//! - a timeout never retries — the wait happened, and doubling it would
//!   mask a genuinely slow or dead peer behind a longer silence;
//! - response-body reads never retry (the body streamed, the bytes are
//!   gone — callers surface it; big downloads are resumable above).
//!
//! The verdict lives here, once, because five clients share the shape.

use std::error::Error as StdError;
use std::io;
use std::time::Duration;

/// Two attempts total: the first observes the death, the second either
/// answers or reports the very same typed error one attempt would have.
const ATTEMPTS: u32 = 2;

/// One short pause between attempts. Enough for a loopback peer that
/// polls its accept loop on a 10 ms tick; imperceptible on real networks.
const BACKOFF: Duration = Duration::from_millis(50);

/// Run an idempotent GET, retrying once when the connection died before
/// any response byte existed. The closure must rebuild the request each
/// call (ureq's builder is consumed by `call()`); every client's call
/// site is a `|| agent.get(url)...call()` with no state, so that is free.
///
/// The error travels boxed: `ureq::Error` is a wide enum, and both the
/// helper's `Result` and every retry closure's stay clippy-quiet because
/// the box is a pointer wide.
pub fn idempotent_get<T>(
    f: impl Fn() -> Result<T, Box<ureq::Error>>,
) -> Result<T, Box<ureq::Error>> {
    let mut attempt = 1;
    loop {
        match f() {
            Ok(value) => return Ok(value),
            Err(e) if attempt < ATTEMPTS && died_before_response(&e) => {
                attempt += 1;
                std::thread::sleep(BACKOFF);
                continue;
            }
            Err(e) => return Err(e),
        }
    }
}

/// True only when the transport error's source chain carries a
/// reset-class io error — the socket died before a response existed.
/// A malformed status line (no io error in the chain) and every
/// `Error::Status` are facts about the peer, not flukes of the wire.
fn died_before_response(e: &ureq::Error) -> bool {
    let ureq::Error::Transport(transport) = e else {
        return false;
    };
    let mut source: Option<&(dyn StdError + 'static)> = transport.source();
    while let Some(err) = source {
        if let Some(io_err) = err.downcast_ref::<io::Error>() {
            return matches!(
                io_err.kind(),
                io::ErrorKind::ConnectionReset
                    | io::ErrorKind::ConnectionAborted
                    | io::ErrorKind::BrokenPipe
            );
        }
        source = err.source();
    }
    false
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    use super::{died_before_response, idempotent_get};

    fn agent() -> ureq::Agent {
        ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(5))
            .timeout(Duration::from_secs(5))
            .build()
    }

    /// A mock whose first connection is accepted and dropped **unread**:
    /// the client's status-line read then dies before one response byte
    /// existed (the clean-close flavor of the documented windows flake
    /// class — the reset flavor differs only in which io error kind the
    /// source chain carries, and the predicate treats them alike). The
    /// second connection is answered. The call must succeed through the
    /// retry, and exactly two connections must arrive.
    #[test]
    fn a_connection_that_dies_unread_never_kills_the_call() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let url = format!("http://{}/answer", listener.local_addr().expect("addr"));
        let seen = Arc::new(AtomicU32::new(0));
        let seen_for_thread = Arc::clone(&seen);
        std::thread::spawn(move || {
            for (index, stream) in listener.incoming().enumerate() {
                let mut stream = match stream {
                    Ok(s) => s,
                    Err(_) => continue,
                };
                let _ = seen_for_thread.fetch_add(1, Ordering::SeqCst);
                if index == 0 {
                    // Drop unread: a FIN with no answer — the exact
                    // "died before a response byte existed" shape.
                    drop(stream);
                    continue;
                }
                let mut request = Vec::new();
                let mut buf = [0u8; 4096];
                let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                while let Ok(n) = stream.read(&mut buf) {
                    if n == 0 {
                        break;
                    }
                    request.extend_from_slice(&buf[..n]);
                    // The terminator check must see the bytes just read:
                    // a request that lands in one segment would otherwise
                    // wait out the whole read timeout before answering.
                    if request.windows(4).any(|w| w == b"\r\n\r\n") {
                        break;
                    }
                }
                let head = "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 6\r\nConnection: close\r\n\r\n";
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(b"answer");
                let _ = stream.flush();
                return; // both connections served; the thread retires.
            }
        });

        let response = idempotent_get(|| agent().get(&url).call().map_err(Box::new))
            .map_err(|e| *e)
            .expect("retried call answers");
        assert_eq!(response.into_string().expect("body"), "answer");
        assert_eq!(seen.load(Ordering::SeqCst), 2, "exactly one retry");
    }

    /// A response that arrived — even an error status — is the peer
    /// speaking, never a fluke of the wire: no retry may follow.
    #[test]
    fn a_status_that_arrived_never_retries() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let url = format!("http://{}/gone", listener.local_addr().expect("addr"));
        let seen = Arc::new(AtomicU32::new(0));
        let seen_for_thread = Arc::clone(&seen);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                seen_for_thread.fetch_add(1, Ordering::SeqCst);
                let mut buf = [0u8; 4096];
                let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                let _ = stream.read(&mut buf);
                let head = "HTTP/1.1 410 Gone\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.flush();
                return; // one connection answered; the thread retires.
            }
        });

        let err = idempotent_get(|| agent().get(&url).call().map_err(Box::new))
            .map_err(|e| *e)
            .expect_err("410 is an error");
        assert!(
            matches!(err, ureq::Error::Status(410, _)),
            "the status is reported as itself: {err}"
        );
        assert_eq!(seen.load(Ordering::SeqCst), 1, "no retry after a status");
    }

    /// The predicate itself, against an error shape ureq really
    /// produces: a refused connection is a fact about the peer, not a
    /// wire fluke — no retry. (The reset-class arm of the predicate is
    /// what the drop-unread mock above exercises end to end.)
    #[test]
    fn the_predicate_reads_the_source_chain() {
        let refused = agent()
            .get("http://127.0.0.1:1/never")
            .call()
            .expect_err("port 1 refuses");
        assert!(
            !died_before_response(&refused),
            "connection refused is a fact about the peer: {refused}"
        );
    }
}
