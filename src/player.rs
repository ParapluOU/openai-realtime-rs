use crate::RealtimeClient;
use log::{debug, info};
use rodio::buffer::SamplesBuffer;
use rodio::{OutputStream, OutputStreamHandle, PlayError, Sink, Source};
use std::thread;

pub struct Player {
    client: RealtimeClient,
    output_stream: Option<OutputStream>,
    output_stream_handle: Option<OutputStreamHandle>,
    sink: Option<Sink>,
}

impl Player {
    pub fn new(client: RealtimeClient) -> Self {
        Self {
            client,
            output_stream: None,
            output_stream_handle: None,
            sink: None,
        }
    }

    pub fn play(client: RealtimeClient) -> Self {
        let mut player = Self::new(client);
        player.start();
        player
    }

    pub fn start(&mut self) -> &mut Self {
        info!("starting audio stream player...");

        let inner_client = self.client.clone();

        // Create an RealtimeAudioIter and buffer
        let (async_source, buffer_filler) = inner_client.audio_stream();

        // Set up Rodio output stream
        let (output_stream, stream_handle) = OutputStream::try_default().unwrap();

        stream_handle
            .play_raw(async_source.buffered().convert_samples())
            .unwrap();

        // self.sink = sink.into();
        self.output_stream = output_stream.into();
        self.output_stream_handle = stream_handle.into();

        self
    }
}
