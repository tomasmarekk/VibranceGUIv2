//! Read-only executable metadata shown in the program library.
//! Icons and descriptions come from PE resources; the executable is never started.

use std::{ffi::c_void, ptr};

use windows_sys::Win32::{
    Graphics::Gdi::{
        BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, DeleteObject, GetDC,
        GetDIBits, GetObjectW, HBITMAP, HDC, ReleaseDC,
    },
    Storage::FileSystem::{GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW},
    UI::{
        Shell::SHDefExtractIconW,
        WindowsAndMessaging::{DestroyIcon, GetIconInfo, HICON, ICONINFO},
    },
};

/// Upper bound for icon edges; larger bitmaps indicate a malformed resource.
const MAX_ICON_EDGE: i32 = 512;
/// Descriptions longer than this are rarely names, so the file stem reads better.
const MAX_DESCRIPTION_CHARS: usize = 80;

fn wide(value: &str) -> Option<Vec<u16>> {
    (!value.contains('\0')).then(|| value.encode_utf16().chain(std::iter::once(0)).collect())
}

struct OwnedIcon(HICON);

impl Drop for OwnedIcon {
    fn drop(&mut self) {
        // SAFETY: the handle came from SHDefExtractIconW and is destroyed exactly once.
        unsafe {
            DestroyIcon(self.0);
        }
    }
}

struct OwnedBitmap(HBITMAP);

impl Drop for OwnedBitmap {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: GetIconInfo transfers ownership of copies of both icon bitmaps.
            unsafe {
                DeleteObject(self.0);
            }
        }
    }
}

struct ScreenDc(HDC);

impl ScreenDc {
    fn new() -> Option<Self> {
        // SAFETY: a null window requests the screen DC, which ReleaseDC returns on drop.
        let dc = unsafe { GetDC(ptr::null_mut()) };
        (!dc.is_null()).then_some(Self(dc))
    }
}

impl Drop for ScreenDc {
    fn drop(&mut self) {
        // SAFETY: the DC was obtained by GetDC for the screen and is released once.
        unsafe {
            ReleaseDC(ptr::null_mut(), self.0);
        }
    }
}

/// Reads a bitmap as top-down 32-bit BGRA rows of exactly `width * height` pixels.
fn bitmap_bgra(dc: &ScreenDc, bitmap: HBITMAP, width: i32, height: i32) -> Option<Vec<u8>> {
    let mut info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: u32::try_from(size_of::<BITMAPINFOHEADER>()).ok()?,
            biWidth: width,
            biHeight: -height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            ..Default::default()
        },
        ..Default::default()
    };
    let rows = u32::try_from(height).ok()?;
    let mut pixels = vec![0u8; usize::try_from(width).ok()? * usize::try_from(height).ok()? * 4];
    // SAFETY: `pixels` holds every requested 32-bit row and `info` describes that layout;
    // 32-bit BI_RGB output has no color table beyond the BITMAPINFO allocation.
    let copied = unsafe {
        GetDIBits(
            dc.0,
            bitmap,
            0,
            rows,
            pixels.as_mut_ptr().cast(),
            &mut info,
            DIB_RGB_COLORS,
        )
    };
    (u32::try_from(copied).ok() == Some(rows)).then_some(pixels)
}

/// Converts icon BGRA data to straight RGBA, deriving alpha from the AND mask
/// for legacy icons whose color bitmap carries no alpha channel.
fn icon_rgba(color: &[u8], mask: Option<&[u8]>) -> Vec<u8> {
    let (pixels, _) = color.as_chunks::<4>();
    let has_alpha = pixels.iter().any(|&[_, _, _, alpha]| alpha != 0);
    let mask = mask.map(|mask| mask.as_chunks::<4>().0);
    pixels
        .iter()
        .enumerate()
        .flat_map(|(index, &[blue, green, red, alpha])| {
            let alpha = if has_alpha {
                alpha
            } else {
                // A black AND-mask pixel is opaque; a white one shows the background.
                mask.and_then(|mask| mask.get(index))
                    .map_or(255, |&[value, ..]| if value == 0 { 255 } else { 0 })
            };
            [red, green, blue, alpha]
        })
        .collect()
}

fn encode_png(rgba: &[u8], width: u32, height: u32) -> Option<Vec<u8>> {
    let mut output = Vec::new();
    let mut encoder = png::Encoder::new(&mut output, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::Fast);
    let mut writer = encoder.write_header().ok()?;
    writer.write_image_data(rgba).ok()?;
    writer.finish().ok()?;
    Some(output)
}

/// Extracts an executable's primary icon at `size` pixels as a straight-alpha PNG.
///
/// Returns `None` for missing or inaccessible files and executables without icons;
/// callers then show their own placeholder.
pub(crate) fn executable_icon_png(path: &str, size: u16) -> Option<Vec<u8>> {
    let path = wide(path)?;
    let mut handle: HICON = ptr::null_mut();
    let size = u32::from(size);
    // SAFETY: `path` is NUL-terminated and `handle` is a writable slot; the small icon is
    // optional and not requested. The shell only reads resources from the file.
    let result = unsafe {
        SHDefExtractIconW(
            path.as_ptr(),
            0,
            0,
            &mut handle,
            ptr::null_mut(),
            size | (size << 16),
        )
    };
    if result != 0 || handle.is_null() {
        return None;
    }
    let icon = OwnedIcon(handle);
    // SAFETY: ICONINFO is plain data that GetIconInfo fully initializes on success.
    let mut info: ICONINFO = unsafe { std::mem::zeroed() };
    // SAFETY: the icon handle is live and `info` is a writable ICONINFO.
    if unsafe { GetIconInfo(icon.0, &mut info) } == 0 {
        return None;
    }
    let color = OwnedBitmap(info.hbmColor);
    let mask = OwnedBitmap(info.hbmMask);
    // Monochrome icons store both masks in hbmMask and are not used by modern executables.
    if color.0.is_null() {
        return None;
    }
    let mut bitmap = BITMAP::default();
    // SAFETY: `bitmap` is a writable BITMAP and the byte count matches its size.
    let written = unsafe {
        GetObjectW(
            color.0,
            i32::try_from(size_of::<BITMAP>()).ok()?,
            (&mut bitmap as *mut BITMAP).cast::<c_void>(),
        )
    };
    let (width, height) = (bitmap.bmWidth, bitmap.bmHeight);
    if written == 0
        || !(1..=MAX_ICON_EDGE).contains(&width)
        || !(1..=MAX_ICON_EDGE).contains(&height)
    {
        return None;
    }
    let dc = ScreenDc::new()?;
    let color_pixels = bitmap_bgra(&dc, color.0, width, height)?;
    let mask_pixels = if color_pixels
        .as_chunks::<4>()
        .0
        .iter()
        .all(|&[_, _, _, alpha]| alpha == 0)
        && !mask.0.is_null()
    {
        bitmap_bgra(&dc, mask.0, width, height)
    } else {
        None
    };
    let rgba = icon_rgba(&color_pixels, mask_pixels.as_deref());
    encode_png(
        &rgba,
        u32::try_from(width).ok()?,
        u32::try_from(height).ok()?,
    )
}

/// Returns a version-resource value inside `block`, or `None` if absent.
///
/// `unit` is the byte size of one reported length unit: string values report
/// UTF-16 characters, binary values report bytes.
fn query_value<'a>(block: &'a [u8], key: &str, unit: usize) -> Option<&'a [u8]> {
    let key = wide(key)?;
    let mut value: *mut c_void = ptr::null_mut();
    let mut length = 0u32;
    // SAFETY: `block` is the buffer filled by GetFileVersionInfoW and `key` is NUL-terminated.
    if unsafe { VerQueryValueW(block.as_ptr().cast(), key.as_ptr(), &mut value, &mut length) } == 0
        || value.is_null()
    {
        return None;
    }
    let start = value.addr().checked_sub(block.as_ptr().addr())?;
    let end = start.checked_add(usize::try_from(length).ok()?.checked_mul(unit)?)?;
    // Only resource data inside the owned block is read; anything else is ignored.
    block.get(start..end.min(block.len()))
}

fn utf16_string(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&pair| u16::from_le_bytes(pair))
        .take_while(|unit| *unit != 0)
        .collect();
    String::from_utf16_lossy(&units)
}

/// Reads a human-readable application name from the executable's version resource.
///
/// File descriptions are preferred because Windows shows them in Task Manager;
/// product names are a fallback. Unusable values return `None`.
pub(crate) fn executable_description(path: &str) -> Option<String> {
    let wide_path = wide(path)?;
    let mut ignored = 0;
    // SAFETY: the path is NUL-terminated and `ignored` is a writable output.
    let size = unsafe { GetFileVersionInfoSizeW(wide_path.as_ptr(), &mut ignored) };
    if size == 0 {
        return None;
    }
    let mut block = vec![0u8; usize::try_from(size).ok()?];
    // SAFETY: `block` provides exactly `size` writable bytes for the version resource.
    if unsafe { GetFileVersionInfoW(wide_path.as_ptr(), 0, size, block.as_mut_ptr().cast()) } == 0 {
        return None;
    }
    let mut languages: Vec<String> = query_value(&block, "\\VarFileInfo\\Translation", 1)
        .map(|translations| {
            translations
                .as_chunks::<4>()
                .0
                .iter()
                .map(
                    |&[language_low, language_high, codepage_low, codepage_high]| {
                        let language = u16::from_le_bytes([language_low, language_high]);
                        let codepage = u16::from_le_bytes([codepage_low, codepage_high]);
                        format!("{language:04x}{codepage:04x}")
                    },
                )
                .collect()
        })
        .unwrap_or_default();
    languages.extend(["040904b0", "040904e4", "04090000"].map(String::from));
    ["FileDescription", "ProductName"]
        .into_iter()
        .flat_map(|field| {
            languages
                .iter()
                .map(move |language| format!("\\StringFileInfo\\{language}\\{field}"))
        })
        .find_map(|key| {
            let value = utf16_string(query_value(&block, &key, 2)?);
            let value = value.trim();
            (!value.is_empty()
                && value.chars().count() <= MAX_DESCRIPTION_CHARS
                && !value.chars().any(char::is_control))
            .then(|| value.to_owned())
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn system_executable() -> Option<String> {
        let root = std::env::var("SystemRoot").ok()?;
        let path = format!("{root}\\explorer.exe");
        std::path::Path::new(&path).is_file().then_some(path)
    }

    #[test]
    fn alpha_icons_keep_their_alpha_and_swap_channel_order() {
        let rgba = icon_rgba(&[1, 2, 3, 128, 4, 5, 6, 0], None);
        assert_eq!(rgba, [3, 2, 1, 128, 6, 5, 4, 0]);
    }

    #[test]
    fn legacy_icons_take_transparency_from_the_and_mask() {
        let color = [10, 20, 30, 0, 40, 50, 60, 0];
        let mask = [0, 0, 0, 0, 255, 255, 255, 0];
        assert_eq!(
            icon_rgba(&color, Some(&mask)),
            [30, 20, 10, 255, 60, 50, 40, 0]
        );
    }

    #[test]
    fn encoded_icons_are_png_images_of_the_requested_size() {
        let png = encode_png(&[255; 2 * 3 * 4], 2, 3).unwrap();
        assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
        let decoder = png::Decoder::new(std::io::Cursor::new(png));
        let info = decoder.read_info().unwrap().info().clone();
        assert_eq!((info.width, info.height), (2, 3));
    }

    #[test]
    fn missing_and_malformed_paths_have_no_metadata() {
        assert_eq!(executable_icon_png("C:\\missing\\nothing.exe", 64), None);
        assert_eq!(executable_icon_png("bad\0path.exe", 64), None);
        assert_eq!(executable_description("C:\\missing\\nothing.exe"), None);
    }

    #[test]
    fn windows_shell_executable_provides_icon_and_description() {
        let Some(path) = system_executable() else {
            eprintln!("skipped: explorer.exe is not installed on this Windows image");
            return;
        };
        let png = executable_icon_png(&path, 64).expect("explorer.exe has an icon resource");
        assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
        let description = executable_description(&path).expect("explorer.exe has a description");
        assert!(!description.is_empty());
    }
}
