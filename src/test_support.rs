use crate::core::Shared;
use crate::transfer::{self, Status, Transfer};
use std::net::{SocketAddr, TcpListener};
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::{Arc, Condvar};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

pub struct TransferSettings {
    pub port: AtomicU16,
    pub inbox: tempfile::TempDir,
    pub changed: Condvar,
}

impl Default for TransferSettings {
    fn default() -> Self {
        Self {
            port: AtomicU16::new(transfer::FT_PORT),
            inbox: tempfile::tempdir().expect("test inbox"),
            changed: Condvar::new(),
        }
    }
}

pub struct TransferServer {
    pub addr: SocketAddr,
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl TransferServer {
    pub fn new(shared: Arc<Shared>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("test transfer listener");
        listener.set_nonblocking(true).unwrap();
        let addr = listener.local_addr().unwrap();
        let thread = transfer::serve_listener(shared.clone(), listener);
        Self {
            addr,
            shared,
            thread: Some(thread),
        }
    }
}

impl Drop for TransferServer {
    fn drop(&mut self) {
        self.shared.request_stop();
        if let Some(thread) = self.thread.take() {
            thread.join().expect("transfer listener panicked");
        }
    }
}

pub struct TransferPair {
    pub sender: Arc<Shared>,
    pub receiver: Arc<Shared>,
    _server: TransferServer,
}

impl TransferPair {
    pub fn new() -> Self {
        let sender = Shared::new(false);
        let receiver = Shared::new(false);
        for shared in [&sender, &receiver] {
            *shared.peer_ip.lock().unwrap() = Some("127.0.0.1".into());
            shared
                .peer_secrets
                .lock()
                .unwrap()
                .insert("127.0.0.1".into(), "test-secret".into());
        }
        receiver.perms.lock().unwrap().files_in = true;
        let server = TransferServer::new(receiver.clone());
        sender
            .test_transfer
            .port
            .store(server.addr.port(), Ordering::Relaxed);
        Self {
            sender,
            receiver,
            _server: server,
        }
    }
}

pub fn wait_transfers(shared: &Shared, count: usize) -> Vec<Transfer> {
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut transfers = shared.transfers.lock().unwrap();
    loop {
        if transfers.len() >= count && transfers.iter().all(|t| t.status != Status::Active) {
            return transfers.clone();
        }
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .expect("transfer timed out");
        transfers = shared
            .test_transfer
            .changed
            .wait_timeout(transfers, remaining)
            .unwrap()
            .0;
    }
}
