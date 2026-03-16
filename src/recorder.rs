use crate::RealtimeClient;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::SampleFormat::I16;
use cpal::{FromSample, Sample, SampleFormat, SizedSample};
use log::{debug, error, info};
use rubato::FftFixedInOut;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use voice_activity_detector::VoiceActivityDetector;

pub struct Recorder {
    client: RealtimeClient,
    stream: Option<cpal::Stream>,
    buffer: Arc<Mutex<VecDeque<i16>>>,
    downsample_buffer: Arc<Mutex<Vec<i16>>>, // New field for downsampling
    vad_threshold: f32,                      // New field for VAD threshold
}

impl Recorder {
    const BUFFER_SIZE: usize = 1024 * 30;

    pub fn new(client: RealtimeClient, vad_threshold: f32) -> Self {
        Self {
            client,
            stream: None,
            buffer: Arc::new(Mutex::new(Default::default())),
            downsample_buffer: Arc::new(Mutex::new(Vec::with_capacity(2))), // Buffer for two samples
            vad_threshold,
        }
    }

    pub fn capture(client: RealtimeClient, vad_threshold: Option<f32>) -> anyhow::Result<Self> {
        let mut recorder = Self::new(client, vad_threshold.unwrap_or(0.5));

        recorder.start()?;

        Ok(recorder)
    }

    pub fn start(&mut self) -> anyhow::Result<()> {
        info!("starting microphone stream...");

        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .expect("No input device available");

        let config = device.default_input_config()?;

        debug!("recorder device config: {:#?}", &config);

        let (tx, mut rx) = tokio::sync::mpsc::channel(512);

        // Capture input audio and process the samples.
        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => Self::build_input_stream::<f32>(
                &device,
                &config.into(),
                self.buffer.clone(),
                self.downsample_buffer.clone(),
                tx,
                self.vad_threshold, // Pass the VAD threshold
            )?,
            cpal::SampleFormat::I16 => Self::build_input_stream::<i16>(
                &device,
                &config.into(),
                self.buffer.clone(),
                self.downsample_buffer.clone(),
                tx,
                self.vad_threshold, // Pass the VAD threshold
            )?,
            cpal::SampleFormat::U16 => Self::build_input_stream::<u16>(
                &device,
                &config.into(),
                self.buffer.clone(),
                self.downsample_buffer.clone(),
                tx,
                self.vad_threshold, // Pass the VAD threshold
            )?,
            _ => {
                unimplemented!()
            }
        };

        // Start the audio stream.
        stream.play()?;

        let inner_client = self.client.clone();

        // listen for produced chunks and pass to client
        tokio::spawn(async move {
            info!("starting Recorder task to append input audio");

            while let Some(chunk) = rx.recv().await {
                info!("received audio chunk to append from Recorder");

                if let Err(e) = inner_client.append_input_audio(chunk).await {
                    error!("Error appending audio: {:?}", e);
                }
            }
        });

        self.stream = stream.into();

        Ok(())
    }

    fn build_input_stream<T>(
        device: &cpal::Device,
        config: &cpal::StreamConfig,
        sample_buffer: Arc<Mutex<VecDeque<i16>>>,
        downsample_buffer: Arc<Mutex<Vec<i16>>>,
        sender: tokio::sync::mpsc::Sender<Vec<i16>>,
        vad_threshold: f32, // New parameter
    ) -> anyhow::Result<cpal::Stream>
    where
        T: cpal::Sample + SizedSample,
        i16: FromSample<T>,
    {
        let channels = config.channels as usize;

        // https://crates.io/crates/voice_activity_detector
        let mut vad = VoiceActivityDetector::builder()
            .sample_rate(24000i64)
            .chunk_size(Self::BUFFER_SIZE)
            .build()
            .unwrap();

        debug!("initialized VAD");

        let stream = device.build_input_stream(
            config,
            move |data: &[T], _: &cpal::InputCallbackInfo| {
                let mut buffer = sample_buffer.lock().unwrap();
                let mut downsample_buf = downsample_buffer.lock().unwrap();

                for frame in data.chunks(channels) {
                    let sample_i16 = frame[0].to_sample::<i16>();
                    downsample_buf.push(sample_i16);

                    if downsample_buf.len() == 2 {
                        // Average two samples for downsampling
                        let downsampled = (downsample_buf[0] as i32 + downsample_buf[1] as i32) / 2;
                        buffer.push_back(downsampled as i16);
                        downsample_buf.clear();
                    }
                }

                let cur_buffer_len = buffer.len();

                if cur_buffer_len >= Self::BUFFER_SIZE {
                    let chunk: Vec<i16> = buffer.drain(..Self::BUFFER_SIZE).collect();

                    // Use the configurable VAD threshold
                    if vad.predict(chunk.clone()) > vad_threshold {
                        debug!(
                            "Detected speech. Sending Recorder sample to client (buffer size: {})",
                            cur_buffer_len
                        );
                        sender.try_send(chunk).unwrap();
                    }
                }
            },
            move |err| {
                eprintln!("Error occurred: {:?}", err);
            },
            None,
        )?;

        Ok(stream)
    }
}
