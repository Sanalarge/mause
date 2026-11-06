use crate::core::Shared;
use std::collections::HashMap;
use std::net::UdpSocket;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

pub const DISCOVERY_PORT: u16 = 24801;

pub struct Found {
    pub id: String,
    pub name: String,
    pub port: String,
    pub seen: Instant,
}

pub type Peers = Arc<Mutex<HashMap<String, Found>>>;

pub fn self_id() -> &'static str {
    ID.get_or_init(crate::config::random_id)
}

static ID: OnceLock<String> = OnceLock::new();

pub fn set_self_id(id: &str) {
    let _ = ID.set(id.to_string());
}

pub fn announce(shared: Arc<Shared>, name: String, port: String) {
    thread::spawn(move || {
        let Ok(sock) = UdpSocket::bind("0.0.0.0:0") else {
            return;
        };
        let _ = sock.set_broadcast(true);
        let msg = format!("MAUSE2|{name}|{port}|{}", self_id());
        while !shared.stop.load(Ordering::Relaxed) {
            let _ = sock.send_to(msg.as_bytes(), ("255.255.255.255", DISCOVERY_PORT));
            thread::sleep(Duration::from_secs(2));
        }
    });
}

pub fn peers() -> Peers {
    static P: OnceLock<Peers> = OnceLock::new();
    P.get_or_init(listen).clone()
}

pub fn all() -> Vec<(String, String, String, String)> {
    peers()
        .lock()
        .unwrap()
        .iter()
        .filter(|(_, f)| f.seen.elapsed() < Duration::from_secs(7) && f.id != self_id())
        .map(|(ip, f)| (ip.clone(), f.name.clone(), f.port.clone(), f.id.clone()))
        .collect()
}

fn listen() -> Peers {
    let peers: Peers = Arc::default();
    let p2 = peers.clone();
    thread::spawn(move || {
        let Ok(sock) = UdpSocket::bind(("0.0.0.0", DISCOVERY_PORT)) else {
            return;
        };
        let mut buf = [0u8; 256];
        while let Ok((n, from)) = sock.recv_from(&mut buf) {
            let s = String::from_utf8_lossy(&buf[..n]);
            let mut it = s.split('|');
            if it.next() != Some("MAUSE2") {
                continue;
            }
            let (name, port, id) = (
                it.next().unwrap_or("?").to_string(),
                it.next().unwrap_or("24800").to_string(),
                it.next().unwrap_or("").to_string(),
            );
            p2.lock().unwrap().insert(
                from.ip().to_string(),
                Found {
                    id,
                    name,
                    port,
                    seen: Instant::now(),
                },
            );
        }
    });
    peers
}
