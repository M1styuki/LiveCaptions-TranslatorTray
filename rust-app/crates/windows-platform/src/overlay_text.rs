//! Thread-local DirectWrite layouts and real glyph-outline drawing into one
//! premultiplied BGRA DIB. No Slint transparency or repeated-offset text copies.
use std::{ffi::c_void, ptr::NonNull};
use windows::{
    Win32::{
        Foundation::{E_INVALIDARG, E_NOTIMPL, RECT},
        Graphics::{
            Direct2D::{Common::*, *},
            DirectWrite::*,
            Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM,
            Gdi::*,
        },
    },
    core::{BOOL, IUnknown, Interface, Ref, Result, implement, w},
};
use windows_numerics::{Matrix3x2, Vector2};

pub fn color(rgb: u32, alpha: u8) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: ((rgb >> 16) & 255) as f32 / 255.0,
        g: ((rgb >> 8) & 255) as f32 / 255.0,
        b: (rgb & 255) as f32 / 255.0,
        a: alpha as f32 / 255.0,
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextStyle {
    pub size: f32,
    pub bold: bool,
    pub rgb: u32,
    pub stroke: f32,
    pub prefix: Option<ColorPrefix>,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColorPrefix {
    pub utf16_len: u32,
    pub rgb: u32,
}

struct LayoutCache {
    text: String,
    width: f32,
    size: f32,
    bold: bool,
    layout: IDWriteTextLayout,
}

pub struct Canvas {
    factory: ID2D1Factory,
    write: IDWriteFactory,
    target: Option<ID2D1DCRenderTarget>,
    pub dc: HDC,
    bitmap: HBITMAP,
    previous: HGDIOBJ,
    bits: NonNull<u8>,
    pub width: i32,
    pub height: i32,
    layouts: Vec<Option<LayoutCache>>,
    icons: Vec<Option<Vec<ID2D1PathGeometry>>>,
}

impl Canvas {
    pub fn new(width: i32, height: i32, scale: f32) -> Result<Self> {
        if !(1..=16_384).contains(&width)
            || !(1..=16_384).contains(&height)
            || !scale.is_finite()
            || scale <= 0.0
        {
            return Err(E_INVALIDARG.into());
        }
        // SAFETY: the factories, DC and selected DIB are owned and used on this
        // thread only. Failure paths release objects already created.
        unsafe {
            let factory: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let write: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let properties = D2D1_RENDER_TARGET_PROPERTIES {
                r#type: D2D1_RENDER_TARGET_TYPE_SOFTWARE,
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                },
                dpiX: 96.0 * scale,
                dpiY: 96.0 * scale,
                ..Default::default()
            };
            let target = factory.CreateDCRenderTarget(&properties)?;
            let dc = CreateCompatibleDC(None);
            if dc.is_invalid() {
                return Err(windows::core::Error::from_thread());
            }
            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: width,
                    biHeight: -height,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut pointer = std::ptr::null_mut();
            let bitmap =
                match CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut pointer, None, 0) {
                    Ok(bitmap) => bitmap,
                    Err(error) => {
                        let _ = DeleteDC(dc);
                        return Err(error);
                    }
                };
            let previous = SelectObject(dc, HGDIOBJ(bitmap.0));
            let canvas = Self {
                factory,
                write,
                target: Some(target),
                dc,
                bitmap,
                previous,
                bits: NonNull::new(pointer.cast()).expect("DIB pixels"),
                width,
                height,
                layouts: (0..15).map(|_| None).collect(),
                icons: (0..12).map(|_| None).collect(),
            };
            canvas.target().BindDC(
                dc,
                &RECT {
                    left: 0,
                    top: 0,
                    right: width,
                    bottom: height,
                },
            )?;
            canvas
                .target()
                .SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
            Ok(canvas)
        }
    }
    fn target(&self) -> &ID2D1DCRenderTarget {
        self.target.as_ref().expect("live canvas")
    }
    pub fn begin(&self, rgb: u32, opacity: u8) {
        // SAFETY: all drawing takes place on this DC's owning thread.
        unsafe {
            self.target().BeginDraw();
            self.target().Clear(Some(&color(rgb, opacity)));
        }
    }
    pub fn finish(&self) -> Result<()> {
        unsafe { self.target().EndDraw(None, None) }
    }
    pub fn text(
        &mut self,
        slot: usize,
        text: &str,
        x: f32,
        y: f32,
        width: f32,
        style: &TextStyle,
    ) -> Result<f32> {
        if text.is_empty() {
            return Ok(0.0);
        }
        if slot >= self.layouts.len() || width <= 0.0 {
            return Err(E_INVALIDARG.into());
        }
        let reuse = self.layouts[slot].as_ref().is_some_and(|item| {
            item.text == text
                && item.width == width
                && item.size == style.size
                && item.bold == style.bold
        });
        unsafe {
            if !reuse {
                let format = self.write.CreateTextFormat(
                    w!("Segoe UI"),
                    None,
                    if style.bold {
                        DWRITE_FONT_WEIGHT_BOLD
                    } else {
                        DWRITE_FONT_WEIGHT_NORMAL
                    },
                    DWRITE_FONT_STYLE_NORMAL,
                    DWRITE_FONT_STRETCH_NORMAL,
                    style.size,
                    w!("zh-CN"),
                )?;
                format.SetWordWrapping(DWRITE_WORD_WRAPPING_WRAP)?;
                let layout = self.write.CreateTextLayout(
                    &text.encode_utf16().collect::<Vec<_>>(),
                    &format,
                    width,
                    10_000.0,
                )?;
                self.layouts[slot] = Some(LayoutCache {
                    text: text.into(),
                    width,
                    size: style.size,
                    bold: style.bold,
                    layout,
                });
            }
            let layout = &self.layouts[slot].as_ref().unwrap().layout;
            // Effects are presentation only. Clear the previous draw's range
            // even when this layout is reused after a style/history edit.
            let length = text.encode_utf16().count() as u32;
            layout.SetDrawingEffect(
                None::<&IUnknown>,
                DWRITE_TEXT_RANGE {
                    startPosition: 0,
                    length,
                },
            )?;
            if let Some(prefix) = style.prefix.filter(|value| value.utf16_len > 0) {
                let brush = self
                    .target()
                    .CreateSolidColorBrush(&color(prefix.rgb, 255), None)?;
                let effect: IUnknown = brush.cast()?;
                layout.SetDrawingEffect(
                    &effect,
                    DWRITE_TEXT_RANGE {
                        startPosition: 0,
                        length: prefix.utf16_len.min(length),
                    },
                )?;
            }
            let fill = self
                .target()
                .CreateSolidColorBrush(&color(style.rgb, 255), None)?;
            let outline = self.target().CreateSolidColorBrush(&color(0, 255), None)?;
            let renderer: IDWriteTextRenderer = OutlineRenderer {
                factory: self.factory.clone(),
                target: self.target().clone(),
                fill,
                outline,
                stroke: style.stroke,
            }
            .into();
            layout.Draw(None, &renderer, x, y)?;
            let mut metrics = DWRITE_TEXT_METRICS::default();
            layout.GetMetrics(&mut metrics)?;
            Ok(metrics.height)
        }
    }
    pub fn rectangle(&self, rect: D2D_RECT_F, rgb: u32, opacity: u8) -> Result<()> {
        unsafe {
            let brush = self
                .target()
                .CreateSolidColorBrush(&color(rgb, opacity), None)?;
            self.target().FillRectangle(&rect, &brush);
        }
        Ok(())
    }
    pub fn rounded_rectangle(
        &self,
        rect: D2D_RECT_F,
        radius: f32,
        rgb: u32,
        opacity: u8,
    ) -> Result<()> {
        unsafe {
            let brush = self
                .target()
                .CreateSolidColorBrush(&color(rgb, opacity), None)?;
            self.target().FillRoundedRectangle(
                &D2D1_ROUNDED_RECT {
                    rect,
                    radiusX: radius,
                    radiusY: radius,
                },
                &brush,
            );
        }
        Ok(())
    }
    pub(crate) fn icon(
        &mut self,
        index: usize,
        x: f32,
        y: f32,
        size: f32,
        opacity: u8,
        rgb: u32,
    ) -> Result<()> {
        let shape = crate::toolbar_icons::TOOLBAR[index];
        if self.icons[index].is_none() {
            self.icons[index] = Some(self.icon_paths(shape)?);
        }
        self.fill_icon(
            self.icons[index].as_ref().unwrap(),
            x,
            y,
            size / shape.size,
            opacity,
            rgb,
        )
    }
    fn icon_paths(&self, icon: &crate::toolbar_icons::Icon) -> Result<Vec<ID2D1PathGeometry>> {
        use crate::toolbar_icons::Command;
        icon.paths
            .iter()
            .map(|path| unsafe {
                let geometry = self.factory.CreatePathGeometry()?;
                let sink = geometry.Open()?;
                sink.SetFillMode(if path.even_odd {
                    D2D1_FILL_MODE_ALTERNATE
                } else {
                    D2D1_FILL_MODE_WINDING
                });
                for command in path.commands {
                    match *command {
                        Command::Move(x, y) => {
                            sink.BeginFigure(Vector2 { X: x, Y: y }, D2D1_FIGURE_BEGIN_FILLED)
                        }
                        Command::Line(x, y) => sink.AddLine(Vector2 { X: x, Y: y }),
                        Command::Cubic([x1, y1, x2, y2, x3, y3]) => {
                            sink.AddBezier(&D2D1_BEZIER_SEGMENT {
                                point1: Vector2 { X: x1, Y: y1 },
                                point2: Vector2 { X: x2, Y: y2 },
                                point3: Vector2 { X: x3, Y: y3 },
                            })
                        }
                        Command::Close => sink.EndFigure(D2D1_FIGURE_END_CLOSED),
                    }
                }
                sink.Close()?;
                Ok(geometry)
            })
            .collect()
    }
    fn fill_icon(
        &self,
        paths: &[ID2D1PathGeometry],
        x: f32,
        y: f32,
        scale: f32,
        opacity: u8,
        rgb: u32,
    ) -> Result<()> {
        // The checked-in SVGs are compiled to native geometry during development.
        // There is no SVG parser, icon font or UI runtime on the overlay thread.
        unsafe {
            let brush = self
                .target()
                .CreateSolidColorBrush(&color(rgb, opacity), None)?;
            let transform = Matrix3x2 {
                M11: scale,
                M22: scale,
                M31: x,
                M32: y,
                ..Default::default()
            };
            for path in paths {
                let geometry = self.factory.CreateTransformedGeometry(path, &transform)?;
                self.target()
                    .FillGeometry(&geometry, &brush, None::<&ID2D1Brush>);
            }
        }
        Ok(())
    }
    pub fn pixels(&self) -> &[u8] {
        // SAFETY: DIB allocation covers width*height*4; this borrow cannot outlive
        // the bitmap. Callers inspect only after EndDraw/GDI flush.
        unsafe {
            std::slice::from_raw_parts(
                self.bits.as_ptr(),
                self.width as usize * self.height as usize * 4,
            )
        }
    }
}
impl Drop for Canvas {
    fn drop(&mut self) {
        self.target.take();
        // SAFETY: restore the previous GDI object before deleting the owned DIB/DC.
        unsafe {
            SelectObject(self.dc, self.previous);
            let _ = DeleteObject(HGDIOBJ(self.bitmap.0));
            let _ = DeleteDC(self.dc);
        }
    }
}

#[implement(IDWriteTextRenderer)]
struct OutlineRenderer {
    factory: ID2D1Factory,
    target: ID2D1DCRenderTarget,
    fill: ID2D1SolidColorBrush,
    outline: ID2D1SolidColorBrush,
    stroke: f32,
}

#[allow(non_snake_case)]
impl IDWritePixelSnapping_Impl for OutlineRenderer_Impl {
    fn IsPixelSnappingDisabled(&self, _: *const c_void) -> Result<BOOL> {
        Ok(false.into())
    }
    fn GetCurrentTransform(&self, _: *const c_void, transform: *mut DWRITE_MATRIX) -> Result<()> {
        if transform.is_null() {
            return Err(E_INVALIDARG.into());
        }
        unsafe {
            *transform = DWRITE_MATRIX {
                m11: 1.0,
                m22: 1.0,
                ..Default::default()
            };
        }
        Ok(())
    }
    fn GetPixelsPerDip(&self, _: *const c_void) -> Result<f32> {
        let mut x = 96.0;
        let mut y = 96.0;
        unsafe {
            self.target.GetDpi(&mut x, &mut y);
        }
        Ok(x / 96.0)
    }
}

#[allow(non_snake_case)]
impl IDWriteTextRenderer_Impl for OutlineRenderer_Impl {
    fn DrawGlyphRun(
        &self,
        _: *const c_void,
        x: f32,
        y: f32,
        _: DWRITE_MEASURING_MODE,
        run: *const DWRITE_GLYPH_RUN,
        _: *const DWRITE_GLYPH_RUN_DESCRIPTION,
        effect: Ref<IUnknown>,
    ) -> Result<()> {
        if run.is_null() {
            return Err(E_INVALIDARG.into());
        }
        // SAFETY: DirectWrite owns this live glyph run/font face for the callback.
        // Its shaped glyph indices/offsets include script shaping and fallback.
        unsafe {
            let run = &*run;
            let face = run.fontFace.as_ref().ok_or(E_INVALIDARG)?;
            let path = self.factory.CreatePathGeometry()?;
            let sink = path.Open()?;
            face.GetGlyphRunOutline(
                run.fontEmSize,
                run.glyphIndices,
                Some(run.glyphAdvances),
                Some(run.glyphOffsets),
                run.glyphCount,
                run.isSideways.as_bool(),
                !run.bidiLevel.is_multiple_of(2),
                &sink,
            )?;
            sink.Close()?;
            let transform = Matrix3x2 {
                M11: 1.0,
                M22: 1.0,
                M31: x,
                M32: y,
                ..Default::default()
            };
            let geometry = self.factory.CreateTransformedGeometry(&path, &transform)?;
            if self.stroke > 0.0 {
                self.target
                    .DrawGeometry(&geometry, &self.outline, self.stroke, None);
            }
            let effect_fill = effect
                .as_ref()
                .and_then(|effect| effect.cast::<ID2D1SolidColorBrush>().ok());
            self.target
                .FillGeometry(&geometry, effect_fill.as_ref().unwrap_or(&self.fill), None);
        }
        Ok(())
    }
    fn DrawUnderline(
        &self,
        _: *const c_void,
        x: f32,
        y: f32,
        line: *const DWRITE_UNDERLINE,
        _: Ref<IUnknown>,
    ) -> Result<()> {
        if line.is_null() {
            return Err(E_INVALIDARG.into());
        }
        unsafe {
            let line = &*line;
            self.target.FillRectangle(
                &D2D_RECT_F {
                    left: x,
                    top: y + line.offset,
                    right: x + line.width,
                    bottom: y + line.offset + line.thickness,
                },
                &self.fill,
            );
        }
        Ok(())
    }
    fn DrawStrikethrough(
        &self,
        _: *const c_void,
        x: f32,
        y: f32,
        line: *const DWRITE_STRIKETHROUGH,
        _: Ref<IUnknown>,
    ) -> Result<()> {
        if line.is_null() {
            return Err(E_INVALIDARG.into());
        }
        unsafe {
            let line = &*line;
            self.target.FillRectangle(
                &D2D_RECT_F {
                    left: x,
                    top: y + line.offset,
                    right: x + line.width,
                    bottom: y + line.offset + line.thickness,
                },
                &self.fill,
            );
        }
        Ok(())
    }
    fn DrawInlineObject(
        &self,
        _: *const c_void,
        _: f32,
        _: f32,
        _: Ref<IDWriteInlineObject>,
        _: BOOL,
        _: BOOL,
        _: Ref<IUnknown>,
    ) -> Result<()> {
        Err(E_NOTIMPL.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn toolbar_vectors_fit_buttons_and_render_at_multiple_scales() {
        for scale in [1.0, 1.25, 1.5, 2.0] {
            let mut canvas =
                Canvas::new((336.0 * scale) as i32, (28.0 * scale) as i32, scale).unwrap();
            canvas.begin(0, 0);
            for index in 0..12 {
                canvas
                    .icon(index, index as f32 * 28.0 + 5.0, 5.0, 18.0, 255)
                    .unwrap();
            }
            canvas.finish().unwrap();
            for index in 0..12 {
                let left = (index as f32 * 28.0 * scale) as usize;
                let right = ((index + 1) as f32 * 28.0 * scale) as usize;
                let count = canvas
                    .pixels()
                    .chunks_exact(canvas.width as usize * 4)
                    .flat_map(|row| row[left * 4..right * 4].as_chunks::<4>().0.iter())
                    .filter(|p| p[3] > 0)
                    .count();
                assert!(count > 20, "icon {index} at scale {scale} must be visible");
            }
            if let Some(directory) = std::env::var_os("LIVE_CAPTIONS_EVIDENCE_DIR") {
                write_bmp(
                    &canvas,
                    &std::path::PathBuf::from(directory).join(format!("toolbar-{scale}.bmp")),
                );
            }
        }
    }

    fn write_bmp(canvas: &Canvas, path: &std::path::Path) {
        let mut bmp = Vec::new();
        bmp.extend_from_slice(b"BM");
        bmp.extend_from_slice(&(54 + canvas.pixels().len() as u32).to_le_bytes());
        bmp.extend_from_slice(&[0; 4]);
        bmp.extend_from_slice(&54u32.to_le_bytes());
        bmp.extend_from_slice(&40u32.to_le_bytes());
        bmp.extend_from_slice(&canvas.width.to_le_bytes());
        bmp.extend_from_slice(&(-canvas.height).to_le_bytes());
        bmp.extend_from_slice(&1u16.to_le_bytes());
        bmp.extend_from_slice(&32u16.to_le_bytes());
        bmp.extend_from_slice(&[0; 24]);
        bmp.extend_from_slice(canvas.pixels());
        std::fs::write(path, bmp).unwrap();
    }

    #[test]
    #[ignore = "development asset generator; renders into memory only, writes checked-in tray ICO resources"]
    fn regenerate_tray_icon_resources() {
        for (name, icon) in [
            ("tray-enabled", &crate::toolbar_icons::TRAY_ENABLED),
            ("tray-disabled", &crate::toolbar_icons::TRAY_DISABLED),
        ] {
            let mut images = Vec::new();
            for size in [16i32, 20, 24, 32, 48, 64] {
                let canvas = Canvas::new(size, size, 1.0).unwrap();
                canvas.begin(0, 0);
                let paths = canvas.icon_paths(icon).unwrap();
                canvas
                    .fill_icon(&paths, 0.0, 0.0, size as f32 / icon.size, 255, 0xffffff)
                    .unwrap();
                canvas.finish().unwrap();
                let mask_stride = ((size + 31) / 32 * 4) as usize;
                let mut dib = Vec::new();
                dib.extend_from_slice(&40u32.to_le_bytes());
                dib.extend_from_slice(&size.to_le_bytes());
                dib.extend_from_slice(&(size * 2).to_le_bytes());
                dib.extend_from_slice(&1u16.to_le_bytes());
                dib.extend_from_slice(&32u16.to_le_bytes());
                dib.extend_from_slice(&[0; 24]);
                for row in canvas.pixels().chunks_exact(size as usize * 4).rev() {
                    for p in row.as_chunks::<4>().0.iter() {
                        dib.extend_from_slice(&[255, 255, 255, p[3]]);
                    }
                }
                for row in canvas.pixels().chunks_exact(size as usize * 4).rev() {
                    let mut mask = vec![0; mask_stride];
                    for (x, p) in row.as_chunks::<4>().0.iter().enumerate() {
                        if p[3] == 0 {
                            mask[x / 8] |= 0x80 >> (x % 8);
                        }
                    }
                    dib.extend_from_slice(&mask);
                }
                images.push((size as u8, dib));
            }
            let mut ico = vec![0, 0, 1, 0, images.len() as u8, 0];
            let mut offset = 6 + images.len() as u32 * 16;
            for (size, image) in &images {
                ico.extend_from_slice(&[*size, *size, 0, 0, 1, 0, 32, 0]);
                ico.extend_from_slice(&(image.len() as u32).to_le_bytes());
                ico.extend_from_slice(&offset.to_le_bytes());
                offset += image.len() as u32;
            }
            for (_, image) in images {
                ico.extend_from_slice(&image);
            }
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join(format!("../app/resources/{name}.ico"));
            std::fs::write(path, ico).unwrap();
        }
    }
    #[test]
    fn cached_layout_keeps_previous_translation_color_and_clears_old_effects() {
        let mut canvas = Canvas::new(700, 100, 1.0).unwrap();
        let text = "前文😀 MMM 当前NNN";
        let mut style = TextStyle {
            size: 32.0,
            bold: false,
            rgb: 0xff0000,
            stroke: 1.5,
            prefix: Some(ColorPrefix {
                utf16_len: 9,
                rgb: 0x00ff00,
            }),
        };
        let colors = |canvas: &Canvas| {
            let pixels = canvas.pixels().as_chunks::<4>().0;
            (
                pixels
                    .iter()
                    .filter(|pixel| pixel[3] == 255 && pixel[1] > 240 && pixel[2] < 10)
                    .count(),
                pixels
                    .iter()
                    .filter(|pixel| pixel[3] == 255 && pixel[2] > 240 && pixel[1] < 10)
                    .count(),
            )
        };
        canvas.begin(0, 0);
        let height = canvas.text(0, text, 8.0, 8.0, 680.0, &style).unwrap();
        canvas.finish().unwrap();
        let (green, red) = colors(&canvas);
        assert!(
            green > 20 && red > 20,
            "prefix and current text must both have opaque glyph interiors"
        );
        style.prefix = None;
        canvas.begin(0, 0);
        assert_eq!(
            canvas.text(0, text, 8.0, 8.0, 680.0, &style).unwrap(),
            height
        );
        canvas.finish().unwrap();
        let (green, red) = colors(&canvas);
        assert_eq!(
            green, 0,
            "cached prefix effects must not leak into the next caption/style"
        );
        assert!(red > 40);
    }
    #[test]
    fn native_dib_renders_shaped_multilingual_glyphs_and_keeps_text_opaque() {
        let mut canvas = Canvas::new(900, 360, 1.0).unwrap();
        canvas.begin(0x184060, 150);
        let style = TextStyle {
            size: 26.0,
            bold: false,
            rgb: 0xffffff,
            stroke: 3.0,
            prefix: None,
        };
        let mut top = 12.0;
        for (index, text) in [
            "English 与中文：字幕测试。",
            "日本語の字幕、مرحبا بالعالم",
            "Emoji 😀 🐈 🎵 — 123",
        ]
        .iter()
        .enumerate()
        {
            top += canvas.text(index, text, 12.0, top, 870.0, &style).unwrap() + 12.0;
        }
        canvas.finish().unwrap();
        let pixels = canvas.pixels();
        assert!(
            pixels
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel[3] == 255 && pixel[0] > 220 && pixel[1] > 220 && pixel[2] > 220),
            "opaque glyph interiors"
        );
        assert!(
            pixels
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel[3] == 150),
            "background alpha is applied once"
        );
        assert!(
            pixels
                .as_chunks::<4>()
                .0
                .iter()
                .all(|pixel| pixel[0] <= pixel[3] && pixel[1] <= pixel[3] && pixel[2] <= pixel[3]),
            "premultiplied BGRA"
        );
        if let Some(directory) = std::env::var_os("LIVE_CAPTIONS_EVIDENCE_DIR") {
            let mut bmp = Vec::new();
            let length = 54 + pixels.len() as u32;
            bmp.extend_from_slice(b"BM");
            bmp.extend_from_slice(&length.to_le_bytes());
            bmp.extend_from_slice(&[0; 4]);
            bmp.extend_from_slice(&54_u32.to_le_bytes());
            bmp.extend_from_slice(&40_u32.to_le_bytes());
            bmp.extend_from_slice(&canvas.width.to_le_bytes());
            bmp.extend_from_slice(&(-canvas.height).to_le_bytes());
            bmp.extend_from_slice(&1_u16.to_le_bytes());
            bmp.extend_from_slice(&32_u16.to_le_bytes());
            bmp.extend_from_slice(&[0; 24]);
            bmp.extend_from_slice(pixels);
            std::fs::write(
                std::path::PathBuf::from(directory).join("native-text-multilingual.bmp"),
                bmp,
            )
            .unwrap();
        }
    }
}
