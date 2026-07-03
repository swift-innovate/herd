//! Embedded tray icons.
//!
//! The four state ICOs are compiled into the exe with `include_bytes!` so the
//! binary is self-contained. Each is decoded to RGBA at runtime (the `image`
//! crate's `ico` decoder picks the largest frame, which the OS then scales to
//! the tray size).

use crate::state::IconState;
use anyhow::{Context, Result};

const GREEN: &[u8] = include_bytes!("../assets/herd-tray-green.ico");
const AMBER: &[u8] = include_bytes!("../assets/herd-tray-amber.ico");
const RED: &[u8] = include_bytes!("../assets/herd-tray-red.ico");
const GRAY: &[u8] = include_bytes!("../assets/herd-tray-gray.ico");

fn ico_bytes(state: IconState) -> &'static [u8] {
    match state {
        IconState::Green => GREEN,
        IconState::Amber => AMBER,
        IconState::Red => RED,
        IconState::Gray => GRAY,
    }
}

/// Decode an ICO byte slice into `(rgba, width, height)`.
fn decode_rgba(bytes: &[u8]) -> Result<(Vec<u8>, u32, u32)> {
    let img = image::load_from_memory_with_format(bytes, image::ImageFormat::Ico)
        .context("decode embedded tray ICO")?
        .to_rgba8();
    let (w, h) = img.dimensions();
    Ok((img.into_raw(), w, h))
}

/// Build the `tray_icon::Icon` for a given state.
pub fn tray_icon_for(state: IconState) -> Result<tray_icon::Icon> {
    let (rgba, w, h) = decode_rgba(ico_bytes(state))?;
    tray_icon::Icon::from_rgba(rgba, w, h).context("build tray icon from rgba")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_state_ico_decodes_to_nonempty_rgba() {
        for state in [
            IconState::Green,
            IconState::Amber,
            IconState::Red,
            IconState::Gray,
        ] {
            let (rgba, w, h) = decode_rgba(ico_bytes(state)).expect("decode");
            assert!(w > 0 && h > 0, "{state:?} has zero dimensions");
            assert_eq!(
                rgba.len(),
                (w * h * 4) as usize,
                "{state:?} RGBA length must be 4·w·h"
            );
        }
    }
}
