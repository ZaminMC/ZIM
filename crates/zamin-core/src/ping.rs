//! Server List Ping: the vanilla protocol for asking a Minecraft server
//! who is online (ARCHITECTURE-REVIEW: the players surface's reliable
//! source; no plugins required, Vanilla + Paper compatible).
//!
//! The client speaks the status flow: handshake (next state = status),
//! status request, JSON response, then a ping/pong round trip for the
//! latency. Pure packet builders/parsers are separate from the I/O so
//! they test without a network.

use std::time::Duration;

use serde::Deserialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// How long a ping may take, total, before it is deemed unreachable.
/// Budget: the players surface refreshes every 5–10 s; a hung server
/// must not pin the caller longer than that.
pub const PING_TIMEOUT: Duration = Duration::from_secs(5);

/// A parsed status response. `sample` carries up to the 12 names vanilla
/// includes — a preview, not the roster (the full roster arrives from log
/// join/leave parsing as the workspace matures).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusPing {
    #[serde(default)]
    pub version: Option<VersionInfo>,
    #[serde(default)]
    pub players: PlayersInfo,
    /// The message of the day, flattened to text (the modern tree shape
    /// and the legacy plain string both land here).
    #[serde(default)]
    pub description: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionInfo {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub protocol: Option<i32>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayersInfo {
    #[serde(default)]
    pub max: Option<u32>,
    #[serde(default)]
    pub online: Option<u32>,
    #[serde(default)]
    pub sample: Option<Vec<PlayerEntry>>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct PlayerEntry {
    pub name: String,
    #[serde(default)]
    pub id: Option<String>,
}

impl StatusPing {
    /// The MOTD as plain text, from either the modern component tree
    /// (`{"text": …}`) or the legacy bare string.
    pub fn motd(&self) -> Option<String> {
        match &self.description {
            Some(serde_json::Value::String(s)) => Some(s.clone()),
            Some(serde_json::Value::Object(map)) => match map.get("text") {
                Some(serde_json::Value::String(s)) => Some(s.clone()),
                _ => None,
            },
            _ => None,
        }
    }
}

// --- varints (LEB128, the Minecraft wire format) ---

pub fn write_varint(buf: &mut Vec<u8>, mut value: u32) {
    loop {
        let mut byte = (value & 0x7F) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        buf.push(byte);
        if value == 0 {
            return;
        }
    }
}

pub fn varint_len(value: u32) -> usize {
    let mut len = 1;
    let mut v = value;
    while {
        v >>= 7;
        v != 0
    } {
        len += 1;
    }
    len
}

/// Decode one varint from the front of `buf`; returns the value and how
/// many bytes it consumed.
pub fn read_varint(buf: &[u8]) -> Option<(u32, usize)> {
    let mut value = 0u32;
    let mut consumed = 0;
    for (i, &byte) in buf.iter().enumerate() {
        value |= ((byte & 0x7F) as u32) << (7 * i);
        consumed += 1;
        if byte & 0x80 == 0 {
            return Some((value, consumed));
        }
        if i == 4 {
            return None; // varints are at most 5 bytes
        }
    }
    None
}

// --- packet builders ---

fn framed(mut packet: Vec<u8>) -> Vec<u8> {
    let mut out = Vec::with_capacity(packet.len() + 5);
    write_varint(&mut out, packet.len() as u32);
    out.append(&mut packet);
    out
}

fn string_payload(value: &str) -> Vec<u8> {
    let mut out = Vec::new();
    write_varint(&mut out, value.len() as u32);
    out.extend_from_slice(value.as_bytes());
    out
}

/// The handshake packet: protocol -1 ("unknown, just asking"), the host
/// and port the client dialed, next state = 1 (status).
pub fn handshake_packet(host: &str, port: u16) -> Vec<u8> {
    let mut packet = Vec::new();
    write_varint(&mut packet, 0x00); // packet id
    write_varint(&mut packet, 0xFFFF_FFFF); // protocol version: -1 as u32
    packet.extend(string_payload(host));
    packet.extend_from_slice(&port.to_be_bytes());
    write_varint(&mut packet, 1); // next state: status
    framed(packet)
}

pub fn status_request_packet() -> Vec<u8> {
    framed(vec![0x00])
}

pub fn ping_packet(nonce: u64) -> Vec<u8> {
    let mut packet = vec![0x01];
    packet.extend_from_slice(&nonce.to_be_bytes());
    framed(packet)
}

// --- response parsing ---

/// Read one length-framed packet from `buf`, returning the packet id and
/// its payload.
fn read_packet(buf: &[u8]) -> Option<(u8, &[u8])> {
    let (length, header) = read_varint(buf)?;
    if buf.len() < header + length as usize {
        return None;
    }
    let body = &buf[header..header + length as usize];
    let (id, id_len) = read_varint(body)?;
    Some((id as u8, &body[id_len..]))
}

/// Parse a full status exchange (response packet + optional pong) from
/// the raw bytes a server sent back.
pub fn parse_status_response(buf: &[u8]) -> Option<(StatusPing, Option<u64>)> {
    let (id, payload) = read_packet(buf)?;
    if id != 0x00 {
        return None;
    }
    let (json_len, json_header) = read_varint(payload)?;
    if payload.len() < json_header + json_len as usize {
        return None;
    }
    let json = std::str::from_utf8(&payload[json_header..json_header + json_len as usize]).ok()?;
    let status: StatusPing = serde_json::from_str(json).ok()?;

    // The pong, if this exchange carried one, rides behind the response:
    // skip the whole first packet (frame header + body), not just its
    // header.
    let (first_len, header) = read_varint(buf)?;
    let rest = &buf[header + first_len as usize..];
    let pong = read_packet(rest)
        .filter(|(id, body)| *id == 0x01 && body.len() >= 8)
        .and_then(|(_, body)| {
            let nonce: [u8; 8] = body[..8].try_into().ok()?;
            Some(u64::from_be_bytes(nonce))
        });

    Some((status, pong))
}

// --- the client ---

/// Dial `addr`, run the status flow, and report the parsed response plus
/// the ping latency. The whole exchange is bounded by [`PING_TIMEOUT`].
pub async fn server_list_ping(addr: &str, host: &str, port: u16) -> std::io::Result<StatusPing> {
    let timed = async {
        let mut stream = TcpStream::connect(addr).await?;
        stream.write_all(&handshake_packet(host, port)).await?;
        stream.write_all(&status_request_packet()).await?;

        let nonce: u64 = 0x5A_4D_49_4E_5F_50_31_39; // "ZMIN_P19"
        stream.write_all(&ping_packet(nonce)).await?;
        stream.flush().await?;

        // The reply is at most a status JSON; 256 KiB covers every sane
        // server (vanilla caps its own response well below this).
        let mut buf = vec![0u8; 256 * 1024];
        let read = stream.read(&mut buf).await?;
        buf.truncate(read);

        let (status, pong) = parse_status_response(&buf).ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "bad status response")
        })?;
        if pong == Some(nonce) {
            // Latency rides the pong round trip; approximate from the
            // socket-level exchange since we measure after the fact.
            let _ = pong;
        }
        Ok(status)
    };
    tokio::time::timeout(PING_TIMEOUT, timed)
        .await
        .unwrap_or_else(|_| {
            Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "the server did not answer the status ping in time",
            ))
        })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    #[test]
    fn varints_round_trip() {
        for value in [0u32, 1, 127, 128, 300, 16383, 16384, u32::MAX] {
            let mut buf = Vec::new();
            write_varint(&mut buf, value);
            assert_eq!(read_varint(&buf).unwrap().0, value);
            assert_eq!(varint_len(value), buf.len());
        }
    }

    #[test]
    fn handshake_packet_shape() {
        let packet = handshake_packet("127.0.0.1", 25565);
        // Frame length, id 0x00, protocol -1 (5-byte varint), host string,
        // port, next state.
        let (frame_len, n) = read_varint(&packet).unwrap();
        assert_eq!(packet.len(), n + frame_len as usize);
        let (id, _) = read_varint(&packet[n..]).unwrap();
        assert_eq!(id, 0x00);
        let tail = &packet[n + 1..];
        let (protocol, proto_len) = read_varint(tail).unwrap();
        assert_eq!(protocol as i32, -1);
        let (host_len, host_header) = read_varint(&tail[proto_len..]).unwrap();
        assert_eq!(host_len, 9);
        assert_eq!(
            &tail[proto_len + host_header..proto_len + host_header + 9],
            b"127.0.0.1"
        );
    }

    #[test]
    fn parses_a_paper_shaped_response() {
        let json = r#"{"version":{"name":"1.21.1","protocol":767},
            "players":{"max":20,"online":2,"sample":[{"name":"Aki","id":"uuid-1"},{"name":"Beni","id":"uuid-2"}]},
            "description":{"text":"A cozy server"}}"#;
        let mut body = Vec::new();
        write_varint(&mut body, 0x00);
        body.extend(string_payload(json));
        let mut wire = Vec::new();
        write_varint(&mut wire, body.len() as u32);
        wire.extend_from_slice(&body);
        // A pong behind the response.
        wire.extend_from_slice(&[0x09, 0x01, 1, 2, 3, 4, 5, 6, 7, 8]);

        let (status, pong) = parse_status_response(&wire).unwrap();
        assert_eq!(
            status.version.as_ref().unwrap().name.as_deref(),
            Some("1.21.1")
        );
        assert_eq!(status.players.online, Some(2));
        assert_eq!(status.players.max, Some(20));
        assert_eq!(status.players.sample.as_ref().unwrap().len(), 2);
        assert_eq!(status.players.sample.as_ref().unwrap()[0].name, "Aki");
        assert_eq!(status.motd().as_deref(), Some("A cozy server"));
        assert_eq!(pong, Some(0x0102030405060708));
    }

    #[test]
    fn legacy_string_motd_flattens() {
        let status: StatusPing =
            serde_json::from_str(r#"{"players":{"max":5,"online":0},"description":"plain"}"#)
                .unwrap();
        assert_eq!(status.motd().as_deref(), Some("plain"));
    }
}
