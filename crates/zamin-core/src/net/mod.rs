//! Port management. Availability is a bind-test; the final authority is the
//! server binding at boot — a race here produces a typed boot failure that
//! names the port (ADR for ports: desired value lives in server config, the
//! actual value in `server.properties`; reconciliation is explicit).

use std::net::{Ipv4Addr, SocketAddrV4, TcpListener};

use crate::error::CoreError;

/// True when a TCP listener can bind the port on all interfaces and then
/// release it. Lasts only as long as no one else grabs it — that is fine;
/// the preflight treats a race as a boot failure with the port named.
pub fn is_port_available(port: u16) -> bool {
    TcpListener::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, port)).is_ok()
}

/// Availability or a typed in-use error carrying the port.
pub fn check_available(port: u16) -> Result<(), CoreError> {
    if is_port_available(port) {
        Ok(())
    } else {
        Err(CoreError::PortInUse { port })
    }
}

/// First available port at or above `from`, skipping ports already known to
/// the caller (e.g. desired ports of other managed servers). `None` if
/// nothing in the range is free.
pub fn first_available(from: u16, up_to: u16, skip: impl Fn(u16) -> bool) -> Option<u16> {
    (from..=up_to).find(|&p| !skip(p) && is_port_available(p))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_port_reports_available() {
        // Bind our own listener first, release it, then expect availability.
        // (A fixed "free" port can race with the OS; this avoids that.)
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        assert!(is_port_available(port));
    }

    #[test]
    fn held_port_reports_in_use() {
        // Bind on all interfaces: on Windows, 0.0.0.0:port stays bindable
        // while only 127.0.0.1:port is held (no SO_EXCLUSIVEADDRUSE), so
        // the probe and the holder must use the same address.
        let listener = std::net::TcpListener::bind("0.0.0.0:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        assert!(check_available(port).is_err());
    }

    #[test]
    fn first_available_skips_and_finds() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let held = listener.local_addr().unwrap().port();
        // Search a small range above the held port; the skip closure stands
        // in for "other managed servers' desired ports".
        let base = held.saturating_add(1);
        let found = first_available(base, base + 50, |p| p == base);
        assert_eq!(found, Some(base + 1));
    }
}
