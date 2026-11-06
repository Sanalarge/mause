use chacha20poly1305::aead::Aead;
use chacha20poly1305::{ChaCha20Poly1305, KeyInit};
use mause_protocol::{self as net, Secure};
use std::io::{self, Read};
use std::net::{SocketAddr, TcpStream, UdpSocket};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

pub const AUDIO_PORT: u16 = 24802;
pub const SCREEN_PORT: u16 = 24806;
pub const CAMERA_PORT: u16 = 24807;

const MAX_PACKET: usize = 8192;
const PACKET_PCM: usize = 1280;
const MAX_SCREEN_FRAME: usize = 16 << 20;
const TAG_LENGTH: usize = 16;

fn nonce(counter: u64) -> [u8; 12] {
    let mut bytes = [0u8; 12];
    bytes[4..].copy_from_slice(&counter.to_be_bytes());
    bytes
}

pub struct AudioOut {
    socket: UdpSocket,
    target: String,
    cipher: ChaCha20Poly1305,
    counter: u64,
}

impl AudioOut {
    pub fn new(ip: &str, port: u16, key: [u8; 32]) -> io::Result<Self> {
        Ok(Self {
            socket: UdpSocket::bind("0.0.0.0:0")?,
            target: format!("{ip}:{port}"),
            cipher: ChaCha20Poly1305::new(&key.into()),
            counter: 0,
        })
    }

    pub fn send(&mut self, pcm: &[u8]) -> io::Result<()> {
        for part in pcm.chunks(PACKET_PCM) {
            let sealed = self
                .cipher
                .encrypt(&nonce(self.counter).into(), part)
                .map_err(|_| io::Error::other("ses şifrelenemedi"))?;
            let mut packet = self.counter.to_be_bytes().to_vec();
            packet.extend(sealed);
            self.counter += 1;
            self.socket.send_to(&packet, &self.target)?;
        }
        Ok(())
    }
}

pub struct AudioIn {
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl Drop for AudioIn {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

pub fn listen_audio(port: u16) -> io::Result<UdpSocket> {
    let socket = UdpSocket::bind(("0.0.0.0", port))?;
    socket.set_read_timeout(Some(Duration::from_millis(300)))?;
    Ok(socket)
}

pub fn start_audio_in(
    socket: UdpSocket,
    peer_ip: String,
    key: [u8; 32],
    mut on_pcm: impl FnMut(&[u8]) + Send + 'static,
) -> AudioIn {
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let worker = thread::spawn(move || {
        let cipher = ChaCha20Poly1305::new(&key.into());
        let mut buffer = [0u8; MAX_PACKET];
        let mut newest: Option<u64> = None;
        while !flag.load(Ordering::Relaxed) {
            let Ok((length, from)) = socket.recv_from(&mut buffer) else {
                continue;
            };
            if from.ip().to_string() != peer_ip || length <= 8 + TAG_LENGTH {
                continue;
            }
            let counter = u64::from_be_bytes(buffer[..8].try_into().unwrap());
            if newest.is_some_and(|latest| counter <= latest) {
                continue;
            }
            let Ok(pcm) = cipher.decrypt(&nonce(counter).into(), &buffer[8..length]) else {
                continue;
            };
            newest = Some(counter);
            on_pcm(&pcm);
        }
    });
    AudioIn {
        stop,
        worker: Some(worker),
    }
}

pub fn connect_screen(ip: &str, secret: &str) -> io::Result<Secure> {
    connect_stream(ip, SCREEN_PORT, secret)
}

pub fn connect_stream(ip: &str, port: u16, secret: &str) -> io::Result<Secure> {
    let address: SocketAddr = format!("{ip}:{port}")
        .parse()
        .map_err(io::Error::other)?;
    let stream = TcpStream::connect_timeout(&address, Duration::from_secs(3))?;
    stream.set_nodelay(true)?;
    net::handshake(stream, true, secret)
}

pub fn frame_bytes(jpeg: &[u8], width: u16, height: u16) -> Vec<u8> {
    let mut out = Vec::with_capacity(jpeg.len() + 8);
    out.extend(((jpeg.len() + 4) as u32).to_le_bytes());
    out.extend(width.to_le_bytes());
    out.extend(height.to_le_bytes());
    out.extend_from_slice(jpeg);
    out
}

pub enum ScreenMessage {
    Frame {
        width: u16,
        height: u16,
        jpeg: Vec<u8>,
    },
    Failed(String),
}

pub fn read_screen_message(reader: &mut impl Read) -> io::Result<ScreenMessage> {
    let mut length = [0u8; 4];
    reader.read_exact(&mut length)?;
    let length = u32::from_le_bytes(length) as usize;
    if !(5..=MAX_SCREEN_FRAME).contains(&length) {
        return Err(io::Error::other("geçersiz ekran karesi"));
    }
    let mut body = vec![0u8; length];
    reader.read_exact(&mut body)?;
    let width = u16::from_le_bytes([body[0], body[1]]);
    let height = u16::from_le_bytes([body[2], body[3]]);
    if width == 0 && height == 0 {
        return Ok(ScreenMessage::Failed(
            String::from_utf8_lossy(&body[4..]).into_owned(),
        ));
    }
    Ok(ScreenMessage::Frame {
        width,
        height,
        jpeg: body[4..].to_vec(),
    })
}

pub fn error_frame(message: &str) -> Vec<u8> {
    let mut out = ((message.len() + 4) as u32).to_le_bytes().to_vec();
    out.extend([0u8; 4]);
    out.extend(message.as_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    const AUDIO_CHUNK: usize = 3840;

    #[test]
    fn ses_paketi_sifrelenip_cozulur() {
        let receiver = UdpSocket::bind("127.0.0.1:0").unwrap();
        receiver
            .set_read_timeout(Some(Duration::from_millis(300)))
            .unwrap();
        let port = receiver.local_addr().unwrap().port();
        let (sent, got) = mpsc::channel();
        let key = [7u8; 32];
        let input = start_audio_in(receiver, "127.0.0.1".into(), key, move |pcm| {
            let _ = sent.send(pcm.to_vec());
        });
        let mut output = AudioOut::new("127.0.0.1", port, key).unwrap();
        let first = vec![1u8; AUDIO_CHUNK];
        let second = vec![2u8; AUDIO_CHUNK];
        output.send(&first).unwrap();
        output.send(&second).unwrap();
        let mut received = Vec::new();
        while received.len() < AUDIO_CHUNK * 2 {
            let part = got.recv_timeout(Duration::from_secs(2)).unwrap();
            assert!(part.len() <= PACKET_PCM, "paket parçalanmadan gitmeli");
            received.extend(part);
        }
        assert_eq!(&received[..AUDIO_CHUNK], &first[..]);
        assert_eq!(&received[AUDIO_CHUNK..], &second[..]);
        drop(input);
    }

    #[test]
    fn yanlis_anahtarli_ses_paketi_atilir() {
        let receiver = UdpSocket::bind("127.0.0.1:0").unwrap();
        receiver
            .set_read_timeout(Some(Duration::from_millis(100)))
            .unwrap();
        let port = receiver.local_addr().unwrap().port();
        let (sent, got) = mpsc::channel();
        let input = start_audio_in(receiver, "127.0.0.1".into(), [1u8; 32], move |pcm| {
            let _ = sent.send(pcm.to_vec());
        });
        AudioOut::new("127.0.0.1", port, [2u8; 32])
            .unwrap()
            .send(&[0u8; AUDIO_CHUNK])
            .unwrap();
        assert!(got.recv_timeout(Duration::from_millis(500)).is_err());
        drop(input);
    }

    #[test]
    fn ekran_mesaji_okunur() {
        let mut stream = frame_bytes(&[1, 2, 3, 4], 640, 480);
        stream.extend(error_frame("yakalanamadı"));
        let mut reader = &stream[..];
        match read_screen_message(&mut reader).unwrap() {
            ScreenMessage::Frame {
                width,
                height,
                jpeg,
            } => {
                assert_eq!((width, height), (640, 480));
                assert_eq!(jpeg, [1, 2, 3, 4]);
            }
            ScreenMessage::Failed(_) => panic!("kare bekleniyordu"),
        }
        match read_screen_message(&mut reader).unwrap() {
            ScreenMessage::Failed(reason) => assert_eq!(reason, "yakalanamadı"),
            ScreenMessage::Frame { .. } => panic!("hata bekleniyordu"),
        }
        assert!(read_screen_message(&mut reader).is_err());
        let mut bad: &[u8] = &[2, 0, 0, 0, 1, 2];
        assert!(read_screen_message(&mut bad).is_err());
    }

    #[test]
    fn ekran_karesi_masaustunun_bekledigi_bicimde() {
        let frame = frame_bytes(&[9, 8, 7], 1080, 2400);
        assert_eq!(u32::from_le_bytes(frame[..4].try_into().unwrap()), 7);
        assert_eq!(u16::from_le_bytes(frame[4..6].try_into().unwrap()), 1080);
        assert_eq!(u16::from_le_bytes(frame[6..8].try_into().unwrap()), 2400);
        assert_eq!(&frame[8..], &[9, 8, 7]);
        let error = error_frame("hata");
        assert_eq!(u32::from_le_bytes(error[..4].try_into().unwrap()), 8);
        assert_eq!(&error[4..8], &[0, 0, 0, 0]);
        assert_eq!(&error[8..], b"hata");
    }
}
