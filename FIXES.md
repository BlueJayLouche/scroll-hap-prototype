# Fixes Applied

## Drag & Drop Issues Fixed

### 1. Browser opens file instead of handling drop
**Problem:** The browser's default behavior wasn't being prevented.

**Solution:** Added global event listeners on `document.body` to prevent default for all drag events:

```javascript
['dragenter', 'dragover', 'dragleave', 'drop'].forEach(eventName => {
    document.body.addEventListener(eventName, (e) => {
        e.preventDefault();
        e.stopPropagation();
    }, false);
});
```

### 2. Click to load does nothing
**Problem:** File input element wasn't being added to DOM before clicking.

**Solution:** Append input to body, click, then remove:

```javascript
const input = document.createElement('input');
input.type = 'file';
input.style.display = 'none';
document.body.appendChild(input);
input.click();
setTimeout(() => document.body.removeChild(input), 100);
```

### 3. HAP WASM parsing issues
**Problems:**
- Codec type was hardcoded to "Hap1"
- FPS was hardcoded to 30.0
- Missing stts (time-to-sample) parsing

**Solutions:**
- Added proper stsd parsing to extract actual codec type
- Added mdhd parsing to get timescale and duration
- Added stts parsing to calculate actual FPS

## To Build and Test

```bash
# 1. Build WASM module (one-time)
cd hap-wasm-approach
./build.sh

# 2. Serve the prototype
cd ..
./serve.sh 8080

# 3. Open browser
# http://localhost:8080
```

## Browser Console Debugging

Open browser console (F12) to see:
- "HAP WASM module initialized" - WASM loaded
- "Buffer size: X bytes" - File received
- "Parsing QuickTime container..." - Parsing started
- "Found codec: HapX" - Codec detected
- "Parsed: WxH @ Xfps, N frames" - Parse successful

## Common Issues

### "Failed to load WASM module"
- Make sure you ran `./build.sh` in `hap-wasm-approach/`
- Check that `pkg/hap_wasm.js` and `pkg/hap_wasm_bg.wasm` exist

### "No HAP track found"
- Make sure you're dropping a valid HAP .mov file
- Check console for parse error details

### Video doesn't display
- Check if WebGL 2 is supported (look for GPU/CPU badge)
- Check console for decode errors

## Testing

### WebP Approach
1. Convert a video: `cd tools/video-to-webp && cargo run -- input.mp4 ./frames`
2. Open webp-approach demo
3. Drop the `frames` folder

### HAP Approach
1. Create HAP video: `cd hap-rs && cargo run --example encode_video -- output.mov 512 512 60`
2. Build WASM: `cd scroll-hap-prototype/hap-wasm-approach && ./build.sh`
3. Serve: `./serve.sh 8080`
4. Drop the .mov file
