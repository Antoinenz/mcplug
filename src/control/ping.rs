//! Minecraft server-list ping — player count without a plugin or RCON.

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

fn varint(mut n: u32) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let b = (n & 0x7f) as u8;
        n >>= 7;
        out.push(if n != 0 { b | 0x80 } else { b });
        if n == 0 {
            return out;
        }
    }
}

fn read_varint(s: &mut TcpStream) -> std::io::Result<u32> {
    let (mut n, mut shift) = (0u32, 0);
    loop {
        let mut b = [0u8; 1];
        s.read_exact(&mut b)?;
        n |= ((b[0] & 0x7f) as u32) << shift;
        if b[0] & 0x80 == 0 {
            return Ok(n);
        }
        shift += 7;
        if shift > 35 {
            return Err(std::io::Error::other("varint too long"));
        }
    }
}

fn packet(payload: &[u8]) -> Vec<u8> {
    let mut p = varint(payload.len() as u32);
    p.extend_from_slice(payload);
    p
}

/// Blocking ping; `None` if the server doesn't answer.
pub fn player_count_sync(port: u16) -> Option<u32> {
    let addr = ("127.0.0.1", port).to_socket_addrs().ok()?.next()?;
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_secs(3)).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(3))).ok()?;
    let host = b"127.0.0.1";
    let mut hs = vec![0x00];
    hs.extend(varint(770));
    hs.extend(varint(host.len() as u32));
    hs.extend_from_slice(host);
    hs.extend_from_slice(&port.to_be_bytes());
    hs.push(0x01);
    s.write_all(&packet(&hs)).ok()?;
    s.write_all(&packet(&[0x00])).ok()?;
    let _len = read_varint(&mut s).ok()?;
    let _id = read_varint(&mut s).ok()?;
    let n = read_varint(&mut s).ok()? as usize;
    let mut buf = vec![0u8; n];
    s.read_exact(&mut buf).ok()?;
    let v: serde_json::Value = serde_json::from_slice(&buf).ok()?;
    v["players"]["online"].as_u64().map(|n| n as u32)
}

pub async fn player_count(port: u16) -> Option<u32> {
    tokio::task::spawn_blocking(move || player_count_sync(port)).await.ok().flatten()
}
