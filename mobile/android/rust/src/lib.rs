mod clipboard;
mod files;
mod media;

use jni::objects::{GlobalRef, JByteArray, JClass, JIntArray, JObject, JString, JValue};
use jni::sys::{jboolean, jint, jlong};
use jni::{JNIEnv, JavaVM};
use mause_protocol::{self as net, Frame, Secure};
use media::{AudioIn, AudioOut};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{self, ErrorKind, Write};
use std::net::{SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::os::fd::FromRawFd;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const PREF_AUDIO_OUT: u16 = 1 << 1;
const PREF_MIC_OUT: u16 = 1 << 2;
const PREF_FILES_IN: u16 = 1 << 3;
const DEFAULT_PREFS: u16 = (1 << 0) | (1 << 4) | (1 << 8);
const SUPPORTS_PREFS: u32 = 1 << 15;
const SUPPORTS_SCREEN: u32 = 1 << 14;
const SUPPORTS_SCREEN2: u32 = 1 << 13;
const SUPPORTS_HARDWARE: u32 = 1 << 10;
const DEVICE_CAMERA_STOP: u8 = 2;
const SCREEN_STOP: u8 = 0;
const SCREEN_START: u8 = 1;
const SCREEN_DENIED: u8 = 2;
const SCREEN_FAILED: u8 = 3;

struct Runtime {
    stop: Arc<AtomicBool>,
}

struct Callbacks {
    vm: JavaVM,
    target: GlobalRef,
}

#[derive(Clone)]
struct ViewHandle {
    stop: Arc<AtomicBool>,
    stream: Arc<Mutex<Option<Secure>>>,
}

#[derive(Clone)]
struct Link {
    ip: String,
    secret: String,
    hash: [u8; 32],
    clipboard_images: bool,
}

#[derive(Clone)]
struct Pair {
    name: String,
    secret: String,
}

static RUNTIME: Mutex<Option<Runtime>> = Mutex::new(None);
static PAIR_DECISION: Mutex<Option<bool>> = Mutex::new(None);
static PAIR_READY: Condvar = Condvar::new();
static WRITER: Mutex<Option<Secure>> = Mutex::new(None);
static CALLBACKS: Mutex<Option<Arc<Callbacks>>> = Mutex::new(None);
static LINK: Mutex<Option<Link>> = Mutex::new(None);
static PREFS: AtomicU16 = AtomicU16::new(DEFAULT_PREFS);
static SCREEN_ALLOWED: AtomicBool = AtomicBool::new(false);
static SCREEN_CONNECTING: AtomicBool = AtomicBool::new(false);
static SCREEN_TX: Mutex<Option<Secure>> = Mutex::new(None);
static AUDIO_OUT: Mutex<[Option<AudioOut>; 2]> = Mutex::new([None, None]);
static AUDIO_IN: Mutex<[Option<AudioIn>; 2]> = Mutex::new([None, None]);
static ROOTS: Mutex<Option<Arc<files::Roots>>> = Mutex::new(None);
static VIEW: Mutex<Option<ViewHandle>> = Mutex::new(None);
static PEER_HARDWARE: AtomicBool = AtomicBool::new(false);
static CAMERA_TX: Mutex<Option<Secure>> = Mutex::new(None);

impl Callbacks {
    fn call(&self, f: impl FnOnce(&mut JNIEnv) -> jni::errors::Result<()>) {
        let Ok(mut env) = self.vm.attach_current_thread_permanently() else {
            return;
        };
        let _ = env.with_local_frame(16, |env| f(env));
    }

    fn string_call(&self, method: &str, value: &str) {
        self.call(|env| {
            let text = JObject::from(env.new_string(value)?);
            env.call_method(
                self.target.as_obj(),
                method,
                "(Ljava/lang/String;)V",
                &[JValue::Object(&text)],
            )?;
            Ok(())
        });
    }

    fn status(&self, value: &str) {
        self.string_call("nativeStatus", value);
    }

    fn pair(&self, computer: &str, code: &str) {
        self.call(|env| {
            let computer = JObject::from(env.new_string(computer)?);
            let code = JObject::from(env.new_string(code)?);
            env.call_method(
                self.target.as_obj(),
                "nativePairRequest",
                "(Ljava/lang/String;Ljava/lang/String;)V",
                &[JValue::Object(&computer), JValue::Object(&code)],
            )?;
            Ok(())
        });
    }

    fn connected(&self, value: bool) {
        self.call(|env| {
            env.call_method(
                self.target.as_obj(),
                "nativeConnected",
                "(Z)V",
                &[JValue::Bool(value as u8)],
            )?;
            Ok(())
        });
    }

    fn enter(&self, position: u16, side: u8) {
        self.call(|env| {
            env.call_method(
                self.target.as_obj(),
                "nativeEnter",
                "(II)V",
                &[JValue::Int(position as i32), JValue::Int(side as i32)],
            )?;
            Ok(())
        });
    }

    fn events(&self, method: &str, events: &[(u16, u16, i32)]) {
        let values: Vec<jint> = events
            .iter()
            .flat_map(|(kind, code, value)| [*kind as jint, *code as jint, *value as jint])
            .collect();
        self.call(|env| {
            let array = env.new_int_array(values.len() as i32)?;
            env.set_int_array_region(&array, 0, &values)?;
            let array = JObject::from(array);
            env.call_method(
                self.target.as_obj(),
                method,
                "([I)V",
                &[JValue::Object(&array)],
            )?;
            Ok(())
        });
    }

    fn input(&self, events: &[(u16, u16, i32)]) {
        self.events("nativeInput", events);
    }

    fn screen_input(&self, events: &[(u16, u16, i32)]) {
        self.events("nativeScreenInput", events);
    }

    fn activity(&self, text: &str) {
        self.string_call("nativeActivity", text);
    }

    fn audio_request(&self, kind: u8, on: bool) {
        self.call(|env| {
            env.call_method(
                self.target.as_obj(),
                "nativeAudioRequest",
                "(IZ)V",
                &[JValue::Int(kind as i32), JValue::Bool(on as u8)],
            )?;
            Ok(())
        });
    }

    fn audio_play(&self, kind: u8, pcm: &[u8]) {
        self.call(|env| {
            let array = JObject::from(env.byte_array_from_slice(pcm)?);
            env.call_method(
                self.target.as_obj(),
                "nativeAudioPlay",
                "(I[B)V",
                &[JValue::Int(kind as i32), JValue::Object(&array)],
            )?;
            Ok(())
        });
    }

    fn screen_start(&self, options: &[u8]) {
        self.call(|env| {
            let array = JObject::from(env.byte_array_from_slice(options)?);
            env.call_method(
                self.target.as_obj(),
                "nativeScreenStart",
                "([B)V",
                &[JValue::Object(&array)],
            )?;
            Ok(())
        });
    }

    fn screen_stop(&self) {
        self.call(|env| {
            env.call_method(self.target.as_obj(), "nativeScreenStop", "()V", &[])?;
            Ok(())
        });
    }

    fn view_frame(&self, width: u16, height: u16, jpeg: &[u8]) {
        self.call(|env| {
            let array = JObject::from(env.byte_array_from_slice(jpeg)?);
            env.call_method(
                self.target.as_obj(),
                "nativeViewFrame",
                "(II[B)V",
                &[
                    JValue::Int(width as i32),
                    JValue::Int(height as i32),
                    JValue::Object(&array),
                ],
            )?;
            Ok(())
        });
    }

    fn view_ended(&self, reason: &str) {
        self.string_call("nativeViewEnded", reason);
    }

    fn view_info(&self, control: bool) {
        self.call(|env| {
            env.call_method(
                self.target.as_obj(),
                "nativeViewInfo",
                "(Z)V",
                &[JValue::Bool(control as u8)],
            )?;
            Ok(())
        });
    }

    fn device(&self, op: u8, data: &[u8]) {
        self.call(|env| {
            let array = JObject::from(env.byte_array_from_slice(data)?);
            env.call_method(
                self.target.as_obj(),
                "nativeDevice",
                "(I[B)V",
                &[JValue::Int(op as i32), JValue::Object(&array)],
            )?;
            Ok(())
        });
    }

    fn pref_on(&self, pref: u8) {
        self.call(|env| {
            env.call_method(
                self.target.as_obj(),
                "nativePrefOn",
                "(I)V",
                &[JValue::Int(pref as i32)],
            )?;
            Ok(())
        });
    }

    fn pref_off(&self, pref: u8) {
        self.call(|env| {
            env.call_method(
                self.target.as_obj(),
                "nativePrefOff",
                "(I)V",
                &[JValue::Int(pref as i32)],
            )?;
            Ok(())
        });
    }

    fn release(&self) {
        self.call(|env| {
            env.call_method(self.target.as_obj(), "nativeRelease", "()V", &[])?;
            Ok(())
        });
    }

    fn clipboard(&self, text: &str) {
        self.string_call("nativeClipboard", text);
    }
}

fn text(env: &mut JNIEnv, value: &JString) -> Option<String> {
    env.get_string(value).ok().map(String::from)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_bytaymur_mause_NativeBridge_start(
    mut env: JNIEnv,
    this: JObject,
    name: JString,
    id: JString,
    port: jint,
    width: jint,
    height: jint,
    storage_path: JString,
) -> jboolean {
    let (Some(name), Some(id), Some(storage_path)) = (
        text(&mut env, &name),
        text(&mut env, &id),
        text(&mut env, &storage_path),
    ) else {
        return 0;
    };
    let Ok(vm) = env.get_java_vm() else {
        return 0;
    };
    let Ok(target) = env.new_global_ref(this) else {
        return 0;
    };
    let mut runtime = RUNTIME.lock().unwrap();
    if runtime.is_some() {
        return 1;
    }
    let stop = Arc::new(AtomicBool::new(false));
    let callbacks = Arc::new(Callbacks { vm, target });
    *CALLBACKS.lock().unwrap() = Some(callbacks.clone());
    let config = AgentConfig {
        name: clean(&name),
        id: clean(&id),
        port: port.clamp(1024, 65535) as u16,
        width: width.clamp(1, u16::MAX as i32) as u16,
        height: height.clamp(1, u16::MAX as i32) as u16,
        store: PathBuf::from(storage_path).join("pairs.v1"),
    };
    let worker_stop = stop.clone();
    thread::spawn(move || run_agent(config, callbacks, worker_stop));
    *runtime = Some(Runtime { stop });
    1
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_bytaymur_mause_NativeBridge_stop(
    _env: JNIEnv,
    _class: JClass,
) {
    if let Some(runtime) = RUNTIME.lock().unwrap().take() {
        runtime.stop.store(true, Ordering::Relaxed);
        PAIR_READY.notify_all();
    }
    if let Some(writer) = WRITER.lock().unwrap().take() {
        writer.shutdown();
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_bytaymur_mause_NativeBridge_leave(
    _env: JNIEnv,
    _this: JObject,
    position: jint,
) {
    let _ = send(&Frame::Leave(position.clamp(0, u16::MAX as i32) as u16));
}

fn send(frame: &Frame) -> io::Result<()> {
    match WRITER.lock().unwrap().as_mut() {
        Some(writer) => net::write_frame(writer, frame),
        None => Err(io::Error::other("bağlantı yok")),
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_bytaymur_mause_NativeBridge_answerPair(
    _env: JNIEnv,
    _class: JClass,
    accept: jboolean,
) {
    *PAIR_DECISION.lock().unwrap() = Some(accept != 0);
    PAIR_READY.notify_all();
}

struct AgentConfig {
    name: String,
    id: String,
    port: u16,
    width: u16,
    height: u16,
    store: PathBuf,
}

fn run_agent(config: AgentConfig, callbacks: Arc<Callbacks>, stop: Arc<AtomicBool>) {
    let listener = match TcpListener::bind(("0.0.0.0", config.port)) {
        Ok(listener) => listener,
        Err(error) => {
            callbacks.status(&format!("Bağlantı noktası açılamadı: {error}"));
            *RUNTIME.lock().unwrap() = None;
            return;
        }
    };
    listener.set_nonblocking(true).ok();
    let pairs = Arc::new(Mutex::new(load_pairs(&config.store)));
    announce(&config, stop.clone());
    serve_files(callbacks.clone(), stop.clone());
    callbacks.status("Ağdaki bilgisayarlar bekleniyor");
    while !stop.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, _)) => {
                stream.set_nodelay(true).ok();
                let lookup_pairs = pairs.clone();
                let lookup = move |mode: u8, peer_id: &str| {
                    if mode == net::MODE_PAIR {
                        Some(net::PAIR_SECRET.to_owned())
                    } else {
                        lookup_pairs
                            .lock()
                            .unwrap()
                            .get(peer_id)
                            .map(|pair| pair.secret.clone())
                    }
                };
                match net::accept_hello(stream, &lookup) {
                    Ok((secure, mode, peer_id, _)) if mode == net::MODE_PAIR => {
                        let _ = pair_session(
                            secure,
                            &peer_id,
                            &config,
                            &callbacks,
                            &pairs,
                            &stop,
                        );
                    }
                    Ok((secure, _, _, secret)) => {
                        let _ = control_session(secure, secret, &config, &callbacks, &stop);
                    }
                    Err(error) => callbacks.status(&format!("Bağlantı reddedildi: {error}")),
                }
            }
            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(40));
            }
            Err(error) => {
                callbacks.status(&format!("Ağ hatası: {error}"));
                thread::sleep(Duration::from_millis(300));
            }
        }
    }
    callbacks.connected(false);
    callbacks.status("Durduruldu");
    *CALLBACKS.lock().unwrap() = None;
    *RUNTIME.lock().unwrap() = None;
}

fn announce(config: &AgentConfig, stop: Arc<AtomicBool>) {
    let message = format!("MAUSE2|{}|{}|{}", config.name, config.port, config.id);
    thread::spawn(move || {
        let Ok(socket) = UdpSocket::bind("0.0.0.0:0") else {
            return;
        };
        socket.set_broadcast(true).ok();
        while !stop.load(Ordering::Relaxed) {
            let _ = socket.send_to(message.as_bytes(), ("255.255.255.255", 24801));
            thread::sleep(Duration::from_secs(2));
        }
    });
}

fn pair_session(
    mut secure: Secure,
    header_id: &str,
    config: &AgentConfig,
    callbacks: &Callbacks,
    pairs: &Mutex<HashMap<String, Pair>>,
    stop: &AtomicBool,
) -> io::Result<()> {
    let hash = secure.hash;
    let Frame::Hello(identity, ..) = net::read_frame(&mut secure)? else {
        return Err(io::Error::other("eşleşme isteği geçersiz"));
    };
    let (identity_id, computer) = parse_identity(&identity);
    let peer_id = if identity_id.is_empty() {
        header_id.to_owned()
    } else {
        identity_id
    };
    *PAIR_DECISION.lock().unwrap() = None;
    callbacks.pair(&computer, &net::sas(&hash));
    callbacks.status("Eşleşme onayı bekleniyor");
    let deadline = Instant::now() + Duration::from_secs(90);
    let mut decision = PAIR_DECISION.lock().unwrap();
    while decision.is_none() && !stop.load(Ordering::Relaxed) && Instant::now() < deadline {
        decision = PAIR_READY
            .wait_timeout(decision, Duration::from_millis(250))
            .unwrap()
            .0;
    }
    if decision.take() != Some(true) {
        net::write_frame(&mut secure, &Frame::Deny("kullanıcı reddetti".into()))?;
        callbacks.status("Eşleşme reddedildi");
        return Ok(());
    }
    let mut random = [0u8; 32];
    getrandom::fill(&mut random).map_err(|e| io::Error::other(e.to_string()))?;
    let secret = hex(&random);
    pairs.lock().unwrap().insert(
        peer_id,
        Pair {
            name: computer,
            secret: secret.clone(),
        },
    );
    save_pairs(&config.store, &pairs.lock().unwrap())?;
    net::write_frame(
        &mut secure,
        &Frame::PairAccept(format!("{secret}\u{1}{}\n{}", config.id, config.name)),
    )?;
    callbacks.status("Eşleşti. Bağlantı bekleniyor");
    Ok(())
}

fn control_session(
    mut secure: Secure,
    secret: String,
    config: &AgentConfig,
    callbacks: &Arc<Callbacks>,
    stop: &AtomicBool,
) -> io::Result<()> {
    let Frame::Hello(identity, ..) = net::read_frame(&mut secure)? else {
        return Err(io::Error::other("oturum isteği geçersiz"));
    };
    let desktop_flags = identity
        .lines()
        .nth(2)
        .and_then(|flags| flags.trim().parse::<u32>().ok())
        .unwrap_or(0xFF);
    PEER_HARDWARE.store(
        desktop_flags != 0xFF && desktop_flags & SUPPORTS_HARDWARE != 0,
        Ordering::Relaxed,
    );
    let prefs = PREFS.load(Ordering::Relaxed);
    let flags = (prefs as u32 & 0x1F)
        | SUPPORTS_PREFS
        | SUPPORTS_SCREEN
        | SUPPORTS_SCREEN2
        | SUPPORTS_HARDWARE
        | clipboard::SUPPORTED;
    net::write_frame(
        &mut secure,
        &Frame::Size(
            config.width,
            config.height,
            format!("{}\n{}\n{flags}", config.id, config.name),
        ),
    )?;
    let ip = secure.peer_addr()?.ip().to_string();
    *LINK.lock().unwrap() = Some(Link {
        ip,
        secret,
        hash: secure.hash,
        clipboard_images: desktop_flags & clipboard::SUPPORTED != 0,
    });
    *WRITER.lock().unwrap() = Some(secure.try_clone()?);
    callbacks.connected(true);
    callbacks.status("Bilgisayara bağlı");
    let _ = send(&Frame::Prefs(prefs));
    let result = read_session(&mut secure, callbacks, stop);
    WRITER.lock().unwrap().take();
    end_media(callbacks);
    LINK.lock().unwrap().take();
    callbacks.release();
    callbacks.connected(false);
    result
}

fn read_session(
    secure: &mut Secure,
    callbacks: &Arc<Callbacks>,
    stop: &AtomicBool,
) -> io::Result<()> {
    while !stop.load(Ordering::Relaxed) {
        match net::read_frame(secure)? {
            Frame::Enter(position, side) => callbacks.enter(position, side),
            Frame::Events(events) => callbacks.input(&events),
            Frame::Release | Frame::Reclaim => callbacks.release(),
            Frame::ClipText(text) if clipboard::can_receive() => callbacks.clipboard(&text),
            Frame::Ping => send(&Frame::Ping)?,
            Frame::SetPref(pref, on) => set_pref(pref, on, callbacks)?,
            Frame::AudioReq(kind, on) if kind < 2 => audio_request(kind, on, callbacks),
            Frame::ScreenOpts(options) => start_screen(options, callbacks),
            Frame::Screen(SCREEN_START) => start_screen(Vec::new(), callbacks),
            Frame::Screen(SCREEN_STOP) => stop_screen(callbacks),
            Frame::ScreenInput(events) => callbacks.screen_input(&events),
            Frame::ScreenInfo(text) => callbacks.view_info(text.lines().next() == Some("1")),
            Frame::Device(op, data) if PEER_HARDWARE.load(Ordering::Relaxed) => {
                callbacks.device(op, &data)
            }
            Frame::Screen(SCREEN_DENIED) => {
                stop_view();
                callbacks.view_ended(
                    "Bilgisayar ekran paylaşımına izin vermiyor. Bilgisayarda bu telefonun altındaki üst satırdan Ekran simgesini açın.",
                );
            }
            Frame::Screen(SCREEN_FAILED) => {
                stop_view();
                callbacks.view_ended("Bilgisayar ekranını yakalayamadı.");
            }
            _ => {}
        }
    }
    Ok(())
}

fn set_pref(pref: u8, on: bool, callbacks: &Callbacks) -> io::Result<()> {
    if pref > 8 {
        return Ok(());
    }
    if on {
        callbacks.pref_on(pref);
        return Ok(());
    }
    let bits = PREFS.fetch_and(!(1u16 << pref), Ordering::Relaxed) & !(1u16 << pref);
    callbacks.pref_off(pref);
    send(&Frame::Prefs(bits))
}

fn audio_request(kind: u8, on: bool, callbacks: &Callbacks) {
    let slot = kind as usize;
    if !on {
        AUDIO_OUT.lock().unwrap()[slot].take();
        callbacks.audio_request(kind, false);
        return;
    }
    let allowed = if kind == 0 { PREF_AUDIO_OUT } else { PREF_MIC_OUT };
    if PREFS.load(Ordering::Relaxed) & allowed == 0 {
        callbacks.activity("Bilgisayar ses istedi ama bu cihazda paylaşım kapalı");
        return;
    }
    let Some(link) = LINK.lock().unwrap().clone() else {
        return;
    };
    let key = net::audio_key(&link.hash, kind, false);
    match AudioOut::new(&link.ip, media::AUDIO_PORT + kind as u16, key) {
        Ok(output) => {
            AUDIO_OUT.lock().unwrap()[slot] = Some(output);
            callbacks.audio_request(kind, true);
        }
        Err(error) => callbacks.activity(&format!("Ses gönderilemedi: {error}")),
    }
}

fn start_screen(options: Vec<u8>, callbacks: &Arc<Callbacks>) {
    if !SCREEN_ALLOWED.load(Ordering::Relaxed) {
        let _ = send(&Frame::Screen(SCREEN_DENIED));
        return;
    }
    if SCREEN_TX.lock().unwrap().is_some() {
        callbacks.screen_start(&options);
        return;
    }
    let Some(link) = LINK.lock().unwrap().clone() else {
        return;
    };
    if SCREEN_CONNECTING.swap(true, Ordering::AcqRel) {
        return;
    }
    let callbacks = callbacks.clone();
    thread::spawn(move || {
        match media::connect_screen(&link.ip, &link.secret) {
            Ok(stream) => {
                *SCREEN_TX.lock().unwrap() = Some(stream);
                callbacks.screen_start(&options);
            }
            Err(error) => {
                callbacks.activity(&format!("Ekran paylaşımı bağlanamadı: {error}"));
                let _ = send(&Frame::Screen(SCREEN_FAILED));
            }
        }
        SCREEN_CONNECTING.store(false, Ordering::Release);
    });
}

fn stop_screen(callbacks: &Callbacks) {
    SCREEN_TX.lock().unwrap().take();
    callbacks.screen_stop();
}

fn stop_view() {
    if let Some(handle) = VIEW.lock().unwrap().take() {
        handle.stop.store(true, Ordering::Relaxed);
        if let Some(stream) = handle.stream.lock().unwrap().as_ref() {
            stream.shutdown();
        }
    }
}

fn watch_screen(on: bool, options: Vec<u8>, callbacks: Arc<Callbacks>) {
    stop_view();
    if !on {
        let _ = send(&Frame::Screen(SCREEN_STOP));
        return;
    }
    let Some(link) = LINK.lock().unwrap().clone() else {
        callbacks.view_ended("Bilgisayarla bağlantı yok");
        return;
    };
    let listener = match TcpListener::bind(("0.0.0.0", media::SCREEN_PORT)) {
        Ok(listener) => listener,
        Err(error) => {
            callbacks.view_ended(&format!("Ekran alıcısı açılamadı: {error}"));
            return;
        }
    };
    listener.set_nonblocking(true).ok();
    let handle = ViewHandle {
        stop: Arc::new(AtomicBool::new(false)),
        stream: Arc::new(Mutex::new(None)),
    };
    *VIEW.lock().unwrap() = Some(handle.clone());
    let _ = send(&Frame::ScreenOpts(options));
    thread::spawn(move || run_view(listener, link, handle, callbacks));
}

fn run_view(listener: TcpListener, link: Link, handle: ViewHandle, callbacks: Arc<Callbacks>) {
    let deadline = Instant::now() + Duration::from_secs(60);
    let stream = loop {
        if handle.stop.load(Ordering::Relaxed) {
            return;
        }
        match listener.accept() {
            Ok((stream, from)) if from.ip().to_string() == link.ip => break stream,
            Ok(_) => {}
            Err(_) if Instant::now() > deadline => {
                callbacks.view_ended(
                    "Bilgisayar ekran göndermedi. Bilgisayarda Ekran açık ve güvenlik duvarı 24806 portuna izin vermeli.",
                );
                return;
            }
            Err(_) => thread::sleep(Duration::from_millis(50)),
        }
    };
    drop(listener);
    stream.set_nonblocking(false).ok();
    stream.set_nodelay(true).ok();
    let mut secure = match net::handshake(stream, false, &link.secret) {
        Ok(secure) => secure,
        Err(error) => {
            callbacks.view_ended(&format!("Ekran bağlantısı kurulamadı: {error}"));
            return;
        }
    };
    *handle.stream.lock().unwrap() = secure.try_clone().ok();
    secure.set_read_timeout(Some(Duration::from_secs(90))).ok();
    while !handle.stop.load(Ordering::Relaxed) {
        match media::read_screen_message(&mut secure) {
            Ok(media::ScreenMessage::Frame {
                width,
                height,
                jpeg,
            }) => callbacks.view_frame(width, height, &jpeg),
            Ok(media::ScreenMessage::Failed(reason)) => {
                callbacks.view_ended(&format!("Bilgisayar: {reason}"));
                break;
            }
            Err(error) => {
                if !handle.stop.load(Ordering::Relaxed) {
                    callbacks.view_ended(&format!("Ekran paylaşımı kesildi: {error}"));
                }
                break;
            }
        }
    }
    handle.stream.lock().unwrap().take();
}

fn end_media(callbacks: &Callbacks) {
    if CAMERA_TX.lock().unwrap().take().is_some() {
        callbacks.device(DEVICE_CAMERA_STOP, &[]);
    }
    stop_view();
    stop_screen(callbacks);
    let active: Vec<usize> = {
        let mut output = AUDIO_OUT.lock().unwrap();
        (0..2).filter(|slot| output[*slot].take().is_some()).collect()
    };
    for slot in active {
        callbacks.audio_request(slot as u8, false);
    }
    let mut input = AUDIO_IN.lock().unwrap();
    input[0].take();
    input[1].take();
}

fn serve_files(callbacks: Arc<Callbacks>, stop: Arc<AtomicBool>) {
    let listener = match TcpListener::bind(("0.0.0.0", files::PORT)) {
        Ok(listener) => listener,
        Err(error) => {
            callbacks.activity(&format!("Dosya sunucusu başlatılamadı: {error}"));
            return;
        }
    };
    listener.set_nonblocking(true).ok();
    thread::spawn(move || {
        while !stop.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((stream, from)) => {
                    let callbacks = callbacks.clone();
                    thread::spawn(move || {
                        file_connection(stream, from.ip().to_string(), &callbacks)
                    });
                }
                Err(error) if error.kind() == ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(100));
                }
                Err(_) => thread::sleep(Duration::from_millis(300)),
            }
        }
    });
}

fn file_connection(stream: TcpStream, ip: String, callbacks: &Callbacks) {
    let Some(link) = LINK.lock().unwrap().clone().filter(|link| link.ip == ip) else {
        return;
    };
    stream.set_nonblocking(false).ok();
    stream.set_nodelay(true).ok();
    let Ok(secure) = net::handshake(stream, false, &link.secret) else {
        return;
    };
    secure.set_read_timeout(Some(Duration::from_secs(60))).ok();
    secure.set_write_timeout(Some(Duration::from_secs(60))).ok();
    let allowed = PREFS.load(Ordering::Relaxed) & PREF_FILES_IN != 0;
    let notify = |text: &str| callbacks.activity(text);
    let mut secure = secure;
    use std::io::Read;
    let mut magic = [0; 4];
    if secure.read_exact(&mut magic).is_err() {
        return;
    }
    if &magic == clipboard::MAGIC {
        let _ = clipboard::receive(&mut secure, &link, callbacks);
    } else {
        let roots = ROOTS.lock().unwrap().clone();
        if let Some(roots) = roots {
            let _ = files::serve_connection(&mut secure, magic, &roots, allowed, &notify);
        }
    }
}

fn connect_transfer(link: &Link) -> io::Result<Secure> {
    let address: SocketAddr = format!("{}:{}", link.ip, files::PORT)
        .parse()
        .map_err(io::Error::other)?;
    let stream = TcpStream::connect_timeout(&address, Duration::from_secs(3))?;
    stream.set_nodelay(true).ok();
    let secure = net::handshake(stream, true, &link.secret)?;
    secure.set_read_timeout(Some(Duration::from_secs(60)))?;
    secure.set_write_timeout(Some(Duration::from_secs(60)))?;
    Ok(secure)
}

fn send_file(file: File, name: String, size: u64, callbacks: Arc<Callbacks>) {
    let outcome = (|| -> io::Result<()> {
        let link = LINK
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| io::Error::other("bilgisayarla bağlantı yok"))?;
        let mut secure = connect_transfer(&link)?;
        let mut source = file;
        files::send(&mut secure, &name, size, &mut source)
    })();
    match outcome {
        Ok(()) => callbacks.activity(&format!("Bilgisayara gönderildi: {name}")),
        Err(error) => callbacks.activity(&format!("{name} gönderilemedi: {error}")),
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_bytaymur_mause_NativeBridge_configureFiles(
    mut env: JNIEnv,
    _this: JObject,
    share_root: JString,
    inbox: JString,
) {
    if let (Some(share), Some(inbox)) = (text(&mut env, &share_root), text(&mut env, &inbox)) {
        *ROOTS.lock().unwrap() = Some(Arc::new(files::Roots {
            share: PathBuf::from(share),
            inbox: PathBuf::from(inbox),
        }));
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_bytaymur_mause_NativeBridge_deviceReply(
    env: JNIEnv,
    _this: JObject,
    op: jint,
    data: JByteArray,
) {
    if !PEER_HARDWARE.load(Ordering::Relaxed) {
        return;
    }
    let bytes = env.convert_byte_array(&data).unwrap_or_default();
    let _ = send(&Frame::Device(op as u8, bytes));
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_bytaymur_mause_NativeBridge_cameraOpen(
    _env: JNIEnv,
    _this: JObject,
) -> jboolean {
    let Some(link) = LINK.lock().unwrap().clone() else {
        return 0;
    };
    match media::connect_stream(&link.ip, media::CAMERA_PORT, &link.secret) {
        Ok(stream) => {
            *CAMERA_TX.lock().unwrap() = Some(stream);
            1
        }
        Err(_) => 0,
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_bytaymur_mause_NativeBridge_cameraFrame(
    env: JNIEnv,
    _this: JObject,
    jpeg: JByteArray,
    length: jint,
    width: jint,
    height: jint,
) -> jboolean {
    let Ok(mut bytes) = env.convert_byte_array(&jpeg) else {
        return 0;
    };
    bytes.truncate(length.max(0) as usize);
    let frame = media::frame_bytes(&bytes, width as u16, height as u16);
    let mut guard = CAMERA_TX.lock().unwrap();
    let Some(stream) = guard.as_mut() else {
        return 0;
    };
    if stream.write_all(&frame).is_ok() {
        1
    } else {
        guard.take();
        0
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_bytaymur_mause_NativeBridge_cameraClose(
    _env: JNIEnv,
    _this: JObject,
) {
    CAMERA_TX.lock().unwrap().take();
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_bytaymur_mause_NativeBridge_watchScreen(
    env: JNIEnv,
    _this: JObject,
    on: jboolean,
    options: JByteArray,
) {
    let options = env.convert_byte_array(&options).unwrap_or_default();
    if let Some(callbacks) = CALLBACKS.lock().unwrap().clone() {
        watch_screen(on != 0, options, callbacks);
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_bytaymur_mause_NativeBridge_sendScreenInput(
    env: JNIEnv,
    _this: JObject,
    events: JIntArray,
) {
    let length = env.get_array_length(&events).unwrap_or(0).max(0) as usize;
    let mut values = vec![0 as jint; length];
    if env.get_int_array_region(&events, 0, &mut values).is_err() {
        return;
    }
    let events: Vec<(u16, u16, i32)> = values
        .as_chunks::<3>()
        .0
        .iter()
        .map(|triple| (triple[0] as u16, triple[1] as u16, triple[2]))
        .collect();
    if !events.is_empty() {
        let _ = send(&Frame::ScreenInput(events));
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_bytaymur_mause_NativeBridge_setPrefs(
    _env: JNIEnv,
    _this: JObject,
    bits: jint,
) {
    let bits = bits as u16;
    PREFS.store(bits, Ordering::Relaxed);
    let _ = send(&Frame::Prefs(bits));
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_bytaymur_mause_NativeBridge_setScreenAllowed(
    _env: JNIEnv,
    _this: JObject,
    allowed: jboolean,
) {
    SCREEN_ALLOWED.store(allowed != 0, Ordering::Relaxed);
    let callbacks = CALLBACKS.lock().unwrap().clone();
    if let (0, Some(callbacks)) = (allowed, callbacks) {
        stop_screen(&callbacks);
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_bytaymur_mause_NativeBridge_requestAudio(
    _env: JNIEnv,
    _this: JObject,
    kind: jint,
    on: jboolean,
) {
    if !(0..2).contains(&kind) {
        return;
    }
    let (kind, slot) = (kind as u8, kind as usize);
    if on == 0 {
        AUDIO_IN.lock().unwrap()[slot].take();
        let _ = send(&Frame::AudioReq(kind, false));
        return;
    }
    let (Some(link), Some(callbacks)) = (
        LINK.lock().unwrap().clone(),
        CALLBACKS.lock().unwrap().clone(),
    ) else {
        return;
    };
    AUDIO_IN.lock().unwrap()[slot].take();
    let socket = match media::listen_audio(media::AUDIO_PORT + kind as u16) {
        Ok(socket) => socket,
        Err(error) => {
            callbacks.activity(&format!("Ses alıcısı açılamadı: {error}"));
            return;
        }
    };
    let key = net::audio_key(&link.hash, kind, true);
    let input = media::start_audio_in(socket, link.ip, key, move |pcm| {
        callbacks.audio_play(kind, pcm)
    });
    AUDIO_IN.lock().unwrap()[slot] = Some(input);
    let _ = send(&Frame::AudioReq(kind, true));
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_bytaymur_mause_NativeBridge_audioPacket(
    env: JNIEnv,
    _this: JObject,
    kind: jint,
    pcm: JByteArray,
    length: jint,
) {
    if !(0..2).contains(&kind) {
        return;
    }
    let Ok(mut bytes) = env.convert_byte_array(&pcm) else {
        return;
    };
    bytes.truncate(length.max(0) as usize);
    if let Some(output) = AUDIO_OUT.lock().unwrap()[kind as usize].as_mut() {
        let _ = output.send(&bytes);
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_bytaymur_mause_NativeBridge_screenFrame(
    env: JNIEnv,
    _this: JObject,
    jpeg: JByteArray,
    length: jint,
    width: jint,
    height: jint,
) -> jboolean {
    let Ok(mut bytes) = env.convert_byte_array(&jpeg) else {
        return 0;
    };
    bytes.truncate(length.max(0) as usize);
    let frame = media::frame_bytes(&bytes, width as u16, height as u16);
    let mut guard = SCREEN_TX.lock().unwrap();
    let Some(stream) = guard.as_mut() else {
        return 0;
    };
    if stream.write_all(&frame).is_ok() {
        1
    } else {
        guard.take();
        0
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_bytaymur_mause_NativeBridge_screenInfo(
    mut env: JNIEnv,
    _this: JObject,
    control: jboolean,
    label: JString,
) {
    let Some(label) = text(&mut env, &label) else {
        return;
    };
    let _ = send(&Frame::ScreenInfo(format!("{control}\n{label}")));
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_bytaymur_mause_NativeBridge_screenFailed(
    mut env: JNIEnv,
    _this: JObject,
    reason: JString,
) {
    let reason = text(&mut env, &reason).unwrap_or_default();
    if let Some(mut stream) = SCREEN_TX.lock().unwrap().take() {
        let _ = stream.write_all(&media::error_frame(&reason));
    } else {
        let _ = send(&Frame::Screen(SCREEN_FAILED));
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_bytaymur_mause_NativeBridge_sendFile(
    mut env: JNIEnv,
    _this: JObject,
    descriptor: jint,
    name: JString,
    size: jlong,
) {
    let name = text(&mut env, &name).unwrap_or_default();
    let file = unsafe { File::from_raw_fd(descriptor) };
    let Some(callbacks) = CALLBACKS.lock().unwrap().clone() else {
        return;
    };
    let name = files_name(&name);
    thread::spawn(move || send_file(file, name, size.max(0) as u64, callbacks));
}

fn files_name(name: &str) -> String {
    let clean = clean(name);
    if clean.is_empty() { "dosya".into() } else { clean }
}

fn clean(value: &str) -> String {
    value.replace(['|', '\n', '\r', '\t'], " ").trim().to_owned()
}

fn parse_identity(value: &str) -> (String, String) {
    let mut lines = value.lines();
    (
        clean(lines.next().unwrap_or("")),
        clean(lines.next().unwrap_or("Bilgisayar")),
    )
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn load_pairs(path: &Path) -> HashMap<String, Pair> {
    fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(3, '\t');
            Some((
                parts.next()?.to_owned(),
                Pair {
                    name: parts.next()?.to_owned(),
                    secret: parts.next()?.to_owned(),
                },
            ))
        })
        .collect()
}

fn save_pairs(path: &Path, pairs: &HashMap<String, Pair>) -> io::Result<()> {
    let mut rows: Vec<String> = pairs
        .iter()
        .map(|(id, pair)| format!("{}\t{}\t{}", clean(id), clean(&pair.name), pair.secret))
        .collect();
    rows.sort();
    let temporary = path.with_extension("tmp");
    fs::write(&temporary, rows.join("\n"))?;
    fs::rename(temporary, path)
}
