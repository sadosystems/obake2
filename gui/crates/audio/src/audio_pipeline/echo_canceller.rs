// obake: echo cancellation (libwebrtc) is only needed for voice calls, which are
// removed. A no-op keeps the audio pipeline unchanged and drops the libwebrtc build.
#[derive(Clone, Default)]
pub struct EchoCanceller;

impl EchoCanceller {
    pub fn process_reverse_stream(&mut self, _buf: &mut [i16]) {}
    pub fn process_stream(&mut self, _buf: &mut [i16]) -> anyhow::Result<()> {
        Ok(())
    }
}
