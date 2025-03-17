use clap::Parser;
use log::{debug, error, info, trace, warn};
use num_traits::FromPrimitive;
use rxing::{
    common::HybridBinarizer, qrcode::QRCodeReader, BinaryBitmap, ImmutableReader,
    Luma8LuminanceSource,
};
use serde_json::json;
use std::process::Command;
use tokio::task::JoinHandle;

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
        let bytes_per_pixel = self.format.bytes_per_pixel();
        let scale_dim = |dim| (dim * bytes_per_pixel) as usize;

        let scaled_image_width = scale_dim(self.width);
        let scaled_x = scale_dim(x);
        let scaled_width = scale_dim(width);
        let cropped = self
            .pixels
            .chunks_exact(scaled_image_width)
            .skip(y as usize)
            .take(height as usize)
            .flat_map(|f| f.iter().skip(scaled_x).take(scaled_width))
            .collect::<Vec<&u8>>();
        assert_eq!(scale_dim(width * height), cropped.len());
        let luma_vec: Vec<u8> = cropped
            .chunks(bytes_per_pixel as usize)
            .map(|pixels| self.format.get_channel(*pixels[0]))
            .collect();
        assert_eq!((width * height) as usize, luma_vec.len());

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

fn capture_screen_and_parse() -> Result<Image, Box<dyn std::error::Error>> {
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
        .ok_or_else(|| format!("Invalid PixelFormat {pixel_format}"))?;

    // Get pixel data (everything after the 12-byte header)
    let pixel_data = data[12..].to_vec();

    Ok(Image::new(width, height, pixel_format, pixel_data))
}

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// X coordinate of top left corner of crop position
    #[arg(short, default_value_t = 0)]
    x: u32,

    /// Y coordinate of top left corner of crop position
    #[arg(short, default_value_t = 0)]
    y: u32,

    /// Width of cropped region
    #[arg(long, default_value_t = 100)]
    width: u32,

    /// Height of cropped region
    #[arg(long, default_value_t = 100)]
    height: u32,

    /// KV store URL
    #[arg(long)]
    api_url: String,

    /// API token for KV store
    #[arg(short, long)]
    token: String,

    /// Loop interval. If not provided, the program will only run once.
    #[arg(long)]
    interval: Option<humantime::Duration>,

    /// Any strings provided here will be removed from the output text before sending to the KV store.
    #[arg(short, long, num_args=1..)]
    substitute: Vec<String>,

    /// The key to use for KV API
    #[arg(short, long)]
    key: String,
}

fn send_to_kv_store(text: &str, args: &Args) -> JoinHandle<()> {
    let text = text.to_string();
    let token = args.token.clone();
    let api_url = args.api_url.clone();
    let key = args.key.clone();

    let text = args
        .substitute
        .iter()
        .fold(text, |text, substitute| text.replace(substitute, ""));

    tokio::spawn(async move {
        let client = reqwest::Client::new();
        let result = client
            .post(api_url)
            .json(&json!({ key: text, "token": token }))
            .send()
            .await;
        result.map_or_else(
            |error| error!("Failed to send to KV Store {error:#?}"),
            |response| {
                info!("Successfully sent '{text}' to KV Store");
                trace!("Response: {response:#?}");
            },
        );
    })
}

enum QrCodeTask {
    NoRequest,
    Request(JoinHandle<()>),
}

impl From<()> for QrCodeTask {
    fn from(_: ()) -> Self {
        QrCodeTask::NoRequest
    }
}

fn parse_qr_code(
    args: &Args,
    reader: &QRCodeReader,
    last_result: &mut String,
) -> Result<QrCodeTask, Box<dyn std::error::Error>> {
    let image = capture_screen_and_parse()?;
    debug!("Captured image: {image:#?}");

    let mut binary_bitmap =
        image.crop_and_create_binary_bitmap(args.x, args.y, args.width, args.height);
    let result = reader.immutable_decode(&mut binary_bitmap);
    if let Ok(result) = result {
        let text = result.getText();
        debug!("Text: {}", text);
        if text != last_result {
            let task = send_to_kv_store(text, &args);
            info!("Detected new QR code '{text}'");
            *last_result = text.to_string();
            return Ok(QrCodeTask::Request(task));
        }
    };
    Ok(QrCodeTask::NoRequest)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    let args = Args::parse();
    let mut last_result = String::new();

    let reader = QRCodeReader::default();

    let interval = args.interval;

    let task = match interval {
        Some(interval) => loop {
            let iteration_start = std::time::Instant::now();
            let next_iteration = iteration_start + interval.into();
            parse_qr_code(&args, &reader, &mut last_result)?;
            let iteration_end = std::time::Instant::now();

            let parse_duration = iteration_end.duration_since(iteration_start);
            debug!(
                "Iteration took ({})",
                humantime::format_duration(parse_duration)
            );

            let sleep_duration = next_iteration.duration_since(iteration_end);
            if sleep_duration.is_zero() {
                let slow_by_duration = iteration_end.duration_since(next_iteration);
                warn!(
                    "Loop overrun! Iteration took ({}), slow by ({})",
                    humantime::format_duration(parse_duration),
                    humantime::format_duration(slow_by_duration)
                );
            } else {
                std::thread::sleep(sleep_duration);
            };
        },
        None => parse_qr_code(&args, &reader, &mut last_result)?,
    };

    match task {
        QrCodeTask::Request(task) => task.await?,
        QrCodeTask::NoRequest => (),
    }

    Ok(())
}
