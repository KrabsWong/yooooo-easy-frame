//! Main-thread access to Sparkle. Source-only builds do not load an updater.

#[cfg(feature = "sparkle")]
mod native {
    use std::ffi::c_char;

    unsafe extern "C" {
        pub fn easy_frame_updater_start() -> bool;
        pub fn easy_frame_updater_version() -> *const c_char;
        pub fn easy_frame_updater_available() -> bool;
        pub fn easy_frame_updater_can_check() -> bool;
        pub fn easy_frame_updater_check();
        pub fn easy_frame_updater_automatic() -> bool;
        pub fn easy_frame_updater_set_automatic(enabled: bool);
    }
}

pub fn start() {
    #[cfg(feature = "sparkle")]
    {
        unsafe {
            let _ = native::easy_frame_updater_start();
        }
    }
}

pub fn available() -> bool {
    #[cfg(feature = "sparkle")]
    {
        unsafe { native::easy_frame_updater_available() }
    }
    #[cfg(not(feature = "sparkle"))]
    {
        false
    }
}

#[allow(dead_code)]
pub fn can_check() -> bool {
    #[cfg(feature = "sparkle")]
    {
        unsafe { native::easy_frame_updater_can_check() }
    }
    #[cfg(not(feature = "sparkle"))]
    {
        false
    }
}

pub fn check() {
    #[cfg(feature = "sparkle")]
    {
        unsafe { native::easy_frame_updater_check() };
    }
}

pub fn automatic() -> bool {
    #[cfg(feature = "sparkle")]
    {
        unsafe { native::easy_frame_updater_automatic() }
    }
    #[cfg(not(feature = "sparkle"))]
    {
        false
    }
}

pub fn set_automatic(enabled: bool) {
    #[cfg(feature = "sparkle")]
    {
        unsafe { native::easy_frame_updater_set_automatic(enabled) };
    }
    #[cfg(not(feature = "sparkle"))]
    {
        let _ = enabled;
    }
}

pub fn version() -> Option<String> {
    #[cfg(feature = "sparkle")]
    unsafe {
        let value = native::easy_frame_updater_version();
        if !value.is_null() {
            return Some(
                std::ffi::CStr::from_ptr(value)
                    .to_string_lossy()
                    .into_owned(),
            );
        }
    }
    None
}
