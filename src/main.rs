use std::{process::Command, thread, time::{Duration, Instant}};
use indicatif::{ProgressBar, ProgressStyle};
use chrono::Local;
use std::path::Path;
use anyhow::{Result, Context};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use ctrlc;
use std::fs;

fn main() -> Result<()> {
    // Configuration
    let interval_secs = 120;
    let output_dir = Path::new("captures");
    
    // Create output directory if it doesn't exist
    std::fs::create_dir_all(output_dir)?;

    // Set up Ctrl+C handler
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    ctrlc::set_handler(move || {
        println!("\nShutting down...");
        r.store(false, Ordering::SeqCst);
    })?;

    // Set up progress bar
    let progress = ProgressBar::new_spinner();
    progress.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner} {msg}")
            .unwrap()
    );

    let mut photo_count = 0;

    // Main capture loop
    while running.load(Ordering::SeqCst) {
        let timestamp = Local::now().format("%Y%m%d_%H%M%S").to_string();
        let nef_path = output_dir.join(format!("{}.nef", timestamp));
        let avif_path = output_dir.join(format!("{}.avif", timestamp));

        photo_count += 1;
        progress.set_message(format!("Taking photo #{}", photo_count));

        // Capture with gphoto2
        let capture_result = Command::new("gphoto2")
            .args([
                "--capture-image-and-download",
                "--filename",
                nef_path.to_str().unwrap(),
            ])
            .output()
            .context("Failed to capture image with gphoto2")?;

        if !capture_result.status.success() {
            eprintln!("gphoto2 error: {}", String::from_utf8_lossy(&capture_result.stderr));
            continue;
        }

        // Process with dcraw and pipe directly to ffmpeg for AVIF conversion
        let dcraw = Command::new("dcraw")
            .args(["-c", "-w", nef_path.to_str().unwrap()])
            .stdout(std::process::Stdio::piped())
            .spawn()
            .context("Failed to start dcraw")?;

        let ffmpeg_result = Command::new("ffmpeg")
            .args([
                "-i", "pipe:0",
                "-c:v", "libsvtav1",
                "-preset", "8",        // Balance between speed and quality (0-13)
                "-crf", "38",         // Quality setting (0-63, lower is better)
                "-svtav1-params", "tune=0", // Optional film grain synthesis
                avif_path.to_str().unwrap(),
            ])
            .stdin(dcraw.stdout.unwrap())
            .output()
            .context("Failed to process with ffmpeg")?;

        if !ffmpeg_result.status.success() {
            eprintln!("ffmpeg error: {}", String::from_utf8_lossy(&ffmpeg_result.stderr));
            continue;
        }

        // Delete the NEF file
        if let Err(e) = fs::remove_file(&nef_path) {
            eprintln!("Failed to delete NEF file: {}", e);
        }


        // Wait for next interval
        if running.load(Ordering::SeqCst) {
            let start = Instant::now();
            for remaining in (0..interval_secs).rev() {
                progress.set_message(format!(
                    "Photo #{} captured and converted to AVIF. Next photo in {}s", 
                    photo_count, 
                    remaining
                ));
                
                if !running.load(Ordering::SeqCst) {
                    break;
                }
                
                thread::sleep(Duration::from_secs(1));
            }
        }
    }

    progress.finish_with_message(format!("Finished! Captured {} photos", photo_count));

    Ok(())
}
