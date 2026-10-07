//! "Extra warmth": the Accessibility Color Tint filter (System Settings →
//! Accessibility → Display → Color Filters), through MediaAccessibility.
//!
//! Night Shift stops at 2700 K and shifts the white point in a colour-managed
//! way, so it looks milder than f.lux at the same kelvin. The colour tint is
//! applied in the display pipeline like Night Shift, so it stays even across
//! apps. macOS clamps its intensity to at least 0.25 (measured: setting 0.2
//! reads back 0.25), so it can't fade in from zero.

use std::ffi::{c_void, CStr};

const FRAMEWORK: &CStr = c"/System/Library/Frameworks/MediaAccessibility.framework/MediaAccessibility";
/// `MADisplayFilterPrefGetType(1)`: category 1 is Color Filters.
const CATEGORY_COLOR: i32 = 1;
/// Filter type for "Color Tint" in that category.
pub const TYPE_COLOR_TINT: i32 = 16;
/// Lowest intensity macOS accepts.
pub const MIN_INTENSITY: f64 = 0.25;
/// Amber, close to f.lux's night colour.
pub const TINT_HUE: f64 = 0.05;

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FilterState {
    pub enabled: bool,
    #[serde(rename = "type")]
    pub kind: i32,
    pub hue: f64,
    pub intensity: f64,
}

pub trait ColorFilter {
    fn read(&self) -> FilterState;
    fn write(&self, s: FilterState);
}

pub struct MediaAccessibility {
    set_enabled: extern "C" fn(i32, bool),
    get_enabled: extern "C" fn(i32) -> bool,
    set_type: extern "C" fn(i32, i32),
    get_type: extern "C" fn(i32) -> i32,
    set_hue: extern "C" fn(f64),
    get_hue: extern "C" fn() -> f64,
    set_intensity: extern "C" fn(f64),
    get_intensity: extern "C" fn() -> f64,
}

/// Look up `name` as a function of type `F`.
///
/// Safety: `F` must be an `extern "C" fn` type matching the symbol's real signature.
unsafe fn symbol<F: Copy>(handle: *mut c_void, name: &CStr) -> Result<F, String> {
    assert_eq!(std::mem::size_of::<F>(), std::mem::size_of::<*mut c_void>());
    let p = libc::dlsym(handle, name.as_ptr());
    if p.is_null() {
        Err(format!("{} not found in MediaAccessibility", name.to_string_lossy()))
    } else {
        Ok(std::mem::transmute_copy::<*mut c_void, F>(&p))
    }
}

impl MediaAccessibility {
    pub fn open() -> Result<Self, String> {
        let h = unsafe { libc::dlopen(FRAMEWORK.as_ptr(), libc::RTLD_NOW) };
        if h.is_null() {
            return Err("MediaAccessibility framework not found".into());
        }
        // Each pointer is cast to the function's exact C signature.
        unsafe {
            Ok(MediaAccessibility {
                set_enabled: symbol(h, c"MADisplayFilterPrefSetCategoryEnabled")?,
                get_enabled: symbol(h, c"MADisplayFilterPrefGetCategoryEnabled")?,
                set_type: symbol(h, c"MADisplayFilterPrefSetType")?,
                get_type: symbol(h, c"MADisplayFilterPrefGetType")?,
                set_hue: symbol(h, c"MADisplayFilterPrefSetSingleColorHue")?,
                get_hue: symbol(h, c"MADisplayFilterPrefGetSingleColorHue")?,
                set_intensity: symbol(h, c"MADisplayFilterPrefSetSingleColorIntensity")?,
                get_intensity: symbol(h, c"MADisplayFilterPrefGetSingleColorIntensity")?,
            })
        }
    }
}

impl ColorFilter for MediaAccessibility {
    fn read(&self) -> FilterState {
        FilterState {
            enabled: (self.get_enabled)(CATEGORY_COLOR),
            kind: (self.get_type)(CATEGORY_COLOR),
            hue: (self.get_hue)(),
            intensity: (self.get_intensity)(),
        }
    }

    /// Writes only what changed: every write is a preference change macOS reacts to.
    fn write(&self, s: FilterState) {
        let cur = self.read();
        if cur.kind != s.kind {
            (self.set_type)(CATEGORY_COLOR, s.kind);
        }
        if (cur.hue - s.hue).abs() > 1e-4 {
            (self.set_hue)(s.hue);
        }
        if (cur.intensity - s.intensity).abs() > 1e-3 {
            (self.set_intensity)(s.intensity);
        }
        if cur.enabled != s.enabled {
            (self.set_enabled)(CATEGORY_COLOR, s.enabled);
        }
    }
}
