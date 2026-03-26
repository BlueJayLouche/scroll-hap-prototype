//! Video to WebP Sequence Converter
//! 
//! Converts video files to a sequence of WebP images for scroll-scrubbing.
//! Requires ffmpeg to be installed on the system.

use anyhow::{Context, Result};
use clap::Parser;
use indicatif::{ProgressBar, ProgressStyle};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Parser)]
#[command(name = "video-to-webp")]
#[command(about = "Convert video to WebP image sequence for scroll-scrubbing")]
struct Args {
    /// Input video file
    #[arg(value_name = "INPUT")]
    input: PathBuf,

    /// Output directory for frames
    #[arg(value_name = "OUTPUT_DIR")]
    output_dir: PathBuf,

    /// Target frame rate (fps)
    #[arg(short, long, default_value = "30")]
    fps: f32,

    /// Maximum width (maintains aspect ratio)
    #[arg(short, long)]
    width: Option<u32>,

    /// Maximum height (maintains aspect ratio)
    #[arg(short = 'H', long)]
    height: Option<u32>,

    /// WebP quality (0-100)
    #[arg(short, long, default_value = "85")]
    quality: u32,

    /// Start time (seconds)
    #[arg(long)]
    start: Option<f32>,

    /// Duration (seconds)
    #[arg(long)]
    duration: Option<f32>,
}

fn main() -> Result<()> {
    let args = Args::parse();

    // Validate input exists
    if !args.input.exists() {
        anyhow::bail!("Input file does not exist: {}", args.input.display());
    }

    // Check ffmpeg is available
    check_ffmpeg()?;

    // Create output directory
    std::fs::create_dir_all(&args.output_dir)
        .with_context(|| format!("Failed to create output directory: {}", args.output_dir.display()))?;

    // Get video info
    let info = get_video_info(&args.input)?;
    println!("📹 Input: {}", args.input.display());
    println!("   Resolution: {}x{}", info.width, info.height);
    println!("   Duration: {:.1}s", info.duration);
    println!("   Source FPS: {:.2}", info.fps);
    println!();

    // Calculate target dimensions
    let (target_width, target_height) = calculate_dimensions(
        info.width,
        info.height,
        args.width,
        args.height,
    );

    // Calculate total frames
    let effective_duration = args.duration.unwrap_or(info.duration - args.start.unwrap_or(0.0));
    let total_frames = (effective_duration * args.fps) as u64;

    println!("⚙️  Conversion settings:");
    println!("   Target: {}x{} @ {:.0}fps", target_width, target_height, args.fps);
    println!("   Quality: {}%", args.quality);
    println!("   Expected frames: {}", total_frames);
    println!();

    // Create progress bar
    let pb = ProgressBar::new(total_frames);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({eta})")
            .unwrap()
            .progress_chars("#>-"),
    );

    // Build ffmpeg command
    let mut cmd = Command::new("ffmpeg");
    
    // Input
    cmd.arg("-i").arg(&args.input);
    
    // Start time
    if let Some(start) = args.start {
        cmd.arg("-ss").arg(start.to_string());
    }
    
    // Duration
    if let Some(duration) = args.duration {
        cmd.arg("-t").arg(duration.to_string());
    }
    
    // Video filter for scaling
    let scale_filter = format!("scale={}:{}", target_width, target_height);
    cmd.arg("-vf").arg(format!("{},fps={}", scale_filter, args.fps));
    
    // WebP settings
    cmd.arg("-c:v").arg("libwebp");
    cmd.arg("-quality").arg(args.quality.to_string());
    cmd.arg("-compression_level").arg("6"); // 0-6, higher = smaller files
    cmd.arg("-preset").arg("picture"); // Optimized for images
    
    // Output pattern
    let output_pattern = args.output_dir.join("frame_%04d.webp");
    cmd.arg("-y"); // Overwrite
    cmd.arg(&output_pattern);

    // Run ffmpeg
    println!("🚀 Starting conversion...\n");
    
    let output = cmd.output()
        .with_context(|| "Failed to run ffmpeg. Is it installed?")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("ffmpeg failed:\n{}", stderr);
    }

    // Count output files
    let frame_count = count_frames(&args.output_dir)?;
    pb.finish_with_message(format!("✅ Converted {} frames", frame_count));

    // Calculate total size
    let total_size = calculate_dir_size(&args.output_dir)?;
    let avg_size = total_size / frame_count.max(1);

    println!("\n📊 Results:");
    println!("   Frames: {}", frame_count);
    println!("   Total size: {}", format_bytes(total_size));
    println!("   Avg frame: {}", format_bytes(avg_size));
    println!("   Est. memory: {} (if all loaded)", format_bytes(total_size * 4)); // *4 for RGBA
    println!("\n📁 Output: {}", args.output_dir.display());

    Ok(())
}

#[derive(Debug)]
struct VideoInfo {
    width: u32,
    height: u32,
    duration: f32,
    fps: f32,
}

fn check_ffmpeg() -> Result<()> {
    match Command::new("ffmpeg").arg("-version").output() {
        Ok(_) => Ok(()),
        Err(_) => anyhow::bail!(
            "ffmpeg not found. Please install ffmpeg:\n\
             - macOS: brew install ffmpeg\n\
             - Ubuntu: sudo apt install ffmpeg\n\
             - Windows: https://ffmpeg.org/download.html"
        ),
    }
}

fn get_video_info(path: &Path) -> Result<VideoInfo> {
    let output = Command::new("ffprobe")
        .args([
            "-v", "error",
            "-select_streams", "v:0",
            "-show_entries", "stream=width,height,r_frame_rate,duration",
            "-of", "csv=p=0",
        ])
        .arg(path)
        .output()
        .with_context(|| "Failed to run ffprobe")?;

    if !output.status.success() {
        anyhow::bail!("ffprobe failed");
    }

    let output_str = String::from_utf8_lossy(&output.stdout);
    let parts: Vec<&str> = output_str.trim().split(',').collect();

    if parts.len() < 4 {
        anyhow::bail!("Could not parse video info");
    }

    let width = parts[0].parse::<u32>()?;
    let height = parts[1].parse::<u32>()?;
    
    // Parse FPS fraction (e.g., "30000/1001")
    let fps_parts: Vec<&str> = parts[2].split('/').collect();
    let fps = if fps_parts.len() == 2 {
        fps_parts[0].parse::<f32>()? / fps_parts[1].parse::<f32>()?
    } else {
        parts[2].parse::<f32>()?
    };

    let duration = parts[3].parse::<f32>().unwrap_or(0.0);

    Ok(VideoInfo {
        width,
        height,
        duration,
        fps,
    })
}

fn calculate_dimensions(
    orig_width: u32,
    orig_height: u32,
    max_width: Option<u32>,
    max_height: Option<u32>,
) -> (u32, u32) {
    let mut width = orig_width;
    let mut height = orig_height;

    if let Some(max_w) = max_width {
        if width > max_w {
            height = (height as f32 * (max_w as f32 / width as f32)) as u32;
            width = max_w;
        }
    }

    if let Some(max_h) = max_height {
        if height > max_h {
            width = (width as f32 * (max_h as f32 / height as f32)) as u32;
            height = max_h;
        }
    }

    // Ensure dimensions are even (required by some codecs)
    width = (width / 2) * 2;
    height = (height / 2) * 2;

    (width, height)
}

fn count_frames(dir: &Path) -> Result<u64> {
    let count = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .filter(|e| {
            let name = e.file_name();
            let name = name.to_string_lossy();
            name.ends_with(".webp") || name.ends_with(".png") || name.ends_with(".jpg")
        })
        .count() as u64;
    Ok(count)
}

fn calculate_dir_size(dir: &Path) -> Result<u64> {
    let total: u64 = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .filter_map(|e| e.metadata().ok())
        .map(|m| m.len())
        .sum();
    Ok(total)
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB"];
    let mut size = bytes as f64;
    let mut unit_idx = 0;

    while size >= 1024.0 && unit_idx < UNITS.len() - 1 {
        size /= 1024.0;
        unit_idx += 1;
    }

    format!("{:.1} {}", size, UNITS[unit_idx])
}
