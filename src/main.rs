use clap::Parser;
use num_traits::FromPrimitive;
use rxing::{
    common::HybridBinarizer, qrcode::QRCodeReader, BinaryBitmap, ImmutableReader,
    Luma8LuminanceSource,
};
use std::process::Command;

#[derive(num_derive::FromPrimitive, Debug)]
enum PixelFormat {
    A8 = 0x00000008,
    RGBA4444 = 0x00000007,
    RGBA8888 = 0x00000001,
    RGB565 = 0x00000004,
}

impl PixelFormat {
    fn bytes_per_pixel(&self) -> u32 {
        match self {
            PixelFormat::A8 => 1,
            PixelFormat::RGBA4444 => 2,
            PixelFormat::RGBA8888 => 4,
            PixelFormat::RGB565 => 2,
        }
    }

    // Gets some arbitrary channel from the low byte of data
    fn get_channel(&self, byte: u8) -> u8 {
        match self {
            PixelFormat::A8 => byte,
            PixelFormat::RGBA4444 => byte & 0xF,
            PixelFormat::RGBA8888 => byte,
            PixelFormat::RGB565 => byte & 0x1F,
        }
    }
}

struct Image {
    width: u32,
    height: u32,
    format: PixelFormat,
    pixels: Vec<u8>,
}

impl std::fmt::Debug for Image {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}x{} (format: {:#?})",
            self.width, self.height, self.format,
        )
    }
}

impl Image {
    fn new(width: u32, height: u32, format: PixelFormat, pixels: Vec<u8>) -> Image {
        Image {
            width,
            height,
            format,
            pixels,
        }
    }

    fn crop_and_create_binary_bitmap(
        &self,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    ) -> BinaryBitmap<HybridBinarizer<Luma8LuminanceSource>> {
        // Calculate the offset and size of the cropped image
        let offset = (y * self.width + x) * self.format.bytes_per_pixel();
        let size = width * height * self.format.bytes_per_pixel();
        let (offset, size) = (offset as usize, size as usize);

        let cropped = &self.pixels[offset..offset + size];
        let luma_vec = cropped
            .chunks(self.format.bytes_per_pixel() as usize)
            .map(|pixels| self.format.get_channel(pixels[0]))
            .collect();

        BinaryBitmap::new(HybridBinarizer::new(Luma8LuminanceSource::new(
            luma_vec, width, height,
        )))
    }
}

fn capture_screen() -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    // Run the screencap command and capture its output
    let output = Command::new("screencap").output()?;

    if !output.status.success() {
        return Err("screencap command failed".into());
    }

    Ok(output.stdout)
}

fn parse_screen_capture() -> Result<Image, Box<dyn std::error::Error>> {
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

    Ok(Image::new(width, height, pixel_format, pixel_data))
}

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// API token for KV store
    #[arg(short, long)]
    token: String,

    /// X coordinate of top left corner of crop position
    #[arg(short, default_value_t = 0)]
    x: u32,

    /// Y coordinate of top left corner of crop position
    #[arg(short, default_value_t = 0)]
    y: u32,

    /// Width of cropped region
    #[arg(short, long, default_value_t = 100)]
    width: u32,

    /// Height of cropped region
    #[arg(long, default_value_t = 100)]
    height: u32,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Starting");
    let args = Args::parse();

    let reader = QRCodeReader::default();

    let raw_data = capture_screen()?;
    println!("Captured {} bytes", raw_data.len());

    let image = parse_screen_capture()?;
    println!("Captured image: {:#?}", image);
    println!("Pixel data size: {} bytes", image.pixels.len());

    let mut binary_bitmap =
        image.crop_and_create_binary_bitmap(args.x, args.y, args.width, args.height);
    let result = reader.immutable_decode(&mut binary_bitmap)?;
    let text = result.getText();
    println!("Text: {}", text);

    Ok(())
}
