use blake2::{Blake2s256, Digest};
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub enum Frame {
    Hello(String, u16, u16, u8),
    Size(u16, u16, String),
    Enter(u16, u8),
    Leave(u16),
    Release,
    Reclaim,
    Events(Vec<(u16, u16, i32)>),
    AudioReq(u8, bool),
    Ping,
    PairAccept(String),
    Deny(String),
    ClipText(String),
    Prefs(u16),
    SetPref(u8, bool),
    Screen(u8),
    ScreenOpts(Vec<u8>),
    ScreenInfo(String),
    ScreenInput(Vec<(u16, u16, i32)>),
    Usb(u8, String),
    Version(String),
    Device(u8, Vec<u8>),
}

fn put_events(b: &mut Vec<u8>, evs: &[(u16, u16, i32)]) {
    for &(t, c, v) in evs {
        b.extend(t.to_le_bytes());
        b.extend(c.to_le_bytes());
        b.extend(v.to_le_bytes());
    }
}

fn get_events(b: &[u8]) -> Vec<(u16, u16, i32)> {
    b.chunks_exact(8)
        .map(|c| {
            (
                u16::from_le_bytes([c[0], c[1]]),
                u16::from_le_bytes([c[2], c[3]]),
                i32::from_le_bytes([c[4], c[5], c[6], c[7]]),
            )
        })
        .collect()
}

pub fn write_frame(w: &mut impl Write, f: &Frame) -> io::Result<()> {
    let mut b = Vec::new();
    match f {
        Frame::Hello(s, w, h, side) => {
            b.push(1);
            b.extend(w.to_le_bytes());
            b.extend(h.to_le_bytes());
            b.push(*side);
            b.extend(s.as_bytes());
        }
        Frame::Size(x, y, id) => {
            b.push(2);
            b.extend(x.to_le_bytes());
            b.extend(y.to_le_bytes());
            b.extend(id.as_bytes());
        }
        Frame::Deny(r) => {
            b.push(10);
            b.extend(r.as_bytes());
        }
        Frame::PairAccept(t) => {
            b.push(11);
            b.extend(t.as_bytes());
        }
        Frame::Ping => b.push(12),
        Frame::Enter(y, side) => {
            b.push(3);
            b.extend(y.to_le_bytes());
            b.push(*side);
        }
        Frame::Leave(y) => {
            b.push(4);
            b.extend(y.to_le_bytes());
        }
        Frame::Release => b.push(5),
        Frame::Reclaim => b.push(7),
        Frame::AudioReq(k, on) => {
            b.push(8);
            b.push(*k);
            b.push(*on as u8);
        }
        Frame::ClipText(t) => {
            b.push(9);
            b.extend(t.as_bytes());
        }
        Frame::Prefs(bits) => {
            b.push(13);
            b.extend(bits.to_le_bytes());
        }
        Frame::SetPref(p, on) => {
            b.push(14);
            b.push(*p);
            b.push(*on as u8);
        }
        Frame::Screen(v) => {
            b.push(15);
            b.push(*v);
        }
        Frame::ScreenOpts(o) => {
            b.push(16);
            b.extend(o);
        }
        Frame::ScreenInfo(t) => {
            b.push(17);
            b.extend(t.as_bytes());
        }
        Frame::ScreenInput(evs) => {
            b.push(18);
            put_events(&mut b, evs);
        }
        Frame::Usb(op, t) => {
            b.push(19);
            b.push(*op);
            b.extend(t.as_bytes());
        }
        Frame::Device(op, data) => {
            b.push(21);
            b.push(*op);
            b.extend(data);
        }
        Frame::Version(v) => {
            b.push(20);
            b.extend(v.as_bytes());
        }
        Frame::Events(evs) => {
            b.push(6);
            put_events(&mut b, evs);
        }
    }
    let mut out = (b.len() as u32).to_le_bytes().to_vec();
    out.extend(b);
    w.write_all(&out)
}

pub fn read_frame(r: &mut impl Read) -> io::Result<Frame> {
    let mut l = [0u8; 4];
    r.read_exact(&mut l)?;
    let n = u32::from_le_bytes(l) as usize;
    if n == 0 || n > 1 << 20 {
        return Err(io::Error::other("geçersiz çerçeve"));
    }
    let mut b = vec![0u8; n];
    r.read_exact(&mut b)?;
    let u16at = |i: usize| u16::from_le_bytes([b[i], b[i + 1]]);
    let need = |k: usize| {
        if n >= k {
            Ok(())
        } else {
            Err(io::Error::other("kısa çerçeve"))
        }
    };
    Ok(match b[0] {
        1 => {
            need(6)?;
            Frame::Hello(
                String::from_utf8_lossy(&b[6..]).into_owned(),
                u16at(1),
                u16at(3),
                b[5],
            )
        }
        2 => {
            need(5)?;
            Frame::Size(
                u16at(1),
                u16at(3),
                String::from_utf8_lossy(&b[5..]).into_owned(),
            )
        }
        10 => Frame::Deny(String::from_utf8_lossy(&b[1..]).into_owned()),
        11 => Frame::PairAccept(String::from_utf8_lossy(&b[1..]).into_owned()),
        12 => Frame::Ping,
        3 => {
            need(4)?;
            Frame::Enter(u16at(1), b[3])
        }
        4 => {
            need(3)?;
            Frame::Leave(u16at(1))
        }
        5 => Frame::Release,
        7 => Frame::Reclaim,
        8 => {
            need(3)?;
            Frame::AudioReq(b[1], b[2] != 0)
        }
        9 => Frame::ClipText(String::from_utf8_lossy(&b[1..]).into_owned()),
        13 => {
            need(3)?;
            Frame::Prefs(u16at(1))
        }
        14 => {
            need(3)?;
            Frame::SetPref(b[1], b[2] != 0)
        }
        15 => {
            need(2)?;
            Frame::Screen(b[1])
        }
        16 => Frame::ScreenOpts(b[1..].to_vec()),
        17 => Frame::ScreenInfo(String::from_utf8_lossy(&b[1..]).into_owned()),
        18 => Frame::ScreenInput(get_events(&b[1..])),
        19 => {
            need(2)?;
            Frame::Usb(b[1], String::from_utf8_lossy(&b[2..]).into_owned())
        }
        20 => Frame::Version(String::from_utf8_lossy(&b[1..]).into_owned()),
        21 => {
            need(2)?;
            Frame::Device(b[1], b[2..].to_vec())
        }
        6 => Frame::Events(get_events(&b[1..])),
        _ => return Err(io::Error::other("bilinmeyen çerçeve")),
    })
}

const NOISE: &str = "Noise_NNpsk0_25519_ChaChaPoly_BLAKE2s";
const CHUNK: usize = 32 * 1024;

fn err(m: impl Into<String>) -> io::Error {
    io::Error::other(m.into())
}

fn pake_key(s: &mut TcpStream, secret: &str, initiator: bool) -> io::Result<[u8; 32]> {
    use spake2::{Ed25519Group, Identity, Password, Spake2};
    let (st, msg) = Spake2::<Ed25519Group>::start_symmetric(
        &Password::new(secret.as_bytes()),
        &Identity::new(b"mause/1"),
    );
    let peer = if initiator {
        send_raw(s, &msg)?;
        recv_raw(s, 256)?
    } else {
        let p = recv_raw(s, 256)?;
        send_raw(s, &msg)?;
        p
    };
    let key = st
        .finish(&peer)
        .map_err(|_| err("kimlik doğrulaması başarısız"))?;
    let mut h = Blake2s256::new();
    h.update(b"mause-psk-v1");
    h.update(&key);
    Ok(h.finalize().into())
}

static FAILS: Mutex<
    Option<std::collections::HashMap<std::net::IpAddr, (u32, std::time::Instant)>>,
> = Mutex::new(None);

fn rate_ok(ip: std::net::IpAddr) -> bool {
    let m = FAILS.lock().unwrap();
    match m.as_ref().and_then(|m| m.get(&ip)) {
        Some((n, t)) => *n < 5 || t.elapsed() > Duration::from_secs(60),
        None => true,
    }
}

fn note_result(ip: std::net::IpAddr, ok: bool) {
    let mut g = FAILS.lock().unwrap();
    let m = g.get_or_insert_with(Default::default);
    if ok {
        m.remove(&ip);
    } else {
        let e = m.entry(ip).or_insert((0, std::time::Instant::now()));
        if e.1.elapsed() > Duration::from_secs(60) {
            *e = (0, std::time::Instant::now());
        }
        e.0 += 1;
        e.1 = std::time::Instant::now();
    }
}

fn send_raw(s: &mut TcpStream, b: &[u8]) -> io::Result<()> {
    let mut out = (b.len() as u16).to_le_bytes().to_vec();
    out.extend_from_slice(b);
    s.write_all(&out)
}

fn recv_raw(s: &mut TcpStream, max: usize) -> io::Result<Vec<u8>> {
    let mut l = [0u8; 2];
    s.read_exact(&mut l)?;
    let n = u16::from_le_bytes(l) as usize;
    if n > max {
        return Err(err("geçersiz kayıt boyutu"));
    }
    let mut b = vec![0u8; n];
    s.read_exact(&mut b)?;
    Ok(b)
}

pub const PAIR_SECRET: &str = "mause-pairing-v1";

pub const MODE_NORMAL: u8 = 0;
pub const MODE_PAIR: u8 = 1;

pub fn sas(hash: &[u8; 32]) -> String {
    let mut h = Blake2s256::new();
    h.update(b"mause-sas-v1");
    h.update(hash);
    let d: [u8; 32] = h.finalize().into();
    let n = u32::from_le_bytes([d[0], d[1], d[2], d[3]]) % 1_000_000;
    format!("{:03} {:03}", n / 1000, n % 1000)
}

pub fn connect_hello(mut s: TcpStream, my_id: &str, mode: u8, secret: &str) -> io::Result<Secure> {
    s.set_write_timeout(Some(Duration::from_secs(5)))?;
    let mut h = vec![mode];
    h.extend_from_slice(my_id.as_bytes());
    send_raw(&mut s, &h)?;
    handshake(s, true, secret)
}

pub fn accept_hello(
    mut s: TcpStream,
    lookup: &dyn Fn(u8, &str) -> Option<String>,
) -> io::Result<(Secure, u8, String, String)> {
    let ip = s.peer_addr()?.ip();
    if !rate_ok(ip) {
        return Err(err("çok fazla başarısız deneme, bir süre bekleyin"));
    }
    s.set_read_timeout(Some(Duration::from_secs(5)))?;
    let h = recv_raw(&mut s, 128)?;
    if h.is_empty() {
        return Err(err("boş başlık"));
    }
    let (mode, id) = (h[0], String::from_utf8_lossy(&h[1..]).into_owned());
    let Some(secret) = lookup(mode, &id) else {
        note_result(ip, false);
        return Err(err("tanınmayan cihaz (eşleşmemiş)"));
    };
    let sec = handshake(s, false, &secret)?;
    Ok((sec, mode, id, secret))
}

pub struct Secure {
    s: TcpStream,
    st: Arc<snow::StatelessTransportState>,
    send_n: u64,
    recv_n: u64,
    rbuf: Vec<u8>,
    rpos: usize,
    pub hash: [u8; 32],
}

pub fn handshake(mut s: TcpStream, initiator: bool, secret: &str) -> io::Result<Secure> {
    let ip = s.peer_addr().map(|a| a.ip()).ok();
    if !initiator {
        if let Some(ip) = ip {
            if !rate_ok(ip) {
                return Err(err("çok fazla başarısız deneme, bir süre bekleyin"));
            }
        }
    }
    let r = handshake_inner(&mut s, initiator, secret);
    if !initiator {
        if let Some(ip) = ip {
            note_result(ip, r.is_ok());
        }
    }
    r.map(|(st, hash)| Secure {
        s,
        st: Arc::new(st),
        send_n: 0,
        recv_n: 0,
        rbuf: Vec::new(),
        rpos: 0,
        hash,
    })
}

fn handshake_inner(
    s: &mut TcpStream,
    initiator: bool,
    secret: &str,
) -> io::Result<(snow::StatelessTransportState, [u8; 32])> {
    s.set_read_timeout(Some(Duration::from_secs(5)))?;
    s.set_write_timeout(Some(Duration::from_secs(5)))?;
    let psk = pake_key(s, secret, initiator)?;
    let params = NOISE.parse().map_err(|e| err(format!("{e}")))?;
    let b = snow::Builder::new(params)
        .psk(0, &psk)
        .map_err(|e| err(format!("{e}")))?
        .prologue(b"mause/1")
        .map_err(|e| err(format!("{e}")))?;
    let mut hs = if initiator {
        b.build_initiator()
    } else {
        b.build_responder()
    }
    .map_err(|e| err(format!("{e}")))?;
    let auth = |_| err("kimlik doğrulaması başarısız (parola yanlış olabilir)");
    let mut buf = [0u8; 1024];
    if initiator {
        let n = hs
            .write_message(&[], &mut buf)
            .map_err(|e| err(format!("{e}")))?;
        send_raw(s, &buf[..n])?;
        let m = recv_raw(s, 1024)?;
        hs.read_message(&m, &mut buf).map_err(auth)?;
    } else {
        let m = recv_raw(s, 1024)?;
        hs.read_message(&m, &mut buf).map_err(auth)?;
        let n = hs
            .write_message(&[], &mut buf)
            .map_err(|e| err(format!("{e}")))?;
        send_raw(s, &buf[..n])?;
    }
    let mut hash = [0u8; 32];
    hash.copy_from_slice(&hs.get_handshake_hash()[..32]);
    let st = hs
        .into_stateless_transport_mode()
        .map_err(|e| err(format!("{e}")))?;
    s.set_read_timeout(None)?;
    s.set_write_timeout(None)?;
    Ok((st, hash))
}

impl Secure {
    pub fn try_clone(&self) -> io::Result<Secure> {
        Ok(Secure {
            s: self.s.try_clone()?,
            st: self.st.clone(),
            send_n: self.send_n,
            recv_n: self.recv_n,
            rbuf: Vec::new(),
            rpos: 0,
            hash: self.hash,
        })
    }
    pub fn shutdown(&self) {
        let _ = self.s.shutdown(std::net::Shutdown::Both);
    }
    pub fn peer_addr(&self) -> io::Result<SocketAddr> {
        self.s.peer_addr()
    }
    pub fn set_read_timeout(&self, d: Option<Duration>) -> io::Result<()> {
        self.s.set_read_timeout(d)
    }
    pub fn set_write_timeout(&self, d: Option<Duration>) -> io::Result<()> {
        self.s.set_write_timeout(d)
    }
}

impl Read for Secure {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        while self.rpos >= self.rbuf.len() {
            let ct = recv_raw(&mut self.s, CHUNK + 64)?;
            let mut pt = vec![0u8; ct.len()];
            let n = self
                .st
                .read_message(self.recv_n, &ct, &mut pt)
                .map_err(|_| err("şifre çözülemedi: kayıt doğrulanamadı"))?;
            self.recv_n += 1;
            pt.truncate(n);
            self.rbuf = pt;
            self.rpos = 0;
        }
        let n = out.len().min(self.rbuf.len() - self.rpos);
        out[..n].copy_from_slice(&self.rbuf[self.rpos..self.rpos + n]);
        self.rpos += n;
        Ok(n)
    }
}

impl Write for Secure {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        let n = data.len().min(CHUNK);
        let mut ct = vec![0u8; n + 64];
        let l = self
            .st
            .write_message(self.send_n, &data[..n], &mut ct)
            .map_err(|e| err(format!("şifrelenemedi: {e}")))?;
        self.send_n += 1;
        send_raw(&mut self.s, &ct[..l])?;
        Ok(n)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.s.flush()
    }
}

pub fn audio_key(hash: &[u8; 32], kind: u8, sender_is_initiator: bool) -> [u8; 32] {
    let mut h = Blake2s256::new();
    h.update(b"mause-audio-v1");
    h.update(hash);
    h.update([kind, sender_is_initiator as u8]);
    h.finalize().into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::thread;

    fn pair(a: &'static str, b: &'static str) -> (io::Result<Secure>, io::Result<Secure>) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        let srv = thread::spawn(move || handshake(l.accept().unwrap().0, false, b));
        let cli = handshake(TcpStream::connect(addr).unwrap(), true, a);
        (cli, srv.join().unwrap())
    }

    #[test]
    fn cihaz_cercevesi_gidip_gelir() {
        let mut bytes = Vec::new();
        write_frame(&mut bytes, &Frame::Device(4, vec![1, 2, 3])).unwrap();
        write_frame(&mut bytes, &Frame::Device(0, Vec::new())).unwrap();
        let mut reader = &bytes[..];
        match read_frame(&mut reader).unwrap() {
            Frame::Device(op, data) => assert_eq!((op, data), (4, vec![1, 2, 3])),
            _ => panic!("cihaz çerçevesi bekleniyordu"),
        }
        match read_frame(&mut reader).unwrap() {
            Frame::Device(op, data) => assert_eq!((op, data.len()), (0, 0)),
            _ => panic!("cihaz çerçevesi bekleniyordu"),
        }
    }

    #[test]
    fn dogru_parola_sifreli_kanal() {
        let (c, s) = pair("ABCD-EFGH-JKLM-NPQR", "ABCD-EFGH-JKLM-NPQR");
        let (mut c, mut s) = (c.unwrap(), s.unwrap());
        assert_eq!(c.hash, s.hash, "iki taraf aynı oturum özetini görmeli");
        write_frame(&mut c, &Frame::Events(vec![(1, 30, 1), (2, 0, -5)])).unwrap();
        match read_frame(&mut s).unwrap() {
            Frame::Events(e) => assert_eq!(e, vec![(1, 30, 1), (2, 0, -5)]),
            _ => panic!("yanlış çerçeve"),
        }
        let big: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
        let big2 = big.clone();
        let w = thread::spawn(move || {
            c.write_all(&big2).unwrap();
        });
        let mut got = vec![0u8; big.len()];
        s.read_exact(&mut got).unwrap();
        w.join().unwrap();
        assert_eq!(got, big);
    }

    #[test]
    fn kisa_parola_da_calisir() {
        let (c, s) = pair("1234", "1234");
        assert!(c.is_ok() && s.is_ok(), "4 haneli kod ile kanal kurulmalı");
        let (c, s) = pair("482913", "482914");
        assert!(s.is_err() || c.is_err(), "bir hane farklı kod reddedilmeli");
    }

    #[test]
    fn art_arda_yanlis_denemeler_engellenir() {
        let ip: std::net::IpAddr = "10.9.9.9".parse().unwrap();
        for _ in 0..5 {
            assert!(rate_ok(ip));
            note_result(ip, false);
        }
        assert!(!rate_ok(ip), "5 başarısız denemeden sonra engellenmeli");
        note_result(ip, true);
        assert!(rate_ok(ip), "başarılı bağlantı sayacı sıfırlamalı");
    }

    #[test]
    fn yanlis_parola_reddedilir() {
        let (c, s) = pair("ABCD-EFGH-JKLM-NPQR", "XXXX-EFGH-JKLM-NPQR");
        assert!(
            c.is_err() || s.is_err(),
            "farklı parolalarla kanal kurulmamalı"
        );
        assert!(s.is_err(), "kabul eden taraf yanlış parolayı reddetmeli");
    }

    #[test]
    fn bozulan_kayit_reddedilir() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        let secret = "ABCD-EFGH-JKLM-NPQR";
        let proxy = TcpListener::bind("127.0.0.1:0").unwrap();
        let paddr = proxy.local_addr().unwrap();
        let srv = thread::spawn(move || handshake(l.accept().unwrap().0, false, secret).unwrap());
        thread::spawn(move || {
            let (mut a, _) = proxy.accept().unwrap();
            let mut b = TcpStream::connect(addr).unwrap();
            let (mut a2, mut b2) = (a.try_clone().unwrap(), b.try_clone().unwrap());
            thread::spawn(move || {
                io::copy(&mut b2, &mut a2).ok();
            });
            let mut buf = [0u8; 4096];
            let mut count = 0;
            while let Ok(n) = a.read(&mut buf) {
                if n == 0 {
                    break;
                }
                count += 1;
                if count > 2 {
                    buf[n - 1] ^= 0xFF;
                }
                if b.write_all(&buf[..n]).is_err() {
                    break;
                }
            }
        });
        let mut c = handshake(TcpStream::connect(paddr).unwrap(), true, secret).unwrap();
        let mut s = srv.join().unwrap();
        write_frame(&mut c, &Frame::Release).unwrap();
        assert!(
            read_frame(&mut s).is_err(),
            "bozulmuş kayıt kabul edilmemeli"
        );
    }

    #[test]
    fn ses_anahtarlari_yonlere_gore_ayri() {
        let h = [9u8; 32];
        assert_ne!(audio_key(&h, 0, true), audio_key(&h, 0, false));
        assert_ne!(audio_key(&h, 0, true), audio_key(&h, 1, true));
    }
}
