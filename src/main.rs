use std::process::Command;

use num_traits::FromPrimitive;

#[derive(num_derive::FromPrimitive, Debug)]
enum PixelFormat {
    A8 = 0x00000008,
    JPEG = 0x00000100,
    La88 = 0x0000000a,
    L8 = 0x00000009,
    OPAQUE = 0xffffffff,
    Rgba1010102 = 0x0000002b,
    Rgba4444 = 0x00000007,
    Rgba5551 = 0x00000006,
    Rgba8888 = 0x00000001,
    RgbaF16 = 0x00000016,
    Rgbx8888 = 0x00000002,
    Rgb332 = 0x0000000b,
    Rgb565 = 0x00000004,
    Rgb888 = 0x00000003,
    TRANSLUCENT = 0xfffffffd,
    TRANSPARENT = 0xfffffffe,
    UNKNOWN = 0x00000000,
    YcbCr420Sp = 0x00000011,
    YcbCr422I = 0x00000014,
    YcbCr422Sp = 0x00000010,
}

fn capture_screen() -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    // Run the screencap command and capture its output
    let output = Command::new("screencap").output()?;

    if !output.status.success() {
        return Err("screencap command failed".into());
    }

    Ok(output.stdout)
}

// If you need to parse the header information separately:
fn parse_screen_capture() -> Result<(u32, u32, PixelFormat, Vec<u8>), Box<dyn std::error::Error>> {
    let data = capture_screen()?;

    if data.len() < 12 {
        // Minimum size for header (3 * 4 bytes)
        return Err("Invalid screencap data".into());
    }

    // Parse header information
    let width = u32::from_le_bytes(data[0..4].try_into()?);
    let height = u32::from_le_bytes(data[4..8].try_into()?);
    let pixel_format = u32::from_le_bytes(data[8..12].try_into()?);
    let pixel_format = PixelFormat::from_u32(pixel_format)
        .ok_or_else(|| format!("Invalisd PixelFormat {pixel_format}"))?;

    // Get pixel data (everything after the 12-byte header)
    let pixel_data = data[12..].to_vec();

    Ok((width, height, pixel_format, pixel_data))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Starting");
    // If you just need the raw data
    let raw_data = capture_screen()?;
    println!("Captured {} bytes", raw_data.len());

    // If you need the parsed data
    let (width, height, format, pixels) = parse_screen_capture()?;
    println!(
        "Captured image: {}x{} (format: {:#?})",
        width, height, format
    );
    println!("Pixel data size: {} bytes", pixels.len());

    Ok(())
}
