//! Community charts for a song the player asks for (ADR-0025).
//!
//! Encore (`api.enchor.us`) is the catalogue behind the Bridge app:
//! guitar charts made by people, and the charts of the commercial games.
//! When a player adds a song by name, a finished chart from there is
//! better than any chart BeatByte can generate — so the import asks here
//! first and generates only when nothing fits.
//!
//! The crate is split the way the catalogue lookup is:
//! - [`hit`] reads a search answer (untrusted JSON) into [`Hit`]s;
//! - [`choose`] decides, purely, which of them is the song and in which
//!   order to try them — the rules the bulk tools in `tools/bridge-bulk`
//!   proved on more than eight thousand songs;
//! - [`sng`] unpacks a downloaded `.sng` package (untrusted bytes) into
//!   the folder `beatbyte_library::bridge::import` converts;
//! - [`client`] is the only part that touches the network.
//!
//! Nothing here runs unless the player asked for a song.

pub mod choose;
pub mod client;
pub mod hit;
pub mod sng;

pub use choose::{Wanted, candidates};
pub use client::Client;
pub use hit::{Hit, parse_search};
pub use sng::{Package, decode};
