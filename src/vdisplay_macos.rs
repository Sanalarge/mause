use core_graphics::display::CGDisplay;
use dispatch2::DispatchQueue;
use objc2::msg_send;
use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyClass, AnyObject, Bool};
use objc2_foundation::{NSArray, NSSize, NSString};
use std::io;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

const VENDOR_ID: usize = 0x4d41;
const PRODUCT_ID: usize = 0x5553;
const SERIAL: usize = 0x0001;
const REFRESH_RATE: f64 = 60.0;
const PIXELS_PER_MILLIMETER: f64 = 3.6;
const APPEAR_TIMEOUT: Duration = Duration::from_secs(4);

static OWNER: Mutex<()> = Mutex::new(());

struct Display(Retained<AnyObject>);

unsafe impl Send for Display {}

pub struct Lease {
    _display: Display,
    id: u32,
    _owner: MutexGuard<'static, ()>,
}

pub fn supported() -> bool {
    [
        "CGVirtualDisplay",
        "CGVirtualDisplayDescriptor",
        "CGVirtualDisplaySettings",
        "CGVirtualDisplayMode",
    ]
    .iter()
    .all(|name| class(name).is_some())
}

fn class(name: &str) -> Option<&'static AnyClass> {
    let name = std::ffi::CString::new(name).ok()?;
    AnyClass::get(&name)
}

fn required(name: &str) -> io::Result<&'static AnyClass> {
    class(name).ok_or_else(|| {
        io::Error::other(
            "Bu macOS sürümü sanal ekranı desteklemiyor; macOS 14 veya daha yenisi gerekir.",
        )
    })
}

fn new_object(name: &str) -> io::Result<Retained<AnyObject>> {
    let cls = required(name)?;
    let object: Option<Retained<AnyObject>> = unsafe { msg_send![cls, new] };
    object.ok_or_else(|| io::Error::other(format!("{name} oluşturulamadı")))
}

fn descriptor(width: u16, height: u16) -> io::Result<Retained<AnyObject>> {
    let descriptor = new_object("CGVirtualDisplayDescriptor")?;
    let name = NSString::from_str("Mause");
    let millimeters = NSSize::new(
        f64::from(width) / PIXELS_PER_MILLIMETER,
        f64::from(height) / PIXELS_PER_MILLIMETER,
    );
    unsafe {
        let _: () = msg_send![&*descriptor, setDispatchQueue: DispatchQueue::main()];
        let _: () = msg_send![&*descriptor, setName: &*name];
        let _: () = msg_send![&*descriptor, setMaxPixelsWide: usize::from(width)];
        let _: () = msg_send![&*descriptor, setMaxPixelsHigh: usize::from(height)];
        let _: () = msg_send![&*descriptor, setSizeInMillimeters: millimeters];
        let _: () = msg_send![&*descriptor, setVendorID: VENDOR_ID];
        let _: () = msg_send![&*descriptor, setProductID: PRODUCT_ID];
        let _: () = msg_send![&*descriptor, setSerialNum: SERIAL];
    }
    Ok(descriptor)
}

fn settings(width: u16, height: u16) -> io::Result<Retained<AnyObject>> {
    let mode_class = required("CGVirtualDisplayMode")?;
    let mode: Option<Retained<AnyObject>> = unsafe {
        let allocated: Allocated<AnyObject> = msg_send![mode_class, alloc];
        msg_send![
            allocated,
            initWithWidth: usize::from(width),
            height: usize::from(height),
            refreshRate: REFRESH_RATE
        ]
    };
    let mode = mode.ok_or_else(|| io::Error::other("sanal ekran kipi oluşturulamadı"))?;
    let modes = NSArray::from_retained_slice(&[mode]);
    let settings = new_object("CGVirtualDisplaySettings")?;
    unsafe {
        let _: () = msg_send![&*settings, setHiDPI: 0usize];
        let _: () = msg_send![&*settings, setModes: &*modes];
    }
    Ok(settings)
}

fn create(width: u16, height: u16) -> io::Result<(Display, u32)> {
    let descriptor = descriptor(width, height)?;
    let settings = settings(width, height)?;
    let display_class = required("CGVirtualDisplay")?;
    let display: Option<Retained<AnyObject>> = unsafe {
        let allocated: Allocated<AnyObject> = msg_send![display_class, alloc];
        msg_send![allocated, initWithDescriptor: &*descriptor]
    };
    let display = display.ok_or_else(|| io::Error::other("macOS sanal ekranı oluşturmadı"))?;
    let applied: Bool = unsafe { msg_send![&*display, applySettings: &*settings] };
    if !applied.as_bool() {
        return Err(io::Error::other(
            "macOS sanal ekran çözünürlüğünü kabul etmedi",
        ));
    }
    let id: u32 = unsafe { msg_send![&*display, displayID] };
    if id == 0 {
        return Err(io::Error::other("macOS sanal ekrana kimlik vermedi"));
    }
    Ok((Display(display), id))
}

fn create_on_main(width: u16, height: u16) -> io::Result<(Display, u32)> {
    let slot: Mutex<Option<io::Result<(Display, u32)>>> = Mutex::new(None);
    DispatchQueue::main().exec_sync(|| {
        *slot.lock().unwrap() = Some(create(width, height));
    });
    slot.into_inner()
        .unwrap_or(None)
        .unwrap_or_else(|| Err(io::Error::other("sanal ekran oluşturma yanıt vermedi")))
}

fn active(id: u32) -> bool {
    CGDisplay::active_displays().is_ok_and(|ids| ids.contains(&id))
}

impl Lease {
    pub fn begin((width, height): (u16, u16)) -> io::Result<Self> {
        let owner = OWNER.try_lock().map_err(|_| {
            io::Error::other("Sanal ekran zaten başka bir paylaşımda kullanılıyor.")
        })?;
        let (display, id) = create_on_main(width, height)?;
        let started = Instant::now();
        while !active(id) {
            if started.elapsed() > APPEAR_TIMEOUT {
                return Err(io::Error::other("macOS sanal ekranı etkinleştirmedi"));
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        Ok(Self {
            _display: display,
            id,
            _owner: owner,
        })
    }

    pub fn monitor_index(&self) -> Option<usize> {
        crate::platform::display_index(self.id)
    }
}
