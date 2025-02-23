use openh264::decoder::Decoder;
use openh264::encoder::{Encoder, EncoderConfig};
use openh264::OpenH264API;

fn is_send_sync(_: impl Send + Sync + 'static) {}

#[test]
#[cfg(feature = "source")]
fn decoder_encoder_are_send_sync() {
    is_send_sync(Decoder::new());
    is_send_sync(Encoder::with_api_config(OpenH264API::from_source(), EncoderConfig::default()));
}
