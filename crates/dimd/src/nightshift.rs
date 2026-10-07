//! Night Shift through CoreBrightness's private `CBBlueLightClient`.
//!
//! Why not gamma tables like f.lux: on M5 Pro/Max Macs running macOS 26,
//! `CGSetDisplayTransferByTable` and `CGSetDisplayTransferByFormula` report
//! success but the display pipeline ignores them (Apple FB22273730,
//! developer.apple.com/forums/thread/819331). Custom ColorSync profiles do
//! change colours, but apps that colour-manage themselves (Chrome, Electron)
//! cancel or double the shift. Night Shift is applied in the display pipeline
//! and tints everything evenly, on built-in and external displays.
//!
//! Every `objc_msgSend` call is cast to the method's exact C signature: an
//! untyped call passes `float` arguments in the wrong registers, and Night
//! Shift silently receives garbage (it snaps to 0 % or 100 %).

use std::ffi::{c_char, c_void, CStr};

type Id = *mut c_void;
type Sel = *mut c_void;

#[link(name = "objc")]
extern "C" {
    fn objc_getClass(name: *const c_char) -> Id;
    fn sel_registerName(name: *const c_char) -> Sel;
    fn objc_msgSend();
    fn objc_autoreleasePoolPush() -> *mut c_void;
    fn objc_autoreleasePoolPop(pool: *mut c_void);
}

const FRAMEWORK: &CStr = c"/System/Library/PrivateFrameworks/CoreBrightness.framework/CoreBrightness";

/// `getBlueLightStatus:` fills this; type encoding `{?=BBBi{?={?=ii}{?=ii}}QB}`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
struct RawStatus {
    active: bool,
    enabled: bool,
    sun_schedule_permitted: bool,
    mode: i32,
    schedule: [i32; 4],
    disable_flags: u64,
    available: bool,
}

/// Night Shift's state as far as we care.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct State {
    pub enabled: bool,
    /// 0 = manual, 1 = sunset to sunrise, 2 = custom schedule.
    pub mode: i32,
    /// Colour temperature while enabled.
    pub kelvin: f32,
    /// 0..1, the slider in System Settings.
    pub strength: f32,
}

/// What the daemon needs from Night Shift. A trait so the daemon logic can be
/// tested without touching the real display.
pub trait NightShift {
    fn read(&self) -> Result<State, String>;
    fn set_enabled(&self, on: bool) -> Result<(), String>;
    fn set_mode(&self, mode: i32) -> Result<(), String>;
    fn set_kelvin(&self, kelvin: f32) -> Result<(), String>;
    fn set_strength(&self, strength: f32) -> Result<(), String>;
}

pub struct CoreBrightness {
    client: Id,
}

// The client is an XPC proxy; we only ever use it behind the daemon's mutex.
unsafe impl Send for CoreBrightness {}

fn sel(name: &CStr) -> Sel {
    unsafe { sel_registerName(name.as_ptr()) }
}

/// Run `f` inside an autorelease pool: the daemon has no run loop to drain one.
fn pooled<T>(f: impl FnOnce() -> T) -> T {
    unsafe {
        let pool = objc_autoreleasePoolPush();
        let r = f();
        objc_autoreleasePoolPop(pool);
        r
    }
}

fn check(ok: bool, what: &str) -> Result<(), String> {
    if ok {
        Ok(())
    } else {
        Err(format!("Night Shift refused {what}"))
    }
}

impl CoreBrightness {
    pub fn open() -> Result<Self, String> {
        pooled(|| unsafe {
            if libc::dlopen(FRAMEWORK.as_ptr(), libc::RTLD_NOW).is_null() {
                return Err("CoreBrightness framework not found".into());
            }
            let class = objc_getClass(c"CBBlueLightClient".as_ptr());
            if class.is_null() {
                return Err("CBBlueLightClient not available on this macOS".into());
            }
            let supported: extern "C" fn(Id, Sel) -> bool = std::mem::transmute(objc_msgSend as *const ());
            if !supported(class, sel(c"supportsBlueLightReduction")) {
                return Err("this Mac doesn't support Night Shift".into());
            }
            let new: extern "C" fn(Id, Sel) -> Id = std::mem::transmute(objc_msgSend as *const ());
            let client = new(class, sel(c"new"));
            if client.is_null() {
                return Err("couldn't create a CBBlueLightClient".into());
            }
            Ok(CoreBrightness { client })
        })
    }

    fn get_f32(&self, name: &CStr) -> Option<f32> {
        let mut v = f32::NAN;
        let f: extern "C" fn(Id, Sel, *mut f32) -> bool = unsafe { std::mem::transmute(objc_msgSend as *const ()) };
        pooled(|| f(self.client, sel(name), &mut v)).then_some(v)
    }

    fn call_bool(&self, name: &CStr, arg: bool) -> bool {
        let f: extern "C" fn(Id, Sel, bool) -> bool = unsafe { std::mem::transmute(objc_msgSend as *const ()) };
        pooled(|| f(self.client, sel(name), arg))
    }
}

impl NightShift for CoreBrightness {
    fn read(&self) -> Result<State, String> {
        let mut raw = RawStatus::default();
        let status: extern "C" fn(Id, Sel, *mut RawStatus) -> bool =
            unsafe { std::mem::transmute(objc_msgSend as *const ()) };
        if !pooled(|| status(self.client, sel(c"getBlueLightStatus:"), &mut raw)) {
            return Err("couldn't read Night Shift status".into());
        }
        if !raw.available {
            return Err("Night Shift is unavailable right now".into());
        }
        Ok(State {
            enabled: raw.enabled,
            mode: raw.mode,
            kelvin: self.get_f32(c"getCCT:").ok_or("couldn't read Night Shift temperature")?,
            strength: self.get_f32(c"getStrength:").ok_or("couldn't read Night Shift strength")?,
        })
    }

    fn set_enabled(&self, on: bool) -> Result<(), String> {
        check(self.call_bool(c"setEnabled:", on), "on/off")
    }

    fn set_mode(&self, mode: i32) -> Result<(), String> {
        let f: extern "C" fn(Id, Sel, i32) -> bool = unsafe { std::mem::transmute(objc_msgSend as *const ()) };
        check(pooled(|| f(self.client, sel(c"setMode:"), mode)), "schedule mode")
    }

    fn set_kelvin(&self, kelvin: f32) -> Result<(), String> {
        let f: extern "C" fn(Id, Sel, f32, bool) -> bool = unsafe { std::mem::transmute(objc_msgSend as *const ()) };
        check(pooled(|| f(self.client, sel(c"setCCT:commit:"), kelvin, true)), "the colour temperature")
    }

    fn set_strength(&self, strength: f32) -> Result<(), String> {
        let f: extern "C" fn(Id, Sel, f32, bool) -> bool = unsafe { std::mem::transmute(objc_msgSend as *const ()) };
        check(pooled(|| f(self.client, sel(c"setStrength:commit:"), strength, true)), "the strength")
    }
}
