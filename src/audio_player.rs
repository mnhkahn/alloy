//! A single native player: switching versions always releases the previous audio.
use gpui_rhai::{CapabilityHandler, UiValue};
use serde_json::json;

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn alloy_audio_create(bytes: *const u8, length: usize) -> usize;
    fn alloy_audio_destroy(handle: usize);
    fn alloy_audio_play(handle: usize) -> i32;
    fn alloy_audio_pause(handle: usize);
    fn alloy_audio_rewind(handle: usize);
    fn alloy_audio_playing(handle: usize) -> i32;
    fn alloy_audio_position(handle: usize) -> f64;
    fn alloy_audio_duration(handle: usize) -> f64;
}
#[derive(Default)]
pub struct AudioPlayer {
    handle: usize,
    asset: String,
}
impl AudioPlayer {
    fn stop(&mut self) {
        #[cfg(target_os = "macos")]
        if self.handle != 0 {
            unsafe {
                alloy_audio_destroy(self.handle);
            }
        }
        self.handle = 0;
        self.asset.clear();
    }
}
impl Drop for AudioPlayer {
    fn drop(&mut self) {
        self.stop();
    }
}
impl CapabilityHandler for AudioPlayer {
    fn call(&mut self, method: &str, input: UiValue) -> Result<UiValue, String> {
        if !matches!(method, "status" | "stop" | "toggle" | "restart") {
            return Err("Unknown audio method".into());
        }
        if method == "stop" {
            self.stop();
        }
        #[cfg(target_os = "macos")]
        if matches!(method, "toggle" | "restart") {
            let UiValue::String(asset) = input else {
                return Err("Expected audio asset".into());
            };
            if !asset.starts_with("audio-") {
                return Err("Invalid audio asset".into());
            }
            if self.asset != asset {
                self.stop();
                let (_, bytes) = alloy::preview::load_asset(&asset)?;
                self.handle = unsafe { alloy_audio_create(bytes.as_ptr(), bytes.len()) };
                if self.handle == 0 {
                    return Err("Cannot decode this audio file. The file may be damaged or its codec unsupported.".into());
                }
                self.asset = asset;
            }
            unsafe {
                if method == "restart" {
                    alloy_audio_rewind(self.handle);
                }
                if method == "toggle" && alloy_audio_playing(self.handle) != 0 {
                    alloy_audio_pause(self.handle);
                } else if alloy_audio_play(self.handle) == 0 {
                    return Err("Cannot start audio playback.".into());
                }
            }
        }
        #[cfg(not(target_os = "macos"))]
        if matches!(method, "toggle" | "restart") {
            let _ = input;
            return Err("Audio playback is available on macOS.".into());
        }
        let (mut playing, mut position, mut duration) = (false, 0.0, 0.0);
        #[cfg(target_os = "macos")]
        if self.handle != 0 {
            unsafe {
                playing = alloy_audio_playing(self.handle) != 0;
                position = alloy_audio_position(self.handle);
                duration = alloy_audio_duration(self.handle);
            }
        }
        Ok(crate::bridge::to_ui(
            json!({"asset":self.asset,"playing":playing,"position":position,"duration":duration}),
        ))
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    fn silent_wav() -> Vec<u8> {
        let mut bytes = b"RIFF".to_vec();
        bytes.extend(16036u32.to_le_bytes());
        bytes.extend(b"WAVEfmt ");
        bytes.extend(16u32.to_le_bytes());
        bytes.extend(1u16.to_le_bytes()); // PCM
        bytes.extend(1u16.to_le_bytes()); // mono
        bytes.extend(8000u32.to_le_bytes());
        bytes.extend(16000u32.to_le_bytes());
        bytes.extend(2u16.to_le_bytes());
        bytes.extend(16u16.to_le_bytes());
        bytes.extend(b"data");
        bytes.extend(16000u32.to_le_bytes());
        bytes.resize(16044, 0);
        bytes
    }
    #[test]
    fn native_decoder_reads_duration_and_rejects_corrupt_audio() {
        let bytes = silent_wav();
        unsafe {
            let handle = alloy_audio_create(bytes.as_ptr(), bytes.len());
            assert_ne!(handle, 0);
            assert!((alloy_audio_duration(handle) - 1.0).abs() < 0.01);
            assert_eq!(alloy_audio_playing(handle), 0);
            alloy_audio_rewind(handle);
            assert_eq!(alloy_audio_position(handle), 0.0);
            alloy_audio_destroy(handle);
            assert_eq!(alloy_audio_create(b"invalid".as_ptr(), 7), 0);
        }
    }
    #[test]
    fn silent_playback_can_pause_restart_switch_and_stop() {
        let wav = silent_wav();
        let asset = alloy::audio::prepare("test.wav", &wav).asset;
        let mut player = AudioPlayer::default();
        let status = |player: &mut AudioPlayer| {
            let UiValue::Map(fields) = player.call("status", UiValue::Null).unwrap() else {
                panic!("expected audio status");
            };
            fields
        };
        player
            .call("toggle", UiValue::String(asset.clone()))
            .unwrap();
        assert_eq!(status(&mut player)["playing"], UiValue::Bool(true));
        player
            .call("toggle", UiValue::String(asset.clone()))
            .unwrap();
        assert_eq!(status(&mut player)["playing"], UiValue::Bool(false));
        player
            .call("restart", UiValue::String(asset.clone()))
            .unwrap();
        assert_eq!(status(&mut player)["playing"], UiValue::Bool(true));
        let invalid = alloy::audio::prepare("bad.mp3", b"invalid").asset;
        assert!(player.call("toggle", UiValue::String(invalid)).is_err());
        assert_eq!(status(&mut player)["asset"], UiValue::String(String::new()));
        player.call("toggle", UiValue::String(asset)).unwrap();
        player.call("stop", UiValue::Null).unwrap();
        assert_eq!(status(&mut player)["asset"], UiValue::String(String::new()));
        assert_eq!(status(&mut player)["playing"], UiValue::Bool(false));
    }
}
