//! Minimal webm stub for builds where the native libwebm submodule is unavailable.
//! This disables actual WebM recording, but keeps the scrap crate compiling.

pub mod mux {
    use std::io::{Seek, Write};

    pub struct Writer<T>(T)
    where
        T: Write + Seek;

    impl<T: Write + Seek> Writer<T> {
        pub fn new(dest: T) -> Writer<T> {
            Writer(dest)
        }
    }

    #[doc(hidden)]
    pub trait MkvWriter {
        fn mkv_writer(&self) -> usize;
    }

    impl<T: Write + Seek> MkvWriter for Writer<T> {
        fn mkv_writer(&self) -> usize {
            0
        }
    }

    pub trait Track {
        fn is_audio(&self) -> bool {
            false
        }
        fn is_video(&self) -> bool {
            false
        }
        fn add_frame(&mut self, _data: &[u8], _timestamp_ns: u64, _keyframe: bool) -> bool {
            true
        }
    }

    #[derive(Eq, PartialEq, Clone, Copy)]
    pub struct VideoTrack(u64);

    #[derive(Eq, PartialEq, Clone, Copy)]
    pub struct AudioTrack;

    impl Track for VideoTrack {
        fn is_video(&self) -> bool {
            true
        }
    }

    impl Track for AudioTrack {
        fn is_audio(&self) -> bool {
            true
        }
    }

    impl VideoTrack {
        pub fn track_number(&self) -> u64 {
            self.0
        }
    }

    #[derive(Copy, Clone)]
    pub enum VideoCodecId {
        VP8,
        VP9,
        AV1,
    }

    #[derive(Copy, Clone)]
    pub enum AudioCodecId {
        Opus,
        Vorbis,
    }

    pub struct Segment<W> {
        _writer: W,
    }

    impl<W: MkvWriter> Segment<W> {
        pub fn new(dest: W) -> Option<Self> {
            let _ = dest.mkv_writer();
            Some(Segment { _writer: dest })
        }

        pub fn add_video_track(
            &mut self,
            _width: u32,
            _height: u32,
            id: Option<i32>,
            _codec: VideoCodecId,
        ) -> VideoTrack {
            VideoTrack(id.unwrap_or(1) as u64)
        }

        pub fn add_audio_track(
            &mut self,
            _sample_rate: i32,
            _channels: i32,
            _id: Option<i32>,
            _codec: AudioCodecId,
        ) -> AudioTrack {
            AudioTrack
        }

        pub fn set_codec_private(&mut self, _track_number: u64, _data: &[u8]) -> bool {
            true
        }

        pub fn set_app_name(&mut self, _name: &str) {}

        pub fn finalize(self, _duration: Option<u64>) -> bool {
            true
        }

        pub fn try_finalize(self, _duration: Option<u64>) -> Result<W, W> {
            Ok(self._writer)
        }
    }
}
