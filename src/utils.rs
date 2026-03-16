use anyhow::anyhow;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use ndarray::Array1;
use std::convert::TryInto;
use std::path::Path;
use std::time::Duration;

pub struct RealtimeUtils;

impl RealtimeUtils {
    /// Converts Vec<f32> of amplitude data to Vec<i16> (16-bit PCM format)
    pub fn float_to_16_bit_pcm(float_array: &[f32]) -> Vec<i16> {
        float_array
            .iter()
            .map(|&s| {
                let s = s.clamp(-1.0, 1.0);
                if s < 0.0 {
                    (s * 0x8000_f32 as f32) as i16
                } else {
                    (s * 0x7fff_f32 as f32) as i16
                }
            })
            .collect()
    }

    /// Converts a base64 string to Vec<u8>
    pub fn base64_to_array_buffer(base64_str: &str) -> Vec<u8> {
        STANDARD
            .decode(base64_str)
            .expect("Failed to decode base64")
    }

    /// Converts a &[i16] or &[f32] to a base64 string
    /// original Javascript:
    ///
    ///     ```js
    ///     static arrayBufferToBase64(arrayBuffer) {
    ///         let binary = '';
    ///         let bytes = new Uint8Array(arrayBuffer);
    ///         const chunkSize = 0x8000; // 32KB chunk size
    ///         for (let i = 0; i < bytes.length; i += chunkSize) {
    ///             let chunk = bytes.subarray(i, i + chunkSize);
    ///             binary += String.fromCharCode.apply(null, chunk);
    ///         }
    ///         return btoa(binary);
    ///     }
    ///     ```
    pub fn array_buffer_to_base64(array: &[i16]) -> String {
        let bytes: Vec<u8> = array
            .iter()
            .flat_map(|&sample| sample.to_le_bytes())
            .collect();
        STANDARD.encode(bytes)
    }

    /// Converts a base64 string to a Vec<i16> (16-bit PCM format)
    pub fn base64_to_16_bit_pcm(base64_str: &str) -> Vec<i16> {
        let bytes = Self::base64_to_array_buffer(base64_str);
        let mut pcm_data = Vec::with_capacity(bytes.len() / 2);
        for i in (0..bytes.len()).step_by(2) {
            let sample = (bytes[i] as i16) | ((bytes[i + 1] as i16) << 8);
            pcm_data.push(sample);
        }
        pcm_data
    }

    /// helper that doesn't exist in the original JS utils
    pub fn array_buffer_f32_to_base64(array: &[f32]) -> String {
        Self::array_buffer_to_base64(&Self::float_to_16_bit_pcm(array))
    }

    /// Generates an ID to send with events and messages
    /// todo: replace with uuidv4?
    pub fn generate_id(prefix: &str, length: usize) -> String {
        const CHARS: &[u8] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
        let mut id = String::from(prefix);
        for _ in prefix.len()..length {
            let idx = rand::random::<usize>() % CHARS.len();
            id.push(CHARS[idx] as char);
        }
        id
    }

    pub fn calculate_duration(pcm_data: &[i16], sample_rate: u32, channels: u16) -> f32 {
        let total_samples = pcm_data.len() as u32;
        let samples_per_second = sample_rate * channels as u32;

        1.0 + (total_samples as f32 / samples_per_second as f32)
    }

    pub fn samples_from_file(path: &Path) -> anyhow::Result<Vec<i16>> {
        use rodio::{Decoder, Source};
        use std::io::BufReader;

        // Open the audio file
        let file = std::fs::File::open(path)?;
        let decoder = Decoder::new(BufReader::new(file))?;

        let mut samples = Vec::new();

        // Check if the source is stereo (2 channels)
        if decoder.channels() == 2 {
            // Downmix stereo to mono by averaging left and right channels
            let mut sample_pairs = decoder.convert_samples::<i16>().collect::<Vec<i16>>();
            let mut chunks = sample_pairs.chunks(2);
            for pair in chunks {
                let left = pair[0] as i32;
                let right = pair[1] as i32;
                let mono_sample = ((left + right) / 2) as i16;
                samples.push(mono_sample);
            }
        } else {
            return Ok(decoder.into_iter().collect());
        }

        Ok(samples)
    }

    pub async fn play_samples(samples: &[i16]) -> anyhow::Result<()> {
        use rodio::{source::Source, OutputStream};

        // Initialize the audio output stream
        let (_stream, stream_handle) = OutputStream::try_default()?;

        // Create a source from the PCM data
        let sample_rate = 24000; // or the sample rate of your audio
        let channels = 1; // Stereo audio, change this if mono (1) or different

        let duration_secs = RealtimeUtils::calculate_duration(samples, sample_rate, channels);

        let source = rodio::buffer::SamplesBuffer::new(channels as u16, sample_rate, samples);

        // Play the audio
        stream_handle.play_raw(source.convert_samples())?;

        // Sleep the thread or keep the application alive while audio is playing
        tokio::time::sleep(Duration::from_secs(duration_secs.ceil() as u64)).await;

        Ok(())
    }

    pub async fn play_samples_file(path: &Path) -> anyhow::Result<()> {
        Self::play_samples(&Self::samples_from_file(path)?).await
    }
}
