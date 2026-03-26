# Scroll-Scrub Video Prototype

A comparison of two approaches for scroll-driven video animation on the web.

## Quick Start

```bash
./serve.sh
# Open http://localhost:8080
```

This starts a local server with the correct WASM MIME types and CORS headers needed for the HAP approach. You can optionally pass a port: `./serve.sh 3000`

### Option 1: WebP Sequence (Simplest)

Open http://localhost:8080/webp-approach/

### Option 2: HAP + WASM (GPU-accelerated)

```bash
cd hap-wasm-approach
./build.sh  # Builds the WASM module
```

Then open http://localhost:8080/hap-wasm-approach/

## Approaches Compared

### 1. WebP Image Sequence

**How it works:**
- Convert video to WebP frames using `video-to-webp` tool
- Preload all frames as `Image` objects
- Scroll → display corresponding frame

**Pros:**
- ✅ Simple, no WASM complexity
- ✅ Works everywhere
- ✅ Frame-perfect accuracy
- ✅ WebP compression is excellent
- ✅ Easy to implement

**Cons:**
- ❌ High memory usage (all frames in RAM)
- ❌ Slow initial load time
- ❌ Not suitable for long videos (>10s)
- ❌ 4K is challenging

**Best for:** Short hero sections (<5 seconds), 1080p or lower

---

### 2. HAP + WASM

**How it works:**
- Use your Rust HAP decoder compiled to WASM
- Decode frames on-demand during scroll
- Upload compressed DXT textures directly to GPU

**Pros:**
- ✅ Extremely memory efficient
- ✅ Instant start (no preload)
- ✅ Handles 4K easily
- ✅ Silky smooth playback (GPU-native)
- ✅ Same quality as desktop HAP

**Cons:**
- ❌ Requires WASM compilation
- ❌ Browser needs WebGL + compressed texture support
- ❌ More complex build process
- ❌ Larger video files (HAP vs h264)

**Best for:** High-resolution content, longer videos, GPU-heavy scenes

---

## File Structure

```
scroll-hap-prototype/
├── webp-approach/
│   └── index.html          # WebP demo page
├── hap-wasm-approach/
│   ├── index.html          # HAP demo page
│   ├── build.sh            # WASM build script
│   └── hap-wasm/           # Rust WASM crate
│       ├── Cargo.toml
│       └── src/lib.rs
├── tools/
│   └── video-to-webp/      # Conversion tool
│       └── ...
└── shared-assets/          # Put test videos here
```

## Converting Video for Testing

### For WebP approach:

```bash
cd tools/video-to-webp
cargo run -- /path/to/video.mp4 ./output-frames --fps 30 --width 1920 --quality 85
```

Then drop the `output-frames` folder into the webp-approach demo.

### For HAP approach:

Use your existing HAP encoder:

```bash
cd ../hap-rs
cargo run --example encode_video -- /path/to/output.mov 1920 1080 300
```

Then drop the `.mov` file into the hap-wasm-approach demo.

## Performance Comparison

| Metric | WebP | HAP |
|--------|------|-----|
| 1080p30, 5s memory | ~150 MB | ~20 MB |
| 1080p30, 5s load time | 5-10s | Instant |
| 4K60 feasibility | No | Yes |
| 60s video feasible | No | Yes |
| Initial frame delay | 0ms | 5-20ms |

## Recommendations

### Use WebP if:
- Video is short (3-5 seconds)
- Resolution is 1080p or lower
- You want simplest implementation
- Content is motion graphics (not footage)

### Use HAP if:
- Video is 4K or higher
- Video is longer than 10 seconds
- You need instant scrubbing
- You want that buttery GPU-smooth feel
- You're already using HAP workflow

## Browser Support

### WebP Approach
- ✅ All modern browsers
- ✅ Safari (iOS 14+)
- ✅ Works with any HTTP server

### HAP Approach
- ✅ Chrome 80+
- ✅ Firefox 79+
- ✅ Safari 15+ (WebGL 2)
- ⚠️ Requires WebGL compressed texture extension
- ⚠️ CORS headers needed for WASM loading

## Next Steps

1. **Test both approaches** with your actual content
2. **Measure performance** on target devices (especially mobile)
3. **Consider hybrid:** Use WebP for simple animations, HAP for complex footage
4. **Optimize HAP:** Add frame caching, worker-based decoding

## Known Issues

### WebP
- Large memory footprint for long videos
- Initial load can be slow on 3G

### HAP
- WASM module size (~500KB uncompressed)
- Safari sometimes has WebGL 2 issues
- Need to handle missing compressed texture extension

## License

MIT - Same as hap-rs
