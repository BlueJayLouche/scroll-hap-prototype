//! HAP Video Decoder for WebAssembly
//! 
//! Provides scroll-scrubbing video playback using HAP codec in the browser.
//! This module exposes Rust HAP decoding to JavaScript via wasm-bindgen.

use wasm_bindgen::prelude::*;
use js_sys::{ArrayBuffer, Uint8Array};
use web_sys::{console, HtmlCanvasElement, WebGl2RenderingContext, WebGlTexture};
use std::io::{Cursor, Read, Seek, SeekFrom};

// Re-export HAP types
pub use hap_parser::{HapFrame, TextureFormat};

// When the `wee_alloc` feature is enabled, use `wee_alloc` as the global allocator.
#[cfg(feature = "wee_alloc")]
#[global_allocator]
static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

// Initialize panic hook for better error messages
#[wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
    console::log_1(&"HAP WASM module initialized".into());
}

/// Web-friendly HAP video reader that works with in-memory data
#[wasm_bindgen]
pub struct WebHapReader {
    data: Cursor<Vec<u8>>,
    track: VideoTrack,
    frame_count: u32,
    fps: f32,
    timescale: u32,
    width: u32,
    height: u32,
    duration: f64,
    codec_type: String,
}

/// Video track information (simplified from hap-qt)
#[derive(Debug, Clone)]
struct VideoTrack {
    width: u32,
    height: u32,
    frame_count: u32,
    timescale: u32,
    duration: u64,
    sample_sizes: Vec<u32>,
    chunk_offsets: Vec<u64>,
    sample_to_chunk: Vec<SampleToChunkEntry>,
    sample_deltas: Vec<u32>,
    codec_type: String,
}

#[derive(Debug, Clone)]
struct SampleToChunkEntry {
    first_chunk: u32,
    samples_per_chunk: u32,
    sample_description_index: u32,
}

#[wasm_bindgen]
impl WebHapReader {
    /// Create a new WebHapReader from an ArrayBuffer (e.g., from file input)
    #[wasm_bindgen(constructor)]
    pub fn new(buffer: &ArrayBuffer) -> Result<WebHapReader, JsValue> {
        wasm_log("Creating WebHapReader...");
        
        // Convert ArrayBuffer to Vec<u8>
        let uint8_array = Uint8Array::new(buffer);
        let len = uint8_array.length() as usize;
        wasm_log(&format!("Buffer size: {} bytes", len));
        
        let mut data = vec![0u8; len];
        uint8_array.copy_to(&mut data);

        let mut cursor = Cursor::new(data);
        
        // Parse the QuickTime container
        wasm_log("Parsing QuickTime container...");
        let track = Self::parse_movie(&mut cursor)
            .map_err(|e| {
                wasm_log(&format!("Parse error: {}", e));
                JsValue::from_str(&format!("Parse error: {}", e))
            })?;

        let frame_count = track.frame_count;
        let width = track.width;
        let height = track.height;
        let timescale = track.timescale;
        let duration = track.duration;
        
        // Calculate FPS from sample deltas
        let fps = if !track.sample_deltas.is_empty() {
            let avg_delta = track.sample_deltas.iter().sum::<u32>() as f32 / track.sample_deltas.len() as f32;
            if avg_delta > 0.0 {
                timescale as f32 / avg_delta
            } else {
                30.0
            }
        } else {
            30.0
        };

        let codec_type = track.codec_type.clone();
        
        wasm_log(&format!("Parsed: {}x{} @ {}fps, {} frames, codec: {}", 
            width, height, fps, frame_count, codec_type));

        Ok(WebHapReader {
            data: cursor,
            track,
            frame_count,
            fps,
            timescale,
            width,
            height,
            duration: duration as f64 / timescale as f64,
            codec_type,
        })
    }

    /// Get video width
    #[wasm_bindgen(getter)]
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Get video height
    #[wasm_bindgen(getter)]
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Get total frame count
    #[wasm_bindgen(getter)]
    pub fn frame_count(&self) -> u32 {
        self.frame_count
    }

    /// Get frame rate
    #[wasm_bindgen(getter)]
    pub fn fps(&self) -> f32 {
        self.fps
    }

    /// Get duration in seconds
    #[wasm_bindgen(getter)]
    pub fn duration(&self) -> f64 {
        self.duration
    }

    /// Get codec type
    #[wasm_bindgen(getter)]
    pub fn codec_type(&self) -> String {
        self.codec_type.clone()
    }

    /// Read and decode a specific frame
    pub fn read_frame(&mut self, frame_index: u32) -> Result<DecodedFrame, JsValue> {
        if frame_index >= self.frame_count {
            return Err(JsValue::from_str("Frame index out of range"));
        }

        // Find chunk location
        let (chunk_index, sample_offset) = self.frame_to_chunk(frame_index);
        let chunk_offset = self.track.chunk_offsets.get(chunk_index as usize)
            .ok_or_else(|| JsValue::from_str("Invalid chunk index"))?;

        // Calculate frame offset within chunk
        let mut frame_offset = *chunk_offset;
        for i in 0..sample_offset {
            let idx = (frame_index - sample_offset + i) as usize;
            if idx < self.track.sample_sizes.len() {
                frame_offset += self.track.sample_sizes[idx] as u64;
            }
        }

        let frame_size = self.track.sample_sizes.get(frame_index as usize)
            .ok_or_else(|| JsValue::from_str("Invalid frame index"))?;

        // Read frame data
        self.data.seek(SeekFrom::Start(frame_offset))
            .map_err(|e| JsValue::from_str(&format!("Seek error: {}", e)))?;
        
        let mut frame_data = vec![0u8; *frame_size as usize];
        self.data.read_exact(&mut frame_data)
            .map_err(|e| JsValue::from_str(&format!("Read error: {}", e)))?;

        // Parse HAP frame
        let hap_frame = hap_parser::parse_frame(&frame_data)
            .map_err(|e| JsValue::from_str(&format!("HAP parse error: {}", e)))?;

        Ok(DecodedFrame {
            data: hap_frame.texture_data,
            format: hap_frame.texture_format,
            width: self.width,
            height: self.height,
        })
    }
}

impl WebHapReader {
    /// Parse QuickTime movie structure
    fn parse_movie(cursor: &mut Cursor<Vec<u8>>) -> Result<VideoTrack, String> {
        use byteorder::{BigEndian, ReadBytesExt};

        let file_size = cursor.get_ref().len() as u64;
        let mut pos = 0u64;
        let mut moov_data = None;
        let mut mdat_offset = None;

        // Read top-level atoms
        while pos < file_size {
            cursor.seek(SeekFrom::Start(pos)).map_err(|e| e.to_string())?;

            let size = cursor.read_u32::<BigEndian>().map_err(|e| e.to_string())? as u64;
            let mut type_buf = [0u8; 4];
            cursor.read_exact(&mut type_buf).map_err(|e| e.to_string())?;
            let atom_type = String::from_utf8_lossy(&type_buf);

            if size == 0 || size > file_size - pos {
                break;
            }

            match atom_type.as_ref() {
                "moov" => {
                    let mut data = vec![0u8; (size - 8) as usize];
                    cursor.read_exact(&mut data).map_err(|e| e.to_string())?;
                    moov_data = Some(data);
                }
                "mdat" => {
                    mdat_offset = Some(pos);
                }
                _ => {}
            }

            pos += size;
        }

        let moov_data = moov_data.ok_or("No moov atom found")?;
        let mdat_offset = mdat_offset.ok_or("No mdat atom found")?;

        // Parse moov atom
        Self::parse_moov(&moov_data, mdat_offset)
    }

    fn parse_moov(data: &[u8], mdat_offset: u64) -> Result<VideoTrack, String> {
        use byteorder::{BigEndian, ReadBytesExt};
        
        let mut cursor = Cursor::new(data);
        
        while cursor.position() < data.len() as u64 {
            let pos = cursor.position();
            let remaining = data.len() - pos as usize;
            
            if remaining < 8 {
                break;
            }

            let size = cursor.read_u32::<BigEndian>().map_err(|e| e.to_string())? as usize;
            let mut type_buf = [0u8; 4];
            cursor.read_exact(&mut type_buf).map_err(|e| e.to_string())?;
            let atom_type = String::from_utf8_lossy(&type_buf);

            if size == 0 || size > remaining {
                break;
            }

            let atom_data = &data[(pos + 8) as usize..(pos + size as u64) as usize];

            if atom_type == "trak" {
                if let Ok(Some(track)) = Self::parse_trak(atom_data, mdat_offset) {
                    return Ok(track);
                }
            }

            cursor.set_position(pos + size as u64);
        }

        Err("No HAP track found".to_string())
    }

    fn parse_trak(data: &[u8], mdat_offset: u64) -> Result<Option<VideoTrack>, String> {
        use byteorder::{BigEndian, ReadBytesExt};

        let mut width = 0u32;
        let mut height = 0u32;
        let mut sample_sizes = Vec::new();
        let mut chunk_offsets = Vec::new();
        let mut sample_to_chunk = Vec::new();
        let mut sample_deltas = Vec::new();
        let mut timescale = 0u32;
        let mut duration = 0u64;
        let mut codec_type = String::new();

        let mut cursor = Cursor::new(data);

        while cursor.position() < data.len() as u64 {
            let pos = cursor.position();
            let remaining = data.len() - pos as usize;

            if remaining < 8 {
                break;
            }

            let size = cursor.read_u32::<BigEndian>().map_err(|e| e.to_string())? as usize;
            let mut type_buf = [0u8; 4];
            cursor.read_exact(&mut type_buf).map_err(|e| e.to_string())?;
            let atom_type = String::from_utf8_lossy(&type_buf);

            if size == 0 || size > remaining {
                break;
            }

            let atom_data = &data[(pos + 8) as usize..(pos + size as u64) as usize];

            match atom_type.as_ref() {
                "tkhd" => {
                    // Parse track header for dimensions
                    // tkhd format: version(1) + flags(3) + ... + width(4) + height(4)
                    if atom_data.len() >= 80 {
                        let mut tkhd = Cursor::new(atom_data);
                        let version = atom_data[0];
                        if version == 0 {
                            // 32-bit version
                            tkhd.set_position(76); // Width/height offset
                        } else {
                            // 64-bit version
                            tkhd.set_position(84); // Width/height offset
                        }
                        if tkhd.position() + 8 <= atom_data.len() as u64 {
                            width = tkhd.read_u32::<BigEndian>().map_err(|e| e.to_string())? >> 16;
                            height = tkhd.read_u32::<BigEndian>().map_err(|e| e.to_string())? >> 16;
                        }
                    }
                }
                "mdia" => {
                    // Parse media atom
                    let result = Self::parse_mdia(atom_data, mdat_offset)?;
                    sample_sizes = result.sample_sizes;
                    chunk_offsets = result.chunk_offsets;
                    sample_to_chunk = result.sample_to_chunk;
                    sample_deltas = result.sample_deltas;
                    timescale = result.timescale;
                    duration = result.duration;
                    codec_type = result.codec_type;
                }
                _ => {}
            }

            cursor.set_position(pos + size as u64);
        }

        if codec_type.is_empty() || sample_sizes.is_empty() {
            return Ok(None);
        }

        let frame_count = sample_sizes.len() as u32;

        Ok(Some(VideoTrack {
            width,
            height,
            frame_count,
            timescale,
            duration,
            sample_sizes,
            chunk_offsets,
            sample_to_chunk,
            sample_deltas,
            codec_type,
        }))
    }

    fn parse_mdia(data: &[u8], mdat_offset: u64) -> Result<MediaInfo, String> {
        use byteorder::{BigEndian, ReadBytesExt};

        let mut sample_sizes = Vec::new();
        let mut chunk_offsets = Vec::new();
        let mut sample_to_chunk = Vec::new();
        let mut sample_deltas = Vec::new();
        let mut timescale = 0u32;
        let mut duration = 0u64;
        let mut codec_type = String::new();

        let mut cursor = Cursor::new(data);

        while cursor.position() < data.len() as u64 {
            let pos = cursor.position();
            let remaining = data.len() - pos as usize;

            if remaining < 8 {
                break;
            }

            let size = cursor.read_u32::<BigEndian>().map_err(|e| e.to_string())? as usize;
            let mut type_buf = [0u8; 4];
            cursor.read_exact(&mut type_buf).map_err(|e| e.to_string())?;
            let atom_type = String::from_utf8_lossy(&type_buf);

            if size == 0 || size > remaining {
                break;
            }

            let atom_data = &data[(pos + 8) as usize..(pos + size as u64) as usize];

            match atom_type.as_ref() {
                "mdhd" => {
                    // Parse media header for timescale and duration
                    if atom_data.len() >= 24 {
                        let mut mdhd = Cursor::new(atom_data);
                        let version = atom_data[0];
                        if version == 0 {
                            mdhd.set_position(12); // Timescale offset
                            timescale = mdhd.read_u32::<BigEndian>().map_err(|e| e.to_string())?;
                            duration = mdhd.read_u32::<BigEndian>().map_err(|e| e.to_string())? as u64;
                        } else {
                            mdhd.set_position(20); // Timescale offset
                            timescale = mdhd.read_u32::<BigEndian>().map_err(|e| e.to_string())?;
                            duration = mdhd.read_u64::<BigEndian>().map_err(|e| e.to_string())?;
                        }
                    }
                }
                "minf" => {
                    let result = Self::parse_minf(atom_data, mdat_offset)?;
                    sample_sizes = result.sample_sizes;
                    chunk_offsets = result.chunk_offsets;
                    sample_to_chunk = result.sample_to_chunk;
                    sample_deltas = result.sample_deltas;
                    codec_type = result.codec_type;
                }
                _ => {}
            }

            cursor.set_position(pos + size as u64);
        }

        Ok(MediaInfo {
            sample_sizes,
            chunk_offsets,
            sample_to_chunk,
            sample_deltas,
            timescale,
            duration,
            codec_type,
        })
    }

    fn parse_minf(data: &[u8], mdat_offset: u64) -> Result<MediaInfo, String> {
        use byteorder::{BigEndian, ReadBytesExt};

        let mut sample_sizes = Vec::new();
        let mut chunk_offsets = Vec::new();
        let mut sample_to_chunk = Vec::new();
        let mut sample_deltas = Vec::new();
        let mut codec_type = String::new();

        let mut cursor = Cursor::new(data);

        while cursor.position() < data.len() as u64 {
            let pos = cursor.position();
            let remaining = data.len() - pos as usize;

            if remaining < 8 {
                break;
            }

            let size = cursor.read_u32::<BigEndian>().map_err(|e| e.to_string())? as usize;
            let mut type_buf = [0u8; 4];
            cursor.read_exact(&mut type_buf).map_err(|e| e.to_string())?;
            let atom_type = String::from_utf8_lossy(&type_buf);

            if size == 0 || size > remaining {
                break;
            }

            let atom_data = &data[(pos + 8) as usize..(pos + size as u64) as usize];

            if atom_type == "stbl" {
                let result = Self::parse_stbl(atom_data, mdat_offset)?;
                sample_sizes = result.sample_sizes;
                chunk_offsets = result.chunk_offsets;
                sample_to_chunk = result.sample_to_chunk;
                sample_deltas = result.sample_deltas;
                codec_type = result.codec_type;
            }

            cursor.set_position(pos + size as u64);
        }

        Ok(MediaInfo {
            sample_sizes,
            chunk_offsets,
            sample_to_chunk,
            sample_deltas,
            timescale: 0,
            duration: 0,
            codec_type,
        })
    }

    fn parse_stbl(data: &[u8], mdat_offset: u64) -> Result<MediaInfo, String> {
        use byteorder::{BigEndian, ReadBytesExt};

        let mut sample_sizes = Vec::new();
        let mut chunk_offsets = Vec::new();
        let mut sample_to_chunk = Vec::new();
        let mut sample_deltas = Vec::new();
        let mut codec_type = String::new();

        let mut cursor = Cursor::new(data);

        while cursor.position() < data.len() as u64 {
            let pos = cursor.position();
            let remaining = data.len() - pos as usize;

            if remaining < 8 {
                break;
            }

            let size = cursor.read_u32::<BigEndian>().map_err(|e| e.to_string())? as usize;
            let mut type_buf = [0u8; 4];
            cursor.read_exact(&mut type_buf).map_err(|e| e.to_string())?;
            let atom_type = String::from_utf8_lossy(&type_buf);

            if size == 0 || size > remaining {
                break;
            }

            let atom_data = &data[(pos + 8) as usize..(pos + size as u64) as usize];

            match atom_type.as_ref() {
                "stsd" => {
                    // Parse sample description to get codec type
                    if atom_data.len() >= 20 {
                        let mut stsd = Cursor::new(atom_data);
                        let _version = stsd.read_u8().map_err(|e| e.to_string())?;
                        let _flags = stsd.read_u24::<BigEndian>().map_err(|e| e.to_string())?;
                        let entry_count = stsd.read_u32::<BigEndian>().map_err(|e| e.to_string())?;
                        
                        if entry_count > 0 && atom_data.len() >= 28 {
                            let _entry_size = stsd.read_u32::<BigEndian>().map_err(|e| e.to_string())?;
                            let mut codec_buf = [0u8; 4];
                            stsd.read_exact(&mut codec_buf).map_err(|e| e.to_string())?;
                            codec_type = String::from_utf8_lossy(&codec_buf).to_string();
                            wasm_log(&format!("Found codec: {}", codec_type));
                        }
                    }
                }
                "stsz" => {
                    sample_sizes = Self::parse_stsz(atom_data)?;
                }
                "stco" => {
                    chunk_offsets = Self::parse_stco(atom_data, mdat_offset)?;
                }
                "co64" => {
                    chunk_offsets = Self::parse_co64(atom_data, mdat_offset)?;
                }
                "stsc" => {
                    sample_to_chunk = Self::parse_stsc(atom_data)?;
                }
                "stts" => {
                    sample_deltas = Self::parse_stts(atom_data)?;
                }
                _ => {}
            }

            cursor.set_position(pos + size as u64);
        }

        Ok(MediaInfo {
            sample_sizes,
            chunk_offsets,
            sample_to_chunk,
            sample_deltas,
            timescale: 0,
            duration: 0,
            codec_type,
        })
    }

    fn parse_stsz(data: &[u8]) -> Result<Vec<u32>, String> {
        use byteorder::{BigEndian, ReadBytesExt};
        
        let mut cursor = Cursor::new(data);
        let _version = cursor.read_u8().map_err(|e| e.to_string())?;
        let _flags = cursor.read_u24::<BigEndian>().map_err(|e| e.to_string())?;
        let sample_size = cursor.read_u32::<BigEndian>().map_err(|e| e.to_string())?;
        let sample_count = cursor.read_u32::<BigEndian>().map_err(|e| e.to_string())?;

        let mut sizes = Vec::with_capacity(sample_count as usize);

        if sample_size == 0 {
            for _ in 0..sample_count {
                sizes.push(cursor.read_u32::<BigEndian>().map_err(|e| e.to_string())?);
            }
        } else {
            sizes.resize(sample_count as usize, sample_size);
        }

        Ok(sizes)
    }

    fn parse_stco(data: &[u8], mdat_offset: u64) -> Result<Vec<u64>, String> {
        use byteorder::{BigEndian, ReadBytesExt};
        
        let mut cursor = Cursor::new(data);
        let _version = cursor.read_u8().map_err(|e| e.to_string())?;
        let _flags = cursor.read_u24::<BigEndian>().map_err(|e| e.to_string())?;
        let entry_count = cursor.read_u32::<BigEndian>().map_err(|e| e.to_string())?;

        let mut offsets = Vec::with_capacity(entry_count as usize);

        for _ in 0..entry_count {
            let offset = cursor.read_u32::<BigEndian>().map_err(|e| e.to_string())? as u64;
            // Adjust offset to be relative to file start
            offsets.push(if offset < mdat_offset {
                mdat_offset + 8 + offset
            } else {
                offset
            });
        }

        Ok(offsets)
    }

    fn parse_co64(data: &[u8], mdat_offset: u64) -> Result<Vec<u64>, String> {
        use byteorder::{BigEndian, ReadBytesExt};
        
        let mut cursor = Cursor::new(data);
        let _version = cursor.read_u8().map_err(|e| e.to_string())?;
        let _flags = cursor.read_u24::<BigEndian>().map_err(|e| e.to_string())?;
        let entry_count = cursor.read_u32::<BigEndian>().map_err(|e| e.to_string())?;

        let mut offsets = Vec::with_capacity(entry_count as usize);

        for _ in 0..entry_count {
            let offset = cursor.read_u64::<BigEndian>().map_err(|e| e.to_string())?;
            offsets.push(if offset < mdat_offset {
                mdat_offset + 8 + offset
            } else {
                offset
            });
        }

        Ok(offsets)
    }

    fn parse_stsc(data: &[u8]) -> Result<Vec<SampleToChunkEntry>, String> {
        use byteorder::{BigEndian, ReadBytesExt};
        
        let mut cursor = Cursor::new(data);
        let _version = cursor.read_u8().map_err(|e| e.to_string())?;
        let _flags = cursor.read_u24::<BigEndian>().map_err(|e| e.to_string())?;
        let entry_count = cursor.read_u32::<BigEndian>().map_err(|e| e.to_string())?;

        let mut entries = Vec::with_capacity(entry_count as usize);

        for _ in 0..entry_count {
            entries.push(SampleToChunkEntry {
                first_chunk: cursor.read_u32::<BigEndian>().map_err(|e| e.to_string())?,
                samples_per_chunk: cursor.read_u32::<BigEndian>().map_err(|e| e.to_string())?,
                sample_description_index: cursor.read_u32::<BigEndian>().map_err(|e| e.to_string())?,
            });
        }

        Ok(entries)
    }

    fn parse_stts(data: &[u8]) -> Result<Vec<u32>, String> {
        use byteorder::{BigEndian, ReadBytesExt};
        
        let mut cursor = Cursor::new(data);
        let _version = cursor.read_u8().map_err(|e| e.to_string())?;
        let _flags = cursor.read_u24::<BigEndian>().map_err(|e| e.to_string())?;
        let entry_count = cursor.read_u32::<BigEndian>().map_err(|e| e.to_string())?;

        let mut deltas = Vec::new();

        for _ in 0..entry_count {
            let sample_count = cursor.read_u32::<BigEndian>().map_err(|e| e.to_string())?;
            let sample_delta = cursor.read_u32::<BigEndian>().map_err(|e| e.to_string())?;
            
            for _ in 0..sample_count {
                deltas.push(sample_delta);
            }
        }

        Ok(deltas)
    }

    fn frame_to_chunk(&self, frame_index: u32) -> (u32, u32) {
        let mut sample_count = 0u32;

        for (i, entry) in self.track.sample_to_chunk.iter().enumerate() {
            let next_entry_first_chunk = if i + 1 < self.track.sample_to_chunk.len() {
                self.track.sample_to_chunk[i + 1].first_chunk
            } else {
                self.track.chunk_offsets.len() as u32 + 1
            };

            let chunks_in_entry = next_entry_first_chunk - entry.first_chunk;
            let samples_in_entry = chunks_in_entry * entry.samples_per_chunk;

            if sample_count + samples_in_entry > frame_index {
                let offset_in_entry = frame_index - sample_count;
                let chunk_offset = offset_in_entry / entry.samples_per_chunk;
                let sample_offset = offset_in_entry % entry.samples_per_chunk;

                return (entry.first_chunk - 1 + chunk_offset, sample_offset);
            }

            sample_count += samples_in_entry;
        }

        (0, 0)
    }
}

struct MediaInfo {
    sample_sizes: Vec<u32>,
    chunk_offsets: Vec<u64>,
    sample_to_chunk: Vec<SampleToChunkEntry>,
    sample_deltas: Vec<u32>,
    timescale: u32,
    duration: u64,
    codec_type: String,
}

/// A decoded frame containing raw DXT texture data
#[wasm_bindgen]
pub struct DecodedFrame {
    data: Vec<u8>,
    format: TextureFormat,
    width: u32,
    height: u32,
}

#[wasm_bindgen]
impl DecodedFrame {
    /// Get the raw texture data as Uint8Array
    #[wasm_bindgen(getter)]
    pub fn data(&self) -> Uint8Array {
        unsafe { Uint8Array::view(&self.data) }
    }

    /// Get texture format as string
    #[wasm_bindgen(getter)]
    pub fn format(&self) -> String {
        format!("{:?}", self.format)
    }

    /// Get width
    #[wasm_bindgen(getter)]
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Get height
    #[wasm_bindgen(getter)]
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Get WebGL format constant
    /// COMPRESSED_RGBA_S3TC_DXT5_EXT = 0x83F3
    /// COMPRESSED_RGBA_S3TC_DXT1_EXT = 0x83F1
    #[wasm_bindgen(getter)]
    pub fn webgl_format(&self) -> u32 {
        match self.format {
            TextureFormat::RgbDxt1 => 0x83F1,  // COMPRESSED_RGBA_S3TC_DXT1_EXT
            TextureFormat::RgbaDxt5 => 0x83F3, // COMPRESSED_RGBA_S3TC_DXT5_EXT
            TextureFormat::YcoCgDxt5 => 0x83F3, // Stored as DXT5, needs shader conversion
            TextureFormat::AlphaRgtc1 => 0x8DBB, // COMPRESSED_RED_RGTC1_EXT
            _ => 0x83F3, // Default to DXT5
        }
    }
}

/// Helper to check if WebGL compressed textures are supported
#[wasm_bindgen]
pub fn check_compressed_texture_support(gl: &WebGl2RenderingContext) -> bool {
    let extensions = gl.get_supported_extensions();
    if let Some(exts) = extensions {
        // Convert js_sys::Array to string representation
        let mut found = false;
        for i in 0..exts.length() {
            if let Some(ext) = exts.get(i).as_string() {
                if ext.contains("compressed_texture_s3tc") || ext.contains("WEBGL_compressed_texture_s3tc") {
                    found = true;
                    break;
                }
            }
        }
        found
    } else {
        false
    }
}

/// Log to browser console
#[wasm_bindgen]
pub fn wasm_log(message: &str) {
    console::log_1(&message.into());
}
