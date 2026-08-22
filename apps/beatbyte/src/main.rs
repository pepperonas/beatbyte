//! BeatByte — an original 8-bit rhythm game.
//!
//! This binary is a thin shell: all game construction lives in
//! [`beatbyte_game`], keeping the app layer trivially small.

use bevy::app::AppExit;

fn main() -> AppExit {
    beatbyte_game::run()
}
