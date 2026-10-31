// Copyright © LiveCaptions-TranslatorTray contributors
// SPDX-License-Identifier: GPL-3.0-only

use i_slint_core::{Brush, api::Window, items::WindowItem, window::WindowInner};
use skia_safe::{PixelGeometry, SurfaceProps};
use windows::Win32::UI::WindowsAndMessaging::{
    FE_FONTSMOOTHINGCLEARTYPE, FE_FONTSMOOTHINGORIENTATIONBGR, FE_FONTSMOOTHINGORIENTATIONRGB,
    SPI_GETFONTSMOOTHING, SPI_GETFONTSMOOTHINGORIENTATION, SPI_GETFONTSMOOTHINGTYPE,
    SYSTEM_PARAMETERS_INFO_ACTION, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW,
};

// LCD coverage is valid only when the frame has an opaque, known backdrop.
// Read on each rendered frame so changing Windows font smoothing takes effect
// without recreating the app. This never changes the user's Windows settings.
pub(crate) fn surface_props(window: &Window) -> SurfaceProps {
    let opaque = WindowInner::from_pub(window)
        .window_item_rc()
        .and_then(|item| item.downcast::<WindowItem>())
        .is_some_and(|item| {
            matches!(item.as_pin_ref().background(), Brush::SolidColor(c) if c.alpha() == 255)
        });
    let geometry = if opaque
        && parameter(SPI_GETFONTSMOOTHING).is_some_and(|value| value != 0)
        && parameter(SPI_GETFONTSMOOTHINGTYPE) == Some(FE_FONTSMOOTHINGCLEARTYPE)
    {
        match parameter(SPI_GETFONTSMOOTHINGORIENTATION) {
            Some(FE_FONTSMOOTHINGORIENTATIONRGB) => PixelGeometry::RGBH,
            Some(FE_FONTSMOOTHINGORIENTATIONBGR) => PixelGeometry::BGRH,
            _ => PixelGeometry::Unknown,
        }
    } else {
        PixelGeometry::Unknown
    };
    SurfaceProps::new(Default::default(), geometry)
}

fn parameter(action: SYSTEM_PARAMETERS_INFO_ACTION) -> Option<u32> {
    let mut value = 0u32;
    // All three GET actions above write a 32-bit BOOL/UINT to pvParam.
    unsafe {
        SystemParametersInfoW(
            action,
            0,
            Some((&mut value as *mut u32).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    }
    .ok()
    .map(|()| value)
}
