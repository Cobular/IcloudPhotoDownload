# 🍎 iCloud Photo Album Downloader

A fast, reliable CLI tool to download all photos from Apple Photos web albums.

## Features

- ✨ **Simple CLI interface** - Just provide the album URL
- 🚀 **Concurrent downloads** - Configurable parallel downloads for speed
- 📊 **Progress tracking** - Real-time progress bars and status updates
- 🎯 **High-resolution downloads** - Always downloads the highest quality available

## Installation

1. Make sure you have Rust installed: https://rustup.rs/
2. Clone this repository:
   ```bash
   git clone <repository-url>
   cd IcloudPhotoDownload
   ```
3. Build the project:
   ```bash
   cargo build --release
   ```

## Usage

### Basic Usage

```bash
cargo run -- --url "https://www.icloud.com/sharedalbum/#B2T5oqs3q2VPkhS"
```

### Advanced Usage

```bash
# Specify custom output directory
cargo run -- --url "https://www.icloud.com/sharedalbum/#B2T5oqs3q2VPkhS" --output "./my-photos"

# Control concurrent downloads (default: 5)
cargo run -- --url "https://www.icloud.com/sharedalbum/#B2T5oqs3q2VPkhS" --concurrent 10

# Full example
cargo run -- \
  --url "https://www.icloud.com/sharedalbum/#B2T5oqs3q2VPkhS" \
  --output "./vacation-photos" \
  --concurrent 8
```

### Command Line Options

- `--url` / `-u`: Apple Photos web album URL (required)
- `--output` / `-o`: Output directory for downloaded photos (default: `./photos`)
- `--concurrent` / `-c`: Maximum concurrent downloads (default: `5`)

## How It Works

The tool follows the official Apple Photos sharing protocol:

1. **Extract Album Hash**: Parses the album hash from the provided URL
2. **Fetch Metadata**: Retrieves album information and photo metadata via the webstream endpoint
3. **Get Download URLs**: Requests download URLs in batches of 25 photos via the webasseturls endpoint
4. **Download Photos**: Downloads all photos concurrently with progress tracking

## Example Output

```
🍎 iCloud Photo Album Downloader
================================
📱 Album hash: B2T5oqs3q2VPkhS

🔍 Fetching album metadata...
📸 Album: 'da hike'
📊 Found 150 photos

🔗 Fetching download URLs...
⠁ [00:00:03] [████████████████████████████████████████] 150/150 batches
🎯 Prepared 150 downloads

⬇️  Downloading photos...
⠁ [00:02:15] [████████████████████████████████████████] 150/150 photos downloaded
📊 Results: 150 succeeded, 0 failed

✅ Download complete! Photos saved to: ./photos
```

## License

This project is provided as-is for educational and personal use. Please respect Apple's terms of service when using this tool.

## Troubleshooting

### "Invalid iCloud shared album URL format"
Make sure your URL follows this format:
```
https://www.icloud.com/sharedalbum/#<HASH>
```

### "Webstream request failed"
- Check your internet connection
- Verify the album is still accessible
- Try again in a few minutes (temporary server issues)

### Downloads fail consistently
- Check available disk space
- Verify write permissions in the output directory
- Try reducing concurrent downloads with `--concurrent 1` 
