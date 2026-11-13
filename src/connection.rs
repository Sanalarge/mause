//! Control traffic and keepalives run independently of device initialization.
use crate::net::{self, Frame, Secure};
use std::io::{self, Write};
#[cfg(not(test))]
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

const PING: Duration = Duration::from_secs(1);
const IDLE: Duration = Duration::from_secs(15);

struct State {
    started: Instant,
    received_ms: AtomicU64,
    stopped: AtomicBool,
    reason: Mutex<Option<String>>,
    shutdown: Secure,
    failed: Box<dyn Fn(String) + Send + Sync>,
}

impl State {
    fn close(&self, reason: String) {
        if !self.stopped.swap(true, Ordering::AcqRel) {
            *self.reason.lock().unwrap() = Some(reason.clone());
            self.shutdown.shutdown();
            (self.failed)(reason);
        }
    }
    fn received(&self) {
        self.received_ms
            .store(self.started.elapsed().as_millis() as u64, Ordering::Release);
    }
    fn idle(&self) -> Duration {
        Duration::from_millis(
            (self.started.elapsed().as_millis() as u64)
                .saturating_sub(self.received_ms.load(Ordering::Acquire)),
        )
    }
}

pub struct Writer {
    queue: mpsc::SyncSender<Vec<u8>>,
    state: Arc<State>,
}

impl Drop for Writer {
    fn drop(&mut self) {
        self.state.stopped.store(true, Ordering::Release);
        self.state.shutdown.shutdown();
    }
}

impl Write for Writer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.state.stopped.load(Ordering::Acquire) {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                self.state
                    .reason
                    .lock()
                    .unwrap()
                    .clone()
                    .unwrap_or_else(|| "Kontrol bağlantısı kapandı.".into()),
            ));
        }
        self.queue.send(bytes.to_vec()).map_err(|_| {
            io::Error::new(io::ErrorKind::BrokenPipe, "Kontrol göndericisi kapandı.")
        })?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub fn start(
    stream: Secure,
    received: impl Fn(Frame) -> bool + Send + 'static,
    failed: impl Fn(String) + Send + Sync + 'static,
) -> io::Result<Writer> {
    start_with(stream, received, failed, PING, IDLE)
}

fn start_with(
    mut stream: Secure,
    received: impl Fn(Frame) -> bool + Send + 'static,
    failed: impl Fn(String) + Send + Sync + 'static,
    ping: Duration,
    idle: Duration,
) -> io::Result<Writer> {
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    let mut reader = stream.try_clone()?;
    let state = Arc::new(State {
        started: Instant::now(),
        received_ms: AtomicU64::new(0),
        stopped: AtomicBool::new(false),
        reason: Mutex::new(None),
        shutdown: stream.try_clone()?,
        failed: Box::new(failed),
    });
    let reading = state.clone();
    std::thread::spawn(move || {
        loop {
            match net::read_frame(&mut reader) {
                Ok(frame) => {
                    reading.received();
                    if !matches!(frame, Frame::Ping) && !received(frame) {
                        reading.stopped.store(true, Ordering::Release);
                        reading.shutdown.shutdown();
                        break;
                    }
                }
                Err(e) => {
                    reading.close(format!("Kontrol verisi alınamadı ({:?}): {e}", e.kind()));
                    break;
                }
            }
        }
    });
    let (queue, rx) = mpsc::sync_channel::<Vec<u8>>(256);
    let writing = state.clone();
    std::thread::spawn(move || {
        let mut last_ping = Instant::now();
        while !writing.stopped.load(Ordering::Acquire) {
            if writing.idle() >= idle {
                writing.close(format!(
                    "Karşı PC'den {:.1} saniyedir kontrol verisi alınmıyor.",
                    writing.idle().as_secs_f32()
                ));
                break;
            }
            if last_ping.elapsed() >= ping {
                if let Err(e) = net::write_frame(&mut stream, &Frame::Ping) {
                    writing.close(format!(
                        "Bağlantı yoklaması gönderilemedi ({:?}): {e}",
                        e.kind()
                    ));
                    break;
                }
                last_ping = Instant::now();
            }
            match rx.recv_timeout(
                ping.saturating_sub(last_ping.elapsed())
                    .min(Duration::from_millis(100)),
            ) {
                Ok(bytes) => {
                    if let Err(e) = stream.write_all(&bytes) {
                        writing.close(format!(
                            "Kontrol verisi gönderilemedi ({:?}): {e}",
                            e.kind()
                        ));
                        break;
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
        }
    });
    Ok(Writer { queue, state })
}

/// Only connection lifecycle events are persisted. Disk writes never block a session.
pub fn record(message: String) {
    #[cfg(test)]
    {
        let _ = message;
    }
    #[cfg(not(test))]
    {
        static LOG: OnceLock<mpsc::SyncSender<String>> = OnceLock::new();
        let queue = LOG.get_or_init(|| {
            let (tx, rx) = mpsc::sync_channel::<String>(128);
            std::thread::spawn(move || {
                let dir = crate::platform::config_dir().join("mause");
                if std::fs::create_dir_all(&dir).is_err() {
                    return;
                }
                let path = dir.join("connection.log");
                for message in rx {
                    if std::fs::metadata(&path).is_ok_and(|m| m.len() > 512 * 1024) {
                        let old = dir.join("connection.previous.log");
                        let _ = std::fs::remove_file(&old);
                        let _ = std::fs::rename(&path, old);
                    }
                    if let Ok(mut file) = std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(&path)
                    {
                        let _ = writeln!(
                            file,
                            "{} {}",
                            chrono::Utc::now().to_rfc3339(),
                            message.replace(['\r', '\n'], " ")
                        );
                    }
                }
            });
            tx
        });
        let _ = queue.try_send(message);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{TcpListener, TcpStream};

    fn pair() -> (Secure, Secure) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            net::handshake(listener.accept().unwrap().0, false, "connection-test").unwrap()
        });
        let client =
            net::handshake(TcpStream::connect(addr).unwrap(), true, "connection-test").unwrap();
        (client, server.join().unwrap())
    }

    #[test]
    fn busy_application_does_not_stop_heartbeats_or_invalidate_received_activity() {
        let (a, b) = pair();
        let (events, incoming) = mpsc::channel();
        let (failures, failed) = mpsc::channel();
        let other_failures = failures.clone();
        let mut a = start_with(
            a,
            move |frame| events.send(frame).is_ok(),
            move |e| {
                let _ = failures.send(e);
            },
            Duration::from_millis(40),
            Duration::from_millis(500),
        )
        .unwrap();
        let mut b = start_with(
            b,
            |_| true,
            move |e| {
                let _ = other_failures.send(e);
            },
            Duration::from_millis(40),
            Duration::from_millis(500),
        )
        .unwrap();
        net::write_frame(
            &mut b,
            &Frame::Version("queued during device initialization".into()),
        )
        .unwrap();
        // Neither application's control queue is consumed for longer than the watchdog interval.
        std::thread::sleep(Duration::from_millis(1200));
        assert!(!a.state.stopped.load(Ordering::Acquire));
        assert!(!b.state.stopped.load(Ordering::Acquire));
        assert!(failed.try_recv().is_err());
        assert!(
            matches!(incoming.recv_timeout(Duration::from_secs(1)).unwrap(), Frame::Version(v) if v.starts_with("queued"))
        );
        net::write_frame(&mut a, &Frame::Version("still connected".into())).unwrap();
    }

    #[test]
    fn silent_peer_is_still_disconnected_and_reports_the_real_reason() {
        let (_silent, active) = pair();
        let (tx, errors) = mpsc::channel();
        let writer = start_with(
            active,
            |_| true,
            move |reason| {
                let _ = tx.send(reason);
            },
            Duration::from_millis(40),
            Duration::from_millis(300),
        )
        .unwrap();
        let reason = errors.recv_timeout(Duration::from_secs(3)).unwrap();
        assert!(reason.contains("kontrol verisi alınmıyor"), "{reason}");
        assert!(writer.state.stopped.load(Ordering::Acquire));
    }

    #[test]
    fn encryption_and_frame_order_are_preserved_with_independent_keepalives() {
        let (a, b) = pair();
        let (tx, rx) = mpsc::channel();
        let mut a = start_with(
            a,
            |_| true,
            |_| {},
            Duration::from_millis(5),
            Duration::from_secs(3),
        )
        .unwrap();
        let _b = start_with(
            b,
            move |frame| tx.send(frame).is_ok(),
            |_| {},
            Duration::from_millis(5),
            Duration::from_secs(3),
        )
        .unwrap();
        for i in 0..100 {
            net::write_frame(&mut a, &Frame::Version(i.to_string())).unwrap();
        }
        for i in 0..100 {
            assert!(
                matches!(rx.recv_timeout(Duration::from_secs(3)).unwrap(), Frame::Version(v) if v == i.to_string())
            );
        }
    }

    #[test]
    fn remote_close_reports_receive_error_instead_of_a_generic_disconnect() {
        let (client, server) = pair();
        let (tx, rx) = mpsc::channel();
        let _writer = start(
            server,
            |_| true,
            move |reason| {
                let _ = tx.send(reason);
            },
        )
        .unwrap();
        drop(client);
        let reason = rx.recv_timeout(Duration::from_secs(3)).unwrap();
        assert!(reason.contains("Kontrol verisi alınamadı"), "{reason}");
    }
}
