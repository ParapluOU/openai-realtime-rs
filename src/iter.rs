use crate::types::ResponseAudioDeltaEvent;
use crate::utils::RealtimeUtils;
use crate::RealtimeClient;
use log::{info, warn};
use rodio::Source;
use std::collections::VecDeque;
use std::sync::atomic::AtomicUsize;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc::Receiver;

/// iterator that goes over the Conversation items,
/// and returns all audio data as a continuous stream.
/// it tracks the item number currently under consideration,
/// and the content part number within the current item.
/// as long as an Item is not complete (!item.is_complete())
/// the iterator will wait for new content parts to be added
/// to the current item. If the item is complete, it goes to the next
/// item if there is one. If the last audio byte of the last item was served,
/// the iterator wil have to wait for either the local
/// or the remote participant to further feed the items
pub struct RealtimeAudioIter {
    /// offset of item in Conversation
    item_offset: AtomicUsize,
    /// offset of content part in current item
    content_part_offset: AtomicUsize,
    /// client that needs to be unlocked as short as possible to not
    /// interfere with the event loop
    client: RealtimeClient,
    /// buffer
    buffer: Arc<Mutex<VecDeque<i16>>>,
    /// refiller task handle
    refiller_handle: tokio::task::JoinHandle<()>,
}

impl RealtimeAudioIter {
    pub fn new(client: RealtimeClient, mut receiver: Receiver<Vec<i16>>) -> Self {
        let buffer = Arc::new(Mutex::<VecDeque<i16>>::new(Default::default()));

        let refiller_handle = tokio::spawn({
            let inner_buffer = buffer.clone();
            let inner_client = client.clone();

            async move {
                info!("starting RealtimeAudioIter buffer refiller task");

                loop {
                    let evt = inner_client
                        .wait_for_next::<ResponseAudioDeltaEvent>("response.audio.delta", None)
                        .await
                        .expect("error trying to wait for next ResponseAudioDeltaEvent");

                    let samples = RealtimeUtils::base64_to_16_bit_pcm(&evt.delta);

                    let mut buffer_lock = inner_buffer.lock().unwrap();

                    let before_len = buffer_lock.len();

                    buffer_lock.extend(samples.iter());

                    info!("new audio delta intercepted for audio playback. extended RealtimeAudioIter buffer by {} from {} -> {}", samples.len(), before_len, buffer_lock.len());
                }
            }
        });

        Self {
            item_offset: AtomicUsize::new(0),
            content_part_offset: AtomicUsize::new(0),
            client,
            buffer,
            refiller_handle,
        }
    }
}
impl Iterator for RealtimeAudioIter {
    type Item = i16;

    /// probably needs to use some sort of channel as the methods
    /// for retrieving and checking items are all async
    fn next(&mut self) -> Option<Self::Item> {
        let mut buffer_lock = self.buffer.try_lock();

        while buffer_lock.is_err() {
            info!("waiting to acquire lock on RealtimeAudioIter buffer");
            buffer_lock = self.buffer.try_lock()
        }

        let mut buffer_lock = buffer_lock.unwrap();

        let buffer_size = buffer_lock.len();

        // info!("requesting next sample from iterator ({})...", buffer_size);

        let sample = buffer_lock.pop_front();

        if sample.is_none() {
            assert_eq!(0, buffer_size);
        }

        match sample {
            None => Some(0),
            Some(v) => {
                // info!("sample value: {}", v);
                Some(v)
            }
        }
    }
}

impl Source for RealtimeAudioIter {
    fn current_frame_len(&self) -> Option<usize> {
        None // Continuous stream
    }

    fn channels(&self) -> u16 {
        1 // Mono sound
    }

    fn sample_rate(&self) -> u32 {
        24000
    }

    fn total_duration(&self) -> Option<Duration> {
        None // Continuous, no total duration
    }
}
