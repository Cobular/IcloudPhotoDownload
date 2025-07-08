use anyhow::{anyhow, Context, Result};
use clap::Parser;
use futures::future::join_all;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use regex::Regex;
use reqwest::Client;
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;

// Custom deserialization functions for string-to-number conversion
mod deserialize_helpers {
    use super::*;

    pub fn deserialize_optional_u32_from_string<'de, D>(
        deserializer: D,
    ) -> Result<Option<u32>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let opt: Option<String> = Option::deserialize(deserializer)?;
        match opt {
            Some(s) => s.parse::<u32>()
                .map(Some)
                .map_err(|e| serde::de::Error::custom(format!("Failed to parse '{}' as u32: {}", s, e))),
            None => Ok(None),
        }
    }
}

#[derive(Parser)]
#[command(name = "icloud-photo-download")]
#[command(about = "Download all photos from an Apple Photos web album")]
struct Args {
    /// Apple Photos web album URL (e.g., https://www.icloud.com/sharedalbum/#B2T5oqs3q2VPkhS)
    #[arg(short, long)]
    url: String,

    /// Output directory for downloaded photos
    #[arg(short, long, default_value = "./photos")]
    output: String,

    /// Maximum concurrent downloads
    #[arg(short, long, default_value = "5")]
    concurrent: usize,
}

#[derive(Deserialize, Debug)]
struct WebstreamResponse {
    #[serde(rename = "streamCtag")]
    stream_ctag: Option<String>,
    #[serde(rename = "streamName")]
    stream_name: Option<String>,
    photos: Vec<Photo>,
    #[serde(flatten)]
    extra: HashMap<String, serde_json::Value>,
}

#[derive(Deserialize, Debug)]
struct Photo {
    #[serde(rename = "photoGuid")]
    photo_guid: String,
    #[serde(rename = "batchGuid")]
    batch_guid: Option<String>,
    derivatives: HashMap<String, Derivative>,
    #[serde(rename = "dateCreated")]
    date_created: Option<String>,
    caption: Option<String>,
    #[serde(deserialize_with = "deserialize_helpers::deserialize_optional_u32_from_string")]
    width: Option<u32>,
    #[serde(deserialize_with = "deserialize_helpers::deserialize_optional_u32_from_string")]
    height: Option<u32>,
    #[serde(flatten)]
    extra: HashMap<String, serde_json::Value>,
}

#[derive(Deserialize, Debug)]
struct Derivative {
    #[serde(rename = "fileSize")]
    file_size: Option<String>,
    checksum: String,
    #[serde(deserialize_with = "deserialize_helpers::deserialize_optional_u32_from_string")]
    width: Option<u32>,
    #[serde(deserialize_with = "deserialize_helpers::deserialize_optional_u32_from_string")]
    height: Option<u32>,
    #[serde(flatten)]
    extra: HashMap<String, serde_json::Value>,
}

#[derive(Serialize)]
struct WebstreamRequest {
    #[serde(rename = "streamCtag")]
    stream_ctag: Option<String>,
}

#[derive(Serialize)]
struct AssetUrlsRequest {
    #[serde(rename = "photoGuids")]
    photo_guids: Vec<String>,
}

#[derive(Deserialize, Debug)]
struct AssetUrlsResponse {
    locations: HashMap<String, Location>,
    items: HashMap<String, AssetUrl>,
    #[serde(flatten)]
    extra: HashMap<String, serde_json::Value>,
}

#[derive(Deserialize, Debug)]
struct Location {
    scheme: String,
    hosts: Vec<String>,
    #[serde(flatten)]
    extra: HashMap<String, serde_json::Value>,
}

#[derive(Deserialize, Debug)]
struct AssetUrl {
    #[serde(rename = "url_expiry")]
    url_expiry: Option<String>,
    #[serde(rename = "url_location")]
    url_location: String,
    #[serde(rename = "url_path")]
    url_path: String,
    #[serde(flatten)]
    extra: HashMap<String, serde_json::Value>,
}

struct DownloadInfo {
    photo_guid: String,
    checksum: String,
    download_url: String,
    filename: String,
    size_info: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    println!("🍎 iCloud Photo Album Downloader");
    println!("================================");

    // Extract hash from URL
    let hash = extract_hash_from_url(&args.url)
        .context("Failed to extract hash from URL")?;
    
    println!("📱 Album hash: {}", hash);

    // Create output directory
    fs::create_dir_all(&args.output)
        .context("Failed to create output directory")?;

    let client = Client::new();

    // Step 1: Get webstream data
    println!("\n🔍 Fetching album metadata...");
    let webstream_data = fetch_webstream(&client, &hash).await
        .context("Failed to fetch album metadata")?;

    let album_name = webstream_data.stream_name
        .as_deref()
        .unwrap_or("Unknown Album");
    let photo_count = webstream_data.photos.len();

    println!("📸 Album: '{}'", album_name);
    println!("📊 Found {} photos", photo_count);

    if photo_count == 0 {
        println!("✅ No photos to download");
        return Ok(());
    }

    // Step 2: Get download URLs in batches
    println!("\n🔗 Fetching download URLs...");
    let download_infos = fetch_download_urls(&client, &hash, &webstream_data.photos).await
        .context("Failed to fetch download URLs")?;

    println!("🎯 Prepared {} downloads", download_infos.len());

    // Step 3: Download photos
    println!("\n⬇️  Downloading photos...");
    download_photos(&client, download_infos, &args.output, args.concurrent).await
        .context("Failed to download photos")?;

    println!("\n✅ Download complete! Photos saved to: {}", args.output);
    Ok(())
}

fn extract_hash_from_url(url: &str) -> Result<String> {
    let re = Regex::new(r"icloud\.com/sharedalbum/#([A-Za-z0-9]+)")
        .context("Failed to compile regex")?;
    
    let captures = re.captures(url)
        .ok_or_else(|| anyhow!("Invalid iCloud shared album URL format"))?;
    
    let hash = captures.get(1)
        .ok_or_else(|| anyhow!("No hash found in URL"))?
        .as_str()
        .to_string();
    
    Ok(hash)
}

async fn fetch_webstream(client: &Client, hash: &str) -> Result<WebstreamResponse> {
    let url = format!("https://p153-sharedstreams.icloud.com/{}/sharedstreams/webstream", hash);
    
    let request_body = WebstreamRequest {
        stream_ctag: None,
    };

    let response = client
        .post(&url)
        .header("Accept", "*/*")
        .header("Accept-Language", "en-US,en;q=0.9")
        .header("Content-Type", "text/plain")
        .header("Origin", "https://www.icloud.com")
        .header("Referer", "https://www.icloud.com/")
        .json(&request_body)
        .send()
        .await
        .context("Failed to send webstream request")?;

    if !response.status().is_success() {
        return Err(anyhow!("Webstream request failed with status: {}", response.status()));
    }

    let webstream_data: WebstreamResponse = response
        .json()
        .await
        .context("Failed to parse webstream response")?;

    Ok(webstream_data)
}

async fn fetch_download_urls(
    client: &Client,
    hash: &str,
    photos: &[Photo],
) -> Result<Vec<DownloadInfo>> {
    let url = format!("https://p153-sharedstreams.icloud.com/{}/sharedstreams/webasseturls", hash);
    
    // Collect photo GUIDs in batches of 25
    let mut download_infos = Vec::new();
    let batch_size = 25;
    
    let progress_bar = ProgressBar::new(photos.len() as u64);
    progress_bar.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} batches")?
            .progress_chars("#>-"),
    );

    for batch in photos.chunks(batch_size) {
        let photo_guids: Vec<String> = batch.iter()
            .map(|p| p.photo_guid.clone())
            .collect();

        let request_body = AssetUrlsRequest { photo_guids };

        let response = client
            .post(&url)
            .header("Accept", "*/*")
            .header("Accept-Language", "en-US,en;q=0.9")
            .header("Content-Type", "text/plain")
            .header("Origin", "https://www.icloud.com")
            .header("Referer", "https://www.icloud.com/")
            .json(&request_body)
            .send()
            .await
            .context("Failed to send asset URLs request")?;

        if !response.status().is_success() {
            return Err(anyhow!("Asset URLs request failed with status: {}", response.status()));
        }

        let assets_response: AssetUrlsResponse = response
            .json()
            .await
            .context("Failed to parse asset URLs response")?;

        // Process this batch
        for photo in batch {
            if let Some(download_info) = process_photo_for_download(photo, &assets_response)? {
                download_infos.push(download_info);
            }
        }

        progress_bar.inc(batch.len() as u64);
    }

    progress_bar.finish_with_message("URL fetching complete");
    Ok(download_infos)
}

fn process_photo_for_download(
    photo: &Photo,
    assets_response: &AssetUrlsResponse,
) -> Result<Option<DownloadInfo>> {
    // Find the highest resolution derivative
    let best_derivative = photo.derivatives
        .iter()
        .max_by_key(|(size, _)| size.parse::<u32>().unwrap_or(0));

    let (_size_key, derivative) = match best_derivative {
        Some((key, deriv)) => (key, deriv),
        None => return Ok(None), // No derivatives found
    };

    // Get the download URL for this checksum
    let asset_url = match assets_response.items.get(&derivative.checksum) {
        Some(url) => url,
        None => return Ok(None), // No URL found for this checksum
    };

    // Construct the full download URL
    let location = assets_response.locations
        .get(&asset_url.url_location)
        .ok_or_else(|| anyhow!("Location not found for: {}", asset_url.url_location))?;

    let download_url = format!("{}://{}{}", 
        location.scheme,
        location.hosts.first()
            .ok_or_else(|| anyhow!("No hosts found for location"))?,
        asset_url.url_path
    );

    // Extract filename from URL path
    let filename = Path::new(&asset_url.url_path)
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| {
            // Remove query parameters
            name.split('?').next().unwrap_or(name).to_string()
        })
        .unwrap_or_else(|| format!("{}.jpg", photo.photo_guid));

    let size_info = format!("{}x{}", 
        derivative.width.map_or("?".to_string(), |w| w.to_string()),
        derivative.height.map_or("?".to_string(), |h| h.to_string())
    );

    Ok(Some(DownloadInfo {
        photo_guid: photo.photo_guid.clone(),
        checksum: derivative.checksum.clone(),
        download_url,
        filename,
        size_info,
    }))
}

async fn download_photos(
    client: &Client,
    download_infos: Vec<DownloadInfo>,
    output_dir: &str,
    max_concurrent: usize,
) -> Result<()> {
    let multi_progress = MultiProgress::new();
    let main_progress = multi_progress.add(ProgressBar::new(download_infos.len() as u64));
    main_progress.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} photos downloaded")?
            .progress_chars("#>-"),
    );

    // Use semaphore to limit concurrent downloads
    let semaphore = tokio::sync::Semaphore::new(max_concurrent);
    
    let download_tasks: Vec<_> = download_infos
        .into_iter()
        .map(|info| {
            let client = client.clone();
            let output_dir = output_dir.to_string();
            let semaphore = &semaphore;
            let main_progress = main_progress.clone();

            async move {
                let _permit = semaphore.acquire().await.unwrap();
                
                let result = download_single_photo(&client, &info, &output_dir).await;
                main_progress.inc(1);
                
                match result {
                    Ok(_) => Ok(info.filename),
                    Err(e) => {
                        eprintln!("❌ Failed to download {}: {}", info.filename, e);
                        Err(e)
                    }
                }
            }
        })
        .collect();

    let results = join_all(download_tasks).await;
    main_progress.finish_with_message("All downloads complete");

    // Count successes and failures
    let mut success_count = 0;
    let mut failure_count = 0;

    for result in results {
        match result {
            Ok(_) => success_count += 1,
            Err(_) => failure_count += 1,
        }
    }

    println!("📊 Results: {} succeeded, {} failed", success_count, failure_count);

    if failure_count > 0 {
        return Err(anyhow!("{} downloads failed", failure_count));
    }

    Ok(())
}

async fn download_single_photo(
    client: &Client,
    info: &DownloadInfo,
    output_dir: &str,
) -> Result<()> {
    let response = client
        .get(&info.download_url)
        .header("Accept", "image/avif,image/webp,image/apng,image/svg+xml,image/*,*/*;q=0.8")
        .header("Accept-Language", "en-US,en;q=0.9")
        .header("Referer", "https://www.icloud.com/")
        .header("Sec-Fetch-Dest", "image")
        .send()
        .await
        .context("Failed to start download")?;

    if !response.status().is_success() {
        return Err(anyhow!("Download failed with status: {}", response.status()));
    }

    let content = response
        .bytes()
        .await
        .context("Failed to read response bytes")?;

    let file_path = Path::new(output_dir).join(&info.filename);
    let mut file = File::create(&file_path)
        .await
        .context("Failed to create output file")?;

    file.write_all(&content)
        .await
        .context("Failed to write file")?;

    file.sync_all()
        .await
        .context("Failed to sync file")?;

    Ok(())
}
